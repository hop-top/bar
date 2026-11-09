//! `tokens`, `cache`, `cachettl`: totals read from the session transcript.

use super::{Ctx, Placeholder};
use crate::options::Options;
use crate::style::{dim, fg};
use crate::transcript::{self, Stats};

/// Last stretch before the cache goes cold.
const WARN_BELOW: i64 = 60;

/// `12.3k↑ 4.1k↓`: input (incl. cache writes) and output tokens.
pub struct Tokens;
/// `r 1.2M w 80k`: cache reads and writes, or the hit rate.
pub struct CacheStats;
/// `4m12s`: time left on the prompt cache; `cold` once expired.
pub struct CacheTtl;

pub(super) fn stats(ctx: &Ctx) -> Option<Stats> {
    transcript::load(ctx.cache?, &ctx.session.transcript_path)
}

impl Placeholder for Tokens {
    fn render(&self, ctx: &Ctx, opts: &Options) -> Option<String> {
        tokens(ctx, opts, &stats(ctx)?)
    }
}

impl Placeholder for CacheStats {
    fn render(&self, ctx: &Ctx, opts: &Options) -> Option<String> {
        cache(ctx, opts, &stats(ctx)?)
    }
}

impl Placeholder for CacheTtl {
    fn render(&self, ctx: &Ctx, opts: &Options) -> Option<String> {
        cachettl(ctx, opts, &stats(ctx)?)
    }
}

/// `1.2k`, `80k`, `1.2M`, `2.5B`: one decimal, a trailing `.0` dropped,
/// unit chosen after rounding (so 999 950 reads `1M`).
pub fn compact(n: u64) -> String {
    if n < 1000 {
        return n.to_string();
    }
    let tenths = |div: u64| ((u128::from(n) * 10 + u128::from(div) / 2) / u128::from(div)) as u64;
    let (t, unit) = if tenths(1_000) < 10_000 {
        (tenths(1_000), 'k')
    } else if tenths(1_000_000) < 10_000 {
        (tenths(1_000_000), 'M')
    } else {
        (tenths(1_000_000_000), 'B')
    };
    if t % 10 == 0 {
        format!("{}{unit}", t / 10)
    } else {
        format!("{}.{}{unit}", t / 10, t % 10)
    }
}

fn input_total(s: &Stats) -> u64 {
    s.input.saturating_add(s.cache_creation)
}

pub fn tokens(_ctx: &Ctx, opts: &Options, s: &Stats) -> Option<String> {
    let (i, o) = (input_total(s), s.output);
    if i == 0 && o == 0 {
        return None;
    }
    let up = format!("{}{}", compact(i), dim("↑"));
    let down = format!("{}{}", compact(o), dim("↓"));
    Some(match opts.get("show") {
        Some("in") => up,
        Some("out") => down,
        Some("total") => compact(i.saturating_add(o)),
        _ => format!("{up} {down}"),
    })
}

pub fn cache(_ctx: &Ctx, opts: &Options, s: &Stats) -> Option<String> {
    let read = format!("{} {}", dim("r"), compact(s.cache_read));
    let write = format!("{} {}", dim("w"), compact(s.cache_creation));
    match opts.get("show") {
        Some("read") => Some(read),
        Some("write") => Some(write),
        Some("hit") => {
            let all = s
                .cache_read
                .saturating_add(s.input)
                .saturating_add(s.cache_creation);
            (all > 0).then(|| {
                let pct = (s.cache_read as f64 / all as f64 * 100.0).round() as u64;
                format!("{} {pct}%", dim("hit"))
            })
        }
        _ => (s.cache_read > 0 || s.cache_creation > 0).then(|| format!("{read} {write}")),
    }
}

pub fn cachettl(ctx: &Ctx, opts: &Options, s: &Stats) -> Option<String> {
    let ttl = opts
        .get("ttl")
        .and_then(|t| t.trim().parse::<i64>().ok())
        .filter(|&t| t > 0)
        .unwrap_or_else(|| s.turns.ttl());
    let mtime = (s.mtime > 0).then_some(s.mtime);
    // an unfinished scan has only seen older messages; the file's mtime
    // is the better guess for the latest request
    let last = match (s.last_ts, mtime) {
        (Some(t), Some(m)) if s.partial => t.max(m),
        (t, m) => t.or(m)?,
    };
    // a clock behind the transcript counts as just written
    let elapsed = ctx.now.saturating_sub(last).max(0);
    let left = ttl.saturating_sub(elapsed);
    if left <= 0 {
        return Some(dim("cold"));
    }
    let text = match (left / 3600, left / 60 % 60, left % 60) {
        (0, 0, s) => format!("{s}s"),
        (0, m, s) => format!("{m}m{s:02}s"),
        (h, m, _) => format!("{h}h{m:02}m"),
    };
    let c = &ctx.config.colors;
    Some(fg(if left < WARN_BELOW { c.warn } else { c.good }, &text))
}

/// Stand-in values for `bar preview`.
pub fn sample_stats(now: i64) -> Stats {
    Stats {
        input: 9_800,
        cache_creation: 2_500,
        cache_read: 1_200_000,
        output: 4_100,
        compactions: 2,
        skills: 3,
        ctx_first: 41_000,
        ctx_prev: 72_800,
        ctx_last: 76_000,
        ctx_reqs: 15,
        last_ts: Some(now - 48),
        mtime: now,
        partial: false,
        turns: transcript::Turns::sample(),
    }
}
