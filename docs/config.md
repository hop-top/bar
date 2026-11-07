# Configuration notes

## usage

`#{usage[type=5h|7d|h|d, view=bar|pct|time, width=5, click=toggle|none]}`

- `bar` and `pct` read `2h13m` then the view; `time` is the time alone. Expired or missing windows render nothing.
- Colour follows the pace score of `rule usage[type=5h]` / `rule usage[type=7d]`; without a rule the built-ins apply (`early warn=20% bad=5%`, final `1h` / `1d` with `bad=30%`).
- Toggle state: `pct-5h` / `pct-7d` files (mode 0600) in the state directory (`BAR_STATE_DIR`, else the kit state dir for `bar`). While a file exists, `bar` and `pct` swap. `bar toggle <5h|7d>` flips it.
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

## bar check

`bar check` lints the config without rendering and prints the file it read (or says the built-in layout is in use).

- Parse errors with line numbers. An unreadable file is one error without a line.
- Layout placeholders (`left`, `center`, `right`) and `default` lines: unknown placeholder, unknown option, bad option value. Plugin placeholders are checked against their `--ext-info` schema.
- `prs` / `issues`: invalid `limit`, invalid `author` / `assignee` / `label` expression.
- Warnings: `gh` not on `PATH` while `prs` or `issues` is used; an expression that expands to more than 8 queries (only the first 8 run).
- Options holding `%command%` or `%$VAR%` are skipped; they resolve at render time.

Exit 0 when there are no errors (warnings allowed), 2 on a config error. `--format json` prints `{path, ok, issues: [{level, line, message}]}`; `line` is 0 when an issue has no line.

## bar preview

`bar preview [--state early|burning|final]` renders your layout with built-in sample data: model, context, usage windows, repo, branch, PRs, issues and account label. It reads no session, account, git repository, `gh` or plugin, and writes no state. Plugin placeholders render empty.

- `early`: plenty of budget; bars green.
- `burning`: spending fast; bars orange to yellow.
- `final`: last stretch with budget left over; bars red.

Width comes from `COLUMNS`, else 120.
