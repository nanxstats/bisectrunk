use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};
use serde::{Deserialize, Serialize};

#[derive(Debug, Parser)]
#[command(name = "bisectrunk", version, about)]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) command: Command,
}

#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    /// Find the first bad commit using parallel k-section search.
    Bisect(BisectArgs),
    /// Evaluate a range of commits in parallel.
    Scan(ScanArgs),
    /// Evaluate one commit while developing hooks.
    Run(RunArgs),
    /// Continue an interrupted run.
    Resume {
        /// Existing bisectrunk run directory.
        run_dir: PathBuf,
    },
    /// Re-render reports from a run directory.
    Report {
        /// Existing bisectrunk run directory.
        run_dir: PathBuf,
    },
    /// Remove run resources and optionally the shared cache.
    Clean {
        /// Run directory to remove.
        run_dir: Option<PathBuf>,
        /// Also clear cached mirrors and environments.
        #[arg(long)]
        cache: bool,
    },
}

#[derive(Clone, Debug, Args, Default)]
pub(crate) struct SharedArgs {
    /// Subject repository URL or local path.
    #[arg(long)]
    pub(crate) repo: Option<String>,
    /// Downstream project directory.
    #[arg(long)]
    pub(crate) project: Option<PathBuf>,
    /// Build/install hook.
    #[arg(long)]
    pub(crate) setup: Option<String>,
    /// Workload hook.
    #[arg(long)]
    pub(crate) run: Option<String>,
    /// Classification mechanism.
    #[arg(long, value_enum)]
    pub(crate) oracle: Option<OracleKind>,
    /// Known-good artifact for comparison.
    #[arg(long)]
    pub(crate) baseline: Option<PathBuf>,
    /// Artifact path relative to BISECTRUNK_OUT.
    #[arg(long)]
    pub(crate) artifact: Option<PathBuf>,
    /// Custom artifact comparison hook.
    #[arg(long)]
    pub(crate) compare: Option<String>,
    /// Number of concurrent workers.
    #[arg(long)]
    pub(crate) jobs: Option<usize>,
    /// Number of bad/skip re-evaluations.
    #[arg(long)]
    pub(crate) retries: Option<usize>,
    /// Per-evaluation timeout such as 20m.
    #[arg(long)]
    pub(crate) timeout: Option<String>,
    /// Follow only the first parent.
    #[arg(long)]
    pub(crate) first_parent: bool,
    /// Restrict commits to paths in the subject repository.
    #[arg(long, num_args = 1.., action = clap::ArgAction::Append)]
    pub(crate) paths: Vec<PathBuf>,
    /// Explicit run directory.
    #[arg(long)]
    pub(crate) run_dir: Option<PathBuf>,
    /// Mirror and environment cache directory.
    #[arg(long)]
    pub(crate) cache_dir: Option<PathBuf>,
    /// Resource retention policy.
    #[arg(long, value_enum)]
    pub(crate) keep: Option<KeepPolicy>,
    /// Hook environment assignment, KEY=VALUE.
    #[arg(long, value_name = "K=V", action = clap::ArgAction::Append)]
    pub(crate) env: Vec<String>,
    /// Shell executable used for hooks.
    #[arg(long)]
    pub(crate) shell: Option<PathBuf>,
    /// Terminal output format.
    #[arg(long, value_enum)]
    pub(crate) format: Option<OutputFormat>,
    /// Treat a failed setup hook as bad instead of skip.
    #[arg(long, value_enum)]
    pub(crate) setup_failure: Option<SetupFailure>,
    /// Bypass completed evaluation cache reads.
    #[arg(long)]
    pub(crate) no_cache: bool,
    /// Configuration file path.
    #[arg(long)]
    pub(crate) config: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub(crate) struct BisectArgs {
    #[command(flatten)]
    pub(crate) shared: SharedArgs,
    /// Known-good revision.
    #[arg(long)]
    pub(crate) good: Option<String>,
    /// Known-bad revision.
    #[arg(long)]
    pub(crate) bad: Option<String>,
    /// Classification vocabulary as good,bad.
    #[arg(long)]
    pub(crate) terms: Option<String>,
    /// Skip endpoint classification checks.
    #[arg(long)]
    pub(crate) no_verify_endpoints: bool,
    /// Policy for non-monotone probe results.
    #[arg(long, value_enum)]
    pub(crate) on_inconsistent: Option<InconsistentPolicy>,
}

#[derive(Debug, Args)]
pub(crate) struct ScanArgs {
    #[command(flatten)]
    pub(crate) shared: SharedArgs,
    /// Revision range A..B.
    #[arg(long)]
    pub(crate) range: Option<String>,
    /// Evaluate every Nth commit.
    #[arg(long, conflicts_with = "sample")]
    pub(crate) stride: Option<usize>,
    /// Evaluate N evenly spaced commits.
    #[arg(long, conflicts_with = "stride")]
    pub(crate) sample: Option<usize>,
    /// Stop after the earliest bad classification is bracketed.
    #[arg(long)]
    pub(crate) stop_on_first_bad: bool,
}

#[derive(Debug, Args)]
pub(crate) struct RunArgs {
    #[command(flatten)]
    pub(crate) shared: SharedArgs,
    /// Revision to evaluate.
    #[arg(long)]
    pub(crate) at: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub(crate) enum OracleKind {
    #[default]
    Exit,
    Compare,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub(crate) enum KeepPolicy {
    All,
    #[default]
    Failed,
    None,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub(crate) enum OutputFormat {
    #[default]
    Auto,
    Json,
    Plain,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub(crate) enum SetupFailure {
    #[default]
    Skip,
    Bad,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub(crate) enum InconsistentPolicy {
    #[default]
    Abort,
    Leftmost,
    Retry,
}
