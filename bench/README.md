# Status-line benchmark

How long Claude Code waits for each status-line tool, and what each costs in CPU and memory, with the same visible content, the same input and the same machine.

<!-- bench:tools:start -->
| Tool | Language | Version | Installed as |
|---|---|---|---|
| bar | Rust | commit `d64182c` | `cargo build --release --locked` from this repository |
| [ccstatusline](https://github.com/sirmalloc/ccstatusline) | TypeScript (Node) | 2.2.34 | `npm install -g`, as found in the environment |
| [claude-powerline](https://github.com/Owloops/claude-powerline) | TypeScript (Node) | 1.32.8 | `npm install -g @owloops/claude-powerline`, as found in the environment |
| [CCometixLine](https://github.com/Haleclipse/CCometixLine) (`ccline`) | Rust | 2.0.1 | release binary `ccline-linux-arm64.tar.gz` (as found in the environment), placed at `~/.claude/ccline/ccline` as its README says |
<!-- bench:tools:end -->

Harness: [ben](https://github.com/hop-top/ben), a Go benchmark runner, with the suites in [ben/](ben/). Every number below the tool table up to [Earlier run](#earlier-run-hyperfine-and-wait4) comes from one `make bench` run and is written by it ([Running](#running)).

## ben results

<!-- bench:results:start -->
bar `d64182c`, run on 2026-11-10; benchmark inputs `sha256:58a7eb8c46b7…` ([scripts/bench-inputs](../scripts/bench-inputs)). Warm: 100 runs after 10 warm-up runs. Cold: 30 runs, the tool's whole `HOME` reset to config only before each. Load average 1.8 to 2.2 during the run ([results/ben-load-*.txt](results/)).

| Tool | Warm latency median ± sd | Warm p95 | Warm CPU median | Cold latency median ± sd | Cold p95 | Cold CPU median |
|---|---|---|---|---|---|---|
| bar | 0.83 ± 0.38 | 0.98 | 0.77 | 1.88 ± 0.07 | 2.05 | 1.71 |
| bar-notokens | **0.80** ± 0.05 | 0.91 | **0.75** | **1.03** ± 0.10 | 1.21 | **0.91** |
| ccline | 0.92 ± 0.03 | 0.98 | 0.87 | 6.55 ± 0.78 | 7.13 | 2.89 |
| claude-powerline | 41.53 ± 5.25 | 48.88 | 48.18 | 57.48 ± 1.40 | 59.74 | 73.83 |
| ccstatusline | 91.97 ± 3.22 | 99.04 | 117.47 | 96.99 ± 1.84 | 99.70 | 123.00 |
| floor: `env` → `/usr/bin/true` | 0.63 ± 0.10 | 0.77 | 0.58 |  |  |  |

Max RSS, ben, warm median (MiB): bar 16.7, bar-notokens 18.2, ccline 18.2, claude-powerline 102.4, ccstatusline 115.9; the floor reads 17.0. GNU time, mean of 20 warm runs ([results/ben-time-v-*.txt](results/)): bar 3.8, bar-notokens 3.9, ccline 5.0 MiB.

Total cold CPU per render, foreground plus detached jobs, `rusage.py`, 30 runs each (`results/ben-total-cold-<tool>.json`, medians, ms): bar 5.6 (1.2 foreground, 4.3 in 2 jobs), bar-notokens 1.7 (0.4 foreground, 1.3 in 1 job), ccline 2.1, claude-powerline 71.4, ccstatusline 123.9.
<!-- bench:results:end -->

How to read it:

- ben counts only the processes it waits for. Each value for the Rust tools carries the floor row: ben's spawn plus the `env` exec that sets `HOME`. Subtracting the floor gives the tool's own cost approximately (the floor is a separate process, not part of each run).
- ben starts children with Go's `os/exec`, which on Linux uses a vfork-style clone, and `ru_maxrss` keeps the parent's high-water mark across `exec`. Every ben RSS value therefore includes ben's own (the floor's RSS), so the Rust tools' real footprint comes from GNU time instead. The Node tools' footprint dwarfs ben's, so their ben values stand.
- Detached jobs are not in ben's numbers; the total cold CPU line adds them. On a cold state bar renders without running `git` or reading more than 1 MiB of the transcript inline, and leaves two detached `bar refresh` jobs: `git status`, and the rest of the transcript scan for token totals. bar-notokens leaves only the `git status` job. ccline runs `git` inline on a cold cache and leaves nothing behind. After the first render bar reads only appended transcript bytes.

### Like-for-like: bar without token totals vs ccline

ccline has no session token metric, so the default bar layout (which includes `#{tokens}`) does extra work ccline never does: one 5 MiB transcript scan, done by a detached job. `bar-notokens` ([configs/bar-notokens/bar.conf](configs/bar-notokens/bar.conf)) is the same layout minus ` │ #{tokens}`, so its segments match ccline's: model, context %, branch + dirty, cost, 5-hour usage, 7-day usage, duration. Same binary, own `HOME`, measured in the same suites as the rows above. Rendered lines, colour stripped ([results/ben-output-*.txt](results/)), then ben suites and `rusage.py`, ms unless noted:

<!-- bench:like:start -->
```text
bar-notokens  Opus 5.5 │ ctx 42% │ main ~1 ?1 │ $4.27 │ 37% 2h12m │ 61% 3d3h │ 1h02m
ccline        Opus 5.5 · High │ 42% │ main * +1 │ $4.27 │ 38% · 13:20 │ 61% · Fri 15:07 │ 1h2m
```

| Metric | bar (with tokens) | bar-notokens | ccline |
|---|---|---|---|
| Warm latency median | 0.83 | **0.80** | 0.92 |
| Warm CPU median | 0.77 | **0.75** | 0.87 |
| Cold latency median | 1.88 | **1.03** | 6.55 |
| Cold CPU median (render only) | 1.71 | **0.91** | 2.89 |
| Cold total CPU incl. detached jobs, median | 5.6 (1.2 + 4.3 in 2 jobs) | **1.7 (0.4 + 1.3 in 1 job)** | 2.1 (none) |
| Warm max RSS, GNU time (MiB) | **3.8** | 3.9 | 5.0 |

bar-notokens is lower than ccline on 6 of 6 metrics; bar is lower than ccline on 5 of 6 metrics (not on: cold total CPU incl. detached jobs, median).
<!-- bench:like:end -->

Same seven values; only style differs (ccline adds the effort level, has no `ctx` label and shows resets as clock times). Where the two differ by mechanism:

- Cold latency: ccline waits on `git` inline when nothing is cached; bar defers it to a detached job.
- Token totals: the transcript scan and a second detached job are what bar pays for `#{tokens}`, a feature ccline does not have. It is paid once per session; later renders read only appended bytes.
- Warm, both Rust tools render from caches and spawn nothing; differences of a fraction of a millisecond are within the spread of one machine.

### Badge

The `bench` badge in the top-level README reads the ben results committed here and counts cases: one per cell of the README Benchmarks tables, that is `bar` against claude-powerline and ccstatusline, and `bar-notokens` against ccline, each on warm latency, cold latency, cold total CPU including detached jobs, and memory (12 cases). bar wins a case when its value is strictly lower. The badge shows wins out of cases: green with no losses, orange with one, red with two or more.

Values: warm and cold latency medians from `ben-warm.json` and `ben-cold.json`; cold total CPU as the `cpu_ms` plus `bg_cpu_ms` medians from `ben-total-cold-<tool>.json`; memory as the mean max RSS of the GNU time runs in `ben-time-v-<tool>.txt` where present (bar, bar-notokens, ccline), otherwise the warm `max_rss_kb` median from `ben-warm.json` (the Node tools; it includes ben's own RSS, see above). These are the README values.

`scripts/bench-badge` prints the cases and writes `docs/badges/bench.svg`; `scripts/bench-badge --check` (part of `make check`) fails when the committed badge no longer matches the results. `make bench` redraws it.

## What each tool shows

Target content, the same for every tool where it can: model, context %, git branch + dirty state, session cost, 5-hour usage, 7-day usage, session duration, token totals. Plain separators (` │ `), no powerline, no icons, no network segments (no PRs, CI, usage APIs, update checks). Rendered lines from the run, colour stripped (with escapes visible in `results/ben-output-<tool>.txt`):

<!-- bench:lines:start -->
```text
bar               Opus 5.5 │ ctx 42% │ main ~1 ?1 │ $4.27 │ 37% 2h12m │ 61% 3d3h │ 1h02m │ 1.7M↑ 870k↓
bar-notokens      Opus 5.5 │ ctx 42% │ main ~1 ?1 │ $4.27 │ 37% 2h12m │ 61% 3d3h │ 1h02m
ccline            Opus 5.5 · High │ 42% │ main * +1 │ $4.27 │ 38% · 13:20 │ 61% · Fri 15:07 │ 1h2m
claude-powerline   M Opus 5.5  C 42%  ~ main (~1 ?1) *  S $4.27 (199.0M tokens)  B 38% (2h 13m)  W 61% (3d 4h)  T 1.0h 
ccstatusline       Model: Opus 5.5 │ Ctx Used: 42.0% │ main  *? │ Cost: $4.27 │ 5h: 37.5%  Reset: 2hr 12m │ Weekly: 61.2%  Weekly Reset: 3d 3hr 59m │ Session: 1hr 2m │ Total: 120.0M 
```
<!-- bench:lines:end -->

| Item | bar | ccstatusline | claude-powerline | ccline |
|---|---|---|---|---|
| model | `model` | `model` | `model` | `model` (also prints the effort level; no option to drop it) |
| context % | `ctx` | `context-percentage` | `context`, used %, no autocompact buffer | `context`, `{pct}%` |
| branch + dirty | `branch`, `dirty` | `git-branch` (raw value, no glyph), `git-status` | `git` with working-tree counts | `git` (also shows ahead count) |
| session cost | `cost` | `session-cost` | `session`, `costSource: official` | `cost` |
| 5h / 7d usage | `usage` pct + time left | `session-usage` + `reset-timer`, `weekly-usage` + `weekly-reset-timer` | `block`, `weekly` | `quota.5h`, `quota.7d`, text (reset as clock time) |
| duration | `duration` | `session-clock` | `metrics`, duration only | `duration` |
| token totals | `tokens` (in/out, excluding cache reads) | `tokens-total` (includes cache reads) | `session` tokens (includes cache reads) | **not available**: no session token metric (`context` tokens are the current context, not totals) |

Differences that remain: labels and separators follow each tool's own style (ccstatusline prints `Model:`/`Cost:` labels; claude-powerline's text charset keeps one-letter segment markers and uses spaces instead of `│`). Token totals mean different things: bar shows input (plus cache writes) and output; ccstatusline and claude-powerline add cache reads, hence their larger totals for the same transcript (the two also count differently). Values come from the same stdin, so cost, percentages and duration agree up to rounding.

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
- Subagent rows: the `bar-subagent` and `ccline-subagent` candidates of the warm and cold suites read [fixtures/subagent.json](fixtures/subagent.json), a `subagentStatusLine` JSON with three rows (two running, one completed) per the [subagent status line docs](https://code.claude.com/docs/en/statusline#subagent-status-lines); `run.sh setup` sets their `startTime` relative to the current time. bar runs `bar subagent` with the default row layout; ccline runs its usual command, which detects the task list, with its default row line. Both share the `HOME` of their status-line candidate.
- Working directory: a git repository with 4 commits, one ahead of its upstream, one modified file, one untracked file.
- Isolation: each tool runs with `HOME=/opt/slbench/home/<tool>`, so caches never cross. The suites run back to back, one candidate at a time, with nothing else from the benchmark running; `uptime` load is recorded before and after each suite ([results/ben-load-*.txt](results/)).
- Suites ([ben/](ben/)): `warm.yaml`, `cold.yaml` (`prepare: run.sh _reset <tool>` before each run: waits until the previous run's detached jobs have exited, then restores the tool's `HOME` to config only; the OS page cache stays warm) and `floor.yaml` (the harness floor). Each candidate uses `args:` (no shell) and `input_file:` for stdin. ben has no per-candidate environment, so `cmd` is `/usr/bin/env` with `HOME=… COLUMNS=200 <tool> …`; `env` execs the tool, so the measured process is the tool, but env's own start-up is included (the floor). Metrics `latency_ms`, `cpu_ms`, `cpu_user_ms`, `cpu_sys_ms`, `max_rss_kb`; scorer `single:latency_ms`, median.
- Total cold CPU: [lib/rusage.py](lib/rusage.py) with the same reset before each run. It measures CPU from `wait4` rusage at microsecond resolution and makes itself a child subreaper, so jobs a tool detaches are re-parented to it and their CPU is collected too.
- Memory of bar, bar-notokens and ccline: warm runs under `/usr/bin/time -v` after priming the caches (`results/ben-time-v-<tool>.txt`).
- ben's JSON records a host name; `run.sh ben` sets it to `container`.

## Environment

<!-- bench:env:start -->
- Native run (`BENCH_RUNNER=native`), no image: a prepared Linux environment, CPU Apple M4 Max.
- Environment: Arch Linux ARM, Linux aarch64, kernel 7.2.12, 10 CPUs visible, no CPU quota, 12 GiB RAM, glibc 2.43.
- Node 26.12.0, npm 12.2.1, Bun 1.4.4, Rust 1.99.1, git 2.56.0, Python 3.14.8, hyperfine 2.0.0, GNU time 1.10.
- ben: commit `2b24904` (reports itself as 0.1.0), as found in the environment.
- Run on 2026-11-10, `COLUMNS=200`, bar commit `d64182c`.
<!-- bench:env:end -->

## Caveats

- One machine, one container, one run. The ordering is stable but the absolute numbers are specific to that Linux VM; process start-up on macOS is slower, so expect larger absolute times there.
- The host is not necessarily idle (load figures recorded). Sub-millisecond differences between bar and ccline are within noise.
- Content is equivalent, not identical (see [What each tool shows](#what-each-tool-shows)). ccline shows no token totals, so it never scans the transcript for them; the `bar-notokens` rows remove that difference. bar defers that scan out of the render; the Node tools do it inline on every render (claude-powerline caches its session totals).
- bar's first render on a cold state shows token totals from the first 1 MiB only; the detached scan completes them within a few renders. The other tools show full totals on the first render.
- Cold for claude-powerline is measured with its downloaded price table seeded, so the row measures the tool rather than GitHub. It is refreshed once a day.
- Default cache lifetimes are kept (bar git 10 s, ccstatusline 5 s, ccline 2 s); they are not equal.
- In the container PID 1 does not reap orphans, so the detached jobs of runs under ben and GNU time stay as zombie entries until the container exits; this does not affect timings.
- The other tools are pinned to releases at least 14 days old when pinned ([docs/dependencies.md](../docs/dependencies.md#benchmark-image)), so they can trail their latest releases.

## Running

```sh
make bench         # image, bar from HEAD, ben suites, results, docs, badge; stages the changes
make bench-check   # exit 1 "benchmarks stale: run make bench" when HEAD's inputs differ from the recorded run
make bench BENCH_RUNNER=native BENCH_EXEC='docker exec -i <container>' BENCH_CPU='<host CPU>'   # the same run in a prepared Linux container on this machine, no image
```

`make bench` needs Docker or Podman and nothing else from the machine. It builds [image/Dockerfile](image/Dockerfile) (once per change to `image/`; base images pinned by digest, every download checked against a recorded checksum, ben pinned by commit), streams `git archive HEAD` into it, builds bar there with `cargo build --release --locked`, runs `run.sh ben` and copies the results out; the container never sees the working tree. It then replaces `results/ben-*`, writes `results/ben-meta.json` (bar commit, inputs hash, runner, tool versions, container and host CPU; no host name, user or path), redraws the generated sections of this file and of the top-level README (between `<!-- bench:… -->` markers, by `scripts/bench-render`) and the badge, and stages it all for a `chore(bench): …` commit. It refuses to start when the benchmark inputs or those outputs have uncommitted changes, so the results always belong to a commit. A run takes about 5 minutes after the image is built. Building the image needs a GitHub token with read access to `hop-top/uri` (a private module ben depends on), from `GITHUB_TOKEN` or the `gh` CLI; it is passed as a build secret and kept in no layer.

Native mode (`BENCH_RUNNER=native`) skips the image and runs the same steps in a prepared Linux environment: the image's layout under `SLB` (default `/opt/slbench`: `bin/ben`, `ccline/dl/ccline`, `bun/bin`) with Node, ccstatusline, claude-powerline, hyperfine, GNU time, jq and git on `PATH`; Rust (`$SLB/cargo/bin`, with `CARGO_HOME` and `RUSTUP_HOME` under `$SLB` unless set) and `$SLB/bun/bin` are added to `PATH` when present. `BENCH_EXEC` is a command prefix that runs a command there with stdin and stdout attached (`docker exec -i <container>` for a container on this machine, `ssh <host>`, or empty for this machine itself). The archive goes to `$SLB/bar` (it refuses to replace a `$SLB/bar` it did not write, or a `$SLB/bench` that is not a link), [image/entry.sh](image/entry.sh) runs from it, and the results come back as a tar on stdout; recording, docs and badge are the same as with the image. `results/ben-meta.json` records `"runner": "native"`, the environment and the tool versions found there; the CPU model comes from the environment, or from `BENCH_CPU` when the environment does not expose it. The Docker run stays the default.

Freshness: `scripts/bench-inputs` hashes the inputs that decide the results at a commit (`src/`, `presets/`, `Cargo.toml`, `Cargo.lock`, `bench/image/`, `bench/ben/`, `bench/configs/`, `bench/fixtures/`, `bench/lib/`, `bench/run.sh`), with the package's own `version` dropped from `Cargo.toml` and `Cargo.lock`. `make bench-check` compares HEAD's hash with the one in `results/ben-meta.json`. Results, docs and the badge are outputs, so committing them keeps the run fresh, and so does a release PR's version bump. Releases require it: see [Releasing](../docs/technical.md#releasing).

`make check` runs `make bench-docs-check` (the generated sections match `results/`) and `make bench-badge-check`, not `bench-check`: a change to `src/` makes the run stale without failing ordinary PRs.

By hand, in the image: `docker run --rm -it --entrypoint bash bar-bench:<tag>` (tag printed by `make bench`), a checkout of this repository at `/opt/slbench/bar`, `ln -s /opt/slbench/bar/bench /opt/slbench/bench`, `cargo build --release --locked`, then `bench/run.sh <subcommand>`: `setup`, `render`, `bench [tool…]`, `stale [tool…]`, `runners`, `bgjob`, `all` (the [hyperfine harness](#earlier-run-hyperfine-and-wait4)) and `ben` (what `make bench` runs). `WARM_RUNS`, `COLD_RUNS`, `CPU_RUNS` and `OUT` override the defaults. Paths assume `SLB=/opt/slbench`.

## Earlier run: hyperfine and wait4

The same comparison through hyperfine and `rusage.py` (`run.sh all`), run once by hand on 2026-11-07 with bar commit `09432ae`, ccstatusline 2.2.34 (`npm install -g ccstatusline@2.2.34`), claude-powerline 1.32.8 and ccline 2.0.1, in a Docker Linux arm64 container on an Apple M4 Max host: Arch Linux ARM, kernel 7.2, 10 vCPUs, no CPU quota, 31 GiB RAM, glibc 2.43, Node 26.12.0, npm 12.2.1, Bun 1.4.4, Rust 1.99.1, git 2.56.0, Python 3.14.8, hyperfine 2.0.0, GNU time 1.10; load average 0.6 to 2.8. `make bench` does not rerun it. It adds what ben does not measure: the stale scenario, the README invocations most people paste (`npx -y …@latest`, `bunx …`) and claude-powerline's price-table download. Raw data: the `results/` files without the `ben-` prefix.

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

- Warm, the two Rust tools are tied in practice: 0.3 vs 0.4 ms wall, 0.36 vs 0.50 ms CPU. Both render from caches and spawn nothing. The Node tools sit at 40 to 95 ms, of which about 10 ms is Node start-up alone.
- Cold, bar's foreground is the fastest (2.5 ms) because it never runs `git` or reads the whole transcript inline: it defers both to two detached jobs that cost another 8.7 ms of CPU. ccline runs `git` inline on a cold cache (6.3 ms wall, 2.7 ms CPU: most of the wall time is waiting on `git`) and leaves nothing behind. Counting all work, bar's cold run costs more CPU in total (2.4 + 8.7 = 11.1 ms) than ccline's (2.7 ms); bar's extra is mostly the full scan of the 5 MiB transcript for token totals, which ccline does not show.
- Stale (the steady state in a session), bar and ccline behave alike: a 4 to 6 ms render that starts one detached `git status` refresh of about 7 to 8 ms CPU. bar's render costs slightly less (3.9 vs 4.2 ms CPU, 4.2 vs 5.8 ms wall); its refresh costs slightly more (7.8 vs 6.9 ms).
- Memory: bar 3.5 MiB, ccline 5.3 MiB, the Node tools 100 to 116 MiB per render.
- claude-powerline under Bun (`bunx --bun`, 24 ms) is faster than its global Node install (41 ms). ccstatusline through `bunx`/`npx` with `@latest` pays 120 to 280 ms of package resolution on every render, as its README warns; the pinned global install avoids that.

Method, beyond [Method](#method):

- Warm: `hyperfine -N --warmup 10 --runs 100 --input session.json`.
- Cold: `hyperfine -N --runs 30 --prepare 'run.sh _reset <tool>'`. The reset waits until the previous run's detached jobs have exited, then restores the tool's `HOME` to config only. The OS page cache stays warm (dropping it needs privileges the container does not have).
- CPU and memory: 20 warm runs each under `/usr/bin/time -v` (memory: max RSS) and [lib/rusage.py](lib/rusage.py) (CPU: `wait4` rusage at microsecond resolution; GNU time prints CPU in 10 ms steps, which reads as 0 for the Rust tools). `rusage.py` makes itself a child subreaper, so jobs a tool detaches are re-parented to it and their CPU is collected too; that is the "detached" column. Max RSS from `rusage.py` is not used: `ru_maxrss` keeps the parent's high-water mark across `exec`.
- Stale: `rusage.py --runs 5 --prepare 'sleep 11'` after priming the caches.
- bar's background jobs: on a cold state a render spawns two detached `bar refresh` jobs (`git status`, and the rest of the transcript scan beyond the 1 MiB read inline); in steady state one (`git status` every 10 s). Their CPU is in the tables above; per-run data in `results/bar-bgjob-cold.json`, `results/rusage-*-bar.json`.

A ben run at bar `f0d19c3` on the same machine agreed with these within 1 to 2 ms for the Node tools.

## Files

- `run.sh`: the runner.
- `image/`: the `make bench` image (`Dockerfile`, `entry.sh` run inside it and by native mode, `helpers.sh` for the build).
- `configs/<tool>/`: configs used (`configs/bar-notokens/`: bar without token totals, for the like-for-like comparison).
- `fixtures/session.json`, `fixtures/subagent.json`, `fixtures/gen-transcript.mjs`: status line and subagent row inputs, transcript generator (the 5 MiB transcript itself is not committed).
- `ben/`: ben suites (`warm.yaml`, `cold.yaml`, `floor.yaml`).
- `lib/rusage.py`: CPU, memory and detached-job accounting; `lib/procs.py`: finds a tool's leftover jobs; `lib/summarize.py`: tables; `lib/envinfo.py`: tool versions and container environment for `ben-meta.json`.
- `results/ben-*`: the `make bench` run: ben JSON, load records, rendered lines and summary; `ben-total-cold-<tool>.json`: cold foreground plus detached-job CPU (`rusage.py`); `ben-time-v-<tool>.txt`: GNU `time -v` for bar, bar-notokens, ccline; `ben-meta.json`: commit, inputs hash, versions, environment.
- `results/` without the `ben-` prefix: the earlier hyperfine run (`hyperfine-*.json`, `rusage-{warm,cold,stale}-*.json`, `time-v-*`, `output-*`, `load-*`, `summary.md`).
