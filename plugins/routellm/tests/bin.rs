#![cfg(unix)]
//! The binary end to end: `--ext-info`, `--check`, a render, the commands
//! and an action, against a fake proxy and RouteLLM.

mod common;
#[path = "../../../tests/common/deadline.rs"]
mod deadline;
mod fake;

use common::{healthy, Env, KEY};
use serde_json::{json, Value};
use std::os::unix::fs::PermissionsExt;
use std::process::Command;

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
    deadline::run(&mut c, Some(input.as_bytes()))
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
    assert_eq!(v["aliases"], json!(["rtellm"]));
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

fn texts(v: &Value) -> Vec<String> {
    v["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["text"].as_str().unwrap().to_string())
        .collect()
}

fn render_for(h: &Home, e: &Env, model: &str, options: Value) -> Value {
    let input = json!({"options":options,"session":{"model_id":model},
        "env":{"ANTHROPIC_BASE_URL": e.proxy.server.origin()}})
    .to_string();
    out_json(&feed(h.cmd(), &input))
}

#[test]
fn render_shows_the_state_and_what_a_click_does() {
    let e = Env::new("render", false, healthy(), 1000);
    let h = Home::new("render", &e);
    let input = json!({"env":{"ANTHROPIC_BASE_URL": e.proxy.server.origin()}}).to_string();
    let v = out_json(&feed(h.cmd(), &input));
    assert_eq!(texts(&v), ["on"]);
    assert_eq!(v["items"][0]["style"], "good");
    assert_eq!(v["items"][0]["label"], "RL");
    assert!(v["items"][0]["icon"].is_string());
    assert_eq!(v["items"][0]["tokens"]["to"], "off");
    assert_eq!(v["tokens"]["to"], "off");
    assert!(v["tokens"]["instance"].is_string());
    // a session on another base URL: hidden, no request
    let n = e.proxy.server.count();
    let other = json!({"env":{"ANTHROPIC_BASE_URL":"http://127.0.0.1:1"}}).to_string();
    assert_eq!(out_json(&feed(h.cmd(), &other))["text"], "");
    assert_eq!(e.proxy.server.count(), n);
}

#[test]
fn a_routed_session_shows_alias_and_tier() {
    let e = Env::new("routed-on", false, healthy(), 1000);
    let h = Home::new("routed-on", &e);
    // ON: `alias[tier]`, one item, green
    let v = render_for(&h, &e, "route-auto", json!({}));
    assert_eq!(texts(&v), ["route-auto[auto]"]);
    assert_eq!(v["items"][0]["style"], "good");
    assert_eq!(v["items"][0]["tokens"]["to"], "off");
    // the old multi-item form stays reachable
    let v = render_for(
        &h,
        &e,
        "route-auto",
        json!({"show":"state,alias,tier,marker,caps"}),
    );
    assert_eq!(
        texts(&v),
        ["on", "route-auto[auto]", "\u{2601}", "vis tools 200k"]
    );
    let v = render_for(&h, &e, "route-private", json!({}));
    assert_eq!(texts(&v), ["route-private[private]"]);
    // a model that is no alias: the state alone, and `routed` hides it
    assert_eq!(
        texts(&render_for(&h, &e, "claude-opus-4", json!({}))),
        ["on"]
    );
    assert_eq!(
        render_for(&h, &e, "claude-opus-4", json!({"routed":true}))["text"],
        ""
    );
}

#[test]
fn routing_off_shows_the_tier_and_where_the_alias_goes() {
    let e = Env::new("routed-off", false, healthy(), 1000);
    let h = Home::new("routed-off", &e);
    // OFF: tier and the full model the alias maps to; a private alias with no row
    let o = feed(h.cmd().arg("off").take_mut(), "");
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let v = render_for(&h, &e, "route-auto", json!({}));
    assert_eq!(texts(&v), ["auto[claude-sonnet-5-5]"]);
    assert_eq!(v["items"][0]["style"], "dim");
    assert_eq!(v["items"][0]["tokens"]["to"], "on");
    let v = render_for(&h, &e, "route-private", json!({}));
    assert_eq!(texts(&v), ["private[\u{2717}]"]);
    assert_eq!(v["items"][0]["style"], "bad");
    // nothing secret in any output
    for m in ["route-auto", "route-private"] {
        let t = render_for(&h, &e, m, json!({"show":"state,alias,tier,marker,caps"})).to_string();
        for secret in [fake::SECRET, fake::HEADER_SECRET, KEY] {
            assert!(!t.contains(secret));
        }
    }
}

#[test]
fn the_action_timeout_and_samples_are_declared() {
    let e = Env::new("decl", false, healthy(), 1000);
    let h = Home::new("decl", &e);
    let v = out_json(&feed(h.cmd().arg("--ext-info").take_mut(), ""));
    for a in ["toggle", "on", "off"] {
        assert_eq!(
            v["actions"][a]["timeout"],
            bar_routellm::toggle::WORST_SECS + 20,
            "{a}"
        );
        assert_eq!(v["actions"][a]["tokens"], json!(["instance", "to"]));
    }
    for o in [
        "show", "label", "compact", "stale", "link", "routed", "base-url",
    ] {
        assert!(v["options"][o].is_object(), "{o}");
    }
    assert_eq!(v["web"]["samples"].as_array().unwrap().len(), 3);
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
        t.contains("model-alias:") && t.contains("fork: true") && t.contains("      keys:"),
        "{t}"
    );
    assert!(
        !t.contains("oauth-model-alias:") && !t.contains("api-key-entries"),
        "{t}"
    );
    assert!(!t.contains(KEY));
    let o = feed(h.cmd().args(["setup", "--layout", "legacy"]).take_mut(), "");
    let t = String::from_utf8_lossy(&o.stdout).to_string();
    assert!(
        t.contains("oauth-model-alias:")
            && t.contains("api-key-entries:")
            && !t.contains("keys:\n"),
        "{t}"
    );
    let o = feed(h.cmd().args(["setup", "--layout", "v9"]).take_mut(), "");
    assert_eq!(o.status.code(), Some(2));
    let o = feed(h.cmd().args(["status", "--layout", "v8"]).take_mut(), "");
    assert_eq!(o.status.code(), Some(2));
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
