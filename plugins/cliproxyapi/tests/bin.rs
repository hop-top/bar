#![cfg(unix)]
//! The binary end to end: `--ext-info`, `--check` and a render against a
//! fake management server.

mod fake;

use fake::{ok, Server};
use serde_json::{json, Value};
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Stdio};

const BIN: &str = env!("CARGO_BIN_EXE_bar-cliproxyapi");

struct Home(std::path::PathBuf);

impl Home {
    fn new(name: &str) -> Home {
        let d = std::env::temp_dir().join(format!("cpa-bin-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        Home(d)
    }
    fn file(&self, name: &str, text: &str, mode: u32) -> std::path::PathBuf {
        let p = self.0.join(name);
        std::fs::write(&p, text).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(mode)).unwrap();
        p
    }
    fn cmd(&self, conf: &std::path::Path) -> Command {
        let mut c = Command::new(BIN);
        c.env("BAR_CLIPROXYAPI_CONFIG", conf)
            .env("BAR_STATE_DIR", self.0.join("state"))
            .env_remove("ANTHROPIC_BASE_URL");
        c
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn render(mut c: Command, input: Value) -> Value {
    let mut ch = c
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    ch.stdin
        .take()
        .unwrap()
        .write_all(input.to_string().as_bytes())
        .unwrap();
    let out = ch.wait_with_output().unwrap();
    assert!(out.status.success());
    serde_json::from_slice(&out.stdout).unwrap()
}

#[test]
fn ext_info_declares_the_protocol_extensions() {
    let out = Command::new(BIN).arg("--ext-info").output().unwrap();
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["name"], "cliproxyapi");
    assert_eq!(v["ttl"], "60s");
    assert_eq!(v["timeout"], "15s");
    assert_eq!(v["inputs"]["env"][0], "ANTHROPIC_BASE_URL");
    assert_eq!(v["inputs"]["session"], json!(["model_id", "model"]));
    let caps: Vec<&str> = v["capabilities"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert!(caps.contains(&"placeholder") && caps.contains(&"check"));
}

#[test]
fn renders_only_for_the_configured_instance() {
    let listing = json!({"files":[{"auth_index":"ix1","type":"claude","email":"a@corp.test",
        "quota":{"signals":{"Anthropic-Ratelimit-Unified-7d-Utilization":"0.5",
                            "Anthropic-Ratelimit-Unified-7d-Reset":"4102444800"}}}]});
    let s = Server::start(move |_| ok(listing.to_string()));
    let h = Home::new("render");
    let key = h.file("key", "bin-test-key\n", 0o600);
    let conf = h.file(
        "c.conf",
        &format!(
            "instance {}\n key-file {}\n alias \"*\" me\n",
            s.origin(),
            key.display()
        ),
        0o600,
    );
    let input = |url: &str, model: &str| json!({"options":{"label":"alias"},"session":{"model_id":model},"env":{"ANTHROPIC_BASE_URL":url}});
    let o = render(
        h.cmd(&conf),
        input(&format!("{}/", s.origin()), "claude-opus-4"),
    );
    let it = &o["items"][0];
    assert_eq!(it["label"], "me");
    assert!(it["text"].as_str().unwrap().starts_with("7d 50% "), "{it}");
    assert_eq!(it["tokens"], json!({"account":"ix1","provider":"claude"}));
    // a pool of one: usage and the preferred account ride along
    assert_eq!(o["usage"]["seven_day"]["used_percentage"], 50.0);
    assert_eq!(o["account"], json!({"id":"ix1","label":"me"}));
    // another base URL, or a non-matching model: hidden, no request
    let n = s.count();
    assert_eq!(
        render(h.cmd(&conf), input("http://127.0.0.1:1", "claude-opus-4"))["text"],
        ""
    );
    assert_eq!(
        render(h.cmd(&conf), input(&s.origin(), "grok-4"))["text"],
        ""
    );
    assert_eq!(s.count(), n);
    // the cache file lands in bar's state dir, owner-only
    let cache_dir = h.0.join("state/plugins/cliproxyapi");
    assert_eq!(std::fs::read_dir(cache_dir).unwrap().count(), 1);
}

#[test]
fn options_shape_the_items_and_the_pool() {
    let listing = json!({"files":[
        {"auth_index":"ix1","type":"claude","email":"a@corp.test","priority":2,
         "quota":{"signals":{"Anthropic-Ratelimit-Unified-5h-Utilization":"0.2",
                             "Anthropic-Ratelimit-Unified-5h-Reset":"4102444800",
                             "Anthropic-Ratelimit-Unified-7d-Utilization":"0.5",
                             "Anthropic-Ratelimit-Unified-7d-Reset":"4102444800"}}},
        {"auth_index":"ix2","type":"claude","email":"b@corp.test",
         "quota":{"signals":{"Anthropic-Ratelimit-Unified-5h-Utilization":"0.6",
                             "Anthropic-Ratelimit-Unified-5h-Reset":"4102444800",
                             "Anthropic-Ratelimit-Unified-7d-Utilization":"0.1",
                             "Anthropic-Ratelimit-Unified-7d-Reset":"4102444800"}}}]});
    let s = Server::start(move |_| ok(listing.to_string()));
    let h = Home::new("opts");
    let key = h.file("key", "bin-test-key\n", 0o600);
    let conf = h.file(
        "c.conf",
        &format!("instance {}\n key-file {}\n", s.origin(), key.display()),
        0o600,
    );
    let go = |opts: Value| {
        render(
            h.cmd(&conf),
            json!({"options":opts,"session":{"model_id":"claude-opus-4"},"env":{"ANTHROPIC_BASE_URL":s.origin()}}),
        )
    };
    let o = go(json!({"windows":"5h","count":"used","sort":"name","pool":"mean"}));
    let items = o["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(
        (items[0]["label"].as_str(), items[1]["label"].as_str()),
        (Some("a"), Some("b"))
    );
    assert!(items[0]["text"].as_str().unwrap().starts_with("5h 20% "));
    assert_eq!(o["usage"]["five_hour"]["used_percentage"], 40.0);
    assert_eq!(o["usage"]["seven_day"]["used_percentage"], 30.0);
    // the pool option changes the usage, not the items
    assert_eq!(
        go(json!({"pool":"max"}))["usage"]["five_hour"]["used_percentage"],
        60.0
    );
    assert_eq!(
        go(json!({"pool":"active"}))["usage"]["five_hour"]["used_percentage"],
        20.0
    );
    // ix1's priority tier is its own: it is the preferred account
    assert_eq!(o["account"]["id"], "ix1");
    // a link template, and the account filter
    let o = go(json!({"accounts":"b","link":"x://use?a={account}"}));
    assert_eq!(o["items"].as_array().unwrap().len(), 1);
    assert_eq!(o["items"][0]["link"], "x://use?a=ix2");
}

#[test]
fn a_bad_key_file_never_reaches_the_network() {
    let s = Server::start(|_| ok("{}"));
    let h = Home::new("badkey");
    let key = h.file("key", "bin-test-key\n", 0o644);
    let conf = h.file(
        "c.conf",
        &format!("instance {}\n key-file {}\n", s.origin(), key.display()),
        0o600,
    );
    let o = render(
        h.cmd(&conf),
        json!({"session":{"model_id":"claude-x"},"env":{"ANTHROPIC_BASE_URL":s.origin()}}),
    );
    assert_eq!(
        (o["text"].as_str(), o["style"].as_str()),
        (Some("key?"), Some("bad"))
    );
    assert_eq!(s.count(), 0);
}

#[test]
fn a_loose_config_file_hides_the_segment_and_check_says_why() {
    let h = Home::new("check");
    let conf = h.file(
        "c.conf",
        "instance http://127.0.0.1:1\n key-file /k\n",
        0o644,
    );
    let o = render(
        h.cmd(&conf),
        json!({"env":{"ANTHROPIC_BASE_URL":"http://127.0.0.1:1"}}),
    );
    assert_eq!(o["text"], "");
    let out = h.cmd(&conf).arg("--check").output().unwrap();
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    let w = v["warnings"][0].as_str().unwrap();
    assert!(w.contains("mode 0644"), "{w}");
}
