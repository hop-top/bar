# bar

A status line for [Claude Code](https://code.claude.com/docs/en/statusline) that any script can extend: name it `bar-<name>`, put it on `PATH`, and it becomes a placeholder, cached so the line never waits.

![bar with the powerline skin and nord palette: model, context, repo and branch, PRs with CI marks, issues, 5-hour and 7-day usage bars, account.](docs/gallery/skin-powerline.svg)

[![CI](https://github.com/hop-top/bar/actions/workflows/test-rs-caller.yml/badge.svg?branch=next)](https://github.com/hop-top/bar/actions/workflows/test-rs-caller.yml)
![macOS](https://img.shields.io/badge/macOS-000000?logo=apple&logoColor=white)
![Linux](https://img.shields.io/badge/Linux-FCC624?logo=linux&logoColor=black)
![Windows](https://img.shields.io/badge/Windows-0078D4?logo=data:image/svg%2bxml;base64,PHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIHZpZXdCb3g9IjAgMCAyNCAyNCI+PHBhdGggZmlsbD0id2hpdGUiIGQ9Ik0wIDMuNCA5LjggMnY5LjRIMHptMTEgLTEuNkwyNCAwdjExLjNIMTF6TTAgMTIuNmg5LjhWMjJMMCAyMC42em0xMSAwaDEzVjI0bC0xMy0xLjh6Ii8+PC9zdmc+)

## Why bar

- **Plugins are plain programs.** Any executable that prints one JSON object is a placeholder. bar runs it in the background and caches the result ([Plugins](docs/plugins.md)).
- **The layout is a config file.** tmux-style, options on every placeholder, linted by `bar check`, previewed by `bar preview`.
- **It knows your Claude usage.** 5-hour and 7-day bars coloured by pace, projections, sparklines; cost, burn rate, tokens, spend history.
- **It knows your repo.** Branch and dirty state from files, PRs and issues from `gh`, failing CI marked.

### With token counters

| Tool | Warm | Cold | Cold CPU, incl. background | Memory |
|---|---|---|---|---|
| **bar** | **0.77 ms** | **1.65 ms** | **4.5 ms** | **2.9 MiB** |
| [claude-powerline](https://github.com/Owloops/claude-powerline) | 41.8 ms | 59.1 ms | 74.0 ms | 102 MiB |
| [ccstatusline](https://github.com/sirmalloc/ccstatusline) | 91.6 ms | 97.4 ms | 122.9 ms | 116 MiB |

### No token counters

| Tool | Warm | Cold | Cold CPU, incl. background | Memory |
|---|---|---|---|---|
| **bar** | **0.75 ms** | **0.95 ms** | **1.7 ms** | **2.8 MiB** |
| [ccline](https://github.com/Haleclipse/CCometixLine) | 0.92 ms | 6.54 ms | 2.2 ms | 5.3 MiB |

Medians, same session data and equivalent layouts ([method and raw results](bench/README.md#ben-results)).

## Install

Download an archive from [Releases](https://github.com/hop-top/bar/releases) and put `bar` on your `PATH`, or build from source with a [Rust toolchain](https://rustup.rs):

```sh
git clone https://github.com/hop-top/bar.git
cd bar
cargo install --path . --locked
```

Optional: `gh` for PRs and issues, `jq` for the reference plugins.

## Use

In `~/.claude/settings.json`:

```json
{ "statusLine": { "type": "command", "command": "bar render", "refreshInterval": 5 } }
```

If the line stays empty, use the absolute path to `bar` (on Windows with forward slashes).

```sh
bar preview --preset          # every preset, with sample data
bar init --preset powerline   # write one as your config
bar check                     # lint the config
```

```text
set skin powerline
set palette nord
set left  "#{model}#{effort} │ #{ctx} │ #{repo}:#{branch} │ #{prs[limit=5]}"
set right "#{usage[type=5h]} │ #{usage[type=7d]}"
```

Skins, palettes, icons and multi-line layouts: [configuration](docs/config.md), [gallery](docs/gallery.md).

## Write a plugin

```sh
#!/bin/sh
# bar-load: 1-minute load average as #{load}
if [ "$1" = "--ext-info" ]; then
  echo '{"name":"load","version":"0.1.0","capabilities":["discover","placeholder"],"ttl":"30s"}'
  exit 0
fi
load=$(uptime | sed 's/.*load averages*: *//; s/[, ].*//')
printf '{"label":"load","text":"%s"}\n' "$load"
```

Save it as `bar-load` on `PATH`, make it executable, add `#{load}` to a line. Protocol and reference plugins: [docs/plugins.md](docs/plugins.md).

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).

## Licence

[Apache-2.0](LICENSE)
