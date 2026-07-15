---
icon: lucide/house
---

# bisectrunk

[![CI tests](https://github.com/nanxstats/bisectrunk/actions/workflows/ci.yml/badge.svg)](https://github.com/nanxstats/bisectrunk/actions/workflows/ci.yml)
[![Documentation](https://github.com/nanxstats/bisectrunk/actions/workflows/docs.yml/badge.svg)](https://nanx.me/bisectrunk/)
[![crates.io](https://img.shields.io/crates/v/bisectrunk.svg)](https://crates.io/crates/bisectrunk)

`bisectrunk` finds the commit that changed a result.
It searches for behavior changes in a Git repository while owning the
repetitive work around each candidate: isolated checkout, installation,
execution, classification, caching, and reporting.

## Why

A typical use case starts with report drift. A notebook or report rendered
correctly months ago, the downstream project has not changed, but an unpinned
dependency has.

Give `bisectrunk` a known-good revision, a known-bad revision, and shell hooks.
It evaluates multiple candidates at once and returns the exact upstream commit,
or an honest candidate set when broken commits make a unique answer impossible.

## Example

```bash
bisectrunk bisect --repo ../dependency --good v1.0.0 --bad main \
  --setup './install-into "$BISECTRUNK_ENV" "$BISECTRUNK_WORKTREE"' \
  --run './check-project' --jobs 8
```

## Beyond bisect

Use `scan` when behavior may be non-monotone, `run` while developing hooks, and
`resume` after an interruption. No R or Python runtime is built into the binary;
the hook contract works with any toolchain.
