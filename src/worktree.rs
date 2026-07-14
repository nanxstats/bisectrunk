use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use xshell::{Shell, cmd};

#[derive(Debug)]
pub(crate) struct Worktree {
    mirror: PathBuf,
    path: PathBuf,
    removed: bool,
}

impl Worktree {
    pub(crate) fn create(mirror: &Path, path: PathBuf, sha: &str) -> Result<Self> {
        if path.exists() {
            fs::remove_dir_all(&path)
                .with_context(|| format!("remove old worktree directory {}", path.display()))?;
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("create worktree parent {}", parent.display()))?;
        }
        let shell = Shell::new().context("initialize shell for worktree creation")?;
        cmd!(
            shell,
            "git --git-dir {mirror} worktree add --detach {path} {sha}"
        )
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
        let shell = Shell::new().context("initialize shell for worktree cleanup")?;
        let mirror = &self.mirror;
        let path = &self.path;
        cmd!(
            shell,
            "git --git-dir {mirror} worktree remove --force {path}"
        )
        .run()
        .with_context(|| format!("remove worktree {}", self.path.display()))?;
        cmd!(shell, "git --git-dir {mirror} worktree prune")
            .run()
            .with_context(|| format!("prune mirror worktrees {}", self.mirror.display()))?;
        self.removed = true;
        Ok(())
    }
}

impl Drop for Worktree {
    fn drop(&mut self) {
        if self.removed {
            return;
        }
        if let Ok(shell) = Shell::new() {
            let mirror = &self.mirror;
            let path = &self.path;
            let _ = cmd!(
                shell,
                "git --git-dir {mirror} worktree remove --force {path}"
            )
            .quiet()
            .run();
            let _ = cmd!(shell, "git --git-dir {mirror} worktree prune")
                .quiet()
                .run();
        }
    }
}
