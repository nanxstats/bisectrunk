# AGENTS.md

`bisectrunk` is a synchronous Rust CLI. Keep the core language-agnostic: all
ecosystem behavior belongs in user hooks.

## Architecture

- `cli.rs`: clap syntax only. Every new long flag must also be added to
  `docs/guide/cli.md`; a test enforces this.
- `config.rs`: TOML schema and explicit CLI-over-file resolution.
- `gitrepo.rs`: read-only history, range, metadata, path, and merge-base logic.
- `mirror.rs`: cached mirror acquisition and fetch.
- `worktree.rs`: detached worktree registration and cleanup.
- `hooks.rs`: shell process contract, log capture, and timeouts.
- `oracle.rs`: exit protocol, artifact normalization/comparison, and diffs.
- `evaluate.rs`: the single composition point for one subject commit.
- `scheduler.rs`: scoped worker threads and crossbeam channels.
- `strategy/`: commit selection only; both strategies use the scheduler/evaluator.
- `state.rs`: resolved run plans, atomic ledger writes, and evaluation cache.
- `report.rs` and `progress.rs`: all user-visible reports and status output.
- `pins.rs`: fixed companion dependency setup and shared pin environments.
- `lib.rs`: parse, resolve, and dispatch orchestration.

## Non-negotiable rules

- Use `git2` only for read-only local history operations. Use the Git CLI via
  `xshell` for clone, fetch, and every worktree lifecycle operation so credential
  helpers and SSH configuration behave exactly as users expect.
- Reuse `xshell` rather than raw `Command` for Git invocations.
- Always remove and prune registered worktrees, including interrupt paths.
- Tests must never require R, Python, or network access. Build deterministic
  synthetic repositories and use inline shell hooks.
- Hook stdout/stderr belongs only in per-commit log files.
- New public behavior requires tests and documentation. New flags require an
  update to `docs/guide/cli.md`.
- Update `CHANGELOG.md` using Keep a Changelog sections for user-visible changes.
- Before every commit run `cargo fmt`,
  `cargo clippy --all-targets -- -D warnings`, and `cargo test`.
- No `unsafe`; add `anyhow::Context` at fallible boundaries; avoid `unwrap` and
  `expect` outside tests except commented static invariants.
