# Configuration notes

## usage

`#{usage[type=5h|7d|h|d, view=bar|pct|time, width=5, click=toggle|none]}`

- `bar` and `pct` read `2h13m` then the view; `time` is the time alone. Expired or missing windows render nothing.
- Colour follows the pace score of `rule usage[type=5h]` / `rule usage[type=7d]`; without a rule the built-ins apply (`early warn=20% bad=5%`, final `1h` / `1d` with `bad=30%`).
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

## Clickable commands

`model`, `effort` and `ctx` link to a slash command typed into the session they belong to: `/model`, `/effort`, `/compact`. On by default; off with `click=none` or while the handler cannot route them: the `click-installed` marker must read `cmd` (written by `bar click install`); a marker from a toggle-only install keeps usage toggles and no command links, until `bar click install` is run again.

- Link: `bar://cmd/<command>?tty=<ttysN>&sid=<session id>`. Emitted only on macOS, and only when the parent process has a `ttysN` terminal and the session id is lowercase hex and dashes.
- Allowlist: exactly `model`, `effort`, `compact`, fixed in core. Options, config and plugins cannot add to it; any page or app can open a `bar://` URL, so the allowlist and the checks below are the boundary.
- `bar click run <url>` (the handler calls it) fails closed. It refuses a malformed link (scheme, host and path exact; `tty` and `sid` once each and nothing else; no encoded or control characters), a command off the list, a session that is missing, dead or on another terminal, no tmux pane showing the session, a busy session, a session waiting on a permission prompt, and a non-empty input box (draft or open menu). A refusal posts a notification, appends to `click.log` in the state directory and exits 1 (the handler app wraps the call in `try`, so no error dialog appears); it never touches the terminal.
- Pane: the one on the session's own terminal, else the pane whose screen carries `sid=<id>` (a background session shown through another client). A pane found by id must still show it on the second pass.
- Idle and empty box are checked twice, a second apart, then `tmux send-keys -l "/<command>"` and `Enter`.
- Empty box: the line right above the last `❯` prompt marker line is the top border (only `─`, at least 20 cells); the bottom border is the last line below with the same width. After the marker only dim placeholder text may follow (outside dim spans only whitespace and SGR codes; reverse video refuses; `\e[0m`, `\e[00m` and `\e[m` all reset), and every line between the marker and the bottom border is blank, so a draft starting with an empty line or holding a typed `─` rule refuses. Missing borders refuse. The parser reads screen output: `tests/fixtures/sessions/synthetic/` holds hand-made panes (`empty*.ansi` must parse as empty, every other file must refuse); real captures go in a dir named after the Claude Code version (`claude --version`), and the test checks every dir there.
- Waiting state: `bar click hook` reads Claude Code hook JSON on stdin and keeps `sessions/<session id>` (mode 0600) in the state directory: set on a permission request, permission or elicitation notification, `AskUserQuestion` or a failed stop; cleared on prompt submit, session end, and the main agent's stop or tool result.

`bar click install` also wires the hook: it adds a `'<bar>' click hook` command entry under `hooks` in Claude Code's `settings.json` (`CLAUDE_CONFIG_DIR`, else `~/.claude`) for each event above that lacks one, prints every addition before writing, re-reads the file right before writing (after the app is built) so a concurrent edit is kept, copies it to a unique `settings.json.bak-<time>-<pid>`, and writes atomically, keeping the file's mode. A symlinked `settings.json` is written through to its target; a link to a missing file is refused. A file it cannot parse as Claude Code settings is left untouched and the install stops. `--dry-run` prints the additions and writes nothing.

CI covers links, parsing, every refusal, pane choice, the double check and the keys sent with a fake environment; nothing in the tests runs tmux, `osascript` or `ps`. Checked by hand, in a Claude Code session inside tmux after `bar click install`. During each check, save the pane with `tmux capture-pane -p -e -t <pane> > tests/fixtures/sessions/<version>/<name>.ansi` (`empty.ansi`, `draft.ansi`, `menu.ansi`, ...) and commit it, so the parser is tested against the real screen of that version:

1. Click `model`, `effort` and `ctx` with an empty input box: `/model`, `/effort` and `/compact` are typed and run.
2. Type a draft without sending it, then click `model`: a "not sent" notification appears, the draft is untouched, and `click.log` reads `the input box is not empty`.
3. Trigger a permission prompt (a tool call needing approval), then click `model`: a "not sent" notification appears, the prompt is untouched, and `click.log` reads `session is waiting for you`.
