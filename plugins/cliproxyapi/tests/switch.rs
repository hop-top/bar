#![cfg(unix)]
//! `list`, `use` and `--action use` end to end against a fake management
//! server that keeps priorities: synthetic accounts only.

#[path = "../../../tests/common/deadline.rs"]
mod deadline;
mod fake;

use fake::{pool, Server};
use serde_json::{json, Value};
use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_bar-cliproxyapi");
const KEY: &str = "switch-test-key";

struct Home(std::path::PathBuf);

impl Home {
    fn new(name: &str) -> Home {
        let d = std::env::temp_dir().join(format!("cpa-sw-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        Home(d)
    }
    fn file(&self, name: &str, text: &str) -> std::path::PathBuf {
        let p = self.0.join(name);
        std::fs::write(&p, text).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600)).unwrap();
        p
    }
    /// A config with one instance per origin.
    fn conf(&self, origins: &[String]) -> std::path::PathBuf {
        let key = self.file("key", &format!("{KEY}\n"));
        let text: String = origins
            .iter()
            .map(|o| format!("instance {o}\n key-file {}\n", key.display()))
            .collect();
        self.file("c.conf", &text)
    }
    fn run(&self, conf: &std::path::Path, args: &[&str], stdin: &str) -> Output {
        self.run_env(conf, args, stdin, &[])
    }
    fn run_env(
        &self,
        conf: &std::path::Path,
        args: &[&str],
        stdin: &str,
        env: &[(&str, String)],
    ) -> Output {
        let mut cmd = Command::new(BIN);
        cmd.args(args)
            .envs(env.iter().map(|(k, v)| (*k, v)))
            .env("BAR_CLIPROXYAPI_CONFIG", conf)
            .env("BAR_STATE_DIR", self.0.join("state"))
            .env_remove("ANTHROPIC_BASE_URL");
        deadline::run(&mut cmd, Some(stdin.as_bytes()))
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn cred(name: &str, ty: &str, email: &str, priority: Option<i64>) -> Value {
    let mut v = json!({"name": format!("{name}.json"), "auth_index": format!("ix-{name}"),
        "type": ty, "email": email});
    if let Some(p) = priority {
        v["priority"] = p.into();
    }
    v
}

fn fleet() -> Value {
    json!([
        cred("a", "claude", "alice@corp.test", None),
        cred("b", "claude", "bob@corp.test", Some(2)),
        cred("c", "claude", "cy@corp.test", Some(2)),
        cred("d", "codex", "dee@corp.test", Some(9)),
    ])
}

fn text(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).to_string()
}
fn err(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).to_string()
}
fn prio(state: &Value, name: &str) -> i64 {
    state
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["name"] == format!("{name}.json"))
        .unwrap()["priority"]
        .as_i64()
        .unwrap_or(0)
}

#[test]
fn use_raises_above_the_tie_and_persists() {
    let (s, st) = pool(fleet(), KEY);
    let h = Home::new("use");
    let conf = h.conf(&[s.origin()]);
    let o = h.run(&conf, &["use", "alice", "--instance", &s.origin()], "");
    assert!(o.status.success(), "{}", err(&o));
    assert_eq!(text(&o).trim(), "alice (claude): priority 0 -> 3");
    assert_eq!(prio(&st.lock().unwrap(), "a"), 3);
    // others and the other provider are untouched; one PATCH only
    assert_eq!(prio(&st.lock().unwrap(), "b"), 2);
    assert_eq!(prio(&st.lock().unwrap(), "d"), 9);
    let p = s.patches();
    assert_eq!(p.len(), 1);
    assert_eq!(p[0].path, "/v8/management/credentials/fields");
    let body: Value = serde_json::from_str(&p[0].body).unwrap();
    assert_eq!(body, json!({"name": "a.json", "priority": 3}));
    assert_eq!(p[0].auth, format!("bearer {}", KEY.to_ascii_lowercase()));
    // again: already preferred, no second PATCH
    let o = h.run(&conf, &["use", "alice"], "");
    assert!(text(&o).contains("already preferred"), "{}", text(&o));
    assert_eq!(s.patches().len(), 1);
    // no email in anything printed
    assert!(!text(&o).contains("corp.test"));
}

#[test]
fn reset_restores_recorded_priorities_and_leaves_manual_changes() {
    let (s, st) = pool(fleet(), KEY);
    let h = Home::new("reset");
    let conf = h.conf(&[s.origin()]);
    for who in ["alice", "bob"] {
        assert!(h.run(&conf, &["use", who], "").status.success());
    }
    // alice 3, bob 4 (above alice); the user then edits cy by hand: not ours
    assert_eq!(prio(&st.lock().unwrap(), "b"), 4);
    let o = h.run(&conf, &["use", "--reset", "bob"], "");
    assert!(o.status.success(), "{}", err(&o));
    assert_eq!(prio(&st.lock().unwrap(), "b"), 2);
    assert_eq!(prio(&st.lock().unwrap(), "a"), 3);
    // someone changed alice meanwhile: reset keeps it
    st.lock().unwrap()[0]["priority"] = 7.into();
    let o = h.run(&conf, &["use", "--reset"], "");
    assert!(text(&o).contains("changed since"), "{}", text(&o));
    assert_eq!(prio(&st.lock().unwrap(), "a"), 7);
    // nothing recorded any more
    let n = s.patches().len();
    let o = h.run(&conf, &["use", "--reset"], "");
    assert!(text(&o).contains("nothing to reset"));
    assert_eq!(s.patches().len(), n);
    // the state file is private and holds no email or name
    let dir = h.0.join("state/plugins/cliproxyapi");
    let f = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.to_string_lossy().ends_with(".switch.json"))
        .unwrap();
    let mode = std::fs::metadata(&f).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
    let raw = std::fs::read_to_string(&f).unwrap();
    assert!(
        !raw.contains("corp.test") && !raw.contains(".json\""),
        "{raw}"
    );
}

#[test]
fn reset_undoes_the_original_priority_after_two_switches() {
    let (s, st) = pool(fleet(), KEY);
    let h = Home::new("twice");
    let conf = h.conf(&[s.origin()]);
    h.run(&conf, &["use", "alice"], "");
    h.run(&conf, &["use", "cy"], "");
    h.run(&conf, &["use", "alice"], "");
    assert_eq!(prio(&st.lock().unwrap(), "a"), 5);
    let o = h.run(&conf, &["use", "--reset", "alice"], "");
    assert!(o.status.success());
    assert_eq!(prio(&st.lock().unwrap(), "a"), 0);
}

#[test]
fn dry_run_sends_no_patch_and_changes_nothing() {
    let (s, st) = pool(fleet(), KEY);
    let h = Home::new("dry");
    let conf = h.conf(&[s.origin()]);
    let o = h.run(&conf, &["use", "alice", "--dry-run"], "");
    assert!(o.status.success());
    assert_eq!(text(&o).trim(), "dry run: alice (claude): priority 0 -> 3");
    assert!(s.patches().is_empty());
    assert_eq!(prio(&st.lock().unwrap(), "a"), 0);
    // and no state to reset
    let o = h.run(&conf, &["use", "--reset", "--dry-run"], "");
    assert!(text(&o).contains("nothing to reset"));
}

#[test]
fn refusals_exit_1_and_send_nothing() {
    let mut f = fleet();
    f[2]["disabled"] = true.into();
    f.as_array_mut()
        .unwrap()
        .push(cred("e", "codex", "alice@other.test", None));
    let (s, _) = pool(f, KEY);
    let h = Home::new("refuse");
    let conf = h.conf(&[s.origin()]);
    for (args, why) in [
        (vec!["use", "cy"], "disabled"),
        (vec!["use", "nobody"], "no account"),
        (vec!["use", "alice"], "ambiguous"),
        (vec!["use", "alice", "--provider", "grok"], ""),
    ] {
        let o = h.run(&conf, &args, "");
        assert!(!o.status.success(), "{args:?}");
        assert!(err(&o).contains(why), "{args:?}: {}", err(&o));
    }
    // narrowed by provider the same name works
    assert!(h
        .run(&conf, &["use", "alice", "--provider", "codex"], "")
        .status
        .success());
    assert_eq!(s.patches().len(), 1);
    // usage errors exit 2
    assert_eq!(h.run(&conf, &["use"], "").status.code(), Some(2));
    assert_eq!(
        h.run(&conf, &["list", "--reset"], "").status.code(),
        Some(2)
    );
    assert_eq!(h.run(&conf, &["use", "a", "b"], "").status.code(), Some(2));
}

#[test]
fn list_shows_labels_priorities_and_status_without_emails() {
    let mut f = fleet();
    f[2]["unavailable"] = true.into();
    let (s, _) = pool(f, KEY);
    let h = Home::new("list");
    let conf = h.conf(&[s.origin()]);
    let o = h.run(&conf, &["list"], "");
    assert!(o.status.success(), "{}", err(&o));
    let t = text(&o);
    assert_eq!(t.lines().count(), 4, "{t}");
    assert!(
        t.contains("alice") && t.contains("unavailable") && t.contains("ix-d"),
        "{t}"
    );
    assert!(!t.contains("corp.test") && !t.contains(".json"), "{t}");
    let o = h.run(&conf, &["list", "--provider", "codex"], "");
    assert_eq!(text(&o).lines().count(), 1);
}

#[test]
fn a_refused_key_stops_every_further_call() {
    let (s, _) = pool(fleet(), "another-key");
    let h = Home::new("auth");
    let conf = h.conf(&[s.origin()]);
    let o = h.run(&conf, &["use", "alice"], "");
    assert_eq!(o.status.code(), Some(1));
    assert!(err(&o).contains("401"), "{}", err(&o));
    assert_eq!(s.count(), 1);
    // held: the next run, a list, and an action make no call
    assert!(!h.run(&conf, &["use", "alice"], "").status.success());
    assert!(!h.run(&conf, &["list"], "").status.success());
    let a = json!({"action":"use","tokens":{"account":"ix-a","provider":"claude"}});
    assert!(!h
        .run(&conf, &["--action", "use"], &a.to_string())
        .status
        .success());
    assert_eq!(s.count(), 1);
}

#[test]
fn a_refused_change_is_also_a_hard_stop() {
    // the listing is fine, the PATCH is refused
    let files = fleet();
    let listing = json!({ "files": files }).to_string();
    let s = Server::start(move |r| {
        if r.method == "PATCH" {
            fake::status(403)
        } else {
            fake::ok(listing.clone())
        }
    });
    let h = Home::new("patch403");
    let conf = h.conf(&[s.origin()]);
    assert!(!h.run(&conf, &["use", "alice"], "").status.success());
    let n = s.count();
    assert!(!h.run(&conf, &["use", "alice"], "").status.success());
    assert_eq!(s.count(), n);
}

#[test]
fn falls_back_to_the_v0_routes() {
    let files = fleet();
    let listing = json!({ "files": files }).to_string();
    let s = Server::start(move |r| match (r.method.as_str(), r.path.as_str()) {
        ("GET", "/v0/management/auth-files") => fake::ok(listing.clone()),
        ("PATCH", "/v0/management/auth-files/fields") => fake::ok("{}"),
        _ => fake::status(404),
    });
    let h = Home::new("v0");
    let conf = h.conf(&[s.origin()]);
    let o = h.run(&conf, &["use", "alice"], "");
    assert!(o.status.success(), "{}", err(&o));
    assert_eq!(s.patches()[0].path, "/v0/management/auth-files/fields");
}

#[test]
fn the_action_validates_tokens_and_picks_the_instance_from_the_snapshot() {
    let (s1, st1) = pool(fleet(), KEY);
    let (s2, st2) = pool(
        json!([
            cred("z", "claude", "zed@corp.test", None),
            cred("y", "claude", "yan@corp.test", Some(1))
        ]),
        KEY,
    );
    let h = Home::new("action");
    let conf = h.conf(&[s1.origin(), s2.origin()]);
    let act = |account: &str, provider: &str| {
        json!({"action":"use","tokens":{"account":account,"provider":provider}}).to_string()
    };
    // several instances and nothing known about the account: refused, no call
    let o = h.run(&conf, &["--action", "use"], &act("ix-z", "claude"));
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(s1.count() + s2.count(), 0);
    // the snapshot knows it once a listing was collected (a render)
    for s in [&s1, &s2] {
        let input =
            json!({"session":{"model_id":"claude-x"},"env":{"ANTHROPIC_BASE_URL":s.origin()}});
        assert!(h.run(&conf, &[], &input.to_string()).status.success());
    }
    let o = h.run(&conf, &["--action", "use"], &act("ix-z", "claude"));
    assert!(o.status.success(), "{}", err(&o));
    assert_eq!(text(&o).trim(), "zed (claude): priority 0 -> 2");
    assert_eq!(prio(&st2.lock().unwrap(), "z"), 2);
    assert_eq!(prio(&st1.lock().unwrap(), "a"), 0);
    // wrong provider for the account, unknown account, malformed tokens, other action
    for bad in [
        act("ix-z", "codex"),
        act("ix-nope", "claude"),
        act("a b", "claude"),
        "{}".into(),
        "x".into(),
    ] {
        let before = s1.patches().len() + s2.patches().len();
        let o = h.run(&conf, &["--action", "use"], &bad);
        assert_eq!(o.status.code(), Some(1), "{bad}");
        assert_eq!(s1.patches().len() + s2.patches().len(), before);
    }
    assert_eq!(
        h.run(&conf, &["--action", "nope"], "{}").status.code(),
        Some(2)
    );
}

#[test]
fn ext_info_declares_the_use_action() {
    let o = deadline::run(Command::new(BIN).arg("--ext-info"), None);
    let v: Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(
        v["actions"]["use"],
        json!({"description":"Prefer this account","tokens":["account","instance","provider"],"confirm":false})
    );
}

#[test]
fn a_position_names_the_account_label_index_draws() {
    let mut a = cred("a", "claude", "ann@corp.test", None);
    a["disabled"] = true.into();
    let fleet = json!([
        a,
        cred("b", "codex", "bea@corp.test", None),
        cred("c", "claude", "cy@corp.test", Some(1)),
        cred("d", "claude", "dee@corp.test", Some(1)),
    ]);
    // quota signals so the render draws items
    let mut fleet = fleet;
    for f in fleet.as_array_mut().unwrap() {
        f["quota"] = json!({"signals":{"Anthropic-Ratelimit-Unified-7d-Utilization":"0.5",
            "Anthropic-Ratelimit-Unified-7d-Reset":"4102444800"}});
    }
    let (s, st) = pool(fleet, KEY);
    let h = Home::new("positions");
    let conf = h.conf(&[s.origin()]);
    let input = json!({"options":{"label":"index","provider":"claude"},
        "session":{"model_id":"claude-opus-4"},"env":{"ANTHROPIC_BASE_URL":s.origin()}});
    let o = h.run(&conf, &[], &input.to_string());
    let out: Value = serde_json::from_slice(&o.stdout).unwrap();
    let drawn: Vec<(String, String)> = out["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| {
            (
                i["label"].as_str().unwrap().to_string(),
                i["tokens"]["account"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    assert!(drawn.contains(&("#2".into(), "ix-d".into())), "{drawn:?}");
    // the same number, given to `use`, names the same account
    let o = h.run(&conf, &["use", "#2", "--provider", "claude"], "");
    assert!(o.status.success(), "{}", err(&o));
    assert!(text(&o).starts_with("dee (claude)"), "{}", text(&o));
    assert_eq!(prio(&st.lock().unwrap(), "d"), 2);
    // without a provider, a position is ambiguous across providers
    let o = h.run(&conf, &["use", "#2"], "");
    assert_eq!(o.status.code(), Some(1));
    assert!(err(&o).contains("--provider"), "{}", err(&o));
}

#[test]
fn a_hand_edit_between_switches_becomes_the_priority_a_reset_restores() {
    let (s, st) = pool(fleet(), KEY);
    let h = Home::new("handedit");
    let conf = h.conf(&[s.origin()]);
    h.run(&conf, &["use", "alice"], "");
    assert_eq!(prio(&st.lock().unwrap(), "a"), 3);
    // the user lowers alice by hand, then switches to her again
    st.lock().unwrap()[0]["priority"] = 1.into();
    h.run(&conf, &["use", "alice"], "");
    assert_eq!(prio(&st.lock().unwrap(), "a"), 3);
    h.run(&conf, &["use", "--reset", "alice"], "");
    assert_eq!(prio(&st.lock().unwrap(), "a"), 1);
}

// --- next, next-soon, pick ---

/// A claude account with a 7d window: `left` percent left, resetting in
/// `resets_in` seconds from now (the listing's passive headers).
fn quota(name: &str, left: u32, resets_in: i64, priority: Option<i64>) -> Value {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let mut v = cred(name, "claude", &format!("{name}@corp.test"), priority);
    v["quota"] = json!({"signals":{
        "Anthropic-Ratelimit-Unified-7d-Utilization": format!("{}", (100 - left) as f64 / 100.0),
        "Anthropic-Ratelimit-Unified-7d-Reset": (now + resets_in).to_string()}});
    v
}

fn quota_fleet() -> Value {
    json!([
        quota("a", 20, 100_000, Some(5)), // the proxy's preferred account
        quota("b", 90, 500_000, None),
        quota("c", 85, 3_000, None),
        quota("d", 40, 1_000, None),
        cred("e", "claude", "e@corp.test", None), // unread
    ])
}

#[test]
fn next_switches_to_the_account_with_the_most_left() {
    let (s, st) = pool(quota_fleet(), KEY);
    let h = Home::new("next");
    let conf = h.conf(&[s.origin()]);
    let o = h.run(&conf, &["use", "--next", "--provider", "claude"], "");
    assert!(o.status.success(), "{}", err(&o));
    assert!(
        text(&o).starts_with("most-left: b (claude), 90% left"),
        "{}",
        text(&o)
    );
    assert_eq!(prio(&st.lock().unwrap(), "b"), 6);
    assert_eq!(s.patches().len(), 1);
}

#[test]
fn next_soon_prefers_the_sooner_reset_among_the_most_left() {
    let (s, st) = pool(quota_fleet(), KEY);
    let h = Home::new("nextsoon");
    let conf = h.conf(&[s.origin()]);
    // b has 90 left and c 85 (inside the 10-point band): c resets first
    let o = h.run(
        &conf,
        &[
            "use",
            "--next",
            "--strategy",
            "soonest-reset",
            "--provider",
            "claude",
        ],
        "",
    );
    assert!(o.status.success(), "{}", err(&o));
    assert!(
        text(&o).starts_with("soonest-reset: c (claude), 85% left"),
        "{}",
        text(&o)
    );
    assert_eq!(prio(&st.lock().unwrap(), "c"), 6);
}

#[test]
fn next_refuses_without_a_provider_when_two_are_read_and_without_quota() {
    let (s, _st) = pool(fleet(), KEY);
    let h = Home::new("nextrefuse");
    let conf = h.conf(&[s.origin()]);
    let o = h.run(&conf, &["use", "--next"], "");
    assert_eq!(o.status.code(), Some(1));
    assert!(err(&o).contains("--provider"), "{}", err(&o));
    // accounts with no data are never picked blindly
    let o = h.run(&conf, &["use", "--next", "--provider", "claude"], "");
    assert_eq!(o.status.code(), Some(1));
    assert!(err(&o).contains("quota left"), "{}", err(&o));
    assert!(s.patches().is_empty());
    let o = h.run(&conf, &["use", "--next", "--strategy", "nope"], "");
    assert_eq!(o.status.code(), Some(2));
}

#[test]
fn pick_without_a_terminal_lists_the_ranking_and_changes_nothing() {
    let (s, _st) = pool(quota_fleet(), KEY);
    let h = Home::new("picklist");
    let conf = h.conf(&[s.origin()]);
    let o = h.run(&conf, &["pick", "--provider", "claude"], "");
    assert!(o.status.success(), "{}", err(&o));
    let t = text(&o);
    let lines: Vec<&str> = t.lines().collect();
    assert_eq!(lines.len(), 5, "{t}");
    // best first; the preferred account is marked; the unread one is last
    assert!(
        lines[0].contains(" b ") && lines[0].contains("90% left") && lines[0].contains("7d"),
        "{t}"
    );
    assert!(lines[0].contains('\u{21BB}'), "{t}");
    assert!(lines[1].contains(" c "), "{t}");
    assert!(lines[3].contains('*') && lines[3].contains(" a "), "{t}");
    assert!(
        lines[4].contains(" e ") && lines[4].contains("no data"),
        "{t}"
    );
    assert!(s.patches().is_empty());
    assert!(!t.contains("corp.test"));
    // the strategy changes the order
    let o = h.run(
        &conf,
        &[
            "pick",
            "--provider",
            "claude",
            "--strategy",
            "soonest-reset",
        ],
        "",
    );
    assert!(
        text(&o).lines().next().unwrap().contains(" c "),
        "{}",
        text(&o)
    );
}

#[test]
fn pick_on_a_terminal_switches_to_the_number_asked_for() {
    use bar_cliproxyapi::{config, rank::Strategy, switch};
    use bar_plugin::secret::Secret;
    let (s, st) = pool(quota_fleet(), KEY);
    let h = Home::new("pickask");
    let mut cfg = config::parse(&format!("instance {}\n key-file /k\n", s.origin()), None);
    let inst = cfg.instances.remove(0);
    let key = Secret::new(KEY.as_bytes().to_vec());
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let state = h.0.join("state");
    let ask = |input: &str| {
        let live = switch::connect(&inst, &key, &state, now).unwrap();
        let mut reader = std::io::Cursor::new(input.to_string());
        let mut out = Vec::new();
        switch::run_pick(
            &live,
            Some("claude"),
            Strategy::MostLeft,
            Some(&mut reader),
            &mut out,
        )
        .unwrap();
        String::from_utf8(out).unwrap()
    };
    // quitting, an empty line and end of input change nothing
    for quit in ["q\n", "\n", ""] {
        ask(quit);
    }
    assert!(s.patches().is_empty());
    // a bad answer asks again; 2 is the second row (c)
    let out = ask("9\nx\n2\n");
    assert!(out.contains("not a choice"), "{out}");
    assert!(out.contains("c (claude): priority 0 -> 6"), "{out}");
    assert_eq!(prio(&st.lock().unwrap(), "c"), 6);
}

#[test]
fn the_next_actions_take_a_provider_and_an_instance() {
    let (s, st) = pool(quota_fleet(), KEY);
    let h = Home::new("nextact");
    let conf = h.conf(&[s.origin()]);
    let id = bar_cliproxyapi::switch::instance_id(&s.origin());
    let call = |action: &str, tokens: Value| {
        h.run(
            &conf,
            &["--action", action],
            &json!({"action": action, "tokens": tokens}).to_string(),
        )
    };
    let o = call("next-soon", json!({"provider":"claude","instance":id}));
    assert!(o.status.success(), "{}", err(&o));
    assert_eq!(prio(&st.lock().unwrap(), "c"), 6);
    // a token naming no configured instance, or a malformed one, refuses
    let o = call(
        "next",
        json!({"provider":"claude","instance":"0123456789abcdef"}),
    );
    assert_eq!(o.status.code(), Some(1));
    let o = call("next", json!({"provider":"claude","instance":"x"}));
    assert_eq!(o.status.code(), Some(1));
    let o = call("next", json!({"provider":"claude"}));
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(s.patches().len(), 1);
}

// --- the pick action (native chooser, stubbed) ---

/// A chooser stand-in: logs its arguments, one per line, to `log`, and
/// answers with `reply`: `cancel`, `item N` (the Nth item offered, from 1)
/// or `raw TEXT`.
fn chooser(h: &Home) -> (String, String, std::path::PathBuf) {
    let log = h.0.join("chooser.log");
    let prog = h.file(
        "chooser.sh",
        r#"#!/bin/sh
for a in "$@"; do printf '%s\n' "$a"; done > "$FAKE_LOG"
case "$FAKE_REPLY" in
  cancel) exit 0 ;;
  "item "*) n=${FAKE_REPLY#item }; shift 3; shift $((n-1)); printf '%s' "$1" ;;
  "raw "*) printf '%s' "${FAKE_REPLY#raw }" ;;
esac
"#,
    );
    std::fs::set_permissions(&prog, std::fs::Permissions::from_mode(0o700)).unwrap();
    (prog.display().to_string(), log.display().to_string(), log)
}

fn pick(h: &Home, conf: &std::path::Path, s: &Server, reply: &str) -> (Output, Vec<String>) {
    let (prog, log, logp) = chooser(h);
    let id = bar_cliproxyapi::switch::instance_id(&s.origin());
    let a = json!({"action":"pick","tokens":{"provider":"claude","instance":id}});
    let o = h.run_env(
        conf,
        &["--action", "pick"],
        &a.to_string(),
        &[
            ("BAR_CLIPROXYAPI_CHOOSER", prog),
            ("FAKE_LOG", log),
            ("FAKE_REPLY", reply.to_string()),
        ],
    );
    let argv = std::fs::read_to_string(logp)
        .unwrap_or_default()
        .lines()
        .map(String::from)
        .collect();
    (o, argv)
}

#[test]
fn pick_offers_the_ranking_with_the_top_recommendation_selected() {
    let (s, _) = pool(quota_fleet(), KEY);
    let h = Home::new("chooselist");
    let conf = h.conf(&[s.origin()]);
    let (o, argv) = pick(&h, &conf, &s, "cancel");
    assert!(o.status.success(), "{}", err(&o));
    // title, prompt, default, then the items: most left first, the
    // proxy's preferred account marked, the unread one last
    assert_eq!(argv.len(), 3 + 5, "{argv:?}");
    let items = &argv[3..];
    assert_eq!(argv[2], items[0]);
    let labels: Vec<&str> = items.iter().map(|i| i.split(' ').nth(1).unwrap()).collect();
    assert_eq!(labels, ["b", "c", "d", "a", "e"], "{items:?}");
    assert!(
        items[0].starts_with("1. b \u{2014} 7d 90% left \u{21BB}"),
        "{}",
        items[0]
    );
    assert!(items[3].contains("[current]") && !items[0].contains("[current]"));
    assert!(items[4].contains("no data"), "{}", items[4]);
    assert!(!argv.iter().any(|a| a.contains("corp.test")));
}

#[test]
fn pick_cancel_changes_nothing() {
    let (s, _) = pool(quota_fleet(), KEY);
    let h = Home::new("choosecancel");
    let conf = h.conf(&[s.origin()]);
    for reply in ["cancel", "raw false"] {
        let (o, _) = pick(&h, &conf, &s, reply);
        assert!(o.status.success(), "{}", err(&o));
        assert_eq!(text(&o).trim(), "cancelled");
    }
    assert!(s.patches().is_empty());
}

#[test]
fn pick_applies_the_choice_as_use_does() {
    let (s, st) = pool(quota_fleet(), KEY);
    let h = Home::new("choosepick");
    let conf = h.conf(&[s.origin()]);
    // the third row is d
    let (o, _) = pick(&h, &conf, &s, "item 3");
    assert!(o.status.success(), "{}", err(&o));
    assert!(
        text(&o).contains("d (claude): priority 0 -> 6"),
        "{}",
        text(&o)
    );
    assert_eq!(prio(&st.lock().unwrap(), "d"), 6);
    assert_eq!(s.patches().len(), 1);
    // `use --reset` can undo it: the previous priority was recorded
    let o = h.run(&conf, &["use", "--reset", "d"], "");
    assert!(o.status.success(), "{}", err(&o));
    assert_eq!(prio(&st.lock().unwrap(), "d"), 0);
}

#[test]
fn pick_refuses_a_reply_that_was_not_offered() {
    let (s, _) = pool(quota_fleet(), KEY);
    let h = Home::new("choosebad");
    let conf = h.conf(&[s.origin()]);
    let (o, _) = pick(&h, &conf, &s, "raw 9. zed \u{2014} nothing");
    assert_eq!(o.status.code(), Some(1));
    assert!(err(&o).contains("not offered"), "{}", err(&o));
    assert!(s.patches().is_empty());
}

#[test]
fn pick_stops_on_a_refused_key() {
    let (s, _) = pool(quota_fleet(), "another-key");
    let h = Home::new("choose401");
    let conf = h.conf(&[s.origin()]);
    let (o, argv) = pick(&h, &conf, &s, "item 1");
    assert_eq!(o.status.code(), Some(1));
    assert!(err(&o).contains("401"), "{}", err(&o));
    assert!(
        argv.is_empty(),
        "the chooser is not shown without a listing"
    );
    assert_eq!(s.count(), 1);
    let (o, _) = pick(&h, &conf, &s, "item 1");
    assert!(!o.status.success());
    assert_eq!(s.count(), 1);
}

#[test]
fn pick_validates_its_tokens() {
    let (s, _) = pool(quota_fleet(), KEY);
    let h = Home::new("choosetok");
    let conf = h.conf(&[s.origin()]);
    for t in [
        json!({"provider":"claude"}),
        json!({"provider":"nope","instance":"0123456789abcdef"}),
        json!({"provider":"claude","instance":"0123456789abcdef"}),
    ] {
        let o = h.run(
            &conf,
            &["--action", "pick"],
            &json!({"action":"pick","tokens":t}).to_string(),
        );
        assert_eq!(o.status.code(), Some(1));
    }
    assert_eq!(s.count(), 0);
}

#[test]
fn labels_are_sanitized_and_never_reach_the_script() {
    use bar_cliproxyapi::chooser;
    assert_eq!(chooser::sanitize("a\u{1b}[31m\nb\"c", 40), "a [31m b\"c");
    assert_eq!(chooser::sanitize(&"x".repeat(500), 40).chars().count(), 40);
    // the script is fixed text: it names argv and holds no placeholder
    assert!(chooser::SCRIPT.contains("on run argv"));
}

// --- window ids in the pick text ---

/// A claude account with a 5h and a 7d window.
fn two_windows(name: &str, left5: u32, left7: u32) -> Value {
    let mut v = quota(name, left7, 500_000, None);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let sig = v["quota"]["signals"].as_object_mut().unwrap();
    sig.insert(
        "Anthropic-Ratelimit-Unified-5h-Utilization".into(),
        format!("{}", (100 - left5) as f64 / 100.0).into(),
    );
    sig.insert(
        "Anthropic-Ratelimit-Unified-5h-Reset".into(),
        (now + 7200).to_string().into(),
    );
    v
}

#[test]
fn pick_text_drops_the_window_id_only_when_every_row_shows_the_same_single_window() {
    let one = |fleet: Value| {
        let (s, _) = pool(fleet, KEY);
        let h = Home::new("pickprefix");
        let conf = h.conf(&[s.origin()]);
        let listed = text(&h.run(&conf, &["pick", "--provider", "claude"], ""));
        let (_, argv) = pick(&h, &conf, &s, "cancel");
        (listed, argv[3..].to_vec())
    };
    // every account 7d only: no ids, in the list and the chooser alike
    let (listed, items) = one(json!([
        quota("a", 20, 100_000, None),
        quota("b", 90, 500_000, None)
    ]));
    assert!(listed.lines().all(|l| !l.contains("7d")), "{listed}");
    assert!(listed.contains("90% left"), "{listed}");
    assert!(
        items[0].starts_with("1. b \u{2014} 90% left \u{21BB}"),
        "{items:?}"
    );
    assert!(items.iter().all(|i| !i.contains("7d")), "{items:?}");
    // one account with two windows: every id stays
    let (listed, items) = one(json!([
        two_windows("a", 50, 20),
        quota("b", 90, 500_000, None)
    ]));
    assert!(listed.lines().all(|l| l.contains("7d")), "{listed}");
    assert!(listed.contains("5h 50% left"), "{listed}");
    assert!(items.iter().all(|i| i.contains("7d")), "{items:?}");
}

// --- the user's account lines ---

const RULES: &str = r##"[{"glob":"bob@*","label":"team","color":"33"},
    {"glob":"*@corp.test","label":"corp","color":"#112233"}]"##;

fn with_rules(h: &Home, conf: &std::path::Path, args: &[&str]) -> Output {
    h.run_env(conf, args, "", &[("BAR_ACCOUNTS", RULES.to_string())])
}

#[test]
fn names_come_from_the_users_account_lines_the_plugin_s_aliases_first() {
    let (s, st) = pool(fleet(), KEY);
    let h = Home::new("rulesnames");
    // an alias for alice only
    let key = h.file("key", &format!("{KEY}\n"));
    let conf = h.file(
        "c.conf",
        &format!(
            "instance {}\n key-file {}\n alias \"alice@*\" mine\n",
            s.origin(),
            key.display()
        ),
    );
    let o = with_rules(&h, &conf, &["list", "--provider", "claude"]);
    assert!(o.status.success(), "{}", err(&o));
    let t = text(&o);
    let labels: Vec<&str> = t
        .lines()
        .map(|l| l.split_whitespace().next().unwrap())
        .collect();
    // alice: the plugin's alias beats the lines; bob: the first line;
    // cy: the second; no line, no alias: the local part
    assert_eq!(labels, ["mine", "team", "corp"], "{t}");
    assert!(!t.contains("corp.test") && !t.contains("33"), "{t}");
    // the names work for `use`
    let o = with_rules(&h, &conf, &["use", "team", "--provider", "claude"]);
    assert!(o.status.success(), "{}", err(&o));
    assert_eq!(prio(&st.lock().unwrap(), "b"), 3);
    let o = with_rules(&h, &conf, &["use", "alice", "--provider", "claude"]);
    assert_eq!(
        o.status.code(),
        Some(1),
        "the local part is no longer her name"
    );
    // without the lines (a plugin started directly): the local parts
    let o = h.run(&conf, &["list", "--provider", "claude"], "");
    assert!(text(&o).starts_with("mine"), "{}", text(&o));
    assert!(text(&o).contains("bob"), "{}", text(&o));
}

#[test]
fn the_chooser_shows_the_users_labels_and_never_an_email_or_a_glob() {
    let (s, _) = pool(
        json!([quota("a", 20, 100_000, None), quota("b", 90, 500_000, None)]),
        KEY,
    );
    let h = Home::new("ruleschoose");
    let conf = h.conf(&[s.origin()]);
    let (prog, log, logp) = chooser(&h);
    let id = bar_cliproxyapi::switch::instance_id(&s.origin());
    // the rules arrive in the action's stdin, as a click delivers them
    let a = json!({"action":"pick","tokens":{"provider":"claude","instance":id},
        "accounts":[{"glob":"b@*","label":"team","color":"33"}]});
    let o = h.run_env(
        &conf,
        &["--action", "pick"],
        &a.to_string(),
        &[
            ("BAR_CLIPROXYAPI_CHOOSER", prog),
            ("FAKE_LOG", log),
            ("FAKE_REPLY", "cancel".into()),
        ],
    );
    assert!(o.status.success(), "{}", err(&o));
    let argv = std::fs::read_to_string(logp).unwrap();
    assert!(argv.contains("1. team \u{2014}"), "{argv}");
    assert!(argv.contains("2. a \u{2014}"), "{argv}");
    assert!(
        !argv.contains("corp.test") && !argv.contains("b@*"),
        "{argv}"
    );
}
