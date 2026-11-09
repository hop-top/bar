//! The `show` components of the `ctx` placeholder and their text helpers.

use crate::placeholders::tokens::compact;
use crate::session::CtxUsage;
use crate::style::dim;

/// Components `show` accepts.
pub const SHOW_ITEMS: [&str; 8] = [
    "pct", "bar", "gauge", "tokens", "left", "split", "growth", "trend",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Part {
    Pct,
    Bar,
    Gauge,
    Tokens,
    Left,
    Split,
    Growth,
    Trend,
}

/// Known items in order, repeats dropped; unknown ones are skipped.
pub fn parse_show(raw: &str) -> Vec<Part> {
    let mut out = Vec::new();
    for item in raw.split(',') {
        let part = match item.trim() {
            "pct" => Part::Pct,
            "bar" => Part::Bar,
            "gauge" => Part::Gauge,
            "tokens" => Part::Tokens,
            "left" => Part::Left,
            "split" => Part::Split,
            "growth" => Part::Growth,
            "trend" => Part::Trend,
            _ => continue,
        };
        if !out.contains(&part) {
            out.push(part);
        }
    }
    out
}

/// `76k`, `1.2M`: the compact number format of `tokens`.
pub fn num(n: u64) -> String {
    compact(n)
}

/// `+3.2k`, `-800`, `+0`.
pub fn signed(n: i64) -> String {
    let sign = if n < 0 { '-' } else { '+' };
    format!("{sign}{}", compact(n.unsigned_abs()))
}

/// `r 70k w 4.8k new 1.2k`: the last request's context read from cache,
/// written to cache and sent uncached; hidden when all are 0.
pub fn split(u: &CtxUsage) -> Option<String> {
    if u.cache_read == 0 && u.cache_creation == 0 && u.input == 0 {
        return None;
    }
    Some(format!(
        "{} {} {} {} {} {}",
        dim("r"),
        compact(u.cache_read),
        dim("w"),
        compact(u.cache_creation),
        dim("new"),
        compact(u.input)
    ))
}
