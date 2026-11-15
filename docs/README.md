# Docs

Every page in `docs/`, by reader. Install and a first config: the [README](../README.md).

## Using bar

- [Configuration](config.md): `bar.conf`, its directives, every placeholder and option, styling, layout, and the commands around the config.
- [Gallery](gallery.md): what skins, palettes, bars, icons and lines look like, drawn from real `bar` output.
- [Plugins](plugins.md): how bar finds and runs plugins, and the shell plugins that come with it.
- [bar-cliproxyapi](plugins/cliproxyapi.md): per-account quota of a CLIProxyAPI proxy, and account switching.
- [bar-routellm](plugins/routellm.md): routing state and on/off switch of a RouteLLM router behind that proxy.
- [Tray](tray.md): `bar-tray`, plugin actions and session counts in the menu bar or system tray.
- [Waybar](waybar.md): the tray's state as a waybar module.
- [Web configurator](web.md): compose a `bar.conf` in the browser with a live preview.
- [Claude Code plugin](claude-plugin.md): guided setup from inside Claude Code.
- [Feature comparison](comparison.md): bar next to the status lines its benchmark measures.

## Writing plugins

- [Plugins](plugins.md): the protocol (`--ext-info`, render input and output, caching, actions, menus).
- [bar-plugin](../crates/bar-plugin/): the Rust library the compiled plugins use, with an example.
- [`plugins/`](../plugins/): the bundled plugins, as working examples.

## Contributing

- [CONTRIBUTING.md](../CONTRIBUTING.md): setup, checks, quality gates, branches, conformance.
- [Technical notes](technical.md): render flow, cache, performance, platforms, releasing.
- [Dependencies](dependencies.md): the vetting record of every dependency and pinned tool.
- [Benchmark](../bench/README.md): method and results of the status-line benchmark.
- [`tests/`](../tests/), [`scripts/`](../scripts/), [`e2e/`](../e2e/): test layout, helper scripts, conformance stories.

`gallery/` holds the images of the gallery and the README (`make gallery`); `badges/` the README's badges, `bench.svg` drawn by `make bench-badge`.
