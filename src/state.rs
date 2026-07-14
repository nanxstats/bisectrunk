use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::BufReader;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;

use crate::config::ResolvedConfig;
use crate::evaluate::Evaluation;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "strategy", rename_all = "lowercase")]
pub(crate) enum Operation {
    Run {
        at: String,
    },
    Scan {
        range: String,
        commits: Vec<String>,
        stride: usize,
        sample: Option<usize>,
        stop_on_first_bad: bool,
    },
    Bisect {
        good: String,
        bad: String,
        commits: Vec<String>,
        verify_endpoints: bool,
        on_inconsistent: crate::cli::InconsistentPolicy,
        terms: [String; 2],
    },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct RunPlan {
    pub(crate) config: ResolvedConfig,
    pub(crate) operation: Operation,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct RunState {
    pub(crate) version: u32,
    pub(crate) started_at: String,
    pub(crate) updated_at: String,
    pub(crate) complete: bool,
    pub(crate) interrupted: bool,
    pub(crate) evaluations: BTreeMap<String, Evaluation>,
    #[serde(default)]
    pub(crate) rounds: Vec<RoundRecord>,
    pub(crate) conclusion: Option<Conclusion>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct RoundRecord {
    pub(crate) number: usize,
    pub(crate) probes: Vec<String>,
    pub(crate) narrative: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct Conclusion {
    pub(crate) first_bad: String,
    pub(crate) last_good: String,
    #[serde(default)]
    pub(crate) candidates: Vec<String>,
    #[serde(default)]
    pub(crate) first_bad_metadata: Option<crate::gitrepo::CommitMetadata>,
    #[serde(default)]
    pub(crate) last_good_metadata: Option<crate::gitrepo::CommitMetadata>,
}

impl RunState {
    pub(crate) fn new() -> Self {
        let now = jiff::Timestamp::now().to_string();
        Self {
            version: 1,
            started_at: now.clone(),
            updated_at: now,
            complete: false,
            interrupted: false,
            evaluations: BTreeMap::new(),
            rounds: Vec::new(),
            conclusion: None,
        }
    }

    pub(crate) fn record(&mut self, evaluation: Evaluation) {
        self.updated_at = jiff::Timestamp::now().to_string();
        self.evaluations.insert(evaluation.sha.clone(), evaluation);
    }
}

pub(crate) fn save_plan(plan: &RunPlan) -> Result<()> {
    let path = plan.config.execution.run_dir.join("run.toml");
    let contents = toml::to_string_pretty(plan).context("serialize fully resolved run plan")?;
    fs::write(&path, contents).with_context(|| format!("write run plan {}", path.display()))
}

pub(crate) fn load_plan(run_dir: &Path) -> Result<RunPlan> {
    let path = run_dir.join("run.toml");
    let contents =
        fs::read_to_string(&path).with_context(|| format!("read run plan {}", path.display()))?;
    toml::from_str(&contents).with_context(|| format!("parse run plan {}", path.display()))
}

pub(crate) fn save_state(run_dir: &Path, state: &RunState) -> Result<()> {
    fs::create_dir_all(run_dir)
        .with_context(|| format!("create run directory {}", run_dir.display()))?;
    let path = run_dir.join("state.json");
    let mut temporary = NamedTempFile::new_in(run_dir)
        .with_context(|| format!("create temporary state file in {}", run_dir.display()))?;
    serde_json::to_writer_pretty(temporary.as_file_mut(), state)
        .with_context(|| format!("serialize state ledger {}", path.display()))?;
    temporary
        .persist(&path)
        .map_err(|error| error.error)
        .with_context(|| format!("replace state ledger {}", path.display()))?;
    Ok(())
}

pub(crate) fn load_state(run_dir: &Path) -> Result<RunState> {
    let path = run_dir.join("state.json");
    let file =
        File::open(&path).with_context(|| format!("open state ledger {}", path.display()))?;
    serde_json::from_reader(BufReader::new(file))
        .with_context(|| format!("parse state ledger {}", path.display()))
}

pub(crate) fn cache_key(config: &ResolvedConfig, sha: &str) -> Result<String> {
    let env = serde_json::to_string(&config.hooks.env).context("serialize hook environment")?;
    let oracle = serde_json::to_string(&config.oracle).context("serialize oracle config")?;
    let pins = serde_json::to_string(&config.pins).context("serialize pin config")?;
    let setup_failure = serde_json::to_string(&config.execution.setup_failure)
        .context("serialize setup-failure policy")?;
    Ok(crate::util::stable_hash(&[
        &config.subject.repo,
        sha,
        config.hooks.setup.as_deref().unwrap_or(""),
        &config.hooks.run,
        config.hooks.compare.as_deref().unwrap_or(""),
        &env,
        &oracle,
        &pins,
        &setup_failure,
    ]))
}

pub(crate) fn load_cache(config: &ResolvedConfig, sha: &str) -> Result<Option<Evaluation>> {
    if config.execution.no_cache {
        return Ok(None);
    }
    let path = cache_path(config, sha)?;
    if !path.exists() {
        return Ok(None);
    }
    let file =
        File::open(&path).with_context(|| format!("open evaluation cache {}", path.display()))?;
    serde_json::from_reader(BufReader::new(file))
        .with_context(|| format!("parse evaluation cache {}", path.display()))
        .map(Some)
}

pub(crate) fn save_cache(config: &ResolvedConfig, evaluation: &Evaluation) -> Result<()> {
    let path = cache_path(config, &evaluation.sha)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create evaluation cache {}", parent.display()))?;
    }
    let file = File::create(&path)
        .with_context(|| format!("create evaluation cache {}", path.display()))?;
    serde_json::to_writer_pretty(file, evaluation)
        .with_context(|| format!("write evaluation cache {}", path.display()))
}

fn cache_path(config: &ResolvedConfig, sha: &str) -> Result<PathBuf> {
    Ok(config
        .execution
        .cache_dir
        .join("evaluations")
        .join(format!("{}.json", cache_key(config, sha)?)))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::cli::SharedArgs;
    use crate::config::FileConfig;

    use super::*;

    #[test]
    fn cache_key_is_stable_for_reordered_equal_env() {
        let args = SharedArgs {
            repo: Some("repo".into()),
            run: Some("run".into()),
            ..SharedArgs::default()
        };
        let mut first = crate::config::resolve(&args, FileConfig::default()).expect("resolve");
        first.hooks.env = BTreeMap::from([("B".into(), "2".into()), ("A".into(), "1".into())]);
        let mut second = first.clone();
        second.hooks.env = BTreeMap::from([("A".into(), "1".into()), ("B".into(), "2".into())]);
        assert_eq!(
            cache_key(&first, "abc").expect("key"),
            cache_key(&second, "abc").expect("key")
        );
    }
}
