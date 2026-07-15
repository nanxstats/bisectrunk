# Design principles

1. The core is language-agnostic; shell hooks own ecosystem knowledge.
2. Exit codes remain compatible with `git bisect run`.
3. Parallelism is part of each strategy, not a wrapper around sequential bisect.
4. Durable state makes every evaluation inspectable, cacheable, and resumable.
5. Detached worktrees and isolated environments leave the project untouched.
6. Git is the only runtime dependency of the binary; there is no async runtime
   or embedded HTTP client.

These constraints keep the evaluator useful for reports, tests, builds, and
benchmarks across R, Python, Rust, notebooks, and tools not anticipated by the
authors.
