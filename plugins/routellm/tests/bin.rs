#![cfg(unix)]
//! The binary end to end: `--ext-info`, `--check`, a render, the commands
//! and an action, against a fake proxy and RouteLLM.

mod common;
mod fake;

use common::{healthy, Env, KEY};
use serde_json::{json, Value};
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Stdio};

const BIN: &str = env!("CARGO_BIN_EXE_bar-routellm");

struct Home(std::path::PathBuf);

impl Home {
    fn new(name: &str, e: &Env) -> Home {
        let d = std::env::temp_dir().join(format!("rl-bin-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let h = Home(d);
        h.file("key", &format!("{KEY}\n"));
        h.file(
            "routellm.conf",
            &format!(
                "instance {}\n routellm {}\n key-file {}\n",
                e.proxy.server.origin(),
                e.rl.origin(),
                h.0.join("key").display()
            ),
        );
        h
    }
    fn file(&self, name: &str, text: &str) {
        let p = self.0.join(name);
        std::fs::write(&p, text).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    fn cmd(&self) -> Command {
        let mut c = Command::new(BIN);
        c.env("BAR_ROUTELLM_CONFIG", self.0.join("routellm.conf"))
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

fn feed(mut c: Command, input: &str) -> std::process::Output {
    let mut ch = c
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    ch.stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    ch.wait_with_output().unwrap()
}

fn out_json(o: &std::process::Output) -> Value {
    serde_json::from_slice(&o.stdout).unwrap()
}

#[test]
fn ext_info_declares_the_alias_inputs_timeout_and_actions() {
    let e = Env::new("ext", false, healthy(), 1000);
    let h = Home::new("ext", &e);
    let o = feed(h.cmd().arg("--ext-info").take_mut(), "");
    let v = out_json(&o);
    assert_eq!(v["name"], "routellm");
    assert_eq!(v["aliases"], json!(["rl"]));
    assert_eq!(v["inputs"]["env"], json!(["ANTHROPIC_BASE_URL"]));
    assert_eq!(v["timeout"], "8s");
    assert_eq!(v["ttl"], "15s");
    for a in ["toggle", "on", "off"] {
        assert!(v["actions"][a].is_object(), "{a}");
    }
    assert!(v["version"].is_string());
}

trait TakeMut {
    fn take_mut(&mut self) -> Command;
}
impl TakeMut for Command {
    fn take_mut(&mut self) -> Command {
        std::mem::replace(self, Command::new("true"))
    }
}

#[test]
fn render_shows_the_state_and_what_a_click_does() {
    let e = Env::new("render", false, healthy(), 1000);
    let h = Home::new("render", &e);
    let input = json!({"env":{"ANTHROPIC_BASE_URL": e.proxy.server.origin()}}).to_string();
    let v = out_json(&feed(h.cmd(), &input));
    assert_eq!(v["text"], "RL on");
    assert_eq!(v["style"], "good");
    assert_eq!(v["tokens"]["to"], "off");
    // a session on another base URL: hidden, no request
    let n = e.proxy.server.count();
    let other = json!({"env":{"ANTHROPIC_BASE_URL":"http://127.0.0.1:1"}}).to_string();
    assert_eq!(out_json(&feed(h.cmd(), &other))["text"], "");
    assert_eq!(e.proxy.server.count(), n);
}

#[test]
fn off_and_on_commands_and_the_action() {
    let e = Env::new("cmds", false, healthy(), 1000);
    let h = Home::new("cmds", &e);
    let o = feed(h.cmd().arg("off").take_mut(), "");
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(e.proxy.entries.lock().unwrap()[0]["disabled"], true);
    // a stale link cannot flip it back: `to=off` on an off proxy is a no-op
    let id = bar_routellm::config::instance_id(&e.inst.origin);
    let act = json!({"action":"toggle","tokens":{"instance":id,"to":"off"}}).to_string();
    let o = feed(h.cmd().args(["--action", "toggle"]).take_mut(), &act);
    assert!(o.status.success());
    assert!(String::from_utf8_lossy(&o.stdout).contains("already off"));
    let act = json!({"action":"toggle","tokens":{"instance":id,"to":"on"}}).to_string();
    let o = feed(h.cmd().args(["--action", "toggle"]).take_mut(), &act);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(e.proxy.entries.lock().unwrap()[0]["disabled"], false);
    // bad usage
    assert_eq!(
        feed(h.cmd().args(["on", "--bogus"]).take_mut(), "")
            .status
            .code(),
        Some(2)
    );
    assert_eq!(
        feed(h.cmd().args(["off", "--force"]).take_mut(), "")
            .status
            .code(),
        Some(2)
    );
}

#[test]
fn a_refused_command_exits_one_with_the_reason() {
    let e = Env::new("refuse", true, fake::routellm(false, &[]), 1000);
    let h = Home::new("refuse", &e);
    let o = feed(h.cmd().arg("on").take_mut(), "");
    assert_eq!(o.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&o.stderr).contains("not switching on"));
    assert_eq!(e.proxy.entries.lock().unwrap()[0]["disabled"], true);
}

#[test]
fn setup_prints_the_snippet_and_writes_nothing_to_the_proxy() {
    let e = Env::new("setup", false, healthy(), 1000);
    let h = Home::new("setup", &e);
    let o = feed(h.cmd().arg("setup").take_mut(), "");
    let t = String::from_utf8_lossy(&o.stdout).to_string();
    assert!(o.status.success());
    assert!(
        t.contains("oauth-model-alias:") && t.contains("fork: true"),
        "{t}"
    );
    assert!(!t.contains(KEY));
    assert_eq!(e.proxy.server.count(), 0, "setup makes no call");
}

#[test]
fn status_and_tiers_never_print_secrets() {
    let e = Env::new("status", false, healthy(), 1000);
    let h = Home::new("status", &e);
    let o = feed(h.cmd().args(["status", "--probe"]).take_mut(), "");
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let t = String::from_utf8_lossy(&o.stdout).to_string();
    assert!(t.contains("state: on") && t.contains("route-auto"), "{t}");
    let o = feed(h.cmd().arg("tiers").take_mut(), "");
    let t2 = String::from_utf8_lossy(&o.stdout).to_string();
    assert!(
        t2.contains("auto route-auto") && t2.contains("200k"),
        "{t2}"
    );
    for s in [&t, &t2] {
        for secret in [fake::SECRET, fake::HEADER_SECRET, KEY] {
            assert!(!s.contains(secret));
        }
    }
    let j = feed(h.cmd().args(["status", "--json"]).take_mut(), "");
    assert!(!String::from_utf8_lossy(&j.stdout).contains(fake::SECRET));
}

#[test]
fn check_reports_a_loose_config_a_missing_entry_and_a_remembered_refusal() {
    let e = Env::new("check", false, healthy(), 1000);
    let h = Home::new("check", &e);
    // nothing read yet
    let v = out_json(&feed(h.cmd().arg("--check").take_mut(), ""));
    assert!(v["warnings"].to_string().contains("not read yet"), "{v}");
    feed(h.cmd().args(["status", "--probe"]).take_mut(), "");
    let v = out_json(&feed(h.cmd().arg("--check").take_mut(), ""));
    assert_eq!(v["warnings"], json!([]), "{v}");
    // a group-readable config is refused
    std::fs::set_permissions(
        h.0.join("routellm.conf"),
        std::fs::Permissions::from_mode(0o644),
    )
    .unwrap();
    let v = out_json(&feed(h.cmd().arg("--check").take_mut(), ""));
    assert!(v["warnings"].to_string().contains("refused"), "{v}");
    assert_eq!(e.proxy.server.count(), 2, "--check makes no call");
}

#[test]
fn key_from_cliproxyapi_reuses_its_key_source() {
    let e = Env::new("keyfrom", false, healthy(), 1000);
    let h = Home::new("keyfrom", &e);
    h.file(
        "routellm.conf",
        &format!(
            "instance {}\n routellm {}\n key-from cliproxyapi\n",
            e.proxy.server.origin(),
            e.rl.origin()
        ),
    );
    h.file(
        "cliproxyapi.conf",
        &format!(
            "instance {}\n key-file {}\n",
            e.proxy.server.origin(),
            h.0.join("key").display()
        ),
    );
    let mut c = h.cmd();
    c.env("BAR_CLIPROXYAPI_CONFIG", h.0.join("cliproxyapi.conf"));
    let o = feed(c.args(["status", "--probe"]).take_mut(), "");
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(String::from_utf8_lossy(&o.stdout).contains("state: on"));
}
