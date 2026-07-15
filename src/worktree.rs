use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use xshell::{Shell, cmd};

static WORKTREE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[derive(Debug)]
pub(crate) struct Worktree {
    mirror: PathBuf,
    path: PathBuf,
    removed: bool,
}

impl Worktree {
    pub(crate) fn create(mirror: &Path, path: PathBuf, sha: &str) -> Result<Self> {
        let _guard = WORKTREE_LOCK
            .lock()
            .map_err(|_| anyhow::anyhow!("worktree lifecycle lock was poisoned"))?;
        if path.exists() {
            fs::remove_dir_all(&path)
                .with_context(|| format!("remove old worktree directory {}", path.display()))?;
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("create worktree parent {}", parent.display()))?;
        }
        let shell = Shell::new().context("initialize shell for worktree creation")?;
        cmd!(shell, "git --git-dir {mirror} worktree prune")
            .quiet()
            .ignore_stdout()
            .ignore_stderr()
            .run()
            .with_context(|| format!("prune stale worktrees for {}", mirror.display()))?;
        cmd!(
            shell,
            "git --git-dir {mirror} worktree add --detach {path} {sha}"
        )
        .quiet()
        .ignore_stdout()
        .ignore_stderr()
        .run()
        .with_context(|| {
            format!(
                "create detached worktree for commit {sha} at {}",
                path.display()
            )
        })?;
        Ok(Self {
            mirror: mirror.to_owned(),
            path,
            removed: false,
        })
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn remove(&mut self) -> Result<()> {
        if self.removed {
            return Ok(());
        }
        let _guard = WORKTREE_LOCK
            .lock()
            .map_err(|_| anyhow::anyhow!("worktree lifecycle lock was poisoned"))?;
        let shell = Shell::new().context("initialize shell for worktree cleanup")?;
        let mirror = &self.mirror;
        let path = &self.path;
        cmd!(
            shell,
            "git --git-dir {mirror} worktree remove --force {path}"
        )
        .quiet()
        .ignore_stdout()
        .ignore_stderr()
        .run()
        .with_context(|| format!("remove worktree {}", self.path.display()))?;
        cmd!(shell, "git --git-dir {mirror} worktree prune")
            .quiet()
            .ignore_stdout()
            .ignore_stderr()
            .run()
            .with_context(|| format!("prune mirror worktrees {}", self.mirror.display()))?;
        self.removed = true;
        Ok(())
    }

    pub(crate) fn retain(&mut self) {
        self.removed = true;
    }
}

impl Drop for Worktree {
    fn drop(&mut self) {
        if self.removed {
            return;
        }
        if let (Ok(_guard), Ok(shell)) = (WORKTREE_LOCK.lock(), Shell::new()) {
            let mirror = &self.mirror;
            let path = &self.path;
            let _ = cmd!(
                shell,
                "git --git-dir {mirror} worktree remove --force {path}"
            )
            .quiet()
            .ignore_stdout()
            .ignore_stderr()
            .run();
            let _ = cmd!(shell, "git --git-dir {mirror} worktree prune")
                .quiet()
                .ignore_stdout()
                .ignore_stderr()
                .run();
        }
    }
}
