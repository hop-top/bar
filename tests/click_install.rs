// Unix fixtures (shell scripts, modes, POSIX paths); Windows coverage: tests/windows.rs
#![cfg(unix)]
mod common;

use assert_cmd::Command;
use hop_bar::click::{applescript, info_plist_commands, install_plan, run_install, InstallCfg};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

static N: AtomicUsize = AtomicUsize::new(0);

struct Tmp(PathBuf);
impl Tmp {
    fn new() -> Tmp {
        let p = crate::common::tmp::root().join(format!(
            "bar-click-install-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&p).unwrap();
        Tmp(p)
    }
    fn path(&self) -> &Path {
        &self.0
    }
}
impl Drop for Tmp {
    fn drop(&mut self) {
        crate::common::tmp::remove(&self.0);
    }
}

fn cfg(t: &Tmp) -> InstallCfg {
    InstallCfg {
        bar: PathBuf::from("/opt/bar dir/bar"),
        state_dir: t.path().join("state"),
        app: t.path().join("apps/bar.app"),
        settings: t.path().join("claude/settings.json"),
        env: Vec::new(),
    }
}

#[test]
fn applescript_dispatches_toggle_urls_only() {
    let s = applescript(Path::new("/opt/bar dir/bar\"x"), &[]);
    assert!(s.contains("on open location"));
    assert!(s.contains("\"bar://toggle/\""));
    assert!(s.contains("quoted form of theKey"));
    assert!(s.contains("/opt/bar dir/bar\\\"x"));
    assert!(s.contains(" toggle "));
}

#[test]
fn applescript_routes_action_urls_through_bar_click_action() {
    let s = applescript(Path::new("/opt/bar"), &[]);
    let lines: Vec<&str> = s.lines().map(str::trim).collect();
    let at = lines
        .iter()
        .position(|l| l.contains("starts with \"bar://action/\""))
        .expect("action branch");
    assert_eq!(lines[at + 1], "try");
    assert!(
        lines[at + 2].starts_with("do shell script")
            && lines[at + 2].contains("\" click action \"")
    );
    assert!(lines[at + 2].ends_with("quoted form of theURL"));
    assert_eq!(lines[at + 3], "end try");
}

#[test]
fn install_makes_a_private_action_nonce_and_keeps_it() {
    use std::os::unix::fs::PermissionsExt;
    let t = Tmp::new();
    let c = cfg(&t);
    let go = || {
        run_install(&c, true, false, &mut |_| Ok(()), &mut Vec::new()).unwrap();
        std::fs::read_to_string(c.state_dir.join("action-nonce")).unwrap()
    };
    let first = go();
    assert_eq!(first.len(), 32);
    assert!(first.bytes().all(|b| b.is_ascii_hexdigit()));
    let mode = std::fs::metadata(c.state_dir.join("action-nonce"))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
    assert_eq!(go(), first, "links drawn before a reinstall stay valid");
}

#[test]
fn failed_install_makes_no_nonce() {
    let t = Tmp::new();
    let c = cfg(&t);
    let _ = run_install(
        &c,
        true,
        false,
        &mut |_| Err("boom".into()),
        &mut Vec::new(),
    );
    assert!(!c.state_dir.join("action-nonce").exists());
}

#[test]
fn applescript_toggle_call_is_wrapped_in_try() {
    let s = applescript(Path::new("/opt/bar"), &[]);
    let lines: Vec<&str> = s.lines().map(str::trim).collect();
    let at = lines
        .iter()
        .position(|l| l.starts_with("do shell script") && l.contains("\" toggle \""))
        .expect("toggle call");
    assert_eq!(lines[at - 1], "try", "{s}");
    assert_eq!(lines[at + 1], "end try", "{s}");
}

#[test]
fn info_plist_sets_agent_and_scheme() {
    let cmds = info_plist_commands(Path::new("/x/bar.app"));
    let joined: Vec<String> = cmds.iter().map(|c| c.join(" ")).collect();
    assert!(joined
        .iter()
        .any(|c| c.contains("LSUIElement") && c.contains("-bool true")));
    assert!(joined
        .iter()
        .any(|c| c.contains("CFBundleURLSchemes") && c.contains("\"bar\"") && c.contains("-json")));
    assert!(cmds.iter().all(|c| c[0] == "plutil"));
    assert!(cmds
        .iter()
        .all(|c| c.last().unwrap() == "/x/bar.app/Contents/Info.plist"));
}

#[test]
fn install_plan_orders_steps() {
    let t = Tmp::new();
    let c = cfg(&t);
    let plan = install_plan(&c);
    let names: Vec<&str> = plan.commands.iter().map(|c| c[0].as_str()).collect();
    assert_eq!(names[0], "osacompile");
    assert_eq!(names[names.len() - 2], "codesign");
    assert!(names[names.len() - 1].ends_with("lsregister"));
    assert!(plan.commands[0].contains(&c.app.display().to_string()));
    let sign = &plan.commands[plan.commands.len() - 2];
    assert!(sign.contains(&"-".to_string()), "ad hoc signature");
    assert!(plan.script.contains("on open location"));
}

#[test]
fn install_runs_commands_via_runner_and_writes_marker() {
    let t = Tmp::new();
    let c = cfg(&t);
    let mut seen: Vec<Vec<String>> = Vec::new();
    let mut out = Vec::new();
    run_install(
        &c,
        true,
        false,
        &mut |cmd| {
            seen.push(cmd.to_vec());
            Ok(())
        },
        &mut out,
    )
    .unwrap();
    assert_eq!(seen, install_plan(&c).commands);
    let script = std::fs::read_to_string(c.state_dir.join("click-handler.applescript")).unwrap();
    assert!(script.contains("on open location"));
    let marker = c.state_dir.join("click-installed");
    assert_eq!(std::fs::read_to_string(&marker).unwrap(), "cmd");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            marker.metadata().unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}

#[test]
fn install_failure_leaves_no_marker() {
    let t = Tmp::new();
    let c = cfg(&t);
    let mut out = Vec::new();
    let r = run_install(&c, true, false, &mut |_| Err("boom".into()), &mut out);
    assert!(r.is_err());
    assert!(!c.state_dir.join("click-installed").exists());
}

#[test]
fn dry_run_runs_nothing_and_writes_nothing() {
    let t = Tmp::new();
    let c = cfg(&t);
    let mut out = Vec::new();
    run_install(&c, true, true, &mut |_| panic!("ran"), &mut out).unwrap();
    let text = String::from_utf8(out).unwrap();
    assert!(text.contains("osacompile"));
    assert!(text.contains("lsregister"));
    assert!(!c.state_dir.exists());
}

#[test]
fn non_macos_is_unsupported_and_does_nothing() {
    let t = Tmp::new();
    let c = cfg(&t);
    let mut out = Vec::new();
    let e = run_install(&c, false, false, &mut |_| panic!("ran"), &mut out).unwrap_err();
    assert_eq!(e, hop_bar::click::InstallError::Unsupported);
    assert!(e.to_string().contains("--hooks-only"));
    assert!(out.is_empty());
    assert!(!c.state_dir.exists());
}

#[cfg(not(target_os = "macos"))]
#[test]
fn install_cli_unsupported_exits_2() {
    let t = Tmp::new();
    let o = Command::cargo_bin("bar")
        .unwrap()
        .args(["click", "install"])
        .env("HOME", t.path())
        .env("XDG_CONFIG_HOME", t.path().join("cfg"))
        .env("XDG_STATE_HOME", t.path().join("st"))
        .env("BAR_STATE_DIR", t.path().join("bst"))
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(2));
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(
        err.contains("macOS") && err.contains("--hooks-only"),
        "{err}"
    );
    assert!(!t.path().join("bst").exists());
}

#[cfg(target_os = "macos")]
#[test]
fn install_cli_dry_run_is_safe() {
    let t = Tmp::new();
    let o = Command::cargo_bin("bar")
        .unwrap()
        .args(["click", "install", "--dry-run"])
        .env("BAR_STATE_DIR", t.path().join("state"))
        .env("BAR_APP_DIR", t.path().join("apps"))
        .env("CLAUDE_CONFIG_DIR", t.path().join("claude"))
        .env("HOME", t.path())
        .assert()
        .success();
    let _ = o;
    assert!(!t.path().join("state").exists());
    assert!(!t.path().join("apps").exists());
}

// ── Command route and settings.json hook ───────────────────────────────────

#[test]
fn applescript_routes_cmd_urls_to_click_run() {
    let s = applescript(Path::new("/opt/bar"), &[]);
    assert!(s.contains("\"bar://cmd/\""));
    assert!(s.contains("\" click run \" & quoted form of theURL"));
    assert!(s.contains("try"), "a refusal must not raise a dialog");
}

/// Backups of `c.settings`: in the state dir, none beside the file.
fn backups(c: &InstallCfg) -> Vec<PathBuf> {
    let list = |dir: &Path, head: &str| -> Vec<PathBuf> {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return Vec::new();
        };
        let mut v: Vec<PathBuf> = rd
            .map(|e| e.unwrap().path())
            .filter(|p| p.file_name().unwrap().to_string_lossy().starts_with(head))
            .collect();
        v.sort();
        v
    };
    assert!(list(c.settings.parent().unwrap(), "settings.json.bak-").is_empty());
    list(&c.state_dir.join("backups"), "settings.json-")
}

const ORIGINAL: &str = r#"{
  "statusLine": {"type": "command", "command": "bar render"},
  "model": "opus",
  "hooks": {
    "Stop": [{"hooks": [{"type": "command", "command": "afplay done.aiff"}]}]
  }
}
"#;

fn install(c: &InstallCfg, dry: bool) -> (Result<(), String>, String, usize) {
    let mut out = Vec::new();
    let mut ran = 0;
    let settings = c.settings.clone();
    let before = std::fs::read_to_string(&settings).ok();
    let r = run_install(
        c,
        true,
        dry,
        &mut |_| {
            // Commands run before the settings write.
            assert_eq!(std::fs::read_to_string(&settings).ok(), before);
            ran += 1;
            Ok(())
        },
        &mut out,
    );
    (
        r.map_err(|e| e.to_string()),
        String::from_utf8(out).unwrap(),
        ran,
    )
}

#[test]
fn install_lists_settings_change_then_writes_with_backup() {
    let t = Tmp::new();
    let c = cfg(&t);
    std::fs::create_dir_all(c.settings.parent().unwrap()).unwrap();
    std::fs::write(&c.settings, ORIGINAL).unwrap();
    let (r, out, _) = install(&c, false);
    r.unwrap();
    let hook = "'/opt/bar dir/bar' click hook";
    for ev in [
        "PermissionRequest",
        "Notification",
        "UserPromptSubmit",
        "Stop",
    ] {
        assert!(
            out.contains(&format!("add {ev} hook: {hook}")),
            "{ev}: {out}"
        );
    }
    let listed = out.find("add PermissionRequest").unwrap();
    let installed = out.find("installed ").unwrap();
    assert!(listed < installed, "change listed before writing");

    let v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&c.settings).unwrap()).unwrap();
    let keys: Vec<&String> = v.as_object().unwrap().keys().collect();
    assert_eq!(keys, ["statusLine", "model", "hooks"], "order kept");
    let stop = v["hooks"]["Stop"].as_array().unwrap();
    assert_eq!(stop[0]["hooks"][0]["command"], "afplay done.aiff");
    assert_eq!(stop[1]["hooks"][0]["command"], hook);
    assert_eq!(
        v["hooks"]["PermissionRequest"][0]["hooks"][0]["type"],
        "command"
    );

    let b = backups(&c);
    assert_eq!(b.len(), 1);
    assert_eq!(std::fs::read_to_string(&b[0]).unwrap(), ORIGINAL);
    assert!(
        out.contains(&b[0].display().to_string()),
        "backup path printed"
    );
    let leftovers = std::fs::read_dir(c.settings.parent().unwrap())
        .unwrap()
        .filter(|e| {
            e.as_ref()
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains("tmp")
        })
        .count();
    assert_eq!(leftovers, 0, "atomic write leaves no temp file");
}

#[test]
fn install_settings_is_idempotent() {
    let t = Tmp::new();
    let c = cfg(&t);
    std::fs::create_dir_all(c.settings.parent().unwrap()).unwrap();
    std::fs::write(&c.settings, ORIGINAL).unwrap();
    install(&c, false).0.unwrap();
    let once = std::fs::read_to_string(&c.settings).unwrap();
    let (r, out, _) = install(&c, false);
    r.unwrap();
    assert_eq!(std::fs::read_to_string(&c.settings).unwrap(), once);
    assert!(out.contains("hooks already wired"), "{out}");
    assert_eq!(backups(&c).len(), 1);
}

#[test]
fn install_creates_missing_settings_without_backup() {
    let t = Tmp::new();
    let c = cfg(&t);
    install(&c, false).0.unwrap();
    let v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&c.settings).unwrap()).unwrap();
    assert!(v["hooks"]["PermissionRequest"].is_array());
    assert!(backups(&c).is_empty());
}

#[test]
fn install_refuses_invalid_settings_untouched() {
    let t = Tmp::new();
    let c = cfg(&t);
    std::fs::create_dir_all(c.settings.parent().unwrap()).unwrap();
    for bad in [
        "{not json",
        "[1,2]",
        r#"{"hooks": []}"#,
        r#"{"hooks": {"Stop": {}}}"#,
    ] {
        std::fs::write(&c.settings, bad).unwrap();
        let (r, _, ran) = install(&c, false);
        assert!(r.is_err(), "{bad}");
        assert_eq!(ran, 0, "nothing runs when settings cannot be edited");
        assert_eq!(std::fs::read_to_string(&c.settings).unwrap(), bad);
        assert!(!c.state_dir.join("click-installed").exists());
    }
    assert!(backups(&c).is_empty());
}

#[test]
fn install_dry_run_lists_settings_change_without_writing() {
    let t = Tmp::new();
    let c = cfg(&t);
    std::fs::create_dir_all(c.settings.parent().unwrap()).unwrap();
    std::fs::write(&c.settings, ORIGINAL).unwrap();
    let (r, out, ran) = install(&c, true);
    r.unwrap();
    assert_eq!(ran, 0);
    assert!(out.contains("add PermissionRequest hook"));
    assert_eq!(std::fs::read_to_string(&c.settings).unwrap(), ORIGINAL);
    assert!(backups(&c).is_empty());
}

#[test]
fn install_replans_settings_after_concurrent_edit() {
    let t = Tmp::new();
    let c = cfg(&t);
    std::fs::create_dir_all(c.settings.parent().unwrap()).unwrap();
    std::fs::write(&c.settings, ORIGINAL).unwrap();
    let path = c.settings.clone();
    let mut out = Vec::new();
    let mut edited = false;
    run_install(
        &c,
        true,
        false,
        &mut |_| {
            if !edited {
                // Another writer changes settings.json while the app builds.
                let edit = ORIGINAL.replace("\"opus\"", "\"sonnet\"");
                std::fs::write(&path, edit).unwrap();
                edited = true;
            }
            Ok(())
        },
        &mut out,
    )
    .unwrap();
    let v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&c.settings).unwrap()).unwrap();
    assert_eq!(v["model"], "sonnet", "concurrent edit kept");
    assert!(v["hooks"]["PermissionRequest"].is_array());
    let b = backups(&c);
    assert!(std::fs::read_to_string(&b[0]).unwrap().contains("sonnet"));
}

#[test]
fn install_backups_are_unique() {
    let t = Tmp::new();
    let c = cfg(&t);
    std::fs::create_dir_all(c.settings.parent().unwrap()).unwrap();
    for _ in 0..3 {
        std::fs::write(&c.settings, ORIGINAL).unwrap();
        install(&c, false).0.unwrap();
    }
    assert_eq!(backups(&c).len(), 3);
}

#[test]
fn install_writes_through_settings_symlink() {
    let t = Tmp::new();
    let c = cfg(&t);
    let real = t.path().join("dotfiles/settings.json");
    std::fs::create_dir_all(real.parent().unwrap()).unwrap();
    std::fs::create_dir_all(c.settings.parent().unwrap()).unwrap();
    std::fs::write(&real, ORIGINAL).unwrap();
    std::os::unix::fs::symlink(&real, &c.settings).unwrap();
    install(&c, false).0.unwrap();
    let meta = std::fs::symlink_metadata(&c.settings).unwrap();
    assert!(meta.file_type().is_symlink(), "link kept");
    let v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&real).unwrap()).unwrap();
    assert!(v["hooks"]["PermissionRequest"].is_array(), "target written");

    // A dangling link is refused, not replaced.
    let t2 = Tmp::new();
    let c2 = cfg(&t2);
    std::fs::create_dir_all(c2.settings.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(t2.path().join("nowhere.json"), &c2.settings).unwrap();
    let (r, _, ran) = install(&c2, false);
    assert!(r.unwrap_err().contains("symlink"));
    assert_eq!(ran, 0);
    assert!(std::fs::symlink_metadata(&c2.settings)
        .unwrap()
        .file_type()
        .is_symlink());
}

#[test]
fn hooks_only_writes_hooks_idempotently_and_no_app() {
    let t = Tmp::new();
    let c = cfg(&t);
    std::fs::create_dir_all(c.settings.parent().unwrap()).unwrap();
    std::fs::write(
        &c.settings,
        r#"{"model":"x","hooks":{"Stop":[{"hooks":[{"type":"command","command":"other"}]}]}}"#,
    )
    .unwrap();
    let mut out = Vec::new();
    hop_bar::click::run_hooks_only(&c, false, &mut out).unwrap();
    let v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&c.settings).unwrap()).unwrap();
    assert_eq!(v["model"], "x");
    for ev in ["SubagentStart", "SubagentStop", "SessionEnd", "Stop"] {
        assert!(
            v["hooks"][ev].as_array().is_some_and(|a| !a.is_empty()),
            "{ev}"
        );
    }
    assert_eq!(v["hooks"]["Stop"].as_array().unwrap().len(), 2);
    assert!(!c.app.exists());
    // the state dir only holds the settings backup
    let state: Vec<_> = std::fs::read_dir(&c.state_dir)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(state, ["backups"]);
    let first = std::fs::read_to_string(&c.settings).unwrap();
    hop_bar::click::run_hooks_only(&c, false, &mut out).unwrap();
    assert_eq!(std::fs::read_to_string(&c.settings).unwrap(), first);
}

#[test]
fn hooks_only_dry_run_writes_nothing() {
    let t = Tmp::new();
    let c = cfg(&t);
    let mut out = Vec::new();
    hop_bar::click::run_hooks_only(&c, true, &mut out).unwrap();
    assert!(!c.settings.exists());
    assert!(String::from_utf8(out).unwrap().contains("SubagentStart"));
}

#[test]
fn cli_hooks_only_flag_is_accepted() {
    let t = Tmp::new();
    let claude = t.path().join("claude");
    Command::cargo_bin("bar")
        .unwrap()
        .args(["click", "install", "--hooks-only"])
        .env("HOME", t.path())
        .env("CLAUDE_CONFIG_DIR", &claude)
        .env("BAR_STATE_DIR", t.path().join("state"))
        .assert()
        .success();
    assert!(claude.join("settings.json").exists());
    assert!(!t.path().join("Applications").exists());
}

/// The tlc record rides on `PostToolUse`: one entry with no matcher, so
/// every tool (Bash included) reaches the hook, added once.
#[test]
fn hooks_only_wires_post_tool_use_for_bash_once() {
    use hop_bar::click::run_hooks_only;
    let t = Tmp::new();
    let c = cfg(&t);
    std::fs::create_dir_all(c.settings.parent().unwrap()).unwrap();
    std::fs::write(
        &c.settings,
        r#"{"hooks":{"PostToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":"other"}]}]}}"#,
    )
    .unwrap();
    let mut out = Vec::new();
    run_hooks_only(&c, false, &mut out).unwrap();
    let text = String::from_utf8(out).unwrap();
    assert!(text.contains("add PostToolUse hook"), "{text}");
    let once = std::fs::read_to_string(&c.settings).unwrap();
    let v: serde_json::Value = serde_json::from_str(&once).unwrap();
    let post = v["hooks"]["PostToolUse"].as_array().unwrap();
    assert_eq!(post.len(), 2, "{post:?}");
    assert_eq!(post[0]["hooks"][0]["command"], "other");
    assert!(post[1].get("matcher").is_none(), "{post:?}");
    assert_eq!(
        post[1]["hooks"][0]["command"],
        "'/opt/bar dir/bar' click hook"
    );
    let mut out = Vec::new();
    run_hooks_only(&c, false, &mut out).unwrap();
    assert!(String::from_utf8(out)
        .unwrap()
        .contains("hooks already wired"));
    assert_eq!(std::fs::read_to_string(&c.settings).unwrap(), once);
}
