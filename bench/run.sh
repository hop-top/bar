#!/usr/bin/env bash
# Status-line benchmark runner. Runs inside a Linux container as root.
#
#   bench/run.sh setup              fixture repo, transcript, session JSON, per-tool HOMEs
#   bench/run.sh render [tool...]   print one rendered line per tool (escapes visible)
#   bench/run.sh bench [tool...]    warm + cold hyperfine, rusage; one tool at a time
#   bench/run.sh stale [tool...]    render 11 s after the previous one (refresh TTLs expired)
#   bench/run.sh runners            npx / bunx rows (README invocations), runtime floor
#   bench/run.sh bgjob              bar background refresh cost
#   bench/run.sh ben                setup, render, ben suites in bench/ben (floor, warm, cold), total cold CPU incl. detached jobs,
#                                   GNU time max RSS (bar, bar-notokens, ccline); what `make bench` runs
#   bench/run.sh all                setup through bgjob, in order (not ben)
#
# Tools: bar ccstatusline claude-powerline ccline bar-notokens (bar without #{tokens}, like-for-like with ccline)
# Env: SLB (work root, default /opt/slbench), WARM_RUNS (100), COLD_RUNS (30), CPU_RUNS (20)
set -euo pipefail

SLB=${SLB:-/opt/slbench}
BENCH=$(cd "$(dirname "$0")" && pwd)
WORK=$SLB/work
HOMES=$SLB/home
PRISTINE=$SLB/home-pristine
OUT=${OUT:-$BENCH/results}
WARM_RUNS=${WARM_RUNS:-100}
COLD_RUNS=${COLD_RUNS:-30}
CPU_RUNS=${CPU_RUNS:-20}
TOOLS_ALL=(bar ccstatusline claude-powerline ccline bar-notokens)
SID=5b0c9a4e-2d1f-4c3a-9e8b-7f6a5d4c3b2a
PROJ=-opt-slbench-work-repo
TRANSCRIPT=$SLB/work/claude/projects/$PROJ/$SID.jsonl
export COLUMNS=${COLUMNS:-200}
unset XDG_CONFIG_HOME XDG_CACHE_HOME XDG_STATE_HOME XDG_DATA_HOME CLAUDE_CONFIG_DIR CCLINE_HOME \
  CLAUDE_POWERLINE_CONFIG CLAUDE_POWERLINE_CACHE_DIR BAR_STATE_DIR

# Binaries (versions pinned at install time; see README).
BAR_BIN=${BAR_BIN:-$SLB/bar/target/release/bar}
CCLINE_SRC=${CCLINE_SRC:-$SLB/ccline/dl/ccline}

cmd_for() { # tool -> argv used by Claude Code's statusLine.command
  case $1 in
    bar|bar-notokens) echo "$BAR_BIN render" ;;
    ccstatusline) echo "$(command -v ccstatusline)" ;;
    claude-powerline) echo "$(command -v claude-powerline)" ;;
    ccline) echo "$HOMES/ccline/.claude/ccline/ccline" ;;
    *) echo "unknown tool $1" >&2; exit 2 ;;
  esac
}

# PIDs running with this tool's HOME, minus our own ancestors (hyperfine, rusage.py, this script):
# the detached refresh jobs a render left behind.
tool_procs() { python3 -I "$BENCH/lib/procs.py" "$HOMES/$1"; }

# One invocation under the subreaper (detached jobs get reaped, no zombies left behind).
reaped() { local t=$1 o=$2; shift 2
  # shellcheck disable=SC2046
  HOME=$HOMES/$t python3 -I "$BENCH/lib/rusage.py" --runs 1 --input "$WORK/session.json" --stdout "$o" -- $(cmd_for "$t") > /dev/null
}

load() { echo "load: $(cut -d' ' -f1-3 /proc/loadavg)  ($(uptime))"; }

setup() {
  mkdir -p "$WORK" "$OUT"
  # Git repo: 3 commits pushed, 1 local commit ahead of upstream, 1 modified file, 1 untracked file.
  rm -rf "$WORK/origin.git" "$WORK/repo"
  git init -q --bare -b main "$WORK/origin.git"
  git init -q -b main "$WORK/repo"
  (
    cd "$WORK/repo"
    git config user.name bench; git config user.email bench@example.invalid
    git config commit.gpgsign false
    git remote add origin "$WORK/origin.git"
    printf 'fn main() {}\n' > main.rs; git add main.rs; git commit -qm 'feat: init'
    printf '# demo\n' > README.md; git add README.md; git commit -qm 'docs: readme'
    printf 'pub fn add(a: i32, b: i32) -> i32 { a + b }\n' > lib.rs; git add lib.rs; git commit -qm 'feat: add'
    git push -q -u origin main
    printf 'pub fn sub(a: i32, b: i32) -> i32 { a - b }\n' >> lib.rs; git commit -qam 'feat: sub'
    printf 'fn main() { println!("hi"); }\n' > main.rs
    printf 'scratch\n' > notes.txt
    git status -sb | head -3
  )
  # Transcript: ~5 MiB synthetic JSONL ending 2 minutes ago, at Claude Code's layout
  # (<claude dir>/projects/<cwd with / as ->/<session_id>.jsonl).
  rm -rf "$WORK/claude"; mkdir -p "$(dirname "$TRANSCRIPT")"
  node "$BENCH/fixtures/gen-transcript.mjs" "$TRANSCRIPT" 5242880 "$(( $(date +%s) - 120 ))" "$WORK/repo" "$SID"
  # Session JSON: resets_at relative to now so usage windows are live (5h: 2h13m left, 7d: 3d4h left).
  local now; now=$(date +%s)
  jq --argjson f $(( now + 7980 )) --argjson s $(( now + 273600 )) \
     --arg t "$TRANSCRIPT" --arg d "$WORK/repo" \
     '.rate_limits.five_hour.resets_at=$f | .rate_limits.seven_day.resets_at=$s
      | .transcript_path=$t | .cwd=$d | .workspace.current_dir=$d | .workspace.project_dir=$d' \
     "$BENCH/fixtures/session.json" > "$WORK/session.json"
  # Per-tool HOME with the config at its documented location.
  rm -rf "$PRISTINE" "$HOMES"
  mkdir -p "$PRISTINE"/{bar/.config/bar,bar-notokens/.config/bar,ccstatusline/.config/ccstatusline,claude-powerline/.claude,ccline/.claude/ccline}
  cp "$BENCH/configs/bar/bar.conf" "$PRISTINE/bar/.config/bar/bar.conf"
  cp "$BENCH/configs/bar-notokens/bar.conf" "$PRISTINE/bar-notokens/.config/bar/bar.conf"
  cp "$BENCH/configs/ccstatusline/settings.json" "$PRISTINE/ccstatusline/.config/ccstatusline/settings.json"
  cp "$BENCH/configs/claude-powerline/claude-powerline.json" "$PRISTINE/claude-powerline/.claude/claude-powerline.json"
  cp "$BENCH/configs/ccline/config.toml" "$PRISTINE/ccline/.claude/ccline/config.toml"
  install -m 0755 "$CCLINE_SRC" "$PRISTINE/ccline/.claude/ccline/ccline"
  # Each HOME's ~/.claude/projects links to the one transcript (tools that look it up by session id find it).
  local t
  for t in "${TOOLS_ALL[@]}"; do
    mkdir -p "$PRISTINE/$t/.claude/projects/$PROJ"
    ln -s "$TRANSCRIPT" "$PRISTINE/$t/.claude/projects/$PROJ/$SID.jsonl"
  done
  # claude-powerline downloads a model price table (GitHub, cached 24 h) for its session segment.
  # Seed it so cold runs measure the tool, not the download (the download is measured once in `bgjob`).
  local tmp; tmp=$(mktemp -d)
  cp -a "$PRISTINE/claude-powerline/." "$tmp/"
  HOME=$tmp python3 -I "$BENCH/lib/rusage.py" --runs 1 --input "$WORK/session.json" -- claude-powerline > /dev/null
  mkdir -p "$PRISTINE/claude-powerline/.claude/powerline/usage"
  cp "$tmp/.claude/powerline/usage/pricing.json" "$PRISTINE/claude-powerline/.claude/powerline/usage/"
  rm -rf "$tmp"
  cp -a "$PRISTINE" "$HOMES"
}

# Restore a tool's HOME to config-only (drops every cache/state file), after its jobs finish.
reset_home() {
  local t=$1
  for _ in $(seq 1 3000); do [ -z "$(tool_procs "$t")" ] && break; sleep 0.01; done
  rm -rf "${HOMES:?}/$t" && cp -a "$PRISTINE/$t" "$HOMES/$t"
}

render() { # RENDER_PREFIX: output file prefix (default output-)
  local t p=${RENDER_PREFIX:-output-}
  for t in "${@:-${TOOLS_ALL[@]}}"; do
    reset_home "$t"; reaped "$t" /dev/null; sleep 2     # prime caches
    reaped "$t" "$OUT/$p$t.raw"
    {
      echo "# $t: $(cmd_for "$t")  (COLUMNS=$COLUMNS)"
      echo "## escapes visible (cat -v)"; cat -v "$OUT/$p$t.raw"; echo
      echo "## colour stripped"; sed -E 's/\x1b\[[0-9;:]*m//g; s/\x1b\]8;[^\x1b\x07]*(\x1b\\|\x07)//g' "$OUT/$p$t.raw"; echo
    } > "$OUT/$p$t.txt"
    cat "$OUT/$p$t.txt"
  done
}

# Warm max RSS: reset, prime caches, then CPU_RUNS runs under GNU time -v (appended to FILE).
time_v_warm() { local t=$1 f=$2 l=$3 cmd; cmd=$(cmd_for "$t")
  reset_home "$t"
  for _ in 1 2 3; do reaped "$t" /dev/null; sleep 1; done
  sleep 3; load | tee -a "$l"
  : > "$f"
  for _ in $(seq 1 "$CPU_RUNS"); do
    # shellcheck disable=SC2086
    HOME=$HOMES/$t /usr/bin/time -v -a -o "$f" $cmd < "$WORK/session.json" > /dev/null 2>/dev/null
  done
}

bench_tool() {
  local t=$1 cmd; cmd=$(cmd_for "$t")
  echo "=== $t: $cmd"
  reset_home "$t"
  load | tee "$OUT/load-$t.txt"
  # Warm: caches populated by warmup runs.
  HOME=$HOMES/$t hyperfine -N --warmup 10 --runs "$WARM_RUNS" --input "$WORK/session.json" \
    --command-name "$t warm" --export-json "$OUT/hyperfine-warm-$t.json" "$cmd"
  sleep 3; load | tee -a "$OUT/load-$t.txt"
  # Cold: tool HOME restored to config-only before each run (OS page cache stays warm).
  HOME=$HOMES/$t hyperfine -N --runs "$COLD_RUNS" --input "$WORK/session.json" \
    --prepare "$BENCH/run.sh _reset $t" \
    --command-name "$t cold" --export-json "$OUT/hyperfine-cold-$t.json" "$cmd"
  # Prime caches, then CPU/RSS over warm runs: GNU time -v (raw) and wait4 rusage (µs resolution, + detached jobs).
  time_v_warm "$t" "$OUT/time-v-$t.txt" "$OUT/load-$t.txt"
  # shellcheck disable=SC2086
  HOME=$HOMES/$t python3 -I "$BENCH/lib/rusage.py" --runs "$CPU_RUNS" --input "$WORK/session.json" \
    --json "$OUT/rusage-warm-$t.json" -- $cmd
  # shellcheck disable=SC2086
  HOME=$HOMES/$t python3 -I "$BENCH/lib/rusage.py" --runs 10 --input "$WORK/session.json" \
    --prepare "$BENCH/run.sh _reset $t" --json "$OUT/rusage-cold-$t.json" -- $cmd
  python3 -I "$BENCH/lib/summarize.py" time-v "$OUT/time-v-$t.txt" > "$OUT/time-v-$t.json"
}

stale() {
  # Steady state in a session: the previous render was 11 s ago, past every tool's refresh TTL
  # (bar git 10 s, ccstatusline git 5 s, ccline git 2 s). Captures the foreground render and the
  # detached refresh jobs it starts.
  local t
  for t in "${@:-${TOOLS_ALL[@]}}"; do
    echo "=== $t stale"; reset_home "$t"
    for _ in 1 2; do reaped "$t" /dev/null; sleep 1; done
    load | tee "$OUT/load-stale-$t.txt"
    # shellcheck disable=SC2046
    HOME=$HOMES/$t python3 -I "$BENCH/lib/rusage.py" --runs 5 --input "$WORK/session.json" \
      --prepare "sleep 11" --json "$OUT/rusage-stale-$t.json" -- $(cmd_for "$t")
  done
}

runners() {
  # README invocations most users paste. Network: npx/bunx resolve @latest against the registry.
  local rows=(
    "ccstatusline|npx -y ccstatusline@latest"
    "ccstatusline|$SLB/bun/bin/bunx -y ccstatusline@latest"
    "claude-powerline|npx -y @owloops/claude-powerline@latest"
    "claude-powerline|$SLB/bun/bin/bunx --bun @owloops/claude-powerline"
  )
  # Runtime start-up floor, for context.
  hyperfine -N --warmup 5 --runs 50 --command-name "node -e 0 (runtime floor)" \
    --export-json "$OUT/hyperfine-runner-floor-node.json" "node -e 0"
  hyperfine -N --warmup 5 --runs 50 --command-name "bun -e 0 (runtime floor)" \
    --export-json "$OUT/hyperfine-runner-floor-bun.json" "$SLB/bun/bin/bun -e 0"
  local r t c n i=0
  for r in "${rows[@]}"; do
    t=${r%%|*}; c=${r#*|}; i=$((i + 1))
    n=$(echo "$c" | awk '{print $1}' | xargs basename)
    echo "=== $t via $n"; load | tee "$OUT/load-runner-$i.txt"
    HOME=$HOMES/$t PATH=$SLB/bun/bin:$PATH hyperfine -N --warmup 3 --runs 20 --input "$WORK/session.json" \
      --command-name "$t ($n)" --export-json "$OUT/hyperfine-runner-$t-$n.json" "$c"
    # shellcheck disable=SC2086
    HOME=$HOMES/$t PATH=$SLB/bun/bin:$PATH $c < "$WORK/session.json" 2>&1 \
      | sed -E 's/\x1b\[[0-9;:]*m//g' > "$OUT/output-runner-$t-$n.txt"
  done
}

bgjob() {
  # bar: render once on a cold state dir, then collect the detached `bar refresh` job(s) it spawned.
  reset_home bar; load
  HOME=$HOMES/bar python3 -I "$BENCH/lib/rusage.py" --runs 10 --input "$WORK/session.json" \
    --prepare "$BENCH/run.sh _reset bar" --json "$OUT/bar-bgjob-cold.json" -- "$BAR_BIN" render
  find "$HOMES/bar" -type f | sed "s|$HOMES/bar|~|" | sort > "$OUT/bar-state-files.txt"
  # claude-powerline first run with no price table (downloads it from GitHub; network-bound).
  local tmp; tmp=$(mktemp -d); cp -a "$PRISTINE/claude-powerline/." "$tmp/"
  HOME=$tmp hyperfine -N --runs 10 --input "$WORK/session.json" \
    --prepare "rm -f $tmp/.claude/powerline/usage/pricing.json $tmp/.claude/powerline/usage/totals-session-$SID.json" \
    --command-name "claude-powerline cold, no price table" --export-json "$OUT/hyperfine-powerline-pricing-download.json" \
    "$(command -v claude-powerline)"
  rm -rf "$tmp"
}

mkdir -p "$OUT"
run_ben() {
  # ben (Go benchmark runner) at $BEN; its own run store goes under $SLB/benhome.
  local BEN=${BEN:-$SLB/bin/ben} s
  setup; RENDER_PREFIX=ben-output- render; rm -f "$OUT"/ben-output-*.raw
  for s in floor warm cold; do
    load | tee "$OUT/ben-load-$s.txt"
    HOME=$SLB/benhome "$BEN" run --suite "$BENCH/ben/$s.yaml" --format json > "$OUT/ben-$s.json"
    load | tee -a "$OUT/ben-load-$s.txt"
  done
  # Cold foreground + detached-job CPU (ben counts only waited processes), 30 runs per tool.
  local t
  for t in "${TOOLS_ALL[@]}"; do
    # shellcheck disable=SC2046
    HOME=$HOMES/$t python3 -I "$BENCH/lib/rusage.py" --runs "$COLD_RUNS" --input "$WORK/session.json" \
      --prepare "$BENCH/run.sh _reset $t" --json "$OUT/ben-total-cold-$t.json" -- $(cmd_for "$t") > /dev/null
  done
  # Max RSS of the tools whose footprint is below ben's own (ben's ru_maxrss includes it).
  for t in bar bar-notokens ccline; do
    time_v_warm "$t" "$OUT/ben-time-v-$t.txt" "$OUT/ben-load-time-v.txt"
  done
  # ben records the machine's host name; the run is identified by the container, not the host.
  for s in floor warm cold; do
    jq '.metadata.host = "container"' "$OUT/ben-$s.json" > "$OUT/ben-$s.json.tmp" && mv "$OUT/ben-$s.json.tmp" "$OUT/ben-$s.json"
  done
  python3 -I "$BENCH/lib/summarize.py" ben "$OUT" | tee "$OUT/ben-summary.md"
  rm -rf "$WORK/claude"   # the 5 MiB transcript is regenerated by setup
}

case ${1:-all} in
  setup) setup ;;
  render) shift; render "$@" ;;
  bench) shift; for t in "${@:-${TOOLS_ALL[@]}}"; do bench_tool "$t"; done
         python3 -I "$BENCH/lib/summarize.py" table "$OUT" | tee "$OUT/summary.md" ;;
  runners) runners ;;
  stale) shift; stale "$@" ;;
  bgjob) bgjob ;;
  _reset) reset_home "$2" ;;
  ben) run_ben ;;
  all) setup; render; for t in "${TOOLS_ALL[@]}"; do bench_tool "$t"; done; stale; runners; bgjob
       python3 -I "$BENCH/lib/summarize.py" table "$OUT" | tee "$OUT/summary.md" ;;
  *) sed -n '2,17p' "$0"; exit 2 ;;
esac
