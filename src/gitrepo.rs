use anyhow::{Context, Result};
use git2::{Oid, Repository};

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
