//! Placeholder trait, shared context and the name registry.

pub mod account;
pub mod ctx;
pub mod gh;
pub mod model;
pub mod repo;
pub mod usage;

use crate::cache::Cache;
use crate::conf::Config;
use crate::expand::CommandRunner;
use crate::options::Options;
use crate::session::Session;

pub struct Ctx<'a> {
    pub session: &'a Session,
    pub config: &'a Config,
    /// Unix seconds.
    pub now: i64,
    pub runner: &'a dyn CommandRunner,
    /// Background-refresh cache for `gh` data; `None` renders it empty.
    pub cache: Option<&'a Cache>,
}

pub trait Placeholder {
    /// Styled text; `None` or empty when there is nothing to show.
    fn render(&self, ctx: &Ctx, opts: &Options) -> Option<String>;
}

/// `None` for names not implemented (yet): they render as empty.
pub fn lookup(name: &str) -> Option<&'static dyn Placeholder> {
    Some(match name {
        "account" => &account::Account,
        "repo" => &repo::Repo,
        "branch" => &repo::Branch,
        "model" => &model::Model,
        "effort" => &model::Effort,
        "ctx" => &ctx::CtxUsed,
        "usage" => &usage::Usage,
        "prs" => &gh::Prs,
        "issues" => &gh::Issues,
        _ => return None,
    })
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
