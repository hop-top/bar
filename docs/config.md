# Configuration notes

## usage

`#{usage[type=5h|7d|h|d, show=time,bar, alt=time,pct, count=left|used, glyphs=blocks, width=5, click=toggle|none]}`

- `show` lists the components in order from `time`, `bar`, `pct` (`show=bar,pct`, `show=pct,time`); default `time,bar`. `view` stays as an alias: `bar` is `time,bar`, `pct` is `time,pct`, `time` is `time`; `show` wins when both are set. `count=used` makes the percentage and the bar fill show used instead of remaining; the colour still follows the pace score of what remains. `glyphs`: `blocks` (█░), `shades` (▓░), `dots` (●○), `squares` (■□), `bars` (▰▱), `braille` (⣿⣀), `eighths` (█ with a ▏▎▍▌▋▊▉ last cell); an unknown name draws `blocks` and `bar check` flags it.
- `bar toggle` (a click on the bar or percentage) flips between `show` and `alt`; `alt` defaults to `show` with `bar` and `pct` swapped. Time stays outside the link. Expired or missing windows render nothing.
- Expired or missing windows render nothing.
- The bar is one colour: every filled cell takes the colour of the exact value, the same as `pct`; empty cells are muted. The colour follows the pace score of `rule usage[type=5h]` / `rule usage[type=7d]`; without a rule the built-ins apply (`early warn=20% bad=5%`, final `1h` / `1d` with `bad=30%`).
- Toggle state: `pct-5h` / `pct-7d` files (mode 0600) in the state directory (the kit state dir for `bar`; `BAR_STATE_DIR` overrides it, meant for tests). While a file exists, `bar` and `pct` swap. `bar toggle <5h|7d>` flips it.
- Link `bar://toggle/<key>` is emitted only when the handler app is installed (marker `click-installed` in the state directory) and `click` is not `none`.

## Click handler (macOS)

`bar click install` builds `~/Applications/bar-click.app` (`BAR_APP_DIR` overrides the directory) from an AppleScript `on open location` handler, sets `LSUIElement` and the `bar` URL scheme with `plutil`, signs it ad hoc, registers it with `lsregister`, then writes the marker. Elsewhere it prints a message and exits 0. `--dry-run` prints the script and commands and changes nothing.

CI covers the generated script, plist edits, command list and platform guard with fake runners. The install itself is checked by hand:

1. `bar click install --dry-run`: read the script and commands.
2. `bar click install`.
3. `open bar://toggle/5h`: no Dock icon appears; `pct-5h` shows up in the state directory.
4. `open bar://toggle/5h` again: the file is gone.
5. `open bar://toggle/zz`: nothing changes.
6. In a Claude Code session, Cmd-click a `usage` bar (iTerm2): the view flips on the next refresh.

## Config file

Search order, first existing wins:

1. `$XDG_CONFIG_HOME/bar/bar.conf`
2. `~/.config/bar/bar.conf`
3. kit config dir for `bar` (macOS: `~/Library/Application Support/bar/bar.conf`)

No file: built-in layout. A file with an error makes `bar render` print `bar: config error, run bar check`.

## Directives

| Directive | Form |
|-----------|------|
| `set` | `set <key> <value>`; keys `left center right separator gap wrap refresh color.good color.warn color.bad ramp` |
| `default` | `default <placeholder> key=value ...` |
| `account` | `account "<glob>" <label> <color>` |
| `rule` | `rule usage[type=5h] early warn=20% bad=5% final=1h bad=30%` |
| `plugin` | `plugin <name> key=value ...` |

Quote values with spaces. `#` starts a comment.

- `separator`: changes only where segments split (the text a layout is cut at). Output between segments stays ` │ `. Default `│`.
- `wrap`: `auto`, `on` or `off`; reserved, only `auto` behaviour exists.
- `refresh`: default ttl floor for plugins, default `5s`. `gh` lists use 120 s and `%cmd%` values 1 h, whatever this says.

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
| `repo`, `branch` | `link` | on | Link to the repository / branch page |
| | `max` | none | Truncate to this many characters with `…` |
| `usage` | `type` | `5h` | `5h`, `7d`; `h`, `d` shorthand |
| | `view` | `bar` | `bar`, `pct`, `time`; alias of `show` |
| | `show` | `time,bar` | list of `time`, `bar`, `pct` |
| | `alt` | `show`, bar and pct swapped | list of `time`, `bar`, `pct` |
| | `count` | `left` | `left`, `used` |
| | `glyphs` | `blocks` | `blocks`, `shades`, `dots`, `squares`, `bars`, `braille`, `eighths` |
| | `width` | `5` | Bar cells, clamped to 1..40 |
| | `click` | `toggle` | `none` turns the toggle link off; the link appears only once the handler is installed (`bar click install`) |
| `sessions`, `spend` | none | | No options |
| `model`, `effort`, `ctx` | `click` | on | `none` turns the command link off; the link appears only once the handler is installed (`bar click install`) |

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

`bar preview [--state early|burning|final]` renders your layout with built-in sample data: model, context, usage windows, repo, branch, PRs, issues and account label. It reads no session, account, git repository, `gh` or plugin, and writes no state. Plugin placeholders render empty.

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

`bar click install` also wires the hook: it adds a `'<bar>' click hook` command entry under `hooks` in Claude Code's `settings.json` (`CLAUDE_CONFIG_DIR`, else `~/.claude`) for each event above that lacks one, prints every addition before writing, re-reads the file right before writing (after the app is built) so a concurrent edit is kept, copies it to a unique `settings.json.bak-<time>-<pid>`, and writes atomically, keeping the file's mode. A symlinked `settings.json` is written through to its target; a link to a missing file is refused. A file it cannot parse as Claude Code settings is left untouched and the install stops. `--dry-run` prints the additions and writes nothing.

CI covers links, parsing, every refusal, pane choice, the double check and the keys sent with a fake environment; nothing in the tests runs tmux, `osascript` or `ps`. Checked by hand, in a Claude Code session inside tmux after `bar click install`. During each check, save the pane with `tmux capture-pane -p -e -t <pane> > tests/fixtures/sessions/<version>/<name>.ansi` (`empty.ansi`, `draft.ansi`, `menu.ansi`, ...) and commit it, so the parser is tested against the real screen of that version:

1. Click `model`, `effort` and `ctx` with an empty input box: `/model`, `/effort` and `/compact` are typed and run.
2. Type a draft without sending it, then click `model`: a "not sent" notification appears, the draft is untouched, and `click.log` reads `the input box is not empty`.
3. Trigger a permission prompt (a tool call needing approval), then click `model`: a "not sent" notification appears, the prompt is untouched, and `click.log` reads `session is waiting for you`.
