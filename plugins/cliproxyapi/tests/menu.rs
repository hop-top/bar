#![cfg(unix)]
//! `--menu` against fixture caches: no proxy is listening, so any call
//! would fail the test; the menu must come from the cache files.

use bar_cliproxyapi::cache::{self, AuthFail, Cache};
use bar_cliproxyapi::model::{Acct, Win};
use bar_cliproxyapi::switch::instance_id;
use bar_plugin::secret::Secret;
use serde_json::{json, Value};
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Stdio};

const BIN: &str = env!("CARGO_BIN_EXE_bar-cliproxyapi");
const KEY: &str = "menu-secret-key-0123456789";
const A: &str = "http://127.0.0.1:1";
const B: &str = "http://127.0.0.1:2";

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

struct Home(std::path::PathBuf);

impl Home {
    fn new(name: &str) -> Home {
        let d = std::env::temp_dir().join(format!("cpa-menu-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        Home(d)
    }

    fn conf(&self, origins: &[&str]) -> std::path::PathBuf {
        let key = self.0.join("key");
        std::fs::write(&key, format!("{KEY}\n")).unwrap();
        std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o600)).unwrap();
        let text: String = origins
            .iter()
            .map(|o| {
                format!(
                    "instance {o}\n key-file {}\n providers claude,codex\n",
                    key.display()
                )
            })
            .collect();
        let p = self.0.join("c.conf");
        std::fs::write(&p, text).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600)).unwrap();
        p
    }

    fn state(&self) -> std::path::PathBuf {
        self.0.join("state")
    }

    fn cache(&self, origin: &str, c: &Cache) {
        cache::save(&cache::file(&self.state(), origin), c).unwrap();
    }

    fn menu(&self, conf: &std::path::Path) -> (String, Value) {
        let mut ch = Command::new(BIN)
            .arg("--menu")
            .env("BAR_CLIPROXYAPI_CONFIG", conf)
            .env("BAR_STATE_DIR", self.state())
            .env_remove("ANTHROPIC_BASE_URL")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        ch.stdin.take().unwrap().write_all(b"{}").unwrap();
        let out = ch.wait_with_output().unwrap();
        assert!(out.status.success());
        let raw = String::from_utf8(out.stdout).unwrap();
        let v = serde_json::from_str(&raw).unwrap();
        (raw, v)
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn acct(key: &str, provider: &str, label: &str, priority: i64, at: i64) -> Acct {
    Acct {
        key: key.into(),
        provider: provider.into(),
        label: label.into(),
        color: None,
        priority,
        plan: None,
        windows: vec![Win {
            id: "5h".into(),
            used: 59.0,
            resets_at: now() + 7800,
            period_s: 18_000,
        }],
        at,
        disabled: false,
        unavailable: false,
        upstream_auth: false,
        retry_at: 0,
        fails: 0,
    }
}

fn group<'a>(v: &'a Value, id: &str) -> &'a Value {
    v["groups"]
        .as_array()
        .unwrap()
        .iter()
        .find(|g| g["id"] == id)
        .unwrap_or_else(|| panic!("no group {id} in {v}"))
}

fn ids(v: &Value) -> Vec<String> {
    v["groups"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| g["id"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn ext_info_declares_menu() {
    let out = Command::new(BIN).arg("--ext-info").output().unwrap();
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(v["capabilities"]
        .as_array()
        .unwrap()
        .contains(&json!("menu")));
}

#[test]
fn lists_every_instance_and_provider_from_the_caches() {
    let h = Home::new("multi");
    let t = now();
    h.cache(
        A,
        &Cache {
            accounts: vec![
                acct("ix1", "claude", "alice", 5, t),
                acct("ix2", "claude", "bob", 0, t),
                acct("cx1", "codex", "carol", 0, t),
            ],
            ..Default::default()
        },
    );
    h.cache(
        B,
        &Cache {
            accounts: vec![acct("ix9", "claude", "dave", 0, t)],
            ..Default::default()
        },
    );
    let (_, v) = h.menu(&h.conf(&[A, B]));
    let (ia, ib) = (instance_id(A), instance_id(B));
    assert_eq!(
        ids(&v),
        [
            format!("claude@{ia}"),
            format!("go-claude@{ia}"),
            format!("codex@{ia}"),
            format!("go-codex@{ia}"),
            format!("claude@{ib}"),
            format!("go-claude@{ib}"),
        ]
    );
    let g = group(&v, &format!("claude@{ia}"));
    assert_eq!(g["kind"], "radio");
    assert_eq!(g["label"], "Claude \u{B7} 127.0.0.1:1");
    let items = g["items"].as_array().unwrap();
    assert_eq!(items[0]["label"], "alice");
    assert_eq!(items[0]["checked"], true, "the top priority account");
    assert!(items[1]["checked"].is_null());
    assert_eq!(items[0]["action"], "use");
    assert_eq!(
        items[0]["tokens"],
        json!({"account":"ix1","instance":ia,"provider":"claude"})
    );
    assert!(items[0]["detail"]
        .as_str()
        .unwrap()
        .starts_with("5h 59% used"));
    let go = group(&v, &format!("go-codex@{ia}"));
    assert_eq!(go["kind"], "buttons");
    assert_eq!(go["items"][0]["action"], "next");
    assert_eq!(go["items"][1]["action"], "next-soon");
    assert_eq!(
        go["items"][1]["tokens"],
        json!({"instance": ia, "provider": "codex"})
    );
    // a tied top tier has no preferred account
    let g = group(&v, &format!("claude@{ib}"));
    assert_eq!(g["items"][0]["checked"], true);
}

#[test]
fn tokens_match_the_declared_actions() {
    let h = Home::new("decl");
    h.cache(
        A,
        &Cache {
            accounts: vec![acct("ix1", "claude", "alice", 0, now())],
            ..Default::default()
        },
    );
    let (_, v) = h.menu(&h.conf(&[A]));
    let info = Command::new(BIN).arg("--ext-info").output().unwrap();
    let info: Value = serde_json::from_slice(&info.stdout).unwrap();
    for g in v["groups"].as_array().unwrap() {
        for i in g["items"].as_array().unwrap() {
            let a = i["action"].as_str().unwrap();
            let mut have: Vec<&String> = i["tokens"].as_object().unwrap().keys().collect();
            have.sort();
            let mut want: Vec<String> = info["actions"][a]["tokens"]
                .as_array()
                .unwrap_or_else(|| panic!("{a} is not declared"))
                .iter()
                .map(|t| t.as_str().unwrap().to_string())
                .collect();
            want.sort();
            assert_eq!(have, want.iter().collect::<Vec<_>>(), "{a}");
        }
    }
    assert!(!v.to_string().contains("\"pick\""), "pick stays out");
}

#[test]
fn a_stale_cache_is_marked() {
    let h = Home::new("stale");
    h.cache(
        A,
        &Cache {
            accounts: vec![acct("ix1", "claude", "alice", 0, now() - 4000)],
            ..Default::default()
        },
    );
    let (_, v) = h.menu(&h.conf(&[A]));
    let i = &group(&v, &format!("claude@{}", instance_id(A)))["items"][0];
    assert!(i["detail"].as_str().unwrap().contains("stale"));
    assert_eq!(i["style"], "dim");
}

#[test]
fn a_refusal_record_is_shown_and_honoured() {
    let h = Home::new("refused");
    let mut c = Cache {
        accounts: vec![acct("ix1", "claude", "alice", 0, now())],
        ..Default::default()
    };
    let fp = c.fp(&Secret::new(KEY.as_bytes().to_vec()));
    c.auth_fail = Some(AuthFail {
        status: 401,
        at: now(),
        fp,
    });
    h.cache(A, &c);
    let (_, v) = h.menu(&h.conf(&[A]));
    let info = group(&v, &format!("info@{}", instance_id(A)));
    assert_eq!(info["kind"], "info");
    let row = &info["items"][0];
    assert!(row["label"].as_str().unwrap().contains("401"));
    assert_eq!(row["style"], "bad");
    // the cached accounts stay listed
    group(&v, &format!("claude@{}", instance_id(A)));
}

#[test]
fn backoff_and_empty_caches_get_an_info_row() {
    let h = Home::new("backoff");
    h.cache(
        A,
        &Cache {
            retry_at: now() + 600,
            accounts: vec![acct("ix1", "claude", "alice", 0, now())],
            ..Default::default()
        },
    );
    let (_, v) = h.menu(&h.conf(&[A, B]));
    let a = group(&v, &format!("info@{}", instance_id(A)));
    assert_eq!(a["items"][0]["style"], "warn");
    let b = group(&v, &format!("info@{}", instance_id(B)));
    assert_eq!(b["items"][0]["label"], "no accounts cached yet");
    assert!(!ids(&v).contains(&format!("go-claude@{}", instance_id(B))));
}

#[test]
fn no_secret_reaches_the_output() {
    let h = Home::new("secret");
    let mut c = Cache {
        accounts: vec![acct("ix1", "claude", "alice", 0, now())],
        ..Default::default()
    };
    let fp = c.fp(&Secret::new(KEY.as_bytes().to_vec()));
    c.auth_fail = Some(AuthFail {
        status: 403,
        at: now(),
        fp: fp.clone(),
    });
    h.cache(A, &c);
    let conf = h.conf(&[A]);
    let (raw, _) = h.menu(&conf);
    for secret in [KEY, fp.as_str(), c.salt.as_str(), conf.to_str().unwrap()] {
        assert!(!raw.contains(secret), "{secret} leaked into {raw}");
    }
}

#[test]
fn no_config_or_no_cache_is_an_empty_menu() {
    let h = Home::new("none");
    let (_, v) = h.menu(&h.0.join("missing.conf"));
    assert_eq!(v, json!({"groups": []}));
}
