# Web configurator

A static page for building a `bar.conf`: compose each status line from placeholders and text, set their options, pick a preset, skin, palette and icons, and watch the result. Copy or download the file when done, or load an existing `bar.conf` to edit it.

## Run it

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.121 --locked   # same version web/wasm pins
make web
```

Then open `web/index.html` in a browser. It works from `file://` and from any static file server. The page loads nothing from elsewhere and sends nothing anywhere; its Content-Security-Policy allows only its own files. `make web` prints the size of the WebAssembly module (about 400 KB, 160 KB gzipped). With `wasm-opt` (binaryen) on `PATH` the module is optimised further.

## The preview is bar

The preview is drawn by bar's own renderer, compiled to WebAssembly (`web/wasm`, a crate outside bar's build), so it matches `bar render` for the same config. The page turns bar's ANSI output into HTML: 16, 256 and 24-bit colours, bold, and OSC 8 links (web links open; `bar://` click targets show their URL on hover).

What feeds it:

| Data | Source |
|------|--------|
| model, effort, context, usage windows, cost, duration, lines, version, style, vim, agent, thinking, fast mode | the session JSON under the preview (the statusLine JSON Claude Code sends on stdin); edit it to try other values |
| cwd, root, repo, branch, dirty, ahead, PRs, issues, sessions, account, tokens, cache, spend history, sid, tmux, worktree, compactions, skills, update | the samples `bar preview` uses |
| plugins (`bar-<name>`) | the sample in the plugin's [`web` metadata](plugins.md#web-configurator-metadata), painted as a real output; a plugin without one shows its name |

Times count down from the browser clock, in UTC. The preview width follows the columns slider.

## Plugins

Plugins run on your machine, not in the page, so the page works from a manifest of their `--ext-info`: names, option schemas and the optional `web` metadata (picker text, option hints, preview samples). `make web` bundles the manifest of the reference plugins in `plugins/`. For your own, run

```sh
bar web plugins -o plugins.json
```

and load the file with **Load plugins.json**; its entries replace bundled ones of the same name, and the page keeps it for the next visit. Plugins appear in the placeholder picker under their category, with their options and hints; options a plugin does not accept preview as nothing, as in `bar render`. A name in no manifest previews as itself.

## Editing

The `bar.conf` text is the state. Layout, preset and look controls rewrite only the `set` lines they own (`set left`, `set center@2`, `set skin`, ...), so comments, `default`, `rule`, `account`, `plugin` and `set price` lines stay as written. Edit the text directly for anything the controls do not cover; the controls follow.

Issues come from `bar check`, with the options of manifest plugins checked against their schema. A name that is neither a core placeholder nor in the manifest is a warning. A config with errors previews as the line `bar render` prints for one, and the layout controls wait until it parses.

The page keeps the last config in the browser's local storage.

## Font

Icons and the powerline and capsule separators need a Nerd Font. The page uses an installed Hack Nerd Font Mono, else the copy in `web/fonts/`: Hack Nerd Font Mono from Nerd Fonts v3.4.0, subset to text and the icons bar and its reference plugins draw (about 80 KB per weight; licences in `web/fonts/LICENSE`). `scripts/web-font` rebuilds it. An icon outside the subset needs the installed font.
