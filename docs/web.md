# Web configurator

A static page for building a `bar.conf`: compose each status line from placeholders and text, set their options, pick a preset, skin, palette and icons, and watch the result. Copy or download the file when done, or load an existing `bar.conf` to edit it.

It opens on the `powerline` preset. The presets at the top show their own preview; a click applies one (Undo in the notice takes it back), and the header of that list names the preset in use, or `custom` once you edit. After **Copy** or **Download**, a **Use it** panel lists the steps: install bar, save the file as `~/.config/bar/bar.conf`, run `bar install` (or add the `statusLine` entry to `~/.claude/settings.json`), and `bar check`.

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
| cwd, root, repo, branch, origin, upstream, fork, dirty, ahead, diff, sha, tag, stash, gitop, conflicts, jj, PRs, issues, sessions, account, tokens, cache, spend history, budgets, extra usage, sid, tmux, worktree, compactions, skills, update, status | the samples `bar preview` uses |
| plugins (`bar-<name>`) | the sample in the plugin's [`web` metadata](plugins.md#web-configurator-metadata), painted as a real output; a plugin without one shows its name |

Times count down from the browser clock, in UTC. The preview width follows the columns slider, or the width of the page with **fit to width** (on by default on small screens, where the preview also stays at the top while you scroll).

## Plugins

Plugins run on your machine, not in the page, so the page works from a manifest of their `--ext-info`: names, option schemas and the optional `web` metadata (picker text, option hints, preview samples). `make web` bundles the manifest of the reference plugins in `plugins/`. For your own, run

```sh
bar web plugins -o plugins.json
```

(add `--force` to replace an existing file) and load it with **Load plugins.json** (in the Plugins section); its entries replace bundled ones of the same name, and the page keeps it for the next visit. The page says how many entries it loaded and why it skipped any (not an object, no valid name, not a placeholder plugin). A file with no usable entry, `[]` included, changes nothing; **Clear my plugins** removes yours. Plugins appear in the placeholder picker under their category, with their options and hints; options a plugin does not accept preview as nothing, as in `bar render`. A name in no manifest previews as itself.

## Editing

**+ placeholder** opens a search over core placeholders and plugins, each with its description and a sample drawn with the preview data; a name that is in neither list is added as a plugin. Added to a group that ends in a placeholder, it comes after a separator, so values never run together. A chip opens its editor on click: each option with its default and a line of help, or the text of a text chip (spaces show as `␠`).

Keyboard: each group's chips are one tab stop. Arrow keys move between chips, Alt+arrow keys reorder, Delete removes, Enter opens the editor and Escape closes it. The line tabs (one per line in use, a spare empty line after the last, at least four, up to line 64) take arrow keys, Home and End. Focus stays on the chip or field you edited.

The `bar.conf` text is the state. Layout, preset and look controls rewrite only the `set` lines they own (`set left`, `set center@2`, `set skin`, ...), so comments, `default`, `rule`, `account`, `plugin` and `set price` lines stay as written. Edit the text directly for anything the controls do not cover; the controls follow.

Issues come from `bar check`, with the options of manifest plugins checked against their schema. A name that is neither a core placeholder nor in the manifest is a warning. While the text has errors the layout and look controls are locked, so nothing rewrites what you typed; **Revert to last valid** goes back to the last text without errors. Over the preview the page says what is wrong (`2 errors in bar.conf — line 1: unknown set key `palete` (did you mean `palette`?)`) and keeps drawing the last valid config, dimmed. Unknown set keys, directives, placeholders and options get a did-you-mean from the names bar knows; each issue's line number selects that line in the text.

The palette control lists the built-in palettes. The page reads no files, so `set palette` with the name of one of your [palette files](config.md#palette-files) is an error here; it works with the CLI.

**Undo** and **Redo** (Ctrl/Cmd+Z, Ctrl/Cmd+Shift+Z) step through every change, from the controls or typed. The page keeps the config, its undo history and the last valid text in the browser's local storage, so a reload can still undo.

## Font

Icons and the powerline and capsule separators need a Nerd Font. The page uses an installed Hack Nerd Font Mono, else the copy in `web/fonts/`: Hack Nerd Font Mono from Nerd Fonts v3.4.0, subset to text and the icons bar and its reference plugins draw (about 80 KB per weight; licences in `web/fonts/LICENSE`). `scripts/web-font` rebuilds it. An icon outside the subset needs the installed font.
