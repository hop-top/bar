//! Placeholder trait, shared context and the name registry.

pub mod account;
pub mod agents;
pub mod cost;
pub mod ctx;
pub mod dirs;
pub mod gh;
pub mod gitst;
pub mod misc;
pub mod model;
pub mod modes;
pub mod niche;
pub mod repo;
pub mod sessions;
pub mod speed;
pub mod spend;
pub mod spendhist;
pub mod tmux;
pub mod tokens;
pub mod usage;

use crate::cache::Cache;
use crate::conf::Config;
use crate::expand::CommandRunner;
use crate::options::Options;
use crate::session::Session;
use std::path::PathBuf;

pub struct Ctx<'a> {
    pub session: &'a Session,
    pub config: &'a Config,
    /// Unix seconds.
    pub now: i64,
    pub runner: &'a dyn CommandRunner,
    /// Background-refresh cache for `gh` data; `None` renders it empty.
    pub cache: Option<&'a Cache>,
    /// `COLUMNS`, handed to plugins.
    pub columns: Option<usize>,
    /// Where `bar-<name>` plugins are looked up, in order.
    pub plugin_dirs: &'a [PathBuf],
}

pub trait Placeholder {
    /// Styled text; `None` or empty when there is nothing to show.
    fn render(&self, ctx: &Ctx, opts: &Options) -> Option<String>;

    /// As `render`; `label` false leaves out the text label (an icon takes
    /// its place). Placeholders without a label ignore it.
    fn render_labeled(&self, ctx: &Ctx, opts: &Options, label: bool) -> Option<String> {
        let _ = label;
        self.render(ctx, opts)
    }
}

/// Every core placeholder by name; anything else is a plugin.
static REGISTRY: &[(&str, &(dyn Placeholder + Sync))] = &[
    ("account", &account::Account),
    ("repo", &repo::Repo),
    ("branch", &repo::Branch),
    ("model", &model::Model),
    ("effort", &model::Effort),
    ("agents", &agents::Agents),
    ("ctx", &ctx::CtxUsed),
    ("usage", &usage::Usage),
    ("sessions", &sessions::Sessions),
    ("spend", &spend::Spend),
    ("spendhist", &spendhist::SpendHist),
    ("tokens", &tokens::Tokens),
    ("cache", &tokens::CacheStats),
    ("cachettl", &tokens::CacheTtl),
    ("misses", &speed::Misses),
    ("ttft", &speed::Ttft),
    ("tps", &speed::Tps),
    ("cost", &cost::Cost),
    ("burn", &cost::Burn),
    ("duration", &cost::Duration),
    ("api", &cost::Api),
    ("lines", &cost::Lines),
    ("name", &misc::Name),
    ("modes", &modes::Modes),
    ("version", &misc::Version),
    ("style", &misc::Style),
    ("vim", &misc::Vim),
    ("agent", &misc::Agent),
    ("thinking", &misc::Thinking),
    ("fast", &misc::Fast),
    ("cwd", &dirs::Cwd),
    ("root", &dirs::Root),
    ("prs", &gh::Prs),
    ("issues", &gh::Issues),
    ("dirty", &gitst::Dirty),
    ("ahead", &gitst::Ahead),
    ("worktree", &gitst::Worktree),
    ("sid", &niche::Sid),
    ("compactions", &niche::Compactions),
    ("skills", &niche::Skills),
    ("msgs", &niche::Msgs),
    ("tmux", &tmux::Tmux),
];

/// Names of all core placeholders, in registry order.
pub fn names() -> impl Iterator<Item = &'static str> {
    REGISTRY.iter().map(|(n, _)| *n)
}

/// `None` for names that are not core placeholders.
pub fn lookup(name: &str) -> Option<&'static dyn Placeholder> {
    REGISTRY
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, p)| *p as &'static dyn Placeholder)
}

/// The session's directory when it exists, else the process's.
pub(crate) fn work_dir(ctx: &Ctx) -> PathBuf {
    let cwd = PathBuf::from(&ctx.session.cwd);
    if !ctx.session.cwd.is_empty() && cwd.is_dir() {
        cwd
    } else {
        std::env::current_dir().unwrap_or_default()
    }
}

/// Truncates to `max` chars (option `max`) with an ellipsis.
pub fn truncate(text: String, opts: &Options) -> String {
    let Some(max) = opts.get("max").and_then(|m| m.parse::<usize>().ok()) else {
        return text;
    };
    if max == 0 || text.chars().count() <= max {
        return text;
    }
    let mut out: String = text.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// Option `link` is on unless set to `false`, `no`, `off` or `0`.
pub fn link_enabled(opts: &Options) -> bool {
    !matches!(opts.get("link"), Some("false" | "no" | "off" | "0"))
}
