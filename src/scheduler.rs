use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context, Result};
use crossbeam_channel::{Receiver, Sender, unbounded};

use crate::config::ResolvedConfig;
use crate::evaluate::Evaluation;
use crate::mirror::Mirror;
use crate::oracle::Classification;
use crate::progress::Progress;
use crate::state::RunState;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum StopReason {
    Complete,
    Interrupted,
    HookAbort,
    FirstBad,
}

pub(crate) fn install_interrupt_handler() -> Result<Arc<AtomicBool>> {
    let interrupted = Arc::new(AtomicBool::new(false));
    let handler_flag = Arc::clone(&interrupted);
    ctrlc::set_handler(move || handler_flag.store(true, Ordering::SeqCst))
        .context("install Ctrl-C handler")?;
    Ok(interrupted)
}

pub(crate) fn evaluate_commits(
    config: &ResolvedConfig,
    mirror: &Mirror,
    commits: &[String],
    state: &mut RunState,
    progress: &Progress,
    interrupted: &AtomicBool,
    stop_on_first_bad: bool,
) -> Result<StopReason> {
    let mut pending = VecDeque::new();
    for sha in commits {
        if state.evaluations.contains_key(sha) {
            continue;
        }
        if let Some(cached) = crate::state::load_cache(config, sha)? {
            progress.evaluation(&cached, true)?;
            state.record(cached);
            crate::state::save_state(&config.execution.run_dir, state)?;
        } else {
            pending.push_back(sha.clone());
        }
    }
    if pending.is_empty() {
        return Ok(StopReason::Complete);
    }
    let worker_count = config.execution.jobs.min(pending.len()).max(1);
    let (job_sender, job_receiver) = unbounded::<String>();
    let (result_sender, result_receiver) = unbounded::<WorkerResult>();
    std::thread::scope(|scope| -> Result<StopReason> {
        spawn_workers(
            scope,
            worker_count,
            config,
            mirror,
            &job_receiver,
            &result_sender,
            progress,
        );
        drop(result_sender);
        let mut in_flight = 0usize;
        for _ in 0..worker_count {
            if let Some(sha) = pending.pop_front() {
                job_sender
                    .send(sha)
                    .context("dispatch initial evaluation")?;
                in_flight += 1;
            }
        }
        let mut reason = StopReason::Complete;
        let mut first_error = None;
        while in_flight > 0 {
            let result = result_receiver
                .recv()
                .context("receive worker evaluation result")?;
            in_flight -= 1;
            match result.result {
                Ok(evaluation) => {
                    progress.phase(result.worker, &evaluation.sha, "done");
                    progress.evaluation(&evaluation, false)?;
                    let classification = evaluation.classification;
                    crate::state::save_cache(config, &evaluation)?;
                    state.record(evaluation);
                    crate::state::save_state(&config.execution.run_dir, state)?;
                    if classification == Classification::Abort {
                        reason = StopReason::HookAbort;
                    } else if stop_on_first_bad && classification == Classification::Bad {
                        reason = StopReason::FirstBad;
                    }
                }
                Err(error) => {
                    first_error = Some(error);
                }
            }
            if interrupted.load(Ordering::SeqCst) {
                reason = StopReason::Interrupted;
            }
            let may_dispatch = first_error.is_none() && reason == StopReason::Complete;
            if may_dispatch && let Some(sha) = pending.pop_front() {
                job_sender.send(sha).context("dispatch evaluation")?;
                in_flight += 1;
            }
        }
        drop(job_sender);
        if let Some(error) = first_error {
            return Err(error);
        }
        Ok(reason)
    })
}

fn spawn_workers<'scope>(
    scope: &'scope std::thread::Scope<'scope, '_>,
    worker_count: usize,
    config: &'scope ResolvedConfig,
    mirror: &'scope Mirror,
    jobs: &Receiver<String>,
    results: &Sender<WorkerResult>,
    progress: &Progress,
) {
    for worker in 0..worker_count {
        let jobs = jobs.clone();
        let results = results.clone();
        let progress = progress.clone();
        scope.spawn(move || {
            while let Ok(sha) = jobs.recv() {
                progress.phase(worker, &sha, "evaluating");
                let result = crate::evaluate::evaluate(config, mirror, &sha, worker)
                    .with_context(|| format!("evaluate commit {sha} on worker {worker}"));
                if results.send(WorkerResult { worker, result }).is_err() {
                    break;
                }
            }
        });
    }
}

struct WorkerResult {
    worker: usize,
    result: Result<Evaluation>,
}
