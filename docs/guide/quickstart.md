---
icon: lucide/package-open
---

# Quick start

Start with `run`, which evaluates one commit and makes hook development quick:

```bash
bisectrunk run --repo ../subject --at HEAD \
  --run 'test "$(cat "$BISECTRUNK_WORKTREE/marker.txt")" = good'
```

Once the hook behaves correctly, bisect a known-good/known-bad range:

```bash
bisectrunk bisect --repo ../subject --good v1.0.0 --bad HEAD \
  --run 'test "$(cat "$BISECTRUNK_WORKTREE/marker.txt")" = good' \
  --jobs 8
```

Endpoint verification runs first. Each round evaluates evenly spaced probes in
parallel, then narrows toward the leftmost bad result. The final output links to
the run directory, logs, JSON report, and Markdown report.

For histories with multiple transitions, scan instead:

```bash
bisectrunk scan --repo ../subject --range v1.0.0..HEAD \
  --run './classify.sh' --jobs 8
```

Press Ctrl-C once to stop dispatching new work. Finished evaluations are flushed;
use the printed `bisectrunk resume RUN_DIR` command to continue.
