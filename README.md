# bar

A status line for [Claude Code](https://code.claude.com/docs/en/statusline) that any script can extend: name it `bar-<name>`, put it on `PATH`, and it becomes a placeholder, cached so the line never waits.

![bar with the powerline skin and nord palette: model, context, repo and branch, PRs with CI marks, issues, 5-hour and 7-day usage bars, account.](docs/gallery/skin-powerline.svg)

<p align="center">
  <a href="https://github.com/hop-top/bar/actions/workflows/test-rs-caller.yml"><img alt="CI" src="https://github.com/hop-top/bar/actions/workflows/test-rs-caller.yml/badge.svg?branch=next"></a>
  <a href="bench/README.md#ben-results"><img alt="bench" src="docs/badges/bench.svg"></a>
</p>
<p align="center">
  <img alt="macOS" src="docs/badges/macos.svg"> <img alt="Linux" src="docs/badges/linux.svg"> <img alt="Windows" src="docs/badges/windows.svg">
</p>

## Why bar

- **Plugins are plain programs.** Any executable that prints one JSON object is a placeholder. bar runs it in the background and caches the result ([Plugins](docs/plugins.md)).
- **The layout is a config file.** tmux-style, options on every placeholder, linted by `bar check`, previewed by `bar preview`.
- **It knows your Claude usage.** 5-hour and 7-day bars coloured by pace, projections, sparklines; cost, burn rate, tokens, spend history.
- **It knows your repo.** Branch and dirty state from files, PRs and issues from `gh`, failing CI marked.

### Benchmarks

<!-- bench:readme:start -->
With token counters:

| Tool | Warm | Cold | Cold CPU, incl. background | Memory |
|---|---|---|---|---|
| **bar** | **0.77 ms** | **1.65 ms** | **4.5 ms** | **2.8 MiB** |
| [claude-powerline](https://github.com/Owloops/claude-powerline) | 42.1 ms | 57.5 ms | 73.2 ms | 102 MiB |
| [ccstatusline](https://github.com/sirmalloc/ccstatusline) | 92.4 ms | 97.9 ms | 123.4 ms | 116 MiB |

No token counters:

| Tool | Warm | Cold | Cold CPU, incl. background | Memory |
|---|---|---|---|---|
| **bar** | **0.75 ms** | **0.93 ms** | **1.6 ms** | **2.8 MiB** |
| [ccline](https://github.com/Haleclipse/CCometixLine) | 0.92 ms | 6.95 ms | 2.1 ms | 4.9 MiB |
<!-- bench:readme:end -->

Medians, same session data and equivalent layouts ([method and raw results](bench/README.md#ben-results)).

Full feature comparison: [docs/comparison.md](docs/comparison.md).

## Install

Download an archive from [Releases](https://github.com/hop-top/bar/releases) and put `bar` on your `PATH`, or build from source with a [Rust toolchain](https://rustup.rs):

```sh
git clone https://github.com/hop-top/bar.git
cd bar
cargo install --path . --locked
```

Guided setup inside Claude Code: `/plugin marketplace add hop-top/bar`, then `/plugin install bar@hop-top` and `/bar` ([docs](docs/claude-plugin.md)).

Optional: `gh` for PRs and issues, `jq` for the reference plugins.

## Use

```sh
bar install --refresh 5       # set bar as the status line in ~/.claude/settings.json (shows the diff, asks first)
```

Or by hand, in `~/.claude/settings.json`:

```json
{ "statusLine": { "type": "command", "command": "bar render", "refreshInterval": 5 } }
```

If the line stays empty, use the absolute path to `bar` (on Windows with forward slashes).

```sh
bar preview --preset          # every preset, with sample data
bar init --preset powerline   # write one as your config
bar edit                      # edit the config with a live preview
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
