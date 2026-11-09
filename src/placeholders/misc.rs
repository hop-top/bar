use super::{Ctx, Placeholder};
use crate::options::Options;
use crate::style::{dim, sanitize};

fn text(s: &str) -> Option<String> {
    let s = sanitize(s);
    (!s.is_empty()).then(|| dim(&s))
}

/// Session name; the custom name or the generated title.
pub struct Name;
/// Claude Code version.
pub struct Version;
/// Output style name; hidden for `default`.
pub struct Style;
/// Vim mode (`NORMAL`, `INSERT`, ...); hidden when vim mode is off.
pub struct Vim;
/// Agent name; hidden when none.
pub struct Agent;
/// `think` while extended thinking is enabled.
pub struct Thinking;
/// `fast` while fast mode is enabled.
pub struct Fast;

impl Placeholder for Name {
    fn render(&self, ctx: &Ctx, opts: &Options) -> Option<String> {
        let s = sanitize(&ctx.session.name);
        (!s.is_empty()).then(|| dim(&super::truncate(s, opts)))
    }
}

impl Placeholder for Version {
    fn render(&self, ctx: &Ctx, _: &Options) -> Option<String> {
        text(&ctx.session.version)
    }
}

impl Placeholder for Style {
    fn render(&self, ctx: &Ctx, _: &Options) -> Option<String> {
        let s = sanitize(&ctx.session.output_style);
        if s.eq_ignore_ascii_case("default") {
            return None;
        }
        text(&s)
    }
}

impl Placeholder for Vim {
    fn render(&self, ctx: &Ctx, _: &Options) -> Option<String> {
        text(&ctx.session.vim_mode)
    }
}

impl Placeholder for Agent {
    fn render(&self, ctx: &Ctx, _: &Options) -> Option<String> {
        text(&ctx.session.agent)
    }
}

impl Placeholder for Thinking {
    fn render(&self, ctx: &Ctx, _: &Options) -> Option<String> {
        ctx.session.thinking.then(|| dim("think"))
    }
}

impl Placeholder for Fast {
    fn render(&self, ctx: &Ctx, _: &Options) -> Option<String> {
        ctx.session.fast_mode.then(|| dim("fast"))
    }
}
