//! One collection run: list the proxy's accounts, read each account's
//! quota (Claude from the proxy's passive rate-limit snapshot, Codex from
//! the upstream usage endpoint through the proxy's `api-call`), keep what
//! was learned in the plugin cache, and report what is known.
//!
//! Failure handling keeps the proxy safe: a management 401 or 403 stops
//! every further call until the key changes (the proxy bans a client
//! after five failed attempts); a 404 means management is off and waits an
//! hour; other failures back off from a minute to an hour while the last
//! good data is served.

use crate::cache::{self, AuthFail, Cache};
use crate::config::Instance;
use crate::mgmt::{Api, ApiReply, Client, MErr};
use crate::model::{self, Acct, Signals, Win};
use bar_plugin::keysource::fingerprint;
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
pub const AUTH_HOLD: i64 = 24 * 3600;
/// Management disabled: ask again after this long.
pub const OFF_HOLD: i64 = 3600;
/// Upstream calls in flight at once.
pub const PARALLEL: usize = 4;

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

/// What is known without any call: the cache, with its failure state.
pub fn cached(ctx: &Ctx) -> (Snapshot, Cache) {
    let path = cache::file(ctx.state_dir, &ctx.inst.origin.to_string());
    let c = cache::load(&path);
    let status = match &c.auth_fail {
        Some(f) if f.fp == fingerprint(ctx.key) && ctx.now - f.at < AUTH_HOLD => {
            Status::AuthStopped(f.status)
        }
        _ if ctx.now < c.off_until => Status::Off,
        _ if ctx.now < c.retry_at => Status::Backoff,
        _ => Status::Ok,
    };
    (snap(status, &c, ctx.now), c)
}

fn lower_signals(v: &Value) -> Signals {
    v.as_object()
        .map(|m| {
            m.iter()
                .filter_map(|(k, v)| Some((k.to_ascii_lowercase(), v.as_str()?.to_string())))
                .collect()
        })
        .unwrap_or_default()
}

fn valid_index(s: &str) -> bool {
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
        out.push((
            Acct {
                key: key.into(),
                provider,
                label: model::label_for(email, &inst.aliases, n + 1),
                local: model::label_for(email, &[], n + 1),
                priority: f["priority"].as_i64().unwrap_or(0),
                plan: f["id_token"]["plan_type"].as_str().map(model::plan_name),
                windows: vec![],
                at: 0,
                disabled: f["disabled"].as_bool().unwrap_or(false),
                unavailable: f["unavailable"].as_bool().unwrap_or(false),
                upstream_auth: false,
                retry_at: 0,
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

fn stop(path: &Path, c: &mut Cache, status: u16, fp: String, now: i64) {
    c.auth_fail = Some(AuthFail {
        status,
        at: now,
        fp,
    });
    let _ = cache::save(path, c);
}

/// Lists, reads quota, updates the cache. Makes no management call when
/// the cache says one would be refused, disabled or too soon.
pub fn collect(ctx: &Ctx) -> Snapshot {
    // an empty key is a failed attempt to the proxy: never send one
    if ctx.key.as_bytes().is_empty() {
        return Snapshot {
            status: Status::NoKey,
            accounts: vec![],
        };
    }
    let (known, mut c) = cached(ctx);
    if known.status != Status::Ok {
        return known;
    }
    let now = ctx.now;
    let path = cache::file(ctx.state_dir, &ctx.inst.origin.to_string());
    let fp = fingerprint(ctx.key);
    // a changed key clears an old refusal
    if c.auth_fail.take().is_some() {
        let _ = cache::save(&path, &c);
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
                stop(&path, &mut c, s, fp, now);
                return Snapshot {
                    status: Status::AuthStopped(s),
                    accounts: known.accounts,
                };
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
            let _ = cache::save(&path, &c);
            return Snapshot {
                status: Status::Off,
                accounts: vec![],
            };
        }
        c.fails += 1;
        c.retry_at = now + cache::backoff(c.fails);
        let _ = cache::save(&path, &c);
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
    let mut outcomes: Vec<(usize, Up)> = Vec::new();
    for chunk in todo.chunks(PARALLEL) {
        std::thread::scope(|s| {
            let hs: Vec<_> = chunk
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
    }
    if let Some(s) = outcomes.iter().find_map(|(_, u)| match u {
        Up::Mgmt(s) => Some(*s),
        _ => None,
    }) {
        stop(&path, &mut c, s, fp, now);
        return Snapshot {
            status: Status::AuthStopped(s),
            accounts: known.accounts,
        };
    }
    for (i, up) in outcomes {
        let (a, sig) = &mut list[i];
        let passive = model::codex_signal_windows(sig, now);
        match up {
            Up::Got(w, plan) => {
                a.windows = w;
                a.at = now;
                a.upstream_auth = false;
                a.retry_at = 0;
                if plan.is_some() {
                    a.plan = plan;
                }
                continue;
            }
            Up::Rate(r) => a.retry_at = now + r.unwrap_or(300).min(86_400) as i64,
            Up::UpstreamAuth => {
                a.upstream_auth = true;
                a.retry_at = now + 3600;
            }
            Up::Failed | Up::Mgmt(_) => a.retry_at = now + 60,
        }
        if !passive.is_empty() {
            a.windows = passive;
            a.at = now;
        }
    }
    // Codex accounts not read upstream this run: passive data when the
    // snapshot has any and no fresh upstream result is held
    let fetched: Vec<usize> = todo;
    for (i, (a, sig)) in list
        .iter_mut()
        .enumerate()
        .filter(|(_, (a, _))| a.provider == "codex")
    {
        if fetched.contains(&i) {
            continue;
        }
        let fresh = a.at > 0 && now - a.at < TTL_UPSTREAM && !a.windows.is_empty();
        let passive = model::codex_signal_windows(sig, now);
        if !fresh && !passive.is_empty() {
            a.windows = passive;
            a.at = now;
        }
    }

    c.accounts = list.into_iter().map(|(a, _)| a).collect();
    c.api = Some(api.name().into());
    c.fails = 0;
    c.retry_at = 0;
    c.off_until = 0;
    c.auth_fail = None;
    let _ = cache::save(&path, &c);
    snap(Status::Ok, &c, now)
}
