use std::sync::atomic::AtomicBool;

use anyhow::Result;

use crate::config::ResolvedConfig;
use crate::mirror::Mirror;
use crate::progress::Progress;
use crate::scheduler::StopReason;
use crate::state::RunState;

pub(crate) fn select_commits(
    commits: Vec<String>,
    stride: usize,
    sample: Option<usize>,
) -> Vec<String> {
    if let Some(sample) = sample {
        if sample == 0 || commits.is_empty() {
            return Vec::new();
        }
        if sample >= commits.len() {
            return commits;
        }
        if sample == 1 {
            return vec![commits[commits.len() / 2].clone()];
        }
        let last = commits.len() - 1;
        return (0..sample)
            .map(|index| commits[index * last / (sample - 1)].clone())
            .collect();
    }
    commits.into_iter().step_by(stride.max(1)).collect()
}

pub(crate) fn execute(
    config: &ResolvedConfig,
    mirror: &Mirror,
    commits: &[String],
    state: &mut RunState,
    progress: &Progress,
    interrupted: &AtomicBool,
    stop_on_first_bad: bool,
) -> Result<StopReason> {
    crate::scheduler::evaluate_commits(
        config,
        mirror,
        commits,
        state,
        progress,
        interrupted,
        stop_on_first_bad,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stride_and_sample_selection_are_stable() {
        let commits = (0..10).map(|value| value.to_string()).collect::<Vec<_>>();
        assert_eq!(
            select_commits(commits.clone(), 3, None),
            ["0", "3", "6", "9"]
        );
        assert_eq!(select_commits(commits, 1, Some(4)), ["0", "3", "6", "9"]);
    }
}
