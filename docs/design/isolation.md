---
icon: lucide/container
---

# Isolation and caching

One bare mirror per canonical repository identity lives below
`<cache>/repos/<blake3>`. Every worker attaches a detached worktree to that
mirror. Worktree registration changes are serialized, while setup and run hooks
remain parallel. Cleanup always removes and prunes registrations.

Per-commit environment directories are keyed by repository, SHA, and setup hook.
A changed run hook can therefore reuse expensive installations. Evaluation
results use a broader key containing the repository, SHA, setup, run/oracle
configuration, retries, and pins. `--no-cache` bypasses completed evaluation
reads without discarding installed environments.

Fixed companion dependencies (`[[pins]]`) are installed once into shared pin
environments. Candidate hooks receive their paths through
`BISECTRUNK_PIN_ENVS`.
