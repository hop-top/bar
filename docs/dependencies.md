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
