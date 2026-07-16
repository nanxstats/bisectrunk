# Changelog

## bisectrunk 0.1.2

### Dependencies

- Use full version requirements for Cargo dependencies (#11).

## bisectrunk 0.1.1

### Dependencies

- Updated `etcetera`, `git2`, `similar`, and `toml` to their latest compatible
  major versions (#8).
- Raised the minimum supported Rust version from 1.85 to 1.87 since
  `etcetera` v0.11 requires it (#8).

## bisectrunk 0.1.0

### New features

- Initial public release of the bisectrunk CLI (#1).
- Parallel k-section bisection and parallel full history scanning.
- Detached worktree, per-commit environment, pin, evaluation, and mirror caches.
- Exit-code and normalized artifact-comparison oracles with unified diffs.
- Durable run ledgers, reports, Ctrl-C recovery, and `resume` support.
- Plain, interactive, and JSON-lines progress output.
- Synthetic Git fixture, integration, unit, and property-based test coverage.
