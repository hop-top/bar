use assert_cmd::Command;
use hop_bar::click::{
    applescript, info_plist_commands, install_plan, run_install, toggle, InstallCfg,
};
use predicates::prelude::*;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

static N: AtomicUsize = AtomicUsize::new(0);

struct Tmp(PathBuf);
impl Tmp {
    fn new() -> Tmp {
        let p = std::env::temp_dir().join(format!(
            "bar-click-{}-{}",
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
    }
}

#[test]
fn toggle_flips_state_file() {
    let t = Tmp::new();
    assert_eq!(toggle(t.path(), "5h"), Ok(true));
    let f = t.path().join("pct-5h");
    assert!(f.exists());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(f.metadata().unwrap().permissions().mode() & 0o777, 0o600);
    }
    assert_eq!(toggle(t.path(), "5h"), Ok(false));
    assert!(!f.exists());
    assert_eq!(toggle(t.path(), "7d"), Ok(true));
    assert!(t.path().join("pct-7d").exists());
}

#[test]
fn toggle_rejects_unknown_key() {
    let t = Tmp::new();
    for k in ["", "x", "../pct-5h", "5H", "d", "5h/../.."] {
        assert!(toggle(t.path(), k).is_err(), "{k}");
    }
    assert_eq!(std::fs::read_dir(t.path()).unwrap().count(), 0);
}

#[test]
fn toggle_cli_uses_state_dir_and_rejects_bad_key() {
    let t = Tmp::new();
    Command::cargo_bin("bar")
        .unwrap()
        .args(["toggle", "5h"])
        .env("BAR_STATE_DIR", t.path())
        .assert()
        .success();
    assert!(t.path().join("pct-5h").exists());
    Command::cargo_bin("bar")
        .unwrap()
        .args(["toggle", "../x"])
        .env("BAR_STATE_DIR", t.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("unknown key"));
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
        .assert()
        .success();
    let _ = o;
    assert!(!t.path().join("state").exists());
    assert!(!t.path().join("apps").exists());
}
