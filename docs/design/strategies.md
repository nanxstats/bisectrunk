---
icon: lucide/split
---

# Search strategies

## Parallel k-section bisect

With `k` workers, a round chooses up to `k` evenly spaced interior probes. The
leftmost bad probe becomes the new upper bound; the rightmost good probe to its
left becomes the lower bound. A monotone range of `n` commits takes at most
`ceil(log_(k+1)(n))` rounds.

Skipped probes do not move a boundary. Subsequent rounds borrow nearby untested
commits. If every remaining interior commit is skipped, the correct result is a
candidate set rather than a fabricated first-bad SHA.

A good result to the right of a bad result is non-monotone. The default policy
stops and suggests `scan`; `leftmost` continues heuristically, while `retry`
re-evaluates the contradictory probes once.

## Parallel scan

Scan maps the evaluator over an ordered range and reports every good <-> bad
transition. `--stride` selects every Nth commit. `--sample` selects evenly spaced
commits and prints a follow-up bisect command around the first observed
transition.
