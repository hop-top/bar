//! The toggle: one field-level change of the proxy's provider entry,
//! made only on an explicit action, never in a render.
//!
//! Sequence, under the proxy's lock: read the entry (it must exist and
//! point at the configured RouteLLM), do nothing when it already is in the
//! target state, gate ON on RouteLLM being up and serving every aliased
//! tier (`force` overrides; OFF is never gated and never contacts
//! RouteLLM), send the single `PATCH`, read the entry again and compare.
//! A refusal found before the `PATCH` sends nothing.

use crate::cache::{self, Change};
use crate::collect::{self, same_target};
use crate::config::Instance;
use crate::mgmt::{patch_body, Client, MErr};
use bar_plugin::keyguard::Guard;
use bar_plugin::secret::Secret;
use std::path::Path;

/// Why nothing was changed (or why the change could not be confirmed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal(pub String);

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

fn refuse<T>(s: impl Into<String>) -> Result<T, Refusal> {
    Err(Refusal(s.into()))
}

pub struct Req<'a> {
    pub inst: &'a Instance,
    pub key: &'a Secret,
    pub state_dir: &'a Path,
    pub now: i64,
    /// `true`: routing on.
    pub to_on: bool,
    pub dry_run: bool,
    pub force: bool,
}

fn word(on: bool) -> &'static str {
    if on {
        "on"
    } else {
        "off"
    }
}

const AUTH_STOP: &str = "management refused the key";

/// Switches routing; the lines say what was (or, with `dry_run`, would be)
/// done.
pub fn run(r: &Req) -> Result<Vec<String>, Refusal> {
    if r.key.as_bytes().is_empty() {
        return refuse("no key");
    }
    let g = Guard {
        state_dir: r.state_dir,
        mgmt: &r.inst.management,
    };
    let path = cache::file(r.state_dir, &r.inst.origin.to_string());
    let Some(_lock) = cache::lock(&g, &path) else {
        return refuse("the proxy is being used by another bar process, or the state directory is not writable; try again");
    };
    let mut c = cache::load(&path);
    if let Some(st) = g.refusal(r.key, r.now) {
        return refuse(format!(
            "{AUTH_STOP} ({st}); no calls until the key changes"
        ));
    }
    if r.now < c.off_until {
        return refuse("management API is off (404); enable it in the proxy's config");
    }
    let client = Client::new(&r.inst.management, r.key).map_err(Refusal)?;
    let fail = |e: MErr, what: &str| -> Refusal {
        match e {
            MErr::Auth(s) => {
                g.stop(s, r.key, r.now);
                Refusal(format!("{AUTH_STOP} ({s}); no calls until the key changes"))
            }
            MErr::NotFound => Refusal(format!(
                "{what}: the proxy's /v0/management route was not found (management off?)"
            )),
            MErr::Transient(e) => Refusal(format!("{what}: {e}")),
        }
    };
    let it = match client.entry(&r.inst.provider) {
        Ok(Some(it)) => it,
        Ok(None) => {
            return refuse(format!(
                "the proxy has no `{}` provider entry; run `bar rl setup`",
                r.inst.provider
            ))
        }
        Err(e) => return Err(fail(e, "reading the provider entry")),
    };
    if !same_target(&it.base_url, &r.inst.routellm) {
        return refuse(format!(
            "the proxy's `{}` entry does not point at {}; refusing to change it",
            r.inst.provider, r.inst.routellm
        ));
    }
    let now_on = !it.disabled;
    if now_on == r.to_on {
        return Ok(vec![format!("already {}", word(r.to_on))]);
    }
    // the ON gate
    let mut notes = Vec::new();
    if r.to_on {
        let mut problems = Vec::new();
        if !collect::health(&r.inst.routellm) {
            problems.push(format!("RouteLLM at {} is not answering", r.inst.routellm));
        } else if let Some(tiers) = collect::tiers(&r.inst.routellm) {
            for a in &it.aliases {
                if !tiers.iter().any(|t| t.id == a.tier) {
                    problems.push(format!("RouteLLM does not serve tier `{}`", a.tier));
                }
            }
        } else {
            problems.push("RouteLLM did not list its models".into());
        }
        if !problems.is_empty() {
            if r.force {
                notes.push(format!("forced past: {}", problems.join("; ")));
            } else {
                return refuse(format!("not switching on: {}", problems.join("; ")));
            }
        }
    }
    let priority = (r.to_on && it.priority < r.inst.on_priority).then_some(r.inst.on_priority);
    let body = patch_body(&r.inst.provider, !r.to_on, priority);
    let dry = if r.dry_run { "dry run: " } else { "" };
    let mut out = notes;
    out.push(format!(
        "{dry}PATCH {}/openai-compatibility {body}",
        r.inst.management
    ));
    if r.dry_run {
        return Ok(out);
    }
    client
        .set(&r.inst.provider, !r.to_on, priority)
        .map_err(|e| fail(e, "change failed"))?;
    // the proxy answers before the change is applied: compare by reading
    match client.entry(&r.inst.provider) {
        Ok(Some(v)) if v.disabled == !r.to_on => {}
        Ok(_) => {
            return refuse(format!(
                "sent, but the proxy still reports routing {}; check `bar rl status`",
                word(now_on)
            ))
        }
        Err(e) => return Err(fail(e, "verifying")),
    }
    out.push(format!("routing {}", word(r.to_on)));
    c.last = Some(Change {
        at: r.now,
        from: word(now_on).into(),
        to: word(r.to_on).into(),
        version: crate::VERSION.into(),
    });
    if let Some(e) = &mut c.entry {
        e.disabled = !r.to_on;
        if let Some(p) = priority {
            e.priority = p;
        }
    }
    c.at = r.now;
    if !cache::save_state(&path, &c) {
        out.push("could not update the record of changes".into());
    }
    Ok(out)
}
