//! Where Claude Code auto-compacts: the token count a session's context
//! may reach before compaction runs. The statusline JSON does not carry
//! it, so it is worked out the way Claude Code documents it: the
//! `CLAUDE_CODE_AUTO_COMPACT_WINDOW` variable, then the user settings
//! file, then the per-model default, capped at the context window.

use serde_json::Value;

/// Tokens a 1M-window model compacts at by default ("about 967K").
pub const NATIVE_1M_DEFAULT: u64 = 967_000;
/// Claude Code's accepted auto-compact window range.
const MIN_WINDOW: u64 = 100_000;
const MAX_WINDOW: u64 = 1_000_000;

/// `set compact-at`: a token count, or a percent of the context window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompactAt {
    Tokens(u64),
    Pct(u64),
}

impl CompactAt {
    /// `200000`, `200k`, `1M`, `1.5M` or `90%` (1..=100).
    pub fn parse(raw: &str) -> Option<CompactAt> {
        let raw = raw.trim();
        if let Some(p) = raw.strip_suffix('%') {
            let p: u64 = p.trim().parse().ok()?;
            return (1..=100).contains(&p).then_some(CompactAt::Pct(p));
        }
        tokens(raw).filter(|&n| n >= 1).map(CompactAt::Tokens)
    }
}

/// A token count with an optional `k` / `M` suffix; `None` when it is
/// not a non-negative number.
fn tokens(raw: &str) -> Option<u64> {
    let raw = raw.trim();
    let (num, mul) = match raw.as_bytes().last()? {
        b'k' | b'K' => (&raw[..raw.len() - 1], 1e3),
        b'm' | b'M' => (&raw[..raw.len() - 1], 1e6),
        _ => (raw, 1.0),
    };
    let n: f64 = num.trim().parse().ok()?;
    (n.is_finite() && n >= 0.0).then(|| (n * mul).round() as u64)
}

fn truthy(v: Option<String>) -> bool {
    v.is_some_and(|v| matches!(v.trim(), "1" | "true"))
}

/// A settings `autoCompactWindow`: `Some(None)` for `"auto"` (the tuned
/// default), `Some(Some(n))` for a number, `None` when unset or unusable.
fn setting_window(v: Option<&Value>) -> Option<Option<u64>> {
    let v = v?;
    if v.as_str() == Some("auto") {
        return Some(None);
    }
    let n = v.as_f64().filter(|n| n.is_finite() && *n > 0.0)?;
    Some(Some((n as u64).clamp(MIN_WINDOW, MAX_WINDOW)))
}

/// The window Claude Code picks when none is set.
pub fn default_window(window: u64) -> u64 {
    if window >= MAX_WINDOW {
        NATIVE_1M_DEFAULT
    } else {
        window
    }
}

/// Tokens at which the session compacts; `None` when auto-compaction is
/// off or the context window is unknown. `env` reads a variable,
/// `settings` the parsed user settings file (called only when needed).
pub fn threshold(
    window: u64,
    model_id: &str,
    configured: Option<CompactAt>,
    env: &dyn Fn(&str) -> Option<String>,
    settings: &dyn Fn() -> Option<Value>,
) -> Option<u64> {
    if window == 0 {
        return None;
    }
    if let Some(c) = configured {
        return Some(match c {
            CompactAt::Tokens(n) => n.min(window),
            CompactAt::Pct(p) => (window * p / 100).max(1),
        });
    }
    if truthy(env("DISABLE_AUTO_COMPACT")) || truthy(env("DISABLE_COMPACT")) {
        return None;
    }
    let set = settings();
    let get = |k: &str| set.as_ref().and_then(|s| s.get(k));
    if get("autoCompactEnabled").and_then(Value::as_bool) == Some(false) {
        return None;
    }
    let per_model = || {
        let m = get("modelSettings")?.get(model_id)?;
        setting_window(m.get("autoCompactWindow"))
    };
    let chosen = env("CLAUDE_CODE_AUTO_COMPACT_WINDOW")
        .and_then(|v| tokens(&v))
        .map(|n| Some(n.clamp(MIN_WINDOW, MAX_WINDOW)))
        .or_else(per_model)
        .or_else(|| setting_window(get("autoCompactWindow")))
        .flatten();
    let mut at = chosen.unwrap_or_else(|| default_window(window)).min(window);
    // the override applies only where compaction runs before the limit
    if at < window {
        let pct = env("CLAUDE_AUTOCOMPACT_PCT_OVERRIDE")
            .and_then(|v| v.trim().parse::<u64>().ok())
            .filter(|p| (1..=100).contains(p));
        if let Some(p) = pct {
            at = (at * p / 100).max(1);
        }
    }
    Some(at)
}
