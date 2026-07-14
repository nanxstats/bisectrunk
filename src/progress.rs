use std::io::IsTerminal;
use std::sync::Arc;

use anyhow::{Context, Result};
use indicatif::{MultiProgress, ProgressBar, ProgressDrawTarget, ProgressStyle};
use serde::Serialize;

use crate::cli::OutputFormat;
use crate::evaluate::Evaluation;

#[derive(Clone)]
pub(crate) struct Progress {
    inner: Arc<Inner>,
}

struct Inner {
    format: OutputFormat,
    multi: MultiProgress,
    overall: ProgressBar,
    workers: Vec<ProgressBar>,
    rounds: bool,
}

impl Progress {
    pub(crate) fn new(format: OutputFormat, workers: usize, total: usize) -> Result<Self> {
        Self::build(format, workers, total, false)
    }

    pub(crate) fn new_bisect(
        format: OutputFormat,
        workers: usize,
        total_rounds: usize,
    ) -> Result<Self> {
        Self::build(format, workers, total_rounds, true)
    }

    fn build(format: OutputFormat, workers: usize, total: usize, rounds: bool) -> Result<Self> {
        let resolved = match format {
            OutputFormat::Auto if std::io::stderr().is_terminal() => OutputFormat::Auto,
            OutputFormat::Auto => OutputFormat::Plain,
            other => other,
        };
        let multi = MultiProgress::with_draw_target(match resolved {
            OutputFormat::Auto => ProgressDrawTarget::stderr(),
            OutputFormat::Json | OutputFormat::Plain => ProgressDrawTarget::hidden(),
        });
        let overall = multi.add(ProgressBar::new(total as u64));
        overall.set_style(
            ProgressStyle::with_template("{msg} [{bar:32.cyan/blue}] {pos}/{len}")
                .context("build overall progress style")?,
        );
        overall.set_message(if rounds { "rounds" } else { "evaluations" });
        let spinner_style = ProgressStyle::with_template("{spinner:.cyan} worker {prefix}: {msg}")
            .context("build worker progress style")?
            .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]);
        let mut bars = Vec::with_capacity(workers);
        for worker in 0..workers {
            let spinner = multi.add(ProgressBar::new_spinner());
            spinner.set_style(spinner_style.clone());
            spinner.set_prefix(worker.to_string());
            spinner.set_message("idle");
            spinner.enable_steady_tick(std::time::Duration::from_millis(100));
            bars.push(spinner);
        }
        Ok(Self {
            inner: Arc::new(Inner {
                format: resolved,
                multi,
                overall,
                workers: bars,
                rounds,
            }),
        })
    }

    pub(crate) fn phase(&self, worker: usize, sha: &str, phase: &str) {
        if let Some(spinner) = self.inner.workers.get(worker) {
            spinner.set_message(format!("{} {phase}", crate::util::short_sha(sha)));
        }
    }

    pub(crate) fn evaluation(&self, evaluation: &Evaluation, cached: bool) -> Result<()> {
        if !self.inner.rounds {
            self.inner.overall.inc(1);
        }
        match self.inner.format {
            OutputFormat::Json => self.json(&EvaluationEvent {
                event: "evaluation",
                sha: &evaluation.sha,
                classification: evaluation.classification.to_string(),
                exit_code: evaluation.exit_code,
                duration_ms: evaluation.duration_ms,
                cached,
            }),
            OutputFormat::Plain => {
                let suffix = if cached { " cached" } else { "" };
                self.line(&format!(
                    "{} {} (exit {}, {}{})",
                    crate::util::short_sha(&evaluation.sha),
                    evaluation.classification,
                    evaluation.exit_code,
                    humantime::format_duration(std::time::Duration::from_millis(
                        evaluation.duration_ms.min(u64::MAX as u128) as u64
                    )),
                    suffix
                ));
                Ok(())
            }
            OutputFormat::Auto => Ok(()),
        }
    }

    pub(crate) fn line(&self, message: &str) {
        match self.inner.format {
            OutputFormat::Auto => {
                let _ = self.inner.multi.println(message);
            }
            OutputFormat::Json => println!(
                "{}",
                serde_json::json!({ "event": "status", "message": message })
            ),
            OutputFormat::Plain => println!("{message}"),
        }
    }

    pub(crate) fn round_start(&self, round: usize, probes: &[String]) -> Result<()> {
        if self.inner.rounds {
            self.inner.overall.inc(1);
        }
        if self.inner.format == OutputFormat::Json {
            self.json(&serde_json::json!({
                "event": "round_start",
                "round": round,
                "probes": probes,
            }))?;
        }
        Ok(())
    }

    pub(crate) fn conclusion(&self, first_bad: &str, last_good: &str) -> Result<()> {
        if self.inner.format == OutputFormat::Json {
            self.json(&serde_json::json!({
                "event": "conclusion",
                "first_bad": first_bad,
                "last_good": last_good,
            }))?;
        }
        Ok(())
    }

    pub(crate) fn finish(&self) {
        self.inner.overall.finish_and_clear();
        for worker in &self.inner.workers {
            worker.finish_and_clear();
        }
    }

    pub(crate) fn json<T: Serialize>(&self, value: &T) -> Result<()> {
        println!(
            "{}",
            serde_json::to_string(value).context("serialize JSON progress event")?
        );
        Ok(())
    }
}

#[derive(Serialize)]
struct EvaluationEvent<'a> {
    event: &'static str,
    sha: &'a str,
    classification: String,
    exit_code: i32,
    duration_ms: u128,
    cached: bool,
}
