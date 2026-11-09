//! Segment icons: built-in glyph sets, the generic `icon=` option and how an
//! icon joins a rendered value.

use crate::conf::{Config, Icons};
use crate::options::Options;
use crate::placeholders::gh::Checks;
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
        "budget" => ("\u{f200}", "🎯"),            // nf-fa-pie_chart
        "extra" => ("\u{f055}", "💸"),             // nf-fa-plus_circle
        "tokens" => ("\u{f0ec}", "🔢"),            // nf-fa-exchange
        "cache" => ("\u{f1c0}", "💾"),             // nf-fa-database
        "cachettl" => ("\u{f253}", "🕐"),          // nf-fa-hourglass_end
        "misses" => ("\u{f127}", "🧊"),            // nf-fa-chain_broken
        "ttft" => ("\u{f251}", "⏱️"),              // nf-fa-hourglass_start
        "tps" => ("\u{f201}", "📈"),               // nf-fa-line_chart
        "agents" => ("\u{f0c0}", "👥"),            // nf-fa-users
        "cost" => ("\u{f09d}", "💳"),              // nf-fa-credit_card
        "burn" => ("\u{f06d}", "🔥"),              // nf-fa-fire
        "duration" => ("\u{f1da}", "⌚"),          // nf-fa-history
        "api" => ("\u{f254}", "📡"),               // nf-fa-hourglass
        "lines" => ("\u{f121}", "📝"),             // nf-fa-code
        "name" => ("\u{f02d}", "📛"),              // nf-fa-book
        "msgs" => ("\u{f075}", "✉️"),              // nf-fa-comment
        "modes" => ("\u{f085}", "🎛️"),             // nf-fa-cogs
        "version" => ("\u{f02b}", "🔖"),           // nf-fa-tag
        "style" => ("\u{f1fc}", "🎨"),             // nf-fa-paint_brush
        "vim" => ("\u{e7c5}", "⌨️"),               // nf-dev-vim
        "agent" => ("\u{f21b}", "🎭"),             // nf-fa-user_secret
        "thinking" => ("\u{f0eb}", "💭"),          // nf-fa-lightbulb_o
        "fast" => ("\u{f135}", "🚀"),              // nf-fa-rocket
        "cwd" => ("\u{f07c}", "📂"),               // nf-fa-folder_open
        "sid" => ("\u{f292}", "🆔"),               // nf-fa-hashtag
        "compactions" => ("\u{f066}", "🧹"),       // nf-fa-compress
        "skills" => ("\u{f0d0}", "✨"),            // nf-fa-magic
        "tmux" => ("\u{f0db}", "🪟"),              // nf-fa-columns
        "root" => ("\u{f015}", "🏠"),              // nf-fa-home
        "update" => ("\u{f0aa}", "🆙"),            // nf-fa-arrow_circle_up
        "status" => ("\u{f21e}", "🩺"),            // nf-fa-heartbeat
        _ => return None,
    };
    match set {
        Icons::None => None,
        Icons::Nerdfont => Some(nf),
        Icons::Emoji => Some(emoji),
    }
}

/// Mark drawn after a PR number for its CI state. Plain text without an
/// icon set, so marks show under `icons none` too.
pub fn check_mark(set: Icons, state: Checks) -> &'static str {
    // (plain, nerdfont, emoji)
    let (plain, nf, emoji) = match state {
        Checks::Fail => ("✗", "\u{f467}", "❌"),    // nf-oct-x
        Checks::Pending => ("•", "\u{f444}", "🟡"), // nf-oct-dot_fill
        Checks::Pass => ("✓", "\u{f42e}", "✅"),    // nf-oct-check
        Checks::None => return "",
    };
    match set {
        Icons::None => plain,
        Icons::Nerdfont => nf,
        Icons::Emoji => emoji,
    }
}
