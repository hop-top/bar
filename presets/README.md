# presets

Example configs, built into the `bar` binary ([`src/presets.rs`](../src/presets.rs)). `bar preview --preset` shows them all with sample data; `bar init --preset <name>` writes one as your config (`full` when no name is given). Pictures: [gallery](../docs/gallery.md); options: [Presets and bar init](../docs/config.md#presets).

| Preset | Layout |
|---|---|
| [`minimal`](minimal.conf) | Model, context and the 5-hour window |
| [`full`](full.conf) | The built-in layout plus git state and open sessions |
| [`powerline`](powerline.conf) | Segments on coloured backgrounds joined by arrows; needs a Nerd Font or Powerline-patched font |
| [`capsule`](capsule.conf) | Each segment in a rounded pill; needs a Nerd Font |
| [`compact`](compact.conf) | Short values for narrow panes |
| [`two-line`](two-line.conf) | Session and git on the first line, usage and sessions below |

Each file starts with a comment line naming it. A new preset goes in `NAMES` and `get` in `src/presets.rs`; [`tests/presets.rs`](../tests/presets.rs) checks that every file matches what the binary holds and passes `bar check`. The [web configurator](../docs/web.md) offers the same list.
