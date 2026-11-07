use super::{Ctx, Placeholder};
use crate::click;
use crate::conf::Rule;
use crate::options::Options;
use crate::pace::{builtin_rule, score};
use crate::session::Window;
use crate::style::{dim, fg, link, ramp};
use std::path::Path;

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
    let secs = win.resets_at - ctx.now;
    if secs <= 0 {
        return None;
    }
    let left = pct_left(win.used_percentage);
    let rule = rule_for(ctx, key);
    let color = |at: i64| ramp(score(at, Some(secs), period, &rule), &ctx.config.ramp);
    let time = dim(&time_left(secs));
    let mut view = opts.get("view").unwrap_or("bar");
    if view != "time" && click::is_toggled(state, key) {
        view = if view == "bar" { "pct" } else { "bar" };
    }
    let body = match view {
        "time" => return Some(time),
        "pct" => fg(color(left), &format!("{left}%")),
        _ => bar(opts, left, &color),
    };
    let body = match opts.get("click").unwrap_or("toggle") {
        "toggle" if click::handler_installed(state) => link(&format!("bar://toggle/{key}"), &body),
        _ => body,
    };
    Some(format!("{time} {body}"))
}

/// Each filled cell takes the colour of its own level; the last uses the
/// exact percentage so it matches the percentage view.
fn bar(opts: &Options, left: i64, color: &dyn Fn(i64) -> u8) -> String {
    let width = opts
        .get("width")
        .and_then(|w| w.parse::<i64>().ok())
        .unwrap_or(5)
        .clamp(1, 40);
    let filled = (left * width + 50) / 100;
    let mut out = String::new();
    for i in 0..filled {
        let at = if i == filled - 1 {
            left
        } else {
            (i + 1) * 100 / width
        };
        out.push_str(&fg(color(at), "█"));
    }
    let rest = "░".repeat((width - filled) as usize);
    if !rest.is_empty() {
        out.push_str(&dim(&rest));
    }
    out
}
