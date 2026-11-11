//! `bar tray install|uninstall|start|stop|status` against temp dirs and a
//! recording `launchctl`/`kill`: nothing touches the machine.

// Unix fixtures (modes, POSIX paths); the LaunchAgent is macOS-only
#![cfg(unix)]

mod common;
#[path = "common/render_fx.rs"]
mod render_fx;

use hop_bar::conf::{parse, Config};
use hop_bar::tray::install::{install, recorded_argv, suggestions, uninstall, InstallCfg};
use hop_bar::tray::install::{InstallError, InstallOpts};
use hop_bar::tray::process::{self, Layout, Running, Sys};
use render_fx::Tmp;
use serde_json::Value;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

#[derive(Default)]
struct Fake {
    calls: Vec<Vec<String>>,
    /// Commands whose first two words match fail.
    fail: Vec<&'static str>,
}

impl Sys for Fake {
    fn exec(&mut self, cmd: &[String]) -> Result<(), String> {
        self.calls.push(cmd.to_vec());
        let verb = cmd.get(1).map_or("", String::as_str);
        if self.fail.contains(&verb) {
            return Err("exit status: 5".into());
        }
        Ok(())
    }
    fn spawn(&mut self, cmd: &[String]) -> Result<u32, String> {
        self.calls.push(cmd.to_vec());
        Ok(4242)
    }
}

impl Fake {
    fn verbs(&self) -> Vec<String> {
        self.calls
            .iter()
            .map(|c| c[..2.min(c.len())].join(" "))
            .collect()
    }
}

fn layout(t: &Tmp) -> Layout {
    Layout {
        state_dir: t.path().join("state"),
        agents: Some(t.path().join("home/Library/LaunchAgents")),
        uid: Some(501),
        xdg: None,
    }
}

fn cfg(t: &Tmp) -> InstallCfg {
    t.write("bin/bar-tray", "");
    InstallCfg {
        bar: t.path().join("bin/bar"),
        hook_bar: t.path().join("bin/bar"),
        tray: t.path().join("bin/bar-tray"),
        settings: t.path().join("claude/settings.json"),
        env: vec![
            ("PATH".into(), Some("/usr/bin:/bin".into())),
            ("XDG_STATE_HOME".into(), None),
        ],
        layout: layout(t),
    }
}

fn opts() -> InstallOpts {
    InstallOpts {
        dry_run: false,
        autostart: true,
        hooks: true,
        systemd: false,
    }
}

fn config(src: &str) -> Config {
    parse(src).unwrap()
}

fn run(t: &Tmp, o: &InstallOpts, macos: bool, sys: &mut Fake) -> Result<String, InstallError> {
    let mut out = Vec::new();
    install(&cfg(t), o, macos, &config(""), sys, &mut out)?;
    Ok(String::from_utf8(out).unwrap())
}

fn plist(t: &Tmp) -> PathBuf {
    t.path()
        .join("home/Library/LaunchAgents/top.hop.bar-tray.plist")
}

fn hooks(t: &Tmp) -> Option<Value> {
    let s = std::fs::read_to_string(t.path().join("claude/settings.json")).ok()?;
    serde_json::from_str::<Value>(&s)
        .ok()?
        .get("hooks")
        .cloned()
}

fn mode(p: &Path) -> u32 {
    std::fs::metadata(p).unwrap().permissions().mode() & 0o777
}

#[test]
fn install_records_marks_and_hooks() {
    let t = Tmp::new();
    let mut sys = Fake::default();
    let out = run(&t, &opts(), true, &mut sys).unwrap();
    let state = t.path().join("state");
    let rec: Value =
        serde_json::from_str(&std::fs::read_to_string(state.join("tray-dirs.json")).unwrap())
            .unwrap();
    assert_eq!(rec["bar"], t.path().join("bin/bar").display().to_string());
    assert_eq!(
        rec["tray"],
        t.path().join("bin/bar-tray").display().to_string()
    );
    assert_eq!(rec["state_dir"], state.display().to_string());
    assert_eq!(rec["follows_pointer"], true);
    assert_eq!(rec["env"]["PATH"], "/usr/bin:/bin");
    assert_eq!(rec["env"]["XDG_STATE_HOME"], Value::Null);
    assert_eq!(mode(&state.join("tray-dirs.json")), 0o600);
    assert_eq!(
        std::fs::read_to_string(state.join("tray-installed")).unwrap(),
        "hooks"
    );
    assert!(hooks(&t).is_some(), "hooks registered");
    assert!(out.contains("add `tray <plugin>` lines"), "{out}");
}

#[test]
fn install_writes_and_loads_the_agent() {
    let t = Tmp::new();
    let mut sys = Fake::default();
    let out = run(&t, &opts(), true, &mut sys).unwrap();
    let p = std::fs::read_to_string(plist(&t)).unwrap();
    let lock = t.path().join("state/tray.lock").display().to_string();
    for arg in [
        t.path().join("bin/bar-tray").display().to_string(),
        "--bar".into(),
        t.path().join("bin/bar").display().to_string(),
        "--lock".into(),
        lock,
    ] {
        assert!(p.contains(&format!("<string>{arg}</string>")), "{arg}\n{p}");
    }
    assert!(p.contains("<key>PATH</key>"));
    assert!(!p.contains("XDG_STATE_HOME"), "unset vars stay out");
    assert_eq!(
        sys.verbs(),
        ["/bin/launchctl bootout", "/bin/launchctl bootstrap"]
    );
    assert_eq!(sys.calls[1][2], "gui/501");
    assert!(
        out.contains(&format!("installed {}", plist(&t).display())),
        "{out}"
    );
}

#[test]
fn dry_run_changes_nothing() {
    let t = Tmp::new();
    let mut sys = Fake::default();
    let o = InstallOpts {
        dry_run: true,
        ..opts()
    };
    let out = run(&t, &o, true, &mut sys).unwrap();
    assert!(sys.calls.is_empty());
    assert!(!t.path().join("state").exists());
    assert!(!plist(&t).exists());
    assert!(!t.path().join("claude/settings.json").exists());
    assert!(out.contains("tray-dirs.json: {"), "{out}");
    assert!(out.contains("<key>RunAtLoad</key>"), "{out}");
    assert!(out.contains("/bin/launchctl bootstrap gui/501 "), "{out}");
    assert!(out.contains("adds hooks") || out.contains("hook"), "{out}");
}

#[test]
fn no_autostart_and_no_hooks_skip_their_steps() {
    let t = Tmp::new();
    let mut sys = Fake::default();
    let o = InstallOpts {
        autostart: false,
        hooks: false,
        ..opts()
    };
    let out = run(&t, &o, true, &mut sys).unwrap();
    assert!(sys.calls.is_empty());
    assert!(!plist(&t).exists());
    assert_eq!(hooks(&t), None);
    let state = t.path().join("state");
    assert_eq!(
        std::fs::read_to_string(state.join("tray-installed")).unwrap(),
        ""
    );
    assert!(state.join("tray-dirs.json").is_file());
    assert!(out.contains("bar tray start"), "{out}");
}

#[test]
fn autostart_needs_macos_for_now() {
    let t = Tmp::new();
    let mut sys = Fake::default();
    assert_eq!(
        run(&t, &opts(), false, &mut sys).unwrap_err(),
        InstallError::Unsupported
    );
    assert!(!t.path().join("state").exists(), "refused before writing");
    let o = InstallOpts {
        autostart: false,
        ..opts()
    };
    run(&t, &o, false, &mut sys).unwrap();
    assert!(sys.calls.is_empty());
}

#[test]
fn a_missing_tray_fails_install_and_warns_in_dry_run() {
    let t = Tmp::new();
    let mut c = cfg(&t);
    c.tray = t.path().join("bin/nope");
    let mut sys = Fake::default();
    let mut out = Vec::new();
    let e = install(&c, &opts(), true, &config(""), &mut sys, &mut out).unwrap_err();
    assert!(
        matches!(e, InstallError::Failed(ref m) if m.contains("no bar-tray")),
        "{e:?}"
    );
    assert!(!t.path().join("state").exists());
    let o = InstallOpts {
        dry_run: true,
        ..opts()
    };
    install(&c, &o, true, &config(""), &mut sys, &mut out).unwrap();
    assert!(String::from_utf8(out)
        .unwrap()
        .contains("warning: no bar-tray"));
}

#[test]
fn a_failed_bootstrap_is_an_error() {
    let t = Tmp::new();
    let mut sys = Fake {
        fail: vec!["bootstrap", "bootout"],
        ..Fake::default()
    };
    let e = run(&t, &opts(), true, &mut sys).unwrap_err();
    assert!(
        matches!(e, InstallError::Failed(ref m) if m.starts_with("launchctl")),
        "{e:?}"
    );
}

#[test]
fn suggestions_mirror_action_lines_not_yet_granted() {
    let c = config(
        "action cpa cliproxyapi.use\naction cpa2 cliproxyapi.use\naction r routellm.toggle\n\
         action x other.go\ntray routellm\ntray other.stop\n",
    );
    assert_eq!(suggestions(&c), ["tray cliproxyapi.use", "tray other.go"]);
    let t = Tmp::new();
    let mut out = Vec::new();
    install(&cfg(&t), &opts(), true, &c, &mut Fake::default(), &mut out).unwrap();
    let out = String::from_utf8(out).unwrap();
    assert!(
        out.contains("  tray cliproxyapi.use\n  tray other.go\n"),
        "{out}"
    );
    // printed only: the config is not written
    assert!(!t.path().join("cfg").exists());
}

#[test]
fn recorded_argv_is_what_install_wrote() {
    let t = Tmp::new();
    run(&t, &opts(), true, &mut Fake::default()).unwrap();
    assert_eq!(
        recorded_argv(&t.path().join("state")).unwrap(),
        cfg(&t).argv()
    );
    assert_eq!(recorded_argv(&t.path().join("none")), None);
}

/// Holds the tray lock as a running tray would.
fn hold(t: &Tmp) -> std::fs::File {
    bar_tray::lock::acquire(&t.path().join("state/tray.lock"))
        .unwrap()
        .unwrap()
}

#[test]
fn running_follows_the_lock_not_the_file() {
    let t = Tmp::new();
    let lock = t.path().join("state/tray.lock");
    assert_eq!(process::running(&lock), Running::No);
    t.write("state/tray.lock", "77\n");
    assert_eq!(process::running(&lock), Running::No, "stale file");
    let held = hold(&t);
    assert_eq!(
        process::running(&lock),
        Running::Yes(Some(std::process::id()))
    );
    drop(held);
    assert_eq!(process::running(&lock), Running::No);
}

#[test]
fn start_spawns_or_loads_unless_running() {
    let t = Tmp::new();
    let l = layout(&t);
    let argv = cfg(&t).argv();
    let mut sys = Fake::default();
    let mut out = Vec::new();
    process::start(&l, &argv, &mut sys, &mut out).unwrap();
    assert_eq!(sys.calls, std::slice::from_ref(&argv), "no agent: spawned");
    t.write("home/Library/LaunchAgents/top.hop.bar-tray.plist", "x");
    let mut sys = Fake::default();
    process::start(&l, &argv, &mut sys, &mut out).unwrap();
    assert_eq!(sys.verbs(), ["/bin/launchctl bootstrap"]);
    let mut sys = Fake {
        fail: vec!["bootstrap"],
        ..Fake::default()
    };
    process::start(&l, &argv, &mut sys, &mut out).unwrap();
    assert_eq!(
        sys.verbs(),
        ["/bin/launchctl bootstrap", "/bin/launchctl kickstart"],
        "loaded but quit: kickstarted"
    );
    let _held = hold(&t);
    let mut sys = Fake::default();
    process::start(&l, &argv, &mut sys, &mut out).unwrap();
    assert!(sys.calls.is_empty());
    assert!(String::from_utf8(out)
        .unwrap()
        .contains("already running (pid"));
}

#[test]
fn stop_unloads_the_agent_and_signals_a_holder() {
    let t = Tmp::new();
    let l = layout(&t);
    let mut out = Vec::new();
    let mut sys = Fake::default();
    assert!(!process::stop(&l, &mut sys, &mut out).unwrap());
    assert!(sys.calls.is_empty());
    assert!(String::from_utf8(out.clone())
        .unwrap()
        .contains("not running"));
    let _held = hold(&t);
    let mut sys = Fake::default();
    assert!(process::stop(&l, &mut sys, &mut out).unwrap());
    assert_eq!(
        sys.calls,
        [vec![
            "/bin/kill".to_string(),
            "-TERM".into(),
            std::process::id().to_string()
        ]]
    );
    t.write("home/Library/LaunchAgents/top.hop.bar-tray.plist", "x");
    let mut sys = Fake::default();
    process::stop(&l, &mut sys, &mut out).unwrap();
    assert_eq!(sys.verbs()[0], "/bin/launchctl bootout");
}

#[test]
fn status_reports_install_autostart_lock_and_menu_age() {
    let t = Tmp::new();
    let l = layout(&t);
    let now = std::time::SystemTime::now();
    let st = process::status(&l, now);
    assert!(!st.installed && st.autostart.is_none() && st.snapshot_age.is_none());
    assert_eq!(st.running, Running::No);
    run(&t, &opts(), true, &mut Fake::default()).unwrap();
    t.write("state/tray-menu.json", "{}");
    let _held = hold(&t);
    let v = process::status(&l, now + std::time::Duration::from_secs(30)).to_json();
    assert_eq!(v["installed"], true);
    assert_eq!(v["autostart"], plist(&t).display().to_string());
    assert_eq!(v["running"], true);
    assert_eq!(v["pid"], std::process::id());
    assert!(v["snapshot_age"].as_u64().unwrap() >= 29, "{v}");
    // a snapshot spent by a run (mtime zero) has no age
    std::fs::File::options()
        .write(true)
        .open(t.path().join("state/tray-menu.json"))
        .unwrap()
        .set_modified(std::time::SystemTime::UNIX_EPOCH)
        .unwrap();
    assert_eq!(process::status(&l, now).snapshot_age, None);
    let lines = process::status(&l, now).lines().join("\n");
    assert!(lines.contains("running    yes (pid"), "{lines}");
}

fn do_uninstall(t: &Tmp, keep: bool) -> (Fake, String) {
    let mut sys = Fake::default();
    let mut out = Vec::new();
    uninstall(
        &layout(t),
        &t.path().join("claude/settings.json"),
        keep,
        &mut sys,
        &mut out,
    )
    .unwrap();
    (sys, String::from_utf8(out).unwrap())
}

#[test]
fn uninstall_removes_everything_install_wrote() {
    let t = Tmp::new();
    run(&t, &opts(), true, &mut Fake::default()).unwrap();
    t.write("state/tray-menu.json", "{}");
    let (sys, out) = do_uninstall(&t, false);
    assert_eq!(sys.verbs(), ["/bin/launchctl bootout"]);
    assert!(!plist(&t).exists());
    for f in ["tray-menu.json", "tray-dirs.json", "tray-installed"] {
        assert!(!t.path().join("state").join(f).exists(), "{f}");
    }
    assert_eq!(hooks(&t), None, "{out}");
    assert!(out.contains("removed hooks from"), "{out}");
}

#[test]
fn uninstall_keeps_hooks_for_click_or_when_asked() {
    let t = Tmp::new();
    run(&t, &opts(), true, &mut Fake::default()).unwrap();
    t.write("state/click-installed", "cmd");
    let (_, out) = do_uninstall(&t, false);
    assert!(hooks(&t).is_some());
    assert!(out.contains("bar click install uses them"), "{out}");
    assert!(t.path().join("state/click-installed").is_file());

    let t = Tmp::new();
    run(&t, &opts(), true, &mut Fake::default()).unwrap();
    let (_, out) = do_uninstall(&t, true);
    assert!(hooks(&t).is_some());
    assert!(out.contains("--keep-hooks"), "{out}");
}

#[test]
fn hooks_install_did_not_add_are_left_on_uninstall() {
    let t = Tmp::new();
    let c = cfg(&t);
    hop_bar::click::register_hooks(
        &c.settings,
        &c.hook_bar,
        &c.layout.state_dir,
        false,
        &mut Vec::new(),
    )
    .unwrap();
    run(&t, &opts(), true, &mut Fake::default()).unwrap();
    assert_eq!(
        std::fs::read_to_string(t.path().join("state/tray-installed")).unwrap(),
        ""
    );
    do_uninstall(&t, false);
    assert!(hooks(&t).is_some());
}

#[test]
fn a_reinstall_keeps_ownership_of_the_hooks() {
    let t = Tmp::new();
    run(&t, &opts(), true, &mut Fake::default()).unwrap();
    run(&t, &opts(), true, &mut Fake::default()).unwrap();
    do_uninstall(&t, false);
    assert_eq!(hooks(&t), None);
}
