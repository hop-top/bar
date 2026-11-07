//! Segment icons: built-in glyph sets, the generic `icon=` option and how an
//! icon joins a rendered value.

use crate::conf::{Config, Icons};
use crate::options::Options;
use crate::style::{dim, sanitize, visible_width_with};

/// Widest icon accepted, in cells.
pub const MAX_CELLS: usize = 2;

/// What `icon=` asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pick {
    /// Not given: the icon set decides.
    Auto,
    /// `icon=none`.
    Hide,
    Glyph(String),
}

/// The `icon` option split off; the placeholder never sees it.
pub fn split(opts: &Options) -> (Pick, Options) {
    let mut rest = Options::default();
    let mut pick = Pick::Auto;
    for (k, v) in opts.iter() {
        if k == "icon" {
            let g = sanitize(v);
            pick = if v == "none" || g.is_empty() {
                Pick::Hide
            } else {
                Pick::Glyph(g)
            };
        } else {
            rest.set(k, v);
        }
    }
    (pick, rest)
}

/// Whether `raw` is acceptable as `icon=` (`none` included).
pub fn valid(raw: &str, icon_width: usize) -> bool {
    raw == "none"
        || (!raw.is_empty()
            && sanitize(raw) == raw
            && visible_width_with(raw, icon_width) <= MAX_CELLS)
}

/// The glyph to draw: an explicit one beats the set; `default` is the
/// built-in or plugin-supplied one, used only when a set is chosen.
pub fn resolve(config: &Config, pick: &Pick, default: Option<&str>) -> Option<String> {
    let glyph = match pick {
        Pick::Hide => return None,
        Pick::Glyph(g) => g.clone(),
        Pick::Auto if config.icons == Icons::None => return None,
        Pick::Auto => sanitize(default?),
    };
    (!glyph.trim().is_empty() && visible_width_with(&glyph, config.icon_width) <= MAX_CELLS)
        .then_some(glyph)
}

/// `value` led by the muted icon and a space. Outside any link in `value`.
pub fn attach(glyph: &str, value: String) -> String {
    format!("{} {value}", dim(glyph))
}

/// Glyph of a core placeholder in `set`; `opts` tells the usage windows apart.
pub fn builtin(set: Icons, name: &str, opts: &Options) -> Option<&'static str> {
    let usage_7d = matches!(opts.get("type"), Some("7d" | "d"));
    // (nerdfont, emoji)
    let (nf, emoji) = match name {
        "model" => ("\u{f2db}", "🤖"),             // nf-fa-microchip
        "effort" => ("\u{f0e7}", "⚡"),            // nf-fa-bolt
        "ctx" => ("\u{f0e4}", "🧠"),               // nf-fa-tachometer
        "repo" => ("\u{f401}", "📁"),              // nf-oct-repo
        "branch" => ("\u{f418}", "🌿"),            // nf-oct-git_branch
        "dirty" => ("\u{f040}", "✏️"),             // nf-fa-pencil
        "ahead" => ("\u{f062}", "⬆️"),             // nf-fa-arrow_up
        "worktree" => ("\u{f1bb}", "🌳"),          // nf-fa-tree
        "prs" => ("\u{f407}", "🔀"),               // nf-oct-git_pull_request
        "issues" => ("\u{f41b}", "🐛"),            // nf-oct-issue_opened
        "usage" if usage_7d => ("\u{f073}", "📅"), // nf-fa-calendar
        "usage" => ("\u{f017}", "⏳"),             // nf-fa-clock_o
        "account" => ("\u{f007}", "👤"),           // nf-fa-user
        "sessions" => ("\u{f120}", "💬"),          // nf-fa-terminal
        "spend" => ("\u{f155}", "💰"),             // nf-fa-dollar
        "spendhist" => ("\u{f0d6}", "💵"),         // nf-fa-money
        "tokens" => ("\u{f0ec}", "🔢"),            // nf-fa-exchange
        "cache" => ("\u{f1c0}", "💾"),             // nf-fa-database
        "cachettl" => ("\u{f253}", "🕐"),          // nf-fa-hourglass_end
        _ => return None,
    };
    match set {
        Icons::None => None,
        Icons::Nerdfont => Some(nf),
        Icons::Emoji => Some(emoji),
    }
}
