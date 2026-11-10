//! Account switching: `list`, `use <account>` and the `use` action.
//!
//! "Use X" raises X's routing priority above every other available
//! account of its provider (the proxy serves the highest available
//! priority tier). Accounts are never disabled, nothing is lowered, and
//! the previous priorities are kept in the plugin's private state so
//! `--reset` can restore them. The proxy applies a new priority to new
//! selections only: a session already bound to an account stays on it
//! until that account fails over.
//!
//! Failure handling is the collector's: a management 401 or 403 stops
//! every call until the key changes.

use crate::cache::{self, Cache};
use crate::collect::{self, valid_index, AUTH_HOLD};
use crate::config::{Config, Instance};
use crate::mgmt::{Api, Client, MErr};
use crate::model;
use bar_plugin::fsio;
use bar_plugin::keysource;
use bar_plugin::proto::ActionSpec;
use bar_plugin::secret::Secret;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Priorities above this are refused (the proxy keeps an `int`).
pub const MAX_PRIORITY: i64 = 1_000_000;
/// Largest switch-state file read.
pub const MAX_STATE: usize = 16 * 1024;
/// Recorded bumps kept per instance.
pub const MAX_BUMPS: usize = 64;

/// The actions `--ext-info` declares.
pub fn actions() -> BTreeMap<String, ActionSpec> {
    BTreeMap::from([(
        "use".to_string(),
        ActionSpec {
            description: "Prefer this account".into(),
            tokens: vec!["account".into(), "provider".into()],
            confirm: false,
        },
    )])
}

/// One credential of the listing, as far as switching needs it. The
/// `name` is the proxy's id for the credential (a file name): it goes
/// back to the proxy and is never printed.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    /// 1-based position among the provider's enabled accounts, as
    /// `label=index` counts them; 0 for a disabled account.
    pub n: usize,
    pub index: String,
    pub name: String,
    pub provider: String,
    pub label: String,
    pub priority: i64,
    pub disabled: bool,
    pub unavailable: bool,
}

impl Entry {
    fn status(&self) -> &'static str {
        if self.disabled {
            "disabled"
        } else if self.unavailable {
            "unavailable"
        } else {
            "active"
        }
    }
}

fn valid_name(s: &str) -> bool {
    (1..=256).contains(&s.len()) && !s.chars().any(char::is_control)
}

/// The listing's credentials of the instance's providers, in order.
pub fn parse_entries(inst: &Instance, v: &Value) -> Vec<Entry> {
    let files = v
        .get("files")
        .and_then(Value::as_array)
        .or_else(|| v.as_array());
    let mut out = Vec::new();
    let mut count: BTreeMap<String, usize> = BTreeMap::new();
    for (i, f) in files.into_iter().flatten().enumerate() {
        let provider = f["type"]
            .as_str()
            .or_else(|| f["provider"].as_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let index = f["auth_index"].as_str().unwrap_or("");
        let name = f["name"]
            .as_str()
            .or_else(|| f["id"].as_str())
            .unwrap_or("");
        if !inst.providers.contains(&provider) || !valid_index(index) || !valid_name(name) {
            continue;
        }
        let disabled = f["disabled"].as_bool().unwrap_or(false);
        let n = if disabled {
            0
        } else {
            let c = count.entry(provider.clone()).or_default();
            *c += 1;
            *c
        };
        let email = ["email", "label", "account"]
            .iter()
            .find_map(|k| f[*k].as_str())
            .unwrap_or("");
        out.push(Entry {
            n,
            index: index.into(),
            name: name.into(),
            provider,
            label: model::label_for(email, &inst.aliases, i + 1),
            priority: f["priority"].as_i64().unwrap_or(0),
            disabled,
            unavailable: f["unavailable"].as_bool().unwrap_or(false),
        });
    }
    out
}

/// Why nothing was changed.
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

/// The one account `arg` names: an `auth_index`, a label (an alias or the
/// email's local part, any case), or a position (`3` or `#3`) among the
/// provider's enabled accounts, as `label=index` draws it (a position
/// needs `provider` when the instance reads several). Several matches
/// refuse, naming them by label and provider.
pub fn resolve<'a>(
    list: &'a [Entry],
    arg: &str,
    provider: Option<&str>,
) -> Result<&'a Entry, Refusal> {
    let of_provider = |e: &&Entry| provider.is_none_or(|p| e.provider == p);
    let pool: Vec<&Entry> = list.iter().filter(of_provider).collect();
    let exact: Vec<&Entry> = pool.iter().copied().filter(|e| e.index == arg).collect();
    let pos = arg.strip_prefix('#').unwrap_or(arg).parse::<usize>().ok();
    let found: Vec<&Entry> = if !exact.is_empty() {
        exact
    } else if let Some(n) = pos.filter(|n| *n > 0) {
        let mut providers: Vec<&str> = pool
            .iter()
            .filter(|e| !e.disabled)
            .map(|e| e.provider.as_str())
            .collect();
        providers.dedup();
        providers.sort_unstable();
        providers.dedup();
        if providers.len() > 1 {
            return refuse(format!(
                "position {n} is per provider; add --provider ({})",
                providers.join(" or ")
            ));
        }
        pool.iter().copied().filter(|e| e.n == n).collect()
    } else {
        pool.iter()
            .copied()
            .filter(|e| e.label.eq_ignore_ascii_case(arg))
            .collect()
    };
    match found.as_slice() {
        [] => refuse(format!("no account `{arg}`")),
        [one] => Ok(one),
        many => {
            let names: Vec<String> = many
                .iter()
                .map(|e| format!("{} ({})", e.label, e.provider))
                .collect();
            refuse(format!(
                "`{arg}` is ambiguous: {}; name one by its provider (--provider) or index",
                names.join(", ")
            ))
        }
    }
}

/// One planned priority change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub name: String,
    pub index: String,
    pub provider: String,
    pub label: String,
    pub from: i64,
    pub to: i64,
}

/// What `use` would do for `target`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Plan {
    Set(Change),
    /// Already above every other enabled account of its provider.
    Already,
    /// Nothing else of its provider is enabled to compete with.
    Alone,
}

/// Priority one above every other enabled account of the provider: an
/// account only cooling down returns after the cooldown, and must not
/// outrank the one asked for.
pub fn plan_use(list: &[Entry], target: &Entry) -> Result<Plan, Refusal> {
    if target.disabled {
        return refuse(format!(
            "{} is disabled; bar never enables an account",
            target.label
        ));
    }
    let others: Vec<i64> = list
        .iter()
        .filter(|e| e.provider == target.provider && e.index != target.index && !e.disabled)
        .map(|e| e.priority)
        .collect();
    let Some(top) = others.iter().max().copied() else {
        return Ok(Plan::Alone);
    };
    if target.priority > top {
        return Ok(Plan::Already);
    }
    if top >= MAX_PRIORITY {
        return refuse("priorities are at the limit; lower one in the proxy first");
    }
    Ok(Plan::Set(Change {
        name: target.name.clone(),
        index: target.index.clone(),
        provider: target.provider.clone(),
        label: target.label.clone(),
        from: target.priority,
        to: top + 1,
    }))
}

/// A priority this plugin raised, and what it was before.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bump {
    pub index: String,
    pub provider: String,
    pub prev: i64,
    pub set: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct State {
    #[serde(default)]
    pub bumps: Vec<Bump>,
}

/// `<hash>.switch.json` next to the instance's cache: 0600, no email,
/// name, path or key.
pub fn state_file(state_dir: &Path, origin: &str) -> PathBuf {
    cache::file(state_dir, origin).with_extension("switch.json")
}

pub fn load_state(path: &Path) -> State {
    std::fs::File::open(path)
        .ok()
        .and_then(|f| fsio::read_capped(f, MAX_STATE).ok())
        .and_then(|raw| serde_json::from_slice(&raw).ok())
        .unwrap_or_default()
}

fn save_state(path: &Path, s: &State) -> std::io::Result<()> {
    if let Some(d) = path.parent() {
        fsio::private_dir(d)?;
    }
    let json = serde_json::to_vec(s).map_err(std::io::Error::other)?;
    fsio::write_atomic_private(path, &json)
}

/// What a reset would restore: bumps still in force, and those the user
/// has since changed (left alone, forgotten).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResetPlan {
    pub restore: Vec<Change>,
    pub skipped: Vec<String>,
    /// Recorded bumps for accounts that are gone: dropped.
    pub gone: usize,
}

/// Restores the recorded priorities of `only` (every one when `None`).
pub fn plan_reset(list: &[Entry], state: &State, only: Option<&Entry>) -> ResetPlan {
    let mut p = ResetPlan {
        restore: vec![],
        skipped: vec![],
        gone: 0,
    };
    // newest first: the oldest record of an account wins
    for b in state.bumps.iter().rev() {
        if only.is_some_and(|o| o.index != b.index || o.provider != b.provider) {
            continue;
        }
        let Some(e) = list
            .iter()
            .find(|e| e.index == b.index && e.provider == b.provider)
        else {
            p.gone += 1;
            continue;
        };
        if e.priority != b.set {
            p.skipped.push(e.label.clone());
            continue;
        }
        p.restore.push(Change {
            name: e.name.clone(),
            index: e.index.clone(),
            provider: e.provider.clone(),
            label: e.label.clone(),
            from: e.priority,
            to: b.prev,
        });
    }
    p
}

/// A connected instance: its listing, and the cache that holds the
/// failure state.
pub struct Live<'a> {
    pub inst: &'a Instance,
    key: &'a Secret,
    state_dir: &'a Path,
    now: i64,
    client: Client<'a>,
    pub entries: Vec<Entry>,
    /// Held while the instance is used: no other process calls it.
    _lock: fsio::Lock,
}

/// Lists the instance's credentials. Makes no call when the cache says
/// the key was refused or management is off; a 401 or 403 is recorded
/// so nothing else (the collector included) calls until the key changes.
pub fn connect<'a>(
    inst: &'a Instance,
    key: &'a Secret,
    state_dir: &'a Path,
    now: i64,
) -> Result<Live<'a>, Refusal> {
    if key.as_bytes().is_empty() {
        return refuse("no key");
    }
    let path = cache::file(state_dir, &inst.origin.to_string());
    let Some(lock) = cache::lock(&path) else {
        return refuse("the proxy is being used by another bar process, or the state directory is not writable; try again");
    };
    let mut c: Cache = cache::load(&path);
    if let Some(st) = c.refusal(key, AUTH_HOLD, now) {
        return refuse(format!(
            "management refused the key ({st}); no calls until the key changes"
        ));
    }
    if now < c.off_until {
        return refuse("management API is off (404); enable it in the proxy's config");
    }
    let first = c
        .api
        .as_deref()
        .map_or_else(|| Client::api_of(&inst.management), Api::from_name);
    for api in [first, if first == Api::V8 { Api::V0 } else { Api::V8 }] {
        let base = Client::base_for(&inst.management, api);
        let client = Client::new(&base, key).map_err(Refusal)?;
        match client.credentials() {
            Ok(v) => {
                return Ok(Live {
                    inst,
                    key,
                    state_dir,
                    now,
                    client,
                    entries: parse_entries(inst, &v),
                    _lock: lock,
                })
            }
            Err(MErr::NotFound) => continue,
            Err(MErr::Auth(s)) => {
                collect::stop(&path, &mut c, s, key, now);
                return refuse(format!(
                    "management refused the key ({s}); no calls until the key changes"
                ));
            }
            Err(MErr::Transient(e)) => return refuse(format!("management call failed: {e}")),
        }
    }
    refuse("the proxy's management API was not found")
}

impl Live<'_> {
    fn state_path(&self) -> PathBuf {
        state_file(self.state_dir, &self.inst.origin.to_string())
    }

    /// Applies one change; a 401 or 403 is recorded like any other.
    fn patch(&self, c: &Change) -> Result<(), Refusal> {
        match self.client.set_priority(&c.name, c.to) {
            Ok(()) => Ok(()),
            Err(MErr::Auth(s)) => {
                let path = cache::file(self.state_dir, &self.inst.origin.to_string());
                let mut ch = cache::load(&path);
                collect::stop(&path, &mut ch, s, self.key, self.now);
                refuse(format!(
                    "management refused the key ({s}); no calls until the key changes"
                ))
            }
            Err(MErr::NotFound) => refuse(format!("{}: the proxy no longer has it", c.label)),
            Err(MErr::Transient(e)) => refuse(format!("{}: change failed: {e}", c.label)),
        }
    }
}

fn line(c: &Change) -> String {
    format!(
        "{} ({}): priority {} -> {}",
        c.label, c.provider, c.from, c.to
    )
}

/// `use <account>` or `use --reset [account]`: what was (or, with
/// `dry_run`, would be) done, one line per change. `Err` refuses; nothing
/// is sent for a refusal found while planning.
pub fn run_use(
    live: &Live,
    account: Option<&str>,
    provider: Option<&str>,
    reset: bool,
    dry_run: bool,
) -> Result<Vec<String>, Refusal> {
    let target = match account {
        Some(a) => Some(resolve(&live.entries, a, provider)?),
        None if reset => None,
        None => return refuse("name an account"),
    };
    let path = live.state_path();
    let mut state = load_state(&path);
    let mut out = Vec::new();
    let dry = if dry_run { "dry run: " } else { "" };

    if reset {
        let p = plan_reset(&live.entries, &state, target);
        for c in &p.restore {
            out.push(format!("{dry}{}", line(c)));
        }
        for l in &p.skipped {
            out.push(format!("{l}: changed since; left as it is"));
        }
        if out.is_empty() && p.gone == 0 {
            out.push("nothing to reset".into());
        }
        if dry_run {
            return Ok(out);
        }
        for c in &p.restore {
            live.patch(c)?;
        }
        // forget what was restored, what the user changed, what is gone
        state.bumps.retain(|b| {
            let mine = target.is_none_or(|o| o.index == b.index && o.provider == b.provider);
            !mine
        });
        if save_state(&path, &state).is_err() {
            out.push("could not update the record of changes".into());
        }
        return Ok(out);
    }

    let target = target.expect("an account is named");
    match plan_use(&live.entries, target)? {
        Plan::Already => out.push(format!(
            "{} ({}): already preferred (priority {})",
            target.label, target.provider, target.priority
        )),
        Plan::Alone => out.push(format!(
            "{} ({}): no other enabled {} account; nothing to change",
            target.label, target.provider, target.provider
        )),
        Plan::Set(c) => {
            out.push(format!("{dry}{}", line(&c)));
            if target.unavailable {
                out.push(format!(
                    "{}: unavailable now; it is served once it recovers",
                    c.label
                ));
            }
            if !dry_run {
                live.patch(&c)?;
                // the oldest record of an account is what a reset restores,
                // unless the user changed the priority by hand since
                match state
                    .bumps
                    .iter_mut()
                    .find(|b| b.index == c.index && b.provider == c.provider)
                {
                    Some(b) if b.set == c.from => b.set = c.to,
                    Some(b) => (b.prev, b.set) = (c.from, c.to),
                    None => state.bumps.push(Bump {
                        index: c.index.clone(),
                        provider: c.provider.clone(),
                        prev: c.from,
                        set: c.to,
                    }),
                }
                state.bumps.truncate(MAX_BUMPS);
                if save_state(&path, &state).is_err() {
                    out.push(
                        "could not record the previous priority; --reset cannot restore it".into(),
                    );
                }
            }
        }
    }
    Ok(out)
}

/// `list`: one line per account, columns of label, provider, priority,
/// status, `auth_index`. Never an email.
pub fn run_list(live: &Live, provider: Option<&str>) -> Vec<String> {
    let rows: Vec<&Entry> = live
        .entries
        .iter()
        .filter(|e| provider.is_none_or(|p| e.provider == p))
        .collect();
    let w = rows.iter().map(|e| e.label.len()).max().unwrap_or(0);
    rows.iter()
        .map(|e| {
            format!(
                "{:<w$}  {:<7}  {:>3}  {:<11}  {}",
                e.label,
                e.provider,
                e.priority,
                e.status(),
                e.index
            )
        })
        .collect()
}

/// The instance a command talks to: `--instance`, else the session's
/// `ANTHROPIC_BASE_URL`, else the only configured one, else the only one
/// whose cached accounts include `known` (an `auth_index` and provider).
pub fn pick_instance<'a>(
    cfg: &'a Config,
    explicit: Option<&str>,
    env_url: Option<&str>,
    state_dir: &Path,
    known: Option<(&str, &str)>,
) -> Result<&'a Instance, Refusal> {
    use bar_plugin::url::Origin;
    if let Some(u) = explicit.or(env_url).filter(|u| !u.is_empty()) {
        return Origin::parse(u)
            .and_then(|o| cfg.instance_for(&o))
            .map_or_else(|| refuse("no configured instance for that URL"), Ok);
    }
    match cfg.instances.as_slice() {
        [] => return refuse("no instance configured"),
        [one] => return Ok(one),
        _ => {}
    }
    let Some((index, provider)) = known else {
        return refuse("several instances are configured; name one with --instance");
    };
    let hits: Vec<&Instance> = cfg
        .instances
        .iter()
        .filter(|i| {
            cache::load(&cache::file(state_dir, &i.origin.to_string()))
                .accounts
                .iter()
                .any(|a| a.key == index && a.provider == provider)
        })
        .collect();
    match hits.as_slice() {
        [one] => Ok(one),
        [] => refuse("that account is not in any instance's current listing"),
        _ => refuse("that account is in several instances; name one with --instance"),
    }
}

/// The key for `inst`.
pub fn key_for(inst: &Instance) -> Result<Secret, Refusal> {
    keysource::read(&inst.key).map_err(|e| Refusal(format!("{}: {e}", inst.origin)))
}

/// Validates the `--action use` input and returns `(account, provider)`.
/// The values come from a link: they must be well-formed and, checked by
/// the caller against the instance, name an account of the snapshot.
pub fn action_tokens(stdin: &str) -> Result<(String, String), Refusal> {
    let v: Value = serde_json::from_str(stdin).map_err(|_| Refusal("bad input".into()))?;
    if v["action"].as_str() != Some("use") {
        return refuse("unknown action");
    }
    let t = v["tokens"].as_object();
    let get = |k: &str| t.and_then(|m| m.get(k)).and_then(Value::as_str);
    let (Some(a), Some(p)) = (get("account"), get("provider")) else {
        return refuse("needs the account and provider tokens");
    };
    let ok = |s: &str, n: usize| {
        (1..=n).contains(&s.len())
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._@-".contains(&b))
    };
    if !ok(a, 64) || !ok(p, 32) || !crate::config::PROVIDERS.contains(&p) {
        return refuse("malformed tokens");
    }
    Ok((a.into(), p.into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(n: usize, index: &str, provider: &str, label: &str, priority: i64) -> Entry {
        Entry {
            n,
            index: index.into(),
            name: format!("{label}.json"),
            provider: provider.into(),
            label: label.into(),
            priority,
            disabled: false,
            unavailable: false,
        }
    }

    #[test]
    fn priority_beats_the_best_other_enabled_account() {
        let mut l = vec![
            e(1, "a", "claude", "alice", 0),
            e(2, "b", "claude", "bob", 2),
            e(3, "c", "claude", "cy", 5),
            e(4, "d", "codex", "dee", 9),
        ];
        let Plan::Set(c) = plan_use(&l, &l[0]).unwrap() else {
            panic!()
        };
        // another provider's priority is irrelevant
        assert_eq!((c.from, c.to), (0, 6));
        // a rival only cooling down still returns: it counts
        l[2].unavailable = true;
        let Plan::Set(c) = plan_use(&l, &l[0]).unwrap() else {
            panic!()
        };
        assert_eq!(c.to, 6);
        // a disabled rival is not competition
        l[2].disabled = true;
        let Plan::Set(c) = plan_use(&l, &l[0]).unwrap() else {
            panic!()
        };
        assert_eq!(c.to, 3);
        l[1].disabled = true;
        assert_eq!(plan_use(&l, &l[0]).unwrap(), Plan::Alone);
    }

    #[test]
    fn ties_and_already_preferred() {
        let l = vec![
            e(1, "a", "claude", "alice", 3),
            e(2, "b", "claude", "bob", 3),
        ];
        let Plan::Set(c) = plan_use(&l, &l[0]).unwrap() else {
            panic!()
        };
        assert_eq!(c.to, 4);
        let l = vec![
            e(1, "a", "claude", "alice", 4),
            e(2, "b", "claude", "bob", 3),
        ];
        assert_eq!(plan_use(&l, &l[0]).unwrap(), Plan::Already);
    }

    #[test]
    fn a_disabled_target_is_refused_and_the_limit_holds() {
        let mut l = vec![
            e(1, "a", "claude", "alice", 0),
            e(2, "b", "claude", "bob", 0),
        ];
        l[0].disabled = true;
        assert!(plan_use(&l, &l[0]).is_err());
        l[0].disabled = false;
        l[1].priority = MAX_PRIORITY;
        assert!(plan_use(&l, &l[0]).is_err());
    }

    #[test]
    fn names_resolve_or_refuse() {
        let l = vec![
            e(1, "ix1", "claude", "work", 0),
            e(1, "ix2", "codex", "work", 0),
            e(2, "ix3", "claude", "home", 0),
        ];
        assert_eq!(resolve(&l, "ix3", None).unwrap().label, "home");
        assert_eq!(resolve(&l, "HOME", None).unwrap().index, "ix3");
        // positions count per provider: a bare one is refused when the
        // instance reads two
        let err = resolve(&l, "#2", None).unwrap_err().0;
        assert!(err.contains("--provider"), "{err}");
        assert_eq!(resolve(&l, "#2", Some("claude")).unwrap().index, "ix3");
        assert_eq!(resolve(&l, "1", Some("codex")).unwrap().index, "ix2");
        assert_eq!(resolve(&l, "#1", Some("claude")).unwrap().index, "ix1");
        // the same label under two providers is ambiguous until narrowed
        let err = resolve(&l, "work", None).unwrap_err().0;
        assert!(err.contains("ambiguous") && err.contains("codex"), "{err}");
        assert_eq!(resolve(&l, "work", Some("codex")).unwrap().index, "ix2");
        assert!(resolve(&l, "nobody", None).is_err());
        assert!(resolve(&l, "ix1", Some("codex")).is_err());
    }

    #[test]
    fn positions_count_enabled_accounts_per_provider() {
        let cfg = crate::config::parse("instance http://127.0.0.1:1\n key-file /k\n", None);
        let v = serde_json::json!({"files":[
            {"name":"a","auth_index":"a1","type":"claude","email":"a@x.test","disabled":true},
            {"name":"b","auth_index":"b1","type":"codex","email":"b@x.test"},
            {"name":"c","auth_index":"c1","type":"claude","email":"c@x.test"},
            {"name":"d","auth_index":"d1","type":"claude","email":"d@x.test"}]});
        let l = parse_entries(&cfg.instances[0], &v);
        let n: Vec<(&str, usize)> = l.iter().map(|e| (e.index.as_str(), e.n)).collect();
        assert_eq!(n, [("a1", 0), ("b1", 1), ("c1", 1), ("d1", 2)]);
        assert_eq!(resolve(&l, "#2", Some("claude")).unwrap().index, "d1");
        assert!(resolve(&l, "#1", None).is_err());
    }

    #[test]
    fn reset_restores_only_what_is_still_ours() {
        let l = vec![
            e(1, "a", "claude", "alice", 4),
            e(2, "b", "claude", "bob", 9),
            e(3, "c", "codex", "cy", 1),
        ];
        let s = State {
            bumps: vec![
                Bump {
                    index: "a".into(),
                    provider: "claude".into(),
                    prev: 0,
                    set: 4,
                },
                Bump {
                    index: "b".into(),
                    provider: "claude".into(),
                    prev: 0,
                    set: 3,
                },
                Bump {
                    index: "x".into(),
                    provider: "claude".into(),
                    prev: 0,
                    set: 3,
                },
            ],
        };
        let p = plan_reset(&l, &s, None);
        assert_eq!(p.restore.len(), 1);
        assert_eq!((p.restore[0].from, p.restore[0].to), (4, 0));
        assert_eq!(p.skipped, ["bob"]);
        assert_eq!(p.gone, 1);
        assert!(plan_reset(&l, &s, Some(&l[2])).restore.is_empty());
    }

    #[test]
    fn action_tokens_are_checked() {
        let ok = r#"{"action":"use","tokens":{"account":"ix1","provider":"claude"}}"#;
        assert_eq!(action_tokens(ok).unwrap(), ("ix1".into(), "claude".into()));
        for bad in [
            "",
            r#"{"action":"nope","tokens":{}}"#,
            r#"{"action":"use","tokens":{"account":"ix1"}}"#,
            r#"{"action":"use","tokens":{"account":"a b","provider":"claude"}}"#,
            r#"{"action":"use","tokens":{"account":"ix1","provider":"grok"}}"#,
        ] {
            assert!(action_tokens(bad).is_err(), "{bad}");
        }
    }
}
