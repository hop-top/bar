//! Two tray installs of one user (two state dirs) share one autostart:
//! the LaunchAgent, the `Run` value, the autostart entry or the user
//! unit. Each belongs to the install it names; another install neither
//! removes, stops nor (without `--force`) overwrites it. Driven by temp
//! dirs and a recording `launchctl`/`reg`/`systemctl`, so every platform
//! runs on any Unix.

// Unix fixtures (POSIX paths)
#![cfg(unix)]

mod common;
#[path = "common/render_fx.rs"]
mod render_fx;

use hop_bar::conf::Config;
use hop_bar::tray::agent_windows;
use hop_bar::tray::install::{install, uninstall, InstallCfg, InstallError, InstallOpts};
use hop_bar::tray::process::{self, Layout, Sys};
use hop_bar::tray::xdg::Dirs;
use render_fx::Tmp;
use std::path::PathBuf;

/// Records every call; keeps the `Run` value `reg add` sets, which
/// `reg query` prints and `reg delete` removes.
#[derive(Default)]
struct Fake {
    calls: Vec<Vec<String>>,
    run_value: Option<String>,
}

impl Sys for Fake {
    fn exec(&mut self, cmd: &[String]) -> Result<(), String> {
        self.calls.push(cmd.to_vec());
        match cmd.get(1).map(String::as_str) {
            Some("add") => self.run_value = Some(cmd[8].clone()),
            Some("delete") => self.run_value = None,
            Some("query") if self.run_value.is_none() => return Err("exit code: 1".into()),
            _ => {}
        }
        Ok(())
    }
    fn spawn(&mut self, cmd: &[String]) -> Result<u32, String> {
        self.calls.push(cmd.to_vec());
        Ok(4242)
    }
    fn output(&mut self, cmd: &[String]) -> Result<(bool, String), String> {
        let ok = self.exec(cmd).is_ok();
        let out = match (&self.run_value, cmd.get(1).map(String::as_str)) {
            (Some(v), Some("query")) => format!(
                "\r\nHKEY_CURRENT_USER\\Software\\Microsoft\\Windows\\CurrentVersion\\Run\r\n    \
                 bar-tray    REG_SZ    {v}\r\n\r\n"
            ),
            _ => String::new(),
        };
        Ok((ok, out))
    }
}

impl Fake {
    /// Calls that stop, unload, disable or remove an autostart.
    fn undoing(&self) -> Vec<String> {
        self.calls
            .iter()
            .map(|c| c.join(" "))
            .filter(|c| {
                ["bootout", " stop", "disable", "delete"]
                    .iter()
                    .any(|v| c.contains(v))
            })
            .collect()
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum P {
    Linux,
    Systemd,
    Mac,
    Windows,
}

const ALL: [P; 4] = [P::Linux, P::Systemd, P::Mac, P::Windows];

fn dirs(t: &Tmp) -> Dirs {
    Dirs {
        config: t.path().join("cfg"),
        data: t.path().join("data"),
        desktop: None,
    }
}

fn layout(t: &Tmp, p: P, state: &str) -> Layout {
    Layout {
        state_dir: t.path().join(state),
        agents: (p == P::Mac).then(|| t.path().join("home/Library/LaunchAgents")),
        uid: Some(501),
        xdg: matches!(p, P::Linux | P::Systemd).then(|| dirs(t)),
        system_dir: (p == P::Windows).then(|| PathBuf::from("/win/System32")),
    }
}

fn cfg(t: &Tmp, p: P, state: &str) -> InstallCfg {
    t.write("bin/bar-tray", "");
    InstallCfg {
        bar: t.path().join("bin/bar"),
        hook_bar: t.path().join("bin/bar"),
        tray: t.path().join("bin/bar-tray"),
        settings: t.path().join("claude/settings.json"),
        env: vec![("PATH".into(), Some("/usr/bin:/bin".into()))],
        layout: layout(t, p, state),
    }
}

fn opts(p: P, autostart: bool, force: bool) -> InstallOpts {
    InstallOpts {
        dry_run: false,
        autostart,
        hooks: false,
        systemd: autostart && p == P::Systemd,
        force,
    }
}

fn install_in(
    t: &Tmp,
    p: P,
    state: &str,
    o: &InstallOpts,
    sys: &mut Fake,
) -> Result<String, InstallError> {
    let mut out = Vec::new();
    install(
        &cfg(t, p, state),
        o,
        p == P::Mac,
        &Config::default(),
        sys,
        &mut out,
    )?;
    Ok(String::from_utf8(out).unwrap())
}

fn uninstall_in(t: &Tmp, p: P, state: &str, sys: &mut Fake) -> String {
    let mut out = Vec::new();
    let settings = t.path().join("claude/settings.json");
    uninstall(&layout(t, p, state), &settings, false, sys, &mut out).unwrap();
    String::from_utf8(out).unwrap()
}

/// Where the autostart lives, as messages name it.
fn what(t: &Tmp, p: P) -> PathBuf {
    match p {
        P::Linux => dirs(t).desktop_entry(),
        P::Systemd => dirs(t).unit(),
        P::Mac => t
            .path()
            .join("home/Library/LaunchAgents/top.hop.bar-tray.plist"),
        P::Windows => agent_windows::location(),
    }
}

/// The autostart's contents: the file, or the `Run` value.
fn current(t: &Tmp, p: P, sys: &Fake) -> Option<String> {
    match p {
        P::Windows => sys.run_value.clone(),
        _ => std::fs::read_to_string(what(t, p)).ok(),
    }
}

fn autostart_of(t: &Tmp, p: P, state: &str, sys: &mut Fake) -> Option<PathBuf> {
    let now = std::time::SystemTime::now();
    process::status_with(&layout(t, p, state), now, sys).autostart
}

/// The reported case: a second install without autostart, then its
/// uninstall, took the first install's autostart with it.
#[test]
fn uninstall_leaves_another_installs_autostart() {
    for p in ALL {
        let t = Tmp::new();
        let mut sys = Fake::default();
        install_in(&t, p, "state", &opts(p, true, false), &mut sys).unwrap();
        install_in(&t, p, "state2", &opts(p, false, false), &mut sys).unwrap();
        let before = current(&t, p, &sys);
        assert!(before.is_some(), "{p:?}");
        assert_eq!(autostart_of(&t, p, "state2", &mut sys), None, "{p:?}");
        sys.calls.clear();
        let out = uninstall_in(&t, p, "state2", &mut sys);
        assert_eq!(current(&t, p, &sys), before, "{p:?}: {out}");
        assert!(sys.undoing().is_empty(), "{p:?}: {:?}", sys.undoing());
        let kept = format!(
            "kept {}: belongs to the tray install in {}",
            what(&t, p).display(),
            t.path().join("state").display()
        );
        assert!(out.contains(&kept), "{p:?}: {out}");
        assert_eq!(autostart_of(&t, p, "state", &mut sys), Some(what(&t, p)));
    }
}

/// A second install with autostart would overwrite the first's: refused
/// before anything is written, unless forced; then it is the second's.
#[test]
fn install_refuses_another_installs_autostart_unless_forced() {
    for p in ALL {
        let t = Tmp::new();
        let mut sys = Fake::default();
        install_in(&t, p, "state", &opts(p, true, false), &mut sys).unwrap();
        let before = current(&t, p, &sys);
        let e = install_in(&t, p, "state2", &opts(p, true, false), &mut sys).unwrap_err();
        let InstallError::Failed(m) = e else {
            panic!("{p:?}: {e:?}")
        };
        let first = t.path().join("state").display().to_string();
        assert!(
            m.starts_with(&what(&t, p).display().to_string()),
            "{p:?}: {m}"
        );
        assert!(
            m.contains(&format!("tray install in {first}")),
            "{p:?}: {m}"
        );
        assert!(m.contains("--force"), "{p:?}: {m}");
        assert_eq!(current(&t, p, &sys), before, "{p:?}");
        assert!(!t.path().join("state2").exists(), "{p:?}: nothing written");

        install_in(&t, p, "state2", &opts(p, true, true), &mut sys).unwrap();
        let after = current(&t, p, &sys).unwrap();
        assert!(after.contains(&format!("{}/tray", t.path().join("state2").display())));
        let out = uninstall_in(&t, p, "state", &mut sys);
        assert_eq!(
            current(&t, p, &sys).as_deref(),
            Some(after.as_str()),
            "{p:?}"
        );
        assert!(
            out.contains("belongs to the tray install in"),
            "{p:?}: {out}"
        );
    }
}

/// An install whose state dir went away without an uninstall owns
/// nothing any more: the next install takes its autostart over.
#[test]
fn a_vanished_installs_autostart_is_taken_over() {
    for p in ALL {
        let t = Tmp::new();
        let mut sys = Fake::default();
        install_in(&t, p, "state", &opts(p, true, false), &mut sys).unwrap();
        std::fs::remove_dir_all(t.path().join("state")).unwrap();
        install_in(&t, p, "state2", &opts(p, true, false), &mut sys).unwrap();
        let now = current(&t, p, &sys).unwrap();
        assert!(
            now.contains(&t.path().join("state2").display().to_string()),
            "{p:?}"
        );
    }
}

/// Linux has two kinds: installing one removes this install's other
/// kind, never another install's; start and stop use only their own.
#[test]
fn the_other_kind_stays_when_another_installs() {
    let t = Tmp::new();
    let mut sys = Fake::default();
    install_in(
        &t,
        P::Linux,
        "state",
        &opts(P::Linux, true, false),
        &mut sys,
    )
    .unwrap();
    let entry = current(&t, P::Linux, &sys);
    install_in(
        &t,
        P::Systemd,
        "state2",
        &opts(P::Systemd, true, false),
        &mut sys,
    )
    .unwrap();
    assert_eq!(
        current(&t, P::Linux, &sys),
        entry,
        "the first's entry stays"
    );
    assert!(dirs(&t).unit().is_file());
    assert_eq!(
        autostart_of(&t, P::Linux, "state", &mut sys),
        Some(dirs(&t).desktop_entry())
    );
    assert_eq!(
        autostart_of(&t, P::Linux, "state2", &mut sys),
        Some(dirs(&t).unit())
    );
    sys.calls.clear();
    let l = layout(&t, P::Linux, "state");
    process::stop(&l, &mut sys, &mut Vec::new()).unwrap();
    process::start(
        &l,
        &cfg(&t, P::Linux, "state").argv(),
        &mut sys,
        &mut Vec::new(),
    )
    .unwrap();
    assert!(
        !sys.calls.iter().any(|c| c.iter().any(|a| a == "--user")),
        "not the other install's unit: {:?}",
        sys.calls
    );
}
