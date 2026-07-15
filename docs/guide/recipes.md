---
icon: lucide/chef-hat
---

# Recipes

## R

```bash
--setup 'R CMD INSTALL --library="$BISECTRUNK_ENV" "$BISECTRUNK_WORKTREE"' \
--run 'R_LIBS_USER="$BISECTRUNK_ENV" Rscript test.R'
```

## Python with uv

```bash
--setup 'uv venv "$BISECTRUNK_ENV" && uv pip install --python "$BISECTRUNK_ENV/bin/python" "$BISECTRUNK_WORKTREE"' \
--run '"$BISECTRUNK_ENV/bin/python" -m pytest'
```

## Rust

For self-bisection no setup is necessary:

```bash
bisectrunk bisect --repo . --good v1.0.0 --bad HEAD --run 'cargo test -q'
```

## Jupyter or R Markdown artifact comparison

Write the rendered file below `BISECTRUNK_OUT`, then compare it to a golden file:

```bash
--run 'jupyter nbconvert --execute report.ipynb --to html --output "$BISECTRUNK_OUT/report.html"' \
--oracle compare --baseline golden/report.html --artifact report.html
```

For R Markdown, the run hook can call `rmarkdown::render()` with
`output_file=file.path(Sys.getenv("BISECTRUNK_OUT"), "report.html")`.
Normalization regexes in `[oracle].normalize` can remove timestamps and absolute
paths before text comparison. Use `--compare CMD` for structural or
tolerance-aware comparisons.
