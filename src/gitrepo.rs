use anyhow::{Context, Result};
use git2::{DiffOptions, Oid, Repository, Sort};

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub(crate) struct CommitMetadata {
    pub(crate) sha: String,
    pub(crate) author: String,
    pub(crate) date: String,
    pub(crate) subject: String,
}

pub(crate) fn resolve_revision(mirror: &std::path::Path, revision: &str) -> Result<String> {
    let repository = Repository::open_bare(mirror)
        .with_context(|| format!("open local mirror {}", mirror.display()))?;
    let object = repository
        .revparse_single(revision)
        .with_context(|| format!("resolve revision {revision:?} in {}", mirror.display()))?;
    let commit = object
        .peel_to_commit()
        .with_context(|| format!("peel revision {revision:?} to a commit"))?;
    Ok(commit.id().to_string())
}

pub(crate) fn has_commit(mirror: &std::path::Path, sha: &str) -> Result<bool> {
    let repository = Repository::open_bare(mirror)
        .with_context(|| format!("open local mirror {}", mirror.display()))?;
    let oid = Oid::from_str(sha).with_context(|| format!("parse commit id {sha}"))?;
    Ok(repository.find_commit(oid).is_ok())
}

pub(crate) fn ordered_range(
    mirror: &std::path::Path,
    start: &str,
    end: &str,
    first_parent: bool,
    paths: &[std::path::PathBuf],
) -> Result<Vec<String>> {
    let repository = Repository::open_bare(mirror)
        .with_context(|| format!("open local mirror {}", mirror.display()))?;
    let start = resolve_oid(&repository, start)?;
    let end = resolve_oid(&repository, end)?;
    let oids = if first_parent {
        first_parent_range(&repository, start, end)?
    } else {
        let mut walk = repository.revwalk().context("create commit range walk")?;
        walk.push(end)
            .with_context(|| format!("push range end {end}"))?;
        walk.hide(start)
            .with_context(|| format!("hide range start {start}"))?;
        walk.set_sorting(Sort::TOPOLOGICAL | Sort::REVERSE)
            .context("order commit range")?;
        walk.collect::<std::result::Result<Vec<_>, _>>()
            .context("walk ordered commit range")?
    };
    let mut commits = Vec::with_capacity(oids.len());
    for oid in oids {
        if paths.is_empty() || commit_touches_paths(&repository, oid, paths)? {
            commits.push(oid.to_string());
        }
    }
    Ok(commits)
}

pub(crate) fn parse_range(range: &str) -> Result<(&str, &str)> {
    let (start, end) = range
        .split_once("..")
        .ok_or_else(|| anyhow::anyhow!("invalid range {range:?}; expected A..B"))?;
    if start.is_empty() || end.is_empty() || end.contains("..") {
        anyhow::bail!("invalid range {range:?}; expected A..B");
    }
    Ok((start, end))
}

pub(crate) fn metadata(mirror: &std::path::Path, sha: &str) -> Result<CommitMetadata> {
    let repository = Repository::open_bare(mirror)
        .with_context(|| format!("open local mirror {}", mirror.display()))?;
    let oid = Oid::from_str(sha).with_context(|| format!("parse commit id {sha}"))?;
    let commit = repository
        .find_commit(oid)
        .with_context(|| format!("load commit {sha}"))?;
    let author = commit.author();
    let date = jiff::Timestamp::new(commit.time().seconds(), 0)
        .with_context(|| format!("convert timestamp for commit {sha}"))?
        .to_string();
    Ok(CommitMetadata {
        sha: sha.to_owned(),
        author: author.name().unwrap_or("unknown").to_owned(),
        date,
        subject: commit.summary().unwrap_or("(no subject)").to_owned(),
    })
}

pub(crate) fn ensure_ancestor(
    mirror: &std::path::Path,
    ancestor: &str,
    descendant: &str,
) -> Result<()> {
    let repository = Repository::open_bare(mirror)
        .with_context(|| format!("open local mirror {}", mirror.display()))?;
    let ancestor = resolve_oid(&repository, ancestor)?;
    let descendant = resolve_oid(&repository, descendant)?;
    let merge_base = repository
        .merge_base(ancestor, descendant)
        .with_context(|| format!("find merge base of {ancestor} and {descendant}"))?;
    if merge_base != ancestor {
        anyhow::bail!("good revision {ancestor} is not an ancestor of bad revision {descendant}");
    }
    Ok(())
}

fn resolve_oid(repository: &Repository, revision: &str) -> Result<Oid> {
    repository
        .revparse_single(revision)
        .with_context(|| format!("resolve revision {revision:?}"))?
        .peel_to_commit()
        .with_context(|| format!("peel revision {revision:?} to commit"))
        .map(|commit| commit.id())
}

fn first_parent_range(repository: &Repository, start: Oid, end: Oid) -> Result<Vec<Oid>> {
    let mut result = Vec::new();
    let mut commit = repository
        .find_commit(end)
        .with_context(|| format!("load range end {end}"))?;
    while commit.id() != start {
        result.push(commit.id());
        if commit.parent_count() == 0 {
            anyhow::bail!("{start} is not on the first-parent chain ending at {end}");
        }
        commit = commit
            .parent(0)
            .with_context(|| format!("follow first parent from {}", commit.id()))?;
    }
    result.reverse();
    Ok(result)
}

fn commit_touches_paths(
    repository: &Repository,
    oid: Oid,
    paths: &[std::path::PathBuf],
) -> Result<bool> {
    let commit = repository
        .find_commit(oid)
        .with_context(|| format!("load commit {oid} for path filtering"))?;
    let tree = commit.tree().context("load commit tree")?;
    let parent_tree = if commit.parent_count() == 0 {
        None
    } else {
        Some(
            commit
                .parent(0)
                .context("load commit parent")?
                .tree()
                .context("load parent tree")?,
        )
    };
    let mut options = DiffOptions::new();
    for path in paths {
        options.pathspec(path);
    }
    let diff = repository
        .diff_tree_to_tree(parent_tree.as_ref(), Some(&tree), Some(&mut options))
        .with_context(|| format!("diff commit {oid} for path filtering"))?;
    Ok(diff.deltas().len() > 0)
}
