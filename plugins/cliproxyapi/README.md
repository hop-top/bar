# bar-cliproxyapi

The `#{cliproxyapi}` plugin: the 5-hour and 7-day quota of each account behind a [CLIProxyAPI](https://github.com/router-for-me/CLIProxyAPI) proxy, with reset countdowns, feeding `#{usage}` and `#{account}`; `bar cpa list|use|use --next|pick` switches accounts. Setup, options and commands: [docs/plugins/cliproxyapi.md](../../docs/plugins/cliproxyapi.md).

A render never calls the proxy: it reads the plugin's cache and starts a background collection when that is stale. How it talks to the proxy's management API, and why it never risks a ban: [technical notes](../../docs/technical.md#platforms).

## Layout

- [`src/main.rs`](src/main.rs): the binary: `--ext-info`, `--check`, `--menu`, `--action`, a render, and the `list`, `use` and `pick` commands. Everything else is the library in [`src/lib.rs`](src/lib.rs).
- [`src/config.rs`](src/config.rs): the plugin's own `cliproxyapi.conf` (instances and key sources).
- [`src/collect.rs`](src/collect.rs), [`src/mgmt.rs`](src/mgmt.rs), [`src/cache.rs`](src/cache.rs): one collection run, the management calls, the cache file.
- [`src/model.rs`](src/model.rs), [`src/render.rs`](src/render.rs), [`src/pool.rs`](src/pool.rs): accounts and windows, the render output and `--ext-info`, the pooled `usage` and `account`.
- [`src/switch.rs`](src/switch.rs), [`src/rank.rs`](src/rank.rs), [`src/chooser.rs`](src/chooser.rs), [`src/pick_label.rs`](src/pick_label.rs): switching, which account to switch to, the macOS chooser and its labels.
- [`src/menu.rs`](src/menu.rs): the [tray](../../docs/tray.md) menu, from the cache only.

## Build and test

```sh
cargo install --path plugins/cliproxyapi --locked
cargo test -p bar-cliproxyapi --features test-tools
```

The tests run against an in-process fake management server ([`tests/fake.rs`](tests/fake.rs)) with synthetic accounts, on loopback only; `core_compat.rs` and `action_e2e.rs` drive the binary through bar's own plugin code. The `test-tools` feature lets a test stand in for the macOS chooser; a release build refuses it.
