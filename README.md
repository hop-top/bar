# bar

Fast, configurable and extensible status line for [Claude Code](https://code.claude.com/docs/en/statusline). Licence: [Apache-2.0](LICENSE). The built-in model price data comes from models.dev, MIT: [data/LICENSE-models.dev](data/LICENSE-models.dev).

Rendering never waits on the network (slow data comes from a background cache) and costs 3.2 to 3.7 ms of CPU per render ([measured](docs/technical.md#performance)). The layout is a tmux-style config with options on every placeholder, and any `bar-<name>` executable on `PATH` becomes a placeholder.

```text
Opus 4.7[high] │ ctx 38% │ bar:main │ #131 #129 #128 │ !57 !42              4h40m █████ │ 6d ████░              work
```

Output of `bar preview`, colour stripped. In a terminal the usage bars are one colour, red to green by pace, and repo, branch, PRs and issues are links. In a window's final stretch (the last hour of the 5-hour window, the last day of the weekly one; `--state final`) the scale flips: red then means budget that will go unused.

```text
Opus 4.7[high] │ ctx 38% │ bar:main │ #131 #129 #128 │ !57 !42             25m ███░░ │ 20h0m ████░              work
```

## Why bar

Pick bar to compose the line yourself and extend it: a tmux-style config with options on every placeholder, PR and issue segments driven by `gh` search with label/author logic, and a plugin protocol: any `bar-<name>` executable becomes a placeholder, cached so render never waits. Pick [ccstatusline](https://github.com/sirmalloc/ccstatusline) for the widest widget set and a TUI editor, [claude-powerline](https://github.com/Owloops/claude-powerline) for cost tracking and Powerline themes, [CCometixLine](https://github.com/Haleclipse/CCometixLine) for a themed Rust binary on Windows, Linux and macOS. bar has no TUI editor, and clickable commands need macOS and tmux.

What it does not do: no TUI editor, no Windows (it uses Unix APIs).

## Status

- Actively maintained. Bug reports and questions: open an issue.
- Pre-1.0: version 0.0.0, no tagged release yet; config syntax may change. Changes land as Conventional Commits, so `git log` is the changelog until the first release.
- Built and tested on macOS. On Linux (Debian bookworm, x86_64 container) it builds and the test suite passes; not run on a Linux desktop, on aarch64 or with a real Claude Code. Clickable commands need macOS and tmux: `bar click install` exits 2 elsewhere, while `bar click install --hooks-only` works on Linux. Windows is not supported.
- Not on crates.io (`publish = false`); install from a clone.

## Install

Prerequisites:

- Rust toolchain, from [rustup](https://rustup.rs).
- `git`, to clone.
- `gh`, optional: needed for the `prs` and `issues` placeholders.
- `jq`, only for the reference plugins in `plugins/`.

```sh
git clone https://github.com/hop-top/bar.git
cd bar
cargo install --path . --locked
```

Cargo puts the binary at `~/.cargo/bin/bar` (`$CARGO_HOME/bin` if set). Check with `bar --version`.

Binaries: once released. The release workflow attaches `bar-<version>-<target>.tar.gz` for x86_64 and aarch64, Linux (glibc 2.35 or newer) and macOS, to the GitHub release; no release exists yet. A Homebrew formula template is in [packaging/homebrew](packaging/homebrew/bar.rb); there is no tap yet, and the formula only works if the release assets are publicly downloadable.

## Use

Point Claude Code at it in `settings.json`:

```json
{ "statusLine": { "type": "command", "command": "bar render", "refreshInterval": 5 } }
```

Claude Code may not have `~/.cargo/bin` on its `PATH`. If the line stays empty, use the absolute path or set `PATH` in the command:

```json
{ "statusLine": { "type": "command", "command": "/Users/you/.cargo/bin/bar render", "refreshInterval": 5 } }
```

```json
{ "statusLine": { "type": "command", "command": "env PATH=$HOME/.cargo/bin:$PATH bar render", "refreshInterval": 5 } }
```

Without a config file the built-in layout above is used. Write `bar.conf` to change it ([docs/config.md](docs/config.md)), then:

```sh
bar check     # lint the config; prints the file it read; exit 2 on error
bar preview   # layout with sample data; --state early|burning|final
```

### Skins and presets

`set skin powerline` or `set skin capsule` puts each segment on a background colour; `set palette nord|gruvbox|dracula|solarized|mono|terminal` swaps the colours. Both glyph sets need a Nerd Font; `set skin-glyphs ascii` works with any font. Five presets ship in the binary:

```sh
bar preview --preset powerline   # try one with sample data
bar init --preset capsule        # write it as your config; never overwrites
```

```text
 Opus 4.7[high] > ctx 38% > bar:main >                4h40m █████ > 6d ████░ >               < work
```

The `powerline` preset without icons and with `set skin-glyphs ascii`, at `COLUMNS=104`, colour stripped. See [Skins](docs/config.md#skins), [Palettes](docs/config.md#palettes) and [Presets](docs/config.md#presets).

Plugins: `bar plugin list` shows the `bar-<name>` executables found on `PATH`; see [docs/plugins.md](docs/plugins.md). Five reference plugins live in `plugins/`: `ci`, `claude-credits`, `health`, `mem` and `tlc`.

## Docs

| Tier | For | Read |
|------|-----|------|
| User | configure, extend | [Configuration](docs/config.md), [Plugins](docs/plugins.md) |
| Contributor | build, test, send changes | [CONTRIBUTING.md](CONTRIBUTING.md) |
| Technical | design, caching, performance | [Technical notes](docs/technical.md) |

Vetting record for third-party crates: [docs/dependencies.md](docs/dependencies.md).
