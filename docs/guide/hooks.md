# Hook contract

Hooks run through `sh -c` on Unix and `cmd /C` on Windows unless `--shell`
overrides the executable. Their working directory is the project directory. In
self-bisect mode (`--repo .` without `--project`) it is the detached worktree.

| Variable | Value |
|---|---|
| `BISECTRUNK_COMMIT` | Full candidate SHA. |
| `BISECTRUNK_COMMIT_SHORT` | Abbreviated SHA. |
| `BISECTRUNK_WORKTREE` | Detached subject checkout. |
| `BISECTRUNK_ENV` | Per-commit installation directory. |
| `BISECTRUNK_OUT` | Per-commit artifact directory. |
| `BISECTRUNK_PROJECT` | Downstream project directory. |
| `BISECTRUNK_JOB` | Zero-based worker number. |
| `BISECTRUNK_RUN_DIR` | Durable run directory. |
| `BISECTRUNK_PIN_ENVS` | PATH-style fixed-dependency environments, when configured. |
| `BISECTRUNK_BASELINE` | Baseline path inside custom compare hooks. |
| `BISECTRUNK_CANDIDATE` | Candidate path inside custom compare hooks. |

Pin setup hooks additionally receive `BISECTRUNK_PIN_ENV` and
`BISECTRUNK_PIN_WORKTREE`.

## Exit protocol

| Code | Meaning |
|---:|---|
| `0` | good; setup proceeds to run |
| `1` to `124`, `126`, `127` | bad; setup defaults to skip |
| `125` | skip / untestable |
| `128` or greater | abort the entire run |

Timeouts become skips. Hook stdout and stderr are combined under
`logs/<sha>/{setup,run}.log` and never interleaved on the terminal.
