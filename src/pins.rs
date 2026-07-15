use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};

use crate::config::{PinConfig, ResolvedConfig};
use crate::hooks::HookContext;
use crate::mirror::Mirror;
use crate::oracle::{Classification, classify_exit};
use crate::worktree::Worktree;

pub(crate) fn prepare(config: &ResolvedConfig) -> Result<()> {
    for (index, pin) in config.pins.iter().enumerate() {
        prepare_one(config, pin, index)
            .with_context(|| format!("prepare pinned dependency {} at {}", pin.repo, pin.rev))?;
    }
    Ok(())
}

pub(crate) fn environment(config: &ResolvedConfig) -> Result<Option<OsString>> {
    if config.pins.is_empty() {
        return Ok(None);
    }
    std::env::join_paths(config.pins.iter().map(|pin| environment_dir(config, pin)))
        .context("join pinned dependency environment paths")
        .map(Some)
}

fn prepare_one(config: &ResolvedConfig, pin: &PinConfig, index: usize) -> Result<()> {
    let env_dir = environment_dir(config, pin);
    let marker = env_dir.join(".bisectrunk-pin-complete");
    if marker.is_file() {
        return Ok(());
    }
    if env_dir.exists() {
        fs::remove_dir_all(&env_dir)
            .with_context(|| format!("remove incomplete pin environment {}", env_dir.display()))?;
    }
    fs::create_dir_all(&env_dir)
        .with_context(|| format!("create pin environment {}", env_dir.display()))?;
    let mirror = Mirror::acquire(&pin.repo, &config.execution.cache_dir)?;
    let sha = crate::gitrepo::resolve_revision(mirror.path(), &pin.rev)?;
    let worktree_path = config
        .execution
        .run_dir
        .join("pins")
        .join(format!("{index}-{}", crate::util::short_sha(&sha)));
    let mut worktree = Worktree::create(mirror.path(), worktree_path, &sha)?;
    let log_dir = config
        .execution
        .run_dir
        .join("logs")
        .join("pins")
        .join(format!("{index}-{}", crate::util::short_sha(&sha)));
    let out_dir = config
        .execution
        .run_dir
        .join("out")
        .join("pins")
        .join(index.to_string());
    fs::create_dir_all(&out_dir)
        .with_context(|| format!("create pin output directory {}", out_dir.display()))?;
    let mut extra_env: BTreeMap<String, String> = config.hooks.env.clone();
    extra_env.insert(
        "BISECTRUNK_PIN_ENV".into(),
        env_dir.to_string_lossy().into_owned(),
    );
    extra_env.insert(
        "BISECTRUNK_PIN_WORKTREE".into(),
        worktree.path().to_string_lossy().into_owned(),
    );
    let context = HookContext {
        commit: &sha,
        worktree: worktree.path(),
        env_dir: &env_dir,
        out_dir: &out_dir,
        project: &config.subject.project,
        run_dir: &config.execution.run_dir,
        job: index,
        extra_env: &extra_env,
        pin_envs: None,
        baseline: None,
        candidate: None,
    };
    let result = crate::hooks::execute(
        &pin.setup,
        config.hooks.shell.as_deref(),
        &config.subject.project,
        &log_dir.join("setup.log"),
        &context,
        crate::hooks::parse_timeout(config.execution.timeout.as_deref())?,
    )?;
    let classification = classify_exit(result.code, result.timed_out);
    if classification != Classification::Good {
        anyhow::bail!(
            "pin setup for {} at {} returned {} ({classification})",
            pin.repo,
            sha,
            result.code
        );
    }
    fs::write(&marker, format!("{sha}\n"))
        .with_context(|| format!("mark pin environment complete {}", marker.display()))?;
    worktree.remove()?;
    mirror.prune_worktrees()?;
    Ok(())
}

fn environment_dir(config: &ResolvedConfig, pin: &PinConfig) -> PathBuf {
    config
        .execution
        .cache_dir
        .join("pins")
        .join(crate::util::stable_hash(&[&pin.repo, &pin.rev, &pin.setup]))
}
