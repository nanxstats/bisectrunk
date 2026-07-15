use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use etcetera::{BaseStrategy, choose_base_strategy};
use serde::{Deserialize, Serialize};

use crate::cli::{
    InconsistentPolicy, KeepPolicy, OracleKind, OutputFormat, SetupFailure, SharedArgs,
};

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub(crate) struct FileConfig {
    #[serde(default)]
    pub(crate) subject: SubjectConfig,
    #[serde(default)]
    pub(crate) hooks: HooksConfig,
    #[serde(default)]
    pub(crate) oracle: OracleConfig,
    #[serde(default)]
    pub(crate) execution: ExecutionConfig,
    #[serde(default)]
    pub(crate) pins: Vec<PinConfig>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub(crate) struct SubjectConfig {
    pub(crate) repo: Option<String>,
    pub(crate) project: Option<PathBuf>,
    pub(crate) first_parent: Option<bool>,
    pub(crate) paths: Option<Vec<PathBuf>>,
    pub(crate) good: Option<String>,
    pub(crate) bad: Option<String>,
    pub(crate) range: Option<String>,
    pub(crate) at: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub(crate) struct HooksConfig {
    pub(crate) setup: Option<String>,
    pub(crate) run: Option<String>,
    pub(crate) compare: Option<String>,
    pub(crate) shell: Option<PathBuf>,
    #[serde(default)]
    pub(crate) env: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub(crate) struct OracleConfig {
    pub(crate) kind: Option<OracleKind>,
    pub(crate) baseline: Option<PathBuf>,
    pub(crate) artifact: Option<PathBuf>,
    #[serde(default)]
    pub(crate) normalize: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub(crate) struct ExecutionConfig {
    pub(crate) jobs: Option<usize>,
    pub(crate) retries: Option<usize>,
    pub(crate) timeout: Option<String>,
    pub(crate) run_dir: Option<PathBuf>,
    pub(crate) cache_dir: Option<PathBuf>,
    pub(crate) keep: Option<KeepPolicy>,
    pub(crate) format: Option<OutputFormat>,
    pub(crate) setup_failure: Option<SetupFailure>,
    pub(crate) no_cache: Option<bool>,
    pub(crate) terms: Option<String>,
    pub(crate) verify_endpoints: Option<bool>,
    pub(crate) on_inconsistent: Option<InconsistentPolicy>,
    pub(crate) stride: Option<usize>,
    pub(crate) sample: Option<usize>,
    pub(crate) stop_on_first_bad: Option<bool>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct PinConfig {
    pub(crate) repo: String,
    pub(crate) rev: String,
    pub(crate) setup: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct ResolvedConfig {
    pub(crate) subject: ResolvedSubject,
    pub(crate) hooks: ResolvedHooks,
    pub(crate) oracle: ResolvedOracle,
    pub(crate) execution: ResolvedExecution,
    pub(crate) pins: Vec<PinConfig>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct ResolvedSubject {
    pub(crate) repo: String,
    pub(crate) project: PathBuf,
    pub(crate) self_bisect: bool,
    pub(crate) first_parent: bool,
    pub(crate) paths: Vec<PathBuf>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct ResolvedHooks {
    pub(crate) setup: Option<String>,
    pub(crate) run: String,
    pub(crate) compare: Option<String>,
    pub(crate) shell: Option<PathBuf>,
    pub(crate) env: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct ResolvedOracle {
    pub(crate) kind: OracleKind,
    pub(crate) baseline: Option<PathBuf>,
    pub(crate) artifact: Option<PathBuf>,
    pub(crate) normalize: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct ResolvedExecution {
    pub(crate) jobs: usize,
    pub(crate) retries: usize,
    pub(crate) timeout: Option<String>,
    pub(crate) run_dir: PathBuf,
    pub(crate) cache_dir: PathBuf,
    pub(crate) keep: KeepPolicy,
    pub(crate) format: OutputFormat,
    pub(crate) setup_failure: SetupFailure,
    pub(crate) no_cache: bool,
}

pub(crate) fn load(shared: &SharedArgs) -> Result<FileConfig> {
    let path = if let Some(path) = &shared.config {
        Some(path.clone())
    } else {
        let candidate = shared
            .project
            .clone()
            .unwrap_or(std::env::current_dir().context("determine current directory")?)
            .join("bisectrunk.toml");
        candidate.exists().then_some(candidate)
    };
    let Some(path) = path else {
        return Ok(FileConfig::default());
    };
    let contents = fs::read_to_string(&path)
        .with_context(|| format!("read configuration {}", path.display()))?;
    toml::from_str(&contents).with_context(|| format!("parse configuration {}", path.display()))
}

pub(crate) fn resolve(shared: &SharedArgs, file: FileConfig) -> Result<ResolvedConfig> {
    let cwd = std::env::current_dir().context("determine current directory")?;
    let repo =
        shared.repo.clone().or(file.subject.repo).ok_or_else(|| {
            anyhow!("--repo is required (or set subject.repo in bisectrunk.toml)")
        })?;
    let repo_path = Path::new(&repo);
    let canonical_cwd = cwd.canonicalize().unwrap_or_else(|_| cwd.clone());
    let canonical_repo = repo_path.canonicalize().ok();
    let self_bisect = canonical_repo.as_ref() == Some(&canonical_cwd);
    let project_from_user = shared.project.clone().or(file.subject.project);
    let project = match project_from_user {
        Some(path) => path,
        None => cwd,
    };
    let run = shared
        .run
        .clone()
        .or(file.hooks.run)
        .ok_or_else(|| anyhow!("--run is required (or set hooks.run in bisectrunk.toml)"))?;
    let jobs = shared
        .jobs
        .or(file.execution.jobs)
        .unwrap_or_else(default_jobs);
    if jobs == 0 {
        bail!("--jobs must be at least 1");
    }
    let timeout = shared.timeout.clone().or(file.execution.timeout);
    if let Some(value) = &timeout {
        humantime::parse_duration(value)
            .with_context(|| format!("parse timeout duration {value:?}"))?;
    }
    let mut env = file.hooks.env;
    for assignment in &shared.env {
        let (key, value) = assignment
            .split_once('=')
            .ok_or_else(|| anyhow!("invalid --env {assignment:?}; expected KEY=VALUE"))?;
        if key.is_empty() {
            bail!("invalid --env {assignment:?}; key is empty");
        }
        env.insert(key.to_owned(), value.to_owned());
    }
    let cache_dir = shared
        .cache_dir
        .clone()
        .or(file.execution.cache_dir)
        .map(Ok)
        .unwrap_or_else(default_cache_dir)?;
    let run_dir = shared
        .run_dir
        .clone()
        .or(file.execution.run_dir)
        .unwrap_or_else(|| crate::util::new_run_dir(&canonical_cwd));
    let first_parent = if shared.first_parent {
        true
    } else {
        file.subject.first_parent.unwrap_or(false)
    };
    let paths = if shared.paths.is_empty() {
        file.subject.paths.unwrap_or_default()
    } else {
        shared.paths.clone()
    };
    let oracle_kind = shared.oracle.or(file.oracle.kind).unwrap_or_default();
    let baseline = shared.baseline.clone().or(file.oracle.baseline);
    let artifact = shared.artifact.clone().or(file.oracle.artifact);
    if oracle_kind == OracleKind::Compare {
        if baseline.is_none() {
            bail!("--baseline is required with --oracle compare");
        }
        let artifact_path = artifact
            .as_ref()
            .ok_or_else(|| anyhow!("--artifact is required with --oracle compare"))?;
        if artifact_path.is_absolute()
            || artifact_path.components().any(|component| {
                matches!(
                    component,
                    std::path::Component::ParentDir
                        | std::path::Component::RootDir
                        | std::path::Component::Prefix(_)
                )
            })
        {
            bail!("--artifact must be a relative path below BISECTRUNK_OUT");
        }
    }
    for pattern in &file.oracle.normalize {
        regex::Regex::new(pattern)
            .with_context(|| format!("compile oracle normalization regex {pattern:?}"))?;
    }
    Ok(ResolvedConfig {
        subject: ResolvedSubject {
            repo,
            project,
            self_bisect: self_bisect && shared.project.is_none(),
            first_parent,
            paths,
        },
        hooks: ResolvedHooks {
            setup: shared.setup.clone().or(file.hooks.setup),
            run,
            compare: shared.compare.clone().or(file.hooks.compare),
            shell: shared.shell.clone().or(file.hooks.shell),
            env,
        },
        oracle: ResolvedOracle {
            kind: oracle_kind,
            baseline,
            artifact,
            normalize: file.oracle.normalize,
        },
        execution: ResolvedExecution {
            jobs,
            retries: shared.retries.or(file.execution.retries).unwrap_or(0),
            timeout,
            run_dir,
            cache_dir,
            keep: shared.keep.or(file.execution.keep).unwrap_or_default(),
            format: shared.format.or(file.execution.format).unwrap_or_default(),
            setup_failure: shared
                .setup_failure
                .or(file.execution.setup_failure)
                .unwrap_or_default(),
            no_cache: shared.no_cache || file.execution.no_cache.unwrap_or(false),
        },
        pins: file.pins,
    })
}

fn default_jobs() -> usize {
    std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1)
        .min(8)
}

pub(crate) fn default_cache_dir() -> Result<PathBuf> {
    let strategy = choose_base_strategy().context("determine platform cache directory")?;
    Ok(strategy.cache_dir().join("bisectrunk"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_values_override_file_values() {
        let args = SharedArgs {
            repo: Some("cli-repo".into()),
            run: Some("cli-run".into()),
            jobs: Some(7),
            env: vec!["SAME=cli".into(), "CLI=yes".into()],
            ..SharedArgs::default()
        };
        let mut file = FileConfig::default();
        file.subject.repo = Some("file-repo".into());
        file.hooks.run = Some("file-run".into());
        file.hooks.env.insert("SAME".into(), "file".into());
        file.hooks.env.insert("FILE".into(), "yes".into());
        file.execution.jobs = Some(2);
        let resolved = resolve(&args, file).expect("config resolves");
        assert_eq!(resolved.subject.repo, "cli-repo");
        assert_eq!(resolved.hooks.run, "cli-run");
        assert_eq!(resolved.execution.jobs, 7);
        assert_eq!(resolved.hooks.env["SAME"], "cli");
        assert_eq!(resolved.hooks.env["FILE"], "yes");
        assert_eq!(resolved.hooks.env["CLI"], "yes");
    }

    #[test]
    fn file_values_fill_unspecified_cli_values() {
        let args = SharedArgs::default();
        let mut file = FileConfig::default();
        file.subject.repo = Some("file-repo".into());
        file.hooks.run = Some("file-run".into());
        file.execution.jobs = Some(3);
        let resolved = resolve(&args, file).expect("config resolves");
        assert_eq!(resolved.subject.repo, "file-repo");
        assert_eq!(resolved.hooks.run, "file-run");
        assert_eq!(resolved.execution.jobs, 3);
    }
}
