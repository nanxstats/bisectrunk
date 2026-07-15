---
icon: lucide/refresh-ccw
---

# Dependency choices

- `git2` performs read-only history operations against local mirrors.
- `xshell` invokes Git for authentication-aware clone/fetch and worktree lifecycle.
- `crossbeam-channel` and scoped standard threads implement the worker pool.
- `serde`, `serde_json`, and `toml` persist inspectable state and configuration.
- `indicatif` and `console` provide terminal progress; JSON-lines needs no ANSI.
- `blake3` creates cache identities; `similar` creates bounded unified diffs.
- `humantime`, `jiff`, and `etcetera` handle durations, timestamps, and platform
  cache locations.

Subprocess orchestration does not need `tokio`; scheduling is stateful rather
than a rayon-style parallel iterator; Git itself handles network transport, so
there is no `reqwest` dependency.
