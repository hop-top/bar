//! Collection against a fake management server with synthetic accounts.

mod fake;

use bar_cliproxyapi::cache;
use bar_cliproxyapi::collect::{collect, Ctx, Status, AUTH_HOLD, KEEP, OFF_HOLD, TTL_UPSTREAM};
use bar_cliproxyapi::config::{self, Instance};
use bar_plugin::secret::Secret;
use fake::{ok, status, Rec, Reply, Server};
use serde_json::{json, Value};
use std::path::PathBuf;

const T0: i64 = 1_800_000_000;
const KEY: &str = "test-key-AAAA";
const MAIL_A: &str = "alice.synthetic@corp.test";
const MAIL_B: &str = "bob.synthetic@corp.test";

struct Env {
    dir: PathBuf,
    inst: Instance,
}

impl Env {
    fn new(s: &Server, name: &str) -> Env {
        let dir = std::env::temp_dir().join(format!("cpa-collect-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let text = format!(
            "instance {}\n key-file /k\n alias \"*@corp.test\" corp\n",
            s.origin()
        );
        let mut cfg = config::parse(&text, None);
        assert!(cfg.errors.is_empty(), "{:?}", cfg.errors);
        Env {
            dir,
            inst: cfg.instances.remove(0),
        }
    }

    fn run(&self, key: &str, now: i64, fetch: &[&str]) -> bar_cliproxyapi::collect::Snapshot {
        collect(&Ctx {
            inst: &self.inst,
            key: &Secret::new(key.as_bytes().to_vec()),
            state_dir: &self.dir,
            now,
            fetch,
        })
    }

    fn cache_path(&self) -> PathBuf {
        cache::file(&self.dir, &self.inst.origin.to_string())
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn listing() -> Value {
    json!({"observed_at":"2027-01-01T00:00:00Z","files":[
        {"id":"alice.synthetic@corp.test.json","auth_index":"ix-claude-1","type":"claude",
         "email":MAIL_A,"label":MAIL_A,"path":"/home/x/auths/alice.json","disabled":false,
         "quota":{"signals":{
            "Anthropic-Ratelimit-Unified-7d-Utilization":"0.25",
            "Anthropic-Ratelimit-Unified-7d-Reset": (T0 + 86_400).to_string()}}},
        {"id":"bob.json","auth_index":"ix-claude-2","type":"claude","email":MAIL_B,
         "disabled":false,"quota":{"signals":{}}},
        {"id":"c.json","auth_index":"ix-codex-1","type":"codex","email":"carol.synthetic@other.test",
         "id_token":{"plan_type":"pro","chatgpt_account_id":"acct-secret"},"quota":{"signals":{}}},
        {"id":"x.json","auth_index":"ix-xai-1","type":"xai","email":"x@y.test"},
        {"id":"d.json","auth_index":"bad index!","type":"claude","email":"d@y.test"}
    ]})
}

fn wham(primary: Value, secondary: Value) -> Value {
    json!({"plan_type":"pro","email":MAIL_A,"rate_limit":{"allowed":true,"limit_reached":false,
        "primary_window":primary,"secondary_window":secondary}})
}

fn api_reply(status: u16, body: Value) -> String {
    json!({"status_code":status,"header":{},"body":body.to_string()}).to_string()
}

/// Standard handler: v8 paths, one claude/codex mix, codex 7d-only.
fn std_handler(r: &Rec) -> Reply {
    match (r.method.as_str(), r.path.as_str()) {
        ("GET", "/v8/management/credentials") => ok(listing().to_string()),
        ("POST", "/v8/management/requests/api-call") => ok(api_reply(
            200,
            wham(
                json!({"used_percent":42,"limit_window_seconds":604800,"reset_at":T0+1000}),
                Value::Null,
            ),
        )),
        _ => status(404),
    }
}

#[test]
fn collects_claude_from_signals_and_codex_through_api_call() {
    let s = Server::start(std_handler);
    let e = Env::new(&s, "ok");
    let snap = e.run(KEY, T0, &["codex"]);
    assert_eq!(snap.status, Status::Ok);
    // xai and the malformed index are not read
    assert_eq!(snap.accounts.len(), 3);
    let a = &snap.accounts[0];
    assert_eq!(
        (a.provider.as_str(), a.label.as_str(), a.key.as_str()),
        ("claude", "corp", "ix-claude-1")
    );
    assert_eq!(a.windows.len(), 1, "5h absent: omitted");
    assert_eq!(
        (
            a.windows[0].id.as_str(),
            a.windows[0].used,
            a.windows[0].resets_at
        ),
        ("7d", 25.0, T0 + 86_400)
    );
    assert!(
        snap.accounts[1].windows.is_empty(),
        "idle account: no signals, no windows"
    );
    let c = &snap.accounts[2];
    assert_eq!(
        (c.provider.as_str(), c.plan.as_deref()),
        ("codex", Some("pro"))
    );
    assert_eq!(c.windows.len(), 1, "pro plan: 7d only");
    assert_eq!((c.windows[0].id.as_str(), c.windows[0].used), ("7d", 42.0));
    // every request carried the key as a bearer token, and the api-call
    // went to the compiled-in URL with the token placeholder
    let log = s.log.lock().unwrap().clone();
    assert_eq!(log.len(), 2);
    assert!(log
        .iter()
        .all(|r| r.auth == format!("bearer {}", KEY.to_ascii_lowercase())
            || r.auth.eq_ignore_ascii_case(&format!("bearer {KEY}"))));
    let call: Value = serde_json::from_str(&log[1].body).unwrap();
    assert_eq!(call["url"], "https://chatgpt.com/backend-api/wham/usage");
    assert_eq!(call["auth_index"], "ix-codex-1");
    assert_eq!(call["header"]["Authorization"], "Bearer $TOKEN$");
    assert!(call.get("proxy_url").is_none());
}

#[test]
fn cache_holds_no_emails_paths_tokens_or_keys() {
    let s = Server::start(std_handler);
    let e = Env::new(&s, "privacy");
    e.run(KEY, T0, &["codex"]);
    let text = std::fs::read_to_string(e.cache_path()).unwrap();
    for secret in [
        MAIL_A,
        MAIL_B,
        "@",
        "other.test",
        "corp.test",
        "/home/x",
        "acct-secret",
        KEY,
        "TOKEN",
    ] {
        assert!(!text.contains(secret), "cache leaks {secret}");
    }
    assert!(text.contains("ix-claude-1") && text.contains("\"corp\""));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let f = std::fs::metadata(e.cache_path())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        let d = std::fs::metadata(e.cache_path().parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!((f, d), (0o600, 0o700));
    }
}

#[test]
fn auth_failure_stops_all_calls_until_the_key_changes() {
    for code in [401u16, 403] {
        let s = Server::start(move |_| status(code));
        let e = Env::new(&s, &format!("auth{code}"));
        let snap = e.run(KEY, T0, &["codex"]);
        assert_eq!(snap.status, Status::AuthStopped(code));
        assert_eq!(s.count(), 1, "one failed attempt, not a v0 retry");
        // same key, any later time inside the hold: no request at all
        for dt in [1, 600, AUTH_HOLD - 1] {
            assert_eq!(
                e.run(KEY, T0 + dt, &["codex"]).status,
                Status::AuthStopped(code)
            );
        }
        assert_eq!(s.count(), 1);
        // a changed key is tried once, and stops again
        assert_eq!(
            e.run("another-key-BBBB", T0 + 10, &["codex"]).status,
            Status::AuthStopped(code)
        );
        assert_eq!(s.count(), 2);
        // after the hold the same key is tried again
        e.run("another-key-BBBB", T0 + 10 + AUTH_HOLD, &[]);
        assert_eq!(s.count(), 3);
        // the cache stores a fingerprint, never the key
        let text = std::fs::read_to_string(e.cache_path()).unwrap();
        assert!(!text.contains(KEY) && !text.contains("another-key"));
    }
}

#[test]
fn a_new_key_that_works_clears_the_stop() {
    let good = Arc2::new(false);
    let g = good.clone();
    let s = Server::start(move |r| if g.get() { std_handler(r) } else { status(401) });
    let e = Env::new(&s, "recover");
    assert_eq!(
        e.run("old-key-CCCC", T0, &[]).status,
        Status::AuthStopped(401)
    );
    good.set(true);
    assert_eq!(e.run("new-key-DDDD", T0 + 5, &[]).status, Status::Ok);
    assert!(cache::load(&e.cache_path()).auth_fail.is_none());
}

#[test]
fn a_refusal_recorded_by_another_plugin_of_the_proxy_stops_calls() {
    let s = Server::start(std_handler);
    let e = Env::new(&s, "shared");
    // another plugin's v0 base: same origin, same record
    let other = bar_plugin::keyguard::Guard {
        state_dir: &e.dir,
        mgmt: &format!("{}/v0/management", s.origin()),
    };
    other.stop(401, &Secret::new(KEY.as_bytes().to_vec()), T0);
    assert_eq!(e.run(KEY, T0 + 1, &[]).status, Status::AuthStopped(401));
    assert_eq!(s.count(), 0);
    // a changed key is tried; this plugin's own refusal is shared back
    let s2 = Server::start(|_| status(403));
    let e2 = Env::new(&s2, "shared2");
    e2.run(KEY, T0, &[]);
    let g2 = bar_plugin::keyguard::Guard {
        state_dir: &e2.dir,
        mgmt: &format!("{}/v0/management", s2.origin()),
    };
    assert_eq!(
        g2.refusal(&Secret::new(KEY.as_bytes().to_vec()), T0 + 1),
        Some(403)
    );
}

#[test]
fn an_empty_key_is_never_sent() {
    let s = Server::start(std_handler);
    let e = Env::new(&s, "empty");
    let snap = e.run("", T0, &["codex"]);
    assert_eq!(s.count(), 0);
    assert_ne!(snap.status, Status::Ok);
}

#[test]
fn management_off_is_remembered_for_an_hour() {
    let s = Server::start(|_| status(404));
    let e = Env::new(&s, "off");
    assert_eq!(e.run(KEY, T0, &[]).status, Status::Off);
    assert_eq!(
        s.paths(),
        ["/v8/management/credentials", "/v0/management/auth-files"]
    );
    assert_eq!(e.run(KEY, T0 + OFF_HOLD - 1, &[]).status, Status::Off);
    assert_eq!(s.count(), 2);
    e.run(KEY, T0 + OFF_HOLD, &[]);
    assert_eq!(s.count(), 4);
}

#[test]
fn falls_back_to_v0_on_404_and_remembers_it() {
    let s = Server::start(|r| match r.path.as_str() {
        "/v0/management/auth-files" => ok(listing().to_string()),
        "/v0/management/api-call" => ok(api_reply(
            200,
            wham(
                json!({"used_percent":10,"limit_window_seconds":18000,"reset_after_seconds":60}),
                json!({"used_percent":20,"limit_window_seconds":604800,"reset_after_seconds":600}),
            ),
        )),
        _ => status(404),
    });
    let e = Env::new(&s, "v0");
    let snap = e.run(KEY, T0, &["codex"]);
    assert_eq!(snap.status, Status::Ok);
    assert_eq!(
        s.paths(),
        [
            "/v8/management/credentials",
            "/v0/management/auth-files",
            "/v0/management/api-call"
        ]
    );
    let c = &snap.accounts[2];
    assert_eq!(
        c.windows.iter().map(|w| w.id.as_str()).collect::<Vec<_>>(),
        ["5h", "7d"]
    );
    assert_eq!(c.windows[0].resets_at, T0 + 60);
    // next run starts at v0
    e.run(KEY, T0 + 1, &[]);
    assert_eq!(s.paths()[3], "/v0/management/auth-files");
    assert_eq!(s.count(), 4);
}

#[test]
fn upstream_results_are_cached_for_five_minutes() {
    let s = Server::start(std_handler);
    let e = Env::new(&s, "ttl");
    e.run(KEY, T0, &["codex"]);
    let calls = |s: &Server| s.paths().iter().filter(|p| p.ends_with("api-call")).count();
    assert_eq!(calls(&s), 1);
    e.run(KEY, T0 + TTL_UPSTREAM - 1, &["codex"]);
    assert_eq!(calls(&s), 1, "fresh: no upstream call");
    e.run(KEY, T0 + TTL_UPSTREAM, &["codex"]);
    assert_eq!(calls(&s), 2);
    // claude's model asks for no upstream call at all
    e.run(KEY, T0 + 10 * TTL_UPSTREAM, &["claude"]);
    assert_eq!(calls(&s), 2);
    // and the old codex data is still served (stale), not dropped
    let snap = e.run(KEY, T0 + 10 * TTL_UPSTREAM, &["claude"]);
    assert_eq!(snap.accounts[2].windows.len(), 1);
}

#[test]
fn old_data_is_dropped_after_an_hour() {
    let s = Server::start(std_handler);
    let e = Env::new(&s, "keep");
    e.run(KEY, T0, &["codex"]);
    let snap = e.run(KEY, T0 + KEEP + 1, &["claude"]);
    assert!(snap.accounts[2].windows.is_empty());
}

#[test]
fn transient_failures_back_off_and_serve_the_cache() {
    let good = Arc2::new(true);
    let g = good.clone();
    let s = Server::start(move |r| if g.get() { std_handler(r) } else { status(503) });
    let e = Env::new(&s, "backoff");
    assert_eq!(e.run(KEY, T0, &[]).accounts.len(), 3);
    good.set(false);
    let snap = e.run(KEY, T0 + 100, &[]);
    assert_eq!(snap.status, Status::Backoff);
    assert_eq!(snap.accounts.len(), 3, "last good data kept");
    let n = s.count();
    // inside the 60 s backoff: no call
    assert_eq!(e.run(KEY, T0 + 120, &[]).status, Status::Backoff);
    assert_eq!(s.count(), n);
    // second failure doubles it
    e.run(KEY, T0 + 161, &[]);
    assert_eq!(cache::load(&e.cache_path()).fails, 2);
    assert_eq!(cache::load(&e.cache_path()).retry_at, T0 + 161 + 120);
    // recovery resets
    good.set(true);
    assert_eq!(e.run(KEY, T0 + 400, &[]).status, Status::Ok);
    assert_eq!(cache::load(&e.cache_path()).fails, 0);
}

#[test]
fn oversize_responses_are_refused() {
    let big = format!("{{\"files\":[],\"pad\":\"{}\"}}", "a".repeat(300 * 1024));
    let s = Server::start(move |_| ok(big.clone()));
    let e = Env::new(&s, "bigsize");
    assert_eq!(e.run(KEY, T0, &[]).status, Status::Backoff);

    // an oversize api-call wrapper: the account keeps no windows and the
    // management key is not blamed
    let pad = "a".repeat(200 * 1024);
    let s = Server::start(move |r| match r.path.as_str() {
        "/v8/management/credentials" => ok(listing().to_string()),
        _ => ok(json!({"status_code":200,"header":{},"body":pad}).to_string()),
    });
    let e = Env::new(&s, "bigcall");
    let snap = e.run(KEY, T0, &["codex"]);
    assert_eq!(snap.status, Status::Ok);
    assert!(snap.accounts[2].windows.is_empty());
    assert!(cache::load(&e.cache_path()).auth_fail.is_none());
}

#[test]
fn codex_failures_are_per_account_and_not_management_failures() {
    // upstream 401: account flagged, one-hour pause, management fine
    let s = Server::start(|r| match r.path.as_str() {
        "/v8/management/credentials" => ok(listing().to_string()),
        _ => ok(api_reply(401, json!({}))),
    });
    let e = Env::new(&s, "up401");
    let snap = e.run(KEY, T0, &["codex"]);
    assert_eq!(snap.status, Status::Ok);
    assert!(snap.accounts[2].upstream_auth);
    let n = s.count();
    e.run(KEY, T0 + TTL_UPSTREAM + 1, &["codex"]);
    assert_eq!(s.count(), n + 1, "listing only: the account is paused");
    e.run(KEY, T0 + 3601, &["codex"]);
    assert_eq!(
        s.paths().iter().filter(|p| p.ends_with("api-call")).count(),
        2
    );

    // upstream 429 honours Retry-After
    let s = Server::start(|r| match r.path.as_str() {
        "/v8/management/credentials" => ok(listing().to_string()),
        _ => {
            ok(json!({"status_code":429,"header":{"Retry-After":["900"]},"body":"{}"}).to_string())
        }
    });
    let e = Env::new(&s, "up429");
    e.run(KEY, T0, &["codex"]);
    assert_eq!(cache::load(&e.cache_path()).accounts[2].retry_at, T0 + 900);

    // an api-call refused by the management API is a management failure
    let s = Server::start(|r| match r.path.as_str() {
        "/v8/management/credentials" => ok(listing().to_string()),
        _ => status(403),
    });
    let e = Env::new(&s, "mgmt403");
    assert_eq!(e.run(KEY, T0, &["codex"]).status, Status::AuthStopped(403));
    assert_eq!(
        e.run(KEY, T0 + 1, &["codex"]).status,
        Status::AuthStopped(403)
    );
    assert_eq!(s.count(), 2);
}

#[test]
fn codex_windows_may_be_null_and_fall_back_to_passive_signals() {
    // both windows null: no windows, no error
    let s = Server::start(|r| match r.path.as_str() {
        "/v8/management/credentials" => ok(listing().to_string()),
        _ => ok(api_reply(200, wham(Value::Null, Value::Null))),
    });
    let e = Env::new(&s, "nulls");
    let snap = e.run(KEY, T0, &["codex"]);
    assert_eq!(snap.status, Status::Ok);
    assert!(snap.accounts[2].windows.is_empty());

    // upstream fails but the listing carries passive codex headers
    let mut l = listing();
    l["files"][2]["quota"]["signals"] = json!({
        "X-Codex-Primary-Used-Percent":"33","X-Codex-Primary-Window-Minutes":"300",
        "X-Codex-Primary-Reset-After-Seconds":"120"});
    let s = Server::start(move |r| match r.path.as_str() {
        "/v8/management/credentials" => ok(l.to_string()),
        _ => status(502),
    });
    let e = Env::new(&s, "passive");
    let snap = e.run(KEY, T0, &["codex"]);
    let w = &snap.accounts[2].windows;
    assert_eq!(
        (w[0].id.as_str(), w[0].used, w[0].resets_at),
        ("5h", 33.0, T0 + 120)
    );
}

#[test]
fn only_configured_providers_are_read() {
    let s = Server::start(std_handler);
    let mut e = Env::new(&s, "prov");
    e.inst.providers = vec!["claude".into()];
    let snap = e.run(KEY, T0, &["codex"]);
    assert!(snap.accounts.iter().all(|a| a.provider == "claude"));
    assert_eq!(s.count(), 1, "no codex account: no api-call");
}

#[test]
fn loopback_is_reached_directly_despite_proxy_variables() {
    // a bogus proxy in the environment must not be used for loopback
    std::env::set_var("http_proxy", "http://127.0.0.1:9");
    std::env::set_var("HTTP_PROXY", "http://127.0.0.1:9");
    let s = Server::start(std_handler);
    let e = Env::new(&s, "noproxy");
    assert_eq!(e.run(KEY, T0, &[]).status, Status::Ok);
}

fn many_codex(n: usize) -> Value {
    let files: Vec<Value> = (0..n)
        .map(|i| {
            json!({"id":format!("c{i}.json"),"auth_index":format!("ix-codex-{i}"),
                "type":"codex","email":format!("u{i}@other.test"),"quota":{"signals":{}}})
        })
        .collect();
    json!({"files":files})
}

#[test]
fn a_management_refusal_of_the_first_upstream_call_stops_the_rest() {
    let l = many_codex(9);
    let s = Server::start(move |r| match r.path.as_str() {
        "/v8/management/credentials" => ok(l.to_string()),
        _ => status(403),
    });
    let e = Env::new(&s, "mgmt-first");
    assert_eq!(e.run(KEY, T0, &["codex"]).status, Status::AuthStopped(403));
    let calls = s.paths().iter().filter(|p| p.ends_with("api-call")).count();
    assert_eq!(calls, 1, "one failed attempt, not a batch");
    assert!(cache::load(&e.cache_path()).auth_fail.is_some());
}

#[test]
fn upstream_failures_back_off_per_account_and_grow() {
    let l = many_codex(2);
    let s = Server::start(move |r| match r.path.as_str() {
        "/v8/management/credentials" => ok(l.to_string()),
        _ => status(502),
    });
    let e = Env::new(&s, "up-backoff");
    e.run(KEY, T0, &["codex"]);
    let a = cache::load(&e.cache_path()).accounts;
    assert_eq!((a[0].fails, a[0].retry_at), (1, T0 + 60));
    e.run(KEY, T0 + 61, &["codex"]);
    let a = cache::load(&e.cache_path()).accounts;
    assert_eq!((a[0].fails, a[0].retry_at), (2, T0 + 61 + 120));
    e.run(KEY, T0 + 61 + 120, &["codex"]);
    let a = cache::load(&e.cache_path()).accounts;
    assert_eq!((a[0].fails, a[0].retry_at), (3, T0 + 181 + 240));
}

#[test]
fn the_listing_and_a_hold_are_saved_before_the_upstream_phase() {
    use std::sync::{Arc, Mutex};
    let at: Arc<Mutex<Option<PathBuf>>> = Arc::new(Mutex::new(None));
    let seen: Arc<Mutex<Vec<(usize, i64)>>> = Arc::new(Mutex::new(vec![]));
    let (at2, seen2) = (at.clone(), seen.clone());
    let l = many_codex(2);
    let s = Server::start(move |r| match r.path.as_str() {
        "/v8/management/credentials" => ok(l.to_string()),
        _ => {
            if let Some(p) = at2.lock().unwrap().clone() {
                let c = cache::load(&p);
                seen2.lock().unwrap().push((c.accounts.len(), c.retry_at));
            }
            status(502)
        }
    });
    let e = Env::new(&s, "progressive");
    *at.lock().unwrap() = Some(e.cache_path());
    e.run(KEY, T0, &["codex"]);
    let seen = seen.lock().unwrap().clone();
    assert!(!seen.is_empty());
    assert!(
        seen.iter().all(|&(n, r)| n == 2 && r == T0 + 60),
        "{seen:?}"
    );
}

#[test]
fn nothing_is_sent_when_the_state_directory_cannot_be_written() {
    let s = Server::start(std_handler);
    let mut e = Env::new(&s, "readonly");
    let f = e.dir.join("not-a-dir");
    std::fs::write(&f, "x").unwrap();
    e.dir = f;
    let snap = e.run(KEY, T0, &["codex"]);
    assert_ne!(snap.status, Status::Ok);
    assert_eq!(s.count(), 0, "a refusal could not be remembered");
}

#[test]
fn the_key_digest_is_salted_and_short() {
    let s = Server::start(|_| status(401));
    let e = Env::new(&s, "salt");
    e.run(KEY, T0, &[]);
    let text = std::fs::read_to_string(e.cache_path()).unwrap();
    assert!(!text.contains(&bar_plugin::sha256::hex(KEY.as_bytes())));
    let c = cache::load(&e.cache_path());
    assert_eq!(c.auth_fail.unwrap().fp.len(), 32);
    assert_eq!(c.salt.len(), 32);
}

/// A shared flag.
#[derive(Clone)]
struct Arc2(std::sync::Arc<std::sync::atomic::AtomicBool>);

impl Arc2 {
    fn new(v: bool) -> Arc2 {
        Arc2(std::sync::Arc::new(std::sync::atomic::AtomicBool::new(v)))
    }
    fn get(&self) -> bool {
        self.0.load(std::sync::atomic::Ordering::SeqCst)
    }
    fn set(&self, v: bool) {
        self.0.store(v, std::sync::atomic::Ordering::SeqCst)
    }
}
