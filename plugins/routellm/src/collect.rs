//! One collection run: read the proxy's provider entry and claude alias
//! rows, ask RouteLLM for `/health` and `/v1/models` (only while routing
//! is on, or when tiers are wanted), keep what was learned in the plugin
//! cache and report what is known.
//!
//! Failure handling keeps the proxy safe, as `bar-cliproxyapi` does: a
//! management 401 or 403 stops every further call (from every plugin of
//! that proxy) until the key changes; a 404 waits an hour; other failures
//! back off from a minute to an hour while the last good data is served.
//! Processes take turns under a lock, and nothing is sent when the lock or
//! the cache cannot be had. RouteLLM being down never backs off the proxy.

use crate::cache::{self, Cache, Entry, Problem, Tier};
use crate::config::Instance;
use crate::mgmt::{Client, MErr};
use bar_plugin::http::{self, Opts, Scheme};
use bar_plugin::keyguard::{backoff, Guard};
use bar_plugin::secret::Secret;
use bar_plugin::url::Origin;
use serde_json::Value;
use std::path::Path;
use std::time::Instant;

/// Management disabled: ask again after this long.
pub const OFF_HOLD: i64 = 3600;
/// The proxy is read at most this often.
pub const MGMT_FRESH: i64 = 30;
/// RouteLLM is probed at most this often.
pub const PROBE_FRESH: i64 = 15;
/// Data older than this is drawn as stale.
pub const STALE_AFTER: i64 = 120;
/// No new call starts after this many seconds of the run.
pub const DEADLINE: u64 = 6;
pub const CAP_HEALTH: usize = 4 * 1024;
pub const CAP_MODELS: usize = 128 * 1024;
pub const T_PROBE: u64 = 3;

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
    pub cache: Cache,
}

pub struct Ctx<'a> {
    pub inst: &'a Instance,
    pub key: &'a Secret,
    pub state_dir: &'a Path,
    /// Unix seconds.
    pub now: i64,
    /// Ask RouteLLM for its tiers even while routing is off.
    pub want_tiers: bool,
    /// Ignore the freshness windows (an explicit `status --probe`).
    pub force: bool,
}

impl Ctx<'_> {
    pub fn guard(&self) -> Guard<'_> {
        Guard {
            state_dir: self.state_dir,
            mgmt: &self.inst.management,
        }
    }
    pub fn path(&self) -> std::path::PathBuf {
        cache::file(self.state_dir, &self.inst.origin.to_string())
    }
}

fn status_of(ctx: &Ctx, c: &Cache) -> Status {
    if let Some(st) = ctx.guard().refusal(ctx.key, ctx.now) {
        Status::AuthStopped(st)
    } else if ctx.now < c.off_until {
        Status::Off
    } else if ctx.now < c.retry_at {
        Status::Backoff
    } else {
        Status::Ok
    }
}

/// What is known without any call: the cache, with its failure state.
pub fn cached(ctx: &Ctx) -> Snapshot {
    let c = cache::load(&ctx.path());
    Snapshot {
        status: status_of(ctx, &c),
        cache: c,
    }
}

/// `host:port` equality between the entry's `base-url` and the configured
/// RouteLLM (loopback names count as one host).
pub fn same_target(base_url: &str, want: &Origin) -> bool {
    let Some(o) = Origin::parse(base_url) else {
        return false;
    };
    o.port == want.port
        && o.scheme == want.scheme
        && (o.host == want.host || (o.is_loopback() && want.is_loopback()))
}

fn probe_opts(rl: &Origin, cap: usize) -> Opts {
    Opts {
        scheme: Scheme::LoopbackHttp,
        noproxy: rl.is_loopback(),
        user_agent: format!("bar-routellm/{}", crate::VERSION),
        ..Opts::new(T_PROBE, cap)
    }
}

/// `/health`: does RouteLLM answer. No credentials are sent.
pub fn health(rl: &Origin) -> bool {
    http::get(&format!("{rl}/health"), &[], &probe_opts(rl, CAP_HEALTH))
        .ok()
        .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
        .is_some_and(|v| v["status"].as_str() == Some("online"))
}

/// `/v1/models`: the tiers and the capabilities they declare.
pub fn tiers(rl: &Origin) -> Option<Vec<Tier>> {
    let body = http::get(&format!("{rl}/v1/models"), &[], &probe_opts(rl, CAP_MODELS)).ok()?;
    parse_tiers(&body)
}

pub fn parse_tiers(body: &[u8]) -> Option<Vec<Tier>> {
    let v: Value = serde_json::from_slice(body).ok()?;
    let data = v["data"].as_array()?;
    Some(
        data.iter()
            .filter_map(|m| {
                let id = m["id"].as_str()?.to_string();
                let r = &m["routellm"];
                let caps = &r["capabilities"];
                Some(Tier {
                    id,
                    kind: r["kind"].as_str().unwrap_or("").to_string(),
                    vision: caps["vision"].as_bool(),
                    tools: caps["tools"].as_bool(),
                    context: caps["context"].as_u64(),
                })
            })
            .take(64)
            .collect(),
    )
}

/// The fallback is armed when every exposed non-private alias has a
/// claude row, so OFF serves it.
pub fn armed(c: &Cache) -> bool {
    let Some(e) = &c.entry else {
        return false;
    };
    e.aliases
        .iter()
        .filter(|a| a.tier != "private")
        .all(|a| c.rows.iter().any(|r| r.alias == a.alias))
}

/// Reads, updates the cache. Makes no management call when the cache says
/// one would be refused, disabled, too soon or unneeded, or when another
/// process holds the proxy (its result is served instead).
pub fn collect(ctx: &Ctx) -> Snapshot {
    // an empty key is a failed attempt to the proxy: never send one
    if ctx.key.as_bytes().is_empty() {
        return Snapshot {
            status: Status::NoKey,
            cache: cache::load(&ctx.path()),
        };
    }
    let known = cached(ctx);
    if known.status != Status::Ok {
        return known;
    }
    let fresh = !ctx.force && ctx.now - known.cache.at < MGMT_FRESH && known.cache.entry.is_some();
    let on = known.cache.entry.as_ref().is_some_and(|e| !e.disabled);
    let probe_due =
        (on || ctx.want_tiers) && (ctx.force || ctx.now - known.cache.up_at >= PROBE_FRESH);
    if fresh && !probe_due {
        return known;
    }
    let path = ctx.path();
    let Some(_lock) = cache::lock(&ctx.guard(), &path) else {
        return Snapshot {
            status: Status::Backoff,
            ..known
        };
    };
    // what another process learned while this one waited
    let known = cached(ctx);
    if known.status != Status::Ok {
        return known;
    }
    run(ctx, &path, known.cache)
}

fn run(ctx: &Ctx, path: &Path, mut c: Cache) -> Snapshot {
    let now = ctx.now;
    let started = Instant::now();
    let snap = |status, c: Cache| Snapshot { status, cache: c };
    let stop = |s: u16, c: Cache| {
        ctx.guard().stop(s, ctx.key, now);
        snap(Status::AuthStopped(s), c)
    };
    let mgmt_fresh = !ctx.force && now - c.at < MGMT_FRESH && c.entry.is_some();
    if !mgmt_fresh {
        let Ok(client) = Client::new(&ctx.inst.management, ctx.key) else {
            return snap(Status::Backoff, c);
        };
        match client.entry(&ctx.inst.provider) {
            Ok(Some(it)) => {
                c.problem = same_target(&it.base_url, &ctx.inst.routellm)
                    .then_some(())
                    .map_or(Some(Problem::Mismatch), |()| None);
                c.entry = Some(Entry {
                    disabled: it.disabled,
                    priority: it.priority,
                    aliases: it.aliases,
                });
            }
            Ok(None) => {
                c.entry = None;
                c.problem = Some(Problem::Unconfigured);
            }
            Err(MErr::Auth(s)) => return stop(s, c),
            Err(MErr::NotFound) => {
                c.off_until = now + OFF_HOLD;
                let _ = cache::save_state(path, &c);
                return snap(Status::Off, c);
            }
            Err(MErr::Transient(_)) => {
                c.fails += 1;
                c.retry_at = now + backoff(c.fails);
                let _ = cache::save_state(path, &c);
                return snap(Status::Backoff, c);
            }
        }
        if started.elapsed().as_secs() < DEADLINE {
            match client.claude_rows() {
                Ok(r) => c.rows = r,
                Err(MErr::Auth(s)) => return stop(s, c),
                // the rows are secondary: keep what was last seen
                Err(_) => {}
            }
        }
        c.at = now;
        c.fails = 0;
        c.retry_at = 0;
        c.off_until = 0;
        let _ = cache::save_state(path, &c);
    }
    // RouteLLM: only while routing is on (or tiers were asked for); its
    // answers never back off the proxy
    let on = c.entry.as_ref().is_some_and(|e| !e.disabled);
    if (on || ctx.want_tiers) && c.problem.is_none() && started.elapsed().as_secs() < DEADLINE {
        let up = health(&ctx.inst.routellm);
        c.up = Some(up);
        c.up_at = now;
        if up && started.elapsed().as_secs() < DEADLINE {
            if let Some(t) = tiers(&ctx.inst.routellm) {
                c.tiers = t;
            }
        }
        let _ = cache::save_state(path, &c);
    } else if !on && !ctx.want_tiers {
        c.up = None;
    }
    snap(Status::Ok, c)
}
