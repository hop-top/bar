# Configuration

How to write `bar.conf`: where it lives, the directives, every placeholder and its options, styling, layout and the commands around the config. Pictures of skins, palettes, bar glyphs, pace states, icons and PR checks: [Gallery](gallery.md).

- [Overview](#overview): [config file](#config-file), [directives](#directives), [minimal example](#minimal-example)
- [Placeholders](#placeholders): [session](#session), [usage](#usage), [tokens, cache, cachettl](#tokens-cache-cachettl), [spendhist](#spendhist), [git and directories](#git-and-directories), [prs and issues](#prs-and-issues), [account, agents, sessions, tmux](#account-agents-sessions-tmux), [every placeholder](#options-on-every-placeholder), [values](#values)
- [Styling](#styling): [skins](#skins), [palettes](#palettes), [icons](#icons), [glyphs](#glyphs), [colours](#colours)
- [Layout](#layout): [lines and wrapping](#lines-and-wrapping)
- [Commands](#commands): [bar check](#bar-check), [bar preview](#bar-preview), [presets and bar init](#presets), [click handler](#click-handler-macos), [clickable commands](#clickable-commands)

## Overview

### Config file

Search order, first existing wins:

1. `$XDG_CONFIG_HOME/bar/bar.conf` (ignored when relative)
2. `~/.config/bar/bar.conf`
3. kit config dir for `bar` (macOS: `~/Library/Application Support/bar/bar.conf`; Windows: `%LOCALAPPDATA%\bar\bar.conf`)

`~` is the home dir: `HOME`; on Windows `USERPROFILE`, else `HOME`.

No file: built-in layout. A file with an error makes `bar render` print `bar: config error, run bar check`.

### Directives

| Directive | Form |
|-----------|------|
| `set` | `set <key> <value>`; keys `left center right` (and `left@N center@N right@N`, N 1-4) `separator gap wrap refresh icons icon-width skin skin-glyphs palette claude-dir color.good color.warn color.bad ramp`; `set price <model-prefix> <in> <out> <cache_write> <cache_read>` takes five values |
| `default` | `default <placeholder> key=value ...` |
| `account` | `account "<glob>" <label> <color>` |
| `rule` | `rule usage[type=5h] early warn=20% bad=5% final=1h bad=30%` |
| `plugin` | `plugin <name> key=value ...` |

Quote values with spaces. `#` starts a comment.

| `set` key | Values |
|---|---|
| `left`, `center`, `right` | Layout of line 1; `left@N`, `center@N`, `right@N` for lines 2-4. See [Lines and wrapping](#lines-and-wrapping) |
| `separator` | Changes only where segments split (the text a layout is cut at). Output between segments stays ` │ `. Default `│` |
| `wrap` | `off` (default) or `on`; see [Lines and wrapping](#lines-and-wrapping). `auto` is read as `off` |
| `refresh` | Default ttl floor for plugins, default `5s`. `gh` lists use 120 s and `%cmd%` values 1 h, whatever this says |
| `icons` | `none` (default, output unchanged), `nerdfont` or `emoji`. See [Icons](#icons) |
| `icon-width` | `1` (default) or `2`; cells a Nerd Font glyph (private-use codepoint) takes in your font. Layout uses it to pad; emoji widths come from Unicode |
| `skin` | `plain` (default), `powerline` or `capsule`. See [Skins](#skins) |
| `skin-glyphs` | `nerd` (default) or `ascii`. See [Glyphs](#glyphs) |
| `palette` | `default`, `nord`, `gruvbox`, `dracula`, `solarized`, `mono` or `terminal`. See [Palettes](#palettes) |
| `claude-dir` | Claude Code config dir read by [spendhist](#spendhist) |
| `color.good`, `color.warn`, `color.bad`, `ramp` | State colours and usage ramp. See [Colours](#colours) |
| `price` | Model price override for [spendhist](#spendhist) |

### Minimal example

The `minimal` [preset](#presets): model, context and the 5-hour window.

```text
set left "#{model} │ #{ctx}"
set right "#{usage[type=5h]}"
```

## Placeholders

`#{name[key=value,key=value]}`. Inline options override the matching `default` line. Defaults below are what the code applies when an option is unset. Plugins: see [plugins](plugins.md).

### Session

Fields of the session: model, effort, context, cost and the rest of the statusline JSON.

| Placeholder | Option | Default | Meaning |
|---|---|---|---|
| `model`, `effort`, `ctx` | `click` | on | `none` turns the command link off; the link appears only once the handler is installed (`bar click install`). See [Clickable commands](#clickable-commands) |
| `cost` | `fmt` | `usd` | Session cost, `$1.23`; `<$0.01` under a cent. Hidden when absent |
| `burn` | none | | Cost per hour, `$4.10/h`; hidden for the first 60 s of the session |
| `duration` | none | | Session time: `45s`, `12m`, `1h05m` |
| `lines` | none | | `+120 -14`; hidden when both are 0 |
| `version`, `vim`, `agent` | none | | Claude Code version, vim mode, agent name; hidden when absent |
| `style` | none | | Output style; hidden when `default` |
| `thinking`, `fast` | none | | `think` / `fast` while enabled; hidden otherwise |
| `spend` | none | | No options |
| `sid` | `len` | `8` | First `len` chars of the session id; hidden when absent or `len=0` |
| `compactions` | none | | `compact 2`: context compactions in this session's transcript; hidden at 0 |
| `skills` | none | | `skills 3`: Skill tool calls on the main conversation in the transcript; hidden at 0 |

- `cost`, `burn`, `duration`, `lines`, `version`, `style`, `vim`, `agent`, `thinking` and `fast` read the statusline JSON fields `cost.total_cost_usd`, `cost.total_duration_ms`, `cost.total_lines_added`, `cost.total_lines_removed`, `version`, `output_style.name`, `vim.mode`, `agent.name`, `thinking.enabled` and `fast_mode`. Missing, mistyped, negative or non-finite values hide the placeholder. None of these are in the default layout.
- `cost` is the session's own cost; `spend` is the account spend-limit window.
- `sid` reads `session_id`; a non-numeric `len` renders as 8 and `bar check` rejects it.
- `compactions` counts `system` transcript entries with subtype `compact_boundary`; `skills` counts `Skill` tool calls in non-sidechain assistant entries (skills a user starts with a slash command are not counted). Both share the transcript scan of [`tokens`](#tokens-cache-cachettl), so they are empty without a readable `transcript_path`.
- The statusline JSON carries no voice-mode, sandbox or skill field, so there are no placeholders for them.

### usage

The 5-hour or weekly window: time to reset, a bar of what is left, percentages, projections and a sparkline, coloured by pace.

`#{usage[type=5h|7d|h|d, show=time,bar, alt=time,pct, count=left|used, glyphs=blocks, color=solid|gradient, width=5, click=toggle|none]}`

| Option | Default | Meaning |
|---|---|---|
| `type` | `5h` | `5h`, `7d`; `h`, `d` shorthand |
| `view` | `bar` | `bar`, `pct`, `time`; alias of `show` |
| `show` | `time,bar` | list of `time`, `bar`, `pct`, `eta`, `proj`, `spark`, `gauge` |
| `alt` | `show`, bar and pct swapped | list of `time`, `bar`, `pct`, `eta`, `proj`, `spark`, `gauge` |
| `count` | `left` | `left`, `used` |
| `glyphs` | `blocks` | `blocks`, `shades`, `dots`, `squares`, `bars`, `braille`, `eighths`; see [Glyphs](#glyphs) |
| `color` | `solid` | `solid`, `gradient` |
| `width` | `5` | Bar cells, clamped to 1..40; spark samples |
| `click` | `toggle` | `none` turns the toggle link off; the link appears only once the handler is installed (`bar click install`) |

Components (`show`):

- `show` lists the components in order (`show=bar,pct`, `show=pct,time`); default `time,bar`.
- `eta`: the projected time until the budget runs out at the average burn so far. `→1h20m` in the bad colour when the projection is over 100%, a muted `→ok` when the budget lasts the window.
- `proj`: the projected used percent at reset (`p140%`, capped at 999). The bad colour above 100%, otherwise the bar's ramp (thresholds only, no time floors) on the projected remaining, never red.
- `eta` and `proj` both ignore `count`, and both hide while nothing is used or under 2% of the window has elapsed.
- `view` stays as an alias: `bar` is `time,bar`, `pct` is `time,pct`, `time` is `time`; `show` wins when both are set.
- `count=used` makes the percentage and the bar fill show used instead of remaining; the colour still follows the pace score of what remains.
- Expired or missing windows render nothing.

Spark and gauge:

- `spark` draws the used percent of the last `width` samples as `▁▂▃▄▅▆▇█` (default 5 cells), one cell per sample, oldest first. It hides until two samples exist.
- `gauge` is one such cell for the current value (per `count`), in the bar's colour and inside the click link like `bar` and `pct`; `spark` is not linked.
- Both are single-cell glyphs, so widths are unchanged.
- `color=gradient` colours each spark cell by its own value on the plain ramp: no time-left or final-stretch input, so old samples are not scored as if it were now; otherwise the spark takes the bar's colour.
- Sample history: when `spark` is in `show` or `alt`, a render appends `(unix time, used%)` to `hist-5h` / `hist-7d` (mode 0600, written atomically) in the state directory, but only once the step has passed since the last sample: 10 minutes for `5h`, 6 hours for `7d`. The step is fixed.
- At most 64 samples are kept, oldest dropped. A new `resets_at` (the window reset) or a clock that went backwards starts a new series.
- `bar preview` writes nothing, so it shows no spark.

Colour:

- The bar is one colour by default (`color=solid`): every filled cell takes the colour of the exact value, the same as `pct`; empty cells are muted.
- `color=gradient` colours only the filled cells, along the ramp from its bad end up to the current score: cell `i` of `n` filled cells takes the score of `remaining * (i+1) / n`, so the last filled cell equals the solid colour and the adjacent `pct`; `count=used` mirrors it (the first cell is the good end). Empty cells stay muted.
- In the final stretch the scores invert, as for the solid colour.
- The colour follows the pace score of `rule usage[type=5h]` / `rule usage[type=7d]`; without a rule the built-ins apply (`early warn=20% bad=5%`, final `1h` / `1d` with `bad=30%`).

Toggle:

- `bar toggle` (a click on the bar or percentage) flips between `show` and `alt`; `alt` defaults to `show` with `bar` and `pct` swapped. Time, `eta` and `proj` stay outside the link.
- Toggle state: `pct-5h` / `pct-7d` files (mode 0600) in the state directory (the kit state dir for `bar`; `BAR_STATE_DIR` overrides it, meant for tests; on Windows `%LOCALAPPDATA%\bar`). While a file exists, `bar` and `pct` swap. `bar toggle <5h|7d>` flips it.
- Plugins toggle the same way: `bar toggle <plugin name>` flips `toggle-<name>`, and a plugin output with `alt` shows `alt` instead of `text` while it is on (see [plugins](plugins.md#render)). The CLI works without the handler app.
- A plugin output with `label_toggle` names a second, independent toggle on its label or icon: `bar toggle <plugin>.<toggle>` flips `toggle-<plugin>.<toggle>`, linked as `bar://toggle/<plugin>.<toggle>` (see [plugins](plugins.md#render)). The plugin reads it from `toggles` on its stdin.
- Link `bar://toggle/<key>` is emitted only when the handler app is installed (marker `click-installed` in the state directory) and `click` is not `none`.

### tokens, cache, cachettl

Token counts, prompt-cache use and the time the prompt cache has left, from the session transcript.

`#{tokens[show=in|out|total]}`, `#{cache[show=read|write|hit]}`, `#{cachettl[ttl=300]}`

| Placeholder | Option | Default | Meaning |
|---|---|---|---|
| `tokens` | `show` | both | `in`, `out`, `total`; default `12.3k↑ 4.1k↓` |
| `cache` | `show` | `read` and `write` | `read`, `write`, `hit`; default `r 1.2M w 80k`, `hit` is `hit 93%` |
| `cachettl` | `ttl` | `300` | Seconds the prompt cache lives (minimum 1); `3600` for the 1 h cache |

Read from the `transcript_path` in the session JSON (assistant messages' `usage`). Nothing renders without a readable transcript.

- `tokens`: in is `input_tokens` plus `cache_creation_input_tokens`; out is `output_tokens`.
- `cache`: read is `cache_read_input_tokens`, write is `cache_creation_input_tokens`, hit is read over read plus input plus write.
- Numbers are compact: `999`, `12.3k`, `80k`, `1.2M`, `2.5B`.
- A message written as several lines (one per content block) counts once, by `message.id`.
- `cachettl`: `ttl` minus the time since the last assistant message (the transcript mtime when there is none), as `4m12s`; `cold` in muted text once it is over. Green above 60 s left, then the warn colour.
- Totals are incremental. A state file per transcript (mode 0600, `transcript/` in the state directory) keeps the byte offset and running totals; a render reads only the bytes appended since. A smaller file, a different inode (on Windows, creation time), or a changed mtime at the same size rescans from the start.
- A render reads at most 1 MiB. On a bigger backlog it shows the totals so far and a background `bar refresh` finishes the scan, so the numbers catch up within a few renders. Lines over 512 KiB and lines that are not valid JSON are skipped.

### spendhist

Estimated spend for today and the calendar month, from every local transcript.

`#{spendhist[period=today|month|both,tz=local|utc]}`

| Option | Default | Meaning |
|---|---|---|
| `period` | `both` | `today`, `month` |
| `tz` | `local` | `utc` buckets days by UTC date |

Output: `today $3.12 · mon $84.10` (the labels muted), or one bare `$3.12` for `period=today` / `period=month`. This is API-equivalent cost computed from token counts at list prices. It is not subscription billing and will not match an invoice.

Source and scanning:

- Source: every `<claude dir>/projects/*/*.jsonl`. The dir is `set claude-dir <path>`, else `CLAUDE_CONFIG_DIR`, else `~/.claude`.
- The path must be absolute (`~/` is expanded); a relative `claude-dir` or `CLAUDE_CONFIG_DIR` is ignored and the placeholder renders empty. `bar check` warns about a relative `claude-dir`.
- Assistant lines' `message.usage`, `message.model` and `timestamp` count once per `message.id` plus `requestId`, with timestamps after now counted as now, across all files (a resumed session repeats earlier messages).
- Days are local dates; `tz=utc` buckets by UTC date. Default `local`. The local zone is `TZ`, else the system zone, on Unix; on Windows it is the system zone with its year-specific daylight rules, and `TZ` is not read.
- A background job does the scanning, at most once per 60 s (a failed run waits 60 s). A render only reads the finished summary, so it renders empty until the first run completes.
- The first run reads the newest transcripts first and stops at the refresh time limit; later runs continue, so a large history catches up over a few minutes.
- State: `spendhist/<hash>.json` in the state directory (mode 0600, written atomically): per-transcript byte offsets, tokens per day and model for the last 62 days, and the hashes of counted messages. If the file would pass 16 MiB the marks of the least recently modified transcripts and then the oldest hashes are dropped; totals are kept, but a transcript whose mark was dropped is read again.

Cost:

- Cost per message: input, output, cache write and cache read tokens times the model's rates. Cache writes use the 5-minute rate for all cache-creation tokens.
- A model without a price counts as tokens but adds no dollars, and the amount gets a trailing `~`.
- Model ids over 128 characters or with unusual characters, and a day's models past the 63rd, are counted as `other`, which never has a price.

Prices:

- Built in for the Opus, Sonnet, Haiku, Fable and Mythos families, matched by exact id as below. Source: the Anthropic pricing page (platform.claude.com/docs/en/about-claude/pricing), read 2026-11-07.
- A model id matches an entry when it equals it or adds `-YYYYMMDD`; an unlisted version (say a newer minor release) has no price and shows `~` until you add a `set price`.
- Haiku 5.5 requests whose prompt (input plus cache tokens) passes 100,000 tokens are priced at five times the rates, as the page lists; a `set price` rule covers both tiers.
- Override or add with `set price <model-prefix> <in> <out> <cache_write> <cache_read>`, dollars per million tokens, for example `set price claude-opus-4-5 5 25 6.25 0.5`. An override beats the built-in table whatever its length; among overrides the longest prefix wins, a later line breaking ties.
- Models the built-in table lacks (other vendors, gateways, clouds) are priced from a models.dev snapshot built into bar (`data/models.json`; its date shows in `bar check`). models.dev data is MIT licensed.
- Lookup order: `set price` rule, built-in Anthropic table, snapshot.

Snapshot matching:

- An id is matched as given, then without a Bedrock region (`us.`, `eu.`, `apac.`, ...) and vendor (`anthropic.`, `openai/`) prefix; then without a Vertex `@...` or Bedrock `-v1:0` suffix, then without a `-YYYYMMDD` date. The first spelling any provider lists decides, and nothing is matched loosely.
- A vendor prefix other than `anthropic` only matches that provider's own entries and never the built-in Claude table.
- Free (all-zero) listings are ignored when a priced one exists.
- When providers list that id at different prices the model is unknown (`~`); add a `set price`.
- A cache rate the provider does not list is priced as plain input.

Snapshot update:

- `bar prices update` fetches a fresh copy from `https://models.dev/api.json` into `$XDG_DATA_HOME/bar/models.json` (mode 0600, replaced atomically).
- bar uses it when it is dated the same day as the built-in snapshot or later, and not in the future; a file that is invalid or out of range is ignored and `bar check` says why.
- The download is capped at 64 MiB. It runs `curl` (`-fsS --max-time 60`, https only) and is the only place core bar uses the network (plugins have their own).

### Git and directories

Repository, branch, working-tree state and the session directory.

| Placeholder | Option | Default | Meaning |
|---|---|---|---|
| `repo`, `branch` | `link` | on | Link to the repository / branch page |
| | `max` | none | Truncate to this many characters with `…` |
| `dirty` | `show` | all | Comma list of `staged`, `modified`, `untracked`, e.g. `show=staged,modified`. Output `+2 ~3 ?1`; each part omitted at 0, hidden when clean. Unmerged paths count as modified; a `+` suffix on a count (`?2000+`) marks a lower bound when git's output was cut at 64 KiB |
| | `ttl` | `10` | Seconds before the cached `git status` is refreshed in the background (minimum 1) |
| `ahead` | `ttl` | `10` | As `dirty`. Output `↑2 ↓1`; hidden at 0/0 or without an upstream |
| `worktree` | none | | Directory name of a linked worktree; hidden in the main worktree |
| `cwd` | `base` | `0` | Session directory with home as `~`; `1` shows the last component only |
| `root` | `base` | `0` | Git top-level directory of the session directory, read without running `git`; hidden outside a repository |

### prs and issues

Open pull requests and issues of the repository, from `gh` search, coloured by state.

| Placeholder | Option | Default | Meaning |
|---|---|---|---|
| `prs` | `limit` | `5` | Most items shown; `0` hides the placeholder |
| | `author` | none | [Value logic](#label-logic), e.g. `@me` |
| | `status` | `open` | `open`, `closed`, `merged` |
| | `link` | on | `false`, `no`, `off`, `0` give plain text |
| | `checks` | `fail` | Comma list of CI states marked after the number: `fail` (`✗`), `pending` (`•`), `pass` (`✓`); `none` marks nothing. See [PR checks](#pr-checks) |
| `issues` | `limit` | `5` | As `prs` |
| | `assignee`, `author` | none | Value logic |
| | `label` | none | Value logic |
| | `status` | `open` | `open`, `closed` |
| | `link` | on | As `prs` |

#### PR checks

`prs` colours each number red (failed check, changes requested or merge conflict), amber (pending check or review required) or green, and adds a mark after the number for the check states named in `checks` (default `fail`): `#{prs[checks=fail,pending]}` gives `#131✗ #129• #128`. The mark sits outside the number's link and takes the palette's bad, warn or good colour.

- State: from `statusCheckRollup` of the PR's last commit, fetched by the same `gh pr list` call as the list (no extra process, same 120 s TTL and failure backoff).
- Any check run concluding `FAILURE`, `TIMED_OUT`, `CANCELLED`, `ACTION_REQUIRED` or `STARTUP_FAILURE`, or status context `FAILURE` / `ERROR`, fails; else any unfinished run or `PENDING` / `EXPECTED` context is pending; else pass.
- No checks, or a cached list without check data, gets no mark.
- Glyphs follow `icons`: `none` uses `✗` `•` `✓`; `nerdfont` `nf-oct-x` U+F467, `nf-oct-dot_fill` U+F444, `nf-oct-check` U+F42E; `emoji` ❌ 🟡 ✅.

#### Label logic

`author`, `assignee` and `label` take expressions. `&` binds tighter than `|`; parentheses group; `!` negates; quote values holding spaces.

| Written | GitHub query |
|---|---|
| `label=a\|b` | `label:a,b` |
| `label=a&"b c"` | `label:a label:"b c"` |
| `label=(a\|b)&c` | `label:a,b label:c` |
| `label=!a` | `-label:a` |
| `label=a&b\|c` | one query per `\|` branch |

- Values holding spaces, commas or colons are quoted in the query (`area:core` becomes `label:"area:core"`).
- Only `label` folds an OR into `label:a,b`; `author` and `assignee` run one query per OR branch.
- Results of all queries are merged and deduplicated by number, newest first, then cut to `limit`.
- At most 8 queries run per placeholder; `bar check` warns when more are written.

### account, agents, sessions, tmux

Who is signed in, running subagents, other open sessions and the tmux window.

| Placeholder | Option | Default | Meaning |
|---|---|---|---|
| `account` | none | | Signed-in account (`oauthAccount.emailAddress` in `.claude.json`): the label and colour of the first matching `account` line, else the email in grey; `api-key` when only `ANTHROPIC_API_KEY` is set; hidden otherwise |
| `agents` | `names` | `0` | `1` appends the running subagents' types |
| | `max` | none | Truncate the names with `…` |
| `sessions` | none | | No options |
| `tmux` | `max` | | `session:window` of the tmux pane bar runs in; hidden outside tmux |

- `agents` counts subagents tracked by the `bar click hook`; see "Running subagents" under [Clickable commands](#clickable-commands).
- `tmux` reads `$TMUX` and `$TMUX_PANE`; a background `tmux display-message` resolves the names (cached 30 s, 60 s backoff after a failure), so the first render in a pane is empty.

### Options on every placeholder

| Option | Default | Meaning |
|---|---|---|
| `icon` | set default | A glyph (at most 2 cells), or `none` to hide it. See [Icons](#icons) |
| `bg` | palette | Segment background, `0`-`255`, under a `powerline` or `capsule` skin. See [Skins](#skins) |

Plugins included.

### Values

Option values expand before use.

- `@me`: passed to `gh` as the search value; GitHub resolves it to the logged-in user. It is the default for "mine" in the built-in layout.
- `%$VAR%`: environment variable; unset gives an empty value.
- `%cmd%`: run through `sh -c` (on Windows `cmd /D /S /C`, the text passed as typed at a `cmd` prompt) with a 3 s limit; on timeout the command and everything it started are killed. The output is cached for 1 h, keyed by the command text only, so it is shared by every session and directory. It is stored in a 0600 cache file: do not print secrets. A cold cache renders empty and refreshes in the background.

## Styling

### Skins

`set skin plain|powerline|capsule` picks how segments are drawn.

- `plain` (default): segments joined by a muted ` │ `.
- `powerline`: each segment sits on a background colour with one space of padding each side. Left and centre groups are joined by U+E0B0, the right group by U+E0B2; each arrow takes the background it leaves as its colour and the one it enters as its background, so the shapes flow out of the left and centre groups and into the right one from the edge. Neighbours that share a background are split by a thin muted U+E0B1 (left, centre) or U+E0B3 (right) instead.
- `capsule`: each segment is wrapped in U+E0B6 … U+E0B4 on its background, one space between capsules.

Powerline and capsule glyphs need a Nerd Font or a Powerline-patched font; `set skin-glyphs ascii` draws them with any font (see [Glyphs](#glyphs)).

Backgrounds:

- Backgrounds come from the palette's list, in order across the whole line (left, centre, right) and cycled.
- `bg=<0-255>` on a placeholder sets its segment's background; in a segment holding several placeholders the first `bg` wins. `plain` ignores `bg`.
- `bg` is reserved on every placeholder, plugins included; plugins never see it, and `bar check` warns when a plugin declares it. `bar check` rejects a value outside 0-255.

Colours and width:

- Colours inside a segment (usage ramp, `ctx` colour, state colours) still apply on top of the background. A reset inside a segment value restores the segment background; the background ends (`49`) only where the segment ends.
- Every skin glyph counts as one cell, whatever `icon-width` says, and layout counts the padding and glyphs when it fits groups to `COLUMNS`.
- When left and right do not fit beside each other they are joined by one space.

### Palettes

`set palette <name>` sets the muted text colour, the state colours (`color.good`, `color.warn`, `color.bad`), the usage `ramp` and the background list skins use.

| Palette | muted | good | warn | bad | backgrounds |
|---|---|---|---|---|---|
| `default` | 250 | 114 | 179 | 203 | 238 24 236 60 23 |
| `nord` | 248 | 144 | 222 | 167 | 237 24 239 67 60 |
| `gruvbox` | 246 | 142 | 214 | 167 | 237 239 66 241 95 |
| `dracula` | 146 | 84 | 228 | 203 | 236 61 238 97 239 |
| `solarized` | 245 | 106 | 136 | 160 | 23 235 24 30 66 |
| `mono` | 245 | 252 | 248 | 255 | 236 238 240 242 |
| `terminal` | 8 | 2 | 3 | 1 | 0 4 |

- `default` is the built-in colour set, ramp included (`196 … 46`).
- The other palettes use a three-step ramp from their own colours: bad for scores up to 20, warn up to 60, good above (the ramp is indexed by `(score + 9) / 10`).
- An explicit `set color.good|warn|bad` or `set ramp` beats the palette wherever it appears in the file (see [Colours](#colours)).

`terminal` draws the codes 0-15 as ANSI 16-colour SGR instead of `38;5;n`, so the terminal's own theme picks the colours:

- Foreground `30`-`37` / `90`-`97`, skin backgrounds `40`-`47` / `100`-`107`.
- Muted is bright black (`90`), good green (`32`), warn yellow (`33`), bad red (`31`), repo names cyan (`36`).
- The skin backgrounds are black and blue only, the two that keep green, yellow and red text readable in most themes (contrast still depends on the theme; use `bg=` per placeholder to override).
- Its ramp has three steps (bad, warn, good).
- Colours you set to 0-15 yourself (`color.*`, `ramp`, `bg=`, account colours) are drawn the same way under this palette; codes from 16 up keep the 256-colour form.

### Icons

`set icons nerdfont|emoji` adds an icon to every core placeholder (table below); `none` (default) leaves output as is.

- An icon is drawn muted, followed by one space, outside any link.
- When a segment has a text label (`ctx`, `agents`), the icon replaces it; otherwise it prefixes the value.
- `icon=<glyph>` on any placeholder beats the set, `icon=none` hides it; both work with `icons none`.
- Glyphs are sanitised; `bar check` rejects an icon wider than 2 cells (counted with `icon-width`). Values from `%…%` expressions or `default` lines are not checked ahead, and an over-wide one is dropped at render (no icon, label kept).
- PR check marks follow the icon set too; see [PR checks](#pr-checks).

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
| `sid` | `nf-fa-hashtag` U+F292 | 🆔 |
| `compactions` | `nf-fa-compress` U+F066 | 🧹 |
| `skills` | `nf-fa-magic` U+F0D0 | ✨ |
| `tmux` | `nf-fa-columns` U+F0DB | 🪟 |

### Glyphs

Usage bar cells, `glyphs=` on [usage](#usage):

| `glyphs` | Cells |
|---|---|
| `blocks` (default) | █░ |
| `shades` | ▓░ |
| `dots` | ●○ |
| `squares` | ■□ |
| `bars` | ▰▱ |
| `braille` | ⣿⣀ |
| `eighths` | █ with a ▏▎▍▌▋▊▉ last cell |

An unknown name draws `blocks` and `bar check` flags it.

Skin separators, `set skin-glyphs`: `nerd` (default) uses the Nerd Font / Powerline glyphs listed under [Skins](#skins); `ascii` draws `>` / `<` (powerline; `|` between same-background neighbours) and `(` / `)` (capsule) instead, for any font.

### Colours

- `set color.good`, `color.warn`, `color.bad`: the state colours; `set ramp`: the usage ramp. Values are 256-colour codes.
- An explicit `set color.good|warn|bad` or `set ramp` beats the [palette](#palettes) wherever it appears in the file.
- `account "<glob>" <label> <color>` sets the label and colour [`account`](#account-agents-sessions-tmux) shows.
- The usage colour follows the pace score of `rule usage[...]`; see [usage](#usage).

## Layout

### Lines and wrapping

`left`, `center` and `right` make line 1. `set left@N`, `center@N` and `right@N`, N from 2 to 4, make line N; `left@1` is `left`. A line whose groups all render empty is left out, so a line of git state disappears outside a repository.

Each line is laid out on its own, with the same `COLUMNS`, margin, gap and skin: groups padded apart on one row when they fit; otherwise left and right joined on one row and `center` on the next.

`set wrap on` changes the second case:

- A line that does not fit on one row has its segments flowed, left group first, then centre, then right, onto rows of `COLUMNS` minus 4 cells, breaking only between segments; a segment wider than a row gets a row to itself.
- Wrapped rows are left-aligned (centre and right alignment are dropped) and keep the skin: backgrounds keep cycling, and Powerline arrows point right.
- Without `COLUMNS` nothing is measured, so wrapping never starts.

A render prints at most 4 rows, all lines together. Rows past the fourth are dropped. With `wrap on`, the fourth row is filled like any other, whole segments while they fit, and the segments after it are dropped; segments are never cut.

## Commands

### bar check

`bar check` lints the config without rendering and prints the file it read (or says the built-in layout is in use).

- Parse errors with line numbers. An unreadable file is one error without a line.
- Line numbers in `left@N`, `center@N`, `right@N` (1-4; anything else is an error).
- Layout placeholders (`left`, `center`, `right`, and their `@N` forms) and `default` lines: unknown placeholder, unknown option, bad option value. Plugin placeholders are checked against their `--ext-info` schema.
- `prs` / `issues`: invalid `limit`, invalid `checks` item, invalid `author` / `assignee` / `label` expression.
- Warnings: `gh` not on `PATH` while `prs` or `issues` is used; a `default` line for a plugin that is not on `PATH` (a layout using it is an error); an expression that expands to more than 8 queries (only the first 8 run).
- Options holding `%command%` or `%$VAR%` are skipped; they resolve at render time.

Exit 0 when there are no errors (warnings allowed), 2 on a config error. `--format json` prints `{path, ok, issues: [{level, line, message}]}`; `line` is 0 when an issue has no line.

### bar preview

`bar preview [--state early|burning|final] [--preset [<name>]]` renders your layout (or a [preset](#presets)) with built-in sample data for every core placeholder. A bare `--preset` renders every preset in turn, each under a muted `── <name> ──` header.

- Sample session: model, effort, context, usage windows, spend limit, cost, duration, lines, version, style, vim mode, agent, thinking, fast mode, session id, compaction and skill counts.
- Stand-ins for the rest: repo, branch, git state, worktree, PRs (#131 failing CI, #129 pending, #128 passing), issues, account label, subagents, open sessions, spend history, transcript tokens and cache, directories.
- In the `final` state the sample tree is clean, so `dirty` is empty.
- It reads no session, account, git repository, `gh` or plugin, and writes no state. Plugin placeholders render empty.

States:

- `early`: plenty of budget; bars green.
- `burning`: spending fast; bars orange to yellow.
- `final`: last stretch with budget left over; bars red.

Width comes from `COLUMNS`, else 120.

### Presets

Six example configs ship in the binary and in [`presets/`](../presets): `minimal`, `full`, `powerline`, `capsule`, `compact`, `two-line` (git and PRs on line 1, usage and spend on line 2).

- `bar preview --preset <name>` renders one with sample data, ignoring your config; `bar preview --preset` with no name renders all six, each under a muted `── <name> ──` header line.

`bar init [--preset <name>]` writes one (default `full`) to the first search path: `$XDG_CONFIG_HOME/bar/bar.conf`, else `~/.config/bar/bar.conf`.

- It refuses when any search path already holds a config, prints the path it found and exits 2.
- It also refuses when that path would be relative (home unset or relative: `HOME`, on Windows `USERPROFILE` then `HOME`); a relative `XDG_CONFIG_HOME` is ignored, as the XDG spec says, here and in the search order.
- The file is written to a temp file beside it and linked into place, so it is never partial and never replaces a file that appeared meanwhile.
- There is no `--force`; remove or move the file yourself.

### Click handler (macOS)

`bar click install` builds `~/Applications/bar-click.app` (`BAR_APP_DIR` overrides the directory) from an AppleScript `on open location` handler, sets `LSUIElement` and the `bar` URL scheme with `plutil`, signs it ad hoc, registers it with `lsregister`, then writes the marker.

- Elsewhere it writes nothing, prints that it needs macOS and exits 2; `--hooks-only` registers just the hooks on any platform.
- `--dry-run` prints the script and commands and changes nothing.

CI covers the generated script, plist edits, command list and platform guard with fake runners. The install itself is checked by hand:

1. `bar click install --dry-run`: read the script and commands.
2. `bar click install`.
3. `open bar://toggle/5h`: no Dock icon appears; `pct-5h` shows up in the state directory.
4. `open bar://toggle/5h` again: the file is gone.
5. `open 'bar://toggle/a.b.c'`: nothing changes (not `5h`, `7d`, a plugin name or `<plugin>.<toggle>`).
6. In a Claude Code session, Cmd-click a `usage` bar (iTerm2): the view flips on the next refresh.

Links need Claude Code to render them. It shows status-line links only in a terminal it detects as link-capable, and inside tmux detection can fail: links then print as plain text and nothing reacts to a click, though bar emits them.

- Start Claude Code with `FORCE_HYPERLINK=1` (e.g. `export FORCE_HYPERLINK=1` in your shell profile).
- Keep tmux passing links through (`set -as terminal-features ",*:hyperlinks"`, tmux 3.4+).
- Quick test: if Cmd-clicking the repo name does not open the browser, Claude Code is not rendering links.

### Clickable commands

`model`, `effort` and `ctx` link to a slash command typed into the session they belong to: `/model`, `/effort`, `/compact`. On by default; off with `click=none` or while the handler cannot route them: the `click-installed` marker must read `cmd` (written by `bar click install`); a marker from a toggle-only install keeps usage toggles and no command links, until `bar click install` is run again.

Link and allowlist:

- Link: `bar://cmd/<command>?tty=<ttysN>&sid=<session id>`. Emitted only on macOS, and only when the parent process has a `ttysN` terminal and the session id is lowercase hex and dashes.
- Allowlist: exactly `model`, `effort`, `compact`, fixed in core. Options, config and plugins cannot add to it; any page or app can open a `bar://` URL, so the allowlist and the checks below are the boundary.

Running a click:

- `bar click run <url>` (the handler calls it) fails closed. It refuses:
  - a malformed link (scheme, host and path exact; `tty` and `sid` once each and nothing else; no encoded or control characters);
  - a command off the list;
  - a session that is missing, dead or on another terminal;
  - no tmux pane showing the session;
  - a busy session, a session waiting on a permission prompt;
  - a non-empty input box (draft or open menu).
- On Windows it checks nothing, prints that links need macOS and tmux, and exits 2.
- A refusal posts a notification, appends to `click.log` in the state directory and exits 1 (the handler app wraps the call in `try`, so no error dialog appears); it never touches the terminal.
- Idle and empty box are checked twice, a second apart, then `tmux send-keys -l "/<command>"` and `Enter`.

Pane:

- The pane is the one on the session's own terminal, else a pane showing the session's status line.
- Background sessions (`claude --bg`) are supported: such a session runs on a terminal of its own, outside tmux, and is viewed through a client in a pane (`claude attach`, or the agents view of `claude agents` or of another session).
- The viewer pane is found when the link is clicked, so detaching or switching panes after the render is fine.
- A pane counts as a viewer only when, below the last input-box border, it shows a `bar://cmd/` link (allowed command) carrying exactly this session's id and terminal; the id quoted in a transcript, in plain text or with another terminal does not count. The status line must still be there on the second pass.
- Several viewers of one session: the first in `tmux list-panes -a` order gets the keys. Every viewer forwards input to the same session, so the command lands in that session whichever pane types it.
- Limits: only the default tmux server is searched (the handler starts outside tmux, with no `TMUX`); a session viewed in a pane of another server (`tmux -L`, `-S`) refuses as not on screen. A viewer whose status line is not on screen below the input box refuses the same way.

Empty box:

- A box opens where a border (only `─`, at least 20 cells) sits right above a line holding the `❯` prompt marker.
- The real box always opens that way, so a draft that types a fake box adds a second opening, and ambiguity refuses: there must be exactly one opening on screen and no `❯` line below its marker.
- The very next line must be the bottom border (same width, and the last such line on screen), so any draft of more than one line refuses.
- After the marker only dim placeholder text may follow (outside dim spans only whitespace and SGR codes; reverse video refuses; `\e[0m`, `\e[00m` and `\e[m` all reset).
- The parser reads screen output: `tests/fixtures/sessions/synthetic/` holds hand-made panes (`empty*.ansi` must parse as empty, every other file must refuse); real captures go in a dir named after the Claude Code version (`claude --version`), and the test checks every dir there.

Hooks:

- Waiting state: `bar click hook` reads Claude Code hook JSON on stdin and keeps `sessions/<session id>` (mode 0600) in the state directory: set on a permission request, permission or elicitation notification, `AskUserQuestion` or a failed stop; cleared on prompt submit, session end, and the main agent's stop or tool result.
- Running subagents: the same hook keeps `agents/<session id>/<agent id>` (mode 0600, content the agent type) on `SubagentStart`, removes it on `SubagentStop`, and drops the session's directory on `SessionEnd`.
- tlc task: on `PostToolUse` of the `Bash` tool, the same hook keeps `records/tlc/<session id>` (mode 0600, written atomically), JSON `{"task", "cwd", "at"}`, for the [`tlc` plugin](plugins.md#bar-tlc).
  - It records the last `tlc task claim <id>`, `tlc task update <id> … --status IN_PROGRESS`, `tlc task complete <id>` or `tlc task create …` in the command, anywhere in a chain (`&&`, `||`, `;`, `|`, newlines). A `create` takes its id from the `Created task <id>` line of the tool output. Ids are 1 to 64 characters from `A-Z a-z 0-9 - _`; patterns and URIs are skipped.
  - `cwd` is the hook's `cwd`, moved by an earlier `cd <dir>` in the chain and by `tlc -C <dir>` / `--chdir`; after a `cd` it cannot follow (`cd`, `cd -`, `~`, `$VAR`) nothing more is recorded from that command. The plugin finds the project from `cwd` when it renders, the same way it finds the project of `session.cwd`.
  - A subagent's tool call carries the main session's id, so it records for that session. The latest touched task wins; commands without a task action keep the record, and touching the recorded task again from the same directory leaves the file unchanged (so the plugin's cache entry stays warm). A failed command (`PostToolUseFailure`) records nothing.
  - `SessionEnd` drops the session's records; records untouched for 30 days are pruned on the next write.
  - `#{agents}` counts entries younger than 6 hours, so a subagent that died without a stop event ages out; the flip side is that a live agent running longer than 6 hours drops out of the count.
  - Expired entries and orphan temp files are pruned on the next `SubagentStart`.
  - Only agent ids of 1 to 64 characters from `A-Z a-z 0-9 - _` are tracked; others are ignored.
  - Claude Code's statusline JSON has no subagent field and its transcript has no cheap running signal, so `#{agents}` needs the hooks registered: `bar click install` (macOS) or `bar click install --hooks-only` (any platform, hooks only, no app).
  - Both write `<claude dir>/settings.json`, the dir being `CLAUDE_CONFIG_DIR` or `~/.claude`; they refuse (exit 2) when the home dir (`HOME`; on Windows `USERPROFILE`, else `HOME`) is unset or relative, or `CLAUDE_CONFIG_DIR` is relative.
  - On Windows the hook command is the path to `bar.exe` with forward slashes, unquoted (`C:/Users/you/.cargo/bin/bar.exe click hook`), which Git Bash and PowerShell both run; a path with spaces or other shell characters is single-quoted, which only Git Bash accepts.
  - Internal agents (prompt suggestions, side questions) also fire these events and count while they run.

`bar click install` also wires the hook:

- It adds a `'<bar>' click hook` command entry under `hooks` in Claude Code's `settings.json` (`CLAUDE_CONFIG_DIR`, else `~/.claude`) for each event above that lacks one, and prints every addition before writing.
- It re-reads the file right before writing (after the app is built) so a concurrent edit is kept, copies it to a unique `settings.json.bak-<time>-<pid>`, and writes atomically, keeping the file's mode.
- A symlinked `settings.json` is written through to its target; a link to a missing file is refused.
- A file it cannot parse as Claude Code settings is left untouched and the install stops.
- `--dry-run` prints the additions and writes nothing.

CI covers links, parsing, every refusal, pane choice, the double check and the keys sent with a fake environment; nothing in the tests runs tmux, `osascript` or `ps`. Checked by hand, in a Claude Code session inside tmux after `bar click install`. During each check, save the pane with `tmux capture-pane -p -e -t <pane> > tests/fixtures/sessions/<version>/<name>.ansi` (`empty.ansi`, `draft.ansi`, `menu.ansi`, ...) and commit it, so the parser is tested against the real screen of that version:

1. Click `model`, `effort` and `ctx` with an empty input box: `/model`, `/effort` and `/compact` are typed and run.
2. Type a draft without sending it, then click `model`: a "not sent" notification appears, the draft is untouched, and `click.log` reads `the input box is not empty`.
3. Trigger a permission prompt (a tool call needing approval), then click `model`: a "not sent" notification appears, the prompt is untouched, and `click.log` reads `session is waiting for you`.
