use super::{Ctx, Placeholder};
use crate::click;
use crate::conf::Rule;
use crate::options::Options;
use crate::pace::{builtin_rule, score};
use crate::session::Window;
use crate::style::{dim, fg, link, ramp};
use glyphs::{parse_list, swapped, view_alias, Part};
use std::path::Path;

pub mod glyphs;

/// `2h13m ███░░`: time to reset plus a pace-coloured bar, percentage or time.
pub struct Usage;

impl Placeholder for Usage {
    fn render(&self, ctx: &Ctx, opts: &Options) -> Option<String> {
        render_with(ctx, opts, &click::state_dir())
    }
}

/// `3d4h`, `3d`, `2h13m`, `1h0m`, `45m`, `0m`.
pub fn time_left(secs: i64) -> String {
    let (d, h, m) = (secs / 86400, secs % 86400 / 3600, secs % 3600 / 60);
    match (d, h) {
        (1.., 0) => format!("{d}d"),
        (1.., _) => format!("{d}d{h}h"),
        (0, 1..) => format!("{h}h{m}m"),
        _ => format!("{m}m"),
    }
}

/// Percent left, rounded half up, clamped to 0..=100.
pub fn pct_left(used: f64) -> i64 {
    if !used.is_finite() {
        return 0;
    }
    (100.0 - used).clamp(0.0, 100.0).round() as i64
}

fn rule_for(ctx: &Ctx, key: &str) -> Rule {
    let target = format!("usage[type={key}]");
    ctx.config
        .rules
        .iter()
        .find(|r| r.target == target)
        .cloned()
        .unwrap_or_else(|| builtin_rule(key))
}

/// Key, window and period in seconds for the `type` option.
fn pick<'a>(ctx: &'a Ctx, ty: &str) -> Option<(&'static str, &'a Window, i64)> {
    match ty {
        "5h" | "h" => Some(("5h", ctx.session.five_hour.as_ref()?, 5 * 3600)),
        "7d" | "d" => Some(("7d", ctx.session.seven_day.as_ref()?, 7 * 86400)),
        _ => None,
    }
}

/// `state` holds the toggle files and the handler marker.
pub fn render_with(ctx: &Ctx, opts: &Options, state: &Path) -> Option<String> {
    let (key, win, period) = pick(ctx, opts.get("type").unwrap_or("5h"))?;
    let secs = win.resets_at.saturating_sub(ctx.now);
    if secs <= 0 {
        return None;
    }
    let left = pct_left(win.used_percentage);
    let rule = rule_for(ctx, key);
    let color = ramp(score(left, Some(secs), period, &rule), &ctx.config.ramp);
    let time = dim(&time_left(secs));
    let mut show = match opts.get("show") {
        Some(raw) if !parse_list(raw).is_empty() => parse_list(raw),
        _ => view_alias(opts.get("view").unwrap_or("bar")),
    };
    if click::is_toggled(state, key) {
        show = match opts.get("alt").map(parse_list) {
            Some(alt) if !alt.is_empty() => alt,
            _ => swapped(&show),
        };
    }
    let value = if opts.get("count") == Some("used") {
        100 - left
    } else {
        left
    };
    let linked =
        opts.get("click").unwrap_or("toggle") == "toggle" && click::handler_installed(state);
    let wrap = |body: String| {
        if linked {
            link(&format!("bar://toggle/{key}"), &body)
        } else {
            body
        }
    };
    let parts: Vec<String> = show
        .iter()
        .map(|p| match p {
            Part::Time => time.clone(),
            Part::Pct => wrap(fg(color, &format!("{value}%"))),
            Part::Bar => wrap(bar(opts, value, color)),
        })
        .collect();
    Some(parts.join(" "))
}

/// One colour for the whole bar: the exact value's, as in the percentage view.
fn bar(opts: &Options, value: i64, color: u8) -> String {
    let width = opts
        .get("width")
        .and_then(|w| w.parse::<i64>().ok())
        .unwrap_or(5)
        .clamp(1, 40);
    glyphs::bar(opts.get("glyphs").unwrap_or("blocks"), width, value, color)
}
