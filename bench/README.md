# Status-line benchmark

How long Claude Code waits for each status-line tool, and what each costs in CPU and memory, with the same visible content, the same input and the same machine.

| Tool | Language | Version | Installed as |
|---|---|---|---|
| bar | Rust | 0.0.0, commit `09432ae` | `cargo build --release --locked` from this repository |
| [ccstatusline](https://github.com/sirmalloc/ccstatusline) | TypeScript (Node) | 2.2.34 | `npm install -g ccstatusline@2.2.34` (README's "pinned global install") |
| [claude-powerline](https://github.com/Owloops/claude-powerline) | TypeScript (Node) | 1.32.8 | `npm install -g @owloops/claude-powerline@1.32.8` |
| [CCometixLine](https://github.com/Haleclipse/CCometixLine) (`ccline`) | Rust | 2.0.1 | release binary `ccline-linux-arm64.tar.gz` (sha256 `7d11331c…0fcc6`), placed at `~/.claude/ccline/ccline` as its README says |

The README invocations most people paste (`npx -y …@latest`, `bunx …`) are measured as extra rows.

## Results

All times in ms. Warm: 100 runs after 10 warm-up runs. Cold: 30 runs, the tool's whole cache/state directory removed before each. CPU is user + system time of the process Claude Code waits on; detached work is listed separately.

| Tool | Warm median | Warm mean | Warm p95 | Warm min | Cold median | Cold p95 | CPU, warm | Max RSS, warm (MiB) |
|---|---|---|---|---|---|---|---|---|
| bar | **0.3** | 0.3 | 0.4 | 0.3 | **2.5** | 3.2 | **0.36** | **3.5** |
| ccline | 0.4 | 0.4 | 0.5 | 0.4 | 6.3 | 7.1 | 0.50 | 5.3 |
| claude-powerline | 41.3 | 41.6 | 44.2 | 39.4 | 58.7 | 66.1 | 47.9 | 102.5 |
| ccstatusline | 92.0 | 94.3 | 110.0 | 89.5 | 96.9 | 100.4 | 118.9 | 115.9 |

Work left running after the render (detached jobs), and the render that starts it:

| Tool | Cold: foreground CPU | Cold: detached jobs, CPU | Stale¹: wall median | Stale¹: foreground CPU | Stale¹: detached jobs, CPU |
|---|---|---|---|---|---|
| bar | 2.4 | 2 jobs, 8.7 | 4.2 | 3.9 | 1 job, 7.8 |
| ccline | 2.7 | none (git runs inline when nothing is cached) | 5.8 | 4.2 | 1 job, 6.9 |
| claude-powerline | 74.9 | none | 84.8 | 85.0 | none |
| ccstatusline | 124.3 | none | 143.5 | 166.6 | none |

¹ Stale: the previous render was 11 s ago, past every tool's git cache lifetime (bar 10 s, ccstatusline 5 s, ccline 2 s; claude-powerline stores no git cache on disk). This is the usual case with Claude Code's `refreshInterval` or between turns. 5 runs per tool. Each run follows 11 s of idle, so it also pays for cold CPU caches; compare tools within this column rather than against the warm column.

README invocations (20 runs after 3 warm-up runs; `@latest` is resolved against the npm registry, so these depend on the network):

| Invocation | Median | Mean | p95 | Min |
|---|---|---|---|---|
| `bunx --bun @owloops/claude-powerline` | 24.0 | 24.0 | 24.8 | 22.9 |
| `bunx -y ccstatusline@latest` | 213.1 | 223.8 | 272.5 | 191.5 |
| `npx -y @owloops/claude-powerline@latest` | 326.9 | 332.0 | 359.0 | 315.6 |
| `npx -y ccstatusline@latest` | 367.8 | 372.2 | 409.2 | 353.0 |
| `node -e 0` (start-up floor for the Node tools) | 10.1 | 10.2 | 11.6 | 9.0 |
| `bun -e 0` | 0.9 | 0.9 | 1.1 | 0.8 |

claude-powerline's first run with no price table (it downloads one from GitHub and caches it for 24 h): 130 ms median over 10 runs. The cold rows above have that table seeded; see Caveats.

### Reading it

- Warm, the two Rust tools are tied in practice: 0.3 vs 0.4 ms wall, 0.36 vs 0.50 ms CPU. Both render from caches and spawn nothing. The Node tools sit at 40 to 95 ms, of which about 10 ms is Node start-up alone.
- Cold, bar's foreground is the fastest (2.5 ms) because it never runs `git` or reads the whole transcript inline: it defers both to two detached jobs that cost another 8.7 ms of CPU. ccline runs `git` inline on a cold cache (6.3 ms wall, 2.7 ms CPU: most of the wall time is waiting on `git`) and leaves nothing behind. Counting all work, bar's cold run costs more CPU in total (2.4 + 8.7 = 11.1 ms) than ccline's (2.7 ms); bar's extra is mostly the full scan of the 5 MiB transcript for token totals, which ccline does not show.
- Stale (the steady state in a session), bar and ccline behave alike: a 4 to 6 ms render that starts one detached `git status` refresh of about 7 to 8 ms CPU. bar's render costs slightly less (3.9 vs 4.2 ms CPU, 4.2 vs 5.8 ms wall); its refresh costs slightly more (7.8 vs 6.9 ms).
- Memory: bar 3.5 MiB, ccline 5.3 MiB, the Node tools 100 to 116 MiB per render.
- claude-powerline under Bun (`bunx --bun`, 24 ms) is faster than its global Node install (41 ms). ccstatusline through `bunx`/`npx` with `@latest` pays 120 to 280 ms of package resolution on every render, as its README warns; the pinned global install avoids that.

## What each tool shows

Target content, the same for every tool where it can: model, context %, git branch + dirty state, session cost, 5-hour usage, 7-day usage, session duration, token totals. Plain separators (` │ `), no powerline, no icons, no network segments (no PRs, CI, usage APIs, update checks). Rendered lines from the run, colour stripped (with escapes visible in `results/output-<tool>.txt`):

```text
bar               Opus 5.5 │ ctx 42% │ main ~1 ?1 │ $4.27 │ 37% 2h12m │ 61% 3d3h │ 1h02m │ 1.7M↑ 870k↓
ccstatusline       Model: Opus 5.5 │ Ctx Used: 42.0% │ main  *? │ Cost: $4.27 │ 5h: 37.5%  Reset: 2hr 2m │ Weekly: 61.2%  Weekly Reset: 3d 3hr 49m │ Session: 1hr 2m │ Total: 120.0M
claude-powerline   M Opus 5.5  C 42%  ~ main (~1 ?1) *  S $4.27 (199.0M tokens)  B 38% (2h 13m)  W 61% (3d 4h)  T 1.0h
ccline            Opus 5.5 · High │ 42% │ main * +1 │ $4.27 │ 38% · Sun 00:02 │ 61% · Wed 01:49 │ 1h2m
```

| Item | bar | ccstatusline | claude-powerline | ccline |
|---|---|---|---|---|
| model | `model` | `model` | `model` | `model` (also prints the effort level; no option to drop it) |
| context % | `ctx` | `context-percentage` | `context`, used %, no autocompact buffer | `context`, `{pct}%` |
| branch + dirty | `branch`, `dirty` | `git-branch` (raw value, no glyph), `git-status` | `git` with working-tree counts | `git` (also shows ahead count) |
| session cost | `cost` | `session-cost` | `session`, `costSource: official` | `cost` |
| 5h / 7d usage | `usage` pct + time left | `session-usage` + `reset-timer`, `weekly-usage` + `weekly-reset-timer` | `block`, `weekly` | `quota.5h`, `quota.7d`, text (reset as clock time) |
| duration | `duration` | `session-clock` | `metrics`, duration only | `duration` |
| token totals | `tokens` (in/out, excluding cache reads) | `tokens-total` (includes cache reads) | `session` tokens (includes cache reads) | **not available**: no session token metric (`context` tokens are the current context, not totals) |

Differences that remain: labels and separators follow each tool's own style (ccstatusline prints `Model:`/`Cost:` labels; claude-powerline's text charset keeps one-letter segment markers and uses spaces instead of `│`). Token totals mean different things: bar shows input (plus cache writes) and output; ccstatusline and claude-powerline add cache reads, hence 120 M and 199 M for the same transcript (the two also count differently). Values come from the same stdin, so cost, percentages and duration agree up to rounding.

Configs, exactly as used: [configs/](configs/). Each tool reads its config from its documented location under its own `HOME`:

| Tool | Config | State written during the run |
|---|---|---|
| bar | `~/.config/bar/bar.conf` | `~/.local/state/bar/` |
| ccstatusline | `~/.config/ccstatusline/settings.json` | `~/.cache/ccstatusline/` |
| claude-powerline | `~/.claude/claude-powerline.json` | `~/.claude/powerline/` |
| ccline | `~/.claude/ccline/config.toml` (update check off) | `~/.claude/ccline/state/` |

## Method

- Input: [fixtures/session.json](fixtures/session.json), a status-line JSON with every field the targets read (model, workspace, cost, context window, effort, thinking, `rate_limits.five_hour`/`seven_day`, version, session id, transcript path), per the [status line docs](https://code.claude.com/docs/en/statusline). `run.sh setup` rewrites `resets_at` relative to the current time (5 h window: 2 h 13 m left; 7 d window: 3 d 4 h left) so usage segments render.
- Transcript: 5.0 MiB synthetic JSONL from [fixtures/gen-transcript.mjs](fixtures/gen-transcript.mjs) (deterministic; about 1,230 turns over 62 minutes; assistant messages with `message.id`, `requestId`, `message.usage`; one response split over 1 to 3 lines with the same id, as Claude Code writes them). It sits at Claude Code's layout, `<claude dir>/projects/<cwd with / as ->/<session id>.jsonl`, and each tool's `~/.claude/projects/` links to it (claude-powerline looks transcripts up by session id rather than `transcript_path`).
- Working directory: a git repository with 4 commits, one ahead of its upstream, one modified file, one untracked file.
- Isolation: each tool runs with `HOME=/opt/slbench/home/<tool>`, so caches never cross. Tools were measured one at a time with nothing else from the benchmark running; `uptime` load was recorded before each phase ([results/load-*.txt](results/)).
- Warm: `hyperfine -N --warmup 10 --runs 100 --input session.json`.
- Cold: `hyperfine -N --runs 30 --prepare 'run.sh _reset <tool>'`. The reset waits until the previous run's detached jobs have exited, then restores the tool's `HOME` to config only. The OS page cache stays warm (dropping it needs privileges the container does not have).
- CPU and memory: 20 warm runs each under `/usr/bin/time -v` (memory: max RSS) and [lib/rusage.py](lib/rusage.py) (CPU: `wait4` rusage at microsecond resolution; GNU time prints CPU in 10 ms steps, which reads as 0 for the Rust tools). `rusage.py` makes itself a child subreaper, so jobs a tool detaches are re-parented to it and their CPU is collected too; that is the "detached" column. Max RSS from `rusage.py` is not used: `ru_maxrss` keeps the parent's high-water mark across `exec`.
- Stale: `rusage.py --runs 5 --prepare 'sleep 11'` after priming the caches.
- bar's background jobs: on a cold state a render spawns two detached `bar refresh` jobs (`git status`, and the rest of the transcript scan beyond the 1 MiB read inline); in steady state one (`git status` every 10 s). Their CPU is in the tables above; per-run data in `results/bar-bgjob-cold.json`, `results/rusage-*-bar.json`.

## Environment

- Apple Silicon host (Apple M4 Max), Docker Linux arm64 container: Arch Linux ARM, kernel 7.2, 10 vCPUs visible, no CPU quota, 31 GiB RAM, glibc 2.43. The host runs other services; load average during the runs was 0.6 to 2.8.
- Node 26.12.0, npm 12.2.1, Bun 1.4.4, Rust 1.99.1, git 2.56.0, Python 3.14.8, hyperfine 2.0.0, GNU time 1.10.
- Claude Code 2.1.319 installed in the container for the schema version; no Claude Code session ran.
- Run on 2026-11-07, `COLUMNS=200`.

## Caveats

- One machine, one container, one run. The ordering is stable but the absolute numbers are specific to this Linux VM; process start-up on macOS is slower, so expect larger absolute times there.
- The host was not idle (other services; load figures recorded). Warm medians and minimums are close, so noise is small relative to the gaps, but 1 ms-level differences between bar and ccline are within noise.
- Content is equivalent, not identical (see the table above). ccline does less work in one respect: it shows no token totals, so it never scans the transcript for them. bar defers that scan out of the render; the Node tools do it inline on every render (claude-powerline caches its session totals).
- bar's first render on a cold state shows token totals from the first 1 MiB only; the detached scan completes them within a few renders. The other tools show full totals on the first render.
- Cold for claude-powerline is measured with its downloaded price table seeded, so the row measures the tool rather than GitHub (the download case is listed separately). It is refreshed once a day.
- Default cache lifetimes were kept (bar git 10 s, ccstatusline 5 s, ccline 2 s); they are not equal. The stale scenario waits past all of them, so each tool refreshes once per run there.
- The README-invocation rows hit the npm registry, so they depend on network latency.
- In this container PID 1 does not reap orphans, so the detached jobs of runs under hyperfine and GNU time stay as zombie entries until the container restarts; this does not affect timings.
- bar is pre-1.0 and unreleased; the others are their latest releases at the time of the run.

## ben rerun

The same benchmark run a second time through [ben](https://github.com/hop-top/ben), a Go benchmark runner, with suites in [ben/](ben/). Same container, fixture, configs and per-tool `HOME`s as above; only the harness and the bar build differ.

- bar: commit `14a1001` (newer than the `09432ae` used above). Others unchanged: ccstatusline 2.2.34, claude-powerline 1.32.8, ccline 2.0.1.
- ben: commit `2b24904` (reports itself as 0.1.0), cross-compiled for linux/arm64 with `CGO_ENABLED=0`.
- Suites: `warm.yaml` (100 runs after 10 warm-up runs), `cold.yaml` (30 runs, `prepare: run.sh _reset <tool>` before each, the same reset the hyperfine cold runs used), `floor.yaml` (the harness floor, see below). Each candidate uses `args:` (no shell) and `input_file:` for stdin. ben has no per-candidate environment, so `cmd` is `/usr/bin/env` with `HOME=… COLUMNS=200 <tool> …`; `env` execs the tool, so the measured process is the tool, but env's own start-up is included (part of the floor below). Metrics `latency_ms`, `cpu_ms`, `cpu_user_ms`, `cpu_sys_ms`, `max_rss_kb`; scorer `single:latency_ms`, median.
- Load average 1.2 to 2.0 during the run ([results/ben-load-*.txt](results/)). The `metadata.host` field in the JSON was replaced with `container`.

Results, ms ([results/ben-summary.md](results/ben-summary.md), raw samples in `results/ben-{warm,cold,floor}.json`):

| Tool | Warm latency median ± sd | Warm p95 | Warm CPU median | Cold latency median ± sd | Cold p95 | Cold CPU median |
|---|---|---|---|---|---|---|
| bar | **0.84** ± 0.10 | 1.04 | **0.78** | **3.60** ± 0.15 | 3.88 | 3.44 |
| ccline | 0.94 ± 0.03 | 0.99 | 0.88 | 6.65 ± 0.83 | 7.17 | **2.75** |
| claude-powerline | 41.88 ± 3.99 | 45.99 | 48.56 | 57.43 ± 1.85 | 60.55 | 71.73 |
| ccstatusline | 91.91 ± 5.28 | 101.49 | 117.45 | 99.27 ± 8.47 | 110.01 | 124.46 |
| floor: `env` → `/usr/bin/true` | 0.61 ± 0.05 | 0.70 | 0.56 | | | |

Max RSS (median, MiB): bar 16.5, ccline 18.4, claude-powerline 102.1, ccstatusline 115.8; the floor reads 16.6. ben starts children with Go's `os/exec`, which on Linux uses a vfork-style clone, and `ru_maxrss` keeps the parent's high-water mark across `exec`. So every value includes ben's own RSS (about 16.5 MiB), and the Rust tools' real footprint (3.5 and 5.3 MiB under GNU time above) cannot be read from these numbers. The Node tools' values match GNU time to within 1 MiB.

Cross-check against the hyperfine and wait4 runs above:

- Node tools: warm and cold latency agree within 1 to 2 ms (41.9 vs 41.3, 91.9 vs 92.0 warm; 57.4 vs 58.7, 99.3 vs 96.9 cold). CPU agrees within 1 ms warm.
- Rust tools: each value carries the floor (ben spawn plus `env` exec: 0.61 ms latency, 0.56 ms CPU). Minus the floor, warm latency is about 0.23 ms for bar and 0.33 ms for ccline, against 0.3 and 0.4 under hyperfine; the order and the gap are the same. Subtracting is approximate: the floor is a separate process, not part of each run.
- bar cold: 3.60 ms here, about 3.0 ms after the floor, against 2.5 ms under hyperfine; its foreground CPU is about 2.9 ms after the floor against 2.4 ms. ccline's cold numbers did not move (6.65 vs 6.3 ms; 2.75 vs 2.7 ms CPU). The difference is likely the newer bar build (load was similar, 1.9 to 2.0 here); it was not investigated further.
- Where bar loses: on a cold state its foreground CPU (3.4 ms) is now above ccline's (2.75 ms), before counting bar's detached jobs, which ben does not measure (it counts only waited processes; the first run's tables have them: 8.7 ms on a cold state). ccline's cold latency is higher (6.65 vs 3.60 ms) because it waits on `git`, not because it uses more CPU.

Rerun with ben (inside the container, after the toolchain and tools from [Rerun](#rerun)):

```sh
# ben for linux/arm64, built from its checkout on any machine with Go
GOWORK=off CGO_ENABLED=0 GOOS=linux GOARCH=arm64 go build -o ben ./cmd/ben
# copy it to /opt/slbench/bin/ben, then:
/opt/slbench/bench/run.sh ben       # setup, render, floor + warm + cold suites, summary, removes the transcript
```

`run.sh ben` runs ben with `HOME=/opt/slbench/benhome`, so ben's run store stays out of the tools' `HOME`s. The suites take the work root from `task.input.slb` (`/opt/slbench`).

## Rerun

Inside a Linux container (root), with Node/npm, git, jq and Python 3:

```sh
SLB=/opt/slbench; mkdir -p $SLB
# toolchain
pacman -S --needed hyperfine time          # or your distro's packages
curl -sSf https://sh.rustup.rs | RUSTUP_HOME=$SLB/rustup CARGO_HOME=$SLB/cargo sh -s -- -y --profile minimal --no-modify-path
curl -fsSL https://bun.sh/install | BUN_INSTALL=$SLB/bun bash
# tools
npm install -g ccstatusline@2.2.34 @owloops/claude-powerline@1.32.8
mkdir -p $SLB/ccline/dl && cd $SLB/ccline/dl && curl -fsSLO https://github.com/Haleclipse/CCometixLine/releases/download/v2.0.1/ccline-linux-arm64.tar.gz && tar -xzf ccline-linux-arm64.tar.gz
# bar, from a checkout of this repository copied to $SLB/bar
cd $SLB/bar && PATH=$SLB/cargo/bin:$PATH RUSTUP_HOME=$SLB/rustup CARGO_HOME=$SLB/cargo cargo build --release --locked
# benchmark (results land in bench/results unless OUT is set)
$SLB/bar/bench/run.sh all
```

`run.sh` subcommands: `setup`, `render`, `bench [tool…]`, `stale [tool…]`, `runners`, `bgjob`, `all`, and `ben` (see [ben rerun](#ben-rerun)). `WARM_RUNS`, `COLD_RUNS`, `CPU_RUNS` and `OUT` override the defaults. Paths assume `SLB=/opt/slbench`; the fixture's paths are rewritten by `setup` but the generator's default `cwd` and the `PROJ` value in `run.sh` follow that root. A full run takes about 10 minutes.

## Files

- `run.sh`: the runner.
- `configs/<tool>/`: configs used.
- `fixtures/session.json`, `fixtures/gen-transcript.mjs`: input and transcript generator (the 5 MiB transcript itself is not committed).
- `ben/`: ben suites (`warm.yaml`, `cold.yaml`, `floor.yaml`) for the ben rerun.
- `lib/rusage.py`: CPU, memory and detached-job accounting; `lib/procs.py`: finds a tool's leftover jobs; `lib/summarize.py`: tables.
- `results/ben-*`: ben run JSON, load records, rendered lines and summary from the ben rerun.
- `results/`: hyperfine exports (`hyperfine-*.json`), rusage runs (`rusage-{warm,cold,stale}-*.json`), GNU time output and averages (`time-v-*`), rendered lines (`output-*`), load records (`load-*`), `summary.md`.
