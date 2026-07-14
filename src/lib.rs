//! Parallel, environment-aware, resumable Git bisection execution.

#![warn(missing_docs)]
#![forbid(unsafe_code)]

mod cli;
mod config;
mod evaluate;
mod gitrepo;
mod hooks;
mod mirror;
mod oracle;
mod util;
mod worktree;

use std::fs;
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use clap::Parser;

/// Parses command-line arguments and runs the requested bisectrunk operation.
pub fn run() -> Result<ExitCode> {
    dispatch(cli::Cli::parse())
}

fn dispatch(cli: cli::Cli) -> Result<ExitCode> {
    match cli.command {
        cli::Command::Run(args) => run_one(args),
        cli::Command::Bisect(_) => bail!("bisect is implemented in milestone M3"),
        cli::Command::Scan(_) => bail!("scan is implemented in milestone M2"),
        cli::Command::Resume { .. } => bail!("resume is implemented in milestone M2"),
        cli::Command::Report { .. } => bail!("report is implemented in milestone M2"),
        cli::Command::Clean { .. } => bail!("clean is implemented in milestone M2"),
    }
}

fn run_one(args: cli::RunArgs) -> Result<ExitCode> {
    let file = config::load(&args.shared)?;
    let at = args
        .at
        .clone()
        .or_else(|| file.subject.at.clone())
        .ok_or_else(|| {
            anyhow::anyhow!("--at is required (or set subject.at in bisectrunk.toml)")
        })?;
    let config = config::resolve(&args.shared, file)?;
    fs::create_dir_all(&config.execution.run_dir).with_context(|| {
        format!(
            "create run directory {}",
            config.execution.run_dir.display()
        )
    })?;
    let mirror = mirror::Mirror::acquire(&config.subject.repo, &config.execution.cache_dir)?;
    let sha = gitrepo::resolve_revision(mirror.path(), &at)?;
    debug_assert!(gitrepo::has_commit(mirror.path(), &sha)?);
    let evaluation = evaluate::evaluate(&config, &mirror, &sha, 0)?;
    println!(
        "{} {} (exit {})",
        util::short_sha(&evaluation.sha),
        evaluation.classification,
        evaluation.exit_code
    );
    println!("logs: {}", evaluation.log_dir.display());
    mirror.prune_worktrees()?;
    Ok(match evaluation.classification {
        oracle::Classification::Abort => ExitCode::from(4),
        oracle::Classification::Skip => ExitCode::from(2),
        oracle::Classification::Good | oracle::Classification::Bad => ExitCode::SUCCESS,
    })
}
