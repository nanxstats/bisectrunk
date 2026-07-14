# Protocol

The evaluator preserves the `git bisect run` classification protocol: zero is
good, 125 is skip, ordinary nonzero codes below 128 are bad, and 128 or greater
requests an abort. Setup failures default to skip because an unbuildable commit
usually says nothing about the behavior under investigation.

The compare oracle adds an artifact channel without changing hooks. A successful
run writes a relative path below `BISECTRUNK_OUT`. Missing artifacts skip;
matching artifacts are good; differing artifacts are bad. Valid UTF-8 artifacts
are compared after line-ending normalization and configured regex removal.
Custom comparators receive baseline and candidate paths and use the same exit
protocol.

Process exit codes distinguish tool-level outcomes: 0 conclusive, 2
inconclusive, 3 endpoint mismatch, 4 hook-requested abort, and 1 internal error.
