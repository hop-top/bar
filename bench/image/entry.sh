#!/usr/bin/env bash
# The half of `make bench` that runs where the benchmark runs: the image's
# entrypoint, and with BENCH_RUNNER=native the script run in a prepared Linux
# environment. stdin: `git archive` of the commit to measure (with --extracted:
# none, the tree is already at $SLB/bar). Builds bar from it at $SLB/bar, runs
# `bench/run.sh ben` (ben suites, total cold CPU, GNU time), writes results and
# environment to $OUT.
set -euo pipefail
SLB=${SLB:-/opt/slbench}
OUT=${OUT:-/out}
mkdir -p "$SLB/bar" "$OUT"
[ "${1:-}" = --extracted ] || tar -x -C "$SLB/bar"
ln -sfn "$SLB/bar/bench" "$SLB/bench"   # the suites call $SLB/bench/run.sh
cd "$SLB/bar"
cargo build --release --locked --quiet
OUT=$OUT "$SLB/bench/run.sh" ben
python3 -I "$SLB/bench/lib/envinfo.py" > "$OUT/ben-env.json"
[ -z "${HOST_UID:-}" ] || chown -R "$HOST_UID:${HOST_GID:-0}" "$OUT"
