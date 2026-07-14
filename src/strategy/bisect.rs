use std::collections::BTreeSet;
use std::sync::atomic::AtomicBool;

use anyhow::{Context, Result};

use crate::cli::InconsistentPolicy;
use crate::config::ResolvedConfig;
use crate::mirror::Mirror;
use crate::oracle::Classification;
use crate::progress::Progress;
use crate::scheduler::StopReason;
use crate::state::{Conclusion, RoundRecord, RunState};

#[derive(Debug)]
pub(crate) enum BisectOutcome {
    Conclusive(Box<Conclusion>),
    Inconclusive {
        candidates: Vec<String>,
        message: String,
    },
    EndpointFailed(String),
    HookAbort,
    Interrupted,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum NarrowError {
    Inconsistent,
}

pub(crate) fn probes(low: usize, high: usize, jobs: usize, tested: &BTreeSet<usize>) -> Vec<usize> {
    let mut available = ((low + 1)..high)
        .filter(|index| !tested.contains(index))
        .collect::<Vec<_>>();
    let count = jobs.min(available.len());
    if count == 0 {
        return Vec::new();
    }
    let mut selected = Vec::with_capacity(count);
    for probe in 1..=count {
        let target = low + probe * (high - low) / (count + 1);
        let (position, _) = available
            .iter()
            .enumerate()
            .min_by_key(|(_, index)| (index.abs_diff(target), **index))
            // count is non-zero and one available entry is removed per iteration.
            .expect("available probe invariant");
        selected.push(available.remove(position));
    }
    selected.sort_unstable();
    selected
}

pub(crate) fn narrow_interval(
    low: usize,
    high: usize,
    classifications: &[(usize, Classification)],
) -> std::result::Result<(usize, usize), NarrowError> {
    let first_bad = classifications
        .iter()
        .filter(|(_, class)| *class == Classification::Bad)
        .map(|(index, _)| *index)
        .min();
    if let Some(bad) = first_bad
        && classifications
            .iter()
            .any(|(index, class)| *index > bad && *class == Classification::Good)
    {
        return Err(NarrowError::Inconsistent);
    }
    let new_high = first_bad.unwrap_or(high);
    let new_low = classifications
        .iter()
        .filter(|(index, class)| *index < new_high && *class == Classification::Good)
        .map(|(index, _)| *index)
        .max()
        .unwrap_or(low);
    Ok((new_low, new_high))
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn execute(
    config: &ResolvedConfig,
    mirror: &Mirror,
    commits: &[String],
    state: &mut RunState,
    progress: &Progress,
    interrupted: &AtomicBool,
    verify_endpoints: bool,
    inconsistent_policy: InconsistentPolicy,
    terms: &[String; 2],
) -> Result<BisectOutcome> {
    if commits.len() < 2 {
        anyhow::bail!("bisect range must contain distinct good and bad commits");
    }
    if verify_endpoints {
        let endpoints = [commits[0].clone(), commits[commits.len() - 1].clone()];
        match crate::scheduler::evaluate_commits(
            config,
            mirror,
            &endpoints,
            state,
            progress,
            interrupted,
            false,
        )? {
            StopReason::HookAbort => return Ok(BisectOutcome::HookAbort),
            StopReason::Interrupted => return Ok(BisectOutcome::Interrupted),
            StopReason::Complete | StopReason::FirstBad => {}
        }
        let good = state
            .evaluations
            .get(&commits[0])
            .with_context(|| format!("read verified good endpoint {}", commits[0]))?;
        let bad = state
            .evaluations
            .get(&commits[commits.len() - 1])
            .with_context(|| {
                format!("read verified bad endpoint {}", commits[commits.len() - 1])
            })?;
        if good.classification != Classification::Good {
            return Ok(BisectOutcome::EndpointFailed(format!(
                "endpoint {} was labeled {} but the oracle classified it {}",
                commits[0], terms[0], good.classification
            )));
        }
        if bad.classification != Classification::Bad {
            return Ok(BisectOutcome::EndpointFailed(format!(
                "endpoint {} was labeled {} but the oracle classified it {}",
                commits[commits.len() - 1],
                terms[1],
                bad.classification
            )));
        }
    }
    let mut low = 0usize;
    let mut high = commits.len() - 1;
    let mut round = 0usize;
    loop {
        if high == low + 1 {
            return Ok(BisectOutcome::Conclusive(Box::new(Conclusion {
                first_bad: commits[high].clone(),
                last_good: commits[low].clone(),
                candidates: Vec::new(),
                first_bad_metadata: None,
                last_good_metadata: None,
            })));
        }
        let tested = tested_indices(commits, state, low, high);
        let probe_indices = probes(low, high, config.execution.jobs, &tested);
        if probe_indices.is_empty() {
            return Ok(BisectOutcome::Inconclusive {
                candidates: commits[(low + 1)..=high].to_vec(),
                message: "the remaining interval contains only skipped commits".into(),
            });
        }
        round += 1;
        let probe_shas = probe_indices
            .iter()
            .map(|index| commits[*index].clone())
            .collect::<Vec<_>>();
        match crate::scheduler::evaluate_commits(
            config,
            mirror,
            &probe_shas,
            state,
            progress,
            interrupted,
            false,
        )? {
            StopReason::HookAbort => return Ok(BisectOutcome::HookAbort),
            StopReason::Interrupted => return Ok(BisectOutcome::Interrupted),
            StopReason::Complete | StopReason::FirstBad => {}
        }
        let mut classifications = interval_classifications(commits, state, low, high);
        let narrowed = narrow_interval(low, high, &classifications);
        let (new_low, new_high) = match narrowed {
            Ok(interval) => interval,
            Err(NarrowError::Inconsistent) => match inconsistent_policy {
                InconsistentPolicy::Abort => {
                    return Ok(BisectOutcome::Inconclusive {
                        candidates: commits[(low + 1)..=high].to_vec(),
                        message: "a good probe appeared to the right of a bad probe; use `bisectrunk scan` to inspect non-monotone history".into(),
                    });
                }
                InconsistentPolicy::Leftmost => {
                    progress.line(
                        "warning: inconsistent probe results; proceeding with the leftmost bad result",
                    );
                    narrow_leftmost(low, high, &classifications)
                }
                InconsistentPolicy::Retry => {
                    let contradictory = contradictory_shas(commits, &classifications);
                    for sha in &contradictory {
                        state.evaluations.remove(sha);
                    }
                    crate::state::save_state(&config.execution.run_dir, state)?;
                    let mut retry_config = config.clone();
                    retry_config.execution.no_cache = true;
                    match crate::scheduler::evaluate_commits(
                        &retry_config,
                        mirror,
                        &contradictory,
                        state,
                        progress,
                        interrupted,
                        false,
                    )? {
                        StopReason::HookAbort => return Ok(BisectOutcome::HookAbort),
                        StopReason::Interrupted => return Ok(BisectOutcome::Interrupted),
                        StopReason::Complete | StopReason::FirstBad => {}
                    }
                    classifications = interval_classifications(commits, state, low, high);
                    match narrow_interval(low, high, &classifications) {
                        Ok(interval) => interval,
                        Err(NarrowError::Inconsistent) => {
                            return Ok(BisectOutcome::Inconclusive {
                                candidates: commits[(low + 1)..=high].to_vec(),
                                message: "probe results remained inconsistent after one retry; use `bisectrunk scan`".into(),
                            });
                        }
                    }
                }
            },
        };
        let narrative = format!(
            "probed {}; narrowed (`{}`, `{}`] to (`{}`, `{}`]",
            probe_shas
                .iter()
                .map(|sha| crate::util::short_sha(sha))
                .collect::<Vec<_>>()
                .join(", "),
            crate::util::short_sha(&commits[low]),
            crate::util::short_sha(&commits[high]),
            crate::util::short_sha(&commits[new_low]),
            crate::util::short_sha(&commits[new_high])
        );
        if state.rounds.len() < round {
            state.rounds.push(RoundRecord {
                number: round,
                probes: probe_shas,
                narrative,
            });
            crate::state::save_state(&config.execution.run_dir, state)?;
        }
        low = new_low;
        high = new_high;
    }
}

fn tested_indices(
    commits: &[String],
    state: &RunState,
    low: usize,
    high: usize,
) -> BTreeSet<usize> {
    ((low + 1)..high)
        .filter(|index| state.evaluations.contains_key(&commits[*index]))
        .collect()
}

fn interval_classifications(
    commits: &[String],
    state: &RunState,
    low: usize,
    high: usize,
) -> Vec<(usize, Classification)> {
    ((low + 1)..high)
        .filter_map(|index| {
            state
                .evaluations
                .get(&commits[index])
                .map(|evaluation| (index, evaluation.classification))
        })
        .collect()
}

fn narrow_leftmost(
    low: usize,
    high: usize,
    classifications: &[(usize, Classification)],
) -> (usize, usize) {
    let new_high = classifications
        .iter()
        .filter(|(_, class)| *class == Classification::Bad)
        .map(|(index, _)| *index)
        .min()
        .unwrap_or(high);
    let new_low = classifications
        .iter()
        .filter(|(index, class)| *index < new_high && *class == Classification::Good)
        .map(|(index, _)| *index)
        .max()
        .unwrap_or(low);
    (new_low, new_high)
}

fn contradictory_shas(
    commits: &[String],
    classifications: &[(usize, Classification)],
) -> Vec<String> {
    let first_bad = classifications
        .iter()
        .filter(|(_, class)| *class == Classification::Bad)
        .map(|(index, _)| *index)
        .min()
        .unwrap_or(usize::MAX);
    classifications
        .iter()
        .filter(|(index, class)| {
            (*index == first_bad && *class == Classification::Bad)
                || (*index > first_bad && *class == Classification::Good)
        })
        .map(|(index, _)| commits[*index].clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn places_even_k_section_probes() {
        assert_eq!(probes(0, 40, 4, &BTreeSet::new()), [8, 16, 24, 32]);
        assert_eq!(probes(5, 9, 8, &BTreeSet::new()), [6, 7, 8]);
    }

    #[test]
    fn skipped_probe_borrows_nearest_untested_neighbor() {
        let tested = BTreeSet::from([4, 8]);
        assert_eq!(probes(0, 12, 2, &tested), [3, 7]);
    }

    #[test]
    fn narrows_and_detects_non_monotone_results() {
        assert_eq!(
            narrow_interval(
                0,
                20,
                &[(4, Classification::Good), (8, Classification::Bad)]
            ),
            Ok((4, 8))
        );
        assert_eq!(
            narrow_interval(
                0,
                20,
                &[(4, Classification::Bad), (8, Classification::Good)]
            ),
            Err(NarrowError::Inconsistent)
        );
    }

    proptest! {
        #[test]
        fn monotone_k_section_converges_within_log_bound(
            size in 1usize..=2000,
            boundary_seed in any::<usize>(),
            jobs in 1usize..=16,
        ) {
            let boundary = 1 + boundary_seed % size;
            let mut low = 0usize;
            let mut high = size;
            let mut rounds = 0usize;
            while high > low + 1 {
                let round_probes = probes(low, high, jobs, &BTreeSet::new());
                let classes = round_probes
                    .into_iter()
                    .map(|index| {
                        let class = if index < boundary {
                            Classification::Good
                        } else {
                            Classification::Bad
                        };
                        (index, class)
                    })
                    .collect::<Vec<_>>();
                (low, high) = narrow_interval(low, high, &classes).expect("monotone");
                rounds += 1;
            }
            prop_assert_eq!(high, boundary);
            let bound = if size <= 1 {
                0
            } else {
                (f64::log(size as f64, (jobs + 1) as f64).ceil() as usize).max(1)
            };
            prop_assert!(rounds <= bound, "{rounds} rounds > {bound} for n={size}, jobs={jobs}, boundary={boundary}");
        }
    }
}
