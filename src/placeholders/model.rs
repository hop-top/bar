use super::{Ctx, Placeholder};
use crate::options::Options;
use crate::style::{dim, sanitize};
use serde_json::Value;
use std::path::PathBuf;

pub struct Model;
pub struct Effort;

impl Placeholder for Model {
    fn render(&self, ctx: &Ctx, _opts: &Options) -> Option<String> {
        let name = sanitize(&ctx.session.model);
        let name = name.strip_prefix("Claude ").unwrap_or(&name);
        (!name.is_empty()).then(|| dim(name))
    }
}

fn settings_path() -> Option<PathBuf> {
    match std::env::var_os("CLAUDE_CONFIG_DIR").filter(|v| !v.is_empty()) {
        Some(d) => Some(PathBuf::from(d).join("settings.json")),
        None => Some(PathBuf::from(std::env::var_os("HOME")?).join(".claude/settings.json")),
    }
}

/// `modelSettings[<model id>].effortLevel`, then global `effortLevel`.
fn settings_effort(model_id: &str) -> Option<String> {
    let v: Value = serde_json::from_str(&std::fs::read_to_string(settings_path()?).ok()?).ok()?;
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
    fn render(&self, ctx: &Ctx, _opts: &Options) -> Option<String> {
        let level = if ctx.session.effort.is_empty() {
            settings_effort(&ctx.session.model_id)?
        } else {
            ctx.session.effort.clone()
        };
        Some(dim(&format!("[{}]", sanitize(&level))))
    }
}
