use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::cli::SetupFailure;
use crate::config::ResolvedConfig;
use crate::hooks::{HookContext, parse_timeout};
use crate::mirror::Mirror;
use crate::oracle::{Classification, classify_exit};
use crate::worktree::Worktree;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct Evaluation {
    pub(crate) sha: String,
    pub(crate) classification: Classification,
    pub(crate) exit_code: i32,
    pub(crate) duration_ms: u128,
    pub(crate) setup_exit_code: Option<i32>,
    pub(crate) log_dir: PathBuf,
    pub(crate) out_dir: PathBuf,
}

pub(crate) fn evaluate(
    config: &ResolvedConfig,
    mirror: &Mirror,
    sha: &str,
    job: usize,
) -> Result<Evaluation> {
    if config.oracle.kind != crate::cli::OracleKind::Exit {
        bail!("compare oracle is implemented in milestone M4");
    }
    let started = Instant::now();
    let worktree_path = config
        .execution
        .run_dir
        .join("worktrees")
        .join(format!("job-{job}"));
    let env_dir = config
        .execution
        .cache_dir
        .join("envs")
        .join(crate::util::stable_hash(&[
            &config.subject.repo,
            sha,
            config.hooks.setup.as_deref().unwrap_or(""),
        ]));
    let log_dir = config.execution.run_dir.join("logs").join(sha);
    let out_dir = config.execution.run_dir.join("out").join(sha);
    fs::create_dir_all(&env_dir)
        .with_context(|| format!("create environment directory for commit {sha}"))?;
    fs::create_dir_all(&out_dir)
        .with_context(|| format!("create output directory for commit {sha}"))?;
    let mut worktree = Worktree::create(mirror.path(), worktree_path, sha)?;
    let project = if config.subject.self_bisect {
        worktree.path()
    } else {
        &config.subject.project
    };
    let timeout = parse_timeout(config.execution.timeout.as_deref())?;
    let context = HookContext {
        commit: sha,
        worktree: worktree.path(),
        env_dir: &env_dir,
        out_dir: &out_dir,
        project,
        run_dir: &config.execution.run_dir,
        job,
        extra_env: &config.hooks.env,
        pin_envs: None,
    };
    let mut setup_exit_code = None;
    let (classification, exit_code) = if let Some(setup) = &config.hooks.setup {
        let result = crate::hooks::execute(
            setup,
            config.hooks.shell.as_deref(),
            project,
            &log_dir.join("setup.log"),
            &context,
            timeout,
        )
        .with_context(|| format!("execute setup hook for commit {sha}"))?;
        setup_exit_code = Some(result.code);
        let setup_class = classify_exit(result.code, result.timed_out);
        match setup_class {
            Classification::Good => run_hook(config, &context, &log_dir, timeout)?,
            Classification::Bad if config.execution.setup_failure == SetupFailure::Bad => {
                (Classification::Bad, result.code)
            }
            Classification::Abort => (Classification::Abort, result.code),
            Classification::Bad | Classification::Skip => (Classification::Skip, result.code),
        }
    } else {
        run_hook(config, &context, &log_dir, timeout)?
    };
    worktree
        .remove()
        .with_context(|| format!("clean up worktree after commit {sha}"))?;
    Ok(Evaluation {
        sha: sha.to_owned(),
        classification,
        exit_code,
        duration_ms: started.elapsed().as_millis(),
        setup_exit_code,
        log_dir,
        out_dir,
    })
}

fn run_hook(
    config: &ResolvedConfig,
    context: &HookContext<'_>,
    log_dir: &std::path::Path,
    timeout: Option<Duration>,
) -> Result<(Classification, i32)> {
    let result = crate::hooks::execute(
        &config.hooks.run,
        config.hooks.shell.as_deref(),
        context.project,
        &log_dir.join("run.log"),
        context,
        timeout,
    )
    .with_context(|| format!("execute run hook for commit {}", context.commit))?;
    Ok((classify_exit(result.code, result.timed_out), result.code))
}
