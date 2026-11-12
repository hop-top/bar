use super::{Ctx, Placeholder};
use crate::color::Color;
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

/// Whether every text `narrow` matches, `wide` matches too (both globs
/// as [`glob_match`] reads them). Exact: `wide` matching `narrow` itself,
/// whose `*` only a `*` of `wide` can take, is both needed and enough,
/// since a text can put characters neither glob holds where `narrow` has
/// its `*`s.
pub fn glob_covers(wide: &str, narrow: &str) -> bool {
    glob_match(wide, narrow)
}

/// The signed-in account's email, from Claude Code's `.claude.json`.
pub fn email() -> Option<String> {
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

/// The session's effective account, as `#{account}` resolves it: the
/// `set account-source` plugin's account, else the signed-in one.
#[derive(Debug, Clone, PartialEq)]
pub struct Resolved {
    /// The signed-in account's email; `None` when a plugin names the
    /// account (it hands over a label, not an address).
    pub email: Option<String>,
    /// What `#{account}` shows: the matching rule's label, a plugin's
    /// coloured label, else what the account was named by.
    pub shown: String,
    /// The label of the first matching `account` line, or a plugin's
    /// label that came with a colour (it matched the rules itself).
    pub label: Option<String>,
    pub color: Color,
}

fn signed_in(ctx: &Ctx) -> Option<String> {
    ctx.memo.email.get_or_init(email).clone()
}

/// `None` when bar knows no account (an API key, or nothing signed in).
pub fn resolve(ctx: &Ctx) -> Option<Resolved> {
    // a plugin's account stands in for the signed-in one; the same
    // `account "<glob>"` rules apply, matched against its label
    let source = source_account(ctx);
    // a plugin that matched the rules itself names the colour: its
    // label is drawn as is
    if let Some((label, color)) = source.as_ref().and_then(|a| Some((&a.label, a.color?))) {
        return Some(Resolved {
            email: None,
            shown: label.clone(),
            label: Some(label.clone()),
            color,
        });
    }
    let (email, name) = match source {
        Some(a) => (None, a.label),
        None => {
            let e = signed_in(ctx)?;
            (Some(e.clone()), e)
        }
    };
    let rule = ctx
        .config
        .accounts
        .iter()
        .find(|r| glob_match(&r.glob, &name));
    Some(Resolved {
        email,
        shown: rule.map_or_else(|| name.clone(), |r| r.label.clone()),
        label: rule.map(|r| r.label.clone()),
        color: rule.map_or_else(neutral, |r| r.color),
    })
}

impl Placeholder for Account {
    fn render(&self, ctx: &Ctx, _opts: &Options) -> Option<String> {
        let Some(a) = resolve(ctx) else {
            let key = std::env::var("ANTHROPIC_API_KEY")
                .ok()
                .filter(|k| !k.is_empty());
            return key.map(|_| bold(&fg(neutral(), "api-key")));
        };
        Some(bold(&fg(a.color, &sanitize(&a.shown))))
    }
}
