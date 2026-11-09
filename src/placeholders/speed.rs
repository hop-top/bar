//! `ttft`, `tps`, `misses`: turn timing and prompt-cache misses from the
//! transcript scan.

use super::cost::usd;
use super::tokens::{compact, stats};
use super::{Ctx, Placeholder};
use crate::options::Options;
use crate::prices;
use crate::style::dim;
use crate::transcript::Turns;

/// `ttft 1.4s`: time to first token of the last prompt, or the average.
pub struct Ttft;
/// `43 t/s`: output tokens per second over the last turn, or the session.
pub struct Tps;
/// `miss 2 ttl 1 inv 1`: requests that found the prompt cache cold.
pub struct Misses;

impl Placeholder for Ttft {
    fn render(&self, ctx: &Ctx, opts: &Options) -> Option<String> {
        self.render_labeled(ctx, opts, true)
    }

    fn render_labeled(&self, ctx: &Ctx, opts: &Options, label: bool) -> Option<String> {
        ttft(opts, &stats(ctx)?.turns, label)
    }
}

impl Placeholder for Tps {
    fn render(&self, ctx: &Ctx, opts: &Options) -> Option<String> {
        tps(opts, &stats(ctx)?.turns)
    }
}

impl Placeholder for Misses {
    fn render(&self, ctx: &Ctx, opts: &Options) -> Option<String> {
        self.render_labeled(ctx, opts, true)
    }

    fn render_labeled(&self, ctx: &Ctx, opts: &Options, label: bool) -> Option<String> {
        misses(ctx, opts, &stats(ctx)?.turns, label)
    }
}

/// `0.8s`, `12s`, `1m05s`, `1h05m`.
pub fn secs(ms: i64) -> String {
    let ms = ms.max(0);
    if ms < 9_950 {
        let tenths = (ms + 50) / 100;
        return format!("{}.{}s", tenths / 10, tenths % 10);
    }
    let s = (ms + 500) / 1000;
    match (s / 3600, s / 60 % 60, s % 60) {
        (0, 0, s) => format!("{s}s"),
        (0, m, s) => format!("{m}m{s:02}s"),
        (h, m, _) => format!("{h}h{m:02}m"),
    }
}

/// `4.2`, `43`: one decimal under 10.
fn per_sec(r: f64) -> String {
    if r < 9.95 {
        format!("{r:.1}")
    } else {
        format!("{r:.0}")
    }
}

/// `last`, `avg 1.8`, or both, from `show`.
fn last_avg<T>(
    opts: &Options,
    last: Option<T>,
    avg: Option<T>,
    f: impl Fn(T) -> String,
) -> Option<String> {
    let avg = avg.map(|a| format!("{} {}", dim("avg"), f(a)));
    let last = last.map(&f);
    match opts.get("show") {
        Some("avg") => avg,
        Some("both") => match (last, avg) {
            (Some(l), Some(a)) => Some(format!("{l} {a}")),
            (l, a) => l.or(a),
        },
        _ => last,
    }
}

pub fn ttft(opts: &Options, t: &Turns, label: bool) -> Option<String> {
    let v = last_avg(opts, t.ttft_last(), t.ttft_avg(), secs)?;
    Some(if label {
        format!("{} {v}", dim("ttft"))
    } else {
        v
    })
}

pub fn tps(opts: &Options, t: &Turns) -> Option<String> {
    let v = last_avg(opts, t.tps_last(), t.tps_avg(), per_sec)?;
    Some(format!("{v} {}", dim("t/s")))
}

pub const SHOW: &[&str] = &["count", "cause", "cold"];

/// Extra dollars the next request pays if the cache is cold: the cached
/// prefix written again instead of read. `None` when the model is unpriced.
pub fn cold_usd(ctx: &Ctx, t: &Turns) -> Option<f64> {
    let model = ctx.session.model_id.as_str();
    let overrides = &ctx.config.prices;
    (!model.is_empty()).then_some(())?;
    let r = prices::lookup(model, overrides)?;
    // the 1-hour cache writes at twice the input rate
    let write = if t.one_hour() { r[0] * 2 } else { r[2] };
    let long = prices::long_over(model).is_some_and(|over| t.prefix() > over);
    let k = if long {
        prices::long_mult(model, overrides)
    } else {
        1
    };
    let per = write.saturating_sub(r[3]).saturating_mul(k);
    Some(t.prefix() as f64 * per as f64 / 1e12)
}

pub fn misses(ctx: &Ctx, opts: &Options, t: &Turns, label: bool) -> Option<String> {
    let (ttl, inv) = t.misses();
    let n = ttl + inv;
    let show = opts.get("show").unwrap_or("count,cause");
    let mut parts = Vec::new();
    for item in show.split(',').map(str::trim) {
        match item {
            "count" if n > 0 && label => parts.push(format!("{} {n}", dim("miss"))),
            "count" if n > 0 => parts.push(n.to_string()),
            "cause" => {
                for (k, v) in [("ttl", ttl), ("inv", inv)] {
                    if v > 0 {
                        parts.push(format!("{} {v}", dim(k)));
                    }
                }
            }
            "cold" if t.prefix() > 0 => {
                let mut p = format!("{} {}", dim("cold"), compact(t.prefix()));
                if let Some(d) = cold_usd(ctx, t) {
                    p += &format!(" +{}", usd(d));
                }
                parts.push(p);
            }
            _ => {}
        }
    }
    (!parts.is_empty()).then(|| parts.join(" "))
}
