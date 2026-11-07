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
