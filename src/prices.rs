//! Per-model token prices, dollars per million tokens kept as integer
//! micro-dollars: `[input, output, cache write (5 min), cache read]`.

pub type Rates = [u64; 4];

/// Built-in table, matched by model-id prefix, longest first wins.
/// Source: the Anthropic pricing page (platform.claude.com/docs/en/
/// about-claude/pricing), read 2026-11-07. Haiku 5.5 uses the
/// "up to 100,000 tokens" tier; cache writes use the 5-minute rate.
/// Prefixes stop short of unlisted versions on purpose (`claude-opus-4`
/// alone would price a future 4.x as Opus 4).
const BUILTIN: &[(&str, Rates)] = &[
    ("claude-fable-5-1", m(10_000, 50_000, 12_500, 250)),
    ("claude-fable-5", m(10_000, 50_000, 12_500, 1_000)),
    ("claude-mythos-5-1", m(10_000, 50_000, 12_500, 250)),
    ("claude-mythos-5", m(10_000, 50_000, 12_500, 1_000)),
    ("claude-opus-5-5", m(4_000, 20_000, 5_000, 200)),
    ("claude-opus-5", m(5_000, 25_000, 6_250, 500)),
    ("claude-opus-4-8", m(5_000, 25_000, 6_250, 500)),
    ("claude-opus-4-7", m(5_000, 25_000, 6_250, 500)),
    ("claude-opus-4-6", m(5_000, 25_000, 6_250, 500)),
    ("claude-opus-4-5", m(5_000, 25_000, 6_250, 500)),
    ("claude-opus-4-1", m(15_000, 75_000, 18_750, 1_500)),
    ("claude-opus-4-0", m(15_000, 75_000, 18_750, 1_500)),
    ("claude-opus-4-2025", m(15_000, 75_000, 18_750, 1_500)),
    ("claude-sonnet-5-5", m(2_000, 10_000, 2_500, 100)),
    ("claude-sonnet-5", m(2_000, 10_000, 2_500, 200)),
    ("claude-sonnet-4-6", m(3_000, 15_000, 3_750, 300)),
    ("claude-sonnet-4-5", m(3_000, 15_000, 3_750, 300)),
    ("claude-sonnet-4-0", m(3_000, 15_000, 3_750, 300)),
    ("claude-sonnet-4-2025", m(3_000, 15_000, 3_750, 300)),
    ("claude-haiku-5-5", m(100, 500, 125, 10)),
    ("claude-haiku-4-5", m(1_000, 5_000, 1_250, 100)),
];

/// Thousandths of a dollar per MTok to micro-dollars.
const fn m(i: u64, o: u64, w: u64, r: u64) -> Rates {
    [i * 1000, o * 1000, w * 1000, r * 1000]
}

/// `set price <model-prefix> <in> <out> <cache_write> <cache_read>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Price {
    pub prefix: String,
    pub rates: Rates,
}

impl Price {
    /// Rates are dollars per million tokens, non-negative decimals.
    pub fn parse(
        prefix: &str,
        input: &str,
        output: &str,
        write: &str,
        read: &str,
    ) -> Option<Price> {
        let rate = |s: &str| {
            let v: f64 = s.trim().parse().ok()?;
            (v.is_finite() && (0.0..=1e6).contains(&v)).then(|| (v * 1e6).round() as u64)
        };
        (!prefix.is_empty()).then_some(())?;
        Some(Price {
            prefix: prefix.to_string(),
            rates: [rate(input)?, rate(output)?, rate(write)?, rate(read)?],
        })
    }
}

/// An override beats the built-in table; among overrides the longest
/// matching prefix wins, a later line breaking ties.
pub fn lookup(model: &str, overrides: &[Price]) -> Option<Rates> {
    let mut best: Option<&Price> = None;
    for p in overrides.iter().filter(|p| model.starts_with(&p.prefix)) {
        if best.is_none_or(|b| p.prefix.len() >= b.prefix.len()) {
            best = Some(p);
        }
    }
    if let Some(p) = best {
        return Some(p.rates);
    }
    BUILTIN
        .iter()
        .filter(|(p, _)| model.starts_with(p))
        .max_by_key(|(p, _)| p.len())
        .map(|(_, r)| *r)
}
