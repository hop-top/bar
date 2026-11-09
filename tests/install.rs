//! `bar install`: statusLine in Claude Code's settings, diff first,
//! confirmation, backup, other keys kept, `--uninstall` restores.

mod common;
#[path = "common/render_fx.rs"]
mod render_fx;

use assert_cmd::Command;
use hop_bar::install::{self, Env, Error, Opts, Scope};
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

fn opts() -> Opts {
    Opts {
        uninstall: false,
        refresh: None,
        hooks: false,
        yes: true,
    }
}

fn run(e: &Env, o: &Opts, tty: bool, answer: &str) -> (Result<(), Error>, String) {
    let mut out = Vec::new();
    let r = install::run(e, o, tty, &mut answer.as_bytes(), &mut out);
    (r, String::from_utf8(out).unwrap())
}

fn read(p: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
}

fn backups(dir: &Path) -> usize {
    std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().contains(".bak-"))
        .count()
}

const EXISTING: &str = r#"{
  "theme": "dark",
  "statusLine": {
    "type": "command",
    "command": "~/.claude/statusline.sh",
    "padding": 2
  },
  "permissions": {
    "allow": ["Bash(ls)"]
  }
}
"#;

#[test]
fn install_replaces_status_line_keeps_other_keys_and_backs_up() {
    let t = Tmp::new();
    t.write("claude/settings.json", EXISTING);
    let s = t.path().join("claude/settings.json");
    let e = env(&t, s.clone());
    let (r, out) = run(&e, &opts(), false, "");
    r.unwrap();
    let cmd = format!("'{}' render", e.bar.display());
    assert!(
        out.contains("-    \"command\": \"~/.claude/statusline.sh\","),
        "{out}"
    );
    assert!(
        out.contains(&format!("+    \"command\": {},", json!(cmd))),
        "{out}"
    );
    let v = read(&s);
    assert_eq!(v["statusLine"]["command"], json!(cmd));
    assert_eq!(v["statusLine"]["padding"], json!(2));
    assert_eq!(v["theme"], json!("dark"));
    assert_eq!(v["permissions"]["allow"], json!(["Bash(ls)"]));
    // key order kept
    let keys: Vec<_> = v.as_object().unwrap().keys().cloned().collect();
    assert_eq!(keys, ["theme", "statusLine", "permissions"]);
    assert_eq!(backups(&t.path().join("claude")), 1);
    // second run: nothing to do, no new backup
    let (r, out) = run(&e, &opts(), false, "");
    r.unwrap();
    assert!(out.contains("already up to date"), "{out}");
    assert_eq!(backups(&t.path().join("claude")), 1);
}

#[test]
fn uninstall_restores_the_replaced_status_line() {
    let t = Tmp::new();
    t.write("claude/settings.json", EXISTING);
    let s = t.path().join("claude/settings.json");
    let e = env(&t, s.clone());
    run(&e, &opts(), false, "").0.unwrap();
    let un = Opts {
        uninstall: true,
        ..opts()
    };
    let (r, out) = run(&e, &un, false, "");
    r.unwrap();
    assert!(out.contains("restores"), "{out}");
    assert_eq!(read(&s), serde_json::from_str::<Value>(EXISTING).unwrap());
    let (r, out) = run(&e, &un, false, "");
    r.unwrap();
    assert!(out.contains("nothing to remove"), "{out}");
}

#[test]
fn uninstall_removes_status_line_when_none_was_replaced() {
    let t = Tmp::new();
    let s = t.path().join("claude/settings.json");
    let e = env(&t, s.clone());
    let (r, out) = run(&e, &opts(), false, "");
    r.unwrap();
    assert!(out.starts_with("--- /dev/null\n"), "{out}");
    assert_eq!(backups(&t.path().join("claude")), 0);
    let un = Opts {
        uninstall: true,
        ..opts()
    };
    run(&e, &un, false, "").0.unwrap();
    assert_eq!(read(&s), json!({}));
}

#[test]
fn uninstall_leaves_a_foreign_status_line() {
    let t = Tmp::new();
    t.write("claude/settings.json", EXISTING);
    let s = t.path().join("claude/settings.json");
    let e = env(&t, s.clone());
    let un = Opts {
        uninstall: true,
        ..opts()
    };
    let (r, out) = run(&e, &un, false, "");
    r.unwrap();
    assert!(out.contains("left as is"), "{out}");
    assert_eq!(std::fs::read_to_string(&s).unwrap(), EXISTING);
}

#[test]
fn confirmation_is_asked_and_required() {
    let t = Tmp::new();
    t.write("claude/settings.json", EXISTING);
    let s = t.path().join("claude/settings.json");
    let e = env(&t, s.clone());
    let ask = Opts {
        yes: false,
        ..opts()
    };
    // no terminal, no --yes: diff shown, refused
    let (r, out) = run(&e, &ask, false, "y\n");
    assert_eq!(r, Err(Error::NeedsConfirm));
    assert!(out.contains("+++ "), "{out}");
    assert_eq!(std::fs::read_to_string(&s).unwrap(), EXISTING);
    for no in ["\n", "n\n", ""] {
        let (r, out) = run(&e, &ask, true, no);
        assert_eq!(r, Err(Error::Declined));
        assert!(out.contains("[y/N]"), "{out}");
        assert_eq!(std::fs::read_to_string(&s).unwrap(), EXISTING);
    }
    let (r, _) = run(&e, &ask, true, "y\n");
    r.unwrap();
    assert!(read(&s)["statusLine"]["command"]
        .as_str()
        .unwrap()
        .ends_with(" render"));
}

#[test]
fn refresh_and_hooks() {
    let t = Tmp::new();
    let s = t.path().join("claude/settings.json");
    let e = env(&t, s.clone());
    let o = Opts {
        refresh: Some(5),
        hooks: true,
        ..opts()
    };
    run(&e, &o, false, "").0.unwrap();
    let v = read(&s);
    assert_eq!(v["statusLine"]["refreshInterval"], json!(5));
    let hook = format!("'{}' click hook", e.bar.display());
    for ev in hop_bar::click::HOOK_EVENTS {
        assert_eq!(
            v["hooks"][ev][0]["hooks"][0]["command"],
            json!(hook),
            "{ev}"
        );
    }
    let un = Opts {
        uninstall: true,
        hooks: true,
        ..opts()
    };
    run(&e, &un, false, "").0.unwrap();
    assert_eq!(read(&s), json!({}));
}

#[test]
fn hooks_uninstall_keeps_other_hooks() {
    let t = Tmp::new();
    t.write(
        "claude/settings.json",
        r#"{"hooks": {"Stop": [{"hooks": [{"type": "command", "command": "say done"}]}]}}"#,
    );
    let s = t.path().join("claude/settings.json");
    let e = env(&t, s.clone());
    let o = Opts {
        hooks: true,
        ..opts()
    };
    run(&e, &o, false, "").0.unwrap();
    assert_eq!(read(&s)["hooks"]["Stop"].as_array().unwrap().len(), 2);
    let un = Opts {
        uninstall: true,
        hooks: true,
        ..opts()
    };
    run(&e, &un, false, "").0.unwrap();
    assert_eq!(
        read(&s),
        json!({"hooks": {"Stop": [{"hooks": [{"type": "command", "command": "say done"}]}]}})
    );
}

#[test]
fn unparsable_settings_are_left_alone() {
    let t = Tmp::new();
    t.write("claude/settings.json", "{ not json");
    let s = t.path().join("claude/settings.json");
    let (r, _) = run(&env(&t, s.clone()), &opts(), false, "");
    assert!(matches!(r, Err(Error::Failed(_))));
    assert_eq!(std::fs::read_to_string(&s).unwrap(), "{ not json");
}

#[test]
fn windows_command_uses_forward_slashes() {
    let t = Tmp::new();
    let s = t.path().join("claude/settings.json");
    let e = Env {
        bar: PathBuf::from(r"C:\Users\me\.cargo\bin\bar.exe"),
        windows: true,
        ..env(&t, s.clone())
    };
    run(&e, &opts(), false, "").0.unwrap();
    assert_eq!(
        read(&s)["statusLine"]["command"],
        json!("C:/Users/me/.cargo/bin/bar.exe render")
    );
}

#[test]
fn scopes_pick_the_settings_file() {
    let cwd = Path::new("/p");
    let claude = Some(PathBuf::from("/h/.claude"));
    let p = |s| install::settings_path(s, claude.clone(), cwd).unwrap();
    assert_eq!(p(Scope::User), Path::new("/h/.claude/settings.json"));
    assert_eq!(p(Scope::Project), Path::new("/p/.claude/settings.json"));
    assert_eq!(p(Scope::Local), Path::new("/p/.claude/settings.local.json"));
    assert!(install::settings_path(Scope::User, None, cwd).is_err());
}

fn cli(t: &Tmp, args: &[&str]) -> std::process::Output {
    let (envs, cwd) = common::isolation(t.path());
    Command::cargo_bin("bar")
        .unwrap()
        .arg("install")
        .args(args)
        .envs(envs)
        .current_dir(cwd)
        .output()
        .unwrap()
}

#[test]
fn cli_refuses_without_terminal_and_writes_with_yes() {
    let t = Tmp::new();
    let s = t.path().join("claude").join("settings.json");
    let o = cli(&t, &[]);
    assert_eq!(o.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&o.stderr).contains("--yes"));
    assert!(String::from_utf8_lossy(&o.stdout).contains("statusLine"));
    assert!(!s.exists());
    let o = cli(&t, &["--yes", "--refresh", "10"]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(read(&s)["statusLine"]["refreshInterval"], json!(10));
    let o = cli(&t, &["--refresh", "0", "--yes"]);
    assert_eq!(o.status.code(), Some(2));
}

#[test]
fn cli_project_and_local_scopes_write_under_cwd() {
    let t = Tmp::new();
    let (_, cwd) = common::isolation(t.path());
    let o = cli(&t, &["--project", "--yes"]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(cwd.join(".claude/settings.json").is_file());
    let o = cli(&t, &["--local", "--yes"]);
    assert!(o.status.success());
    assert!(cwd.join(".claude/settings.local.json").is_file());
    assert!(!t.path().join("claude/settings.json").exists());
    let o = cli(&t, &["--project", "--local", "--yes"]);
    assert_eq!(o.status.code(), Some(2));
}
