#!/usr/bin/env bash
# `make bench` inside the image. stdin: `git archive` of the commit to measure.
# Builds bar from it at /opt/slbench/bar, runs `bench/run.sh ben` (ben suites,
# total cold CPU, GNU time), writes results and environment to /out.
set -euo pipefail
SLB=/opt/slbench
mkdir -p "$SLB/bar" /out
tar -x -C "$SLB/bar"
ln -sfn "$SLB/bar/bench" "$SLB/bench"   # the suites call $SLB/bench/run.sh
cd "$SLB/bar"
cargo build --release --locked --quiet
OUT=/out "$SLB/bench/run.sh" ben
python3 -I "$SLB/bench/lib/envinfo.py" > /out/ben-env.json
chown -R "${HOST_UID:-0}:${HOST_GID:-0}" /out
