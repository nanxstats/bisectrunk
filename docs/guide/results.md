---
icon: lucide/archive-restore
---

# Results and recovery

Each run creates `bisectrunk-runs/YYYYMMDD-HHMMSS-ID/` unless `--run-dir`
selects another location:

```text
run.toml
state.json
logs/<sha>/setup.log
logs/<sha>/run.log
out/<sha>/
report.json
report.md
```

`run.toml` is the fully resolved plan. `state.json` is the source of truth and is
atomically replaced after each finished evaluation. `report.md` explains rounds,
classifications, transitions, the conclusion, savings, and artifact diffs;
`report.json` carries the same core data for automation.

After Ctrl-C, run the printed command:

```bash
bisectrunk resume bisectrunk-runs/20260714-120000-a1b2c3d
```

Completed ledger entries are not evaluated again. `report RUN_DIR` regenerates
reports, while `clean RUN_DIR` removes a run and `clean --cache` clears shared
mirrors, environments, pins, and evaluation results.
