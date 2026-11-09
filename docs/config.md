# Configuration

How to write `bar.conf`: where it lives, the directives, every placeholder and its options, styling, layout and the commands around the config. Pictures of skins, skin glyphs, segment backgrounds, palettes, bar glyphs, usage components, pace states, icons, lines and wrapping, and PR checks: [Gallery](gallery.md).

- [Overview](#overview): [config file](#config-file), [directives](#directives), [rules](#rules), [minimal example](#minimal-example)
- [Placeholders](#placeholders): [session](#session), [usage](#usage), [tokens, cache, cachettl](#tokens-cache-cachettl), [ttft, tps, misses](#ttft-tps-misses), [spendhist](#spendhist), [git and directories](#git-and-directories), [prs and issues](#prs-and-issues), [account, agents, sessions, tmux](#account-agents-sessions-tmux), [every placeholder](#options-on-every-placeholder), [values](#values)
- [Styling](#styling): [skins](#skins), [palettes](#palettes), [icons](#icons), [glyphs](#glyphs), [colours](#colours)
- [Layout](#layout): [lines and wrapping](#lines-and-wrapping)
- [Commands](#commands): [bar check](#bar-check), [bar preview](#bar-preview), [presets and bar init](#presets), [click handler](#click-handler-macos), [clickable commands](#clickable-commands), [hooks](#hooks)
- [Files and environment](#files-and-environment)

## Overview

### Config file

Search order, first existing wins:

1. `$XDG_CONFIG_HOME/bar/bar.conf` (ignored when relative)
2. `~/.config/bar/bar.conf`
3. platform config dir for `bar` (macOS: `~/Library/Application Support/bar/bar.conf`; Linux: `~/.config/bar/bar.conf`, the same as 2; Windows: `%LOCALAPPDATA%\bar\bar.conf`)

`~` is the home dir: `HOME`; on Windows `USERPROFILE`, else `HOME`. Other files bar reads and writes: [Files and environment](#files-and-environment).

No file: the built-in layout, which is

```text
set left "#{model}#{effort} │ #{ctx} │ #{repo}:#{branch} │ #{prs[limit=3,author=@me]} │ #{issues[limit=3,assignee=@me]}"
set center "#{usage[type=5h]} │ #{usage[type=7d]}"
set right "#{account}"
```

A config file replaces it whole: every group starts empty, so a file that sets no `left`, `center` or `right` renders nothing. Start from a [preset](#presets) (`bar init`) to keep the built-in groups.

A file with an error makes `bar render` print `bar: config error, run bar check`.

### Directives

| Directive | Form |
|-----------|------|
| `set` | `set <key> <value>`; keys `left center right` (and `left@N center@N right@N`, N 1-4) `separator gap wrap refresh icons icon-width skin skin-glyphs palette claude-dir color.good color.warn color.bad ramp`; `set price <model-prefix> <in> <out> <cache_write> <cache_read>` takes five values |
| `default` | `default <placeholder> key=value ...` |
| `account` | `account "<glob>" <label> <color>` |
| `rule` | `rule usage[type=5h] early warn=20% bad=5% final=1h bad=30%`; see [Rules](#rules) |
| `plugin` | `plugin <name> key=value ...`; see [plugins](plugins.md#render) |

Quote values with spaces (double quotes; a value is one token). `#` outside quotes starts a comment. A later `default` line for the same placeholder merges over an earlier one; a later `plugin` line for the same name replaces it; a later `set` wins.

| `set` key | Values |
|---|---|
| `left`, `center`, `right` | Layout of line 1; `left@N`, `center@N`, `right@N` for lines 2-4. See [Lines and wrapping](#lines-and-wrapping) |
| `separator` | Changes only where segments split (the text a layout is cut at). Output between segments stays ` │ `. Default `│` |
| `gap` | Fewest spaces between the left and right groups (and around a centred group) on one row, default `3`; with less room the groups are joined by ` │ ` or the centre group moves to its own row. See [Lines and wrapping](#lines-and-wrapping) |
| `wrap` | `off` (default) or `on`; see [Lines and wrapping](#lines-and-wrapping). `auto` is read as `off` |
| `refresh` | Default ttl floor for plugins, default `5s`. A number with a unit: `s`, `m`, `h` or `d` (`30s`, `2m`). `gh` lists use 120 s and `%cmd%` values 1 h, whatever this says |
| `icons` | `none` (default, output unchanged), `nerdfont` or `emoji`. See [Icons](#icons) |
| `icon-width` | `1` (default) or `2`; cells a Nerd Font glyph (private-use codepoint) takes in your font. Layout uses it to pad; emoji widths come from Unicode |
| `skin` | `plain` (default), `powerline` or `capsule`. See [Skins](#skins) |
| `skin-glyphs` | `nerd` (default) or `ascii`. See [Glyphs](#glyphs) |
| `palette` | `default`, `nord`, `gruvbox`, `dracula`, `solarized`, `mono` or `terminal`. See [Palettes](#palettes) |
| `claude-dir` | Claude Code config dir read by [spendhist](#spendhist) |
| `color.good`, `color.warn`, `color.bad`, `ramp` | State colours (one code `0`-`255`) and usage ramp (exactly 11 codes, quoted, worst score first). See [Colours](#colours) |
| `price` | Model price override for [spendhist](#spendhist) |

`set` takes one value; quote it when it holds spaces. An unknown key or a bad value is a config error.

### Rules

`rule <target> early warn=<amount> bad=<amount> final=<duration> bad=<amount>`, or `early pace` in place of the two early thresholds. A rule sets how a value left in a window is scored 0 (bad end of the ramp) to 100 (good end).

| Part | Meaning |
|---|---|
| `<target>` | `usage[type=5h]` or `usage[type=7d]` (spelled exactly so), or a plugin placeholder name that emits `metric` ([plugins](plugins.md#render)) |
| `early warn=X bad=Y` | Before the final stretch: score 0 at or below `bad` left, 50 at `warn`, 100 once what is left reaches the share of the window still to run. Both floors rise with the time left: `warn` to at least half and `bad` to at least a fifth of that share, so spending ahead of time turns amber, then red |
| `early pace` | Same as `early warn=20% bad=5%` |
| `final=<duration>` | Length of the final stretch before the reset or expiry: `s`, `m`, `h` or `d` |
| `bad=<amount>` (after `final`) | In the final stretch the score is `100 - left * 100 / (bad + 1)`: much left over is red, since it will go unused |
| amounts | Percent (`20%`) or dollars (`$50`, `$12.50`). Usage windows take percent only (a dollar amount reads as `0%`, or `30%` for the final `bad`). Plugin metrics take both: early dollar thresholds are read as shares of the grant, a final dollar `bad` compares cents left |

Plugin metrics without `granted_at` have no pace period, so the early floors do not rise with time.

Without a rule, `usage` uses `early warn=20% bad=5% final=1h bad=30%` for `5h` and `final=1d` for `7d`. A rule's target matches literally: `rule usage[type=h]` matches nothing.

### Minimal example

The `minimal` [preset](#presets): model, context and the 5-hour window.

```text
set left "#{model} │ #{ctx}"
set right "#{usage[type=5h]}"
```

## Placeholders

`#{name[key=value,key=value]}`. Inline options override the matching `default` line. Defaults below are what the code applies when an option is unset. Plugins: see [plugins](plugins.md).

- The `separator` (default `│`) splits a group into segments; a segment whose placeholders all render empty is dropped with its text.
- Other text is drawn muted, and only beside a non-empty placeholder: `#{repo}:#{branch}` loses the colon when the branch is empty, and `#{branch} #{dirty} #{ahead}` on a clean tree reads `main ↑2` (one space kept between two values).
- Double quotes protect `,`, `]` and `}` inside a value: `#{issues[label="a,b"]}`.
- A bare word is an option with an empty value. After `show`, `alt` or `checks` it continues that comma list instead: `#{usage[show=time,pct,width=8]}` sets `show` to `time,pct`.
- An unknown core placeholder name is looked up as a plugin; `bar check` reports names that are neither.

### Session

Fields of the session: model, effort, context, cost and the rest of the statusline JSON.

| Placeholder | Option | Default | Meaning |
|---|---|---|---|
| `model`, `effort`, `ctx` | `click` | on | `none` turns the command link off; the link appears only once the handler is installed (`bar click install`). See [Clickable commands](#clickable-commands) |
| `cost` | `fmt` | `usd` | Session cost, `$1.23`; `<$0.01` under a cent. Hidden when absent. `usd` is the only value |
| `burn` | none | | Cost per hour, `$4.10/h`; hidden for the first 60 s of the session |
| `duration` | none | | Session time: `45s`, `12m`, `1h05m` |
| `lines` | none | | `+120 -14`; hidden when both are 0 |
| `version`, `vim`, `agent` | none | | Claude Code version, vim mode, agent name; hidden when absent |
| `style` | none | | Output style; hidden when `default` |
| `thinking`, `fast` | none | | `think` / `fast` while enabled; hidden otherwise |
| `spend` | none | | Time left on a monthly spend limit (`rate_limits.spend_limit` with `period` `monthly`), muted, e.g. `3d4h`; hidden otherwise |
| `sid` | `len` | `8` | First `len` chars of the session id; hidden when absent or `len=0` |
| `compactions` | none | | `compact 2`: context compactions in this session's transcript; hidden at 0 |
| `skills` | none | | `skills 3`: Skill tool calls on the main conversation in the transcript; hidden at 0 |

- `model`: `model.display_name` (else `model.id`) without a leading `Claude `, muted, e.g. `Opus 4.7`.
- `effort`: `effort.level` as `[high]`, muted. When the session JSON has none, it reads `effortLevel` from `<claude dir>/settings.json` (`modelSettings.<model id>.effortLevel` first); hidden when neither is set.
- `ctx`: `context_window.used_percentage` as `ctx 38%`, coloured on the ramp from green at 0% to red from 91%.
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
- Toggle state: `pct-5h` / `pct-7d` files (mode 0600) in the [state directory](#files-and-environment). While a file exists, `bar` and `pct` swap. `bar toggle <5h|7d>` flips it and prints the view now shown (`5h: pct`, `7d: bar`); an unknown key exits 2.
- Plugins toggle the same way: `bar toggle <plugin name>` flips `toggle-<name>` (printing `<name>: alt` or `<name>: show`), and a plugin output with `alt` shows `alt` instead of `text` while it is on (see [plugins](plugins.md#render)). The CLI works without the handler app.
- A plugin output with `label_toggle` names a second, independent toggle on its label or icon: `bar toggle <plugin>.<toggle>` flips `toggle-<plugin>.<toggle>`, linked as `bar://toggle/<plugin>.<toggle>` (see [plugins](plugins.md#render)). The plugin reads it from `toggles` on its stdin.
- Link `bar://toggle/<key>` is emitted only when the handler app is installed (marker `click-installed` in the state directory) and `click` is not `none`.

### tokens, cache, cachettl

Token counts, prompt-cache use and the time the prompt cache has left, from the session transcript.

`#{tokens[show=in|out|total]}`, `#{cache[show=read|write|hit]}`, `#{cachettl[ttl=3600]}`

| Placeholder | Option | Default | Meaning |
|---|---|---|---|
| `tokens` | `show` | `both` | `in`, `out`, `total`, `both`; `both` is `12.3k↑ 4.1k↓` |
| `cache` | `show` | `both` | `read`, `write`, `hit`, `both`; `both` is `r 1.2M w 80k`, `hit` is `hit 93%` |
| `cachettl` | `ttl` | detected | Seconds the prompt cache lives (minimum 1). Without it: `3600` when the last cache write that names its lifetime went to the 1-hour cache, else `300` |

Read from the `transcript_path` in the session JSON (assistant messages' `usage`). Nothing renders without a readable transcript.

- `tokens`: in is `input_tokens` plus `cache_creation_input_tokens`; out is `output_tokens`.
- `cache`: read is `cache_read_input_tokens`, write is `cache_creation_input_tokens`, hit is read over read plus input plus write.
- Numbers are compact: `999`, `12.3k`, `80k`, `1.2M`, `2.5B`.
- A message written as several lines (one per content block) counts once, by `message.id`.
- `cachettl`: `ttl` minus the time since the last assistant message (the transcript mtime when there is none), as `4m12s`; `cold` in muted text once it is over. Green above 60 s left, then the warn colour.
- The lifetime is detected from `usage.cache_creation`: a write with `ephemeral_1h_input_tokens` and no `ephemeral_5m_input_tokens` means the 1-hour cache; any 5-minute part means the 5-minute cache. A request that writes nothing, or a transcript without the split, keeps the last detected lifetime (5 minutes to begin with).
- Totals are incremental. A state file per transcript (mode 0600, `transcript/` in the state directory) keeps the byte offset and running totals; a render reads only the bytes appended since. A smaller file, a different inode (on Windows, creation time), or a changed mtime at the same size rescans from the start.
- A render reads at most 1 MiB. On a bigger backlog it shows the totals so far and a background `bar refresh` finishes the scan, so the numbers catch up within a few renders. Lines over 512 KiB and lines that are not valid JSON are skipped.

### ttft, tps, misses

Time to first token, output speed and prompt-cache misses, from the same transcript read as [`tokens`](#tokens-cache-cachettl).

`#{ttft[show=last|avg|both]}`, `#{tps[show=last|avg|both]}`, `#{misses[show=count,cause,cold]}`

| Placeholder | Option | Default | Meaning |
|---|---|---|---|
| `ttft` | `show` | `last` | `last`, `avg`, `both`; `ttft 1.4s`, `ttft avg 1.8s`, `ttft 1.4s avg 1.8s` |
| `tps` | `show` | `last` | `last`, `avg`, `both`; `43 t/s`, `avg 38 t/s`, `43 avg 38 t/s` |
| `misses` | `show` | `count,cause` | list of `count` (`miss 2`), `cause` (`ttl 1 inv 1`), `cold` (`cold 180k +$0.68`) |

What the timestamps measure:

- Claude Code stamps each transcript entry with the time it writes it. A prompt's user entry is written when the prompt is submitted; a tool result's user entry when the tool finishes; an assistant entry when one content block (thinking, text or tool call) has finished streaming. One API response is split over several assistant entries that share a `message.id`.
- There are no per-token times. Both numbers are therefore built from these write times and include more than generation alone.
- Only main-conversation entries count (`isSidechain` not `true`).

`ttft`:

- From the prompt's user entry to the first assistant entry after it. Since that entry is written when its first block is complete, the time includes generating that whole block (for example the full thinking block), not just the first token.
- A prompt is a `user` entry whose content holds no `tool_result` block and that is not marked `isMeta`.
- `last` is the most recent answered prompt; a prompt still waiting for its first entry leaves the previous value shown. `avg` is the mean over every answered prompt in the transcript.

`tps`:

- Per API response: `output_tokens` (from the response's last entry) over the time from the user entry that preceded it (the prompt or the last tool result) to the response's last assistant entry. The span includes the wait for the first token, and output tokens include thinking.
- `last` sums the responses since the latest answered prompt, tool-calling rounds included; `avg` sums every response in the transcript. A response with no user entry before it, or a span of zero, is left out.
- One decimal under 10 (`4.2`), whole numbers above.

`misses`:

- A response is a miss when the previous response had at least 1024 cached prompt tokens (cache read plus cache write), and this one reads under half of that and writes more than it reads: the cached prefix had to be written again.
- Cause `ttl`: the time from the previous response's last entry to the user entry that sent this request is longer than the cache lifetime ([detected](#tokens-cache-cachettl) like `cachettl`: 1 hour or 5 minutes). Cause `inv`: the cache was still in its lifetime, so the prefix changed (another model, edited instructions or tools, a cleared or rewound conversation).
- The first response after a compaction starts a new prompt and is never counted.
- `cold`: the cached prefix of the last response, which the next request writes again if the cache has gone cold, and the dollars that adds over reading it: tokens times (cache write rate minus cache read rate) for the session's `model.id`, from the [price table](#spendhist) and `set price` rules. The 1-hour cache writes at twice the input rate. Without a price only the tokens show.
- `count` and `cause` hide at 0 misses; `cold` hides before the first response; the placeholder hides when every item does.

State lives in the per-transcript file of [`tokens`](#tokens-cache-cachettl): the response being written, the last turn and running sums, a few numbers per transcript whatever its length.

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
- Models the built-in table lacks (other vendors, gateways, clouds) are priced from a models.dev snapshot in bar's data dir, fetched on demand (see [snapshot fetch](#spendhist) below); until it is there they show `~`. bar ships no models.dev data. models.dev data is MIT licensed.
- Lookup order: `set price` rule, built-in Anthropic table, snapshot.

Snapshot matching:

- An id is matched as given, then without a Bedrock region (`us.`, `eu.`, `apac.`, ...) and vendor (`anthropic.`, `openai/`) prefix; then without a Vertex `@...` or Bedrock `-v1:0` suffix, then without a `-YYYYMMDD` date. The first spelling any provider lists decides, and nothing is matched loosely.
- A vendor prefix other than `anthropic` only matches that provider's own entries and never the built-in Claude table.
- Free (all-zero) listings are ignored when a priced one exists.
- When providers list that id at different prices the model is unknown (`~`); add a `set price`.
- A cache rate the provider does not list is priced as plain input.

Snapshot fetch:

- File: `$XDG_DATA_HOME/bar/models.json` (mode 0600, replaced atomically), fetched from `https://models.dev/api.json`. Its date and path show in `bar check`.
- When: a render of `spendhist` sees tokens this month for a model that neither a `set price` rule nor the built-in table prices, and the file is missing, unusable or last written over a day ago. The render then starts a background fetch (cache key `prices:models.dev`, at most once a day, retried no sooner than 60 s after a failure) and shows the amount with `~`; a later render uses the new file. A render never waits on the network.
- Never fetched when `spendhist` is not in the layout or every model in it is priced by a rule or the built-in table. To avoid the fetch, add a `set price` rule for each such model.
- `bar prices update` fetches the same file on request.
- A file that is invalid or dated in the future is ignored (`bar check` says why) and fetched again.
- The download is capped at 64 MiB. It runs `curl` (`-fsS`, https only; `--max-time 60` for `bar prices update`, the refresh time limit in the background) and is the only place core bar uses the network (plugins have their own).

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
| `sessions` | none | | Live Claude Code sessions as `>2 ?1 v3`: working (warn colour), waiting on you (bad colour, bold), finished a turn (good colour); each part omitted at 0, hidden when all are 0 |
| `tmux` | `max` | none | `session:window` of the tmux pane bar runs in, muted; truncated to `max` characters; hidden outside tmux |

- `account`: `account "<glob>" <label> <color>` lines are tried in file order; `*` in the glob matches any run of characters. `.claude.json` is read from `CLAUDE_CONFIG_DIR`, else the home dir.
- `agents` renders `agents 2` (`2` alone when an icon replaces the label), with `names=1` `agents 2 Explore Plan`. It counts subagents tracked by the `bar click hook`; see [Hooks](#hooks).
- `sessions` reads `<claude dir>/sessions/*.json` (Claude Code's own session files: process id, session id, `busy` status) and keeps those whose process is alive, the current session included. Waiting and finished come from the markers `bar click hook` writes ([Hooks](#hooks)); without the hooks only working sessions count.
- `tmux` reads `$TMUX` and `$TMUX_PANE`; a background `tmux display-message` resolves the names (cached 30 s, 60 s backoff after a failure), so the first render in a pane is empty.

### Options on every placeholder

| Option | Default | Meaning |
|---|---|---|
| `icon` | set default | A glyph (at most 2 cells), or `none` to hide it. See [Icons](#icons) |
| `bg` | palette | Segment background, `0`-`255`, under a `powerline` or `capsule` skin. See [Skins](#skins) |

Plugins included: both are handled by bar and never sent to a plugin. `click` is reserved the same way on `usage`, `model`, `effort`, `ctx` and every plugin (`toggle` or `none` on `usage` and plugins; `none` turns links off). `show` and `alt` are comma lists wherever they appear (see above).

### Values

Placeholder option values (inline and from `default` lines) expand before use; `plugin` lines, `set` values and layout text do not. A lone `%` or an empty `%%` stays literal.

- `@me`: passed to `gh` as the search value; GitHub resolves it to the logged-in user. It is the default for "mine" in the built-in layout.
- `%$VAR%`: environment variable; unset gives an empty value.
- `%cmd%`: run through `sh -c` (on Windows `cmd /D /S /C`, the text passed as typed at a `cmd` prompt) with a 3 s limit; on timeout the command and everything it started are killed. Stdout is trimmed; a command that fails or times out keeps the last cached value (empty when there is none) and is retried after 60 s. The output is cached for 1 h, keyed by the command text only, so it is shared by every session and directory. It is stored in a 0600 cache file: do not print secrets. A cold cache renders empty and refreshes in the background.

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
| `misses` | `nf-fa-chain_broken` U+F127 | 🧊 |
| `ttft` | `nf-fa-hourglass_start` U+F251 | ⏱️ |
| `tps` | `nf-fa-line_chart` U+F201 | 📈 |
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

Each line is laid out on its own, with the same `COLUMNS`, margin, gap and skin, in a width of `COLUMNS` minus 4 cells:

- Left at the start, right flush at the end, `center` centred in the width (or midway between left and right when centring leaves less than `gap` on one side), when every gap is at least `set gap` spaces.
- Otherwise left and right on one row, joined by ` │ ` when they do not fit apart (one space under a skin), and `center` on the next row.
- Without `COLUMNS` nothing is measured: left and right are joined on one row and `center` goes to the next (`bar preview` uses 120 when `COLUMNS` is unset).

`set wrap on` changes the second case:

- A line that does not fit on one row has its segments flowed, left group first, then centre, then right, onto rows of `COLUMNS` minus 4 cells, breaking only between segments; a segment wider than a row gets a row to itself.
- Wrapped rows are left-aligned (centre and right alignment are dropped) and keep the skin: backgrounds keep cycling, and Powerline arrows point right.
- Without `COLUMNS` nothing is measured, so wrapping never starts.

A render prints at most 4 rows, all lines together. Rows past the fourth are dropped. With `wrap on`, the fourth row is filled like any other, whole segments while they fit, and the segments after it are dropped; segments are never cut.

## Commands

| Command | Does | Exit |
|---|---|---|
| `bar render` | Reads the session JSON on stdin, prints the status line ([technical notes](technical.md#flow)) | always 0 |
| `bar check` | Lints the config ([bar check](#bar-check)) | 0, 2 on a config error |
| `bar preview [--state early\|burning\|final] [--preset [<name>]]` | Layout with sample data ([bar preview](#bar-preview)) | 0 |
| `bar init [--preset <name>]` | Writes a preset as your config ([presets](#presets)) | 0, 2 when a config exists or the path is unusable |
| `bar toggle <key>` | Flips `5h`, `7d`, `<plugin>` or `<plugin>.<toggle>` ([usage](#usage)) | 0, 2 on an unknown key |
| `bar click install [--dry-run] [--hooks-only]` | URL handler app and hooks ([click handler](#click-handler-macos)) | 0, 2 unsupported or bad home, 1 on failure |
| `bar click run <url>` | Types an allow-listed slash command ([clickable commands](#clickable-commands)) | 0, 1 refused, 2 on Windows |
| `bar click hook` | Claude Code hook ([hooks](#hooks)) | always 0 |
| `bar plugin list` | Plugins on `PATH`: name, version, description, path ([plugins](plugins.md#discovery)) | 0 |
| `bar plugin info <name>` | One plugin's `--ext-info` result and path | 0, 1 when no such plugin |
| `bar prices update [--out <file>]` | Fetches models.dev prices ([snapshot fetch](#spendhist)); `--out` writes that file instead of the data dir's `models.json` | 0, 2 on failure |
| `bar --version`, `bar --help`, `bar <command> --help` | Version, help | 0 |

`--format csv|json|table|text|yaml` is accepted by every command and changes the output of `bar check`, `bar plugin list` and `bar plugin info` (default `table`). A usage error (unknown command or flag) exits 2. `bar refresh <request>` is internal: render spawns it to fill the cache in the background.

### bar check

`bar check` lints the config without rendering and prints the file it read (or says the built-in layout is in use).

- Parse errors with line numbers. An unreadable file is one error without a line.
- Line numbers in `left@N`, `center@N`, `right@N` (1-4; anything else is an error).
- Layout placeholders (`left`, `center`, `right`, and their `@N` forms) and `default` lines: unknown placeholder, unknown option, bad option value. Plugin placeholders are checked against their `--ext-info` schema.
- `prs` / `issues`: invalid `limit`, invalid `checks` item, invalid `author` / `assignee` / `label` expression.
- Warnings: `gh` not on `PATH` while `prs` or `issues` is used; a `default` line for a plugin that is not on `PATH` (a layout using it is an error); an expression that expands to more than 8 queries (only the first 8 run).
- Options holding `%command%` or `%$VAR%` are skipped; they resolve at render time.

- It also prints the models.dev snapshot state: its date and path, `not fetched`, or why the file was ignored.

Exit 0 when there are no errors (warnings allowed), 2 on a config error. `--format json` prints `{path, prices: {path, fetched, ignored}, ok, issues: [{level, line, message}]}`; `path` (top level) is `null` when no config file is in use, `prices.fetched` is the snapshot date or `null` when there is no usable file, and `line` is 0 when an issue has no line. `--format yaml`, `csv` and `text` give the same data in that format; the default, `table`, is the plain report.

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

`bar click install` builds `~/Applications/bar-click.app` (`BAR_APP_DIR` overrides the directory) from an AppleScript `on open location` handler (source kept as `click-handler.applescript` in the state directory), sets `LSUIElement`, the bundle id `top.hop.bar-click` and the `bar` URL scheme with `plutil`, signs it ad hoc, registers it with `lsregister`, adds the [hooks](#hooks) to Claude Code's `settings.json`, then writes the marker `click-installed` (content `cmd`) in the state directory.

- The handler passes `bar://toggle/<key>` to `bar toggle <key>` and `bar://cmd/...` to `bar click run`, using the absolute path of the `bar` that ran the install; reinstall after moving the binary.
- Elsewhere it writes nothing, prints that it needs macOS and exits 2; `--hooks-only` registers just the hooks on any platform and writes no marker.
- `--dry-run` prints the script, commands and hook additions and changes nothing; with `--hooks-only`, the hook additions only.
- Exit codes: 0 done; 2 unsupported platform, or the home dir (`HOME`, or `BAR_APP_DIR`) or `CLAUDE_CONFIG_DIR` is not absolute; 1 when a step fails (a command, an unparsable `settings.json`, a write).

Check an install:

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

`model`, `effort` and `ctx` link to a slash command typed into the session they belong to: `/model`, `/effort`, `/compact`. On by default; off with `click=none` or while the handler cannot route them: the `click-installed` marker must read `cmd`, as `bar click install` writes it; a marker with any other content enables toggle links only.

Link and allowlist:

- Link: `bar://cmd/<command>?tty=<ttysN>&sid=<session id>`. Emitted only on macOS, and only when the parent process has a `ttysN` terminal and the session id is lowercase hex and dashes.
- Allowlist: exactly `model`, `effort`, `compact`, fixed in core. Options, config and plugins cannot add to it; any page or app can open a `bar://` URL, so the allowlist and the checks below are the boundary.

Running a click:

- `bar click run <url>` (the handler calls it) fails closed. It refuses:
  - a malformed link (scheme, host and path exact; `tty` and `sid` once each and nothing else; no encoded or control characters);
  - a command off the list;
  - a session that is missing, dead or on another terminal;
  - no tmux pane showing the session;
  - an input box that is not empty: a draft, or anything drawn in its place (menu, picker, permission prompt, question dialog).
- Busy or not makes no difference: only the input box gates the keys. A busy session gets them as typed input, and Claude Code decides what runs mid-turn (`/model` opens its picker right away).
- On Windows it checks nothing, prints that links need macOS and tmux, and exits 2.
- A refusal posts a notification, appends to `click.log` in the state directory and exits 1 (the handler app wraps the call in `try`, so no error dialog appears); it never touches the terminal.
- The empty box (and, for a viewer pane, its status line) is checked twice, a second apart, then `tmux send-keys -l "/<command>"` and `Enter`.

Pane:

- The pane is the one on the session's own terminal, else a pane showing the session's status line.
- Background sessions (`claude --bg`) are supported: such a session runs on a terminal of its own, outside tmux, and is viewed through a client in a pane (`claude attach`, or the agents view of `claude agents` or of another session).
- The viewer pane is found when the link is clicked, so detaching or switching panes after the render is fine.
- A pane counts as a viewer only when, below the last input-box border, it shows a `bar://cmd/` link (allowed command) carrying exactly this session's id and terminal; the id quoted in a transcript, in plain text or with another terminal does not count. The status line must still be there on the second pass.
- Several viewers of one session: the first in `tmux list-panes -a` order gets the keys. Every viewer forwards input to the same session, so the command lands in that session whichever pane types it.
- Limits: only the default tmux server is searched (the handler starts outside tmux, with no `TMUX`); a session viewed in a pane of another server (`tmux -L`, `-S`) refuses as not on screen. A viewer whose status line is not on screen below the input box refuses the same way.

Empty box:

- A box opens where a top border (at least 20 cells) sits right above a line holding the `❯` prompt marker.
- The top border is only `─`, or carries the session name: at least two `─`, a space, the name, a space, at least one `─` (Claude Code 2.1.319 draws named sessions this way). The name may be any visible text without `─`, `❯` or control characters; SGR codes around or inside it are ignored.
- The real box always opens that way, so a draft that types a fake box adds a second opening, and ambiguity refuses: there must be exactly one opening on screen and no `❯` line below its marker.
- The very next line must be the bottom border (only `─`, as many cells as the top border including any name, and the last such line on screen), so any draft of more than one line refuses.
- After the marker only dim placeholder text may follow (outside dim spans only whitespace and SGR codes; reverse video refuses; `\e[0m`, `\e[00m` and `\e[m` all reset).
### Hooks

`bar click hook` is a Claude Code hook command: it reads the hook JSON on stdin, updates files in the state directory and prints nothing. Bad input does nothing; it never fails the session. `bar click install` (macOS) and `bar click install --hooks-only` (any platform) register it.

| Event | Effect |
|---|---|
| `PermissionRequest`, `StopFailure` | waiting marker set |
| `Notification` (`permission_prompt`, `elicitation_dialog`, `elicitation_url_dialog`) | waiting marker set |
| `PreToolUse` of `AskUserQuestion` | waiting marker set |
| `PostToolUse`, `PostToolUseFailure`, `PermissionDenied` (main agent) | waiting marker cleared; `PostToolUse` of `Bash` also updates the tlc record |
| `Stop` (main agent) | marker set to `done` |
| `UserPromptSubmit` | marker cleared |
| `SessionEnd` | marker cleared; the session's subagent entries and plugin records dropped |
| `SubagentStart`, `SubagentStop` | subagent entry added, removed |

Waiting marker: `sessions/<session id>` (mode 0600), content `waiting` or `done`. [`sessions`](#account-agents-sessions-tmux) counts waiting and finished sessions from it. `Stop`, `PostToolUse`, `PostToolUseFailure` and `PermissionDenied` from a subagent (they carry an `agent_id`) leave it alone.

Running subagents:

- `agents/<session id>/<agent id>` (mode 0600, content the agent type), added on `SubagentStart`, removed on `SubagentStop`.
- `#{agents}` counts entries younger than 6 hours, so a subagent that died without a stop event ages out; the flip side is that a live agent running longer than 6 hours drops out of the count.
- Expired entries and orphan temp files are pruned on the next `SubagentStart`.
- Only agent ids of 1 to 64 characters from `A-Z a-z 0-9 - _` are tracked; others are ignored.
- Claude Code's statusline JSON has no subagent field and its transcript has no cheap running signal, so `#{agents}` needs the hooks registered.
- Internal agents (prompt suggestions, side questions) also fire these events and count while they run.

tlc task record: on `PostToolUse` of the `Bash` tool the hook keeps `records/tlc/<session id>` (mode 0600, written atomically), JSON `{"task", "cwd", "at"}`, for the [`tlc` plugin](plugins.md#bar-tlc).

- It records the last `tlc task claim <id>`, `tlc task update <id> … --status IN_PROGRESS`, `tlc task complete <id>` or `tlc task create …` in the command, anywhere in a chain (`&&`, `||`, `;`, `|`, newlines). A `create` takes its id from the `Created task <id>` line of the tool output. Ids are 1 to 64 characters from `A-Z a-z 0-9 - _`; patterns and URIs are skipped.
- `cwd` is the hook's `cwd` (absolute, else nothing is recorded), moved by an earlier `cd <dir>` in the chain and by `tlc -C <dir>` / `--chdir`; after a `cd` it cannot follow (`cd`, `cd -`, `~`, `$VAR`) nothing more is recorded from that command. The plugin finds the project from `cwd` when it renders, the same way it finds the project of `session.cwd`.
- A subagent's tool call carries the main session's id, so it records for that session. The latest touched task wins; commands without a task action keep the record, and touching the recorded task again from the same directory leaves the file unchanged (so the plugin's cache entry stays warm). A failed command (`PostToolUseFailure`) records nothing.
- `SessionEnd` drops the session's records; records untouched for 30 days are pruned on the next write.

Registering the hooks:

- `bar click install` and `bar click install --hooks-only` add a `'<bar>' click hook` command entry under `hooks` in `<claude dir>/settings.json` (`CLAUDE_CONFIG_DIR`, else `~/.claude`) for each of the 12 events above that lacks one, and print every addition before writing. `<bar>` is the absolute path of the running binary.
- They refuse (exit 2) when the home dir (`HOME`; on Windows `USERPROFILE`, else `HOME`) is unset or relative, or `CLAUDE_CONFIG_DIR` is relative.
- The file is re-read right before writing (after the app is built) so a concurrent edit is kept, copied to a unique `settings.json.bak-<time>-<pid>`, and written atomically, keeping its mode.
- A symlinked `settings.json` is written through to its target; a link to a missing file is refused.
- A file that does not parse as Claude Code settings is left untouched and the install stops (exit 1).
- `--dry-run` prints the additions and writes nothing.
- On Windows the hook command is the path to `bar.exe` with forward slashes, unquoted (`C:/Users/you/.cargo/bin/bar.exe click hook`), which Git Bash and PowerShell both run; a path with spaces or other shell characters is single-quoted, which only Git Bash accepts.

Check clickable commands, in a Claude Code session inside tmux after `bar click install`:

1. Click `model`, `effort` and `ctx` with an empty input box: `/model`, `/effort` and `/compact` are typed and run.
2. Type a draft without sending it, then click `model`: a "not sent" notification appears, the draft is untouched, and `click.log` reads `the input box is not empty`.
3. Trigger a permission prompt (a tool call needing approval), then click `model`: a "not sent" notification appears, the prompt is untouched, and `click.log` reads `the input box is not empty`.

## Files and environment

| Dir | Default | Override |
|---|---|---|
| Config | see [Config file](#config-file) | `XDG_CONFIG_HOME` (absolute) |
| State and cache | macOS `~/Library/Application Support/bar`; Linux `~/.local/state/bar`; Windows `%LOCALAPPDATA%\bar` | `BAR_STATE_DIR`, else `XDG_STATE_HOME/bar` (absolute; a leading `~` expands) |
| Data (models.dev snapshot, `models.json`) | macOS `~/Library/Application Support/bar`; Linux `~/.local/share/bar`; Windows `%LOCALAPPDATA%\bar` | `XDG_DATA_HOME/bar` (absolute; a leading `~` expands) |
| Claude Code | `~/.claude` | `CLAUDE_CONFIG_DIR` (absolute); `set claude-dir` for `spendhist` only |

On Unix bar creates its files with mode 0600 and its dirs with 0700; on Windows they inherit the ACL of the per-user dir. Writes go to a temp file renamed into place.

In the state directory:

| Path | Written by | Holds |
|---|---|---|
| `cache/` | render, `bar refresh` | cached `gh`, `git status`, `tmux`, `%cmd%` and plugin results, locks and failure markers |
| `transcript/<hash>.json` | render, `bar refresh` | per-transcript offsets and token totals ([tokens](#tokens-cache-cachettl)) |
| `spendhist/<hash>.json` | `bar refresh` | spend history scan ([spendhist](#spendhist)) |
| `hist-5h`, `hist-7d` | render | usage samples for `spark` |
| `pct-5h`, `pct-7d`, `toggle-<plugin>`, `toggle-<plugin>.<toggle>` | `bar toggle` | toggle state: present means on |
| `click-installed`, `click-handler.applescript` | `bar click install` | handler marker, handler source |
| `click.log` | `bar click run` | refusals |
| `sessions/`, `agents/`, `records/` | `bar click hook` | [Hooks](#hooks) |

Removing the state directory resets toggles, history and caches; nothing in it is configuration.

Environment variables bar reads:

| Variable | Use |
|---|---|
| `COLUMNS` | Terminal width for layout and wrapping; passed to plugins |
| `HOME` (Windows: `USERPROFILE`, else `HOME`) | Home dir |
| `XDG_CONFIG_HOME`, `XDG_STATE_HOME`, `XDG_DATA_HOME` | Base dirs, above |
| `BAR_STATE_DIR` | State directory |
| `BAR_APP_DIR` | Where `bar click install` puts `bar-click.app` (default `~/Applications`) |
| `BAR_REFRESH_TIMEOUT` | Seconds a background `bar refresh` may run, default 20, clamped to 1-29 |
| `CLAUDE_CONFIG_DIR` | Claude Code config dir: `settings.json`, `.claude.json`, `sessions/`, `projects/` |
| `ANTHROPIC_API_KEY` | Only tested for presence, by `account` |
| `TMUX`, `TMUX_PANE` | `tmux` placeholder |
| `TZ` | Local zone for `spendhist` (Unix) |
| `PATH`, `PATHEXT` (Windows) | Plugins, `gh`, `git`, `tmux` |
| `GIT_DIR`, `GIT_WORK_TREE`, `GIT_COMMON_DIR`, `GIT_CONFIG_COUNT`, `GIT_CONFIG_PARAMETERS`, `GIT_CONFIG_SYSTEM`, `GIT_CONFIG_NOSYSTEM` | When any is set, `repo` and `branch` run `git` instead of reading the repository files, and `dirty`, `ahead`, `worktree` render empty ([Performance](technical.md#performance)) |

`FORCE_HYPERLINK` is read by Claude Code, not bar; see [Click handler](#click-handler-macos).
