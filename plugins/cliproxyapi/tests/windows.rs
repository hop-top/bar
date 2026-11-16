#![cfg(unix)]
//! What an account's windows read as, end to end: the segment, the pool,
//! `pick` and the chooser's entries agree that a window past its reset is
//! unused, that a window the provider leaves out of an otherwise valid
//! reading is unused in `{used}`, and that only an account never read is
//! unknown. Synthetic accounts against a fake management server.

#[path = "../../../tests/common/deadline.rs"]
mod deadline;
mod fake;

use fake::{ok, pool, status, Server};
use serde_json::{json, Value};
use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_bar-cliproxyapi");
const KEY: &str = "windows-test-key";
/// Long past, and far ahead.
const PAST: &str = "1000000000";
const AHEAD: &str = "4102444800";

struct Home(std::path::PathBuf);

impl Home {
    fn new(name: &str) -> Home {
        let d = std::env::temp_dir().join(format!("cpa-win-{}-{name}", std::process::id()));
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
    fn conf(&self, s: &Server, extra: &str) -> std::path::PathBuf {
        let key = self.file("key", &format!("{KEY}\n"), 0o600);
        let text = format!(
            "instance {}\n key-file {}\n{extra}",
            s.origin(),
            key.display()
        );
        self.file("c.conf", &text, 0o600)
    }
    fn cmd(&self, s: &Server, extra: &str) -> Command {
        let mut c = Command::new(BIN);
        c.env("BAR_CLIPROXYAPI_CONFIG", self.conf(s, extra))
            .env("BAR_STATE_DIR", self.0.join("state"))
            .env("PATH", "/usr/bin:/bin")
            .env_remove("ANTHROPIC_BASE_URL");
        c
    }
    fn render(&self, s: &Server, model: &str) -> Value {
        let input = json!({"options":{},"session":{"model_id":model},
            "env":{"ANTHROPIC_BASE_URL":s.origin()}});
        let out = deadline::run(&mut self.cmd(s, ""), Some(input.to_string().as_bytes()));
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        serde_json::from_slice(&out.stdout).unwrap()
    }
    fn cli(&self, s: &Server, args: &[&str]) -> Output {
        let mut c = self.cmd(s, "");
        c.args(args);
        deadline::run(&mut c, Some(b""))
    }
    /// The chooser's entries of the `pick` action, with the default label.
    fn entries(&self, s: &Server, provider: &str) -> Vec<String> {
        let log = self.0.join("chooser.log");
        let prog = self.file(
            "chooser.sh",
            "#!/bin/sh\nfor a in \"$@\"; do printf '%s\\n' \"$a\"; done > \"$FAKE_LOG\"\n",
            0o700,
        );
        let id = bar_cliproxyapi::switch::instance_id(&s.origin());
        let a = json!({"action":"pick","tokens":{"provider":provider,"instance":id}});
        let mut c = self.cmd(s, "");
        c.args(["--action", "pick"])
            .env("BAR_CLIPROXYAPI_CHOOSER", &prog)
            .env("FAKE_LOG", &log);
        let o = deadline::run(&mut c, Some(a.to_string().as_bytes()));
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        let argv: Vec<String> = std::fs::read_to_string(&log)
            .unwrap_or_default()
            .lines()
            .map(String::from)
            .collect();
        argv[3..].to_vec()
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A claude account with the given rate-limit headers, `(window, used
/// fraction, reset)`.
fn claude(name: &str, wins: &[(&str, &str, &str)]) -> Value {
    let mut sig = serde_json::Map::new();
    for (id, util, reset) in wins {
        let k = |p: &str| format!("Anthropic-Ratelimit-Unified-{id}-{p}");
        sig.insert(k("Utilization"), json!(util));
        sig.insert(k("Reset"), json!(reset));
    }
    json!({"name": format!("{name}.json"), "auth_index": format!("ix-{name}"),
        "type": "claude", "email": format!("{name}@corp.test"),
        "quota": {"signals": sig}})
}

fn item<'a>(o: &'a Value, label: &str) -> &'a Value {
    o["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["label"] == label)
        .unwrap_or_else(|| panic!("no {label} in {o}"))
}

fn text(o: &Value, label: &str) -> String {
    item(o, label)["text"].as_str().unwrap().to_string()
}

#[test]
fn a_window_past_its_reset_reads_as_unused_everywhere_until_re_read() {
    let (s, files) = pool(
        json!([
            claude("spent", &[("7d", "0.95", PAST)]),
            claude("busy", &[("7d", "0.30", AHEAD)]),
        ]),
        KEY,
    );
    let h = Home::new("reset");
    let o = h.render(&s, "claude-opus-4");
    // the weekly quota is back: all of it left, no countdown, not `bad`
    assert_eq!(text(&o, "spent"), "100%", "{o}");
    assert!(item(&o, "spent").get("style").is_none(), "{o}");
    assert!(text(&o, "busy").starts_with("70% \u{21BB}"), "{o}");
    // the pool counts it as unused, with the reset still to come
    assert_eq!(o["usage"]["seven_day"]["used_percentage"], 15.0, "{o}");
    assert_eq!(o["usage"]["seven_day"]["resets_at"], 4_102_444_800i64);

    let p = h.cli(&s, &["pick", "--provider", "claude"]);
    let t = String::from_utf8_lossy(&p.stdout).to_string();
    let spent = t.lines().find(|l| l.contains(" spent ")).unwrap();
    assert!(spent.ends_with("100% left"), "{t}");

    // the account is used again: the new reading replaces the old
    files.lock().unwrap()[0] = claude("spent", &[("5h", "0.1", AHEAD), ("7d", "0.02", AHEAD)]);
    let o = h.render(&s, "claude-opus-4");
    assert!(text(&o, "spent").starts_with("5h 90% \u{21BB}"), "{o}");
    assert!(text(&o, "spent").contains("7d 98% \u{21BB}"), "{o}");
}

#[test]
fn the_chooser_shows_0_for_a_5h_window_the_reading_leaves_out() {
    let (s, _) = pool(
        json!([
            claude("weekly", &[("7d", "0.62", AHEAD)]),
            claude("both", &[("5h", "0.4", AHEAD), ("7d", "0.5", AHEAD)]),
            claude("reset", &[("5h", "0.9", PAST), ("7d", "0.2", AHEAD)]),
            claude("unread", &[]),
        ]),
        KEY,
    );
    let h = Home::new("label");
    let mut got = h.entries(&s, "claude");
    got.sort();
    assert_eq!(
        got,
        ["both - 40%", "reset - 0%", "unread - ?%", "weekly - 0%"]
    );
}

/// A management server whose `api-call` answers `reply` (status, body).
fn codex_server(reply: (u16, Value)) -> Server {
    let listing = json!({"files":[{"name":"c.json","auth_index":"ix-c","type":"codex",
        "email":"carol@other.test","quota":{"signals":{}}}]});
    Server::start(move |r| match r.path.as_str() {
        "/v8/management/credentials" => ok(listing.to_string()),
        "/v8/management/requests/api-call" => ok(json!({"status_code":reply.0,"header":{},
            "body":reply.1.to_string()})
        .to_string()),
        _ => status(404),
    })
}

#[test]
fn codex_windows_past_their_reset_are_unused_and_a_failed_read_is_unknown() {
    let wham = json!({"plan_type":"pro","rate_limit":{"allowed":true,
        "primary_window":{"used_percent":80,"limit_window_seconds":604800,"reset_at":1_000_000_000},
        "secondary_window":null}});
    let s = codex_server((200, wham));
    let h = Home::new("codex");
    let o = h.render(&s, "gpt-5-codex");
    assert_eq!(text(&o, "carol"), "100%", "{o}");
    assert_eq!(h.entries(&s, "codex"), ["carol - 0%"]);

    let s = codex_server((500, json!({})));
    let h = Home::new("codex-fail");
    let o = h.render(&s, "gpt-5-codex");
    // nothing known: no entry drawn, and `?` in the chooser
    assert!(o.get("items").is_none(), "{o}");
    assert_eq!(h.entries(&s, "codex"), ["carol - ?%"]);
}
