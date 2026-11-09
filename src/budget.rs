//! Dollar budgets (`set budget-<kind>`, `set budget-warn`) and how spend
//! against one is coloured.

use crate::conf::Config;
use crate::style::{bold, fg, ramp};

/// What a budget limits: the session, today, this month, or the 5-hour block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Session,
    Today,
    Month,
    Block,
}

impl Kind {
    pub const NAMES: [&'static str; 4] = ["session", "today", "month", "block"];

    pub fn parse(s: &str) -> Option<Kind> {
        Some(match s {
            "session" => Kind::Session,
            "today" => Kind::Today,
            "month" => Kind::Month,
            "block" => Kind::Block,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        Kind::NAMES[self as usize]
    }
}

/// Default `budget-warn`, percent of a budget.
pub const DEFAULT_WARN: u32 = 80;

/// Limits in cents (`None`: no budget) and the warn threshold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Budgets {
    limits: [Option<u64>; 4],
    /// Percent of a budget from which spend takes the warn colour.
    pub warn: u32,
}

impl Default for Budgets {
    fn default() -> Self {
        Budgets {
            limits: [None; 4],
            warn: DEFAULT_WARN,
        }
    }
}

impl Budgets {
    pub fn get(&self, kind: Kind) -> Option<u64> {
        self.limits[kind as usize]
    }

    pub fn set(&mut self, kind: Kind, cents: Option<u64>) {
        self.limits[kind as usize] = cents;
    }
}

/// Where spend stands against a budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Standing {
    pub color: crate::color::Color,
    /// Spend is past the limit.
    pub over: bool,
}

impl Standing {
    /// `text` in the standing's colour, bold when over.
    pub fn paint(self, text: &str) -> String {
        let t = fg(self.color, text);
        if self.over {
            bold(&t)
        } else {
            t
        }
    }
}

/// Percent of `limit` cents that `usd` is; 0 for a zero limit.
pub fn used_pct(usd: f64, limit: u64) -> f64 {
    if limit == 0 || !usd.is_finite() {
        return 0.0;
    }
    usd.max(0.0) * 10_000.0 / limit as f64
}

/// Under `warn`: the ramp, from its good end at nothing spent to its middle
/// at `warn`. From `warn`: the warn colour; at the limit: the bad colour;
/// past it: bad and `over`.
pub fn standing(usd: f64, limit: u64, cfg: &Config) -> Standing {
    let used = used_pct(usd, limit);
    let warn = f64::from(cfg.budgets.warn.clamp(1, 100));
    // whole cents decide the limit, so $20.00 of $20 is at it, not over
    let cents = (usd.max(0.0) * 100.0).round();
    let over = cents > limit as f64;
    let color = if cents >= limit as f64 {
        cfg.colors.bad
    } else if used >= warn {
        cfg.colors.warn
    } else {
        let score = 100.0 - used * 50.0 / warn;
        ramp(score.round() as i32, &cfg.ramp)
    };
    Standing { color, over }
}

/// `$20`, or `$12.50` when the limit has cents.
pub fn limit_text(cents: u64) -> String {
    if cents.is_multiple_of(100) {
        format!("${}", cents / 100)
    } else {
        format!("${}.{:02}", cents / 100, cents % 100)
    }
}

/// `amount` painted against the `kind` budget when one is set.
pub fn paint_if_set(cfg: &Config, kind: Kind, usd: f64, amount: String) -> String {
    match cfg.budgets.get(kind) {
        Some(limit) => standing(usd, limit, cfg).paint(&amount),
        None => amount,
    }
}
