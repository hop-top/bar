# bar-routellm

The `#{routellm}` plugin: whether a [RouteLLM](https://github.com/lm-sys/RouteLLM) router behind a [CLIProxyAPI](https://github.com/router-for-me/CLIProxyAPI) proxy is routing, with the session's alias and tier, and a switch to turn it on or off from a click or `bar rtellm on|off`. Setup, options and commands: [docs/plugins/routellm.md](../../docs/plugins/routellm.md).

A render reads the plugin's cache and collects only when that is stale; a switch happens only on an explicit action or command, never in a render. Request flow and ban safety, shared with [`bar-cliproxyapi`](../cliproxyapi/): [technical notes](../../docs/technical.md#platforms).

## Layout

- [`src/main.rs`](src/main.rs): the binary: `--ext-info`, `--check`, `--menu`, `--action`, a render, and the `status`, `on`, `off`, `tiers` and `setup` commands. Everything else is the library in [`src/lib.rs`](src/lib.rs).
- [`src/config.rs`](src/config.rs): the plugin's own `routellm.conf`.
- [`src/collect.rs`](src/collect.rs), [`src/mgmt.rs`](src/mgmt.rs), [`src/cache.rs`](src/cache.rs): one collection run, the proxy calls, the cache file.
- [`src/toggle.rs`](src/toggle.rs): the on/off change of the proxy's provider entry.
- [`src/render/`](src/render/): the segment, `--ext-info`, the `status` and `tiers` listings and the `setup` block.
- [`src/menu.rs`](src/menu.rs): the [tray](../../docs/tray.md) menu, from the cache only.

## Build and test

```sh
cargo install --path plugins/routellm --locked
cargo test -p bar-routellm
```

The tests run against an in-process fake proxy and RouteLLM ([`tests/fake.rs`](tests/fake.rs)) with synthetic data, on loopback only; `core_compat.rs` drives the binary through bar's own plugin code.
