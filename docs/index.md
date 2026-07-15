# bisectrunk

## Find the commit that changed the result

`bisectrunk` finds behavior changes in a Git repository while owning the
repetitive work around each candidate: isolated checkout, installation,
execution, classification, caching, and reporting.

A typical case starts with report drift. An R Markdown report or notebook
rendered correctly months ago, the downstream project has not changed, but an
unpinned dependency has. Give `bisectrunk` a known-good revision, a known-bad
revision, and shell hooks. It evaluates multiple candidates at once and returns
the exact upstream commit, or an honest candidate set when broken commits make a
unique answer impossible.

```bash
bisectrunk bisect --repo ../dependency --good v1.0.0 --bad main \
  --setup './install-into "$BISECTRUNK_ENV" "$BISECTRUNK_WORKTREE"' \
  --run './check-project' --jobs 8
```

Use `scan` when behavior may be non-monotone, `run` while developing hooks, and
`resume` after an interruption. No R or Python runtime is built into the binary;
the hook contract works with any toolchain.
