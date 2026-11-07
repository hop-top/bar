# Technical notes

## Flow

`bar render`: stdin session JSON, config, `COLUMNS` in; at most two lines out; always exit 0.

1. `session::parse` (lenient) and `conf::load`.
2. Per group (`left`, `center`, `right`): `format::parse` into tokens, split into segments at the separator, render each placeholder, drop empty segments.
3. Plain skin: `layout::compose` joins each group's segments and fits the groups to `COLUMNS` minus margin 4 (one line, else two). Other skins: `skin::paint` draws each group and returns its text with its width (skin glyphs one cell each), then `layout::compose_groups` fits them the same way.

The palette's muted colour is set per thread for the length of a render (`style::use_muted`), so `style::dim` picks it up without threading the config through every placeholder. The `terminal` palette works the same way (`style::use_ansi`): `fg` and `bg_code` draw codes 0-15 as ANSI 16-colour SGR while it is on.

Module map: `conf` (parser, search path), `format` (placeholder syntax), `render` (wiring), `placeholders/` (core placeholders), `plugin/` (`bar-<name>` host), `cache` (background refresh), `transcript` (incremental token totals), `linescan` (shared incremental line reader), `spendhist` (daily spend totals), `history` (usage samples behind `spark`), `prices`, `tz`, `expr` (label logic to GitHub queries), `pace` (score), `style` (SGR, OSC 8, width), `skin` (powerline and capsule painting), `palette` (named colour sets), `presets` (embedded example configs, `bar init`), `click` (toggle state, URL handler install), `check`, `preview`.

## Cache

Anything slow is cache-backed: `git status` (`dirty`, `ahead`; key `gitst:<repo root>`, run with `GIT_OPTIONAL_LOCKS=0` in the repo root, no shell), `gh` lists, `%command%` values, plugin `--ext-info` and plugin render output. A reader never waits. It returns the stored value, or nothing on a cold cache, and the lock winner spawns a detached `bar refresh <request>`.

- State dir: `BAR_STATE_DIR`, else kit `raw_state_dir("bar")`. Files mode 0600.
- Refresh job carries kinds and options only; secrets stay in the keychain, read by plugins.
- A failed refresh backs off 60 s, whatever the ttl. Locks expire after 30 s. Stored values cap at 64 KiB.

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

`src/spendhist.rs` backs `#{spendhist}`. `bar refresh` runs `Job::SpendHist` (cache key `spendhist:<claude dir>:<tz>`, ttl 60 s, failure backoff 60 s): it lists `projects/*/*.jsonl` newest first (skipping files untouched for over 63 days), scans each from its stored `Mark`, and folds assistant `usage` into `{local date: {model: [in, out, cache write, cache read]}}`. A message is counted once: a 48-bit hash of `message.id` plus `requestId` is kept per day in the state file and checked across all files. The result is one state file per claude dir and zone under `<state>/spendhist/` (0600, atomic rename; per-file marks, 62 days of totals, dedupe hashes; the hashes are trimmed oldest-first past 16 MiB). The job returns only the last 35 days, which is what the cache stores and a render reads; dollars are computed at render from the tokens, so a `set price` change applies at once. A render never opens a transcript. Model ids longer than 128 characters or with characters outside `A-Z a-z 0-9 - . _ : / @ +`, and a day's models past the 63rd, are counted under `other`, which is never priced (the total gets its `~`); a summary that would pass the cache value cap keeps each day's largest models and folds the rest into `other`.

The offset, inode, size and mtime logic (`linescan::Mark`) is shared with `transcript`. Local dates use `libc::localtime_r` in `src/tz.rs`, memoised per half hour; there is no date-time dependency.

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

## Platforms

Unix only: the crate fails to compile off `cfg(unix)`. macOS and Linux build and pass `make check`. FFI goes through the `libc` crate in two `unsafe` sites: `localtime_r` in `src/tz.rs` and `kill(pid, 0)` in `src/placeholders/sessions.rs` (POSIX; `pid_t` is `i32` on both). Clickable commands (`src/click/`, the `bar://` app bundle, `osascript`) are macOS-only: on other systems `bar click install` writes nothing, prints that it needs macOS (and points at `--hooks-only`) and exits 2. `bar click install --hooks-only` registers only the Claude Code hooks and works on Linux. Plugin tests and the reference plugins need `jq`.

Linux check, from a clean copy so the host `target/` stays untouched:

```sh
mkdir /tmp/lr-src /tmp/lr-target
rsync -a --exclude target --exclude .git ./ /tmp/lr-src/
docker run --rm -v /tmp/lr-src:/src -v /tmp/lr-target:/tgt -e CARGO_TARGET_DIR=/tgt -w /src rust:1-bookworm sh -c 'rustup component add clippy rustfmt >/dev/null 2>&1; apt-get update >/dev/null 2>&1; apt-get install -y make git jq >/dev/null 2>&1; make check; echo EXIT=$?'
```

## Releasing

Pushing a `v*` tag runs `.github/workflows/release-rs-caller.yml`. Its `create-release` job checks that the tag version equals the `Cargo.toml` version and creates the release once (`--prerelease` when the tag contains `-`). The `binaries` jobs then build `--release --locked` natively for `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`, `x86_64-apple-darwin` and `aarch64-apple-darwin`, and uploads `bar-<version>-<target>.tar.gz` (containing `bar`) plus a `.sha256` file to that release. Linux binaries are built on `ubuntu-22.04` (glibc 2.35) and need glibc 2.35 or newer; the aarch64 Linux job needs a repo or plan with `ubuntu-22.04-arm` runners available. The crate is not published to crates.io (`publish = false`); the workflow has no publish job.

Homebrew: `packaging/homebrew/bar.rb` is a template. After the release, set `version` (no leading `v`) and replace each `SHA256_<TARGET>` with the hex digest from the matching `.sha256` file, then copy the file into a tap's `Formula/`. No tap exists yet. The release assets must be publicly downloadable for the formula to work.
