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
        subagent: false,
        subagent_only: false,
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

/// Backups of settings files: in the state dir; none beside `dir`.
fn backups(t: &Tmp, dir: &Path) -> usize {
    let count = |d: &Path| {
        std::fs::read_dir(d).map_or(0, |rd| {
            rd.flatten()
                .filter(|e| e.file_name().to_string_lossy().contains(".bak-"))
                .count()
        })
    };
    assert_eq!(count(dir), 0, "backup beside {}", dir.display());
    count(&t.path().join("state").join("backups"))
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
    assert_eq!(backups(&t, &t.path().join("claude")), 1);
    // second run: nothing to do, no new backup
    let (r, out) = run(&e, &opts(), false, "");
    r.unwrap();
    assert!(out.contains("already up to date"), "{out}");
    assert_eq!(backups(&t, &t.path().join("claude")), 1);
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
    assert_eq!(backups(&t, &t.path().join("claude")), 0);
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

#[test]
fn local_scope_backup_stays_out_of_the_project() {
    let t = Tmp::new();
    t.write(
        "proj/.claude/settings.local.json",
        "{\"env\": {\"K\": \"v\"}}\n",
    );
    let dir = t.path().join("proj/.claude");
    let e = env(&t, dir.join("settings.local.json"));
    let (r, out) = run(&e, &opts(), false, "");
    r.unwrap();
    assert_eq!(backups(&t, &dir), 1);
    let store = t.path().join("state").join("backups");
    assert!(
        out.contains(&format!("backup {}", store.display())),
        "{out}"
    );
}

/// Answers `y`, after editing `file` as another program would while the
/// question is up.
struct EditWhileAsking {
    file: PathBuf,
    answered: bool,
}

impl std::io::Read for EditWhileAsking {
    fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
        unreachable!()
    }
}

impl std::io::BufRead for EditWhileAsking {
    fn fill_buf(&mut self) -> std::io::Result<&[u8]> {
        if !self.answered {
            std::fs::write(&self.file, "{\"theme\": \"light\"}\n").unwrap();
            self.answered = true;
            return Ok(b"y\n");
        }
        Ok(b"")
    }
    fn consume(&mut self, _: usize) {}
}

#[test]
fn a_file_changed_during_the_question_is_not_overwritten() {
    let t = Tmp::new();
    t.write("claude/settings.json", EXISTING);
    let s = t.path().join("claude/settings.json");
    let e = env(&t, s.clone());
    let o = Opts {
        yes: false,
        ..opts()
    };
    let mut input = EditWhileAsking {
        file: s.clone(),
        answered: false,
    };
    let mut out = Vec::new();
    let r = install::run(&e, &o, true, &mut input, &mut out);
    let Err(Error::Failed(msg)) = r else {
        panic!("{r:?}");
    };
    assert!(msg.contains("changed since it was read"), "{msg}");
    assert_eq!(read(&s), json!({"theme": "light"}));
    assert!(!t.path().join("state").exists());
}

#[test]
fn cli_project_scope_writes_a_portable_command() {
    let t = Tmp::new();
    let (_, cwd) = common::isolation(t.path());
    let o = cli(&t, &["--project", "--hooks", "--yes"]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let v = read(&cwd.join(".claude/settings.json"));
    assert_eq!(v["statusLine"]["command"], json!("bar render"));
    assert_eq!(
        v["hooks"]["Stop"][0]["hooks"][0]["command"],
        json!("bar click hook")
    );
    let o = cli(&t, &["--local", "--yes"]);
    assert!(o.status.success());
    let v = read(&cwd.join(".claude/settings.local.json"));
    assert_ne!(v["statusLine"]["command"], json!("bar render"));
}

#[test]
fn uninstall_hooks_removes_bar_hooks_of_any_path() {
    let t = Tmp::new();
    let old = r#"{"hooks": {
      "Stop": [
        {"hooks": [{"type": "command", "command": "'/opt/homebrew/Cellar/bar/0.1.0/bin/bar' click hook"}]},
        {"hooks": [{"type": "command", "command": "say done"}]}
      ],
      "SessionEnd": [{"hooks": [{"type": "command", "command": "bar click hook"}]}]
    }}"#;
    t.write("claude/settings.json", old);
    let s = t.path().join("claude/settings.json");
    let e = env(&t, s.clone());
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
fn install_hooks_replaces_bar_hooks_of_another_path() {
    let t = Tmp::new();
    let old = r#"{"hooks": {"Stop": [
        {"hooks": [{"type": "command", "command": "'/opt/homebrew/Cellar/bar/0.1.0/bin/bar' click hook"}]}
    ]}}"#;
    t.write("claude/settings.json", old);
    let s = t.path().join("claude/settings.json");
    let e = env(&t, s.clone());
    let o = Opts {
        hooks: true,
        ..opts()
    };
    run(&e, &o, false, "").0.unwrap();
    let stop = read(&s)["hooks"]["Stop"].clone();
    assert_eq!(stop.as_array().unwrap().len(), 1, "{stop}");
    assert_eq!(
        stop[0]["hooks"][0]["command"],
        json!(format!("'{}' click hook", e.bar.display()))
    );
}

#[cfg(unix)]
#[test]
fn stable_exe_survives_a_homebrew_upgrade() {
    use install::stable_exe;
    let t = Tmp::new();
    t.write("brew/Cellar/bar/1.2.0/bin/bar", "bin");
    let cellar = t.path().join("brew/Cellar/bar/1.2.0/bin/bar");
    std::fs::create_dir_all(t.path().join("brew/bin")).unwrap();
    let link = t.path().join("brew/bin/bar");
    std::os::unix::fs::symlink("../Cellar/bar/1.2.0/bin/bar", &link).unwrap();
    // the brew prefix's bin, without PATH
    assert_eq!(stable_exe(&cellar, None), link);
    // a `bar` on PATH that is this binary
    t.write("other/bar", "other");
    std::fs::create_dir_all(t.path().join("pathbin")).unwrap();
    let on_path = t.path().join("pathbin/bar");
    std::os::unix::fs::symlink(&cellar, &on_path).unwrap();
    let path = std::env::join_paths([t.path().join("other"), t.path().join("pathbin")]).unwrap();
    assert_eq!(stable_exe(&cellar, Some(&path)), on_path);
    // nothing better: the binary itself
    let lone = t.path().join("other/bar");
    assert_eq!(stable_exe(&lone, None), lone);
    // a prefix bin/bar that is another binary is not used
    std::fs::remove_file(&link).unwrap();
    t.write("brew/bin/bar", "different");
    assert_eq!(stable_exe(&cellar, None), cellar);
}

#[test]
fn windows_path_with_spaces_falls_back_to_bar_on_path() {
    use install::command_bar;
    let t = Tmp::new();
    t.write("my bin/bar", "bin");
    let exe = t.path().join("my bin/bar");
    let path = std::env::join_paths([t.path().join("my bin")]).unwrap();
    // both shells run a bare `bar` found on PATH
    assert_eq!(command_bar(&exe, Some(&path), true), PathBuf::from("bar"));
    // not on PATH: the path itself (single-quoted, Git Bash only)
    assert_eq!(command_bar(&exe, None, true), exe);
    // elsewhere quoting works: the path
    assert_eq!(command_bar(&exe, Some(&path), false), exe);
    t.write("plain/bar", "bin");
    let plain = t.path().join("plain/bar");
    if !plain.display().to_string().contains(' ') {
        let path = std::env::join_paths([t.path().join("plain")]).unwrap();
        assert_eq!(command_bar(&plain, Some(&path), true), plain);
    }
}

/// [`EXISTING`] as serde_json lays it out.
const PRETTY: &str = r#"{
  "theme": "dark",
  "statusLine": {
    "type": "command",
    "command": "~/.claude/statusline.sh",
    "padding": 2
  },
  "permissions": {
    "allow": [
      "Bash(ls)"
    ]
  }
}
"#;

/// `+`/`-` lines of a diff, headers left out.
fn changes(diff: &str) -> Vec<&str> {
    diff.lines()
        .filter(|l| {
            (l.starts_with('+') || l.starts_with('-'))
                && !l.starts_with("+++")
                && !l.starts_with("---")
        })
        .collect()
}

#[test]
fn diff_and_write_keep_the_file_indentation() {
    for indent in ["    ", "\t", "  "] {
        let t = Tmp::new();
        let text = PRETTY.replace("  ", indent);
        t.write("claude/settings.json", &text);
        let s = t.path().join("claude/settings.json");
        let e = env(&t, s.clone());
        let (r, out) = run(&e, &opts(), false, "");
        r.unwrap();
        let cmd = format!("'{}' render", e.bar.display());
        assert_eq!(
            changes(&out),
            [
                format!("-{indent}{indent}\"command\": \"~/.claude/statusline.sh\","),
                format!("+{indent}{indent}\"command\": {},", json!(cmd)),
            ],
            "{out}"
        );
        let written = std::fs::read_to_string(&s).unwrap();
        assert_eq!(
            written,
            text.replace("\"~/.claude/statusline.sh\"", &json!(cmd).to_string())
        );
    }
}

#[test]
fn diff_of_a_one_line_file_shows_only_the_change() {
    let t = Tmp::new();
    t.write(
        "claude/settings.json",
        r#"{"theme":"dark","model":"opus","statusLine":{"type":"command","command":"x"}}"#,
    );
    let s = t.path().join("claude/settings.json");
    let e = env(&t, s.clone());
    let (r, out) = run(&e, &opts(), false, "");
    r.unwrap();
    assert_eq!(changes(&out).len(), 2, "{out}");
    assert!(out.contains("  \"model\": \"opus\","), "{out}");
}

#[test]
fn reformatted_file_diff_shows_content_and_says_so() {
    let t = Tmp::new();
    t.write("claude/settings.json", EXISTING);
    let s = t.path().join("claude/settings.json");
    let e = env(&t, s.clone());
    let (r, out) = run(&e, &opts(), false, "");
    r.unwrap();
    assert_eq!(changes(&out).len(), 2, "{out}");
    assert!(out.contains("reformatted"), "{out}");
}

#[test]
fn a_binary_in_the_npx_cache_gets_a_warning() {
    let t = Tmp::new();
    let settings = t.path().join("claude/settings.json");
    let npx = t
        .path()
        .join(".npm/_npx/0123abcd/node_modules/@hop-top/bar-linux-x64/bin/bar");
    let e = Env {
        bar: npx,
        ..env(&t, settings.clone())
    };
    let (r, out) = run(&e, &opts(), false, "");
    assert_eq!(r, Ok(()), "{out}");
    assert!(out.contains("npx cache"), "{out}");
    assert!(out.contains("npm install -g @hop-top/bar"), "{out}");
    // a regular install: no warning
    let (_, out) = run(&env(&t, t.path().join("other.json")), &opts(), false, "");
    assert!(!out.contains("npx cache"), "{out}");
    // a directory merely named like it is not the cache
    let e = Env {
        bar: t.path().join("my_npx_tools/bar"),
        ..env(&t, t.path().join("third.json"))
    };
    assert!(!run(&e, &opts(), false, "").1.contains("npx cache"));
}
