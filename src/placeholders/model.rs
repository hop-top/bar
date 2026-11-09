use super::{Ctx, Placeholder};
use crate::click::{self, Term};
use crate::options::Options;
use crate::style::{dim, sanitize};
use serde_json::Value;
use std::path::Path;

pub struct Model;
pub struct Effort;

impl Placeholder for Model {
    fn render(&self, ctx: &Ctx, opts: &Options) -> Option<String> {
        model_with(ctx, opts, &click::state_dir(), || Term::current().clone())
    }
}

/// `state` holds the handler marker; `term` is probed only for a link.
pub fn model_with(
    ctx: &Ctx,
    opts: &Options,
    state: &Path,
    term: impl FnOnce() -> Term,
) -> Option<String> {
    let name = sanitize(&ctx.session.model);
    let name = name.strip_prefix("Claude ").unwrap_or(&name);
    let body = (!name.is_empty()).then(|| dim(name))?;
    let sid = &ctx.session.session_id;
    Some(click::with_cmd_link(body, "model", sid, opts, state, term))
}

/// `modelSettings[<model id>].effortLevel`, then global `effortLevel`.
fn settings_effort(model_id: &str) -> Option<String> {
    let v = crate::settings::user()?;
    let level = |v: Option<&Value>| {
        v.and_then(|l| l.as_str())
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
    };
    level(
        v.get("modelSettings")
            .and_then(|m| m.get(model_id))
            .and_then(|m| m.get("effortLevel")),
    )
    .or_else(|| level(v.get("effortLevel")))
}

impl Placeholder for Effort {
    fn render(&self, ctx: &Ctx, opts: &Options) -> Option<String> {
        effort_with(ctx, opts, &click::state_dir(), || Term::current().clone())
    }
}

pub fn effort_with(
    ctx: &Ctx,
    opts: &Options,
    state: &Path,
    term: impl FnOnce() -> Term,
) -> Option<String> {
    let level = if ctx.session.effort.is_empty() {
        settings_effort(&ctx.session.model_id)?
    } else {
        ctx.session.effort.clone()
    };
    let body = dim(&format!("[{}]", sanitize(&level)));
    let sid = &ctx.session.session_id;
    Some(click::with_cmd_link(body, "effort", sid, opts, state, term))
}
