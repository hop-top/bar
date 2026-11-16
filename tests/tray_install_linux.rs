//! Linux `bar tray install|uninstall|start|stop|status` against temp XDG
//! dirs and a recording `systemctl`/`kill`/`bar-tray --backend`: the
//! files are platform-independent, so this runs on every Unix.

// Unix fixtures (modes, POSIX paths)
#![cfg(unix)]

mod common;
#[path = "common/render_fx.rs"]
mod render_fx;

use common::tray_hold::Tray;
use hop_bar::conf::Config;
use hop_bar::tray::install::{install, uninstall, InstallCfg, InstallError, InstallOpts};
use hop_bar::tray::process::{self, Layout, Running, Sys};
use hop_bar::tray::xdg::{self, Dirs};
use render_fx::Tmp;
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

#[derive(Default)]
struct Fake {
    calls: Vec<Vec<String>>,
    /// `bar-tray --backend` fails: a build without `linux-sni`.
    no_backend: bool,
    /// `systemctl --user <verb>` fails for these verbs.
    fail: Vec<&'static str>,
    /// The running tray `kill` signals.
    tray: Option<Sender<()>>,
}

impl Sys for Fake {
    fn exec(&mut self, cmd: &[String]) -> Result<(), String> {
        self.calls.push(cmd.to_vec());
        if self.no_backend && cmd.get(1).is_some_and(|a| a == "--backend") {
            return Err("exit status: 1".into());
        }
        if cmd.get(1).is_some_and(|a| a == "--user")
            && self.fail.contains(&cmd.get(2).map_or("", String::as_str))
        {
            return Err("exit status: 1".into());
        }
        if cmd[0] == "/bin/kill" {
            if let Some(t) = &self.tray {
                let _ = t.send(());
            }
        }
        Ok(())
    }
    fn spawn(&mut self, cmd: &[String]) -> Result<u32, String> {
        self.calls.push(cmd.to_vec());
        Ok(4242)
    }
}

impl Fake {
    /// Each call by program file name and its first arguments:
    /// `systemctl --user enable`, `bar-tray --backend`, `bar-tray --bar`.
    fn verbs(&self) -> Vec<String> {
        self.calls
            .iter()
            .map(|c| {
                let prog = PathBuf::from(&c[0]);
                let name = prog.file_name().unwrap().to_string_lossy().to_string();
                let n = if c.get(1).is_some_and(|a| a == "--user") {
                    3
                } else {
                    2
                };
                std::iter::once(name)
                    .chain(c[1..n.min(c.len())].iter().cloned())
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .collect()
    }
}

fn dirs(t: &Tmp, desktop: Option<&str>) -> Dirs {
    Dirs {
        config: t.path().join("cfg"),
        data: t.path().join("data"),
        desktop: desktop.map(String::from),
    }
}

fn layout(t: &Tmp) -> Layout {
    Layout {
        state_dir: t.path().join("state"),
        agents: None,
        uid: Some(1000),
        xdg: Some(dirs(t, None)),
        system_dir: None,
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
        hooks: false,
        systemd: false,
    }
}

fn run_with(c: &InstallCfg, o: &InstallOpts, sys: &mut Fake) -> Result<String, InstallError> {
    let mut out = Vec::new();
    install(c, o, false, &Config::default(), sys, &mut out)?;
    Ok(String::from_utf8(out).unwrap())
}

fn run(t: &Tmp, o: &InstallOpts, sys: &mut Fake) -> Result<String, InstallError> {
    run_with(&cfg(t), o, sys)
}

fn read(p: PathBuf) -> String {
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

fn entry(t: &Tmp) -> PathBuf {
    t.path().join("cfg/autostart/bar-tray.desktop")
}

fn unit(t: &Tmp) -> PathBuf {
    t.path().join("cfg/systemd/user/bar-tray.service")
}

fn icon(t: &Tmp) -> PathBuf {
    t.path()
        .join("data/icons/hicolor/scalable/apps/bar-tray-symbolic.svg")
}

#[test]
fn install_writes_entry_and_icon_and_starts_the_tray() {
    let t = Tmp::new();
    let mut sys = Fake::default();
    let out = run(&t, &opts(), &mut sys).unwrap();
    let e = read(entry(&t));
    let tray = t.path().join("bin/bar-tray").display().to_string();
    let lock = t.path().join("state/tray.lock").display().to_string();
    assert!(e.contains(&format!("\nTryExec={tray}\n")), "{e}");
    assert!(
        e.contains(&format!(
            "\nExec=/usr/bin/env PATH=/usr/bin:/bin {tray} --bar {} --lock {lock}\n",
            t.path().join("bin/bar").display()
        )),
        "{e}"
    );
    assert!(!e.contains("XDG_STATE_HOME"), "unset vars stay out");
    assert_eq!(read(icon(&t)), xdg::ICON_SVG);
    assert!(!unit(&t).exists());
    assert_eq!(sys.verbs(), ["bar-tray --backend", "bar-tray --bar"]);
    assert_eq!(
        sys.calls[1],
        cfg(&t).argv(),
        "started now with the argv written"
    );
    assert!(
        out.contains(&format!("installed {}", entry(&t).display())),
        "{out}"
    );
    assert!(out.contains("started pid 4242"), "{out}");
    assert!(!out.contains("bar tray start"), "{out}");
    assert!(!out.contains("AppIndicator"), "no GNOME hint off GNOME");
}

#[test]
fn systemd_writes_enables_and_starts_the_unit() {
    let t = Tmp::new();
    let mut sys = Fake::default();
    let o = InstallOpts {
        systemd: true,
        ..opts()
    };
    let out = run(&t, &o, &mut sys).unwrap();
    let u = read(unit(&t));
    assert!(u.contains("\nWantedBy=graphical-session.target\n"), "{u}");
    assert!(u.contains("\nRestart=on-failure\n"), "{u}");
    assert!(u.contains("\nEnvironment=PATH=/usr/bin:/bin\n"), "{u}");
    assert!(
        u.contains(&format!(
            "\nExecStart={} --bar ",
            t.path().join("bin/bar-tray").display()
        )),
        "{u}"
    );
    assert!(!entry(&t).exists(), "one autostart at a time");
    assert!(icon(&t).is_file());
    assert_eq!(
        sys.verbs(),
        [
            "bar-tray --backend",
            "systemctl --user daemon-reload",
            "systemctl --user enable",
            "systemctl --user stop",
            "systemctl --user start",
        ]
    );
    assert_eq!(sys.calls[2][3], "bar-tray.service");
    assert!(out.contains("started bar-tray.service"), "{out}");
}

#[test]
fn switching_autostart_kind_removes_the_other() {
    let t = Tmp::new();
    let sd = InstallOpts {
        systemd: true,
        ..opts()
    };
    run(&t, &sd, &mut Fake::default()).unwrap();
    let mut sys = Fake::default();
    let out = run(&t, &opts(), &mut sys).unwrap();
    assert!(!unit(&t).exists() && entry(&t).is_file());
    assert_eq!(
        &sys.verbs()[1..4],
        [
            "systemctl --user disable",
            "systemctl --user daemon-reload",
            "bar-tray --bar"
        ]
    );
    assert!(out.contains(&format!("removed {}", unit(&t).display())));
    run(&t, &sd, &mut Fake::default()).unwrap();
    assert!(unit(&t).is_file() && !entry(&t).exists());
}

#[test]
fn a_tray_without_the_sni_backend_is_refused_before_writing() {
    let t = Tmp::new();
    let mut sys = Fake {
        no_backend: true,
        ..Fake::default()
    };
    let e = run(&t, &opts(), &mut sys).unwrap_err();
    assert!(
        matches!(e, InstallError::Failed(ref m) if m.contains("linux-sni") && m.contains("--no-autostart")),
        "{e:?}"
    );
    assert!(!t.path().join("state").exists() && !t.path().join("cfg").exists());
    assert!(!t.path().join("data").exists());
    // without autostart it installs, says why the tray will not show
    let o = InstallOpts {
        autostart: false,
        ..opts()
    };
    let out = run(&t, &o, &mut sys).unwrap();
    assert!(
        out.contains("warning: ") && out.contains("bar waybar"),
        "{out}"
    );
    assert!(icon(&t).is_file() && !entry(&t).exists());
    assert!(out.contains("bar tray start"), "{out}");
}

#[test]
fn dry_run_shows_files_and_commands_and_writes_nothing() {
    let t = Tmp::new();
    let mut sys = Fake::default();
    let o = InstallOpts {
        dry_run: true,
        systemd: true,
        ..opts()
    };
    let out = run(&t, &o, &mut sys).unwrap();
    assert_eq!(
        sys.verbs(),
        ["bar-tray --backend"],
        "only the read-only probe"
    );
    for p in ["state", "cfg", "data"] {
        assert!(!t.path().join(p).exists(), "{p}");
    }
    assert!(
        out.contains(&format!("write {}:\n[Unit]", unit(&t).display())),
        "{out}"
    );
    assert!(
        out.contains(&format!("write {}\n", icon(&t).display())),
        "{out}"
    );
    assert!(out.contains("--user enable bar-tray.service"), "{out}");
    assert!(out.contains("--user start bar-tray.service"), "{out}");
    let o = InstallOpts {
        dry_run: true,
        ..opts()
    };
    let out = run(&t, &o, &mut Fake::default()).unwrap();
    assert!(out.contains("[Desktop Entry]"), "{out}");
}

#[test]
fn systemd_needs_linux_and_autostart() {
    let t = Tmp::new();
    let mut c = cfg(&t);
    c.layout.xdg = None;
    let o = InstallOpts {
        systemd: true,
        ..opts()
    };
    let e = run_with(&c, &o, &mut Fake::default()).unwrap_err();
    assert!(
        matches!(e, InstallError::Failed(ref m) if m.contains("--systemd")),
        "{e:?}"
    );
    let o = InstallOpts {
        systemd: true,
        autostart: false,
        ..opts()
    };
    assert!(run(&t, &o, &mut Fake::default()).is_err());
    assert!(!t.path().join("state").exists());
}

#[test]
fn gnome_gets_the_appindicator_hint() {
    let t = Tmp::new();
    let mut c = cfg(&t);
    c.layout.xdg = Some(dirs(&t, Some("ubuntu:GNOME")));
    let out = run_with(&c, &opts(), &mut Fake::default()).unwrap();
    assert!(out.contains("AppIndicator extension"), "{out}");
    assert!(
        out.contains("appindicatorsupport@rgcjonas.gmail.com"),
        "{out}"
    );
}

#[test]
fn start_and_stop_go_through_the_unit_when_installed() {
    let t = Tmp::new();
    let l = layout(&t);
    let argv = cfg(&t).argv();
    let mut out = Vec::new();
    let mut sys = Fake::default();
    process::start(&l, &argv, &mut sys, &mut out).unwrap();
    assert_eq!(
        sys.calls,
        std::slice::from_ref(&argv),
        "entry only: spawned"
    );
    t.write("cfg/systemd/user/bar-tray.service", "x");
    let mut sys = Fake::default();
    process::start(&l, &argv, &mut sys, &mut out).unwrap();
    assert_eq!(sys.verbs(), ["systemctl --user start"]);
    let mut sys = Fake {
        fail: vec!["start"],
        ..Fake::default()
    };
    assert!(process::start(&l, &argv, &mut sys, &mut out)
        .unwrap_err()
        .starts_with("systemctl"));

    // not running: systemctl stop still runs, nothing reported stopped
    let mut sys = Fake::default();
    let mut out = Vec::new();
    assert!(!process::stop(&l, &mut sys, &mut out).unwrap());
    assert_eq!(sys.verbs(), ["systemctl --user stop"]);
    assert!(String::from_utf8(out).unwrap().contains("not running"));

    let tray = Tray::hold(&l.lock(), Duration::ZERO);
    assert_eq!(
        process::running(&l.lock()),
        Running::Yes(Some(std::process::id()))
    );
    let mut sys = Fake {
        tray: Some(tray.signaller()),
        ..Fake::default()
    };
    let mut out = Vec::new();
    assert!(process::stop(&l, &mut sys, &mut out).unwrap());
    let out = String::from_utf8(out).unwrap();
    assert!(out.contains("stopped bar-tray.service"), "{out}");
    // the fake `systemctl stop` leaves the lock held, so the holder is
    // signalled too
    assert_eq!(sys.verbs(), ["systemctl --user stop", "kill -TERM"]);
}

/// A tray that exits a while after SIGTERM: a reinstall, from either
/// autostart kind, starts the new tray once the old one let go.
#[test]
fn reinstall_waits_for_the_running_tray_to_exit() {
    for systemd in [false, true] {
        let t = Tmp::new();
        let l = layout(&t);
        let o = InstallOpts { systemd, ..opts() };
        run(&t, &o, &mut Fake::default()).unwrap();
        let tray = Tray::hold(&l.lock(), Duration::from_millis(300));
        let mut sys = Fake {
            tray: Some(tray.signaller()),
            ..Fake::default()
        };
        let out = run(&t, &o, &mut sys).unwrap();
        assert!(!out.contains("already running"), "{out}");
        assert_eq!(process::running(&l.lock()), Running::No, "old tray gone");
        let started = if systemd {
            "started bar-tray.service"
        } else {
            "started pid 4242"
        };
        assert!(out.contains(started), "{out}");
        assert_eq!(sys.verbs().iter().filter(|v| *v == "kill -TERM").count(), 1);
    }
}

/// A tray that never lets go: stop gives up after the wait, with an
/// error naming the pid, and starts nothing.
#[test]
fn stop_gives_up_on_a_tray_that_does_not_exit() {
    let t = Tmp::new();
    let l = layout(&t);
    let _tray = Tray::hold(&l.lock(), Duration::ZERO);
    let mut sys = Fake::default();
    let begun = Instant::now();
    let e = process::stop_waiting(&l, Duration::from_millis(200), &mut sys, &mut Vec::new())
        .unwrap_err();
    assert!(begun.elapsed() >= Duration::from_millis(200));
    assert!(
        e.contains(&format!("pid {} still holds", std::process::id())),
        "{e}"
    );
    assert_eq!(sys.verbs(), ["kill -TERM"]);
}

#[test]
fn status_reports_the_installed_autostart() {
    let t = Tmp::new();
    let l = layout(&t);
    let now = std::time::SystemTime::now();
    assert_eq!(process::status(&l, now).autostart, None);
    run(&t, &opts(), &mut Fake::default()).unwrap();
    assert_eq!(process::status(&l, now).autostart, Some(entry(&t)));
    let sd = InstallOpts {
        systemd: true,
        ..opts()
    };
    run(&t, &sd, &mut Fake::default()).unwrap();
    let st = process::status(&l, now);
    assert_eq!(st.autostart, Some(unit(&t)));
    assert!(st.installed);
}

#[test]
fn uninstall_removes_what_install_wrote() {
    for systemd in [false, true] {
        let t = Tmp::new();
        let o = InstallOpts { systemd, ..opts() };
        run(&t, &o, &mut Fake::default()).unwrap();
        t.write("state/tray-menu.json", "{}");
        let mut sys = Fake::default();
        let mut out = Vec::new();
        uninstall(
            &layout(&t),
            &t.path().join("claude/settings.json"),
            false,
            &mut sys,
            &mut out,
        )
        .unwrap();
        for p in [entry(&t), unit(&t), icon(&t)] {
            assert!(!p.exists(), "{}", p.display());
        }
        for f in ["tray-menu.json", "tray-dirs.json", "tray-installed"] {
            assert!(!t.path().join("state").join(f).exists(), "{f}");
        }
        let verbs = sys.verbs();
        if systemd {
            assert_eq!(
                verbs,
                [
                    "systemctl --user stop",
                    "systemctl --user disable",
                    "systemctl --user daemon-reload"
                ]
            );
        } else {
            assert!(verbs.is_empty(), "{verbs:?}");
        }
    }
}

/// A second install: its own state dir, the same XDG dirs.
fn second(t: &Tmp) -> InstallCfg {
    let mut c = cfg(t);
    c.layout.state_dir = t.path().join("state2");
    c
}

fn uninstall_of(t: &Tmp, l: &Layout) -> String {
    let mut out = Vec::new();
    uninstall(
        l,
        &t.path().join("claude/settings.json"),
        false,
        &mut Fake::default(),
        &mut out,
    )
    .unwrap();
    String::from_utf8(out).unwrap()
}

/// Every install of the user shares the one icon: uninstalling one
/// keeps it while another install remains, the last one removes it.
#[test]
fn the_icon_stays_while_another_install_uses_it() {
    let t = Tmp::new();
    let o = InstallOpts {
        autostart: false,
        ..opts()
    };
    run(&t, &o, &mut Fake::default()).unwrap();
    let b = second(&t);
    run_with(&b, &o, &mut Fake::default()).unwrap();
    let out = uninstall_of(&t, &b.layout);
    assert!(icon(&t).is_file(), "the first install still uses it");
    assert!(
        out.contains(&format!(
            "kept {}: used by the tray install in {}",
            icon(&t).display(),
            t.path().join("state").display()
        )),
        "{out}"
    );
    let out = uninstall_of(&t, &layout(&t));
    assert!(!icon(&t).exists(), "the last install removes it");
    assert!(
        out.contains(&format!("removed {}", icon(&t).display())),
        "{out}"
    );
    assert!(!t.path().join("data/bar").read_dir().unwrap().any(|_| true));
}

/// An install whose state dir went away without an uninstall does not
/// keep the icon forever.
#[test]
fn a_vanished_install_does_not_keep_the_icon() {
    let t = Tmp::new();
    run(&t, &opts(), &mut Fake::default()).unwrap();
    let b = second(&t);
    run_with(&b, &opts(), &mut Fake::default()).unwrap();
    std::fs::remove_dir_all(t.path().join("state")).unwrap();
    uninstall_of(&t, &b.layout);
    assert!(!icon(&t).exists());
}

#[test]
fn the_icon_is_the_one_the_tray_names() {
    let sni = include_str!("../crates/bar-tray/src/backend/sni_map.rs");
    assert!(
        sni.contains(&format!("ICON_NAME: &str = \"{}\";", xdg::ICON_NAME)),
        "bar-tray names another icon"
    );
}
