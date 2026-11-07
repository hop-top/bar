# Technical notes

## Flow

`bar render`: stdin session JSON, config, `COLUMNS` in; at most two lines out; always exit 0.

1. `session::parse` (lenient) and `conf::load`.
2. Per group (`left`, `center`, `right`): `format::parse` into tokens, split into segments at the separator, render each placeholder, drop empty segments.
3. `layout::compose` fits the groups to `COLUMNS` minus margin 4 (one line, else two).

Module map: `conf` (parser, search path), `format` (placeholder syntax), `render` (wiring), `placeholders/` (core placeholders), `plugin/` (`bar-<name>` host), `cache` (background refresh), `transcript` (incremental token totals), `linescan` (shared incremental line reader), `spendhist` (daily spend totals), `prices`, `tz`, `expr` (label logic to GitHub queries), `pace` (score), `style` (SGR, OSC 8, width), `click` (toggle state, URL handler install), `check`, `preview`.

## Cache

Anything slow is cache-backed: `git status` (`dirty`, `ahead`; key `gitst:<repo root>`, run with `GIT_OPTIONAL_LOCKS=0` in the repo root, no shell), `gh` lists, `%command%` values, plugin `--ext-info` and plugin render output. A reader never waits. It returns the stored value, or nothing on a cold cache, and the lock winner spawns a detached `bar refresh <request>`.

- State dir: `BAR_STATE_DIR`, else kit `raw_state_dir("bar")`. Files mode 0600.
- Refresh job carries kinds and options only; secrets stay in the keychain, read by plugins.
- Failed refresh backs off 60 s (or the ttl if shorter; the `git status` job always 60 s). Locks expire after 30 s. Stored values cap at 64 KiB.

## Transcript totals

`src/transcript.rs` folds `message.usage` from each transcript line into running totals. Per transcript it keeps `<state>/transcript/<hash>.json` (0600, atomic rename): inode, size, mtime, committed byte offset, totals, last message id, last assistant timestamp. Only complete lines (newline-terminated) advance the offset, so a half-written last line is read again once finished. A line is parsed only when it contains `"usage":`; lines over 512 KiB are dropped without parsing, and a line cut by the read cap is marked `skip` so it is not re-read forever.

A render reads at most 1 MiB, once per process for all three placeholders. Beyond that `transcript::load` asks the cache for a `Job::Transcript`, which runs `bar refresh` over the rest with no byte cap (time-limited like any refresh) and saves progress after each run. The render never waits for it.

Measured on macOS 15, release build, 20 MB synthetic transcript (a third of the lines are user messages), CPU time of the child over 20 runs:

| Run | CPU median |
|-----|-----------|
| first render, cold state (reads 1 MiB, spawns the scan) | 25 ms |
| warm state, nothing appended | 2.4 ms |
| three lines appended | 5.3 ms |
| same layout, no transcript | 2.7 ms |

## Spend history

`src/spendhist.rs` backs `#{spendhist}`. `bar refresh` runs `Job::SpendHist` (cache key `spendhist:<claude dir>:<tz>`, ttl 60 s, failure backoff 60 s): it lists `projects/*/*.jsonl` newest first (skipping files untouched for over 63 days), scans each from its stored `Mark`, and folds assistant `usage` into `{local date: {model: [in, out, cache write, cache read]}}`. A message is counted once: a 48-bit hash of `message.id` plus `requestId` is kept per day in the state file and checked across all files. The result is one state file per claude dir and zone under `<state>/spendhist/` (0600, atomic rename; per-file marks, 62 days of totals, dedupe hashes; the hashes are trimmed oldest-first past 16 MiB). The job returns only the last 35 days, which is what the cache stores and a render reads; dollars are computed at render from the tokens, so a `set price` change applies at once. A render never opens a transcript.

The offset, inode, size and mtime logic (`linescan::Mark`) is shared with `transcript`. Local dates use `localtime_r` through a small FFI declaration in `src/tz.rs`, memoised per half hour; there is no date-time dependency.

## Check and preview

`check` reuses the parsers the renderer uses (`conf`, `format`, `expr`, `gh::plan`, `plugin::validate`), so it flags what render would silently drop. Core option names and value kinds live in one table in `check.rs`; adding a core option means adding it there.

`preview` goes through the normal layout code with a stand-in for every placeholder (`render::render_sample`). The stand-in reads nothing outside the process: `model`, `effort` and `ctx` run on a synthetic session, `usage` on synthetic windows with a state dir that does not exist, the rest are fixed strings.

## Performance

Target: a cached `bar render` under 50 ms in a release build.

Git state comes from files, not processes (`src/gitfs.rs`). The reader walks up from the working directory to `.git` (directory, or a file with `gitdir:`), follows `commondir` for linked worktrees, reads `HEAD` for the branch and `remote.origin.url` from the common `config`. It reports `Unsupported`, and `src/placeholders/repo.rs` spawns `git --no-optional-locks` instead, for: `GIT_DIR`, `GIT_WORK_TREE`, `GIT_COMMON_DIR`, `GIT_CONFIG_COUNT`, `GIT_CONFIG_PARAMETERS`, `GIT_CONFIG_SYSTEM` or `GIT_CONFIG_NOSYSTEM` in the environment, a bare repository as the working directory, `include`/`includeIf`/`url.*.insteadOf` sections in the repository or global config, `[remote.x]` headers, line continuations, reftable `HEAD`, or a file that is not a regular file of at most 1 MiB (a FIFO never blocks the reader). A plain render spawns no `git`. `dirty`, `ahead` and `worktree` never spawn it either: when the reader reports `Unsupported` they render empty. Everything else is in-process.

Measured on macOS 15 with Apple git 2.39.5 (`/usr/bin/git`), release build, temp repo with a GitHub `origin`, fake `gh` on `PATH` returning `[]`, warm cache, 20 runs after warm-up, CPU time (user + system) of the child via `getrusage`:

| Run | CPU median |
|-----|-----------|
| `bar render`, default layout, spawning git (5 processes) | 52 to 55 ms (92 ms on a saturated machine) |
| `bar render`, default layout, git from files | 3.2 to 3.7 ms |
| `bar --version` | 4.9 ms (saturated machine) |
| one `git` call (`branch`, `remote get-url`, `rev-parse`) | 14 ms (saturated machine) |

Wall time is not reported: it depends on machine load.
