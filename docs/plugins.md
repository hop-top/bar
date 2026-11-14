# Plugins

Any executable named `bar-<name>` on `PATH` (on Windows `bar-<name>.exe`, `.cmd`, `.bat` or `.com`) becomes the placeholder `#{name}`. The protocol follows the kit `ext-discover` contract (`--ext-info`) and extends it with a `placeholder` capability, an `options` schema, a `ttl`, and a render call with JSON on stdin.

## Discovery

- `PATH` entries are scanned in order; relative entries are ignored. After them comes the directory holding the running `bar` (symlinks resolved), so plugins shipped beside `bar` are found without being on `PATH`. A copy on `PATH` wins over one beside `bar`.
- Per name, the first executable regular file wins; non-executables are skipped.
- Windows: a file is executable when its extension is one of `.com`, `.exe`, `.bat`, `.cmd` listed in `PATHEXT` (default `.COM;.EXE;.BAT;.CMD`); the name is the file name without `bar-` and the extension, case ignored in the extension. In one dir the extension listed first in `PATHEXT` wins. Extension-less files (shebang scripts) and `.ps1` are skipped, since Windows cannot start them directly; wrap a PowerShell script in a `.cmd`, e.g. `bar-hello.cmd` holding `@powershell -NoProfile -File "%~dp0bar-hello.ps1" %*`. `.bat`/`.cmd` plugins run through `cmd.exe`, so `echo` output ends in CRLF; bar trims it.
- Names are `[A-Za-z0-9_-]+`. Core placeholder names (every name in [config](config.md#placeholders): `account`, `repo`, `model`, `usage`, `tokens`, `cwd` and the rest) always win: `bar-account` is never used. `bar-tray`, the desktop tray shipped beside `bar`, is not a plugin either.
- `bar plugin list` shows every plugin found (name, version, description, path); `bar plugin info <name>` runs `--ext-info` now and shows the parsed result plus the path, or exits 3 when no such plugin is on `PATH`. Both accept `--format json` (also `yaml`, `csv`, `text`).

### Commands

`bar <name> <args>` runs the plugin found for `<name>` (as above) with the arguments after it, with bar's standard input, output and error, and exits with the plugin's status, as `git` runs `git-<name>`. A name that is no plugin but is an alias a plugin declares (`aliases`, below) runs that plugin; a plugin named so always wins over an alias, and an alias two plugins declare is a usage error naming both. A built-in command of bar always wins (`bar check` is never a plugin's), and a name that is neither is a usage error (exit 2). The protocol calls (`--ext-info`, a render on stdin, `--check`, `--action`) are made by bar on `bar-<name>` directly.

## `--ext-info`

`bar-<name> --ext-info`, stdin closed, 5 s limit. Stdout is one JSON object:

```json
{
  "name": "tlc",
  "version": "0.1.0",
  "description": "Open tasks of the current track",
  "capabilities": ["discover", "placeholder"],
  "options": {
    "status": { "type": "enum", "enum": ["open", "all"], "default": "open" },
    "limit":  { "type": "int", "default": 5 },
    "verbose": { "type": "bool" },
    "label":  { "type": "string" }
  },
  "ttl": "2m",
  "cache": "session"
}
```

| Field | Meaning |
|---|---|
| `name`, `version` | Required strings. Without them, or on timeout or non-zero exit, `bar` falls back to the file name, an empty version and no capabilities. |
| `capabilities` | Must include `placeholder`, or `bar` never renders the plugin. `check` adds the [`--check`](#check) call; `menu` adds the [`--menu`](#menu) call. |
| `description` | Optional string, shown by `bar plugin list`. |
| `options` | Allowed options; keys, choices and string defaults are sanitised like `text`. Types: `int` (sent as a JSON integer), `string`, `bool` (written `true`/`yes`/`on`/`1` or `false`/`no`/`off`/`0`, sent as JSON `true`/`false`), `enum` (with `enum` choices, sent as the string). `default` fills a missing option when it fits the type. Options of unknown type are dropped. `icon`, `bg`, `fg`, `bold`, `gradient` and `click` are reserved: declared, they are dropped and `bar check` warns. |
| `icon` | Optional default segment icon, used when `set icons` is not `none`. Sanitised; at most 2 cells. |
| `aliases` | Optional. Other names `bar <name> <args>` answers to: up to 4 of `[a-z0-9-]`, at most 16 characters each, e.g. `["cpa"]`. They apply to that command only, not to placeholders or config. A built-in command of bar and a plugin of the same name win over an alias. Looked up only when no plugin is named so; the answer is cached like the rest of `--ext-info` |
| `actions` | Optional. Actions a click can run: `{"use": {"description": "Prefer this account", "tokens": ["account", "provider"], "confirm": false, "timeout": 120}}`. Names are letters, digits, `-` and `_`; tokens are up to 8 names of `[a-z0-9_-]`. See [Actions](#actions). `confirm` makes the [tray](tray.md) ask before running the action; a status-line click runs it without asking. `timeout` (seconds, at most 120) lifts the 10 s limit for an action that waits on the user, such as a chooser. `toggle` names the output's `label_toggle` the action fronts; it needs a `session` token, else it is ignored. See [Actions](#actions) |
| `inputs` | Optional. What the output depends on besides options: `{"env": ["ANTHROPIC_BASE_URL"], "session": ["model_id"]}`. Each declared value is part of the cache key, and the declared environment variables are sent as `env` in the render input. `env`: up to 8 names of `[A-Z0-9_]`, at most 64 characters; a name containing `KEY`, `TOKEN`, `SECRET`, `PASSWORD`, `PASSWD`, `CREDENTIAL` or `AUTH` is refused, since values go into the cache key and the refresh process's arguments (a plugin that needs a secret reads its own environment, which it inherits); values longer than 1 KiB count as unset. `session`: any of `cwd`, `model`, `model_id`, `effort`, `session_id`; the volatile fields (`context_used`, `five_hour`, `seven_day`) cannot be declared, they would split the cache on every render. `accounts`: `true` asks for the user's `account "<glob>" <label> <color>` lines, see [Account rules](#account-rules). `account`: `true` asks for the session's account, see [Session account](#session-account). |
| `timeout` | Optional. Limit for the background render, a number of seconds or a duration (`8s`), clamped to 1 to 15 s. Default 3 s; zero or unparsable means the default. |
| `ttl` | How long a render result stays fresh: seconds (`300`) or a duration (`90s`, `5m`, `1h`, `1d`). Default 60 s; zero or unparsable means the default. |
| `cache` | Optional. `"session"`: the output depends on the Claude Code session, so results are cached per session id and the render input carries `session.record`. Any other value or none: shared across sessions. |
| `web` | Optional. What the [web configurator](web.md) shows for the plugin, which it cannot run: see [Web configurator metadata](#web-configurator-metadata). Without it the plugin still works everywhere, and the page and `bar edit` preview it as its name. |

The result is cached for a day per binary path and modification time, so a changed binary is asked again at once; renders never run `--ext-info` themselves. A cold cache renders nothing and asks in the background. Output that does not parse, a non-zero exit or a timeout counts as a failed refresh (kept value, 60 s backoff); `bar plugin info` and `bar check`, which run it directly, fall back to the file name, an empty version and no capabilities.

### Web configurator metadata

The web configurator runs bar's renderer in the browser, where no plugin can run. A plugin can describe itself for it with a `web` object in `--ext-info`; every field is optional, and unknown fields are ignored.

```json
"web": {
  "description": "Closed tasks of the session track",
  "category": "tasks",
  "samples": [
    { "when": { "show": "track" }, "output": { "label": "◉", "text": "web-ui", "score": 77 } },
    { "output": { "label": "◉", "icon": "\uf45d", "text": "7∕9", "alt": "web-ui", "score": 77 } }
  ],
  "options": {
    "status": { "label": "Status filter", "help": "Comma-separated statuses to count", "placeholder": "DONE,CANCELLED", "choices": ["DONE", "DONE,CANCELLED"] }
  }
}
```

| Field | Meaning |
|---|---|
| `description` | Picker text; the top-level `description` when absent. |
| `category` | Picker group, a short word (`git`, `tasks`, `system`, ...). |
| `samples` | Preview outputs, in order. Each has `output`, an object in the [render output](#render) shape (`text` required; `label`, `icon`, `style`, `score`, `metric`, `link`, `alt`, `label_toggle`, `text_toggle`), and optionally `when`, an object of option values. The preview uses the first sample whose `when` values all equal the placeholder's options, after schema defaults are filled in; a sample without `when` matches any options. Values are written as in a layout (`"true"`, `"3"`) or as JSON of the option's type. The output goes through the same paint code as a real render: style and score colours, label, icons, skins. With no match, or no samples, the preview shows the plugin name. A toggle is never on in the preview, so `text` shows, not `alt`. |
| `options` | Hints per declared option: `label` (shown instead of the key), `help` (tooltip), `placeholder` (example in an empty field), `choices` (suggested values; each must be a valid value of the option, so for an `enum` a subset of `enum`). The option schema itself stays in `options` at the top level. |

`bar check` validates the `web` object of each plugin a config uses and reports problems as warnings (``plugin `tlc`: web: samples[0]: ...``): a non-object, a non-string `description`, `category`, `label`, `help` or `placeholder`, a sample without a valid `output`, a `when` or hint for an undeclared option, or a `when` value or choice the option does not accept.

`bar web plugins` prints `plugins.json`, the `--ext-info` of every placeholder plugin bar finds on `PATH` (or in each `--dir DIR` instead), `web` objects included; `-o FILE` writes it to a file, atomically; an existing file is kept unless `--force`. It reads the cached `--ext-info` when fresh and otherwise runs it now (5 s limit), storing the result for later renders. Load the file into the web configurator to compose and preview with your plugins. [`bar edit`](config.md#bar-edit) previews plugins from the same samples, reading `--ext-info` the same way for the plugins its config uses.

## Render

`bar-<name>` with no arguments, run directly (no shell), 3 s limit (`timeout` in `--ext-info` sets up to 15 s). Stdin:

```json
{
  "options": { "status": "open", "limit": 5 },
  "session": {
    "cwd": "/path/to/repo", "model": "Opus", "model_id": "claude-opus-…",
    "effort": "high", "session_id": "…", "context_used": 41.0,
    "five_hour": { "used_percentage": 12.0, "resets_at": 1800000000 },
    "seven_day": null,
    "record": { "task": "T-0042", "cwd": "/path/to/repo", "at": 1800000000 }
  },
  "columns": 160,
  "config": { "org": "…", "keychain": "entry-name" },
  "toggles": { "scope": true },
  "env": { "ANTHROPIC_BASE_URL": "http://127.0.0.1:8317" }
}
```

- `options`: inline options over `default <name>` lines, `%…%` expanded, validated against the schema and typed, schema defaults filled in. An unknown key or a bad value omits the segment.
- `session`: fields of the statusline JSON. `cwd` is `workspace.current_dir`, else `cwd`; `model` is `model.display_name`, else `model.id`; `model_id` is `model.id`; `effort` is `effort.level`; `context_used` is `context_window.used_percentage` (`null` when absent); `five_hour` / `seven_day` are `rate_limits.five_hour` / `rate_limits.seven_day` (`null` when absent). Missing strings are `""`.
- Options are separated by `;` and a list value separates its items with `,`: `#{name[show=a,b;verbose]}` sends `show` as `a,b` and `verbose` as an empty flag.
- `config`: the plugin's own `plugin <name> k=v` line, as strings, without `ttl`. One line per plugin: a later `plugin <name>` line replaces an earlier one whole (`bar check` warns), so all options go on one line. Nothing else from the config or the environment goes in; secrets stay in the keychain, and config holds only entry names.
- `env`: only for plugins declaring `inputs.env`: the declared variables that are set, as strings; unset ones are absent. The key is absent without a declaration.
- `columns`: `COLUMNS`, or `null`.
- `accounts`: only for plugins declaring `inputs.accounts`, and only when the user has `account` lines: see [Account rules](#account-rules).
- `account`: only for plugins declaring `inputs.account`, and only when bar knows the session's account: see [Session account](#session-account).
- `toggles`: the plugin's named toggles that are on in this session, from `bar toggle <name>.<toggle>` (see `label_toggle` and `text_toggle` below): the session's own state, else the global default ([toggles](config.md#usage)); an off toggle is absent, and only this plugin's keys are sent. They are part of the cache key, so a flip is a new entry (sessions that see the same toggles share one): it shows at the next render once the plugin has run for it (the first render after a flip is empty until then, a flip back is immediate while the old entry is cached).
- `session.record`: only for plugins declaring `"cache": "session"`, and only when the state directory holds `records/<name>/<session id>` (a JSON object, at most 4 KiB), written by `bar click hook`. Today the hook writes one for `tlc` only (see [Hooks](config.md#hooks)), keeping any fields it does not own, such as a picked track. Absent otherwise.

Stdout is one JSON object:

```json
{ "label": "[x]", "label_toggle": "scope", "text": "7∕13", "alt": "track-a", "style": "good", "link": "https://…", "score": 54,
  "metric": { "remaining": 14907, "granted": 20000, "expires_at": 1800000000, "granted_at": 1797400000 } }
```

- `items` (optional, an array of at most 16 objects) replaces the output's own `text`: each item is an output of its own (`label`, `label_color`, `icon`, `text`, `alt`, `style`, `score`, `metric`, `link`, `label_toggle`, `text_toggle`) drawn like one, items joined by one space. With `set wrap on`, a line too wide for `COLUMNS` may break between items, never inside one; the next row starts at the left like any wrapped row. Without wrapping the items stay on one row. An item without a string `text`, or with nothing visible, is left out; items do not nest. With `items`, the top-level `text` may be left out (it is not drawn).
- `tokens` (optional, an object) are the values an [action](#actions) takes: strings of at most 64 characters from `A-Za-z0-9._@-`, names from `a-z0-9_-`, at most 8; anything else is dropped. Each item can carry its own; the top-level ones fill in what an item lacks.
- `account` (optional) names an account for `#{account}`: see [Account](#account).
- `usage` (optional) carries usage windows for `#{usage}`: see [Usage windows](#usage-windows).
- `text` (required, a string, unless `items` is given) is stripped of escape and control characters (tabs and line breaks become spaces), line and paragraph separators (U+2028, U+2029) and Unicode format characters (bidi marks and overrides, zero-width spaces, soft hyphens, BOM; the joiners U+200C and U+200D and emoji tag characters are kept); wide characters (CJK, emoji) count as 2 cells. Nothing visible left omits the segment.
- `alt` (optional) is shown instead of `text` while the plugin's toggle is on in the session: `bar toggle <name>` flips the global `toggle-<name>` in the state directory, and a click on the text opens `bar://toggle/<name>?sid=<session id>`, flipping it in that session only, once the handler is installed (`bar click install`), unless the output has its own `link` or the placeholder sets `click=none`. Sanitised like `text`; colour, label and icon stay the same. Without `alt` there is no toggle. Both texts come from one render, so a toggle shows at once, without a refresh.
- `click` is a reserved option name: `toggle` (default) or `none`, never sent to the plugin; a `click` entry in `options` is ignored, and `bar check` warns about it.
- `label` (optional) is sanitised like `text` and drawn dim, plus a space, before `text`. It sits outside the link. Empty or absent: `text` alone.
- `label_color` (optional) draws the `label` in that colour instead of dim: a 256-colour code, `#rrggbb` or an ANSI name, as an [`account` line](config.md#account-agents-sessions-tmux) takes one; anything else is ignored. An icon that replaces the label stays dim.
- `icon` (optional) overrides the `--ext-info` icon for this render. When an icon shows (`set icons` not `none`, or `icon=<glyph>` on the placeholder; `icon=none` hides it), it replaces `label`: dim, a space, then `text`, outside the link. An icon wider than 2 cells is dropped. `icon` is a reserved option name: it is never sent to the plugin, an `icon` entry in `options` is ignored, and `bar check` warns about it. An invalid output icon falls back to the `--ext-info` icon, then to `label`.
- `label_toggle` (optional) names a toggle for the drawn label or icon: a key like a plugin name (letters, digits, `-`, `_`, at most 64; otherwise ignored). When the click handler is installed (`bar click install`) and `click` is not `none`, the label or icon (not `text`) is wrapped in an OSC 8 link `bar://toggle/<plugin>.<toggle>?sid=<session id>`, e.g. `tlc.scope`; a click flips it in that session only, `bar toggle tlc.scope` flips the global default `toggle-tlc.scope`, and the flag reaches the plugin in `toggles`. The text toggle (`alt`, `bar://toggle/<plugin>`) is unchanged.
- `text_toggle` (optional) names a toggle for the text, validated like `label_toggle`. Under the same conditions the text links to `bar://toggle/<plugin>.<toggle>?sid=<session id>` instead of the `alt` toggle (an own `link` still wins), and the `alt` toggle moves to the label or icon, unless `label_toggle` holds it. `bar toggle <plugin>` still flips `alt`, and the flag reaches the plugin in `toggles`. Without `text_toggle` the links are as above.
- `link` becomes an OSC 8 link around `text` only, never the `label`; a URL with control characters leaves plain text.
- Colour, first match wins:
  1. `style`: `good`, `warn`, `bad` (the `color.*` palette) or `dim`. Any other value means no colour, and it still wins over `score` and `metric`.
  2. `score`: clamped to 0..100, then the `ramp`.
  3. `metric` (`remaining`, `granted`, `expires_at` required; `granted_at` optional) with a `rule <name>` whose target is exactly the placeholder name, e.g. `rule claude-credits early pace final=3d bad=$50`. Amounts are cents; times are Unix seconds. In the final stretch the score is `100 - remaining * 100 / (bad + 1)`, so unspent credit turns red before it expires. Before that, the pace score of the percent left of `granted`, with `$` thresholds read as shares of the grant; `granted_at`, when present, gives the pace period (`expires_at - granted_at`); without it only the floors apply.
  4. Otherwise plain.

### Usage windows

A plugin that knows a quota Claude Code does not report (a proxy's pool, say) can put it in `usage`, shaped like the status line's `rate_limits`:

```json
{ "text": "", "usage": { "five_hour": { "used_percentage": 41.0, "resets_at": 1800000000 },
                         "seven_day": { "used_percentage": 12.5, "resets_at": 1800400000 } } }
```

`used_percentage` is a number, clamped to 0..100; `resets_at` is Unix seconds; each window is optional, and a window without both is ignored. With `set usage-source <plugin>` in the user's config, `#{usage}` shows these windows when Claude Code sends none: see [Plugin windows](config.md#plugin-windows). Only the result for the placeholder's `default` options counts, so a plugin should put `usage` in every output, whatever its options. `text` may be empty.

### Actions

A plugin declares actions in `--ext-info` (`actions`) and supplies the values they take in its output (`tokens`, top level or per item). The user binds one to a placeholder in their config with `action <placeholder> <plugin>.<action>` (see [Plugin actions](config.md#plugin-actions)); bar then draws the placeholder, or each item with all the tokens the action takes, as a link, and a click runs

```
bar-<plugin> --action <action>      # stdin: {"action": "<action>", "tokens": {"account": "a1b2", "provider": "claude"}}
```

directly, with a 10 s limit (the action's declared `timeout`, at most 120 s, when it waits on the user) and nothing on stdout needed; the first line it prints, if any, is added to the status (`ok: <line>`, printable characters, at most 80). Exit 0 is success; any other exit is a failure, shown in `action-status` in the state directory. bar has already checked that the user bound the action, that the plugin declares it, and that the token values are in the plugin's cached output, but a plugin should validate its tokens itself: it runs with the user's rights. A click's stdin also holds `output`, the cached output the link was drawn from, as the plugin printed it: a plugin that needs a placeholder option at click time (a label template, say) puts it in its output, which the core ignores beyond the documented fields. A tray item's run has no `output`. The action runs in the background of a click, never on the render path, and the plugin's output refreshes at the next render.

An action declared with `"toggle": "<name>"` fronts that named toggle. Where it is bound, the drawn label or icon of an output whose `label_toggle` is `<name>` (and that carries the action's tokens) links to the action instead of `bar://toggle/<plugin>.<name>`; the text keeps its own link. Once it exits 0, bar flips `<plugin>.<name>` in the session its `session` token names, unless the first line it printed starts with `cancelled` or `kept`. `bar toggle <plugin>.<name>` still flips the toggle without running anything. `bar-tlc`'s `pick` is one: see [Choosing the track](#choosing-the-track).

### Check

A plugin with the `check` capability answers `bar-<plugin> --check` (5 s limit, no stdin) with a JSON array of strings or `{"warnings": ["..."]}`: problems with its own configuration, found without a network call (a missing key file, a remembered refusal). `bar check` shows each, sanitised, at most 8 of 200 characters, as a warning for a plugin the layout uses.

### Menu

A plugin with the `menu` capability answers `bar-<plugin> --menu` with the groups and items a desktop menu shows for it: a picker, toggles, buttons and information rows. Each item names one of the plugin's declared [actions](#actions) and the token values it takes; choosing the item runs that action exactly as a click does. bar asks only plugins the user names in a `tray` line, and the [tray](tray.md) runs only the actions those lines grant. The call is made directly (no shell), with a 3 s limit, `BAR_STATE_DIR` set, and this on stdin:

```json
{ "options": { "provider": "auto" }, "toggles": { "scope": true },
  "accounts": [{"glob": "*@example.test", "label": "work", "color": "33"}] }
```

- `options`: the placeholder's `default <name>` options, typed and with schema defaults filled in, as a render gets them.
- `toggles`: the plugin's named toggles that are on, as in a render.
- `accounts`: only for plugins declaring `inputs.accounts`, and only when the user has `account` lines: see [Account rules](#account-rules). Absent otherwise.
- Nothing from the session: there is none. A menu covers everything the plugin is configured for (every proxy instance, say), not the instance one session happens to use. Unknown fields are ignored.

Stdout is one JSON object:

```json
{ "groups": [
  { "id": "claude@8317", "label": "Claude · :8317", "kind": "radio", "items": [
      { "id": "3", "label": "alice", "detail": "5h 59% ↻2h10m · 7d 71%", "checked": true,
        "action": "use", "tokens": { "account": "3", "provider": "claude" } },
      { "id": "4", "label": "bob", "detail": "5h 12% ↻4h02m · 7d 40%",
        "action": "use", "tokens": { "account": "4", "provider": "claude" } } ] },
  { "id": "next@8317", "kind": "buttons", "items": [
      { "id": "next", "label": "Use most quota left", "action": "next", "tokens": { "provider": "claude" } } ] },
  { "id": "rtellm@8317", "label": "Routing", "kind": "check", "items": [
      { "id": "toggle", "label": "Routing", "checked": true,
        "action": "toggle", "tokens": { "instance": "9f2c", "to": "off" } } ] },
  { "id": "info", "kind": "info", "items": [
      { "id": "auth", "label": "management refused the key", "style": "bad" } ] } ] }
```

- `groups` (required, an array of at most 16 objects): the sections of the menu, in order. Each has `id`, an optional `label` (a heading) and `kind`:
  - `radio`: a picker, one item checked (the first checked one when several are);
  - `check`: toggles, each item checked or not;
  - `buttons`: one-shot actions;
  - `info`: disabled rows, text only; their items carry no action.
- `items` (required, an array of at most 16 objects per group): the rows. Each has `id`, `label` (required, visibly non-empty), optionally `detail` (a second line or a tooltip, as the desktop allows), `checked` (default `false`), `enabled` (default `true`; a disabled item is shown but cannot be chosen), `style` (`good`, `warn`, `bad` or `dim`, as for a render; anything else is dropped), and outside `info` groups `action` (required, a declared action name) with `tokens`.
- `id` of a group or item: letters, digits, `_`, `@`, `:` and `-`, 1 to 32 characters, unique among its siblings (the first of a duplicated id is kept). A dot is not allowed: bar joins `<plugin>.<group>.<item>` into the one id a menu choice is run by, so an id should stay the same from one call to the next for the same thing (an account index, an instance). Stable ids keep a choice made on an older menu meaningful. An item whose joined id would be longer than 96 characters is left out of the tray's menu.
- `tokens` follow the [render output](#render) rules: names from `a-z0-9_-`, values of at most 64 characters from `A-Za-z0-9._@-`, at most 8; an invalid token is dropped, more than 8 drop the item. They must name exactly the tokens the action declares (`actions.<name>.tokens`): an item whose action is undeclared, or whose token names differ from the declaration, is dropped. An action declared with `confirm` is asked for confirmation by the menu before it runs.
- `label`, `detail` and a group's `label` are sanitised like `text` (control and format characters dropped, tabs and line breaks become spaces), trimmed and cut to 120 characters (the tray's menu keeps at most 80). Only the fields above are kept; anything else in the output is discarded.
- A group without a known `kind`, a valid `id` or any usable item is dropped; `{"groups": []}` is a valid answer (nothing to show). An output that is not an object with a `groups` array, a non-zero exit or a timeout means no menu for the plugin this time.

Rules a menu plugin follows:

- **Answered from its own cache.** The call runs every time a menu opens, so it must be cheap: read the plugin's own cache files and configuration and answer. A network call is allowed only when the plugin's own freshness rule says the cache is stale (the same rule its render uses), never on every call; it must respect the plugin's refusal records (a remembered 401 or 403) as a render does. The 3 s limit is a cap, not a budget.
- **No secrets.** The output is written to a file in the state directory and shown by a desktop process, and on some desktops every program of the user's session can read menu labels. Labels, details and tokens carry names, indices, instance ids and status text only: no keys, no addresses the user did not put in an `account` line themselves, no URLs with credentials.
- **Stable, complete.** The menu lists everything the plugin is configured for, each item identified so that the same `id` means the same thing next time, with `checked` reflecting the current state (the preferred account, whether routing is on). A toggle item carries the value to switch to (`"to": "off"` while on), and the action re-reads the current state before changing it, so an item chosen from a stale menu is a no-op, not a flip back.
- **Same action semantics.** An action run from the menu receives the same stdin as from a click (`{"action", "tokens"[, "accounts"]}`), with the same limit, and reports the same way (its first stdout line, its exit status). The plugin validates its tokens itself, as it does for a click.

### Account

`"account": {"id": "a1b2c3", "label": "alice"}`: `id` a stable identifier, `label` what to show (sanitised like `text`); both strings, `label` visibly non-empty. With `set account-source <plugin>` in the user's config, `#{account}` shows the `label` of this output in place of the signed-in account, through the user's `account "<glob>"` rules: see [account](config.md#account-agents-sessions-tmux). As for usage windows, only the result for the placeholder's `default` options counts. `color` (optional, the forms of `label_color`) says the plugin has applied the user's account rules itself: the `label` is then drawn as is, in that colour, without matching the rules again. A plugin should emit `account` only when it is certain which account is current, since the placeholder replaces what Claude Code reports.

### Account rules

A plugin that names accounts (a proxy's, say) can label and colour them the way the user's `account "<glob>" <label> <color>` lines do for the signed-in account. It declares `"inputs": {"accounts": true}` in `--ext-info` and then gets the lines, in file order (the first matching line applies), as

```json
"accounts": [{"glob": "*@example.test", "label": "work", "color": "33"}]
```

`color` is as written in a config (a code, `#rrggbb`). Where the plugin gets it:

- Render: `accounts` in the stdin input. The key is absent without lines. In the cache key the lines take part as a hash, so a changed line is a new entry and no glob is stored in clear; they are not in the refresh process's arguments either, which read the config themselves.
- Action: `accounts` in the `--action` stdin, next to `action` and `tokens`.
- `bar <name> <args>`: the environment variable `BAR_ACCOUNTS`, the same JSON array, from the user's config alone (a project `.bar.conf` is not read); absent without lines. A plugin started directly, not through `bar <name>`, has no rules.

Globs can hold email addresses. A plugin matches them where it sees the address and keeps only the label and colour it settles on: nothing from `accounts` belongs in its output (other than that label and colour), its cache or its logs. Without the declaration none of this is sent, and the cache key is unchanged.

### Session account

A plugin whose output belongs to one account (a balance, a quota) can follow the session's account. It declares `"inputs": {"account": true}` in `--ext-info` and gets, in the render input,

```json
"account": {"email": "alice@example.test", "label": "work"}
```

the account `#{account}` resolves: the account of the `set account-source` plugin when it names one, else the one Claude Code is signed in with.

- `label`: the label of the user's first `account "<glob>"` line that matches, or the source plugin's label when it came with a `color` (the plugin matched the rules itself). Absent when no line matches.
- `email`: the signed-in account's address. Absent when the source plugin names the account: it hands over a label, not an address.
- `account` is absent when bar knows no account (an API key, nobody signed in).

Each value is part of the cache key as a hash, so every account has its own entry and a switch never shows another account's result. The address is not in the refresh process's arguments: that process reads it itself and adds it only while the same account is signed in. Nothing secret is sent. Without the declaration none of this is sent, and the cache key is unchanged.

### Proxy plugins

A plugin for an API proxy that holds several accounts (one `bar-<proxy>` per proxy) follows one output contract, so layouts and rules carry over from one proxy to another:

- It detects its instance from the declared `ANTHROPIC_BASE_URL` input and is hidden when the session's base URL is not one it is configured for.
- Its secrets live in its own owner-only file, keychain or command, never in `bar.conf`, argv, the environment, its output or its cache; it stops calling the proxy's management API after a 401 or 403 until the key changes.
- It emits `items`, one per account, each with a `label` (its own alias, else the user's [`account` line](#account-rules), else the local part of the account name, or `#1`, `#2`) and, for a matched line, its `label_color`, a `text` of `<window> <percent>% ↻<countdown>` per window (the window name left out only when every account shows one window and it is the same for all), a `score` or `style`, optionally a `link`, and `tokens` `account` and `provider`; a top-level `usage` for the provider of the current model (see its `pool` option); and `account` (with its `color` when a line matched) only when the proxy's choice is certain. With exactly one instance configured it also emits, in a session that does not use any proxy, the top-level tokens of that instance and no `account`, so a bound action that takes no `account` token (a chooser, say) is linked on `#{account}` there.
- Its options use the names `provider`, `windows`, `show`, `count`, `label`, `sort`, `accounts`, `compact`, `stale`, `link` and `pool`; window ids are `5h`, `7d` and `wk`; its action is `use`, taking the tokens `account`, `instance` and `provider`.
- It offers subcommands for scripting (`list`, `use <account>`, `pick`), run as `bar <name> <args>` or, by its alias, `bar cpa <args>`.

[`bar-cliproxyapi`](plugins/cliproxyapi.md) is the reference implementation.

A plugin may also drive the proxy rather than only read it. [`bar-routellm`](plugins/routellm.md) switches a RouteLLM router behind the proxy on and off by one field of the proxy's configuration, only on an explicit action (a bound click or `bar rtellm on|off`), and follows the same rules for secrets and for the management API: its own owner-only configuration file, and no call after a 401 or 403 until the key changes. Both proxy plugins share one refusal record per management address, so two plugins with the same key cost one failed attempt, not two.

## Caching and failure

- Results are cached per plugin, working directory (`session.cwd`), options and `plugin` line, plus session id and `session.record` for plugins declaring `"cache": "session"` (a new record is a new entry, so it shows at the next refresh), plus the values of declared `inputs`; other session data does not split the cache. Results are refreshed in the background once older than `ttl`; `plugin <name> ttl=5m` overrides the plugin's value, and `set refresh` is the floor.
- A render never waits on a plugin. Timeouts kill the plugin's whole process group.
- Non-zero exit, timeout or output that does not parse keeps the last good value and backs off for a minute; with nothing cached the segment is omitted.

## Example

```sh
#!/bin/sh
if [ "$1" = "--ext-info" ]; then
  echo '{"name":"hello","version":"0.1.0","capabilities":["placeholder"],"options":{"who":{"type":"string","default":"world"}}}'
  exit 0
fi
who=$(jq -r '.options.who')
printf '{"text":"hi %s","style":"good"}\n' "$who"
```

Save as `bar-hello`, `chmod +x`, put it on `PATH`, then `set left "#{hello[who=bar]}"`. On Windows the plugin needs a `PATHEXT` extension (see Discovery): a compiled `bar-hello.exe`, or a `bar-hello.cmd`.

## Reference plugins

Five POSIX shell scripts ship in `plugins/`. They need `sh` and the tool they wrap, and all but `bar-mem` need `jq`; and are Unix plugins (extension-less, so Windows discovery skips them); put them on `PATH` (copy or symlink) to enable `#{tlc}`, `#{claude-credits}`, `#{ci}`, `#{mem}` and `#{health}`. Colour comes from core: `bar-tlc` emits `score`, `bar-claude-credits` emits `metric`. The only `style` either sets is `bad` on the credits `?`.

Each declares a `web` object with preview samples and option hints, so the [web configurator](web.md) can show it.

A compiled plugin ships beside `bar` in the release archives, Homebrew and npm: [`bar-cliproxyapi`](plugins/cliproxyapi.md) shows per-account quota of a CLIProxyAPI proxy as `#{cliproxyapi}`, one entry per account, and checks its own setup under `bar check`; [`bar-routellm`](plugins/routellm.md) shows whether a RouteLLM router behind that proxy is routing, with the alias and tier of the session, and switches it from a click or `bar rtellm on|off`.

### `bar-tlc`

`#{tlc}` shows `closed∕total` tasks of this session's track, else of the whole project; archived tasks included, and every status but `TODO` and `IN_PROGRESS` (so `DONE`, `SKIPPED`) counts as closed. Needs `tlc`; `ttl` 60 s. It runs `tlc` from the nearest ancestor of `session.cwd` holding `.tlc/config.yaml` (a stray empty `.tlc/` is skipped; a relative `cwd` falls back to the working directory) and never creates one. The search stops at the git work-tree root (the first directory holding `.git`), so a project enclosing the repository is never counted; a linked worktree also checks its repository root (the bare repo directory, else the main worktree); outside a repository only `cwd` itself is checked. With no project in scope, or a project with no tasks, it emits empty text and the segment hides. If any `tlc` call fails, it exits non-zero with no output, so the last good value stays. Emits `score` (closed share, 0..100), so none closed is red and all closed is green.

The session's track comes from `session.record`, which `bar click hook` keeps per session (see [Hooks](config.md#hooks); register them with `bar click install` or `bar click install --hooks-only`): the task the session last claimed, set `IN_PROGRESS`, completed or created. When the record's `cwd` resolves to the same project as `session.cwd`, the plugin reads the task's `track_id` (`tlc task show <id>`) and counts that track; it keeps doing so after the task is done, until the session touches another task. A track picked from the chooser (below) wins over it while the scope stays on track. With neither, a project with exactly one incomplete active track counts that track: `tlc track list --status active` and, when it lists any, one `tlc task list` counting every track's tasks, both run only then; a track is incomplete when fewer of its tasks are closed than it has (counted as the segment counts), so a track without tasks is not; otherwise (no record, a record for another project, a task without track, a task that no longer exists: `tlc task show` exit 3) the whole project counts. Declares `"cache": "session"`, so two sessions in one directory each keep their own value.

`text` and `alt` are the count (`71∕91`) and the scope name: the track slug (`tlc track show <id>`), or `all` for the whole project. The default shows the count; `bar toggle tlc` (or a click on the count, with the handler installed) shows the scope name, and the count is hidden meanwhile.

| Option | Default | Meaning |
|---|---|---|
| `scope` | `track` | `track`: the picked track, else the session's, else the only incomplete active track, else the whole project. `all`: the whole project. |
| `status` | none | Passed to `tlc task list --status` to filter the counted set; letters, digits, `_`, `,` and `-` only (not first), anything else makes the plugin fail (exit 1, no output): a warm cache keeps the last value, only a cold cache renders empty. |
| `show` | `count` | `count` or `track`: what `text` holds. |
| `alt` | the other one | `count` or `track`: what `alt` holds, shown while toggled. |
| `label-track` | `◉` | Mark before the count while the track is counted (icons off). |
| `label-all` | `☰` | Mark while the whole project is counted. |
| `icon-track` | U+F45D (nf-oct-milestone) | Mark with `set icons` not `none`. |
| `icon-all` | U+F45E (nf-oct-checklist) | Same, whole project. |
| `pick-label` | `{slug} - {done}/{total}` | Each entry of the track chooser, a template: see [Choosing the track](#choosing-the-track). |
| `click` | `toggle` | `none` drops the toggle and mark links (core option). |

A mark precedes the count and shows what it covers: `◉` (U+25C9) for the session's track, `☰` (U+2630) for the whole project, both one cell wide (East Asian Width neutral, present in common monospace fonts); the label is drawn dim before `text`. With `set icons` not `none` the Nerd Font glyphs nf-oct-milestone (U+F45D, track) and nf-oct-checklist (U+F45E, whole project) replace the label. The plugin cannot tell which icon set is active: an output icon shows whenever icons are on, so `set icons emoji` also draws the Nerd Font glyphs, which need a Nerd Font. With emoji, set `icon-track` / `icon-all` (for example `#{tlc[icon-track=🎯;icon-all=📋]}`; at most 2 cells), or `icon=none` on the placeholder to fall back to the labels; `icon=<glyph>` on the placeholder beats both icons and so drops the distinction. An empty `label-*` or `icon-*` draws no mark for that scope. The mark follows the scope actually counted: `scope=track` with no track to count shows the project mark.

The mark is a toggle (`label_toggle: "scope"`, link `bar://toggle/tlc.scope`, with the click handler installed): a click inverts the scope in that session, track to project and project to track, and `bar toggle tlc.scope` inverts it for every session that has not flipped it. With no track to count the project stays counted, whatever the toggle. The flag arrives as `toggles.scope`. A click on the count (link `bar://toggle/tlc`) swaps `text` and `alt`, the count and the scope name, at once; the two toggles are independent. After a flip the new count shows at the next render once the plugin has run for it, not at ttl.

#### Choosing the track

With `action tlc tlc.pick` in your config (and the handler installed with action support), a click on the mark runs the plugin's `pick` action instead of flipping `tlc.scope` directly; the action flips it (see [Actions](#actions)):

- Track to project: the scope flips and a picked track is forgotten.
- Project to track: the chooser lists the session's track first (selected), then the project's other active tracks, incomplete ones only (a complete or empty track, the session's included, is not offered), each drawn by the `pick-label` option (below). OK counts the chosen track in this session and flips the scope; Cancel changes nothing (`ok: cancelled` in `action-status`). With fewer than two tracks to offer there is no chooser and the scope flips.
- Track scope with nothing counted yet (no session track, two or more incomplete active tracks, the project shown): the click opens the chooser and keeps the scope; OK counts the chosen track.

`pick-label` is a template: `{name}` is replaced by the track's value; `{{` is a literal `{` and `}}` a literal `}`. It is read left to right: at each `{` the first of `{{`, `{name}` (1 to 32 lower-case letters, digits or `_`) and a lone `{` that fits wins, at each `}` the first of `}}` and a lone `}`. A lone `{` or `}` stays as it is, and an unknown name stays as written (`{nope}`), so a typo shows in the list. The default `{slug} - {done}/{total}` reads `web-ui - 7/9`.

| Placeholder | Value |
|---|---|
| `{slug}` | The track's slug. |
| `{title}` | Its title; empty for a session track that is not active. |
| `{id}` | Its id. |
| `{status}` | Its status as `tlc` lists it (`active`); empty for a session track that is not active. |
| `{done}`, `{total}` | Its closed and all tasks, counted as the segment counts (archived included, all but `TODO` and `IN_PROGRESS` closed), without the `status` filter. |
| `{n}` | The entry's position, from 1. |

The click lists the tasks once (`tlc task list --archived`) for the counts of every track, which pick the incomplete ones and fill `{done}` and `{total}`; the render counts per track only for the fallback above. An entry is cut at 120 characters, control characters become spaces, and it is passed to the chooser as an argument, never as script text. When two entries come out the same, or one is empty or reads `false` (both mean Cancel to the chooser), every entry starts with its position, `1. `, so each choice maps back to its track. The render passes a template other than the default to the action in its output (`pick_label`). bar does not expand braces (see [Values](config.md#values)): `#{tlc[pick-label={n}. {slug} — {title}]}` works inline; a value with `{{` or `}}` goes on a `default` line (`default tlc "pick-label=..."`). bar does expand `%…%` first, so a template with two `%` runs the text between them as a command: keep to one `%` per template.

The pick is kept in the session's record (`track`, `root`), so it holds when the session moves on to another task, and it ends with the session. `bar toggle tlc.scope` and `bar toggle tlc.scope off` flip the scope without asking, and never pick. The chooser is macOS `choose from list` through `/usr/bin/osascript`, like the [account chooser](plugins/cliproxyapi.md#clicking-an-account): the titles travel as arguments of a fixed script, and the action may wait for you up to 120 s. On other systems the plugin does not declare `pick`, so `bar check` reports a binding of it and the mark stays a plain toggle.

`#{tlc[scope=all]}` counts the project; `#{tlc[status=DONE]}` counts within the tasks `tlc` lists for that status; `#{tlc[show=track]}` shows the slug by default and the count when toggled.

```
set left "#{model}#{effort} │ #{ctx} │ #{repo}:#{branch} │ #{prs[limit=3;author=@me]} │ #{tlc}"
```

That is the built-in `left` with `#{issues[...]}` replaced by `#{tlc}`. To keep issues and add tasks:

```
set left "#{model}#{effort} │ #{ctx} │ #{repo}:#{branch} │ #{prs[limit=3;author=@me]} │ #{issues[limit=3;assignee=@me]} │ #{tlc}"
```

### `bar-claude-credits`

`#{claude-credits}` shows the prepaid Console balance, e.g. `$149.07`, with the days to the earliest expiry as a dim `label` (`14d`, none when unknown): promo tranches plus unscoped credit. Needs `curl` and macOS `security`.

`bar-claude-credits` uses a browser session cookie and an undocumented Console endpoint (`/api/organizations/<org>/prepaid/credits`). No public API exposes the balance. The cookie can stop working at any time; the segment then shows `?` in the `bad` colour.

- Store the Console `sessionKey` cookie in a keychain entry, then name it and the organisation id in config: `plugin claude-credits org=<org-id> keychain=<entry-name>`.
- Option `account` (string) names the keychain entry too and wins over `keychain=`: `#{claude-credits[account=<entry-name>]}`, for a second Console account next to the configured one. Same character rules as `keychain=`.
- Every Console account has its own credit. The plugin follows the session's account (`inputs.account`, see [Session account](#session-account)): `org.<label>=<org-id> keychain.<label>=<entry-name>` apply when the account's label (from the user's `account` lines) is `<label>`, letters, digits, `-` and `_`. The plain `org=` / `keychain=` are the fallback for any other account, or when bar knows none. An account with no pair of its own and no fallback shows nothing, never another account's balance; so does a label with `org.<label>` but no `keychain.<label>` (unless the option `account` names the entry). All pairs go on the one `plugin claude-credits` line.
- The cookie is passed to `curl` on stdin only, never in arguments or environment.
- Requests time out after 2 s. HTTP 401 or 403 shows `?`; network errors and any other status exit non-zero without output, keeping the cached value. A missing tool, keychain entry or `org` also exits non-zero (segment omitted while nothing is cached).
- `ttl` 5 m.
- Emits `label`, `metric` (and `style: bad` for the `?`); colour it with a rule such as `rule claude-credits early pace final=3d bad=$50`.

```
plugin claude-credits org=00000000-0000-0000-0000-000000000000 keychain=claude-console-session-work
rule claude-credits early pace final=3d bad=$50
set left "#{model}#{effort} │ #{ctx} │ #{repo}:#{branch} │ #{prs[limit=3;author=@me]} │ #{tlc}"
set center "#{usage[type=5h]} │ #{usage[type=7d]} │ #{claude-credits}"
```

One balance per account, labelled by the `account` lines:

```
account "*@work.example.test" work 33
account "*@home.example.test" home 34
plugin claude-credits org.work=11111111-1111-1111-1111-111111111111 keychain.work=claude-console-work org.home=22222222-2222-2222-2222-222222222222 keychain.home=claude-console-home
```

### `bar-ci`, `bar-mem`, `bar-health`

| Placeholder | Shows | Needs | Colour | `ttl` |
|---|---|---|---|---|
| `#{ci}` | `✓` (success, neutral or skipped), `✗` (any other conclusion) or `…` (not completed) for the latest workflow run of the current branch, linked to the run | `jq`, `git`, `gh` | `score` 100, 0, 50 | 60s |
| `#{mem}` | `mem 63%` used memory | `awk`; macOS `memory_pressure` or `vm_stat`; Linux `/proc/meminfo` (`MemAvailable`) | `score` is the free share | 30s |
| `#{health}` | `✓` or `✗` for a HEAD request, linked to the URL | `jq`, `curl` | `style` good or bad | 30s |

- `ci`: option `name=1` appends the workflow name (`#{ci[name=1]}`). It runs `gh run list --branch <branch> --limit 1` in `session.cwd`. Empty text hides the segment without `gh`, outside a repo, with no git remote, on a detached HEAD or with no runs; a failed `gh` call (network, auth) exits non-zero and keeps the last value. The cache key has no branch, so after a branch switch an outage can show the previous branch's state until `gh` answers.
- `mem`: no options. An unreadable source exits non-zero without output.
- `health`: option `url` (`http` or `https`, no userinfo, no spaces or quotes) is required; option `label` is drawn dim before the mark: `#{health[url=https://example.com/up;label=api]}`. A status below 400 is up, so a redirect (3xx) counts as up and is not followed; a timeout (2 s) or error is down. The URL goes to `curl` on stdin, never in arguments. A bad or missing `url` exits non-zero with no output.
