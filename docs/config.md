# Configuration notes

## usage

`#{usage[type=5h|7d|h|d, show=time,bar, alt=time,pct, count=left|used, glyphs=blocks, width=5, click=toggle|none]}`

- `show` lists the components in order from `time`, `bar`, `pct`, `eta`, `proj` (`show=bar,pct`, `show=pct,time`); default `time,bar`. `eta` is the projected time until the budget runs out at the average burn so far (`→1h20m` in the bad colour when the projection is over 100%, a muted `→ok` when the budget lasts the window); `proj` is the projected used percent at reset (`p140%`, capped at 999): the bad colour above 100%, otherwise the bar's ramp (thresholds only, no time floors) on the projected remaining, never red. Both ignore `count`. Both hide while nothing is used or under 2% of the window has elapsed. `view` stays as an alias: `bar` is `time,bar`, `pct` is `time,pct`, `time` is `time`; `show` wins when both are set. `count=used` makes the percentage and the bar fill show used instead of remaining; the colour still follows the pace score of what remains. `glyphs`: `blocks` (█░), `shades` (▓░), `dots` (●○), `squares` (■□), `bars` (▰▱), `braille` (⣿⣀), `eighths` (█ with a ▏▎▍▌▋▊▉ last cell); an unknown name draws `blocks` and `bar check` flags it.
- `bar toggle` (a click on the bar or percentage) flips between `show` and `alt`; `alt` defaults to `show` with `bar` and `pct` swapped. Time, `eta` and `proj` stay outside the link. Expired or missing windows render nothing.
- The bar is one colour: every filled cell takes the colour of the exact value, the same as `pct`; empty cells are muted. The colour follows the pace score of `rule usage[type=5h]` / `rule usage[type=7d]`; without a rule the built-ins apply (`early warn=20% bad=5%`, final `1h` / `1d` with `bad=30%`).
- Toggle state: `pct-5h` / `pct-7d` files (mode 0600) in the state directory (the kit state dir for `bar`; `BAR_STATE_DIR` overrides it, meant for tests). While a file exists, `bar` and `pct` swap. `bar toggle <5h|7d>` flips it.
- Link `bar://toggle/<key>` is emitted only when the handler app is installed (marker `click-installed` in the state directory) and `click` is not `none`.

## tokens, cache, cachettl

`#{tokens[show=in|out|total]}`, `#{cache[show=read|write|hit]}`, `#{cachettl[ttl=300]}`

Read from the `transcript_path` in the session JSON (assistant messages' `usage`). Nothing renders without a readable transcript.

- `tokens`: in is `input_tokens` plus `cache_creation_input_tokens`; out is `output_tokens`. `cache`: read is `cache_read_input_tokens`, write is `cache_creation_input_tokens`, hit is read over read plus input plus write. Numbers are compact: `999`, `12.3k`, `80k`, `1.2M`, `2.5B`.
- A message written as several lines (one per content block) counts once, by `message.id`.
- `cachettl`: `ttl` minus the time since the last assistant message (the transcript mtime when there is none), as `4m12s`; `cold` in muted text once it is over. Green above 60 s left, then the warn colour.
- Totals are incremental. A state file per transcript (mode 0600, `transcript/` in the state directory) keeps the byte offset and running totals; a render reads only the bytes appended since. A smaller file, a different inode, or a changed mtime at the same size rescans from the start.
- A render reads at most 1 MiB. On a bigger backlog it shows the totals so far and a background `bar refresh` finishes the scan, so the numbers catch up within a few renders. Lines over 512 KiB and lines that are not valid JSON are skipped.

## spendhist

`#{spendhist[period=today|month|both,tz=local|utc]}`

Estimated spend for today and the calendar month: `today $3.12 · mon $84.10` (the labels muted), or one bare `$3.12` for `period=today` / `period=month`. This is API-equivalent cost computed from token counts at list prices. It is not subscription billing and will not match an invoice.

- Source: every `<claude dir>/projects/*/*.jsonl`. The dir is `set claude-dir <path>`, else `CLAUDE_CONFIG_DIR`, else `~/.claude`. The path must be absolute (`~/` is expanded); a relative `claude-dir` or `CLAUDE_CONFIG_DIR` is ignored and the placeholder renders empty. `bar check` warns about a relative `claude-dir`. Assistant lines' `message.usage`, `message.model` and `timestamp` count once per `message.id` plus `requestId`, with timestamps after now counted as now, across all files (a resumed session repeats earlier messages).
- Days are local dates; `tz=utc` buckets by UTC date. Default `local`.
- A background job does the scanning, at most once per 60 s (a failed run waits 60 s). A render only reads the finished summary, so it renders empty until the first run completes. The first run reads the newest transcripts first and stops at the refresh time limit; later runs continue, so a large history catches up over a few minutes.
- State: `spendhist/<hash>.json` in the state directory (mode 0600, written atomically): per-transcript byte offsets, tokens per day and model for the last 62 days, and the hashes of counted messages. If the file would pass 16 MiB the marks of the least recently modified transcripts and then the oldest hashes are dropped; totals are kept, but a transcript whose mark was dropped is read again.
- Cost per message: input, output, cache write and cache read tokens times the model's rates. Cache writes use the 5-minute rate for all cache-creation tokens. A model without a price counts as tokens but adds no dollars, and the amount gets a trailing `~`. Model ids over 128 characters or with unusual characters, and a day's models past the 63rd, are counted as `other`, which never has a price.
- Prices: built in for the Opus, Sonnet, Haiku, Fable and Mythos families, matched by exact id as below. Source: the Anthropic pricing page (platform.claude.com/docs/en/about-claude/pricing), read 2026-11-07; A model id matches an entry when it equals it or adds `-YYYYMMDD`; an unlisted version (say a newer minor release) has no price and shows `~` until you add a `set price`. Haiku 5.5 requests whose prompt (input plus cache tokens) passes 100,000 tokens are priced at five times the rates, as the page lists; a `set price` rule covers both tiers. Override or add with `set price <model-prefix> <in> <out> <cache_write> <cache_read>`, dollars per million tokens, for example `set price claude-opus-4-5 5 25 6.25 0.5`. An override beats the built-in table whatever its length; among overrides the longest prefix wins, a later line breaking ties.

## Click handler (macOS)

`bar click install` builds `~/Applications/bar-click.app` (`BAR_APP_DIR` overrides the directory) from an AppleScript `on open location` handler, sets `LSUIElement` and the `bar` URL scheme with `plutil`, signs it ad hoc, registers it with `lsregister`, then writes the marker. Elsewhere it writes nothing, prints that it needs macOS and exits 2; `--hooks-only` registers just the hooks on any platform. `--dry-run` prints the script and commands and changes nothing.

CI covers the generated script, plist edits, command list and platform guard with fake runners. The install itself is checked by hand:

1. `bar click install --dry-run`: read the script and commands.
2. `bar click install`.
3. `open bar://toggle/5h`: no Dock icon appears; `pct-5h` shows up in the state directory.
4. `open bar://toggle/5h` again: the file is gone.
5. `open bar://toggle/zz`: nothing changes.
6. In a Claude Code session, Cmd-click a `usage` bar (iTerm2): the view flips on the next refresh.

## Config file

Search order, first existing wins:

1. `$XDG_CONFIG_HOME/bar/bar.conf` (ignored when relative)
2. `~/.config/bar/bar.conf`
3. kit config dir for `bar` (macOS: `~/Library/Application Support/bar/bar.conf`)

No file: built-in layout. A file with an error makes `bar render` print `bar: config error, run bar check`.

## Directives

| Directive | Form |
|-----------|------|
| `set` | `set <key> <value>`; keys `left center right separator gap wrap refresh icons icon-width skin skin-glyphs palette claude-dir color.good color.warn color.bad ramp`; `set price <model-prefix> <in> <out> <cache_write> <cache_read>` takes five values |
| `default` | `default <placeholder> key=value ...` |
| `account` | `account "<glob>" <label> <color>` |
| `rule` | `rule usage[type=5h] early warn=20% bad=5% final=1h bad=30%` |
| `plugin` | `plugin <name> key=value ...` |

Quote values with spaces. `#` starts a comment.

- `separator`: changes only where segments split (the text a layout is cut at). Output between segments stays ` │ `. Default `│`.
- `wrap`: `auto`, `on` or `off`; reserved, only `auto` behaviour exists.
- `refresh`: default ttl floor for plugins, default `5s`. `gh` lists use 120 s and `%cmd%` values 1 h, whatever this says.

- `icons`: `none` (default, output unchanged), `nerdfont` or `emoji`. See [Icons](#icons).
- `icon-width`: `1` (default) or `2`; cells a Nerd Font glyph (private-use codepoint) takes in your font. Layout uses it to pad; emoji widths come from Unicode.
- `skin`: `plain` (default), `powerline` or `capsule`. See [Skins](#skins).
- `skin-glyphs`: `nerd` (default) or `ascii`.
- `palette`: `default`, `nord`, `gruvbox`, `dracula`, `solarized` or `mono`. See [Palettes](#palettes).

## Placeholders

`#{name[key=value,key=value]}`. Inline options override the matching `default` line. Defaults below are what the code applies when an option is unset.

| Placeholder | Option | Default | Meaning |
|---|---|---|---|
| `prs` | `limit` | `5` | Most items shown; `0` hides the placeholder |
| | `author` | none | [Value logic](#label-logic), e.g. `@me` |
| | `status` | `open` | `open`, `closed`, `merged` |
| | `link` | on | `false`, `no`, `off`, `0` give plain text |
| `issues` | `limit` | `5` | As `prs` |
| | `assignee`, `author` | none | Value logic |
| | `label` | none | Value logic |
| | `status` | `open` | `open`, `closed` |
| | `link` | on | As `prs` |
| `dirty` | `show` | all | Comma list of `staged`, `modified`, `untracked`, e.g. `show=staged,modified`. Output `+2 ~3 ?1`; each part omitted at 0, hidden when clean. Unmerged paths count as modified; a `+` suffix on a count (`?2000+`) marks a lower bound when git's output was cut at 64 KiB |
| | `ttl` | `10` | Seconds before the cached `git status` is refreshed in the background (minimum 1) |
| `ahead` | `ttl` | `10` | As `dirty`. Output `↑2 ↓1`; hidden at 0/0 or without an upstream |
| `worktree` | none | | Directory name of a linked worktree; hidden in the main worktree |
| `repo`, `branch` | `link` | on | Link to the repository / branch page |
| | `max` | none | Truncate to this many characters with `…` |
| `usage` | `type` | `5h` | `5h`, `7d`; `h`, `d` shorthand |
| | `view` | `bar` | `bar`, `pct`, `time`; alias of `show` |
| | `show` | `time,bar` | list of `time`, `bar`, `pct`, `eta`, `proj` |
| | `alt` | `show`, bar and pct swapped | list of `time`, `bar`, `pct`, `eta`, `proj` |
| | `count` | `left` | `left`, `used` |
| | `glyphs` | `blocks` | `blocks`, `shades`, `dots`, `squares`, `bars`, `braille`, `eighths` |
| | `width` | `5` | Bar cells, clamped to 1..40 |
| | `click` | `toggle` | `none` turns the toggle link off; the link appears only once the handler is installed (`bar click install`) |
| `tokens` | `show` | both | `in`, `out`, `total`; default `12.3k↑ 4.1k↓` |
| `cache` | `show` | `read` and `write` | `read`, `write`, `hit`; default `r 1.2M w 80k`, `hit` is `hit 93%` |
| `cachettl` | `ttl` | `300` | Seconds the prompt cache lives (minimum 1); `3600` for the 1 h cache |
| `agents` | `names` | `0` | `1` appends the running subagents' types |
| | `max` | none | Truncate the names with `…` |
| `spendhist` | `period` | `both` | `today`, `month`; see [spendhist](#spendhist) |
| | `tz` | `local` | `utc` buckets days by UTC date |
| `account` | none | | Signed-in account (`oauthAccount.emailAddress` in `.claude.json`): the label and colour of the first matching `account` line, else the email in grey; `api-key` when only `ANTHROPIC_API_KEY` is set; hidden otherwise |
| `sessions`, `spend` | none | | No options |
| every placeholder, plugins included | `icon` | set default | A glyph (at most 2 cells), or `none` to hide it. See [Icons](#icons) |
| | `bg` | palette | Segment background, `0`-`255`, under a `powerline` or `capsule` skin. See [Skins](#skins) |
| `cost` | `fmt` | `usd` | Session cost, `$1.23`; `<$0.01` under a cent. Hidden when absent |
| `burn` | none | | Cost per hour, `$4.10/h`; hidden for the first 60 s of the session |
| `duration` | none | | Session time: `45s`, `12m`, `1h05m` |
| `lines` | none | | `+120 -14`; hidden when both are 0 |
| `version`, `vim`, `agent` | none | | Claude Code version, vim mode, agent name; hidden when absent |
| `style` | none | | Output style; hidden when `default` |
| `thinking`, `fast` | none | | `think` / `fast` while enabled; hidden otherwise |
| `cwd` | `base` | `0` | Session directory with home as `~`; `1` shows the last component only |
| `root` | `base` | `0` | Git top-level directory of the session directory, read without running `git`; hidden outside a repository |
| `model`, `effort`, `ctx` | `click` | on | `none` turns the command link off; the link appears only once the handler is installed (`bar click install`) |

`cost`, `burn`, `duration`, `lines`, `version`, `style`, `vim`, `agent`, `thinking` and `fast` read the statusline JSON fields `cost.total_cost_usd`, `cost.total_duration_ms`, `cost.total_lines_added`, `cost.total_lines_removed`, `version`, `output_style.name`, `vim.mode`, `agent.name`, `thinking.enabled` and `fast_mode`. Missing, mistyped, negative or non-finite values hide the placeholder. `cost` is the session's own cost; `spend` is the account spend-limit window. None of these are in the default layout.

## Icons

`set icons nerdfont|emoji` adds an icon to every core placeholder (table below); `none` (default) leaves output as is. An icon is drawn muted, followed by one space, outside any link. When a segment has a text label (`ctx`, `agents`), the icon replaces it; otherwise it prefixes the value. `icon=<glyph>` on any placeholder beats the set, `icon=none` hides it; both work with `icons none`. Glyphs are sanitised; `bar check` rejects an icon wider than 2 cells (counted with `icon-width`); values from `%…%` expressions or `default` lines are not checked ahead, and an over-wide one is dropped at render (no icon, label kept).

| Placeholder | nerdfont | emoji |
|---|---|---|
| `model` | `nf-fa-microchip` U+F2DB | 🤖 |
| `effort` | `nf-fa-bolt` U+F0E7 | ⚡ |
| `ctx` | `nf-fa-tachometer` U+F0E4 | 🧠 |
| `repo` | `nf-oct-repo` U+F401 | 📁 |
| `branch` | `nf-oct-git_branch` U+F418 | 🌿 |
| `prs` | `nf-oct-git_pull_request` U+F407 | 🔀 |
| `issues` | `nf-oct-issue_opened` U+F41B | 🐛 |
| `usage` 5h | `nf-fa-clock_o` U+F017 | ⏳ |
| `usage` 7d | `nf-fa-calendar` U+F073 | 📅 |
| `account` | `nf-fa-user` U+F007 | 👤 |
| `sessions` | `nf-fa-terminal` U+F120 | 💬 |
| `spend` | `nf-fa-dollar` U+F155 | 💰 |
| `spendhist` | `nf-fa-money` U+F0D6 | 💵 |
| `tokens` | `nf-fa-exchange` U+F0EC | 🔢 |
| `cache` | `nf-fa-database` U+F1C0 | 💾 |
| `cachettl` | `nf-fa-hourglass_end` U+F253 | 🕐 |
| `dirty` | `nf-fa-pencil` U+F040 | ✏️ |
| `ahead` | `nf-fa-arrow_up` U+F062 | ⬆️ |
| `worktree` | `nf-fa-tree` U+F1BB | 🌳 |
| `agents` | `nf-fa-users` U+F0C0 | 👥 |
| `cost` | `nf-fa-credit_card` U+F09D | 💳 |
| `burn` | `nf-fa-fire` U+F06D | 🔥 |
| `duration` | `nf-fa-history` U+F1DA | ⌚ |
| `lines` | `nf-fa-code` U+F121 | 📝 |
| `version` | `nf-fa-tag` U+F02B | 🔖 |
| `style` | `nf-fa-paint_brush` U+F1FC | 🎨 |
| `vim` | `nf-dev-vim` U+E7C5 | ⌨️ |
| `agent` | `nf-fa-user_secret` U+F21B | 🎭 |
| `thinking` | `nf-fa-lightbulb_o` U+F0EB | 💭 |
| `fast` | `nf-fa-rocket` U+F135 | 🚀 |
| `cwd` | `nf-fa-folder_open` U+F07C | 📂 |
| `root` | `nf-fa-home` U+F015 | 🏠 |

Plugins: see [plugins](plugins.md).

## Skins

`set skin plain|powerline|capsule` picks how segments are drawn.

- `plain` (default): segments joined by a muted ` │ `; output is unchanged from earlier versions.
- `powerline`: each segment sits on a background colour with one space of padding each side. Left and centre groups are joined by U+E0B0, the right group by U+E0B2; each arrow takes the background it leaves as its colour and the one it enters as its background, so the shapes flow out of the left and centre groups and into the right one from the edge. Neighbours that share a background are split by a thin muted U+E0B1 (left, centre) or U+E0B3 (right) instead.
- `capsule`: each segment is wrapped in U+E0B6 … U+E0B4 on its background, one space between capsules.

Powerline and capsule glyphs need a Nerd Font or a Powerline-patched font. `set skin-glyphs ascii` draws `>` / `<` (powerline; `|` between same-background neighbours) and `(` / `)` (capsule) instead, for any font.

Backgrounds come from the palette's list, in order across the whole line (left, centre, right) and cycled. `bg=<0-255>` on a placeholder sets its segment's background; in a segment holding several placeholders the first `bg` wins. `plain` ignores `bg`. `bg` is reserved on every placeholder, plugins included; plugins never see it, and `bar check` warns when a plugin declares it. `bar check` rejects a value outside 0-255.

Colours inside a segment (usage ramp, `ctx` colour, state colours) still apply on top of the background. A reset inside a segment value restores the segment background; the background ends (`49`) only where the segment ends. Every skin glyph counts as one cell, whatever `icon-width` says, and layout counts the padding and glyphs when it fits groups to `COLUMNS`. When left and right do not fit beside each other they are joined by one space.

## Palettes

`set palette <name>` sets the muted text colour, the state colours (`color.good`, `color.warn`, `color.bad`), the usage `ramp` and the background list skins use.

| Palette | muted | good | warn | bad | backgrounds |
|---|---|---|---|---|---|
| `default` | 250 | 114 | 179 | 203 | 238 24 236 60 23 |
| `nord` | 248 | 144 | 222 | 167 | 237 24 239 67 60 |
| `gruvbox` | 246 | 142 | 214 | 167 | 237 239 66 241 95 |
| `dracula` | 146 | 84 | 228 | 203 | 236 61 238 97 239 |
| `solarized` | 245 | 106 | 136 | 160 | 23 235 24 30 66 |
| `mono` | 245 | 252 | 248 | 255 | 236 238 240 242 |

`default` keeps today's colours, ramp included (`196 … 46`). The other palettes use a three-step ramp from their own colours: bad for scores up to 20, warn up to 60, good above (the ramp is indexed by `(score + 9) / 10`). An explicit `set color.good|warn|bad` or `set ramp` beats the palette wherever it appears in the file. Values are 256-colour codes; no gradients.

## Presets

Five example configs ship in the binary and in [`presets/`](../presets): `minimal`, `full`, `powerline`, `capsule`, `compact`.

- `bar preview --preset <name>` renders one with sample data, ignoring your config.
- `bar init [--preset <name>]` writes one (default `full`) to the first search path: `$XDG_CONFIG_HOME/bar/bar.conf`, else `~/.config/bar/bar.conf`. It refuses when any search path already holds a config, prints the path it found and exits 2. It also refuses when that path would be relative (`HOME` unset or relative); a relative `XDG_CONFIG_HOME` is ignored, as the XDG spec says, here and in the search order. The file is written to a temp file beside it and linked into place, so it is never partial and never replaces a file that appeared meanwhile. There is no `--force`; remove or move the file yourself.

## Values

Option values expand before use.

- `@me`: passed to `gh` as the search value; GitHub resolves it to the logged-in user. It is the default for "mine" in the built-in layout.
- `%$VAR%`: environment variable; unset gives an empty value.
- `%cmd%`: run through `sh` with a 3 s limit. The output is cached for 1 h, keyed by the command text only, so it is shared by every session and directory. It is stored in a 0600 cache file: do not print secrets. A cold cache renders empty and refreshes in the background.

## Label logic

`author`, `assignee` and `label` take expressions. `&` binds tighter than `|`; parentheses group; `!` negates; quote values holding spaces.

| Written | GitHub query |
|---|---|
| `label=a\|b` | `label:a,b` |
| `label=a&"b c"` | `label:a label:"b c"` |
| `label=(a\|b)&c` | `label:a,b label:c` |
| `label=!a` | `-label:a` |
| `label=a&b\|c` | one query per `\|` branch |

Values holding spaces, commas or colons are quoted in the query (`area:core` becomes `label:"area:core"`). Only `label` folds an OR into `label:a,b`; `author` and `assignee` run one query per OR branch. Results of all queries are merged and deduplicated by number, newest first, then cut to `limit`. At most 8 queries run per placeholder; `bar check` warns when more are written.

## bar check

`bar check` lints the config without rendering and prints the file it read (or says the built-in layout is in use).

- Parse errors with line numbers. An unreadable file is one error without a line.
- Layout placeholders (`left`, `center`, `right`) and `default` lines: unknown placeholder, unknown option, bad option value. Plugin placeholders are checked against their `--ext-info` schema.
- `prs` / `issues`: invalid `limit`, invalid `author` / `assignee` / `label` expression.
- Warnings: `gh` not on `PATH` while `prs` or `issues` is used; a `default` line for a plugin that is not on `PATH` (a layout using it is an error); an expression that expands to more than 8 queries (only the first 8 run).
- Options holding `%command%` or `%$VAR%` are skipped; they resolve at render time.

Exit 0 when there are no errors (warnings allowed), 2 on a config error. `--format json` prints `{path, ok, issues: [{level, line, message}]}`; `line` is 0 when an issue has no line.

## bar preview

`bar preview [--state early|burning|final] [--preset <name>]` renders your layout (or a [preset](#presets)) with built-in sample data for every core placeholder: a sample session (model, effort, context, usage windows, spend limit, cost, duration, lines, version, style, vim mode, agent, thinking, fast mode) and stand-ins for the rest (repo, branch, git state, worktree, PRs, issues, account label, subagents, open sessions, spend history, transcript tokens and cache, directories). In the `final` state the sample tree is clean, so `dirty` is empty. It reads no session, account, git repository, `gh` or plugin, and writes no state. Plugin placeholders render empty.

- `early`: plenty of budget; bars green.
- `burning`: spending fast; bars orange to yellow.
- `final`: last stretch with budget left over; bars red.

Width comes from `COLUMNS`, else 120.

## Clickable commands

`model`, `effort` and `ctx` link to a slash command typed into the session they belong to: `/model`, `/effort`, `/compact`. On by default; off with `click=none` or while the handler cannot route them: the `click-installed` marker must read `cmd` (written by `bar click install`); a marker from a toggle-only install keeps usage toggles and no command links, until `bar click install` is run again.

- Link: `bar://cmd/<command>?tty=<ttysN>&sid=<session id>`. Emitted only on macOS, and only when the parent process has a `ttysN` terminal and the session id is lowercase hex and dashes.
- Allowlist: exactly `model`, `effort`, `compact`, fixed in core. Options, config and plugins cannot add to it; any page or app can open a `bar://` URL, so the allowlist and the checks below are the boundary.
- `bar click run <url>` (the handler calls it) fails closed. It refuses a malformed link (scheme, host and path exact; `tty` and `sid` once each and nothing else; no encoded or control characters), a command off the list, a session that is missing, dead or on another terminal, no tmux pane showing the session, a busy session, a session waiting on a permission prompt, and a non-empty input box (draft or open menu). A refusal posts a notification, appends to `click.log` in the state directory and exits 1 (the handler app wraps the call in `try`, so no error dialog appears); it never touches the terminal.
- Pane: the one on the session's own terminal, else the pane whose screen carries `sid=<id>` (a background session shown through another client). A pane found by id must still show it on the second pass.
- Idle and empty box are checked twice, a second apart, then `tmux send-keys -l "/<command>"` and `Enter`.
- Empty box: a box opens where a border (only `─`, at least 20 cells) sits right above a line holding the `❯` prompt marker. The real box always opens that way, so a draft that types a fake box adds a second opening, and ambiguity refuses: there must be exactly one opening on screen and no `❯` line below its marker; the very next line must be the bottom border (same width, and the last such line on screen), so any draft of more than one line refuses; and after the marker only dim placeholder text may follow (outside dim spans only whitespace and SGR codes; reverse video refuses; `\e[0m`, `\e[00m` and `\e[m` all reset). The parser reads screen output: `tests/fixtures/sessions/synthetic/` holds hand-made panes (`empty*.ansi` must parse as empty, every other file must refuse); real captures go in a dir named after the Claude Code version (`claude --version`), and the test checks every dir there.
- Waiting state: `bar click hook` reads Claude Code hook JSON on stdin and keeps `sessions/<session id>` (mode 0600) in the state directory: set on a permission request, permission or elicitation notification, `AskUserQuestion` or a failed stop; cleared on prompt submit, session end, and the main agent's stop or tool result.
- Running subagents: the same hook keeps `agents/<session id>/<agent id>` (mode 0600, content the agent type) on `SubagentStart`, removes it on `SubagentStop`, and drops the session's directory on `SessionEnd`. `#{agents}` counts entries younger than 6 hours, so a subagent that died without a stop event ages out; the flip side is that a live agent running longer than 6 hours drops out of the count. Expired entries and orphan temp files are pruned on the next `SubagentStart`. Only agent ids of 1 to 64 characters from `A-Z a-z 0-9 - _` are tracked; others are ignored. Claude Code's statusline JSON has no subagent field and its transcript has no cheap running signal, so `#{agents}` needs the hooks registered: `bar click install` (macOS) or `bar click install --hooks-only` (any platform, hooks only, no app). Both write `<claude dir>/settings.json`, the dir being `CLAUDE_CONFIG_DIR` or `~/.claude`; they refuse (exit 2) when `HOME` is unset or relative, or `CLAUDE_CONFIG_DIR` is relative. Internal agents (prompt suggestions, side questions) also fire these events and count while they run.

`bar click install` also wires the hook: it adds a `'<bar>' click hook` command entry under `hooks` in Claude Code's `settings.json` (`CLAUDE_CONFIG_DIR`, else `~/.claude`) for each event above that lacks one, prints every addition before writing, re-reads the file right before writing (after the app is built) so a concurrent edit is kept, copies it to a unique `settings.json.bak-<time>-<pid>`, and writes atomically, keeping the file's mode. A symlinked `settings.json` is written through to its target; a link to a missing file is refused. A file it cannot parse as Claude Code settings is left untouched and the install stops. `--dry-run` prints the additions and writes nothing.

CI covers links, parsing, every refusal, pane choice, the double check and the keys sent with a fake environment; nothing in the tests runs tmux, `osascript` or `ps`. Checked by hand, in a Claude Code session inside tmux after `bar click install`. During each check, save the pane with `tmux capture-pane -p -e -t <pane> > tests/fixtures/sessions/<version>/<name>.ansi` (`empty.ansi`, `draft.ansi`, `menu.ansi`, ...) and commit it, so the parser is tested against the real screen of that version:

1. Click `model`, `effort` and `ctx` with an empty input box: `/model`, `/effort` and `/compact` are typed and run.
2. Type a draft without sending it, then click `model`: a "not sent" notification appears, the draft is untouched, and `click.log` reads `the input box is not empty`.
3. Trigger a permission prompt (a tool call needing approval), then click `model`: a "not sent" notification appears, the prompt is untouched, and `click.log` reads `session is waiting for you`.
