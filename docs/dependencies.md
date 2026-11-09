# Dependencies

Vetting record for third-party crates added to `hop-bar`.

## unicode-width =0.2.2

- Use: terminal cell width of visible text (CJK and emoji count as 2).
- Source: `unicode-rs/unicode-width`; crates.io release 2025-10-06, over 14 days old.
- Newer 0.2.3 (2026-10-28) is under the 14-day age gate, so 0.2.2 is pinned exactly.
- License: MIT OR Apache-2.0.
- `rsx analyze`: PASS, score 4.0/10, policy balanced (activity and release-cadence scores low; the crate is small and stable).
- Advisories: OSV query for crates.io `unicode-width` 0.2.2 returns none.
- Shared with `hop-top-kit` through `comfy-table`; the pin resolves to one copy.

## unicode-segmentation =1.13.3

- Use: extended grapheme clusters (UAX #29) for `fg=` and gradients, so an emoji ZWJ sequence, a flag or an emoji with its variation selector takes one colour and is measured as one.
- Source: `unicode-rs/unicode-segmentation`; crates.io release 2026-06-01, over 14 days old. Already in `Cargo.lock` (and `web/wasm/Cargo.lock`) at 1.13.3 through `hop-top-kit` (`comfy-table`), so the exact pin resolves to the existing copy and adds no crate to the build.
- Alternatives: hand-written cluster rules (ZWJ, regional-indicator pairs, variation selectors, combining marks), which drift from the Unicode tables the crate tracks.
- License: MIT OR Apache-2.0.
- `rsx analyze unicode-rs/unicode-segmentation`: PASS, score 4.6/10, policy balanced (activity and release-hygiene scores low; the crate is small and stable).
- Advisories: OSV query for crates.io `unicode-segmentation` 1.13.3 returns none.
- Binary size: the release `bar` grows by about 15 KB on macOS arm64.

## libc =0.2.191 (Unix only)

- Use: `localtime_r` and `tm` for local-date buckets in spend history, instead of a hand-written `struct tm`; `kill(pid, 0)` for session liveness. Declared under `[target.'cfg(unix)'.dependencies]`.
- Source: `rust-lang/libc`; already in `Cargo.lock` at 0.2.191 through `hop-top-kit`'s dependencies, so the pin resolves to the existing copy and adds nothing new to the build.
- License: MIT OR Apache-2.0.

## windows-sys =0.61.2 (Windows only)

- Use: kernel32 calls in `src/platform/windows.rs`: clearing the inherit flag on the standard handles before a detached spawn (`GetStdHandle`, `SetHandleInformation`), process liveness (`OpenProcess`, `GetExitCodeProcess`), local zone offset (`GetDynamicTimeZoneInformation`, `SystemTimeToTzSpecificLocalTimeEx`, FILETIME/SYSTEMTIME conversions), and the `CREATE_*`/`DETACHED_PROCESS` flag constants. Features: `Win32_Foundation`, `Win32_System_Console`, `Win32_System_Threading`, `Win32_System_Time`. Declared under `[target.'cfg(windows)'.dependencies]`, so Unix builds do not compile it.
- Source: `microsoft/windows-rs`; crates.io release 2025-10-06, over 14 days old. Already in `Cargo.lock` at 0.61.2 through `clap` (`anstream`, `anstyle-query`, `anstyle-wincon` on Windows), so the exact pin resolves to the existing copy; its only dependency is `windows-link` 0.2.1, also already locked.
- Alternatives: hand-written `extern "system"` declarations (more unsafe surface, no type checking against the Win32 metadata); `chrono`/`time` for the zone offset (a full date-time crate for one number); `dirs` for the home dir (the kit `xdg` module and `USERPROFILE` already cover it).
- License: MIT OR Apache-2.0.
- `rsx analyze microsoft/windows-rs`: PASS, score 6.6/10, policy balanced.
- Advisories: OSV query for crates.io `windows-sys` 0.61.2 returns none.

## ratatui =0.30.2 (feature `edit`)

- Use: drawing for `bar edit` (layout, blocks, lists, paragraphs) and `TestBackend` for its screen tests; `ratatui::try_init`/`restore` set up and restore the terminal, panic included. `default-features = false`, feature `crossterm` only (no macros, calendar, layout cache or underline colour). Optional, enabled by the default `edit` feature; the render path does not reach it.
- Source: `ratatui/ratatui`; crates.io release 2026-06-19, over 14 days old. Pulls `ratatui-core`, `ratatui-widgets` and `ratatui-crossterm` 0.30.2's split crates, all released the same day.
- Alternatives: raw `crossterm` drawing (hand-written layout, diffing and widgets); `cursive` (larger, own event loop and backends).
- License: MIT.
- `rsx analyze ratatui/ratatui`: PASS, score 7.8/10, policy balanced.
- Advisories: OSV query for crates.io `ratatui` 0.30.2 returns none.
- Transitive crates new to the lock are all over 14 days old; `log`, `rustix` and `smallvec` are held at 0.4.34, 1.1.5 and 1.16.4 because newer releases are under the age gate.

## crossterm =0.29.0 (feature `edit`)

- Use: raw mode, alternate screen and key events for `bar edit` on Unix and Windows consoles (key releases, which Windows reports, are ignored). The same version `ratatui-crossterm` uses, so the pin resolves to one copy. Optional, enabled by the default `edit` feature.
- Source: `crossterm-rs/crossterm`; crates.io release 2025-04-05, over 14 days old.
- License: MIT.
- `rsx analyze crossterm-rs/crossterm`: PASS, score 6.3/10, policy balanced.
- Advisories: OSV query for crates.io `crossterm` 0.29.0 returns none.

Binary size: the release `bar` grows by about 390 KB (2.11 MB to 2.50 MB on macOS arm64, 1.80 MB to 2.13 MB stripped) with `edit`; without it the binary is unchanged. `bar render` start-up is unchanged within run-to-run noise (about 3.6 to 3.9 ms wall, 2.4 to 2.6 ms CPU, either way).

## Data: models.dev price snapshot

- Use: prices for models outside the built-in Anthropic table. Not in the repo, the binary or release archives: fetched at run time into the user's data dir (`$XDG_DATA_HOME/bar/models.json`) by `bar prices update`, or in the background when `spendhist` meets a model no `set price` rule or built-in price covers (at most daily; see [config](config.md#spendhist)).
- Source: `https://models.dev/api.json`, project `sst/models.dev`.
- License: MIT (Copyright (c) 2025 models.dev), checked 2026-11-07. bar redistributes none of it.
- Kept per model: provider, id, input/output/cache rates (USD per million tokens). The file header records the source and fetch date.
- No crate is added; the fetch shells out to `curl` with a fixed argv.

## Gallery tooling (not built into bar)

`scripts/gallery` draws [docs/gallery.md](gallery.md) images. It is a development script; nothing here ships in the binary or release archives.

### fontTools ==4.60.2 (Python)

- Use: glyph outlines from the font, written as SVG paths so the images need no font in the viewer. Only `make gallery` needs it; `make gallery-check` and `make check` run on the Python 3 standard library.
- Install: `python3 -m pip install --user fonttools==4.60.2`.
- Source: `fonttools/fonttools`; PyPI release 2025-12-09, over 14 days old.
- License: MIT.
- `rsx analyze`: PASS, score 6.7/10, policy balanced.
- Advisories: OSV query for PyPI `fonttools` 4.60.2 returns none (4.60.1 and earlier: GHSA-768j-98cg-p3fv, in `fontTools.varLib`, which the script does not use).

### Font: Hack Nerd Font Mono

- Use: Regular and Bold faces, `HackNerdFontMono-Regular.ttf` and `HackNerdFontMono-Bold.ttf`, looked up in `~/Library/Fonts`, `/Library/Fonts`, `~/.local/share/fonts`, `/usr/share/fonts` or `GALLERY_FONT_DIR`. Not committed; the images hold only the outlines of the glyphs they show.
- Source: Nerd Fonts (`ryanoasis/nerd-fonts`), patched Hack. Licences: Hack MIT plus the Bitstream Vera licence; the added glyph sets keep their own (Powerline symbols MIT, Font Awesome SIL OFL 1.1, Octicons MIT).
- `✗` and `✓` are missing from the font; the script draws them as simple strokes.

## Benchmark image

`bench/image/Dockerfile` builds the container `make bench` runs in ([bench/README.md](../bench/README.md#running)). Nothing in it ships in the binary or release archives. Every pin below was released at least 14 days before it was pinned; checksums are in the Dockerfile.

| Component | Pin | Released | Checked by |
|---|---|---|---|
| Base: `python:3.14.8-slim-trixie` (Debian 13) | index digest `sha256:fbecd461…` | 2026-10-21 | digest |
| Debian packages (gcc, make, zlib, curl, jq, procps, xz, unzip) | snapshot.debian.org `20261020T000000Z` | snapshot date | apt signatures |
| `golang:1.27.2-trixie` (builds ben only) | index digest `sha256:47667934…` | 2026-10-13 | digest |
| Node.js | 26.12.0 | 2026-10-14 | sha256 (`SHASUMS256.txt`) |
| npm | 12.2.1 | 2026-10-15 | registry sha512 |
| Bun | 1.4.4 | 2026-10-22 | sha256 (release asset digest) |
| Rust (rustc, cargo, std; standalone installer) | 1.99.1 | 2026-10-15 | sha256 (`.sha256` file) |
| git (from source, `NO_RUST`) | 2.56.0 | 2026-09-28 | sha256 (`sha256sums.asc`) |
| GNU time (from source) | 1.10 | 2026-04-15 | sha256 |
| hyperfine | 2.0.0 | 2026-10-07 | sha256 (release asset digest) |
| ccstatusline | 2.2.32 | 2026-10-09 | registry sha512; no dependencies, no install scripts; installed with `--ignore-scripts` |
| @owloops/claude-powerline | 1.32.5 | 2026-10-24 | registry sha512; no dependencies, no install scripts; installed with `--ignore-scripts` |
| CCometixLine (`ccline`) | 2.0.1 | 2026-10-24 | sha256 (release asset digest) |
| ben (`hop-top/ben`) | commit `2b24904` | owned, exempt from the age gate | commit id; Go modules against its `go.sum` |

- ccstatusline 2.2.34 and claude-powerline 1.32.8 (both 2026-11-06) were under the age gate when the image was pinned, so the newest releases older than 14 days are used.
- ben depends on `hop.top/uri`, a private module: building the image needs a GitHub token with read access to `hop-top/uri`, passed as a build secret (`scripts/bench`).
- The rival tools run in the container with the repository snapshot from `git archive`, never the working tree; results leave through one output directory.

## Web configurator (`web/`, not built into bar)

`web/wasm` compiles bar's renderer to WebAssembly for the [web configurator](web.md). It is its own Cargo workspace with its own `Cargo.lock`, seeded from bar's so shared crates resolve to the same versions; the `bar` binary and release archives do not include it.

### wasm-bindgen =0.2.121

- Use: JavaScript bindings for the six exported functions (`render`, `check`, `inspect`, `tokens`, `schema`, `preset`); `wasm-bindgen-cli` at the same version writes the glue in `web/pkg/`.
- Source: `wasm-bindgen/wasm-bindgen`; crates.io release 2026-05-07, over 14 days old.
- Adds, in `web/wasm/Cargo.lock` only: `wasm-bindgen-macro`, `wasm-bindgen-macro-support`, `wasm-bindgen-shared` (all 0.2.121), `bumpalo` 3.20.3 (2026-05-22), `cfg-if` 1.0.5 (2026-09-16), `once_cell` 1.21.4 (2026-03-12), `rustversion` 1.0.23 (2026-07-07), `syn` 2.0.119 (2026-07-15); each over 14 days old.
- Alternatives: hand-written `extern "C"` exports with manual string passing through linear memory (more unsafe code for the same six calls); `wasm-pack` (wraps the same `wasm-bindgen`, adds an npm package layout the page does not need).
- License: MIT OR Apache-2.0 (the added crates too).
- `rsx analyze wasm-bindgen/wasm-bindgen`: PASS, score 7.3/10, policy balanced.
- Advisories: OSV queries for crates.io `wasm-bindgen` 0.2.121 and `bumpalo` 3.20.3 return none.

### Font: Hack Nerd Font Mono (loaded by the page)

- Use: icons and skin separators in the preview. The page uses an installed copy, else fetches `HackNerdFontMono-Regular.ttf` and `-Bold.ttf` from jsDelivr (`cdn.jsdelivr.net/gh/ryanoasis/nerd-fonts@v3.4.0`), else the system monospace font. Nothing is committed or redistributed.
- Source: Nerd Fonts v3.4.0, released 2025-04-24. Licences as under [Gallery tooling](#font-hack-nerd-font-mono).
