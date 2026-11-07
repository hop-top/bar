# Plugins

Any executable named `bar-<name>` on `PATH` becomes the placeholder `#{name}`. The protocol follows the kit `ext-discover` contract (`--ext-info`) and extends it with a `placeholder` capability, an `options` schema, a `ttl`, and a render call with JSON on stdin.

## Discovery

- `PATH` entries are scanned in order; relative entries are ignored.
- Per name, the first executable regular file wins; non-executables are skipped.
- Names are `[A-Za-z0-9_-]+`. Core names (`account`, `repo`, `branch`, `model`, `effort`, `ctx`, `usage`, `prs`, `issues`) always win: `bar-account` is never used.
- `bar plugin list` shows every plugin found; `bar plugin info <name>` shows one plugin's interrogation result. Both accept `--format json`.

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
  "ttl": "2m"
}
```

| Field | Meaning |
|---|---|
| `name`, `version` | Required strings. Without them, or on timeout or non-zero exit, `bar` falls back to the file name, an empty version and no capabilities. |
| `capabilities` | Must include `placeholder`, or `bar` never renders the plugin. |
| `options` | Allowed options; keys, choices and string defaults are sanitised like `text`. Types: `int`, `string`, `bool`, `enum` (with `enum` choices). `default` fills a missing option when it fits the type. Options of unknown type are dropped. |
| `ttl` | How long a render result stays fresh: seconds (`300`) or a duration (`90s`, `5m`, `1h`). Default 60 s. |

The result is cached for a day per binary and modification time; renders never run `--ext-info` themselves. A cold cache renders nothing and asks in the background.

## Render

`bar-<name>` with no arguments, 3 s limit, run directly (no shell). Stdin:

```json
{
  "options": { "status": "open", "limit": 5 },
  "session": {
    "cwd": "/path/to/repo", "model": "Opus", "model_id": "claude-opus-…",
    "effort": "high", "session_id": "…", "context_used": 41.0,
    "five_hour": { "used_percentage": 12.0, "resets_at": 1800000000 },
    "seven_day": null
  },
  "columns": 160,
  "config": { "org": "…", "keychain": "entry-name" }
}
```

- `options`: inline options over `default <name>` lines, `%…%` expanded, validated against the schema and typed. An unknown key or a bad value omits the segment.
- `config`: the plugin's own `plugin <name> k=v` line, as strings, without `ttl`. Nothing else from the config or the environment goes in; secrets stay in the keychain, and config holds only entry names.
- `columns`: `COLUMNS`, or `null`.

Stdout is one JSON object:

```json
{ "label": "[x]", "text": "7∕13", "style": "good", "link": "https://…", "score": 54,
  "metric": { "remaining": 14907, "granted": 20000, "expires_at": 1800000000, "granted_at": 1797400000 } }
```

- `text` (required) is stripped of escape and control characters, line and paragraph separators (U+2028, U+2029) and bidi overrides; wide characters (CJK, emoji) count as 2 cells. Nothing visible left omits the segment.
- `label` (optional) is sanitised like `text` and drawn dim, plus a space, before `text`. It takes no colour and sits outside the link. Empty or absent: `text` alone.
- `link` becomes an OSC 8 link around `text` only, never the `label`; a URL with control characters leaves plain text.
- Colour, first match wins:
  1. `style`: `good`, `warn`, `bad` (the `color.*` palette) or `dim`. Any other value means no colour, and it still wins over `score` and `metric`.
  2. `score`: clamped to 0..100, then the `ramp`.
  3. `metric` (`remaining`, `granted`, `expires_at` required; `granted_at` optional) with a `rule <name>` whose target is exactly the placeholder name, e.g. `rule claude-credits early pace final=3d bad=$50`. Amounts are cents; times are Unix seconds. In the final stretch the score is `100 - remaining * 100 / (bad + 1)`, so unspent credit turns red before it expires. Before that, the pace score of the percent left of `granted`, with `$` thresholds read as shares of the grant; `granted_at`, when present, gives the pace period (`expires_at - granted_at`); without it only the floors apply.
  4. Otherwise plain.

## Caching and failure

- Results are cached per plugin, working directory (`session.cwd`), options and `plugin` line; other session data does not split the cache and refreshed in the background once older than `ttl`; `plugin <name> ttl=5m` overrides the plugin's value, and `set refresh` is the floor.
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

Save as `bar-hello`, `chmod +x`, put it on `PATH`, then `set left "#{hello[who=bar]}"`.

## Reference plugins

Five POSIX shell scripts ship in `plugins/`. They need `sh`, `jq` and the tool they wrap; put them on `PATH` (copy or symlink) to enable `#{tlc}`, `#{claude-credits}`, `#{ci}`, `#{mem}` and `#{health}`. Colour comes from core: `bar-tlc` emits `score`, `bar-claude-credits` emits `metric`. The only `style` either sets is `bad` on the credits `?`.

### `bar-tlc`

`#{tlc}` shows `closed∕total` tasks (`DONE` and `SKIPPED` count as closed) of the track of your latest-updated claimed task, else of all tasks. Needs `tlc`. It runs `tlc` from the nearest ancestor of `session.cwd` holding `.tlc/config.yaml` (a stray empty `.tlc/` is skipped; a relative `cwd` falls back to the working directory) and never creates one. If any `tlc` call fails, or there is no project root, it exits non-zero with no output, so the last good value stays. Emits `score` (closed share, 0..100), so none closed is red and all closed is green.

| Option | Default | Meaning |
|---|---|---|
| `scope` | `track` | `track`: the track of your latest-updated claimed task, else all tasks. `all`: the whole project. |
| `status` | none | Passed to `tlc task list --status` to filter the counted set; letters, digits, `_`, `,` and `-` only (not first), anything else makes the plugin fail (exit 1, no output): a warm cache keeps the last value, only a cold cache renders empty. |

`#{tlc[scope=all]}` counts the project; `#{tlc[status=DONE]}` counts within the tasks `tlc` lists for that status.

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
- Network errors and 5xx exit non-zero without output, keeping the cached value. A missing keychain entry or `org` omits the segment.
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
| `#{ci}` | `✓`, `✗` or `…` for the latest workflow run of the current branch, linked to the run | `git`, `gh` | `score` 100, 0, 50 | 60s |
| `#{mem}` | `mem 63%` used memory | macOS `memory_pressure` or `vm_stat`; Linux `/proc/meminfo` | `score` is the free share | 30s |
| `#{health}` | `✓` or `✗` for a HEAD request, linked to the URL | `curl` | `style` good or bad | 30s |

- `ci`: option `name=1` appends the workflow name (`#{ci[name=1]}`). It runs `gh run list --branch <branch> --limit 1` in `session.cwd`. Empty text hides the segment without `gh`, outside a repo, on a detached HEAD or with no runs; a failed `gh` call exits non-zero and keeps the last value.
- `mem`: no options. An unreadable source exits non-zero without output.
- `health`: option `url` (`http` or `https`, no userinfo, no spaces or quotes) is required; option `label` is drawn dim before the mark: `#{health[url=https://example.com/up,label=api]}`. A status below 400 is up; a timeout (2 s) or error is down. The URL goes to `curl` on stdin, never in arguments. A bad or missing `url` exits non-zero with no output.
