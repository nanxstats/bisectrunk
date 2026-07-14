use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use xshell::{Shell, cmd};

#[derive(Debug)]
pub(crate) struct Mirror {
    path: PathBuf,
}

impl Mirror {
    pub(crate) fn acquire(repo: &str, cache_dir: &Path) -> Result<Self> {
        let identity = canonical_identity(repo)?;
        let key = crate::util::stable_hash(&[&identity]);
        let repos = cache_dir.join("repos");
        fs::create_dir_all(&repos)
            .with_context(|| format!("create mirror cache {}", repos.display()))?;
        let path = repos.join(key);
        let shell = Shell::new().context("initialize shell for git mirror operations")?;
        if path.exists() {
            cmd!(shell, "git --git-dir {path} fetch --prune origin")
                .quiet()
                .ignore_stdout()
                .ignore_stderr()
                .run()
                .with_context(|| {
                    format!("fetch subject repository {repo} into {}", path.display())
                })?;
        } else {
            cmd!(shell, "git clone --mirror {repo} {path}")
                .quiet()
                .ignore_stdout()
                .ignore_stderr()
                .run()
                .with_context(|| {
                    format!("clone subject repository {repo} into {}", path.display())
                })?;
        }
        Ok(Self { path })
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn prune_worktrees(&self) -> Result<()> {
        let shell = Shell::new().context("initialize shell for worktree prune")?;
        let path = &self.path;
        cmd!(shell, "git --git-dir {path} worktree prune")
            .quiet()
            .ignore_stdout()
            .ignore_stderr()
            .run()
            .with_context(|| format!("prune worktrees for mirror {}", self.path.display()))
    }
}

fn canonical_identity(repo: &str) -> Result<String> {
    let path = Path::new(repo);
    if path.exists() {
        return path
            .canonicalize()
            .with_context(|| format!("canonicalize subject repository {}", path.display()))
            .map(|value| value.to_string_lossy().into_owned());
    }
    Ok(repo.to_owned())
}
