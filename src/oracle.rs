use std::fs;
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use similar::TextDiff;

use crate::config::ResolvedConfig;
use crate::hooks::HookContext;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Classification {
    Good,
    Bad,
    Skip,
    Abort,
}

impl std::fmt::Display for Classification {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let value = match self {
            Self::Good => "good",
            Self::Bad => "bad",
            Self::Skip => "skip",
            Self::Abort => "abort",
        };
        formatter.write_str(value)
    }
}

#[derive(Debug)]
pub(crate) struct CompareResult {
    pub(crate) classification: Classification,
    pub(crate) exit_code: i32,
    pub(crate) diff: Option<String>,
}

pub(crate) fn classify_exit(code: i32, timed_out: bool) -> Classification {
    if timed_out || code == 125 {
        Classification::Skip
    } else if code == 0 {
        Classification::Good
    } else if code >= 128 {
        Classification::Abort
    } else {
        Classification::Bad
    }
}

pub(crate) fn compare_artifact(
    config: &ResolvedConfig,
    context: &HookContext<'_>,
    log_dir: &Path,
    timeout: Option<Duration>,
) -> Result<CompareResult> {
    let artifact = config
        .oracle
        .artifact
        .as_ref()
        .context("compare oracle artifact was not resolved")?;
    let candidate = context.out_dir.join(artifact);
    if !candidate.is_file() {
        return Ok(CompareResult {
            classification: Classification::Skip,
            exit_code: 125,
            diff: None,
        });
    }
    let baseline_setting = config
        .oracle
        .baseline
        .as_ref()
        .context("compare oracle baseline was not resolved")?;
    let baseline = if baseline_setting.is_absolute() {
        baseline_setting.clone()
    } else {
        context.project.join(baseline_setting)
    };
    if !baseline.is_file() {
        anyhow::bail!("comparison baseline {} does not exist", baseline.display());
    }
    if let Some(command) = &config.hooks.compare {
        let compare_context = HookContext {
            baseline: Some(&baseline),
            candidate: Some(&candidate),
            ..context.clone()
        };
        let result = crate::hooks::execute(
            command,
            config.hooks.shell.as_deref(),
            context.project,
            &log_dir.join("compare.log"),
            &compare_context,
            timeout,
        )
        .with_context(|| format!("execute compare hook for commit {}", context.commit))?;
        let classification = classify_exit(result.code, result.timed_out);
        let diff = if classification == Classification::Bad {
            make_diff(&baseline, &candidate, &config.oracle.normalize)?
        } else {
            None
        };
        return Ok(CompareResult {
            classification,
            exit_code: result.code,
            diff,
        });
    }
    builtin_compare(&baseline, &candidate, &config.oracle.normalize)
}

fn builtin_compare(
    baseline: &Path,
    candidate: &Path,
    patterns: &[String],
) -> Result<CompareResult> {
    let baseline_bytes = fs::read(baseline)
        .with_context(|| format!("read comparison baseline {}", baseline.display()))?;
    let candidate_bytes = fs::read(candidate)
        .with_context(|| format!("read candidate artifact {}", candidate.display()))?;
    if baseline_bytes == candidate_bytes {
        return Ok(CompareResult {
            classification: Classification::Good,
            exit_code: 0,
            diff: None,
        });
    }
    let normalized_equal = match (
        std::str::from_utf8(&baseline_bytes),
        std::str::from_utf8(&candidate_bytes),
    ) {
        (Ok(baseline_text), Ok(candidate_text)) => {
            normalize_text(baseline_text, patterns)? == normalize_text(candidate_text, patterns)?
        }
        _ => false,
    };
    if normalized_equal {
        return Ok(CompareResult {
            classification: Classification::Good,
            exit_code: 0,
            diff: None,
        });
    }
    Ok(CompareResult {
        classification: Classification::Bad,
        exit_code: 1,
        diff: make_diff(baseline, candidate, patterns)?,
    })
}

pub(crate) fn normalize_text(input: &str, patterns: &[String]) -> Result<String> {
    let mut normalized = input.replace("\r\n", "\n").replace('\r', "\n");
    for pattern in patterns {
        let regex = regex::Regex::new(pattern)
            .with_context(|| format!("compile oracle normalization regex {pattern:?}"))?;
        normalized = regex.replace_all(&normalized, "").into_owned();
    }
    Ok(normalized)
}

fn make_diff(baseline: &Path, candidate: &Path, patterns: &[String]) -> Result<Option<String>> {
    const MAX_DIFF_BYTES: usize = 64 * 1024;
    let baseline_bytes = fs::read(baseline)
        .with_context(|| format!("read comparison baseline {} for diff", baseline.display()))?;
    let candidate_bytes = fs::read(candidate)
        .with_context(|| format!("read candidate artifact {} for diff", candidate.display()))?;
    let (Ok(baseline_text), Ok(candidate_text)) = (
        std::str::from_utf8(&baseline_bytes),
        std::str::from_utf8(&candidate_bytes),
    ) else {
        return Ok(Some(
            "binary artifacts differ; no text diff available".into(),
        ));
    };
    let baseline_text = normalize_text(baseline_text, patterns)?;
    let candidate_text = normalize_text(candidate_text, patterns)?;
    let mut diff = TextDiff::from_lines(&baseline_text, &candidate_text)
        .unified_diff()
        .header("baseline", "candidate")
        .to_string();
    if diff.len() > MAX_DIFF_BYTES {
        let mut boundary = MAX_DIFF_BYTES;
        while !diff.is_char_boundary(boundary) {
            boundary -= 1;
        }
        diff.truncate(boundary);
        diff.push_str("\n... diff truncated by bisectrunk ...\n");
    }
    Ok(Some(diff))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_protocol_matches_git_bisect_run() {
        assert_eq!(classify_exit(0, false), Classification::Good);
        assert_eq!(classify_exit(1, false), Classification::Bad);
        assert_eq!(classify_exit(125, false), Classification::Skip);
        assert_eq!(classify_exit(127, false), Classification::Bad);
        assert_eq!(classify_exit(128, false), Classification::Abort);
        assert_eq!(classify_exit(0, true), Classification::Skip);
    }

    #[test]
    fn text_normalization_handles_line_endings_and_regexes() {
        let patterns = vec![r"timestamp: \d+".to_owned()];
        let left = normalize_text("value\r\ntimestamp: 123\r\n", &patterns).expect("normalize");
        let right = normalize_text("value\ntimestamp: 999\n", &patterns).expect("normalize");
        assert_eq!(left, right);
    }
}
