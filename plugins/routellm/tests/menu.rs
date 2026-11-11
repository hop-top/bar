#![cfg(unix)]
//! `--menu` end to end, from fixture caches: no proxy runs, so any call
//! would fail the test.

use bar_plugin::keyguard::Guard;
use bar_plugin::secret::Secret;
use bar_routellm::cache::{self, Alias, Cache, Entry};
use bar_routellm::config::{self, instance_id};
use serde_json::Value;
use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Stdio};

const BIN: &str = env!("CARGO_BIN_EXE_bar-routellm");
const KEY: &str = "mgmt-key-SECRET-AAAA";
const PROXY: &str = "http://127.0.0.1:9";

struct Home(std::path::PathBuf);

impl Home {
    fn new(name: &str) -> Home {
        let d = std::env::temp_dir().join(format!("rl-menu-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let key = d.join("key");
        std::fs::write(&key, format!("{KEY}\n")).unwrap();
        std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o600)).unwrap();
        let conf = d.join("routellm.conf");
        std::fs::write(
            &conf,
            format!(
                "instance {PROXY}\n routellm http://127.0.0.1:8\n key-file {}\n",
                key.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&conf, std::fs::Permissions::from_mode(0o600)).unwrap();
        Home(d)
    }

    fn state(&self) -> std::path::PathBuf {
        self.0.join("state")
    }

    fn id(&self) -> String {
        let cfg = config::load(&self.0.join("routellm.conf")).unwrap();
        instance_id(&cfg.instances[0].origin)
    }

    fn put(&self, c: &Cache) {
        let p = cache::file(&self.state(), PROXY);
        cache::save(&p, c).unwrap();
    }

    fn menu(&self) -> (Value, String) {
        let o = Command::new(BIN)
            .arg("--menu")
            .env("BAR_ROUTELLM_CONFIG", self.0.join("routellm.conf"))
            .env("BAR_STATE_DIR", self.state())
            .env_remove("ANTHROPIC_BASE_URL")
            .stdin(Stdio::null())
            .output()
            .unwrap();
        let text = String::from_utf8(o.stdout).unwrap();
        (serde_json::from_str(&text).unwrap(), text)
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

fn cache(disabled: bool, at: i64) -> Cache {
    Cache {
        entry: Some(Entry {
            disabled,
            priority: 1000,
            aliases: vec![Alias {
                alias: "route-auto".into(),
                tier: "auto".into(),
            }],
        }),
        at,
        up: Some(true),
        ..Default::default()
    }
}

fn item(v: &Value) -> &Value {
    &v["groups"][0]["items"][0]
}

#[test]
fn on_is_checked_and_the_click_turns_it_off() {
    let h = Home::new("on");
    h.put(&cache(false, now()));
    let (v, _) = h.menu();
    assert_eq!(v["groups"][0]["kind"], "check");
    let i = item(&v);
    assert_eq!(i["checked"], true);
    assert_eq!(i["action"], "toggle");
    assert_eq!(i["tokens"]["to"], "off");
    assert_eq!(i["tokens"]["instance"], h.id());
    assert_eq!(i["id"], h.id());
    assert_eq!(
        v["groups"].as_array().unwrap().len(),
        1,
        "no note when fine"
    );
}

#[test]
fn off_is_unchecked_and_the_click_turns_it_on() {
    let h = Home::new("off");
    h.put(&cache(true, now()));
    let (v, _) = h.menu();
    let i = item(&v);
    assert!(i["checked"].is_null() || i["checked"] == false);
    assert_eq!(i["tokens"]["to"], "on");
}

#[test]
fn a_refused_key_is_a_disabled_item_and_a_note() {
    let h = Home::new("refused");
    h.put(&cache(false, now()));
    let key = Secret::new(KEY.as_bytes().to_vec());
    let cfg = config::load(&h.0.join("routellm.conf")).unwrap();
    let g = Guard {
        state_dir: &h.state(),
        mgmt: &cfg.instances[0].management,
    };
    assert!(g.stop(401, &key, now()));
    let (v, _) = h.menu();
    assert_eq!(item(&v)["enabled"], false);
    let note = &v["groups"][1];
    assert_eq!(note["kind"], "info");
    let label = note["items"][0]["label"].as_str().unwrap();
    assert!(label.contains("refused the key (401)"), "{label}");
    assert_eq!(note["items"][0]["style"], "bad");
}

#[test]
fn a_stale_cache_is_dimmed_and_still_switchable() {
    let h = Home::new("stale");
    h.put(&cache(false, now() - 3600));
    let (v, _) = h.menu();
    let i = item(&v);
    assert_eq!(i["detail"], "stale");
    assert_eq!(i["style"], "dim");
    assert_eq!(i["checked"], true);
    assert!(i["enabled"].is_null() || i["enabled"] == true);
}

#[test]
fn no_cache_is_disabled_with_a_note() {
    let h = Home::new("empty");
    let (v, _) = h.menu();
    assert_eq!(item(&v)["enabled"], false);
    assert!(v["groups"][1]["items"][0]["label"]
        .as_str()
        .unwrap()
        .contains("not read yet"));
}

#[test]
fn routellm_down_is_checked_with_a_note() {
    let h = Home::new("down");
    let mut c = cache(false, now());
    c.up = Some(false);
    h.put(&c);
    let (v, _) = h.menu();
    assert_eq!(item(&v)["checked"], true);
    assert!(v["groups"][1]["items"][0]["label"]
        .as_str()
        .unwrap()
        .contains("not answering"));
}

#[test]
fn the_output_never_holds_the_key() {
    let h = Home::new("secret");
    h.put(&cache(false, now()));
    let (_, text) = h.menu();
    assert!(!text.contains("SECRET"), "{text}");
}

#[test]
fn ext_info_declares_the_menu_capability() {
    let o = Command::new(BIN).arg("--ext-info").output().unwrap();
    let v: Value = serde_json::from_slice(&o.stdout).unwrap();
    assert!(v["capabilities"]
        .as_array()
        .unwrap()
        .iter()
        .any(|c| c == "menu"));
}
