# bisectrunk

[![CI tests](https://github.com/nanxstats/bisectrunk/actions/workflows/ci.yml/badge.svg)](https://github.com/nanxstats/bisectrunk/actions/workflows/ci.yml)
[![Documentation](https://github.com/nanxstats/bisectrunk/actions/workflows/docs.yml/badge.svg)](https://nanx.me/bisectrunk/)
[![crates.io](https://img.shields.io/crates/v/bisectrunk.svg)](https://crates.io/crates/bisectrunk)

`bisectrunk` is a parallel, environment-aware, resumable execution engine for
finding behavior changes in Git history.

Imagine that an R Markdown report or Jupyter notebook rendered correctly six
months ago but differs today. The project did not change; an unpinned upstream
dependency did. `bisectrunk` checks out candidate dependency commits into
detached worktrees, installs each into an isolated environment, runs the project,
and identifies the exact upstream change. The core is language-agnostic: shell
hooks describe how your ecosystem installs and tests a commit.

## Install

`bisectrunk` requires Git and Rust 1.85 or newer:

```bash
cargo install bisectrunk
```

## Quick start

For a subject repository whose own tests classify commits:

```bash
bisectrunk bisect \
  --repo . --good v2.3.0 --bad HEAD \
  --run 'cargo test -q' --jobs 8
```

R dependency with an exit-code oracle:

```bash
bisectrunk bisect \
  --repo https://github.com/tidyverse/dplyr \
  --good v1.1.0 --bad main \
  --setup 'R CMD INSTALL --library="$BISECTRUNK_ENV" "$BISECTRUNK_WORKTREE"' \
  --run 'R_LIBS_USER="$BISECTRUNK_ENV" Rscript test.R' \
  --jobs 8
```

Notebook drift with an artifact-comparison oracle:

```bash
bisectrunk bisect \
  --repo https://github.com/example/dependency \
  --good 2024-01-01 --bad HEAD \
  --setup 'uv pip install --python "$BISECTRUNK_ENV/bin/python" "$BISECTRUNK_WORKTREE"' \
  --run '"$BISECTRUNK_ENV/bin/python" -m jupyter nbconvert --execute report.ipynb --to html --output "$BISECTRUNK_OUT/report.html"' \
  --oracle compare --baseline golden/report.html --artifact report.html
```

Use `scan --range A..B` to map every classification and reveal multiple
transitions. Use `run --at REV` to develop hooks, `resume RUN_DIR` after an
interrupt, and `report RUN_DIR` to regenerate reports. Common controls include
`--jobs`, `--timeout`, `--retries`, `--paths`, `--first-parent`, `--format`, and
`--config`. See the [complete CLI reference](https://nanx.me/bisectrunk/guide/cli/).

## Hook exit-code protocol

| Exit code | Classification |
|---:|---|
| `0` | good |
| `1`–`124`, `126`, `127` | bad |
| `125` | skip / untestable |
| `128` or greater | abort the run |

Hook output is captured under the run directory rather than printed. Every run
also writes a resolved `run.toml`, resumable `state.json`, `report.json`, and
`report.md`.

## License

MIT
