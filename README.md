# bar

A status line for [Claude Code](https://code.claude.com/docs/en/statusline) that any script can extend: name it `bar-<name>`, put it on `PATH`, and it becomes a placeholder, cached so the line never waits.

![bar with the powerline skin and nord palette: model, context, repo and branch, PRs with CI marks, issues, 5-hour and 7-day usage bars, account.](docs/gallery/skin-powerline.svg)

[![CI](https://github.com/hop-top/bar/actions/workflows/test-rs-caller.yml/badge.svg?branch=next)](https://github.com/hop-top/bar/actions/workflows/test-rs-caller.yml)

Written in Rust; runs on macOS, Linux and Windows.

| Tool | Warm render | Cold render | CPU, cold incl. background jobs | Memory |
|---|---|---|---|---|
| **bar** | **0.77 ms** | **1.65 ms** | 4.5 ms (1.7 without token totals) | **2.9 MiB** |
| [ccline](https://github.com/Haleclipse/CCometixLine) | 0.92 ms | 6.54 ms | 2.2 ms | 5.3 MiB |
| [claude-powerline](https://github.com/Owloops/claude-powerline) | 41.8 ms | 59.1 ms | 74.0 ms | 102 MiB |
| [ccstatusline](https://github.com/sirmalloc/ccstatusline) | 91.6 ms | 97.4 ms | 122.9 ms | 116 MiB |

Medians, Linux arm64 container, same session data and equivalent layouts; render times include about 0.6 ms of harness start-up. ccline shows no token totals, which cost bar one transcript scan per session. Method, configs and raw results: [bench/README.md](bench/README.md#ben-results).

## Why bar

- **Plugins are plain programs.** Any executable that prints one JSON object is a placeholder: shell, Python, Go, anything. bar runs it in the background, caches the result and never waits for it at render time ([Plugins](docs/plugins.md)).
- **The layout is a config file, not a UI.** A tmux-style `bar.conf` with options on every placeholder, linted by `bar check` and previewed with sample data by `bar preview`.
- **It knows your Claude usage.** 5-hour and 7-day bars coloured by pace, with projection, sparklines and a flipped scale in a window's last stretch; cost, burn rate, tokens, cache and spend history.
- **It knows your repo.** Branch and dirty state read from files (no `git` process per render), your PRs and issues from `gh` search, with a mark for failing CI.


## Install

From a release: download the archive for your platform from [Releases](https://github.com/hop-top/bar/releases), check it against its `.sha256` file and put `bar` (`bar.exe` on Windows) on your `PATH`. Archives exist for x86_64 and aarch64 on macOS, Linux (glibc 2.35 or newer; on older systems build from source) and Windows.

From source, with a [Rust toolchain](https://rustup.rs):

```sh
git clone https://github.com/hop-top/bar.git
cd bar
cargo install --path . --locked
bar --version
```

Optional: `gh` for the `prs` and `issues` placeholders, `jq` for the reference plugins in [`plugins/`](plugins).

## Use

Point Claude Code at it in `~/.claude/settings.json`:

```json
{ "statusLine": { "type": "command", "command": "bar render", "refreshInterval": 5 } }
```

If the line stays empty, Claude Code cannot find `bar` on its `PATH`; use the absolute path, such as `/Users/you/.cargo/bin/bar render`. On Windows, write the path with forward slashes: `C:/Users/you/.cargo/bin/bar.exe render`.

Without a config file bar shows its built-in layout. To change it, start from a preset and check what you write:

```sh
bar preview --preset             # every preset, with sample data
bar init --preset powerline      # write one as your config; never overwrites
bar check                        # lint the config; exit 2 on error
```

The config is plain text ([docs/config.md](docs/config.md)):

```text
set skin powerline
set palette nord
set left  "#{model}#{effort} │ #{ctx} │ #{repo}:#{branch} │ #{prs[limit=5]}"
set right "#{usage[type=5h]} │ #{usage[type=7d]}"
```

Three skins, seven palettes (one follows your terminal theme), Nerd Font or emoji icons, and up to four lines: see the [gallery](docs/gallery.md).

## Write a plugin

Save this as `bar-load` somewhere on `PATH` and make it executable:

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

Add `#{load}` to a line. `bar plugin list` shows what bar found. Plugins can declare typed options, colour by score, add links and keep per-session state; the protocol and five reference plugins (`ci`, `claude-credits`, `health`, `mem`, `tlc`) are in [docs/plugins.md](docs/plugins.md).

## Status

Pre-1.0 and actively developed: config syntax may still change between minor versions. Changes are listed with each [release](https://github.com/hop-top/bar/releases). Questions and bug reports: [open an issue](https://github.com/hop-top/bar/issues).

## Docs

| For | Read |
|-----|------|
| Configure | [Configuration](docs/config.md), [Gallery](docs/gallery.md) |
| Extend | [Plugins](docs/plugins.md) |
| Contribute | [CONTRIBUTING.md](CONTRIBUTING.md) |
| Internals, performance, releases | [Technical notes](docs/technical.md), [Benchmark](bench/README.md) |

## Contributing

Bug reports, plugins and pull requests are welcome. Features go to the `next` branch, fixes for the current release to `main` ([Branches](CONTRIBUTING.md#branches)); build, test and commit conventions are in [CONTRIBUTING.md](CONTRIBUTING.md).

## Licence

[Apache-2.0](LICENSE). The built-in model prices come from a [models.dev](https://models.dev) snapshot, MIT licensed: [data/LICENSE-models.dev](data/LICENSE-models.dev).
