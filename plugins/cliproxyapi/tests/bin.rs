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
    // `pick` needs a chooser: declared on macOS only, so `bar check` can
    // flag a binding of it elsewhere
    assert_eq!(
        v["actions"]["next"]["tokens"],
        json!(["instance", "provider"])
    );
    assert_eq!(
        v["actions"].get("pick").is_some(),
        cfg!(target_os = "macos")
    );
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
    assert!(it["text"].as_str().unwrap().starts_with("50% "), "{it}");
    assert_eq!(it["tokens"], json!({"account":"ix1","provider":"claude"}));
    // a pool of one: usage and the preferred account ride along
    assert_eq!(o["usage"]["seven_day"]["used_percentage"], 50.0);
    assert_eq!(o["account"], json!({"id":"ix1","label":"me"}));
    // what the `next` actions take, for a link on `#{account}`
    assert_eq!(o["tokens"]["provider"], "claude");
    assert_eq!(
        o["tokens"]["instance"],
        bar_cliproxyapi::switch::instance_id(&s.origin())
    );
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
    let names: Vec<String> = std::fs::read_dir(cache_dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names.iter().filter(|n| n.ends_with(".json")).count(), 1);
    assert_eq!(names.iter().filter(|n| n.ends_with(".lock")).count(), 1);
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
    assert!(items[0]["text"].as_str().unwrap().starts_with("20% "));
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

#[test]
fn concurrent_processes_cost_one_failed_attempt_per_key() {
    let s = Server::start(|_| {
        std::thread::sleep(std::time::Duration::from_millis(400));
        fake::status(401)
    });
    let h = Home::new("race");
    let key = h.file("key", "wrong-key\n", 0o600);
    let conf = h.file(
        "c.conf",
        &format!("instance {}\n key-file {}\n", s.origin(), key.display()),
        0o600,
    );
    let url = s.origin();
    let hs: Vec<_> = (0..6)
        .map(|i| {
            let mut c = h.cmd(&conf);
            // different cwd/option sets in the host: same instance here
            let input = json!({"options":{"label":if i % 2 == 0 {"alias"} else {"index"}},
                "session":{"model_id":"claude-opus-4"},"env":{"ANTHROPIC_BASE_URL":url}});
            std::thread::spawn(move || render(std::mem::replace(&mut c, Command::new(BIN)), input))
        })
        .collect();
    for t in hs {
        assert_eq!(t.join().unwrap()["text"], "key?");
    }
    assert_eq!(s.count(), 1, "one failed attempt across all processes");
}

fn two_account_listing() -> Value {
    json!({"files":[
        {"auth_index":"ix1","type":"claude","email":"alice.synthetic@corp.test","priority":2,
         "quota":{"signals":{"Anthropic-Ratelimit-Unified-7d-Utilization":"0.5",
                             "Anthropic-Ratelimit-Unified-7d-Reset":"4102444800"}}},
        {"auth_index":"ix2","type":"claude","email":"bob.synthetic@other.test",
         "quota":{"signals":{"Anthropic-Ratelimit-Unified-7d-Utilization":"0.1",
                             "Anthropic-Ratelimit-Unified-7d-Reset":"4102444800"}}}]})
}

fn files_under(dir: &std::path::Path) -> Vec<String> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(files_under(&p));
        } else {
            out.push(std::fs::read_to_string(&p).unwrap_or_default());
        }
    }
    out
}

/// One render of the two-account listing with `extra` config lines and the
/// user's `rules`; each call has its own state, so earlier labels are not
/// served from a cache. Returns the output and the text of every file the
/// plugin left in its state directory.
fn labelled(tag: &str, extra: &str, rules: Value) -> (Value, Vec<String>) {
    let listing = two_account_listing();
    let s = Server::start(move |_| ok(listing.to_string()));
    let h = Home::new(tag);
    let key = h.file("key", "bin-test-key\n", 0o600);
    let conf = h.file(
        "c.conf",
        &format!(
            "instance {}\n key-file {}\n{extra}",
            s.origin(),
            key.display()
        ),
        0o600,
    );
    let o = render(
        h.cmd(&conf),
        json!({"options":{},"session":{"model_id":"claude-opus-4"},
               "env":{"ANTHROPIC_BASE_URL":s.origin()},"accounts":rules}),
    );
    let kept = files_under(&h.0.join("state"));
    (o, kept)
}

fn item(o: &Value, label: &str) -> Value {
    o["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["label"] == label)
        .cloned()
        .unwrap_or_else(|| panic!("no {label} in {o}"))
}

#[test]
fn the_users_account_lines_label_and_colour_items_and_the_account() {
    let rules = json!([
        {"glob": "alice.synthetic@*", "label": "work", "color": "33"},
        {"glob": "*", "label": "everyone", "color": "#112233"}
    ]);
    let (o, kept) = labelled("rules1", "", rules);
    // first match wins, per account
    assert_eq!(item(&o, "work")["label_color"], "33");
    assert_eq!(item(&o, "everyone")["label_color"], "#112233");
    assert_eq!(
        o["account"]["label"], "work",
        "alice has the higher priority"
    );
    assert_eq!(o["account"]["color"], "33");
    // nothing from the lines is kept but the label and colour: no email, no glob
    assert!(kept.iter().any(|f| f.contains("work")), "{kept:?}");
    for f in &kept {
        for secret in [
            "corp.test",
            "other.test",
            "synthetic@",
            "@*",
            "alice.synthetic",
        ] {
            assert!(!f.contains(secret), "{secret} in {f}");
        }
    }
}

#[test]
fn no_matching_line_means_the_local_part_and_no_colour() {
    let (o, _) = labelled(
        "rules2",
        "",
        json!([{"glob": "nobody@*", "label": "x", "color": "33"}]),
    );
    assert_eq!(item(&o, "alice.synthe").get("label_color"), None);
    assert_eq!(o["account"]["label"], "alice.synthe");
    assert!(o["account"].get("color").is_none(), "{o}");
}

#[test]
fn the_plugin_s_own_alias_beats_the_lines_but_the_colour_still_comes_from_the_line() {
    let rules = json!([{"glob": "alice.synthetic@*", "label": "work", "color": "33"}]);
    let (o, _) = labelled("rules3", " alias \"alice.synthetic@*\" mine\n", rules);
    assert_eq!(item(&o, "mine")["label_color"], "33");
    assert_eq!(o["account"]["label"], "mine");
    assert!(o["items"]
        .as_array()
        .unwrap()
        .iter()
        .all(|i| i["label"] != "work"));
}

#[test]
fn a_session_without_a_proxy_gets_no_tokens() {
    let s = Server::start(|_| ok("{}"));
    let other = Server::start(|_| ok("{}"));
    let h = Home::new("plain");
    let key = h.file("key", "bin-test-key\n", 0o600);
    let one = h.file(
        "one.conf",
        &format!("instance {}\n key-file {}\n", s.origin(), key.display()),
        0o600,
    );
    let two = h.file(
        "two.conf",
        &format!(
            "instance {}\n key-file {}\ninstance {}\n key-file {}\n",
            s.origin(),
            key.display(),
            other.origin(),
            key.display()
        ),
        0o600,
    );
    // ANTHROPIC_BASE_URL is unset: the input has no `env` entry for it
    let input = json!({"options":{},"session":{"model_id":"claude-opus-4"},"env":{}});
    let o = render(h.cmd(&one), input.clone());
    assert_eq!(o["text"], "");
    assert!(
        o.get("account").is_none() && o.get("items").is_none(),
        "{o}"
    );
    // even with one instance: switching it would not change this session
    assert!(o.get("tokens").is_none(), "{o}");
    assert!(render(h.cmd(&two), input.clone()).get("tokens").is_none());
    // a session on some other URL is not a plain one
    let o = render(
        h.cmd(&one),
        json!({"options":{},"session":{"model_id":"claude-opus-4"},
               "env":{"ANTHROPIC_BASE_URL":"http://127.0.0.1:1"}}),
    );
    assert!(o.get("tokens").is_none(), "{o}");
    // no request went out for any of this
    assert_eq!(s.count() + other.count(), 0);
}

#[test]
fn a_routed_session_shows_the_route_provider() {
    let listing = json!({"files":[
        {"auth_index":"ix1","type":"claude","email":"a@corp.test","priority":1,
         "quota":{"signals":{"Anthropic-Ratelimit-Unified-7d-Utilization":"0.5",
                             "Anthropic-Ratelimit-Unified-7d-Reset":"4102444800"}}}]});
    let s = Server::start(move |_| ok(listing.to_string()));
    let h = Home::new("route");
    let key = h.file("key", "bin-test-key\n", 0o600);
    let conf = h.file(
        "c.conf",
        &format!("instance {}\n key-file {}\n", s.origin(), key.display()),
        0o600,
    );
    let go = |model: &str, opts: Value| {
        render(
            h.cmd(&conf),
            json!({"options":opts,"session":{"model_id":model},"env":{"ANTHROPIC_BASE_URL":s.origin()}}),
        )
    };
    // default: the claude pool, for every route-* alias
    for m in ["route-auto", "route-private"] {
        let o = go(m, json!({}));
        assert_eq!(o["items"][0]["tokens"]["provider"], "claude", "{m}");
        assert_eq!(o["usage"]["seven_day"]["used_percentage"], 50.0, "{m}");
    }
    // `none` hides the segment in a routed session, a real model is unaffected
    assert_eq!(
        go("route-auto", json!({"route-provider":"none"}))["text"],
        ""
    );
    assert_eq!(
        go("claude-opus-4", json!({"route-provider":"none"}))["items"][0]["tokens"]["provider"],
        "claude"
    );
    // codex is not in this proxy's pool: nothing to show
    assert_eq!(
        go("route-auto", json!({"route-provider":"codex"}))["text"],
        ""
    );
}
