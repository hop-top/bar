use assert_cmd::Command;
use hop_bar::click::{applescript, info_plist_commands, install_plan, run_install, InstallCfg};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

static N: AtomicUsize = AtomicUsize::new(0);

struct Tmp(PathBuf);
impl Tmp {
    fn new() -> Tmp {
        let p = std::env::temp_dir().join(format!(
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
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn cfg(t: &Tmp) -> InstallCfg {
    InstallCfg {
        bar: PathBuf::from("/opt/bar dir/bar"),
        state_dir: t.path().join("state"),
        app: t.path().join("apps/bar.app"),
        settings: t.path().join("claude/settings.json"),
    }
}

#[test]
fn applescript_dispatches_toggle_urls_only() {
    let s = applescript(Path::new("/opt/bar dir/bar\"x"));
    assert!(s.contains("on open location"));
    assert!(s.contains("\"bar://toggle/\""));
    assert!(s.contains("quoted form of theKey"));
    assert!(s.contains("/opt/bar dir/bar\\\"x"));
    assert!(s.contains(" toggle "));
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
    assert!(marker.exists());
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
fn non_macos_prints_message_and_exits_ok() {
    let t = Tmp::new();
    let c = cfg(&t);
    let mut out = Vec::new();
    run_install(&c, false, false, &mut |_| panic!("ran"), &mut out).unwrap();
    assert!(String::from_utf8(out).unwrap().contains("macOS"));
    assert!(!c.state_dir.exists());
}

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
    let s = applescript(Path::new("/opt/bar"));
    assert!(s.contains("\"bar://cmd/\""));
    assert!(s.contains("\" click run \" & quoted form of theURL"));
    assert!(s.contains("try"), "a refusal must not raise a dialog");
}

fn backups(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| {
            p.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("settings.json.bak-")
        })
        .collect();
    v.sort();
    v
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
    (r, String::from_utf8(out).unwrap(), ran)
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

    let b = backups(c.settings.parent().unwrap());
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
    assert_eq!(backups(c.settings.parent().unwrap()).len(), 1);
}

#[test]
fn install_creates_missing_settings_without_backup() {
    let t = Tmp::new();
    let c = cfg(&t);
    install(&c, false).0.unwrap();
    let v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&c.settings).unwrap()).unwrap();
    assert!(v["hooks"]["PermissionRequest"].is_array());
    assert!(backups(c.settings.parent().unwrap()).is_empty());
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
    assert!(backups(c.settings.parent().unwrap()).is_empty());
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
    assert!(backups(c.settings.parent().unwrap()).is_empty());
}
