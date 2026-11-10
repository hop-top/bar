#![cfg(unix)]
//! `list`, `use` and `--action use` end to end against a fake management
//! server that keeps priorities: synthetic accounts only.

mod fake;

use fake::{pool, Server};
use serde_json::{json, Value};
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Output, Stdio};

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
        let mut ch = Command::new(BIN)
            .args(args)
            .env("BAR_CLIPROXYAPI_CONFIG", conf)
            .env("BAR_STATE_DIR", self.0.join("state"))
            .env_remove("ANTHROPIC_BASE_URL")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        ch.stdin
            .take()
            .unwrap()
            .write_all(stdin.as_bytes())
            .unwrap();
        ch.wait_with_output().unwrap()
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
    let o = Command::new(BIN).arg("--ext-info").output().unwrap();
    let v: Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(
        v["actions"]["use"],
        json!({"description":"Prefer this account","tokens":["account","provider"],"confirm":false})
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
