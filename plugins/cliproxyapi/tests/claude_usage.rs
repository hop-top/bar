//! Claude accounts' usage read from Anthropic's OAuth usage endpoint
//! through the proxy's `api-call`, against a fake management server that
//! stands in for the proxy (it holds the account token; the plugin never
//! does).

#[cfg(unix)]
#[path = "../../../tests/common/deadline.rs"]
mod deadline;
mod fake;

use bar_cliproxyapi::cache;
use bar_cliproxyapi::collect::{collect, Ctx, Snapshot, TTL_UPSTREAM};
use bar_cliproxyapi::config::{self, Instance};
use bar_plugin::secret::Secret;
use fake::{ok, status, Rec, Reply, Server};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

const T0: i64 = 1_800_000_000;
const KEY: &str = "test-key-AAAA";
/// The account's OAuth token, known to the fake proxy only.
const TOKEN: &str = "sk-ant-oat01-FAKE-TOKEN-zzzz";
const USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";

struct Env {
    dir: PathBuf,
    inst: Instance,
}

fn key_file() -> &'static str {
    if cfg!(windows) {
        "C:/k"
    } else {
        "/k"
    }
}

impl Env {
    fn new(s: &Server, name: &str, extra: &str) -> Env {
        let dir = std::env::temp_dir().join(format!("cpa-claude-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let text = format!("instance {}\n key-file {}\n{extra}", s.origin(), key_file());
        let mut cfg = config::parse(&text, None);
        assert!(cfg.errors.is_empty(), "{:?}", cfg.errors);
        Env {
            dir,
            inst: cfg.instances.remove(0),
        }
    }

    fn run(&self, now: i64) -> Snapshot {
        collect(&Ctx {
            inst: &self.inst,
            key: &Secret::new(KEY.as_bytes().to_vec()),
            state_dir: &self.dir,
            now,
            fetch: &["claude"],
        })
    }

    fn cache_text(&self) -> String {
        std::fs::read_to_string(cache::file(&self.dir, &self.inst.origin.to_string())).unwrap()
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn signals(u7: &str) -> Value {
    json!({"Anthropic-Ratelimit-Unified-7d-Utilization": u7,
           "Anthropic-Ratelimit-Unified-7d-Reset": (T0 + 86_400).to_string()})
}

/// One Claude account, with the given passive signals.
fn listing(sig: &Value) -> Value {
    json!({"files":[{"id":"a.json","auth_index":"ix-claude-1","type":"claude",
        "email":"alice.synthetic@corp.test","quota":{"signals":sig}}]})
}

fn usage(five: Value, seven: Value) -> Value {
    json!({"five_hour":five,"seven_day":seven,"seven_day_opus":null,
           "extra_usage":{"is_enabled":false}})
}

/// The upstream reply the fake proxy relays for one api-call.
#[derive(Clone)]
struct Up {
    status: u16,
    header: Value,
    body: String,
}

fn up_ok(body: Value) -> Up {
    Up {
        status: 200,
        header: json!({"Content-Type":["application/json"]}),
        body: body.to_string(),
    }
}

/// A fake proxy: a settable listing and upstream reply. It does what the
/// real one does with `$TOKEN$` (substitutes the account's token into
/// the upstream request, recorded in `sent`) and, to be hostile, echoes
/// the token back in the reply headers.
struct Proxy {
    server: Server,
    listing: Arc<Mutex<Value>>,
    up: Arc<Mutex<Up>>,
    sent: Arc<Mutex<Vec<Value>>>,
}

fn proxy(sig: Value, up: Up) -> Proxy {
    let listing = Arc::new(Mutex::new(listing(&sig)));
    let up = Arc::new(Mutex::new(up));
    let sent = Arc::new(Mutex::new(Vec::new()));
    let (l, u, st) = (listing.clone(), up.clone(), sent.clone());
    let server = Server::start(move |r: &Rec| -> Reply {
        match (r.method.as_str(), r.path.as_str()) {
            ("GET", "/v8/management/credentials") => ok(l.lock().unwrap().to_string()),
            ("POST", "/v8/management/requests/api-call") => {
                let call: Value = serde_json::from_str(&r.body).unwrap_or_default();
                let mut upstream = call.clone();
                if let Some(h) = upstream["header"].as_object_mut() {
                    for v in h.values_mut() {
                        let s = v.as_str().unwrap_or("").replace("$TOKEN$", TOKEN);
                        *v = Value::from(s);
                    }
                }
                st.lock().unwrap().push(upstream);
                let u = u.lock().unwrap().clone();
                let mut header = u.header.clone();
                header["X-Echo-Authorization"] = json!([format!("Bearer {TOKEN}")]);
                ok(json!({"status_code":u.status,"header":header,"body":u.body}).to_string())
            }
            _ => status(404),
        }
    });
    Proxy {
        server,
        listing,
        up,
        sent,
    }
}

impl Proxy {
    fn calls(&self) -> usize {
        self.server
            .paths()
            .iter()
            .filter(|p| p.ends_with("api-call"))
            .count()
    }
    fn set_signals(&self, sig: Value) {
        *self.listing.lock().unwrap() = listing(&sig);
    }
    fn set_up(&self, up: Up) {
        *self.up.lock().unwrap() = up;
    }
}

fn wins(s: &Snapshot) -> Vec<(String, f64, i64)> {
    s.accounts[0]
        .windows
        .iter()
        .map(|w| (w.id.clone(), w.used, w.resets_at))
        .collect()
}

#[test]
fn an_idle_account_gets_its_figures_from_the_oauth_read() {
    let p = proxy(
        json!({}),
        up_ok(usage(
            json!({"utilization":12.0,"resets_at":"2027-01-15T10:00:00.123456+00:00"}),
            json!({"utilization":40,"resets_at":"2027-01-20T00:00:00Z"}),
        )),
    );
    let e = Env::new(&p.server, "idle", "");
    let snap = e.run(T0);
    assert_eq!(
        wins(&snap),
        [
            ("5h".to_string(), 12.0, 1_800_007_200),
            ("7d".to_string(), 40.0, 1_800_403_200)
        ]
    );
    assert_eq!(snap.accounts[0].at, T0, "captured now");
    assert_eq!(p.calls(), 1);
    // the request names the account, the method, the compiled-in URL and
    // the beta header; the token is the proxy's placeholder, nothing more
    let log = p.server.log.lock().unwrap().clone();
    let call: Value = serde_json::from_str(&log[1].body).unwrap();
    let keys: Vec<&str> = call
        .as_object()
        .unwrap()
        .keys()
        .map(|k| k.as_str())
        .collect();
    assert_eq!(keys, ["auth_index", "method", "url", "header"]);
    assert_eq!(call["auth_index"], "ix-claude-1");
    assert_eq!(call["method"], "GET");
    assert_eq!(call["url"], USAGE_URL);
    assert_eq!(
        call["header"],
        json!({"Authorization":"Bearer $TOKEN$","anthropic-beta":"oauth-2025-04-20"})
    );
}

#[test]
fn a_null_window_is_unused_with_no_reset() {
    let p = proxy(
        json!({}),
        up_ok(usage(
            Value::Null,
            json!({"utilization":"7.5","resets_at":1_800_100_000}),
        )),
    );
    let e = Env::new(&p.server, "null5h", "");
    assert_eq!(
        wins(&e.run(T0)),
        [
            ("5h".to_string(), 0.0, 0),
            ("7d".to_string(), 7.5, 1_800_100_000)
        ]
    );
}

#[test]
fn fresher_passive_headers_win_over_the_last_oauth_read() {
    let p = proxy(
        json!({}),
        up_ok(usage(
            Value::Null,
            json!({"utilization":10,"resets_at":null}),
        )),
    );
    let e = Env::new(&p.server, "fresher", "");
    e.run(T0);
    assert_eq!(p.calls(), 1);
    // new traffic through the proxy: its headers are newer than the read
    p.set_signals(signals("0.5"));
    let snap = e.run(T0 + TTL_UPSTREAM + 10);
    assert_eq!(wins(&snap), [("7d".to_string(), 50.0, T0 + 86_400)]);
    assert_eq!(snap.accounts[0].at, T0 + TTL_UPSTREAM + 10);
    assert_eq!(p.calls(), 1, "fresh passive data: no read");

    // the same headers later are as old as when first seen: the read is
    // newer, and wins
    p.set_up(up_ok(usage(
        json!({"utilization":3,"resets_at":null}),
        json!({"utilization":77,"resets_at":null}),
    )));
    let t2 = T0 + 3 * TTL_UPSTREAM;
    let snap = e.run(t2);
    assert_eq!(p.calls(), 2);
    assert_eq!(
        wins(&snap),
        [("5h".to_string(), 3.0, 0), ("7d".to_string(), 77.0, 0)]
    );
    let snap = e.run(t2 + 60);
    assert_eq!(p.calls(), 2);
    assert_eq!(snap.accounts[0].windows[1].used, 77.0, "stale headers lose");
    assert_eq!(snap.accounts[0].at, t2, "the read's capture time");
}

#[test]
fn one_read_per_account_per_ttl() {
    let p = proxy(
        json!({}),
        up_ok(usage(
            Value::Null,
            json!({"utilization":1,"resets_at":null}),
        )),
    );
    let e = Env::new(&p.server, "ttl", "");
    for dt in [0, 1, 60, TTL_UPSTREAM - 1] {
        e.run(T0 + dt);
    }
    assert_eq!(p.calls(), 1);
    e.run(T0 + TTL_UPSTREAM);
    assert_eq!(p.calls(), 2);
}

#[test]
fn a_refused_or_bad_read_keeps_the_last_value_and_backs_off() {
    let good = up_ok(usage(
        Value::Null,
        json!({"utilization":20,"resets_at":null}),
    ));
    for (name, bad, hold) in [
        (
            "429",
            Up {
                status: 429,
                header: json!({"Retry-After":["900"]}),
                body: "{}".into(),
            },
            900,
        ),
        (
            "403",
            Up {
                status: 403,
                header: json!({}),
                body: r#"{"error":{"type":"permission_error"}}"#.into(),
            },
            3600,
        ),
        ("garbage", up_ok(json!({"type":"error"})), 60),
    ] {
        let p = proxy(json!({}), good.clone());
        let e = Env::new(&p.server, &format!("bad{name}"), "");
        e.run(T0);
        p.set_up(bad);
        let t1 = T0 + TTL_UPSTREAM;
        let snap = e.run(t1);
        assert_eq!(p.calls(), 2, "{name}");
        let a = &snap.accounts[0];
        assert_eq!(
            wins(&snap),
            [("5h".into(), 0.0, 0), ("7d".into(), 20.0, 0)],
            "{name}"
        );
        assert_eq!(a.at, T0, "{name}: the value's own capture time");
        assert!(
            !a.upstream_auth,
            "{name}: the account still serves requests"
        );
        // no read inside the hold, one after it
        e.run(t1 + hold - 1);
        assert_eq!(p.calls(), 2, "{name}");
        p.set_up(good.clone());
        e.run(t1 + hold);
        assert_eq!(p.calls(), 3, "{name}");
    }
}

#[test]
fn headers_mode_never_reads_upstream() {
    let p = proxy(
        json!({}),
        up_ok(usage(
            Value::Null,
            json!({"utilization":1,"resets_at":null}),
        )),
    );
    let e = Env::new(&p.server, "headers", " claude-usage headers\n");
    let snap = e.run(T0);
    assert!(snap.accounts[0].windows.is_empty());
    e.run(T0 + 10 * TTL_UPSTREAM);
    assert_eq!(p.calls(), 0);
    // headers are shown as before
    p.set_signals(signals("0.3"));
    assert_eq!(wins(&e.run(T0 + 11 * TTL_UPSTREAM))[0].1, 30.0);
}

#[test]
fn a_codex_model_reads_no_claude_usage() {
    let p = proxy(
        json!({}),
        up_ok(usage(
            Value::Null,
            json!({"utilization":1,"resets_at":null}),
        )),
    );
    let e = Env::new(&p.server, "codexonly", "");
    collect(&Ctx {
        inst: &e.inst,
        key: &Secret::new(KEY.as_bytes().to_vec()),
        state_dir: &e.dir,
        now: T0,
        fetch: &["codex"],
    });
    assert_eq!(p.calls(), 0);
}

#[test]
fn the_token_stays_with_the_proxy() {
    let p = proxy(
        json!({}),
        up_ok(usage(
            Value::Null,
            json!({"utilization":1,"resets_at":null}),
        )),
    );
    let e = Env::new(&p.server, "token", "");
    e.run(T0);
    p.set_up(Up {
        status: 401,
        header: json!({}),
        body: format!(r#"{{"error":"bad token {TOKEN}"}}"#),
    });
    e.run(T0 + TTL_UPSTREAM);
    assert_eq!(p.calls(), 2);
    // the proxy did put the token in the upstream request...
    let sent = p.sent.lock().unwrap().clone();
    assert_eq!(
        sent[0]["header"]["Authorization"],
        format!("Bearer {TOKEN}")
    );
    // ...but nothing the plugin sent, or kept, has it
    for r in p.server.log.lock().unwrap().iter() {
        assert!(
            !r.body.contains(TOKEN) && !r.auth.contains("oat01"),
            "{r:?}"
        );
    }
    let text = e.cache_text();
    assert!(!text.contains(TOKEN) && !text.contains("oat01") && !text.contains("TOKEN"));
    assert!(!text.contains("corp.test") && !text.contains('@'));
}

/// The binary end to end: a render with a Claude model reads the idle
/// account through the proxy; the token is in no request, no cache file
/// and no output.
#[cfg(unix)]
#[test]
fn a_render_reads_usage_and_never_shows_the_token() {
    use std::os::unix::fs::PermissionsExt;
    let p = proxy(
        json!({}),
        up_ok(usage(
            json!({"utilization":25,"resets_at":"2099-01-01T00:00:00Z"}),
            json!({"utilization":50,"resets_at":"2099-01-02T00:00:00Z"}),
        )),
    );
    let home = std::env::temp_dir().join(format!("cpa-claude-bin-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(&home).unwrap();
    let write = |name: &str, text: &str| {
        let f = home.join(name);
        std::fs::write(&f, text).unwrap();
        std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o600)).unwrap();
        f
    };
    let key = write("key", &format!("{KEY}\n"));
    let conf = write(
        "c.conf",
        &format!(
            "instance {}\n key-file {}\n",
            p.server.origin(),
            key.display()
        ),
    );
    let mut c = std::process::Command::new(env!("CARGO_BIN_EXE_bar-cliproxyapi"));
    c.env("BAR_CLIPROXYAPI_CONFIG", &conf)
        .env("BAR_STATE_DIR", home.join("state"))
        .env("PATH", "/usr/bin:/bin")
        .env_remove("ANTHROPIC_BASE_URL");
    let input = json!({"options":{"count":"used"},"session":{"model_id":"claude-opus-4"},
        "env":{"ANTHROPIC_BASE_URL":p.server.origin()}});
    let out = deadline::run(&mut c, Some(input.to_string().as_bytes()));
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    let o: Value = serde_json::from_str(&stdout).unwrap();
    let text = o["items"][0]["text"].as_str().unwrap_or("");
    assert!(text.contains("25%") && text.contains("50%"), "{o}");
    assert_eq!(p.calls(), 1);
    for s in [
        stdout.as_ref(),
        String::from_utf8_lossy(&out.stderr).as_ref(),
    ] {
        assert!(!s.contains(TOKEN) && !s.contains("oat01"));
    }
    for e in std::fs::read_dir(home.join("state/plugins/cliproxyapi")).unwrap() {
        let t = std::fs::read(e.unwrap().path()).unwrap();
        assert!(!String::from_utf8_lossy(&t).contains("oat01"));
    }
    for r in p.server.log.lock().unwrap().iter() {
        assert!(!r.body.contains("oat01"), "{r:?}");
    }
    let _ = std::fs::remove_dir_all(&home);
}
