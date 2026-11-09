//! `bar install --subagent`: subagentStatusLine set beside statusLine,
//! a replaced one kept and put back by `--uninstall --subagent`.

mod common;
#[path = "common/render_fx.rs"]
mod render_fx;

use hop_bar::install::{self, Env, Error, Opts};
use render_fx::Tmp;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

fn env(t: &Tmp, settings: PathBuf) -> Env {
    Env {
        bar: t.path().join("bin").join("bar"),
        settings,
        state_dir: t.path().join("state"),
        windows: false,
    }
}

fn opts(subagent: bool, uninstall: bool) -> Opts {
    Opts {
        uninstall,
        refresh: None,
        hooks: false,
        subagent,
        subagent_only: false,
        yes: true,
    }
}

fn only(uninstall: bool) -> Opts {
    Opts {
        subagent: false,
        subagent_only: true,
        ..opts(false, uninstall)
    }
}

fn run(e: &Env, o: &Opts) -> (Result<(), Error>, String) {
    let mut out = Vec::new();
    let r = install::run(e, o, false, &mut "".as_bytes(), &mut out);
    (r, String::from_utf8(out).unwrap())
}

fn read(p: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
}

const FOREIGN: &str = r#"{
  "statusLine": {
    "type": "command",
    "command": "~/.claude/statusline.sh"
  },
  "subagentStatusLine": {
    "type": "command",
    "command": "~/.claude/subagent-statusline.sh"
  }
}
"#;

#[test]
fn install_subagent_sets_both_lines() {
    let t = Tmp::new();
    let s = t.path().join("claude/settings.json");
    let e = env(&t, s.clone());
    run(&e, &opts(true, false)).0.unwrap();
    let bar = e.bar.display();
    assert_eq!(
        read(&s),
        json!({
            "statusLine": {"type": "command", "command": format!("'{bar}' render")},
            "subagentStatusLine": {"type": "command", "command": format!("'{bar}' subagent")},
        })
    );
    let (r, out) = run(&e, &opts(true, false));
    r.unwrap();
    assert!(out.contains("already up to date"), "{out}");
}

#[test]
fn install_without_the_flag_leaves_the_subagent_line() {
    let t = Tmp::new();
    t.write("claude/settings.json", FOREIGN);
    let s = t.path().join("claude/settings.json");
    let e = env(&t, s.clone());
    run(&e, &opts(false, false)).0.unwrap();
    let v = read(&s);
    assert_eq!(
        v["subagentStatusLine"]["command"],
        json!("~/.claude/subagent-statusline.sh")
    );
}

#[test]
fn replaced_lines_are_kept_and_put_back() {
    let t = Tmp::new();
    t.write("claude/settings.json", FOREIGN);
    let s = t.path().join("claude/settings.json");
    let e = env(&t, s.clone());
    let (r, out) = run(&e, &opts(true, false));
    r.unwrap();
    assert!(out.contains("the current status line is kept"), "{out}");
    assert!(
        out.contains("the current subagent status line is kept")
            && out.contains("bar install --uninstall --subagent puts it back"),
        "{out}"
    );
    let rec = install::record_path(&e.state_dir, &s);
    let saved = read(&rec);
    assert_eq!(
        saved["statusLine"]["command"],
        json!("~/.claude/statusline.sh")
    );
    assert_eq!(
        saved["subagentStatusLine"]["command"],
        json!("~/.claude/subagent-statusline.sh")
    );

    // the status line alone first: the subagent line and its record stay
    let (r, out) = run(&e, &opts(false, true));
    r.unwrap();
    assert!(
        out.contains(
            "subagentStatusLine still runs bar; bar install --uninstall --subagent removes it"
        ),
        "{out}"
    );
    let v = read(&s);
    assert_eq!(v["statusLine"]["command"], json!("~/.claude/statusline.sh"));
    assert!(v["subagentStatusLine"]["command"]
        .as_str()
        .unwrap()
        .ends_with("' subagent"));
    assert!(read(&rec).get("statusLine").is_none());

    let (r, out) = run(&e, &opts(true, true));
    r.unwrap();
    assert!(
        out.contains("restores the subagent status line bar replaced"),
        "{out}"
    );
    assert_eq!(read(&s), serde_json::from_str::<Value>(FOREIGN).unwrap());
    assert!(!rec.exists(), "record removed once nothing is left in it");
}

#[test]
fn uninstall_subagent_removes_bars_line_when_none_was_replaced() {
    let t = Tmp::new();
    let s = t.path().join("claude/settings.json");
    let e = env(&t, s.clone());
    run(&e, &opts(true, false)).0.unwrap();
    run(&e, &opts(true, true)).0.unwrap();
    assert_eq!(read(&s), json!({}));
}

#[test]
fn uninstall_subagent_leaves_a_foreign_line() {
    let t = Tmp::new();
    t.write("claude/settings.json", FOREIGN);
    let s = t.path().join("claude/settings.json");
    let e = env(&t, s.clone());
    let (r, out) = run(&e, &opts(true, true));
    r.unwrap();
    assert!(
        out.contains("subagentStatusLine does not run bar; left as is"),
        "{out}"
    );
    assert_eq!(std::fs::read_to_string(&s).unwrap(), FOREIGN);
}

#[test]
fn subagent_command_quotes_per_platform() {
    assert_eq!(
        install::subagent_command("/opt/my bar/bar", false),
        "'/opt/my bar/bar' subagent"
    );
    assert_eq!(
        install::subagent_command(r"C:\Users\me\bin\bar.exe", true),
        "C:/Users/me/bin/bar.exe subagent"
    );
    assert_eq!(install::subagent_command("bar", false), "bar subagent");
}

#[test]
fn subagent_only_install_leaves_the_status_line() {
    let t = Tmp::new();
    t.write("claude/settings.json", FOREIGN);
    let s = t.path().join("claude/settings.json");
    let e = env(&t, s.clone());
    let (r, out) = run(&e, &only(false));
    r.unwrap();
    assert!(!out.contains("the current status line is kept"), "{out}");
    let v = read(&s);
    assert_eq!(v["statusLine"]["command"], json!("~/.claude/statusline.sh"));
    assert_eq!(
        v["subagentStatusLine"]["command"],
        json!(format!("'{}' subagent", e.bar.display()))
    );
    let (r, out) = run(&e, &only(true));
    r.unwrap();
    assert!(out.contains("restores the subagent status line"), "{out}");
    assert_eq!(read(&s), serde_json::from_str::<Value>(FOREIGN).unwrap());
}

#[test]
fn subagent_only_uninstall_keeps_bars_status_line() {
    let t = Tmp::new();
    let s = t.path().join("claude/settings.json");
    let e = env(&t, s.clone());
    run(&e, &opts(true, false)).0.unwrap();
    let (r, out) = run(&e, &only(true));
    r.unwrap();
    assert!(!out.contains("still runs bar"), "{out}");
    let v = read(&s);
    assert!(v.get("subagentStatusLine").is_none(), "{v}");
    assert!(v["statusLine"]["command"]
        .as_str()
        .unwrap()
        .ends_with("' render"));
    // the status line uninstalls on its own afterwards
    run(&e, &opts(false, true)).0.unwrap();
    assert_eq!(read(&s), json!({}));
}
