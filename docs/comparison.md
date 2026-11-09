# Feature comparison

bar next to the three status lines its [benchmark](../bench/README.md) measures, at the versions the benchmark uses.

| Tool | Version | Source |
|---|---|---|
| bar | 0.1.0-beta.0 <!-- x-release-please-version --> | this repository |
| [ccstatusline](https://github.com/sirmalloc/ccstatusline/tree/v2.2.34) | 2.2.34 | `v2.2.34`: README, [docs/USAGE.md](https://github.com/sirmalloc/ccstatusline/blob/v2.2.34/docs/USAGE.md), [docs/WINDOWS.md](https://github.com/sirmalloc/ccstatusline/blob/v2.2.34/docs/WINDOWS.md) |
| [claude-powerline](https://github.com/Owloops/claude-powerline/tree/v1.32.8) | 1.32.8 | `v1.32.8`: README, [setup wizard](https://github.com/Owloops/claude-powerline/blob/v1.32.8/plugin/commands/powerline.md) |
| [CCometixLine](https://github.com/Haleclipse/CCometixLine/tree/v2.0.1) (`ccline`) | 2.0.1 | `v2.0.1`: README |

Checked 2026-11-09. Each cell is taken from the project's own documentation at that tag (for bar: [README](../README.md), [configuration](config.md), [plugins](plugins.md), [technical notes](technical.md)). "—" means the documentation describes no such feature; "not documented" means the feature area exists but the docs do not say how it behaves.

All four read Claude Code's status-line JSON and show the model, context-window use, session cost, session duration, the Claude Code version, the output style, the working directory, git state, and 5-hour and 7-day plan usage; all four can draw Nerd Font glyphs and lay the status line out on more than one line.

## Install and runtime

| | bar | ccstatusline | claude-powerline | ccline |
|---|---|---|---|---|
| Language, runtime | Rust, single binary | TypeScript on Node (`engines` ≥ 14) or Bun | TypeScript on Node 18+; no npm dependencies | Rust, single binary |
| Install | release archive, or `cargo install` from a clone; `/bar` setup skill from the Claude Code plugin marketplace | `npx` / `bunx`; the setup flow offers a pinned global install (npm or Bun) | `npx` / `bunx --bun`, or Claude Code plugin marketplace | npm (`@cometix/ccline`; yarn, pnpm, npm mirror), release archive, or `cargo build` |
| Platforms documented | macOS, Linux, Windows (click handler macOS only) | macOS, Linux, Windows (PowerShell, cmd, WSL) | not documented | Linux x64 and arm64 (glibc and static), macOS x64 and arm64, Windows x64 |
| Outside tools | `gh` (GitHub) or `glab` (GitLab) for PRs, merge requests and issues; `git` only as a fallback for unusual repo setups; `jj` 0.32+ for Jujutsu; `curl` for the price snapshot and the opt-in network features; `tmux` for `tmux` | `git`, `gh` or `glab`, `jj` | Git 2.0+ | Git; Claude Code 2.1.153+ (some metrics need newer) |

## Configuration and editing

| | bar | ccstatusline | claude-powerline | ccline |
|---|---|---|---|---|
| Config file | tmux-style text, `bar.conf` in the XDG config dir, plus a project `.bar.conf` up to the git root (`project-config` `off`, `safe` or `full`); `bar config export`, and `bar config import` with merge or replace after a diff preview | JSON, `~/.config/ccstatusline/settings.json` (`--config <path>`); export and import with replace or merge preview | JSON: `./.claude-powerline.json` (project), `~/.claude/claude-powerline.json`, XDG; reloads without restart | TOML, `~/.claude/ccline/config.toml` (`CCLINE_HOME`); own presets saved as files |
| Overrides | `default` lines per placeholder, inline options, `%$VAR%` and `%cmd%` in option values; project `.bar.conf` lines over yours | global overrides (padding, separator, bold, colours, minimalist mode); environment variables for width and context size | CLI flags over environment variables over config files | palette token overrides; apply a preset whole, layout only or look only |
| Editor | `bar edit`: full-screen TUI with live preview, fuzzy placeholder picker, option editor, undo and redo; web configurator (bar's renderer in WebAssembly, runs offline); `/bar` setup skill inside Claude Code | full-screen TUI (Ink): widget picker with fuzzy search, colour editor, live preview | — in the terminal; [Powerline Studio](https://powerline.owloops.com/) web configurator; `/powerline` wizard inside Claude Code | full-screen TUI (ratatui), six pages: live preview, undo and redo, command palette, Nerd Font icon picker |
| Connects to Claude Code | `bar install` writes `statusLine` (`--refresh` sets `refreshInterval`) and optionally `subagentStatusLine`, in user, project or local settings; shows the diff first, keeps a backup; `--uninstall` puts back the previous line | TUI installs and uninstalls `statusLine`, sets `refreshInterval` | wizard writes `settings.json` | configurator writes `statusLine`, `subagentStatusLine`, `refreshInterval`; shows the change first, keeps a backup |
| Presets | 6 (`bar init`) | built-in Powerline themes to copy (count not documented) | wizard templates: 3 segment presets, 3 TUI layouts | 8, plus your own |
| Preview | `bar preview`: sample data, three usage states, every preset, subagent rows; live in `bar edit` and the web configurator | live in the TUI | wizard preview script; live in Powerline Studio | live in the configurator; `ccline preview` with sample sessions |
| Validation | `bar check`: line-numbered errors, plugin option schemas, project lines left out as warnings, JSON output; a broken config renders `bar: config error, run bar check` | invalid file left untouched, defaults rendered with a warning badge; imports validated | not documented | broken config renders the default preset plus `! config` |

## Look

| | bar | ccstatusline | claude-powerline | ccline |
|---|---|---|---|---|
| Skins | plain, powerline, capsule | plain or Powerline mode (custom separators and caps, columns aligned across lines) | minimal, powerline, capsule, tui (bordered panel) | plain, bracket, pill, powerline |
| Palettes | 7: default, nord, gruvbox, dracula, solarized, mono, terminal | colours chosen per widget; built-in Powerline themes | 6: dark, light, nord, tokyo-night, rose-pine, gruvbox; or custom | 13, including `terminal`; own palette files |
| Colour per element | `fg`, `bg`, `bold` per placeholder (colour code, `#rrggbb`, ANSI name or palette role); palette and state colours | foreground, background, bold, dim per widget | `fg`, `bg`, `bold` per segment (custom theme) | `color`, `fg`, `icon_color` per item |
| Colour depth | none, 16, 256 (default), truecolor or auto; `NO_COLOR`, `FORCE_COLOR`, `COLORTERM`; ANSI 16 under `terminal` | 16, 256, truecolor | auto, ANSI, 256, truecolor; `NO_COLOR`, `FORCE_COLOR` | truecolor down to none |
| Thresholds, gradients | 11-step ramp; pace `rule`s; gradient fill on usage and context bars; OKLab gradients per placeholder or across every line (6 presets, or 2 to 8 colours) | gradients per widget or across the whole line (OKLab, 13 named presets); effort level colours | fixed tiers (budget warnings, cache timer) | per-item `levels`, coloured in `steps` or `gradient` |
| Bar glyphs | 7 sets on usage, context, budget and extra-usage bars: blocks, shades, dots, squares, bars, braille, eighths | full, medium, short and line bars | 10 bar styles on context, block and weekly | 6 sets: blocks, eighths, shapes, dots, braille, ascii; forms bar, gauge, sparkline, pace |
| Icons, fallback | `icons` none, nerdfont or emoji; `icon=` per placeholder; `icon-width`; `skin-glyphs ascii` | Nerd Font toggles and custom glyphs per widget; emoji in custom text | `charset` unicode or text; `showIcons`, per-segment `showIcon` | `glyphs` nerd, unicode or ascii; `icon` per item |

## Layout

| | bar | ccstatusline | claude-powerline | ccline |
|---|---|---|---|---|
| Alignment | left, center and right groups per line | flex separators right-align or fill | left-to-right flow; tui grid with columns, per-column alignment and width breakpoints | left and right groups per line |
| Lines | up to 64 lines; `max-rows` (1 to 64, default 4) caps lines and wrapped rows | unlimited | `display.lines` (no limit documented) | any number of `[[line]]` |
| Narrow terminal | width `COLUMNS` − 4; `wrap on` flows segments onto new rows; `max` truncation per placeholder | width `CCSTATUSLINE_WIDTH`, else `COLUMNS`, else probe, − 5; ellipsis truncation; max width per widget | `autoWrap` (on by default); `widthReserve` (45) | wraps between items; `compact` and `minimal` templates per item |
| Spacing | `separator`, `gap` | default padding (both, left or right side), default separator, widget merging | `padding` | `separator` |

## Session data

| | bar | ccstatusline | claude-powerline | ccline |
|---|---|---|---|---|
| Effort and modes | `effort`, `thinking`, `fast`, `vim`; `modes`: voice, sandbox, Remote Control at startup (from Claude Code settings) | thinking effort, fast mode, vim mode, voice, sandbox, remote control | thinking on or off, effort | effort with the model; `vim` |
| Model and window | display name without `Claude ` | strips context suffix; environment overrides for window size | `modelContextLimits` (Claude Code before 2.0.65 only) | `models.toml` registry: names and windows for third-party models |
| Context | `ctx`: percent, bar, gauge, tokens, room before auto-compaction (point worked out or `compact-at`), last request's cache split, growth and trend per request | length, window size, percent, usable percent before auto-compact, bar | text or bar styles; `autocompactBuffer` | text, bar or gauge; trend per turn, room before compaction, breakdown (companion mod) |
| Compactions, skills | counts of both | compaction counter (auto and manual split, tokens reclaimed); skills (last, count or list) | — | — |
| Tokens | in, out, total; time to first token and tokens per second (last, average) | input, output, cached, total; input, output and total speed | session tokens, breakdown | time to first token, tokens per second (companion mod) |
| Prompt cache | read, write, hit ratio; `cachettl` countdown (reported expiry when sent, TTL detected); `misses` with cause and last cause; tokens and cost if cold | hit rate, read, write; cache timer from reported expiry; misses with cause; tokens if cold | `cacheTimer`: elapsed or remaining, TTL detected | time until cold, hit ratio, misses; cost if cold |
| Cost rate | `burn` ($/h) | session cost rate | cost `calculated` or `official`; no rate | `cost.rate` (per hour) |
| Activity, identity | `lines`, `api` (API time), `msgs` (message count); `sid`; `name` (session name) | session clock; session id; session name | API time, last response time, message count, lines added and removed; session id | `lines`; `session_name`; running tools (companion mod) |

## Usage and spend

| | bar | ccstatusline | claude-powerline | ccline |
|---|---|---|---|---|
| 5h and 7d form | time, bar, percent, gauge, sparkline; click toggles bar and percent | percent or bar with time cursor; reset timers with timezone and locale | text or bar with reset countdown | two-row pace bar, text, gauge, sparkline |
| Per-model weekly | `usage[model=...]` with every 7d form (opt-in usage API) | Sonnet, Opus, Fable | — | — |
| Pace, forecast | colour by pace score, configurable `rule`s; `eta` (time to run out), `proj` (percent at reset) | block forecast, block and weekly limit timers | `showPace` (usage against an even spend) | ahead or behind the clock; `quota.eta` |
| Daily, monthly spend | `spendhist`: today and month from all local transcripts at list prices; `set price`; models.dev fallback; `extra`: extra usage this month and today (opt-in usage API) | extra usage today and this month (usage API) | `today`, `month` | `cost.today` (all sessions) |
| Budgets | `budget` for session, today, month and 5-hour block (hourly spend totals), with a warning threshold (`budget-warn`); budgets also colour `cost` and `spendhist`; extra usage daily budget (limit left ÷ days left) | extra usage daily budget (limit left ÷ days left) | budgets for session, today, month and block with warning thresholds | — |
| Spend limits, credits | `spend`: daily, weekly or monthly spend limit, time left and used, limit, left, percent or bar; extra usage limit, utilization, remaining; prepaid Console credits via a reference plugin (macOS) | spend limit usage and amount; extra usage limit, utilization, remaining | — | `quota.spend` |

## Git and project

| | bar | ccstatusline | claude-powerline | ccline |
|---|---|---|---|---|
| Git | `repo`, `branch` (links, truncation), `dirty` (staged, modified, untracked), `ahead` (ahead and behind), `sha`, `tag`, `stash`, `gitop` (operation in progress, with step), `conflicts`, `worktree`, `root`; read from files (`git` only as a fallback) | branch (links, fish-style), staged, unstaged, untracked, insertions, deletions, ahead and behind, conflicts, SHA, root, origin, upstream, fork, worktree | branch, SHA, tag, stash, operation, working-tree counts, ahead and behind, upstream, repo name, worktree, time since commit | `git`, `repo`, `worktree` (fields not documented) |
| PRs, issues, CI | open PRs coloured by checks and review, CI marks; issues; author, assignee and label queries (`gh`); GitLab merge requests and issues with pipeline marks (`glab`, self-hosted hosts too); latest workflow run via the `bar-ci` plugin | PR or MR with status and title (`gh`, `glab`); GitHub CI checks | — | `pr` |
| Jujutsu | `jj`: change, commit, bookmarks, workspace, state (conflict, divergent, empty), description | revision, bookmarks, workspace, changes, root, description | — | — |
| Directory | `cwd` (`~`, last component); `root` | home abbreviation, segment limit, fish-style, optional glyph | full, fish or basename; original repo path in `--worktree` sessions | `dir` with a `style` option |

## Agents and environment

| | bar | ccstatusline | claude-powerline | ccline |
|---|---|---|---|---|
| Agents, sessions | `agent`; `agents`: running subagents with types (hooks); `sessions`: live sessions working, waiting, finished; own row under each subagent (`subagentStatusLine`): status, name, task, model, tokens, elapsed time | — | `agent` name | `agent`; running agents (companion mod); own line under each background task (`subagentStatusLine`) |
| Account | `account`: email mapped to label and colour by glob; `api-key` | account email | via the `env` segment | — |
| Terminal, system | `tmux`; memory via the `bar-mem` plugin; Claude service status with 48 h incident strip (opt-in) | terminal width, memory, Claude service status with 48 h incident strip | `tmux`; any environment variable (`env`) | — |
| Custom text, commands | literal layout text; `%cmd%` values (cached 1 h, run in background) | custom text, symbol, command (stdin JSON, timeout, optional cache), link | shell composition in `statusLine.command` | `cmd`: first line of a shell command, cached, run in background |

## Extensibility

| | bar | ccstatusline | claude-powerline | ccline |
|---|---|---|---|---|
| Plugins | any `bar-<name>` executable on `PATH` is a placeholder: JSON protocol, option schemas, TTL cache, background runs, optional `web` metadata for previews in the web configurator and `bar edit`; 5 reference plugins (`tlc`, `claude-credits`, `ci`, `mem`, `health`) | a custom command can run another status-line tool | Claude Code plugin for the setup wizard | companion Claude Code mod feeds extra data, draws nothing |
| Hooks | `bar click install` (or `--hooks-only`) registers 12 Claude Code hook events: waiting state, subagents, task records | adds hooks for the skills widget when the command contains `ccstatusline` | — | `ccline mod install` |

## Performance

Measured times and memory: [Benchmarks](../README.md#benchmarks) and [method and raw results](../bench/README.md).

| | bar | ccstatusline | claude-powerline | ccline |
|---|---|---|---|---|
| Slow work | git state read from files; `gh`, `glab`, `git status`, `jj`, plugins, `%cmd%` and network fetches in a detached background refresh; transcripts read incrementally | git output cached with a TTL (runs on a miss, 5 s timeout); PR and CI from a disk cache refreshed in background; transcripts streamed | not documented | git in the background; `cmd` cached and run in the background |

## Interaction

| | bar | ccstatusline | claude-powerline | ccline |
|---|---|---|---|---|
| Links | repo, branch, commit, tag, PRs and merge requests, issues, service status | branch, remotes, PR, directory to file manager, VS Code, Cursor or Zed; link widget; status page | — | not documented |
| Clicks | toggles usage and plugin views; `model`, `effort`, `ctx` type `/model`, `/effort`, `/compact` (macOS, tmux) | — | — | — |

## Meta

| | bar | ccstatusline | claude-powerline | ccline |
|---|---|---|---|---|
| Licence | Apache-2.0 | MIT | MIT | MIT |
| Network, updates | core fetches the models.dev price snapshot on demand; opt-in: daily update check (`update`, `bar update`; never downloads), Claude status page, usage API; honours `HTTPS_PROXY`; `gh`, `glab` and plugins make their own calls | Anthropic usage and profile API, status.claude.com, npm registry update check; honours `HTTPS_PROXY` | not documented | daily update check, `update` metric, `ccline update`; never downloads |

## Not in bar

- No npm or npx install.
- No per-source context breakdown (system prompt, tools, memory files, messages); the status-line JSON and transcript do not carry it.
- No auto and manual compaction split or tokens reclaimed; skills as a count only.
- No running-tools display.
- Remote Control only when set to connect at startup; a session connected later is not detected.
- No git insertions and deletions, upstream or origin name, fork, or time since the last commit.
- No fish-style or segment-limited directory.
- No links to a file manager or editor.
- No bordered-panel layout or columns aligned across lines.
- No model registry for third-party model names and context windows.

## Updating this page

Re-check every cell against each project's docs at the new tag whenever `make bench` moves the benchmark to new rival versions, then update the versions, tag links and date at the top.
