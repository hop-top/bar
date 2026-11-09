# Technical notes

## Flow

`bar render`: stdin session JSON, config, `COLUMNS` in; at most `max-rows` (default 4) rows out; always exit 0.

1. `session::parse` (lenient) and `conf::load` with the session's directory: the user config, then the project `.bar.conf` (`conf::project::find`, stats only) applied over it by `conf::parse_layers`.
2. Per group (`left`, `center`, `right`): `format::parse` into tokens, split into segments at the separator, render each placeholder, drop empty segments.
3. Plain skin: `layout::join` joins each group's segments and `layout::compose_groups` fits the groups to `COLUMNS` minus margin 4 (one line, else two). Other skins: `skin::paint` draws each group and returns its text with its width (skin glyphs one cell each), then `layout::compose_groups` fits them the same way. Each configured line (`Config::lines`) goes through this alone; empty lines are skipped and the rows are capped at `max-rows`. With `set wrap on`, a line that `layout::place_groups` reports as not fitting one row is cut into rows by `layout::flow` at segment boundaries and each row is drawn as a left group; segments past the last allowed row are dropped.

`bar subagent`: the `subagentStatusLine` JSON (`columns`, `tasks`) in; one `{"id", "content"}` JSON line per drawn row out; always exit 0. `subagent::parse` (lenient, at most 64 tasks), then per task `subagent::session` builds a `Session` from the task (model, effort, context, directory; no session id or transcript) with `Session::task` set, and `render::render_row` renders the `subagent` layout as one left group through the same segment, skin and palette code, dropping trailing segments past `columns` and clipping what is left with `style::clip` (escape-aware). `placeholders::lookup_in` resolves `task.*` names only when `Session::task` is set; plugins are not looked up there.

The palette's muted colour is set per thread for the length of a render (`style::use_muted`), so `style::dim` picks it up without threading the config through every placeholder. The `terminal` palette works the same way (`style::use_ansi`): `fg` and `bg_code` draw codes 0-15 as ANSI 16-colour SGR while it is on. So does the colour depth (`style::use_depth`, from `set color-depth` and, for `auto` or unset, `NO_COLOR`, `TERM`, `COLORTERM` and `FORCE_COLOR`): `fg` and `bg_code` take a `color::Color` (a code or RGB) and quantise it to the depth as they write it, so placeholders never look at the depth; at `none` lines are laid out with the plain skin and the finished rows go through `style::strip_colors`. `fg=`, `bold` and `gradient=` (`look`) and `set gradient` recolour finished text with `gradient::recolor`, which copies escape sequences (dropping foreground colours the ink replaces), draws the ink again after each other CSI and gives each grapheme cluster (`unicode-segmentation`) one colour, measured as `style::visible_width_with` measures it, so links, widths and truncation are unaffected.

Module map: `conf` (parser, search path, `project` lookup and policy), `format` (placeholder syntax), `render` (wiring), `placeholders/` (core placeholders), `plugin/` (`bar-<name>` host), `cache` (background refresh), `transcript` (incremental token totals, turn timing, cache misses), `linescan` (shared incremental line reader), `spendhist` (daily spend totals), `status` (service status summary), `usageapi` (opt-in usage endpoint: credentials, fetch, derived numbers), `history` (usage samples behind `spark`), `prices` (rates; `models` data-dir snapshot, `norm` id normaliser, `update` fetch), `tz`, `expr` (label logic to GitHub queries), `pace` (score), `style` (SGR, OSC 8, width), `skin` (powerline and capsule painting), `palette` (named colour sets), `presets` (embedded example configs, `bar init`), `click` (toggle state, URL handler install, hook state: waiting marker, subagents, `tlc` records), `subagent` (subagent rows), `check`, `preview`.

## Cache

Anything slow is cache-backed: `git status` (`dirty`, `ahead`; key `gitst:<repo root>`, run with `GIT_OPTIONAL_LOCKS=0` in the repo root, no shell), `jj log` (`jj`; key `jj:<workspace root>`, run with `--ignore-working-copy` so it never snapshots, no shell), `gh` and `glab` lists, `%command%` values, plugin `--ext-info` and plugin render output. A reader never waits. It returns the stored value, or nothing on a cold cache, and the lock winner spawns a detached `bar refresh <request>`.

- State dir: `BAR_STATE_DIR`, else kit `raw_state_dir("bar")`: `XDG_STATE_HOME/bar`, else `~/Library/Application Support/bar` (macOS), `~/.local/state/bar` (Linux), `%LOCALAPPDATA%\bar` (Windows). Layout: [Files and environment](config.md#files-and-environment). Files mode 0600, dirs 0700 on Unix; on Windows they inherit the ACL of the per-user state dir.
- Detached refresh: on Unix the child gets its own process group (`process_group(0)`), stdio `/dev/null`, and is not waited on. On Windows it is created with `DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP`, plus `CREATE_BREAKAWAY_FROM_JOB` when the caller's job object allows it (retried without otherwise), stdio null; before the spawn the render clears the inherit flag on its own standard handles, so the child holds no copy of the pipe Claude Code reads and the render's output ends when the render exits.
- `bar refresh` runs in the state dir, not the render's working directory (often a cloned repository), except for a `%command%` job, which keeps the directory it was rendered in.
- Job children (`git`, `jj`, `gh`, `glab`, `tmux`, plugins, `%command%`) run in their own process group (Unix) or with `CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP` (Windows, so no console window flashes); on timeout the group is killed with `kill -KILL -<pgid>`, on Windows the tree with `taskkill /T /F` and the child directly. On Windows `git`, `jj`, `gh`, `glab` and `tmux` are looked up on `PATH` with `PATHEXT` (so a `gh.cmd` shim works); `Command` alone only tries `.exe`. Tools (`git`, `jj`, `gh`, `glab`, `tmux`, `curl`, `security`) are looked up in the absolute `PATH` dirs only: an empty or relative entry (`::`, `.`) would resolve against the working directory, often a cloned repository, so it is skipped on every OS.
- Store: write a sibling temp file, rename over the target (Windows `MoveFileEx` with replace).
- Refresh job carries kinds and options only; secrets stay in the keychain, read by plugins.
- A refresh runs at most 20 s (`BAR_REFRESH_TIMEOUT`, seconds, clamped to 1-29 so it ends before its lock expires); each job also has its own limit (`%command%` 3 s, plugin render 3 s, `--ext-info` 5 s).
- A failed refresh keeps the stored value and backs off 60 s, whatever the ttl. Locks expire after 30 s. Stored values cap at 64 KiB.

## Transcript totals

`src/transcript.rs` folds `message.usage` from each transcript line into running totals. Per transcript it keeps `<state>/transcript/<hash>.json` (0600, atomic rename): inode, size, mtime, committed byte offset, totals, last message id, last assistant timestamp, the context size of the first, previous and last main-conversation request since the last compaction with their count (for `ctx` `growth` and `trend`), main-conversation prompt and assistant message counts (for `msgs`; prompts come from the same user-entry reading as the turn state), and the turn state below. Only complete lines (newline-terminated) advance the offset, so a half-written last line is read again once finished. A line is parsed only when it contains `"usage":`, `compact_boundary`, `Skill` or `"type":"user"`; lines over 512 KiB are dropped without parsing, and a line cut by the read cap is marked `skip` so it is not re-read forever. `transcript/facts.rs` reads a line typed (serde, borrowed strings, unneeded values skipped) and falls back to a full `serde_json::Value` tree where the typed reading cannot vouch for the line; both readings agree on every field.

`transcript/turns.rs` (`Turns`) feeds `ttft`, `tps`, `misses` and the detected cache lifetime of `cachettl` from the same pass: the pending prompt time, the last user entry time, the response being written (id, start, last entry, output tokens), the last turn's sums, session sums, the last cached prefix, the 1-hour flag, a compaction flag and the two miss counters. Constant size per transcript; nothing is kept per turn. The state format carries a version; an older file is ignored and the transcript rescanned.

A render reads at most 1 MiB, once per process for every transcript placeholder. Beyond that `transcript::load` asks the cache for a `Job::Transcript`, which runs `bar refresh` over the rest with no byte cap (time-limited like any refresh) and saves progress after each run. The render never waits for it.

Measured on macOS 15, release build, 20 MB synthetic transcript (a third of the lines are user messages), CPU time of the child over 20 runs:

| Run | CPU median |
|-----|-----------|
| first render, cold state (reads 1 MiB, spawns the scan) | 25 ms |
| warm state, nothing appended | 2.4 ms |
| three lines appended | 5.3 ms |
| same layout, no transcript | 2.7 ms |

## Spend history

`src/spendhist.rs` backs `#{spendhist}`. `bar refresh` runs `Job::SpendHist` (cache key `spendhist:<claude dir>:<tz>`, ttl 60 s, failure backoff 60 s): it lists `projects/*/*.jsonl` newest first (skipping files untouched for over 63 days), scans each from its stored `Mark`, and folds assistant `usage` into `{local date: {model: [in, out, cache write, cache read]}}`. A message is counted once: a 48-bit hash of `message.id` plus `requestId` is kept per day in the state file and checked across all files. The result is one state file per claude dir and zone under `<state>/spendhist/` (0600, atomic rename; per-file marks, 62 days of totals, dedupe hashes; the hashes are trimmed oldest-first past 16 MiB). The state also keeps totals per UTC hour for the last 25 hours (`hours`). A state file without `hours` (from a bar without hourly totals) gets one extra pass over the already-scanned part of transcripts modified in those 25 hours that adds to `hours` only, deduplicated within the pass, so days are not counted twice; until the pass completes the state is saved without `hours` and the pass starts over. The job returns only the last 35 days, which is what the cache stores and a render reads; dollars are computed at render from the tokens, so a `set price` change applies at once. When a model with tokens this month has no `set price` rule or built-in price and the data-dir models.dev snapshot is missing, unusable or over a day old (mtime), the render also requests `Job::Prices` (cache key `prices:models.dev`, ttl 1 day, failure backoff 60 s), which runs the `bar prices update` fetch with the refresh time limit as curl's `--max-time`; the snapshot is read once per process, so the new prices show from the next render. A render never opens a transcript. Model ids longer than 128 characters or with characters outside `A-Z a-z 0-9 - . _ : / @ +`, and a day's models past the 63rd, are counted under `other`, which is never priced (the total gets its `~`); a summary that would pass the cache value cap keeps each day's largest models and folds the rest into `other`.

The offset, inode, size and mtime logic (`linescan::Mark`) is shared with `transcript`. Local dates use `libc::localtime_r` on Unix and `GetDynamicTimeZoneInformation` + `SystemTimeToTzSpecificLocalTimeEx` on Windows (`src/platform/`), memoised per half hour; there is no date-time dependency.

## Update check

`src/update.rs` backs `#{update}` and `bar update`, both off unless `set update-check on`. `bar refresh` runs `Job::Update { prerelease }` (cache key `update:stable` or `update:prerelease`, by whether the running version has a SemVer prerelease part; ttl 1 day): `curl` fetches `GET /repos/hop-top/bar/releases?per_page=30` from the GitHub API (no token; 4 MiB cap; the refresh time limit as `--max-time`), drafts and, on the stable channel, prereleases are dropped, and of the tags `v<semver>` or `bar-v<semver>` (others are ignored) the one with the highest SemVer precedence is stored. A failure stores an empty value rather than failing the job, so it waits a day instead of the 60 s backoff and stays well under GitHub's unauthenticated rate limit. The render compares the stored tag with `CARGO_PKG_VERSION`; a `0.0.0` development build is not compared. `src/net.rs` holds the one `curl` invocation the price snapshot and the update check share: `-q` first, so no `.curlrc` changes it, and `--` before the URL.

`src/status.rs` and `placeholders/status.rs` back `#{status}`, off unless `set service-status on` (a project config cannot set it in safe mode, like `update-check`). `bar refresh` runs `Job::Status { strip }` (cache key `status:summary` or `status:strip`, ttl 5 min, the 60 s failure backoff): `curl` fetches `https://status.claude.com/api/v2/summary.json` (Statuspage: overall `status.indicator`, `components`, unresolved `incidents`) and, for `strip`, `/api/v2/incidents.json` (the latest 50 incidents, which replace the summary's list); 4 MiB cap per body, the refresh time limit shared as `--max-time`. The job stores a compact JSON (`ind`, component names and states, and incidents of the last 72 hours with title, impact, start, end or 0 while active, affected component names; at most 40). A failed fetch or unexpected JSON fails the job, so the previous value stays and the next try waits 60 s. The render parses the value, applies the `components` filter (case-insensitive substring of the component name; an incident naming no component counts for all) and draws the state, the active incident or the strip; cells are `ceil(48 / hours)` blocks ending at the render time, each the worst impact among overlapping incidents with impact `minor` to `critical`. Component states map `degraded_performance` to minor, `partial_outage` to major and `major_outage` to critical; maintenance is ignored. Incident text passes through `sanitize`. The `status` cache key is shared by every placeholder that does not ask for `strip`.

`src/usageapi.rs` backs `#{extra}` and `#{usage[model=...]}`, both off unless `set usage-api on`. `bar refresh` runs `Job::UsageApi { dir, hashed, prev }` (cache key `usage-api:<claude dir>`, ttl 5 min): `usageapi/creds.rs` reads the OAuth access token (macOS keychain via `security ... -w` read from a pipe, else `<dir>/.credentials.json`; `hashed` picks the keychain service suffix, `sha256(dir)[..8]`, computed by `usageapi/sha256.rs`), and `net::request` sends it to the system curl (`platform::system_program`: `/usr/bin` on macOS, the system directory on Windows; the `test-tools` cargo feature, refused in release builds, lets tests fake it through `PATH`) as a config line on stdin (`-K -`), without `-f`, with a `-w` trailer of the status and `Retry-After`. The job never carries the token: it is read inside `bar refresh` and dropped after the request; the token, the credentials JSON it came from and the curl config line live in `secret::Secret` buffers zeroed on drop (the credentials JSON is parsed borrowing the token, so serde holds no copy). Only derived numbers are stored (`Data`: per-model windows, extra usage, fetch time, the day's starting amount, `retry_at`). A 429 or 401/403 stores the previous numbers with `retry_at`, and so does any other failure (curl error, other status, bad JSON) with `fails` counting them for a 300 s wait doubling to 3600 s; only a missing token leaves the job failed for the cache's 60 s backoff; the render reads the stored value first and does not start a refresh before `retry_at`. `prev` is the cache file of the last value, which the job reads for the baseline and to keep numbers through a rate limit. `placeholders/money.rs` renders limit components for both `extra` and `spend`, and holds the money text every placeholder uses (`usd`, `amount` in a currency, whole-dollar budget limits); `budget.rs` colours spend against a `set budget-<kind>` limit for `budget`, `cost` and `spendhist`.

## Check and preview

`check` reuses the parsers the renderer uses (`conf`, `format`, `expr`, `gh::plan`, `plugin::validate`), so it flags what render would silently drop. Core option names and value kinds live in one table in `check.rs`; adding a core option means adding it there.

`preview` goes through the normal layout code with a stand-in for every placeholder (`render::render_sample`). The stand-in reads nothing outside the process: `model`, `effort` and `ctx` run on a synthetic session, `usage` on synthetic windows with a state dir that does not exist, the rest are fixed strings.

`bar edit` (`src/edit/`, cargo feature `edit`, on by default) edits the config as text: `doc` keeps the file's lines and rewrites one token span per change, `group` splits a layout value into segments the way `format::segments` does and rewrites single placeholders, `model` holds undo and redo as whole-text snapshots. The preview is `preview::render_config` on the parsed buffer, its ANSI output turned into ratatui spans. Option kinds come from the `check.rs` table and plugin `--ext-info` schemas (asked once per plugin, when its options are first opened). Only the `edit` subcommand reaches the module; `bar render` and `bar refresh` dispatch before clap and never touch it.

## Performance

Target: a cached `bar render` under 50 ms in a release build.

Git state comes from files, not processes (`src/gitfs.rs`). The reader walks up from the working directory to `.git` (directory, or a file with `gitdir:`), follows `commondir` for linked worktrees, reads `HEAD` for the branch and `remote.origin.url` from the common `config`. Refs resolve through loose files, then `packed-refs` (`src/gitfs/refs.rs`); tags at HEAD come from `refs/tags/` and the peeled `^` lines of `packed-refs`; the stash count is the line count of `logs/refs/stash`; the operation in progress comes from the marker files git's prompt script reads (`src/gitfs/ops.rs`); unmerged paths come from the index, versions 2 to 4, read whole up to 2 MiB (`src/gitfs/index.rs`). It reports `Unsupported`, and `src/placeholders/repo.rs` spawns `git --no-optional-locks` instead (except in a `bar subagent` row, where the value is left unknown), for: `GIT_DIR`, `GIT_WORK_TREE`, `GIT_COMMON_DIR`, `GIT_CONFIG_COUNT`, `GIT_CONFIG_PARAMETERS`, `GIT_CONFIG_SYSTEM` or `GIT_CONFIG_NOSYSTEM` in the environment, a bare repository as the working directory, `include`/`includeIf`/`url.*.insteadOf` sections in the repository or global config, `[remote.x]` headers, line continuations, reftable `HEAD`, or a file that is not a regular file of at most 1 MiB (a FIFO never blocks the reader). A plain render spawns no `git`. `sha` falls back to `git rev-parse` the same way. `dirty`, `ahead`, `worktree`, `tag`, `stash`, `gitop` and `conflicts` never spawn it in render: when the reader reports `Unsupported` they render empty, except that `tag` asks a cached background `git for-each-ref` when a loose tag might be an annotated tag of HEAD (or the refs are unreadable), and `conflicts` reads the cached background `git status` for an index it does not parse. Everything else is in-process.

Measured on macOS 15 with Apple git 2.39.5 (`/usr/bin/git`), release build, temp repo with a GitHub `origin`, fake `gh` on `PATH` returning `[]`, warm cache, 20 runs after warm-up, CPU time (user + system) of the child via `getrusage`:

| Run | CPU median |
|-----|-----------|
| `bar render`, default layout, spawning git (5 processes) | 52 to 55 ms (92 ms on a saturated machine) |
| `bar render`, default layout, git from files | 3.2 to 3.7 ms |
| `bar --version` | 4.9 ms (saturated machine) |
| one `git` call (`branch`, `remote get-url`, `rev-parse`) | 14 ms (saturated machine) |

Wall time is not reported: it depends on machine load.

## Platforms

Unix (macOS, Linux) and Windows; the crate fails to compile elsewhere. Every OS difference lives in `src/platform/` (`unix.rs`, `windows.rs`, same function set): home dir, private dirs and files, file identity and mtime, executables and `PATHEXT`, the shell for `%command%`, child process groups and tree kill, detached spawn, process liveness, local zone offset. macOS and Linux build and pass `make check`.

FFI: on Unix the `libc` crate, two `unsafe` sites (`localtime_r`, `kill(pid, 0)`); on Windows `windows-sys` (kernel32: `GetStdHandle`/`SetHandleInformation`, `OpenProcess`/`GetExitCodeProcess`, the time-zone conversions).

| Unix | Windows |
|---|---|
| `HOME` | `USERPROFILE`, else `HOME` |
| modes 0700 / 0600 | inherited ACL |
| inode | creation time |
| execute bit | `PATHEXT` extension (`.com .exe .bat .cmd`) |
| `sh -c` | `cmd /D /S /C` |
| `process_group(0)` | `CREATE_NEW_PROCESS_GROUP` (+ `DETACHED_PROCESS` or `CREATE_NO_WINDOW`) |
| `kill -KILL -<pgid>` | `taskkill /T /F` + `Child::kill` |
| `kill(pid, 0)` | `OpenProcess` + `GetExitCodeProcess` |
| `localtime_r` (`TZ` honoured) | `SystemTimeToTzSpecificLocalTimeEx` (system zone) |
| `/etc/gitconfig` | `%PROGRAMDATA%\Git\config`, `<git install>\etc\gitconfig` |

Click handler dirs (`src/click/dirs.rs`, `src/click/pointer.rs`): LaunchServices starts the applet with the launchd environment, so the shell's `XDG_*` and `BAR_STATE_DIR` are missing there. `bar click install` bakes `HANDLER_ENV` into the AppleScript as a shell prefix (`unset <unset vars>; VAR='value' … '<bar>'`, POSIX single quotes, then AppleScript string escaping) and records the resolved dirs in `Contents/Resources/bar-dirs.json`, written after `osacompile` and before `codesign`. `bar check` (macOS) reads that record; nothing on the render path does. The render path keeps `<macOS default state dir>/state-dir`, a symlink to its state dir: after the output is flushed, one `readlink` (or `lstat` from the default dir); a write only on change, via a temp symlink renamed over the old one, and not within 60 s of the last change. `bar toggle` and `bar click run` follow it only when their own state dir resolves to the default and the target holds `click-installed`.

Clickable commands (`src/click/`, the `bar://` app bundle, `osascript`) are macOS-only: on other systems `bar click install` writes nothing, prints that it needs macOS (and points at `--hooks-only`) and exits 2; on Windows `bar click run` exits 2 at once. `bar click install --hooks-only` registers only the Claude Code hooks and works on Linux and Windows. Plugin tests and the reference plugins need `jq`.

Windows check from macOS (no Windows machine needed for the build and lint; nothing is executed):

```sh
rustup target add x86_64-pc-windows-msvc aarch64-pc-windows-msvc x86_64-pc-windows-gnu
for t in x86_64-pc-windows-msvc aarch64-pc-windows-msvc x86_64-pc-windows-gnu; do
  cargo clippy --locked --target "$t" --all-targets -- -D warnings
done
```

Tests run on Windows in the `windows` job of `.github/workflows/test-rs-caller.yml` (`windows-2022`). Test files built on shell-script fixtures, POSIX modes or POSIX paths carry `#![cfg(unix)]`; `tests/windows.rs` is Windows-only.

Linux check, from a clean copy so the host `target/` stays untouched:

```sh
mkdir /tmp/lr-src /tmp/lr-target
rsync -a --exclude target --exclude .git ./ /tmp/lr-src/
docker run --rm -v /tmp/lr-src:/src -v /tmp/lr-target:/tgt -e CARGO_TARGET_DIR=/tgt -w /src rust:1-bookworm sh -c 'rustup component add clippy rustfmt >/dev/null 2>&1; apt-get update >/dev/null 2>&1; apt-get install -y make git jq >/dev/null 2>&1; make check; echo EXIT=$?'
```

## Releasing

Versions come from [release-please](https://github.com/googleapis/release-please) with hop-top/.github's target-versioning extension: `.github/workflows/release-please.yml` calls its shared `release-please-on-push.yml`, pinned to a release commit SHA (`release-please-preflight.yml` carries the same pin). Config: `.github/release-please-config.json`; manifest: `.github/.release-please-manifest.json`. Every push to `next`, `main` or `release/<major>.<minor>` keeps one release PR open against that branch. Merging it sets `version` in `Cargo.toml` and `Cargo.lock`, writes `CHANGELOG.md`, tags `v<version>` and creates the GitHub release (a prerelease exactly when the version has a suffix).

The branch decides the version:

- `next`: prereleases. A line starts at `x.y.z-alpha.0` and counts up; a change that needs a higher target (`feat` or breaking: next minor before 1.0; anything else: next patch) opens a new line at `alpha.0`. Moving a line to beta, then rc, is an `x-channel` record in the config (`{"line": "x.y.z", "channel": "beta"}`), one step at a time; the next run cuts `x.y.z-<channel>.0`.
- `main`: stable. Merging `next` into `main` with the package at `x.y.z-rc.N` cuts `x.y.z`. Once stable, `main` takes `fix` and `perf` patches only; a `feat` or breaking change there blocks the release. An alpha or beta on `main` is skipped.
- `release/<major>.<minor>`: patch releases only.

Benchmarks: the last step before merging any release PR is `make bench` on the release PR's base branch (`next`, `main` or `release/<major>.<minor>`), its staged results committed there as `chore(bench): …` through the usual PR. Not on the release PR branch: release-please recreates that branch from its base on every run and drops commits pushed to it. The bench commit is hidden from the changelog and release-please refreshes the release PR on top of it. `make bench-check` passes when the inputs hash recorded in `bench/results/ben-meta.json` equals the commit's (`scripts/bench-inputs`: sources, lockfile, benchmark image, suites, configs, fixtures; the package's own version is excluded, so the release PR's bump does not make a run stale). Two places enforce it: the `bench-fresh` check (`.github/workflows/bench-fresh.yml`) runs on PRs from `release-please--*` branches and is a required check, and the tag build (below) runs it before creating the release. A stale run fails both with `benchmarks stale: run make bench`. Method: [bench/README.md](../bench/README.md#running).

`Release-As` footers are ignored and `release-as` is refused; the manifest is written by release PRs only. Release PRs wait for approval from `@hop-top/release` (`.github/CODEOWNERS` covers `CHANGELOG.md`). `release-please-preflight.yml` checks the release config on every PR.

Each `v*` tag runs `.github/workflows/release-rs-caller.yml` (release-please creates tags with the release-bot App token, so tag workflows run). Its `create-release` job runs `make bench-check`, checks that the tag version equals the `Cargo.toml` version and creates the release unless it already exists (`--prerelease` when the tag contains `-`). The `binaries` jobs then build `--release --locked` natively for `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`, `x86_64-apple-darwin` and `aarch64-apple-darwin`, and on `windows-2022` for `x86_64-pc-windows-msvc` and (cross-built) `aarch64-pc-windows-msvc`, and upload `bar-<version>-<target>.tar.gz` (containing `bar`), or `.zip` (containing `bar.exe`) for Windows, plus a `.sha256` file to that release. Linux binaries are built on `ubuntu-22.04` (glibc 2.35) and need glibc 2.35 or newer; aarch64 Linux builds on `ubuntu-22.04-arm`. The crate is not published to crates.io (`publish = false`); the workflow has no publish job.

Homebrew: `packaging/homebrew/bar.rb` is a template covering macOS and Linux, x86_64 and aarch64; it installs `bar`. After the release, set `version` (no leading `v`) and replace each `SHA256_<TARGET>` with the hex digest from the matching `.sha256` file, then copy the file into a tap's `Formula/`.
