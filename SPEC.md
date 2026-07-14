# bisectrunk design specification

## 1. Problem statement

A downstream project (an R Markdown report, a Jupyter notebook, a test script,
a build) produces different output today than it did months ago because an
**unpinned upstream dependency** changed. The dependency's Git history holds
the answer, but finding the exact commit that introduced the change means,
for each candidate commit: check out the dependency at that commit,
build/install it into an isolated environment, run the downstream workload
against it, and judge the result. `git bisect` navigates history but knows
nothing about environments, installation, execution, or parallelism.
Ad-hoc scripts automate one narrow shape of this and are tied to a
single language toolchain.

`bisectrunk` generalizes this into a language-agnostic, parallel, resumable
**bisect executor**: it owns history navigation, per-commit checkout,
environment isolation, scheduling, result classification, and reporting,
while delegating the ecosystem-specific parts (how to install, how to run,
how to judge) to user-supplied hook commands governed by a small, stable contract.

## 2. Design principles

1. **Language-agnostic core, contract at the edges.**
   The core never assumes R, Python, or any toolchain. All ecosystem knowledge
   lives in `setup` / `run` / `compare` hook commands that communicate through
   environment variables and exit codes. Documented recipes (R, Python/uv, Rust)
   are just example hooks.
2. **`git bisect run` compatible.**
   The exit-code protocol (0 = good, 125 = skip, 1--127 = bad, >=128 = abort)
   is preserved verbatim, so any script already written for `git bisect run`
   works under `bisectrunk` unchanged, and gains parallelism, isolation,
   and reports for free.
3. **Parallelism as a first-class strategy, not a bolt-on.**
   Scanning is embarrassingly parallel; bisecting parallelizes via *k*-section
   search (test *k* interior points per round, shrinking the range by a factor
   of *k* + 1 per round instead of 2).
4. **Everything is resumable and inspectable.**
   Every evaluation is cached by `(repo, commit, hooks-hash)`; a run directory
   holds logs, artifacts, state, and reports. Interrupting and re-running never
   repeats finished work.
5. **Don't touch the user's stuff.**
   The user's project directory is never modified; dependency checkouts live in
   detached worktrees off a cached mirror clone; installations go into
   per-commit environment directories.
6. **Single static binary.**
   No runtime dependency on R or Python. The only external requirement is a
   `git` executable on `PATH` (see §8 for the git2-vs-CLI split).

## 3. Concepts and terminology

| Term | Meaning |
|---|---|
| **Subject repo** | The Git repository being bisected (usually a dependency; may be the project itself). |
| **Project dir** | The user's downstream project (report, notebook, tests). Optional; defaults to CWD. |
| **Worktree** | A detached `git worktree` of the subject repo at one commit; disposable. |
| **Env dir** | A per-commit directory where `setup` installs the built dependency (e.g., an R library path, a virtualenv). Cacheable. |
| **Hook** | A user command string executed through the shell with the bisectrunk env-var contract. |
| **Oracle** | The mechanism that classifies a commit: `exit` (run hook's exit code) or `compare` (artifact vs. baseline). |
| **Classification** | One of `good`, `bad`, `skip`, `abort` per commit. |
| **Strategy** | How commits are chosen for evaluation: `bisect` (*k*-section) or `scan` (map over range). |

## 4. CLI surface

Binary name: `bisectrunk`. Global flags before the subcommand; a `bisectrunk.toml`
config file can express everything the flags can (flags override config).

```
bisectrunk bisect   Find the first bad commit via parallel k-section search
bisectrunk scan     Evaluate every commit in a range (or a stride/sample) in parallel
bisectrunk run      Evaluate a single commit: for developing/debugging hooks
bisectrunk resume   Continue an interrupted run from its run directory
bisectrunk report   Re-render report.md / report.json from a run directory
bisectrunk clean    Remove worktrees/envs of a run; --cache also clears mirrors
```

### 4.1 Shared options (bisect / scan / run)

| Flag | Meaning | Default |
|---|---|---|
| `--repo <url-or-path>` | Subject repo: URL, local path, or `.` | required |
| `--project <dir>` | Downstream project directory (CWD of `run` hook) | CWD |
| `--setup <cmd>` | Hook: build/install subject at `$BISECTRUNK_COMMIT` into `$BISECTRUNK_ENV` | none (skip phase) |
| `--run <cmd>` | Hook: execute workload; exit code classifies commit | required |
| `--oracle <exit\|compare>` | Classification mechanism | `exit` |
| `--baseline <file>` | Known-good artifact (oracle=compare) | --- |
| `--artifact <relpath>` | Artifact the run hook writes under `$BISECTRUNK_OUT` | --- |
| `--compare <cmd>` | Custom compare hook (exit 0 = match/good, 1 = differ/bad, 125 = skip) | builtin byte/text compare |
| `--jobs <N>` | Parallel workers | logical cores, capped at 8 |
| `--retries <N>` | Re-run a `bad`/`skip` classification N times to defeat flakiness | 0 |
| `--timeout <dur>` | Per-evaluation wall-clock limit (e.g., `20m`); timeout -> `skip` | none |
| `--first-parent` | Restrict history walk to first-parent chain | off |
| `--paths <p>...` | Only consider commits touching these paths in the subject repo | --- |
| `--run-dir <dir>` | Run directory location | `./bisectrunk-runs/<ts>-<id>` |
| `--cache-dir <dir>` | Mirror + env cache root | XDG cache (`~/.cache/bisectrunk`) |
| `--keep <all\|failed\|none>` | Retention of worktrees/env dirs after run | `failed` |
| `--env KEY=VAL`... | Extra env vars passed to all hooks | --- |
| `--shell <path>` | Shell used to execute hooks | `sh -c` (Unix), `cmd /C` (Windows) |
| `--format <auto\|json\|plain>` | Terminal output mode | `auto` |

### 4.2 `bisect`-specific

| Flag | Meaning | Default |
|---|---|---|
| `--good <rev>` | Known-good boundary (exclusive) | required |
| `--bad <rev>` | Known-bad boundary (inclusive) | `HEAD` |
| `--terms <old,new>` | Alternate vocabulary (e.g., `old,new`, `fast,slow`) | `good,bad` |
| `--verify-endpoints` / `--no-verify-endpoints` | Evaluate both boundaries first and abort if the oracle disagrees | on |
| `--on-inconsistent <abort\|leftmost\|retry>` | Policy when results are non-monotonic | `abort` |

### 4.3 `scan`-specific

| Flag | Meaning | Default |
|---|---|---|
| `--range <A..B>` | Commit range (git rev-list semantics) | required |
| `--stride <N>` | Evaluate every Nth commit | 1 |
| `--sample <N>` | Evaluate N evenly spaced commits | --- |
| `--stop-on-first-bad` | Terminate early once the earliest bad is bracketed | off |

### 4.4 Example invocations

R dependency, exit-code oracle (testthat script exits nonzero on failure):

```bash
bisectrunk bisect \
  --repo https://github.com/tidyverse/dplyr \
  --good v1.1.0 --bad main \
  --setup 'R CMD INSTALL --library="$BISECTRUNK_ENV" "$BISECTRUNK_WORKTREE"' \
  --run   'R_LIBS_USER="$BISECTRUNK_ENV" Rscript test.R' \
  --jobs 8
```

Rendered-report drift, compare oracle:

```bash
bisectrunk bisect \
  --repo https://github.com/some/dep --good 2024-01-01 --bad HEAD \
  --setup 'uv pip install --python "$BISECTRUNK_ENV/bin/python" "$BISECTRUNK_WORKTREE"' \
  --run   '"$BISECTRUNK_ENV/bin/python" -m jupyter nbconvert --execute report.ipynb --to html --output "$BISECTRUNK_OUT/report.html"' \
  --oracle compare --baseline golden/report.html --artifact report.html
```

Bisecting the subject repo itself (classic `git bisect run`, parallelized):

```bash
bisectrunk bisect --repo . --good v2.3.0 --bad HEAD --run 'cargo test -q'
```

When `--project` is not given and `--repo` is the current repository, the run hook's
CWD is the worktree itself: the classic case falls out of the general one.

### 4.5 Config file

`bisectrunk.toml` in the project dir (or via `--config`) mirrors the CLI:

```toml
[subject]
repo = "https://github.com/tidyverse/dplyr"
first_parent = true
paths = ["R/", "src/"]

[hooks]
setup = 'R CMD INSTALL --library="$BISECTRUNK_ENV" "$BISECTRUNK_WORKTREE"'
run   = 'R_LIBS_USER="$BISECTRUNK_ENV" Rscript test.R'

[oracle]
kind = "compare"
baseline = "golden/report.html"
artifact = "report.html"
normalize = ["\\b\\d{4}-\\d{2}-\\d{2}[ T]\\d{2}:\\d{2}:\\d{2}\\b"]  # regexes stripped before text compare

[execution]
jobs = 8
timeout = "20m"
retries = 1

[[pins]] # Optional: companion deps installed once at a fixed rev,
repo = "https://github.com/r-lib/vctrs" # Shared read-only across all evaluations
rev = "v0.6.5"
setup = 'R CMD INSTALL --library="$BISECTRUNK_PIN_ENV" "$BISECTRUNK_PIN_WORKTREE"'
```

`[[pins]]` covers "one moving dependency, others frozen." A full cartesian matrix
across multiple moving repos is explicitly out of scope for v1 (see §12).

## 5. Hook contract

Hooks run through the shell with CWD = project dir (or worktree in self-bisect mode)
and receive:

| Variable | Value |
|---|---|
| `BISECTRUNK_COMMIT` | Full SHA under evaluation |
| `BISECTRUNK_COMMIT_SHORT` | Abbreviated SHA |
| `BISECTRUNK_WORKTREE` | Path to the subject checkout at that commit |
| `BISECTRUNK_ENV` | Per-commit environment dir (created empty or restored from cache) |
| `BISECTRUNK_OUT` | Per-commit output dir for artifacts |
| `BISECTRUNK_PROJECT` | Project dir |
| `BISECTRUNK_JOB` | Worker index (0-based): useful for port allocation etc. |
| `BISECTRUNK_RUN_DIR` | The run directory |
| `BISECTRUNK_PIN_ENVS` | `PATH`-style list of pinned-dependency env dirs (if pins configured) |

**Exit-code protocol** (identical for `setup` and `run`, matching `git bisect run`):

| Exit code | Meaning |
|---|---|
| `0` | good (for `setup`: proceed to `run`) |
| `125` | skip: commit untestable (build broken, etc.) |
| `1--124`, `126`, `127` | bad (for `setup`: configurable --- default `skip`, since a failed install usually means "untestable," mirroring bisectr's `on_error = "skip"`; `--setup-failure bad` opts into treating install failure as the regression itself) |
| `>=128` | abort the entire run (also produced by signals) |

Timeout maps to `skip`. All hook stdout/stderr is captured to
`logs/<sha>/{setup,run}.log`, never interleaved on the terminal.

## 6. Oracles

**`exit` (default).** The run hook's exit code is the classification. Zero-friction
migration from `git bisect run`.

**`compare`.** The run hook must produce `$BISECTRUNK_OUT/<artifact>`.
Classification: artifact matches baseline -> good; differs -> bad;
missing -> skip. Builtin comparison is byte-equality,
with a `text` mode that normalizes line endings and strips
config-supplied regexes (timestamps, session info, absolute paths:
the usual false-positive sources in rendered reports).
`--compare <cmd>` swaps in a user command (receives `BISECTRUNK_BASELINE` and `BISECTRUNK_CANDIDATE`) for structural diffs (HTML DOM, image perceptual hash,
numeric tolerance).

## 7. Search strategies

### 7.1 `bisect`: parallel *k*-section

Let the candidate list be the ordered commits strictly between the good and bad
boundaries (after `--first-parent` / `--paths` filtering).

1. **Endpoint verification** (default on): evaluate `--good` and `--bad` in parallel;
   abort with a clear message if the oracle contradicts the user's claim. This is the
   single most valuable guardrail against wasted compute.
2. Each round, pick `k = min(jobs, remaining)` interior probe points that split the
   current interval into k + 1 near-equal parts; evaluate them concurrently.
3. Classify: find the leftmost `bad` probe; the new interval is (rightmost `good`
   probe left of it, that probe]. Skipped probes are replaced by nearest untested
   neighbors (git's skip semantics); if an interval collapses to only-skips, report
   the candidate *set* rather than a single commit.
4. Monotonicity check: a `good` probe to the right of a `bad` probe within one round
   triggers `--on-inconsistent` (default `abort` with a suggestion to use `scan`,
   which reveals multiple transitions/flakiness; `retry` re-evaluates the
   contradictory probes; `leftmost` proceeds heuristically).
5. Terminate when the interval contains one commit -> **first bad commit**, reported
   with its metadata, the last good commit, and links to logs/artifacts.

Round complexity is ⌈log₍ₖ₊₁₎ n⌉, so 8 workers turn a 5000-commit range into ~4
rounds instead of ~13 sequential steps. Evaluations completed in any round are
cached, so overlapping probes across rounds are free.

### 7.2 `scan`: parallel map

Evaluates all (or stride/sampled) commits in the range through the same worker pool;
report lists every classification and every *transition* (good -> bad and bad -> good
boundaries). This is the tool for suspected non-monotone behavior, for building a
behavior timeline, and for the "just show me everything" workflow the original R
prototype approximated. `--sample N` then `bisect` on the bracketed interval is the
recommended two-phase pattern for very large ranges, and the report suggests the
exact follow-up command.

## 8. Git layer

- **History (read) operations** use the `git2` crate (vendored libgit2): rev
  parsing, rev-list ordering, merge-base, first-parent walks, path-touch
  filtering, commit metadata. Pure in-process, no parsing of porcelain output.
- **Network and worktree operations** shell out to the `git` CLI via `xshell`:
  `clone --mirror`, `fetch`, `worktree add --detach`, `worktree remove`.
  Rationale: the user's existing credential helpers, SSH agents, and proxy
  config work unmodified (libgit2 auth is a notorious tar pit), and worktree
  behavior matches what users can inspect by hand. This follows the
  "orchestrate real tools" philosophy.
- One **mirror clone per subject repo** lives in the cache
  (`<cache>/repos/<sha256(url)>`); worktrees attach to it, so N workers share
  one object store and per-commit checkout is cheap.

## 9. Caching and the run directory

**Evaluation cache key:** `(repo-url, commit-sha, hash(setup-cmd), hash(run-cmd or compare config), pins-hash)`.
Cached entries store the classification, exit code, duration, and log/artifact
paths. `--no-cache` bypasses reads; `clean --cache` purges.

**Env-dir cache:** keyed by `(repo, commit, setup-hash)`: a re-bisect with a
modified run hook but unchanged setup reuses every installed environment,
which is where the wall-clock time actually goes.

**Run directory layout:**

```
bisectrunk-runs/20260713-142233-a1b2c3d/
├── run.toml          # Fully resolved config (flags + file merged)
├── state.json        # Append-friendly evaluation ledger; source of truth for resume
├── logs/<sha>/       # setup.log, run.log per evaluated commit
├── out/<sha>/        # Artifacts (compare oracle)
├── report.json       # Machine-readable result
└── report.md         # Human-readable summary with per-round narrative
```

`resume` reloads `run.toml` + `state.json` and continues; Ctrl-C (via a `ctrlc`
handler) finishes in-flight bookkeeping, prunes worktrees, and prints the resume
command. `bisectrunk` never leaves stale worktrees registered against the mirror.

## 10. Output and UX

- `indicatif` MultiProgress: one overall bar (round r/R for bisect,
  m/n commits for scan) plus one spinner per worker showing `<short-sha> <phase>`.
- Final summary always names: first bad commit (subject, author, date, title),
  last good commit, evaluation count vs. range size (the "saved you X runs" line),
  wall-clock, and the paths to logs and reports.
- `--format json` streams JSON-lines events (round start, evaluation result,
  conclusion) for CI consumption; plain mode for dumb terminals.
- Process exit codes: `0` conclusive result; `2` inconclusive (all-skip interval,
  non-monotone abort); `3` endpoint verification failed; `4` aborted by hook >=128;
  `1` internal error.

## 11. Crate map and dependencies

```
src/
├── main.rs          # Thin ExitCode wrapper over lib::run()
├── lib.rs           # run(): parse -> resolve config -> dispatch subcommand
├── cli.rs           # clap derive definitions
├── config.rs        # bisectrunk.toml schema, merge with CLI, serde
├── gitrepo.rs       # git2 history ops: ranges, first-parent, path filter
├── mirror.rs        # cache-dir mirror clones, fetch, locking
├── worktree.rs      # Worktree lifecycle via xshell git CLI
├── hooks.rs         # Shell execution, env contract, log capture, timeout
├── oracle.rs        # Exit + compare classification, normalization
├── evaluate.rs      # One commit end-to-end: worktree -> setup -> run -> classify
├── scheduler.rs     # Worker pool (scoped threads + crossbeam-channel)
├── strategy/
│   ├── bisect.rs    # k-section rounds, skip resolution, monotonicity
│   └── scan.rs      # Map + transition detection
├── state.rs         # Run dir, state.json ledger, resume, eval cache
├── report.rs        # report.md / report.json rendering
├── progress.rs      # indicatif wiring
└── util.rs
```

Dependencies (beyond the ones you listed: `anyhow`, `clap` derive, `indicatif`,
`tempfile`, `xshell`, `git2` with vendored features):

| Crate | Why |
|---|---|
| `serde` + `serde_json` | state ledger, report.json, JSON-lines output |
| `toml` | config file |
| `crossbeam-channel` | job/result queues for the worker pool (std threads suffice; no async runtime: this is process orchestration, not high-concurrency I/O) |
| `ctrlc` | graceful interrupt -> cleanup + resume hint |
| `similar` | readable text diffs in compare-oracle reports |
| `blake3` | fast cache keys and baseline hashing |
| `etcetera` (or `dirs`) | XDG-correct cache directory |
| `humantime` | `--timeout 20m` parsing and duration display |
| `jiff` | timestamps in run IDs and reports |
| `console` | styling consistent with indicatif |

Deliberately excluded: `tokio`/async (thread pool of subprocesses needs no runtime),
`rayon` (scheduling is stateful and round-based, not data-parallel), `reqwest`
(git CLI handles all network).

## 12. Non-goals (v1)

- Multi-repo cartesian/matrix bisecting (cross's data-frame case): roadmap; the
  `[[pins]]` mechanism covers the common "one moving, rest frozen" instance.
- Provisioning language toolchains (installing R/Python themselves): that is
  not this tool's territory; bisectrunk assumes the toolchain exists and documents recipes.
- Distributed execution across machines: the state ledger is designed so a remote
  scheduler could be added without changing the evaluation contract.
- Interactive good/bad prompting (bisectr's `bisect_return_interactive`): v1.1
  candidate: an `--interactive` oracle that opens the artifact and prompts g/b/s.

## 13. Testing strategy

All tests run without R, Python, or network:

- **Fixture repos:** helper builds synthetic Git histories in `tempfile` dirs via
  the git CLI, embedding a "regression" at a known commit (a file whose content
  flips), including branchy/merge histories and commits that break the build
  (-> skip paths).
- **Hook fixtures** are tiny `sh` scripts reading the fixture state and exiting
  0/1/125/130: this exercises the full protocol with no ecosystem installed.
- **Unit:** k-section probe placement, interval narrowing, skip-neighbor expansion,
  monotonicity detection, cache-key stability, normalization regexes, config merge
  precedence.
- **Integration:** end-to-end bisect on fixture repos asserting the exact first-bad
  SHA; scan transition detection; resume-after-kill; endpoint-verification failure;
  `run` single-commit mode.
- **Property test (optional, `proptest`):** for random monotone classifications over
  random range sizes and worker counts, k-section always converges to the planted
  transition in <= ⌈log₍ₖ₊₁₎ n⌉ rounds.

## 14. Documentation site (Zensical)

Structure: `zensical.toml` + `docs/`.

```
nav:
  Home        index.md             # What/why, quick start, the report-drift story
  Guide       guide/install.md     # cargo install / prebuilt binaries
              guide/quickstart.md  # First bisect in 5 minutes
              guide/cli.md         # Full flag reference (kept in lockstep with clap)
              guide/hooks.md       # env-var contract + exit-code protocol
              guide/recipes.md     # R (R CMD INSTALL), Python (uv), Rust, notebooks
              guide/results.md     # Run directory, reports, resume
  Design      design/principles.md
              design/strategies.md # k-section math, scan, skip semantics
              design/isolation.md  # Mirrors, worktrees, env dirs, caching
              design/protocol.md   # Exit codes, git bisect run compatibility
              design/deps.md       # Why each crate; why no async
```

## 15. Milestones

1. **M1: evaluate one commit:** cli/config, mirror, worktree, hooks, exit oracle, `bisectrunk run`. (Everything else composes this.)
2. **M2: scan:** scheduler, state ledger, progress UI, report, resume.
3. **M3: bisect:** k-section strategy, endpoint verification, skip/monotonicity handling.
4. **M4: compare oracle:** artifacts, normalization, custom compare, diffs in reports.
5. **M5: polish:** pins, retries/timeouts, JSON output mode, docs site, CI, prebuilt release binaries.
