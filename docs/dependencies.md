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

## libc =0.2.191

- Use: `localtime_r` and `tm` for local-date buckets in spend history, instead of a hand-written `struct tm`.
- Source: `rust-lang/libc`; already in `Cargo.lock` at 0.2.191 through `hop-top-kit`'s dependencies, so the pin resolves to the existing copy and adds nothing new to the build.
- License: MIT OR Apache-2.0.

## Data: models.dev price snapshot

- Use: prices for models outside the built-in Anthropic table, embedded as `data/models.json` and refreshed with `scripts/update-prices` (or `bar prices update` for a per-user copy).
- Source: `https://models.dev/api.json`, project `sst/models.dev`.
- License: MIT (Copyright (c) 2025 models.dev), checked 2026-11-07; the licence permits redistribution of the data with the notice.
- Kept per model: provider, id, input/output/cache rates (USD per million tokens), context limit. The file header records the source and fetch date.
- No crate is added; the fetch shells out to `curl` with a fixed argv.
