use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use serde::Serialize;

use crate::oracle::Classification;
use crate::state::{Operation, RunPlan, RunState};

#[derive(Debug, Serialize)]
struct Report<'a> {
    subject_repo: &'a str,
    strategy: &'static str,
    evaluations: Vec<&'a crate::evaluate::Evaluation>,
    transitions: Vec<Transition>,
    conclusion: &'a Option<crate::state::Conclusion>,
    complete: bool,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct Transition {
    pub(crate) from_sha: String,
    pub(crate) from: Classification,
    pub(crate) to_sha: String,
    pub(crate) to: Classification,
}

pub(crate) fn render(plan: &RunPlan, state: &RunState) -> Result<()> {
    let commits = operation_commits(&plan.operation);
    let evaluations = if commits.is_empty() {
        state.evaluations.values().collect::<Vec<_>>()
    } else {
        commits
            .iter()
            .filter_map(|sha| state.evaluations.get(sha))
            .collect::<Vec<_>>()
    };
    let transitions = transitions(commits, state);
    let report = Report {
        subject_repo: &plan.config.subject.repo,
        strategy: strategy_name(&plan.operation),
        evaluations,
        transitions,
        conclusion: &state.conclusion,
        complete: state.complete,
    };
    let run_dir = &plan.config.execution.run_dir;
    let json_path = run_dir.join("report.json");
    let json = serde_json::to_string_pretty(&report).context("serialize JSON report")?;
    fs::write(&json_path, json)
        .with_context(|| format!("write JSON report {}", json_path.display()))?;
    let markdown_path = run_dir.join("report.md");
    fs::write(&markdown_path, markdown(plan, state, &report))
        .with_context(|| format!("write Markdown report {}", markdown_path.display()))?;
    Ok(())
}

pub(crate) fn transitions(commits: &[String], state: &RunState) -> Vec<Transition> {
    let mut result = Vec::new();
    let mut previous = None;
    for sha in commits {
        let Some(evaluation) = state.evaluations.get(sha) else {
            continue;
        };
        if !matches!(
            evaluation.classification,
            Classification::Good | Classification::Bad
        ) {
            continue;
        }
        if let Some((previous_sha, previous_classification)) = previous
            && previous_classification != evaluation.classification
        {
            result.push(Transition {
                from_sha: previous_sha,
                from: previous_classification,
                to_sha: sha.clone(),
                to: evaluation.classification,
            });
        }
        previous = Some((sha.clone(), evaluation.classification));
    }
    result
}

fn markdown(plan: &RunPlan, state: &RunState, report: &Report<'_>) -> String {
    let mut output = format!(
        "# bisectrunk report\n\n- Subject repo: `{}`\n- Strategy: `{}`\n- Started: {}\n- Updated: {}\n\n",
        plan.config.subject.repo, report.strategy, state.started_at, state.updated_at
    );
    match &plan.operation {
        Operation::Scan { range, .. } => output.push_str(&format!("- Range: `{range}`\n\n")),
        Operation::Bisect { good, bad, .. } => {
            output.push_str(&format!("- Range: `{good}..{bad}`\n\n"));
        }
        Operation::Run { at } => output.push_str(&format!("- Commit: `{at}`\n\n")),
    }
    if !state.rounds.is_empty() {
        output.push_str("## Rounds\n\n");
        for round in &state.rounds {
            output.push_str(&format!("{}. {}\n", round.number, round.narrative));
        }
        output.push('\n');
    }
    output.push_str(
        "## Evaluations\n\n| SHA | Classification | Exit code | Duration |\n|---|---:|---:|---:|\n",
    );
    for evaluation in &report.evaluations {
        output.push_str(&format!(
            "| `{}` | {} | {} | {} |\n",
            crate::util::short_sha(&evaluation.sha),
            evaluation.classification,
            evaluation.exit_code,
            humantime::format_duration(std::time::Duration::from_millis(
                evaluation.duration_ms.min(u64::MAX as u128) as u64
            ))
        ));
    }
    output.push_str("\n## Transitions\n\n");
    if report.transitions.is_empty() {
        output.push_str("No classification transitions found.\n");
    } else {
        for transition in &report.transitions {
            output.push_str(&format!(
                "- `{}` {} → `{}` {}\n",
                crate::util::short_sha(&transition.from_sha),
                transition.from,
                crate::util::short_sha(&transition.to_sha),
                transition.to
            ));
        }
    }
    if let Some(conclusion) = &state.conclusion {
        output.push_str("\n## Conclusion\n\n");
        if conclusion.candidates.is_empty() {
            output.push_str(&format!(
                "First bad commit: `{}`\n\nLast good commit: `{}`\n",
                conclusion.first_bad, conclusion.last_good
            ));
            if let Some(metadata) = &conclusion.first_bad_metadata {
                output.push_str(&format!(
                    "\nFirst bad details: {} — {} — {}\n",
                    metadata.author, metadata.date, metadata.subject
                ));
            }
        } else {
            output.push_str("The first bad commit is one of:\n\n");
            for candidate in &conclusion.candidates {
                output.push_str(&format!("- `{candidate}`\n"));
            }
        }
    }
    let total = operation_commits(&plan.operation).len();
    let evaluated = state.evaluations.len();
    output.push_str(&format!(
        "\n## Savings\n\n{evaluated} evaluations instead of {total} (saved {}).\n",
        total.saturating_sub(evaluated)
    ));
    output
}

fn strategy_name(operation: &Operation) -> &'static str {
    match operation {
        Operation::Run { .. } => "run",
        Operation::Scan { .. } => "scan",
        Operation::Bisect { .. } => "bisect",
    }
}

pub(crate) fn operation_commits(operation: &Operation) -> &[String] {
    match operation {
        Operation::Run { .. } => &[],
        Operation::Scan { commits, .. } | Operation::Bisect { commits, .. } => commits,
    }
}

pub(crate) fn report_paths(run_dir: &Path) -> (String, String) {
    (
        run_dir.join("report.md").display().to_string(),
        run_dir.join("report.json").display().to_string(),
    )
}
