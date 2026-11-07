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
{ "text": "[x] 7∕13", "style": "good", "link": "https://…", "score": 54,
  "metric": { "remaining": 14907, "granted": 20000, "expires_at": 1800000000, "granted_at": 1797400000 } }
```

- `text` (required) is stripped of escape and control characters, line and paragraph separators (U+2028, U+2029) and bidi overrides; wide characters (CJK, emoji) count as 2 cells. Nothing visible left omits the segment.
- `link` becomes an OSC 8 link; a URL with control characters leaves plain text.
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
