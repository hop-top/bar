use super::{Ctx, Placeholder};
use crate::options::Options;
use crate::style::{bold, fg, neutral, sanitize};
use serde_json::Value;
use std::path::PathBuf;

pub struct Account;

/// Glob with `*` only (any run of characters, possibly empty).
pub fn glob_match(pattern: &str, text: &str) -> bool {
    let (p, t): (Vec<char>, Vec<char>) = (pattern.chars().collect(), text.chars().collect());
    let (mut pi, mut ti) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while ti < t.len() {
        if pi < p.len() && p[pi] == '*' {
            star = Some((pi, ti));
            pi += 1;
        } else if pi < p.len() && p[pi] == t[ti] {
            pi += 1;
            ti += 1;
        } else if let Some((sp, st)) = star {
            pi = sp + 1;
            ti = st + 1;
            star = Some((sp, st + 1));
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|&c| c == '*')
}

fn email() -> Option<String> {
    let dir = match std::env::var_os("CLAUDE_CONFIG_DIR").filter(|v| !v.is_empty()) {
        Some(d) => PathBuf::from(d),
        None => crate::platform::home()?,
    };
    let v: Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join(".claude.json")).ok()?).ok()?;
    v.get("oauthAccount")?
        .get("emailAddress")?
        .as_str()
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

/// The label `set account-source` supplies: the plugin's cached
/// default-options `account`. Absent or cold, `None`.
pub fn source_account(ctx: &Ctx) -> Option<crate::plugin::Account> {
    let name = ctx.config.account_source.as_deref()?;
    crate::plugin::default_output(ctx, name)?.account.clone()
}

impl Placeholder for Account {
    fn render(&self, ctx: &Ctx, _opts: &Options) -> Option<String> {
        // a plugin's account stands in for the signed-in one; the same
        // `account "<glob>"` rules apply, matched against its label
        let source = source_account(ctx);
        // a plugin that matched the rules itself names the colour: its
        // label is drawn as is
        if let Some((label, color)) = source.as_ref().and_then(|a| Some((&a.label, a.color?))) {
            return Some(bold(&fg(color, &sanitize(label))));
        }
        let shown = source.map(|a| a.label).or_else(email);
        let Some(email) = shown else {
            let key = std::env::var("ANTHROPIC_API_KEY")
                .ok()
                .filter(|k| !k.is_empty());
            return key.map(|_| bold(&fg(neutral(), "api-key")));
        };
        let (label, color) = ctx
            .config
            .accounts
            .iter()
            .find(|r| glob_match(&r.glob, &email))
            .map(|r| (r.label.clone(), r.color))
            .unwrap_or((email, neutral()));
        Some(bold(&fg(color, &sanitize(&label))))
    }
}
