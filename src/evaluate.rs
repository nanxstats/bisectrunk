use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::cli::{KeepPolicy, SetupFailure};
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
    pub(crate) completed_at: String,
    pub(crate) setup_exit_code: Option<i32>,
    pub(crate) log_dir: PathBuf,
    pub(crate) out_dir: PathBuf,
    #[serde(default)]
    pub(crate) diff: Option<String>,
}

pub(crate) fn evaluate(
    config: &ResolvedConfig,
    mirror: &Mirror,
    sha: &str,
    job: usize,
) -> Result<Evaluation> {
    let mut evaluation = evaluate_once(config, mirror, sha, job)?;
    for _ in 0..config.execution.retries {
        if !matches!(
            evaluation.classification,
            Classification::Bad | Classification::Skip
        ) {
            break;
        }
        evaluation = evaluate_once(config, mirror, sha, job)?;
    }
    Ok(evaluation)
}

fn evaluate_once(
    config: &ResolvedConfig,
    mirror: &Mirror,
    sha: &str,
    job: usize,
) -> Result<Evaluation> {
    let started = Instant::now();
    let worktree_path = config
        .execution
        .run_dir
        .join("worktrees")
        .join(format!("job-{job}"))
        .join(sha);
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
    let pin_envs = crate::pins::environment(config)?;
    let context = HookContext {
        commit: sha,
        worktree: worktree.path(),
        env_dir: &env_dir,
        out_dir: &out_dir,
        project,
        run_dir: &config.execution.run_dir,
        job,
        extra_env: &config.hooks.env,
        pin_envs: pin_envs.as_deref(),
        baseline: None,
        candidate: None,
    };
    let mut setup_exit_code = None;
    let setup_marker = env_dir.join(".bisectrunk-setup-complete");
    let (classification, exit_code, diff) = if let Some(setup) = &config.hooks.setup {
        if setup_marker.is_file() {
            run_hook(config, &context, &log_dir, started, timeout)?
        } else {
            let result = crate::hooks::execute(
                setup,
                config.hooks.shell.as_deref(),
                project,
                &log_dir.join("setup.log"),
                &context,
                remaining_timeout(started, timeout),
            )
            .with_context(|| format!("execute setup hook for commit {sha}"))?;
            setup_exit_code = Some(result.code);
            let setup_class = classify_exit(result.code, result.timed_out);
            match setup_class {
                Classification::Good => {
                    fs::write(&setup_marker, b"complete\n").with_context(|| {
                        format!("mark environment setup complete for commit {sha}")
                    })?;
                    run_hook(config, &context, &log_dir, started, timeout)?
                }
                Classification::Bad if config.execution.setup_failure == SetupFailure::Bad => {
                    (Classification::Bad, result.code, None)
                }
                Classification::Abort => (Classification::Abort, result.code, None),
                Classification::Bad | Classification::Skip => {
                    (Classification::Skip, result.code, None)
                }
            }
        }
    } else {
        run_hook(config, &context, &log_dir, started, timeout)?
    };
    let retain = match config.execution.keep {
        KeepPolicy::All => true,
        KeepPolicy::Failed => classification != Classification::Good,
        KeepPolicy::None => false,
    };
    if retain {
        worktree.retain();
    } else {
        worktree
            .remove()
            .with_context(|| format!("clean up worktree after commit {sha}"))?;
    }
    Ok(Evaluation {
        sha: sha.to_owned(),
        classification,
        exit_code,
        duration_ms: started.elapsed().as_millis(),
        completed_at: jiff::Timestamp::now().to_string(),
        setup_exit_code,
        log_dir,
        out_dir,
        diff,
    })
}

fn run_hook(
    config: &ResolvedConfig,
    context: &HookContext<'_>,
    log_dir: &std::path::Path,
    started: Instant,
    timeout: Option<Duration>,
) -> Result<(Classification, i32, Option<String>)> {
    let result = crate::hooks::execute(
        &config.hooks.run,
        config.hooks.shell.as_deref(),
        context.project,
        &log_dir.join("run.log"),
        context,
        remaining_timeout(started, timeout),
    )
    .with_context(|| format!("execute run hook for commit {}", context.commit))?;
    let classification = classify_exit(result.code, result.timed_out);
    if config.oracle.kind == crate::cli::OracleKind::Exit || classification != Classification::Good
    {
        return Ok((classification, result.code, None));
    }
    let compared = crate::oracle::compare_artifact(
        config,
        context,
        log_dir,
        remaining_timeout(started, timeout),
    )?;
    Ok((compared.classification, compared.exit_code, compared.diff))
}

fn remaining_timeout(started: Instant, timeout: Option<Duration>) -> Option<Duration> {
    timeout.map(|limit| limit.saturating_sub(started.elapsed()))
}
