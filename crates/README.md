# crates

Workspace libraries and apps besides `bar` itself (the root crate `hop-bar`, in [`src/`](../src/)).

- [`bar-plugin/`](bar-plugin/): the Rust plugin library: protocol types and the helpers the compiled plugins share.
- [`bar-tray/`](bar-tray/): `bar-tray`, the menu bar / tray app ([docs/tray.md](../docs/tray.md)).

The compiled plugins are workspace members too, under [`plugins/`](../plugins/). The web configurator's wasm crate, [`web/wasm/`](../web/wasm/), is outside the workspace.

Every crate is `publish = false`. `make check` builds, lints and tests them all ([CONTRIBUTING.md](../CONTRIBUTING.md)).
