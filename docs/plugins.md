# Plugins

Any executable named `bar-<name>` on `PATH` (on Windows `bar-<name>.exe`, `.cmd`, `.bat` or `.com`) becomes the placeholder `#{name}`. The protocol follows the kit `ext-discover` contract (`--ext-info`) and extends it with a `placeholder` capability, an `options` schema, a `ttl`, and a render call with JSON on stdin.

## Discovery

- `PATH` entries are scanned in order; relative entries are ignored. After them comes the directory holding the running `bar` (symlinks resolved), so plugins shipped beside `bar` are found without being on `PATH`. A copy on `PATH` wins over one beside `bar`.
- Per name, the first executable regular file wins; non-executables are skipped.
- Windows: a file is executable when its extension is one of `.com`, `.exe`, `.bat`, `.cmd` listed in `PATHEXT` (default `.COM;.EXE;.BAT;.CMD`); the name is the file name without `bar-` and the extension, case ignored in the extension. In one dir the extension listed first in `PATHEXT` wins. Extension-less files (shebang scripts) and `.ps1` are skipped, since Windows cannot start them directly; wrap a PowerShell script in a `.cmd`, e.g. `bar-hello.cmd` holding `@powershell -NoProfile -File "%~dp0bar-hello.ps1" %*`. `.bat`/`.cmd` plugins run through `cmd.exe`, so `echo` output ends in CRLF; bar trims it.
- Names are `[A-Za-z0-9_-]+`. Core placeholder names (every name in [config](config.md#placeholders): `account`, `repo`, `model`, `usage`, `tokens`, `cwd` and the rest) always win: `bar-account` is never used.
- `bar plugin list` shows every plugin found (name, version, description, path); `bar plugin info <name>` runs `--ext-info` now and shows the parsed result plus the path, or exits 1 when no such plugin is on `PATH`. Both accept `--format json` (also `yaml`, `csv`, `text`).

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
| `capabilities` | Must include `placeholder`, or `bar` never renders the plugin. |
| `description` | Optional string, shown by `bar plugin list`. |
| `options` | Allowed options; keys, choices and string defaults are sanitised like `text`. Types: `int` (sent as a JSON integer), `string`, `bool` (written `true`/`yes`/`on`/`1` or `false`/`no`/`off`/`0`, sent as JSON `true`/`false`), `enum` (with `enum` choices, sent as the string). `default` fills a missing option when it fits the type. Options of unknown type are dropped. `icon`, `bg`, `fg`, `bold`, `gradient` and `click` are reserved: declared, they are dropped and `bar check` warns. |
| `icon` | Optional default segment icon, used when `set icons` is not `none`. Sanitised; at most 2 cells. |
| `inputs` | Optional. What the output depends on besides options: `{"env": ["ANTHROPIC_BASE_URL"], "session": ["model_id"]}`. Each declared value is part of the cache key, and the declared environment variables are sent as `env` in the render input. `env`: up to 8 names of `[A-Z0-9_]`, at most 64 characters; a name containing `KEY`, `TOKEN`, `SECRET`, `PASSWORD`, `PASSWD`, `CREDENTIAL` or `AUTH` is refused, since values go into the cache key and the refresh process's arguments (a plugin that needs a secret reads its own environment, which it inherits); values longer than 1 KiB count as unset. `session`: any of `cwd`, `model`, `model_id`, `effort`, `session_id`; the volatile fields (`context_used`, `five_hour`, `seven_day`) cannot be declared, they would split the cache on every render. |
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
| `samples` | Preview outputs, in order. Each has `output`, an object in the [render output](#render) shape (`text` required; `label`, `icon`, `style`, `score`, `metric`, `link`, `alt`, `label_toggle`), and optionally `when`, an object of option values. The preview uses the first sample whose `when` values all equal the placeholder's options, after schema defaults are filled in; a sample without `when` matches any options. Values are written as in a layout (`"true"`, `"3"`) or as JSON of the option's type. The output goes through the same paint code as a real render: style and score colours, label, icons, skins. With no match, or no samples, the preview shows the plugin name. A toggle is never on in the preview, so `text` shows, not `alt`. |
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
- `show`, `alt` and `checks` are list options for every placeholder, plugins included: a bare part after one continues its comma list, so `#{name[show=a,b]}` sends `show` as `a,b`, and `#{name[show=a,verbose]}` sends `show` as `a,verbose` rather than a separate `verbose` flag. Avoid those names for other kinds of option, or expect a comma list; put flags before them.
- `config`: the plugin's own `plugin <name> k=v` line, as strings, without `ttl`. Nothing else from the config or the environment goes in; secrets stay in the keychain, and config holds only entry names.
- `env`: only for plugins declaring `inputs.env`: the declared variables that are set, as strings; unset ones are absent. The key is absent without a declaration.
- `columns`: `COLUMNS`, or `null`.
- `toggles`: the plugin's named toggles that are on, from `bar toggle <name>.<toggle>` (see `label_toggle` below); an off toggle is absent, and only this plugin's keys are sent. They are part of the cache key, so a flip is a new entry: it shows at the next render once the plugin has run for it (the first render after a flip is empty until then, a flip back is immediate while the old entry is cached).
- `session.record`: only for plugins declaring `"cache": "session"`, and only when the state directory holds `records/<name>/<session id>` (a JSON object, at most 4 KiB), written by `bar click hook`. Today the hook writes one for `tlc` only (see [Hooks](config.md#hooks)). Absent otherwise.

Stdout is one JSON object:

```json
{ "label": "[x]", "label_toggle": "scope", "text": "7∕13", "alt": "track-a", "style": "good", "link": "https://…", "score": 54,
  "metric": { "remaining": 14907, "granted": 20000, "expires_at": 1800000000, "granted_at": 1797400000 } }
```

- `items` (optional, an array of at most 16 objects) replaces the output's own `text`: each item is an output of its own (`label`, `icon`, `text`, `alt`, `style`, `score`, `metric`, `link`, `label_toggle`) drawn like one, items joined by one space. With `set wrap on`, a line too wide for `COLUMNS` may break between items, never inside one; the next row starts at the left like any wrapped row. Without wrapping the items stay on one row. An item without a string `text`, or with nothing visible, is left out; items do not nest. With `items`, the top-level `text` may be left out (it is not drawn).
- `usage` (optional) carries usage windows for `#{usage}`: see [Usage windows](#usage-windows).
- `text` (required, a string, unless `items` is given) is stripped of escape and control characters (tabs and line breaks become spaces), line and paragraph separators (U+2028, U+2029) and Unicode format characters (bidi marks and overrides, zero-width spaces, soft hyphens, BOM; the joiners U+200C and U+200D and emoji tag characters are kept); wide characters (CJK, emoji) count as 2 cells. Nothing visible left omits the segment.
- `alt` (optional) is shown instead of `text` while the plugin's toggle is on: `bar toggle <name>` flips `toggle-<name>` in the state directory, and a click on the text opens `bar://toggle/<name>` once the handler is installed (`bar click install`), unless the output has its own `link` or the placeholder sets `click=none`. Sanitised like `text`; colour, label and icon stay the same. Without `alt` there is no toggle. Both texts come from one render, so a toggle shows at once, without a refresh.
- `click` is a reserved option name: `toggle` (default) or `none`, never sent to the plugin; a `click` entry in `options` is ignored, and `bar check` warns about it.
- `label` (optional) is sanitised like `text` and drawn dim, plus a space, before `text`. It takes no colour and sits outside the link. Empty or absent: `text` alone.
- `icon` (optional) overrides the `--ext-info` icon for this render. When an icon shows (`set icons` not `none`, or `icon=<glyph>` on the placeholder; `icon=none` hides it), it replaces `label`: dim, a space, then `text`, outside the link. An icon wider than 2 cells is dropped. `icon` is a reserved option name: it is never sent to the plugin, an `icon` entry in `options` is ignored, and `bar check` warns about it. An invalid output icon falls back to the `--ext-info` icon, then to `label`.
- `label_toggle` (optional) names a toggle for the drawn label or icon: a key like a plugin name (letters, digits, `-`, `_`, at most 64; otherwise ignored). When the click handler is installed (`bar click install`) and `click` is not `none`, the label or icon (not `text`) is wrapped in an OSC 8 link `bar://toggle/<plugin>.<toggle>`, e.g. `tlc.scope`; `bar toggle tlc.scope` flips the state file `toggle-tlc.scope` and the flag reaches the plugin in `toggles`. The text toggle (`alt`, `bar://toggle/<plugin>`) is unchanged.
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

### `bar-tlc`

`#{tlc}` shows `closed∕total` tasks of this session's track, else of the whole project; archived tasks included, and every status but `TODO` and `IN_PROGRESS` (so `DONE`, `SKIPPED`) counts as closed. Needs `tlc`; `ttl` 60 s. It runs `tlc` from the nearest ancestor of `session.cwd` holding `.tlc/config.yaml` (a stray empty `.tlc/` is skipped; a relative `cwd` falls back to the working directory) and never creates one. The search stops at the git work-tree root (the first directory holding `.git`), so a project enclosing the repository is never counted; a linked worktree also checks its repository root (the bare repo directory, else the main worktree); outside a repository only `cwd` itself is checked. With no project in scope, or a project with no tasks, it emits empty text and the segment hides. If any `tlc` call fails, it exits non-zero with no output, so the last good value stays. Emits `score` (closed share, 0..100), so none closed is red and all closed is green.

The session's track comes from `session.record`, which `bar click hook` keeps per session (see [Hooks](config.md#hooks); register them with `bar click install` or `bar click install --hooks-only`): the task the session last claimed, set `IN_PROGRESS`, completed or created. When the record's `cwd` resolves to the same project as `session.cwd`, the plugin reads the task's `track_id` (`tlc task show <id>`) and counts that track; it keeps doing so after the task is done, until the session touches another task. No record, a record for another project, a task without track, or a task that no longer exists (`tlc task show` exit 3) counts the whole project. Declares `"cache": "session"`, so two sessions in one directory each keep their own value.

`text` and `alt` are the count (`71∕91`) and the scope name: the track slug (`tlc track show <id>`), or `all` for the whole project. The default shows the count; `bar toggle tlc` (or a click, with the handler installed) shows the scope name, and the count is hidden meanwhile.

| Option | Default | Meaning |
|---|---|---|
| `scope` | `track` | `track`: the session's track, else the whole project. `all`: the whole project. |
| `status` | none | Passed to `tlc task list --status` to filter the counted set; letters, digits, `_`, `,` and `-` only (not first), anything else makes the plugin fail (exit 1, no output): a warm cache keeps the last value, only a cold cache renders empty. |
| `show` | `count` | `count` or `track`: what `text` holds. |
| `alt` | the other one | `count` or `track`: what `alt` holds, shown while toggled. |
| `label-track` | `◉` | Mark before the count while the track is counted (icons off). |
| `label-all` | `☰` | Mark while the whole project is counted. |
| `icon-track` | U+F45D (nf-oct-milestone) | Mark with `set icons` not `none`. |
| `icon-all` | U+F45E (nf-oct-checklist) | Same, whole project. |
| `click` | `toggle` | `none` drops the toggle and mark links (core option). |

A mark precedes the count and shows what it covers: `◉` (U+25C9) for the session's track, `☰` (U+2630) for the whole project, both one cell wide (East Asian Width neutral, present in common monospace fonts); the label is drawn dim before `text`. With `set icons` not `none` the Nerd Font glyphs nf-oct-milestone (U+F45D, track) and nf-oct-checklist (U+F45E, whole project) replace the label. The plugin cannot tell which icon set is active: an output icon shows whenever icons are on, so `set icons emoji` also draws the Nerd Font glyphs, which need a Nerd Font. With emoji, set `icon-track` / `icon-all` (for example `#{tlc[icon-track=🎯,icon-all=📋]}`; at most 2 cells), or `icon=none` on the placeholder to fall back to the labels; `icon=<glyph>` on the placeholder beats both icons and so drops the distinction. An empty `label-*` or `icon-*` draws no mark for that scope. The mark follows the scope actually counted: `scope=track` without a session track shows the project mark.

The mark is a toggle (`label_toggle: "scope"`, link `bar://toggle/tlc.scope`, with the click handler installed): `bar toggle tlc.scope` inverts the scope, track to project and project to track. Without a session track the project stays counted, whatever the toggle. The flag arrives as `toggles.scope`; the text toggle (count and track name) is independent. After a flip the new count shows at the next render once the plugin has run for it, not at ttl.

`#{tlc[scope=all]}` counts the project; `#{tlc[status=DONE]}` counts within the tasks `tlc` lists for that status; `#{tlc[show=track]}` shows the slug by default and the count when toggled.

```
set left "#{model}#{effort} │ #{ctx} │ #{repo}:#{branch} │ #{prs[limit=3,author=@me]} │ #{tlc}"
```

That is the built-in `left` with `#{issues[...]}` replaced by `#{tlc}`. To keep issues and add tasks:

```
set left "#{model}#{effort} │ #{ctx} │ #{repo}:#{branch} │ #{prs[limit=3,author=@me]} │ #{issues[limit=3,assignee=@me]} │ #{tlc}"
```

### `bar-claude-credits`

`#{claude-credits}` shows the prepaid Console balance, e.g. `$149.07`, with the days to the earliest expiry as a dim `label` (`14d`, none when unknown): promo tranches plus unscoped credit. Needs `curl` and macOS `security`.

`bar-claude-credits` uses a browser session cookie and an undocumented Console endpoint (`/api/organizations/<org>/prepaid/credits`). No public API exposes the balance. The cookie can stop working at any time; the segment then shows `?` in the `bad` colour.

- Store the Console `sessionKey` cookie in a keychain entry, then name it and the organisation id in config: `plugin claude-credits org=<org-id> keychain=<entry-name>`.
- Option `account` (string) names the keychain entry too and wins over `keychain=`: `#{claude-credits[account=<entry-name>]}`, for a second Console account next to the configured one. Same character rules as `keychain=`.
- The cookie is passed to `curl` on stdin only, never in arguments or environment.
- Requests time out after 2 s. HTTP 401 or 403 shows `?`; network errors and any other status exit non-zero without output, keeping the cached value. A missing tool, keychain entry or `org` also exits non-zero (segment omitted while nothing is cached).
- `ttl` 5 m.
- Emits `label`, `metric` (and `style: bad` for the `?`); colour it with a rule such as `rule claude-credits early pace final=3d bad=$50`.

```
plugin claude-credits org=00000000-0000-0000-0000-000000000000 keychain=claude-console-session-work
rule claude-credits early pace final=3d bad=$50
set left "#{model}#{effort} │ #{ctx} │ #{repo}:#{branch} │ #{prs[limit=3,author=@me]} │ #{tlc}"
set center "#{usage[type=5h]} │ #{usage[type=7d]} │ #{claude-credits}"
```

### `bar-ci`, `bar-mem`, `bar-health`

| Placeholder | Shows | Needs | Colour | `ttl` |
|---|---|---|---|---|
| `#{ci}` | `✓` (success, neutral or skipped), `✗` (any other conclusion) or `…` (not completed) for the latest workflow run of the current branch, linked to the run | `jq`, `git`, `gh` | `score` 100, 0, 50 | 60s |
| `#{mem}` | `mem 63%` used memory | `awk`; macOS `memory_pressure` or `vm_stat`; Linux `/proc/meminfo` (`MemAvailable`) | `score` is the free share | 30s |
| `#{health}` | `✓` or `✗` for a HEAD request, linked to the URL | `jq`, `curl` | `style` good or bad | 30s |

- `ci`: option `name=1` appends the workflow name (`#{ci[name=1]}`). It runs `gh run list --branch <branch> --limit 1` in `session.cwd`. Empty text hides the segment without `gh`, outside a repo, with no git remote, on a detached HEAD or with no runs; a failed `gh` call (network, auth) exits non-zero and keeps the last value. The cache key has no branch, so after a branch switch an outage can show the previous branch's state until `gh` answers.
- `mem`: no options. An unreadable source exits non-zero without output.
- `health`: option `url` (`http` or `https`, no userinfo, no spaces or quotes) is required; option `label` is drawn dim before the mark: `#{health[url=https://example.com/up,label=api]}`. A status below 400 is up, so a redirect (3xx) counts as up and is not followed; a timeout (2 s) or error is down. The URL goes to `curl` on stdin, never in arguments. A bad or missing `url` exits non-zero with no output.
