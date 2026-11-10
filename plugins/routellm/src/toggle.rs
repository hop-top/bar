//! The toggle: one field-level change of the proxy's provider entry,
//! made only on an explicit action, never in a render.
//!
//! Sequence: read the entry under the proxy's lock (it must exist and
//! point at the configured RouteLLM) and do nothing when it already is in
//! the target state; for ON, release the lock and gate on RouteLLM being up
//! and serving every aliased tier (`force` overrides; OFF is never gated
//! and never contacts RouteLLM, and RouteLLM is never asked while a lock is
//! held); then, under the lock again, read the entry once more, send the
//! single `PATCH` and read it back until it shows the change. The proxy
//! answers before it applies the change, so the read-back waits a little
//! and tries a few times; when the change cannot be confirmed the cached
//! state is invalidated and the report says the state is unknown. A
//! refusal found before the `PATCH` sends nothing. An explicit toggle
//! ignores the 404 hold and the backoff of the collector, never a refused
//! key.

use crate::cache::{self, Change};
use crate::collect::{self, same_target};
use crate::config::Instance;
use crate::mgmt::{patch_body, Client, Item, MErr};
use bar_plugin::keyguard::{Guard, LOCK_WAIT};
use bar_plugin::secret::Secret;
use std::path::Path;
use std::time::Duration;

/// Pauses before each read-back of the entry after the `PATCH`.
const READBACK_PAUSES_MS: [u64; 3] = [150, 400, 900];

/// The longest a toggle can take, in seconds: both lock passes (the
/// proxy's lock and the instance's, each waited for), the two reads of the
/// entry, RouteLLM's health and models, the `PATCH`, the read-backs and
/// their pauses.
pub const WORST_SECS: u64 = 2 * 2 * LOCK_WAIT.as_secs()
    + (2 + 1 + READBACK_PAUSES_MS.len() as u64) * crate::mgmt::T_CALL
    + 2 * collect::T_PROBE
    + 2;

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

struct Call<'a> {
    r: &'a Req<'a>,
    g: Guard<'a>,
    client: Client<'a>,
}

impl Call<'_> {
    fn fail(&self, e: MErr, what: &str) -> Refusal {
        match e {
            MErr::Auth(s) => {
                let kept = self.g.stop(s, self.r.key, self.r.now);
                let more = if kept {
                    ""
                } else {
                    "; the refusal could not be recorded, so do not retry until the key is fixed"
                };
                Refusal(format!(
                    "{AUTH_STOP} ({s}); no calls until the key changes{more}"
                ))
            }
            MErr::NotFound => Refusal(format!(
                "{what}: the proxy's /v0/management route was not found (management off?)"
            )),
            MErr::Transient(e) => Refusal(format!("{what}: {e}")),
        }
    }

    /// The configured entry, as the proxy has it now.
    fn read(&self, what: &str) -> Result<Item, Refusal> {
        let inst = self.r.inst;
        let it = match self.client.entry(&inst.provider) {
            Ok(Some(it)) => it,
            Ok(None) => {
                return refuse(format!(
                    "the proxy has no `{}` provider entry; run `bar rtellm setup`",
                    inst.provider
                ))
            }
            Err(e) => return Err(self.fail(e, what)),
        };
        if !same_target(&it.base_url, &inst.routellm) {
            return refuse(format!(
                "the proxy's `{}` entry does not point at {}; refusing to change it",
                inst.provider, inst.routellm
            ));
        }
        Ok(it)
    }
}

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
    let call = Call {
        r,
        g,
        client: Client::new(&r.inst.management, r.key).map_err(Refusal)?,
    };
    let mut notes = Vec::new();
    if r.to_on {
        // pass one: is there anything to do? Then the gate, unlocked.
        let it = {
            let _lock = take(&call.g, &path)?;
            check_key(&call)?;
            let it = call.read("reading the provider entry")?;
            if !it.disabled {
                return Ok(vec!["already on".into()]);
            }
            it
        };
        notes = gate(r, &it);
        if !notes.is_empty() {
            if !r.force {
                return refuse(format!("not switching on: {}", notes.join("; ")));
            }
            notes = vec![format!("forced past: {}", notes.join("; "))];
        }
    }
    let _lock = take(&call.g, &path)?;
    let mut c = cache::load(&path);
    check_key(&call)?;
    let it = call.read("reading the provider entry")?;
    let now_on = !it.disabled;
    if now_on == r.to_on {
        return Ok(vec![format!("already {}", word(r.to_on))]);
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
    let sent = call.client.set(&r.inst.provider, !r.to_on, priority);
    let unconfirmed = |c: &mut cache::Cache, why: String| -> Refusal {
        // whatever the cache holds is no longer known to be true
        c.at = 0;
        let _ = cache::save_state(&path, c);
        Refusal(why)
    };
    let sent_err = match sent {
        Ok(()) => None,
        Err(MErr::Auth(s)) => return Err(call.fail(MErr::Auth(s), "change failed")),
        Err(MErr::NotFound) => {
            return Err(unconfirmed(
                &mut c,
                format!(
                    "change failed: the `{}` entry was not found; it may have been removed or renamed since it was read",
                    r.inst.provider
                ),
            ))
        }
        Err(MErr::Transient(e)) => Some(e),
    };
    // the proxy answers before the change is applied (and a reply may be
    // lost): compare by reading, a few times
    let mut last = String::new();
    let mut confirmed = false;
    for pause in READBACK_PAUSES_MS {
        std::thread::sleep(Duration::from_millis(pause));
        match call.client.entry(&r.inst.provider) {
            Ok(Some(v)) if v.disabled == !r.to_on => {
                confirmed = true;
                break;
            }
            Ok(_) => last = format!("the proxy still reports routing {}", word(now_on)),
            Err(MErr::Auth(s)) => return Err(call.fail(MErr::Auth(s), "verifying")),
            Err(MErr::NotFound) => last = "the entry could not be read back".into(),
            Err(MErr::Transient(e)) => last = format!("verifying: {e}"),
        }
    }
    if !confirmed {
        let why = match &sent_err {
            Some(e) => format!(
                "change failed: {e}; state unknown ({last}), re-checking on the next status"
            ),
            None => {
                format!("sent, but {last}; state not confirmed, re-checking on the next status")
            }
        };
        return Err(unconfirmed(&mut c, why));
    }
    if let Some(e) = sent_err {
        out.push(format!("the change request failed ({e}) but was applied"));
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
    c.fails = 0;
    c.retry_at = 0;
    c.off_until = 0;
    if !cache::save_state(&path, &c) {
        out.push("could not update the record of changes".into());
    }
    Ok(out)
}

fn take(g: &Guard, path: &Path) -> Result<bar_plugin::keyguard::Held, Refusal> {
    cache::lock(g, path).ok_or_else(|| Refusal("the proxy is being used by another bar process, or the state directory is not writable; try again".into()))
}

/// A refused key stops everything, hold or no hold.
fn check_key(call: &Call) -> Result<(), Refusal> {
    match call.g.refusal(call.r.key, call.r.now) {
        Some(st) => refuse(format!(
            "{AUTH_STOP} ({st}); no calls until the key changes"
        )),
        None => Ok(()),
    }
}

/// What stands in the way of switching on: RouteLLM not answering, or not
/// serving an aliased tier. Asks RouteLLM, so it runs with no lock held.
fn gate(r: &Req, it: &Item) -> Vec<String> {
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
    problems
}
