# CLI reference

Run `bisectrunk --help` or `bisectrunk <COMMAND> --help` for generated help.

## Commands

- `bisect`: parallel k-section search between `--good` and `--bad` (`HEAD` by default).
- `scan`: evaluate `--range A..B`, optionally with `--stride`, `--sample`, or
  `--stop-on-first-bad`.
- `run`: evaluate one revision selected by `--at`.
- `resume RUN_DIR`: continue from `state.json`.
- `report RUN_DIR`: regenerate both reports without evaluating.
- `clean [RUN_DIR] [--cache]`: remove run resources and optionally shared caches.

## Shared evaluation flags

| Flag | Purpose |
|---|---|
| `--repo URL_OR_PATH` | Subject repository. |
| `--project DIR` | Downstream project and hook working directory. |
| `--setup CMD` | Build/install hook. |
| `--run CMD` | Required workload hook. |
| `--oracle exit\|compare` | Classification mechanism. |
| `--baseline FILE` | Known-good artifact for compare mode. |
| `--artifact RELPATH` | Candidate artifact below `BISECTRUNK_OUT`. |
| `--compare CMD` | Custom compare hook. |
| `--jobs N` | Worker count; defaults to logical CPUs capped at eight. |
| `--retries N` | Re-evaluate bad/skip results. |
| `--timeout DURATION` | Per-hook limit such as `20m`; timeout means skip. |
| `--first-parent` | Follow only first-parent history. |
| `--paths PATH...` | Keep commits touching selected subject paths. |
| `--run-dir DIR` | Explicit run directory. |
| `--cache-dir DIR` | Explicit mirror/environment/evaluation cache. |
| `--keep all\|failed\|none` | Retention policy. |
| `--env K=V` | Extra hook environment; repeatable. |
| `--shell PATH` | Override `sh -c` / `cmd /C`. |
| `--format auto\|json\|plain` | Interactive, JSON-lines, or plain output. |
| `--setup-failure skip\|bad` | Classification for nonzero setup hooks. |
| `--no-cache` | Bypass completed evaluation cache reads. |
| `--config FILE` | Explicit TOML configuration. |

## Bisect flags

`--good REV`, `--bad REV`, `--terms GOOD,BAD`, `--no-verify-endpoints`, and
`--on-inconsistent abort|leftmost|retry`. Endpoint verification is enabled by
default; configuration may set `verify_endpoints = false`.

## Scan and run flags

Scan uses `--range A..B`, `--stride N`, `--sample N`, and
`--stop-on-first-bad`. Run uses `--at REV`.

## Configuration

By default `bisectrunk.toml` is loaded from the project directory. CLI values
override file values.

```toml
[subject]
repo = "../dependency"
first_parent = true
paths = ["src/"]

[hooks]
setup = './install "$BISECTRUNK_WORKTREE" "$BISECTRUNK_ENV"'
run = "./test.sh"

[oracle]
kind = "compare"
baseline = "golden/output.txt"
artifact = "output.txt"
normalize = ["timestamp: \\d+"]

[execution]
jobs = 8
timeout = "20m"
retries = 1

[[pins]]
repo = "../companion"
rev = "v2.0.0"
setup = './install "$BISECTRUNK_PIN_WORKTREE" "$BISECTRUNK_PIN_ENV"'
```

Standard `--help` and `--version` switches are also available.
