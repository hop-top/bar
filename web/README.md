# web

The web configurator: a static page for composing a `bar.conf` with a live preview drawn by bar's own renderer, compiled to WebAssembly. What it does and how to use it: [docs/web.md](../docs/web.md).

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.121 --locked   # the version web/wasm pins
make web                                                    # builds web/pkg/; then open web/index.html
```

- [`index.html`](index.html), [`style.css`](style.css): the page.
- [`app.js`](app.js): page wiring; the `bar.conf` text is the state, every change re-renders the preview.
- [`compose.js`](compose.js), [`picker.js`](picker.js), [`conf.js`](conf.js), [`history.js`](history.js): the line composer, the placeholder picker, edits to the config text, undo and redo.
- [`ansi.js`](ansi.js): ANSI output to HTML for the preview.
- [`wasm/`](wasm/): the `bar-web` crate, wasm-bindgen exports over the root crate's `web` module. Outside the Cargo workspace, with its own `Cargo.lock`.
- [`fonts/`](fonts/): a Hack Nerd Font Mono subset, used when the font is not installed; rebuilt by [`scripts/web-font`](../scripts/web-font).
- `pkg/` (not committed): the build output of [`scripts/web`](../scripts/web): the wasm module, its glue and `plugins.js`, the manifest of the shell plugins in [`plugins/`](../plugins/).

`make web-check` (part of `make check`) builds and lints `web/wasm` when the toolchain has the wasm32 target, and skips otherwise.
