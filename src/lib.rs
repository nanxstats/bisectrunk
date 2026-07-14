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
mod progress;
mod report;
mod scheduler;
mod state;
mod strategy;
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
        cli::Command::Scan(args) => scan(args),
        cli::Command::Resume { run_dir } => resume(&run_dir),
        cli::Command::Report { run_dir } => report_run(&run_dir),
        cli::Command::Clean { run_dir, cache } => clean(run_dir.as_deref(), cache),
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
    let plan = state::RunPlan {
        config: config.clone(),
        operation: state::Operation::Run { at: sha.clone() },
    };
    state::save_plan(&plan)?;
    let mut ledger = state::RunState::new();
    state::save_state(&config.execution.run_dir, &ledger)?;
    let progress = progress::Progress::new(config.execution.format, 1, 1)?;
    progress.phase(0, &sha, "evaluating");
    let evaluation = if let Some(cached) = state::load_cache(&config, &sha)? {
        cached
    } else {
        let value = evaluate::evaluate(&config, &mirror, &sha, 0)?;
        state::save_cache(&config, &value)?;
        value
    };
    progress.evaluation(&evaluation, false)?;
    ledger.record(evaluation.clone());
    ledger.complete = true;
    state::save_state(&config.execution.run_dir, &ledger)?;
    report::render(&plan, &ledger)?;
    progress.finish();
    progress.line(&format!("logs: {}", evaluation.log_dir.display()));
    mirror.prune_worktrees()?;
    Ok(match evaluation.classification {
        oracle::Classification::Abort => ExitCode::from(4),
        oracle::Classification::Skip => ExitCode::from(2),
        oracle::Classification::Good | oracle::Classification::Bad => ExitCode::SUCCESS,
    })
}

fn scan(args: cli::ScanArgs) -> Result<ExitCode> {
    let file = config::load(&args.shared)?;
    let range = args
        .range
        .clone()
        .or_else(|| file.subject.range.clone())
        .ok_or_else(|| {
            anyhow::anyhow!("--range is required (or set subject.range in bisectrunk.toml)")
        })?;
    let stride = args.stride.or(file.execution.stride).unwrap_or(1);
    if stride == 0 {
        bail!("--stride must be at least 1");
    }
    let sample = args.sample.or(file.execution.sample);
    if sample == Some(0) {
        bail!("--sample must be at least 1");
    }
    let stop_on_first_bad =
        args.stop_on_first_bad || file.execution.stop_on_first_bad.unwrap_or(false);
    let config = config::resolve(&args.shared, file)?;
    fs::create_dir_all(&config.execution.run_dir).with_context(|| {
        format!(
            "create run directory {}",
            config.execution.run_dir.display()
        )
    })?;
    let mirror = mirror::Mirror::acquire(&config.subject.repo, &config.execution.cache_dir)?;
    let (start, end) = gitrepo::parse_range(&range)?;
    let commits = gitrepo::ordered_range(
        mirror.path(),
        start,
        end,
        config.subject.first_parent,
        &config.subject.paths,
    )?;
    let commits = strategy::scan::select_commits(commits, stride, sample);
    if commits.is_empty() {
        bail!("range {range:?} contains no commits after filtering");
    }
    let plan = state::RunPlan {
        config: config.clone(),
        operation: state::Operation::Scan {
            range,
            commits: commits.clone(),
            stride,
            sample,
            stop_on_first_bad,
        },
    };
    state::save_plan(&plan)?;
    let mut ledger = state::RunState::new();
    state::save_state(&config.execution.run_dir, &ledger)?;
    execute_scan(&plan, &mirror, &commits, &mut ledger, stop_on_first_bad)
}

fn execute_scan(
    plan: &state::RunPlan,
    mirror: &mirror::Mirror,
    commits: &[String],
    ledger: &mut state::RunState,
    stop_on_first_bad: bool,
) -> Result<ExitCode> {
    let progress = progress::Progress::new(
        plan.config.execution.format,
        plan.config.execution.jobs.min(commits.len()),
        commits.len(),
    )?;
    let interrupted = scheduler::install_interrupt_handler()?;
    let reason = strategy::scan::execute(
        &plan.config,
        mirror,
        commits,
        ledger,
        &progress,
        &interrupted,
        stop_on_first_bad,
    )?;
    ledger.interrupted = reason == scheduler::StopReason::Interrupted;
    ledger.complete = matches!(
        reason,
        scheduler::StopReason::Complete | scheduler::StopReason::FirstBad
    );
    state::save_state(&plan.config.execution.run_dir, ledger)?;
    report::render(plan, ledger)?;
    mirror.prune_worktrees()?;
    progress.finish();
    let (markdown, json) = report::report_paths(&plan.config.execution.run_dir);
    progress.line(&format!("report: {markdown}"));
    progress.line(&format!("report JSON: {json}"));
    if reason == scheduler::StopReason::Interrupted {
        progress.line(&format!(
            "resume with: bisectrunk resume {}",
            plan.config.execution.run_dir.display()
        ));
    }
    Ok(match reason {
        scheduler::StopReason::HookAbort => ExitCode::from(4),
        scheduler::StopReason::Interrupted => ExitCode::from(2),
        scheduler::StopReason::Complete | scheduler::StopReason::FirstBad => ExitCode::SUCCESS,
    })
}

fn resume(run_dir: &std::path::Path) -> Result<ExitCode> {
    let mut plan = state::load_plan(run_dir)?;
    plan.config.execution.run_dir = run_dir.to_owned();
    let mut ledger = state::load_state(run_dir)?;
    ledger.interrupted = false;
    if ledger.complete {
        report::render(&plan, &ledger)?;
        let progress = progress::Progress::new(plan.config.execution.format, 0, 0)?;
        progress.line("run is already complete; reports were refreshed");
        return Ok(ExitCode::SUCCESS);
    }
    let mirror =
        mirror::Mirror::acquire(&plan.config.subject.repo, &plan.config.execution.cache_dir)?;
    match &plan.operation {
        state::Operation::Scan {
            commits,
            stop_on_first_bad,
            ..
        } => execute_scan(&plan, &mirror, commits, &mut ledger, *stop_on_first_bad),
        state::Operation::Run { .. } => bail!("single-commit runs cannot be resumed"),
        state::Operation::Bisect { .. } => bail!("bisect resume is implemented in milestone M3"),
    }
}

fn report_run(run_dir: &std::path::Path) -> Result<ExitCode> {
    let mut plan = state::load_plan(run_dir)?;
    plan.config.execution.run_dir = run_dir.to_owned();
    let ledger = state::load_state(run_dir)?;
    report::render(&plan, &ledger)?;
    let progress = progress::Progress::new(plan.config.execution.format, 0, 0)?;
    progress.line(&format!("reports refreshed in {}", run_dir.display()));
    Ok(ExitCode::SUCCESS)
}

fn clean(run_dir: Option<&std::path::Path>, clear_cache: bool) -> Result<ExitCode> {
    let cache_dir = if let Some(run_dir) = run_dir {
        state::load_plan(run_dir)
            .ok()
            .map(|plan| plan.config.execution.cache_dir)
    } else {
        None
    };
    if let Some(run_dir) = run_dir
        && run_dir.exists()
    {
        fs::remove_dir_all(run_dir)
            .with_context(|| format!("remove run directory {}", run_dir.display()))?;
    }
    if clear_cache {
        let cache_dir = cache_dir
            .map(Ok)
            .unwrap_or_else(config::default_cache_dir)?;
        if cache_dir.exists() {
            fs::remove_dir_all(&cache_dir)
                .with_context(|| format!("remove cache directory {}", cache_dir.display()))?;
        }
    }
    let progress = progress::Progress::new(cli::OutputFormat::Plain, 0, 0)?;
    progress.line("clean complete");
    Ok(ExitCode::SUCCESS)
}
