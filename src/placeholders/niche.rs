//! `sid`, `compactions`, `skills`: small session facts. The first reads
//! the statusline JSON, the others the transcript scan.

use super::{Ctx, Placeholder};
use crate::options::Options;
use crate::style::{dim, sanitize};
use crate::transcript;

const DEFAULT_SID_LEN: usize = 8;

/// `abcdef01`: the first `len` chars of the session id.
pub struct Sid;
/// `compact 2`: context compactions so far; hidden at 0.
pub struct Compactions;
/// `skills 3`: skill calls so far; hidden at 0.
pub struct Skills;

impl Placeholder for Sid {
    fn render(&self, ctx: &Ctx, opts: &Options) -> Option<String> {
        let len = opts
            .get("len")
            .and_then(|l| l.trim().parse::<usize>().ok())
            .unwrap_or(DEFAULT_SID_LEN);
        let id: String = sanitize(&ctx.session.session_id)
            .chars()
            .take(len)
            .collect();
        (!id.is_empty()).then(|| dim(&id))
    }
}

fn counted(label: &str, n: u64, show_label: bool) -> Option<String> {
    match (n, show_label) {
        (0, _) => None,
        (n, true) => Some(format!("{} {n}", dim(label))),
        (n, false) => Some(n.to_string()),
    }
}

fn stats(ctx: &Ctx) -> Option<transcript::Stats> {
    transcript::load(ctx.cache?, &ctx.session.transcript_path)
}

impl Placeholder for Compactions {
    fn render(&self, ctx: &Ctx, opts: &Options) -> Option<String> {
        self.render_labeled(ctx, opts, true)
    }

    fn render_labeled(&self, ctx: &Ctx, _: &Options, label: bool) -> Option<String> {
        counted("compact", stats(ctx)?.compactions, label)
    }
}

impl Placeholder for Skills {
    fn render(&self, ctx: &Ctx, opts: &Options) -> Option<String> {
        self.render_labeled(ctx, opts, true)
    }

    fn render_labeled(&self, ctx: &Ctx, _: &Options, label: bool) -> Option<String> {
        counted("skills", stats(ctx)?.skills, label)
    }
}

/// Stand-in values for `bar preview`.
pub fn sample(name: &str, label: bool) -> Option<String> {
    match name {
        "compactions" => counted("compact", 2, label),
        "skills" => counted("skills", 3, label),
        _ => None,
    }
}
