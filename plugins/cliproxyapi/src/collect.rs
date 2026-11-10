//! One collection run: list the proxy's accounts, read each account's
//! quota (Claude from the proxy's passive rate-limit snapshot, Codex from
//! the upstream usage endpoint through the proxy's `api-call`), keep what
//! was learned in the plugin cache, and report what is known.
//!
//! Failure handling keeps the proxy safe: a management 401 or 403 stops
//! every further call until the key changes (the proxy bans a client
//! after five failed attempts); a 404 means management is off and waits an
//! hour; other failures back off from a minute to an hour while the last
//! good data is served. Processes of one instance take turns under a
//! lock: the cache is read, the proxy called and the outcome saved by one
//! at a time, so a refused key costs one failed attempt, not one per
//! process. Nothing is sent when the lock or the cache cannot be had.

use crate::cache::{self, AuthFail, Cache};
use crate::config::Instance;
use crate::mgmt::{Api, ApiReply, Client, MErr};
use crate::model::{self, Acct, Signals, Win};
use bar_plugin::keyguard::Guard;
use bar_plugin::secret::Secret;
use serde_json::Value;
use std::path::Path;

/// Codex's usage endpoint, compiled in: the proxy's `api-call` sends the
/// account's token to any URL it is given, so no configuration may choose
/// one.
pub const CODEX_USAGE_URL: &str = "https://chatgpt.com/backend-api/wham/usage";
/// Upstream results stay fresh this long, whatever the plugin's `ttl`.
pub const TTL_UPSTREAM: i64 = 300;
/// Data older than this is drawn as stale.
pub const STALE_AFTER: i64 = 360;
/// Data older than this is dropped.
pub const KEEP: i64 = 3600;
/// A refused key is not retried for this long (or until it changes).
pub const AUTH_HOLD: i64 = bar_plugin::keyguard::AUTH_HOLD;
/// Management disabled: ask again after this long.
pub const OFF_HOLD: i64 = 3600;
/// Upstream calls in flight at once.
pub const PARALLEL: usize = 4;
/// No new batch of upstream calls starts after this many seconds of the
/// run (the host stops a job at 15 s).
pub const DEADLINE: u64 = 10;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Ok,
    /// Management refused the key (401 or 403); nothing was sent since.
    AuthStopped(u16),
    /// Management is disabled on the proxy.
    Off,
    /// The key source gave nothing: no call was made.
    NoKey,
    /// A transient failure; cached data is served.
    Backoff,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    pub status: Status,
    pub accounts: Vec<Acct>,
}

pub struct Ctx<'a> {
    pub inst: &'a Instance,
    pub key: &'a Secret,
    pub state_dir: &'a Path,
    /// Unix seconds.
    pub now: i64,
    /// Providers whose accounts may be read through the upstream API
    /// (only Codex needs it; Claude's quota is passive).
    pub fetch: &'a [&'a str],
}

fn snap(status: Status, c: &Cache, now: i64) -> Snapshot {
    let mut accounts = c.accounts.clone();
    for a in &mut accounts {
        if now - a.at > KEEP {
            a.windows.clear();
        }
    }
    Snapshot { status, accounts }
}

fn guard<'a>(ctx: &'a Ctx) -> Guard<'a> {
    Guard {
        state_dir: ctx.state_dir,
        mgmt: &ctx.inst.management,
    }
}

/// What is known without any call: the cache, with its failure state.
pub fn cached(ctx: &Ctx) -> (Snapshot, Cache) {
    let path = cache::file(ctx.state_dir, &ctx.inst.origin.to_string());
    let c = cache::load(&path);
    let g = guard(ctx);
    let status = if let Some(st) = c
        .refusal(ctx.key, AUTH_HOLD, ctx.now)
        .or_else(|| g.refusal(ctx.key, ctx.now))
    {
        Status::AuthStopped(st)
    } else if ctx.now < c.off_until {
        Status::Off
    } else if ctx.now < c.retry_at {
        Status::Backoff
    } else {
        Status::Ok
    };
    (snap(status, &c, ctx.now), c)
}

pub fn lower_signals(v: &Value) -> Signals {
    v.as_object()
        .map(|m| {
            m.iter()
                .filter_map(|(k, v)| Some((k.to_ascii_lowercase(), v.as_str()?.to_string())))
                .collect()
        })
        .unwrap_or_default()
}

pub fn valid_index(s: &str) -> bool {
    (1..=64).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
}

/// Accounts of the listing that the instance reads, with their passive
/// signals. Emails are used for the label only.
fn parse_listing(inst: &Instance, v: &Value) -> Vec<(Acct, Signals)> {
    let files = v
        .get("files")
        .and_then(Value::as_array)
        .or_else(|| v.as_array());
    let mut out = Vec::new();
    for (n, f) in files.into_iter().flatten().enumerate() {
        let provider = f["type"]
            .as_str()
            .or_else(|| f["provider"].as_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let key = f["auth_index"].as_str().unwrap_or("");
        if !inst.providers.contains(&provider) || !valid_index(key) {
            continue;
        }
        let email = ["email", "label", "account"]
            .iter()
            .find_map(|k| f[*k].as_str())
            .unwrap_or("");
        let (label, color) = model::identify(email, &inst.aliases, &inst.rules, n + 1);
        out.push((
            Acct {
                key: key.into(),
                provider,
                label,
                color,
                priority: f["priority"].as_i64().unwrap_or(0),
                plan: f["id_token"]["plan_type"].as_str().map(model::plan_name),
                windows: vec![],
                at: 0,
                disabled: f["disabled"].as_bool().unwrap_or(false),
                unavailable: f["unavailable"].as_bool().unwrap_or(false),
                upstream_auth: false,
                retry_at: 0,
                fails: 0,
            },
            lower_signals(&f["quota"]["signals"]),
        ));
    }
    out
}

enum Up {
    Got(Vec<Win>, Option<String>),
    Rate(Option<u64>),
    UpstreamAuth,
    Failed,
    Mgmt(u16),
}

fn upstream(client: &Client, key: &str, now: i64) -> Up {
    let headers = [
        ("Authorization", "Bearer $TOKEN$"),
        ("Accept", "application/json"),
        ("User-Agent", "codex_cli_rs/0.0.0"),
    ];
    match client.api_call(key, "GET", CODEX_USAGE_URL, &headers) {
        Ok(ApiReply {
            status: 200..=299,
            body,
            ..
        }) => {
            let (w, plan) = model::codex_windows(&body, now);
            if w.is_empty() && body["rate_limit"].is_null() {
                Up::Failed
            } else {
                Up::Got(w, plan)
            }
        }
        Ok(ApiReply {
            status: 401 | 403, ..
        }) => Up::UpstreamAuth,
        Ok(ApiReply {
            status: 429,
            retry_after,
            ..
        }) => Up::Rate(retry_after),
        Ok(_) => Up::Failed,
        Err(MErr::Auth(s)) => Up::Mgmt(s),
        Err(_) => Up::Failed,
    }
}

/// Records a management refusal of `key`. The save falls back to a file
/// without accounts when the full cache cannot be written; `false` when
/// the refusal could not be written at all (the caller still stops).
pub fn stop(g: &Guard, path: &Path, c: &mut Cache, status: u16, key: &Secret, now: i64) -> bool {
    // the shared record first: other plugins of this proxy honour it
    let shared = g.stop(status, key, now);
    let fp = c.fp(key);
    c.auth_fail = Some(AuthFail {
        status,
        at: now,
        fp,
    });
    cache::save_state(path, c) || shared
}

fn stopped(s: u16, known: Snapshot) -> Snapshot {
    Snapshot {
        status: Status::AuthStopped(s),
        accounts: known.accounts,
    }
}

/// Lists, reads quota, updates the cache. Makes no management call when
/// the cache says one would be refused, disabled or too soon, or when
/// another process holds the instance (its result is served instead).
pub fn collect(ctx: &Ctx) -> Snapshot {
    // an empty key is a failed attempt to the proxy: never send one
    if ctx.key.as_bytes().is_empty() {
        return Snapshot {
            status: Status::NoKey,
            accounts: vec![],
        };
    }
    let (known, _) = cached(ctx);
    if known.status != Status::Ok {
        return known;
    }
    let path = cache::file(ctx.state_dir, &ctx.inst.origin.to_string());
    let Some(_lock) = cache::lock(&guard(ctx), &path) else {
        return Snapshot {
            status: Status::Backoff,
            ..known
        };
    };
    // what another process learned while this one waited
    let (known, c) = cached(ctx);
    if known.status != Status::Ok {
        return known;
    }
    run(ctx, &path, known, c)
}

fn run(ctx: &Ctx, path: &Path, known: Snapshot, mut c: Cache) -> Snapshot {
    let now = ctx.now;
    // a changed key clears an old refusal
    if c.auth_fail.take().is_some() {
        let _ = cache::save_state(path, &c);
    }

    // the listing, v8 first (or the generation that answered last), the
    // other once on 404
    let first = c
        .api
        .as_deref()
        .map_or_else(|| Client::api_of(&ctx.inst.management), Api::from_name);
    let mut found = None;
    let mut missing = true;
    for api in [first, if first == Api::V8 { Api::V0 } else { Api::V8 }] {
        let base = Client::base_for(&ctx.inst.management, api);
        let Ok(client) = Client::new(&base, ctx.key) else {
            return known;
        };
        match client.credentials() {
            Ok(v) => {
                found = Some((base, v, api));
                break;
            }
            Err(MErr::NotFound) => continue,
            Err(MErr::Auth(s)) => {
                stop(&guard(ctx), path, &mut c, s, ctx.key, now);
                return stopped(s, known);
            }
            Err(MErr::Transient(_)) => {
                missing = false;
                break;
            }
        }
    }
    let Some((base, listing, api)) = found else {
        if missing {
            c.off_until = now + OFF_HOLD;
            let _ = cache::save_state(path, &c);
            return Snapshot {
                status: Status::Off,
                accounts: vec![],
            };
        }
        c.fails += 1;
        c.retry_at = now + cache::backoff(c.fails);
        let _ = cache::save_state(path, &c);
        return snap(Status::Backoff, &c, now);
    };
    let client = Client::new(&base, ctx.key).expect("checked above");

    let mut list = parse_listing(ctx.inst, &listing);
    for (a, _) in &mut list {
        if let Some(p) = c
            .accounts
            .iter()
            .find(|p| p.key == a.key && p.provider == a.provider)
        {
            a.windows = p.windows.clone();
            a.at = p.at;
            a.upstream_auth = p.upstream_auth;
            a.retry_at = p.retry_at;
            a.fails = p.fails;
            if a.plan.is_none() {
                a.plan = p.plan.clone();
            }
        }
    }

    // Claude: the passive snapshot, else what was last seen
    for (a, sig) in list.iter_mut().filter(|(a, _)| a.provider == "claude") {
        let w = model::claude_windows(sig);
        if !w.is_empty() {
            a.windows = w;
            a.at = now;
        }
    }

    // Codex: upstream when this provider is wanted and the data is old
    let want = ctx.fetch.contains(&"codex");
    let todo: Vec<usize> = list
        .iter()
        .enumerate()
        .filter(|(_, (a, _))| {
            let fresh = a.at > 0 && now - a.at < TTL_UPSTREAM && !a.windows.is_empty();
            a.provider == "codex" && want && !a.disabled && now >= a.retry_at && !fresh
        })
        .map(|(i, _)| i)
        .collect();

    // the listing is saved before the upstream phase, with a short hold:
    // a run cut off in it is not repeated at once
    let store = |c: &mut Cache, list: &[(Acct, Signals)]| {
        c.accounts = list.iter().map(|(a, _)| a.clone()).collect();
        cache::save_state(path, c)
    };
    c.api = Some(api.name().into());
    c.off_until = 0;
    c.retry_at = now + cache::backoff(1);
    store(&mut c, &list);

    // the first call goes alone: a management refusal costs one attempt
    let mut batches: Vec<&[usize]> = Vec::new();
    if let Some((one, rest)) = todo.split_first().map(|(_, r)| (&todo[..1], r)) {
        batches.push(one);
        batches.extend(rest.chunks(PARALLEL));
    }
    let started = std::time::Instant::now();
    let mut done: Vec<usize> = Vec::new();
    let mut mgmt = None;
    for batch in batches {
        if started.elapsed().as_secs() >= DEADLINE {
            break;
        }
        let mut outcomes: Vec<(usize, Up)> = Vec::new();
        std::thread::scope(|s| {
            let hs: Vec<_> = batch
                .iter()
                .map(|&i| {
                    let key = list[i].0.key.clone();
                    let client = &client;
                    (i, s.spawn(move || upstream(client, &key, now)))
                })
                .collect();
            for (i, h) in hs {
                outcomes.push((i, h.join().unwrap_or(Up::Failed)));
            }
        });
        for (i, up) in outcomes {
            done.push(i);
            if let Up::Mgmt(s) = up {
                mgmt = Some(s);
                continue;
            }
            let (a, sig) = &mut list[i];
            apply(a, sig, up, now);
        }
        if mgmt.is_some() {
            break;
        }
        store(&mut c, &list);
    }
    if let Some(s) = mgmt {
        c.accounts = list.into_iter().map(|(a, _)| a).collect();
        stop(&guard(ctx), path, &mut c, s, ctx.key, now);
        return stopped(s, known);
    }
    // Codex accounts not read upstream this run: passive data when the
    // snapshot has any and no fresh upstream result is held
    for (i, (a, sig)) in list
        .iter_mut()
        .enumerate()
        .filter(|(_, (a, _))| a.provider == "codex")
    {
        if done.contains(&i) {
            continue;
        }
        let fresh = a.at > 0 && now - a.at < TTL_UPSTREAM && !a.windows.is_empty();
        let passive = model::codex_signal_windows(sig, now);
        if !fresh && !passive.is_empty() {
            a.windows = passive;
            a.at = now;
        }
    }

    c.fails = 0;
    c.retry_at = 0;
    c.auth_fail = None;
    store(&mut c, &list);
    snap(Status::Ok, &c, now)
}

/// Folds one upstream outcome (not a management refusal) into `a`.
fn apply(a: &mut Acct, sig: &Signals, up: Up, now: i64) {
    let passive = model::codex_signal_windows(sig, now);
    match up {
        Up::Got(w, plan) => {
            a.windows = w;
            a.at = now;
            a.upstream_auth = false;
            a.retry_at = 0;
            a.fails = 0;
            if plan.is_some() {
                a.plan = plan;
            }
            return;
        }
        Up::Rate(r) => a.retry_at = now + r.unwrap_or(300).min(86_400) as i64,
        Up::UpstreamAuth => {
            a.upstream_auth = true;
            a.retry_at = now + 3600;
        }
        Up::Failed | Up::Mgmt(_) => {
            a.fails += 1;
            a.retry_at = now + cache::backoff(a.fails);
        }
    }
    if !passive.is_empty() {
        a.windows = passive;
        a.at = now;
    }
}
