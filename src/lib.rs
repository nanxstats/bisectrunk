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
mod pins;
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
        cli::Command::Bisect(args) => bisect(args),
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
    pins::prepare(&config)?;
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
    pins::prepare(&config)?;
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

fn bisect(args: cli::BisectArgs) -> Result<ExitCode> {
    let file = config::load(&args.shared)?;
    let good = args
        .good
        .clone()
        .or_else(|| file.subject.good.clone())
        .ok_or_else(|| {
            anyhow::anyhow!("--good is required (or set subject.good in bisectrunk.toml)")
        })?;
    let bad = args
        .bad
        .clone()
        .or_else(|| file.subject.bad.clone())
        .unwrap_or_else(|| "HEAD".into());
    let terms = parse_terms(
        args.terms
            .clone()
            .or_else(|| file.execution.terms.clone())
            .as_deref()
            .unwrap_or("good,bad"),
    )?;
    let verify_endpoints = if args.no_verify_endpoints {
        false
    } else {
        file.execution.verify_endpoints.unwrap_or(true)
    };
    let on_inconsistent = args
        .on_inconsistent
        .or(file.execution.on_inconsistent)
        .unwrap_or_default();
    let config = config::resolve(&args.shared, file)?;
    fs::create_dir_all(&config.execution.run_dir).with_context(|| {
        format!(
            "create run directory {}",
            config.execution.run_dir.display()
        )
    })?;
    let mirror = mirror::Mirror::acquire(&config.subject.repo, &config.execution.cache_dir)?;
    pins::prepare(&config)?;
    gitrepo::ensure_ancestor(mirror.path(), &good, &bad)?;
    let good_sha = gitrepo::resolve_revision(mirror.path(), &good)?;
    let bad_sha = gitrepo::resolve_revision(mirror.path(), &bad)?;
    if good_sha == bad_sha {
        bail!("--good and --bad resolve to the same commit {good_sha}");
    }
    let mut commits = gitrepo::ordered_range(
        mirror.path(),
        &good,
        &bad,
        config.subject.first_parent,
        &config.subject.paths,
    )?;
    if commits.last() != Some(&bad_sha) {
        commits.push(bad_sha.clone());
    }
    commits.insert(0, good_sha.clone());
    let plan = state::RunPlan {
        config: config.clone(),
        operation: state::Operation::Bisect {
            good: good_sha,
            bad: bad_sha,
            commits: commits.clone(),
            verify_endpoints,
            on_inconsistent,
            terms: terms.clone(),
        },
    };
    state::save_plan(&plan)?;
    let mut ledger = state::RunState::new();
    state::save_state(&config.execution.run_dir, &ledger)?;
    execute_bisect(
        &plan,
        &mirror,
        &commits,
        &mut ledger,
        verify_endpoints,
        on_inconsistent,
        &terms,
    )
}

#[allow(clippy::too_many_arguments)]
fn execute_bisect(
    plan: &state::RunPlan,
    mirror: &mirror::Mirror,
    commits: &[String],
    ledger: &mut state::RunState,
    verify_endpoints: bool,
    on_inconsistent: cli::InconsistentPolicy,
    terms: &[String; 2],
) -> Result<ExitCode> {
    let progress = progress::Progress::new_bisect(
        plan.config.execution.format,
        plan.config.execution.jobs.min(commits.len()),
        estimated_rounds(commits.len().saturating_sub(1), plan.config.execution.jobs),
    )?;
    let interrupted = scheduler::install_interrupt_handler()?;
    let outcome = strategy::bisect::execute(
        &plan.config,
        mirror,
        commits,
        ledger,
        &progress,
        &interrupted,
        verify_endpoints,
        on_inconsistent,
        terms,
    )?;
    let exit = match outcome {
        strategy::bisect::BisectOutcome::Conclusive(conclusion) => {
            let mut conclusion = *conclusion;
            conclusion.first_bad_metadata =
                Some(gitrepo::metadata(mirror.path(), &conclusion.first_bad)?);
            conclusion.last_good_metadata =
                Some(gitrepo::metadata(mirror.path(), &conclusion.last_good)?);
            progress.line(&format!(
                "first {} commit: {}",
                terms[1], conclusion.first_bad
            ));
            progress.line(&format!(
                "last {} commit: {}",
                terms[0], conclusion.last_good
            ));
            progress.conclusion(&conclusion.first_bad, &conclusion.last_good)?;
            ledger.conclusion = Some(conclusion);
            ledger.complete = true;
            ExitCode::SUCCESS
        }
        strategy::bisect::BisectOutcome::Inconclusive {
            candidates,
            message,
        } => {
            progress.line(&format!("inconclusive: {message}"));
            progress.line(&format!("candidate set: {}", candidates.join(" ")));
            ledger.conclusion = Some(state::Conclusion {
                first_bad: String::new(),
                last_good: String::new(),
                candidates,
                first_bad_metadata: None,
                last_good_metadata: None,
            });
            ExitCode::from(2)
        }
        strategy::bisect::BisectOutcome::EndpointFailed(message) => {
            progress.line(&format!("endpoint verification failed: {message}"));
            ExitCode::from(3)
        }
        strategy::bisect::BisectOutcome::HookAbort => ExitCode::from(4),
        strategy::bisect::BisectOutcome::Interrupted => {
            ledger.interrupted = true;
            progress.line(&format!(
                "resume with: bisectrunk resume {}",
                plan.config.execution.run_dir.display()
            ));
            ExitCode::from(2)
        }
    };
    state::save_state(&plan.config.execution.run_dir, ledger)?;
    report::render(plan, ledger)?;
    mirror.prune_worktrees()?;
    progress.finish();
    let (markdown, json) = report::report_paths(&plan.config.execution.run_dir);
    progress.line(&format!("report: {markdown}"));
    progress.line(&format!("report JSON: {json}"));
    Ok(exit)
}

fn estimated_rounds(commits: usize, jobs: usize) -> usize {
    if commits <= 1 {
        return 0;
    }
    let base = jobs.max(1) + 1;
    let mut covered = 1usize;
    let mut rounds = 0usize;
    while covered < commits {
        covered = covered.saturating_mul(base);
        rounds += 1;
    }
    rounds
}

fn parse_terms(value: &str) -> Result<[String; 2]> {
    let values = value.split(',').collect::<Vec<_>>();
    if values.len() != 2 || values.iter().any(|term| term.is_empty()) {
        bail!("invalid --terms {value:?}; expected good,bad");
    }
    Ok([values[0].to_owned(), values[1].to_owned()])
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
    pins::prepare(&plan.config)?;
    match &plan.operation {
        state::Operation::Scan {
            commits,
            stop_on_first_bad,
            ..
        } => execute_scan(&plan, &mirror, commits, &mut ledger, *stop_on_first_bad),
        state::Operation::Run { .. } => bail!("single-commit runs cannot be resumed"),
        state::Operation::Bisect {
            commits,
            verify_endpoints,
            on_inconsistent,
            terms,
            ..
        } => execute_bisect(
            &plan,
            &mirror,
            commits,
            &mut ledger,
            *verify_endpoints,
            *on_inconsistent,
            terms,
        ),
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
    let plan = run_dir.and_then(|path| state::load_plan(path).ok());
    let cache_dir = plan
        .as_ref()
        .map(|plan| plan.config.execution.cache_dir.clone());
    if let Some(run_dir) = run_dir
        && run_dir.exists()
    {
        fs::remove_dir_all(run_dir)
            .with_context(|| format!("remove run directory {}", run_dir.display()))?;
    }
    if let Some(plan) = &plan {
        let mirror =
            mirror::Mirror::acquire(&plan.config.subject.repo, &plan.config.execution.cache_dir)?;
        mirror.prune_worktrees()?;
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
