//! The Windows side of `bar tray install|uninstall|start|stop|status`:
//! the `Run` value, the tray started from the recorded dirs, `taskkill`.
//! Driven by a layout with a system dir and a recording `reg`/`taskkill`,
//! so it runs (and nothing touches a registry) on any machine.

// Unix fixtures (POSIX paths stand in for Windows ones)
#![cfg(unix)]

mod common;
#[path = "common/render_fx.rs"]
mod render_fx;

use hop_bar::conf::parse;
use hop_bar::tray::agent_windows::{KEY, OVERFLOW_HINT};
use hop_bar::tray::install::{install, uninstall, InstallCfg, InstallOpts};
use hop_bar::tray::process::{self, Layout, Running, Sys};
use render_fx::Tmp;
use std::path::PathBuf;

/// Records every call; `reg query` succeeds while `run_value` is set,
/// which `reg add` / `reg delete` change, and `fail` names verbs that fail.
#[derive(Default)]
struct Fake {
    calls: Vec<Vec<String>>,
    run_value: Option<String>,
    fail: Vec<&'static str>,
}

impl Sys for Fake {
    fn exec(&mut self, cmd: &[String]) -> Result<(), String> {
        self.calls.push(cmd.to_vec());
        let verb = cmd.get(1).map_or("", String::as_str);
        if self.fail.iter().any(|f| cmd.iter().any(|a| a == f)) {
            return Err("exit code: 1".into());
        }
        match verb {
            "add" => self.run_value = Some(cmd[8].clone()),
            "query" if self.run_value.is_none() => return Err("exit code: 1".into()),
            "delete" => self.run_value = None,
            _ => {}
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
        state_dir: t.path().join("local/bar"),
        agents: None,
        uid: None,
        xdg: None,
        system_dir: Some(PathBuf::from("/win/System32")),
    }
}

fn cfg(t: &Tmp) -> InstallCfg {
    t.write("bin/bar-tray.exe", "");
    InstallCfg {
        bar: t.path().join("bin/bar.exe"),
        hook_bar: t.path().join("bin/bar.exe"),
        tray: t.path().join("bin/bar-tray.exe"),
        settings: t.path().join("claude/settings.json"),
        env: vec![
            ("PATH".into(), Some("/win/System32".into())),
            ("BAR_STATE_DIR".into(), None),
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

fn run(c: &InstallCfg, o: &InstallOpts, sys: &mut Fake) -> String {
    let mut out = Vec::new();
    install(c, o, false, &parse("").unwrap(), sys, &mut out).unwrap();
    String::from_utf8(out).unwrap()
}

fn reg() -> String {
    "/win/System32/reg.exe".into()
}

fn record(t: &Tmp) -> PathBuf {
    t.path().join("local/bar/tray-dirs.json")
}

fn tray(t: &Tmp) -> String {
    t.path().join("bin/bar-tray.exe").display().to_string()
}

#[test]
fn install_sets_the_run_value_then_starts_the_tray() {
    let t = Tmp::new();
    let mut sys = Fake::default();
    let out = run(&cfg(&t), &opts(), &mut sys);
    let command = format!("\"{}\" --dirs \"{}\"", tray(&t), record(&t).display());
    assert_eq!(
        sys.calls,
        [
            vec![
                reg(),
                "add".into(),
                KEY.into(),
                "/v".into(),
                "bar-tray".into(),
                "/t".into(),
                "REG_SZ".into(),
                "/d".into(),
                command.clone(),
                "/f".into(),
            ],
            vec![tray(&t), "--dirs".into(), record(&t).display().to_string()],
        ]
    );
    assert_eq!(sys.run_value.as_deref(), Some(command.as_str()));
    assert!(record(&t).is_file(), "record written before the value");
    assert!(out.contains(&format!("installed {KEY}\\bar-tray")), "{out}");
    assert!(out.contains("started pid 4242"), "{out}");
    assert!(out.contains(OVERFLOW_HINT), "first install: {out}");
    assert!(!out.contains("start it with"), "{out}");
    // nothing but the Run value is ever written
    assert!(!sys
        .calls
        .iter()
        .flatten()
        .any(|a| a.contains("IsPromoted") || a.contains("NotifyIconSettings")));
}

#[test]
fn the_overflow_hint_is_for_the_first_install_only() {
    let t = Tmp::new();
    run(&cfg(&t), &opts(), &mut Fake::default());
    let out = run(&cfg(&t), &opts(), &mut Fake::default());
    assert!(!out.contains(OVERFLOW_HINT), "{out}");
}

#[test]
fn a_running_tray_is_left_running_on_reinstall() {
    let t = Tmp::new();
    let _held = bar_tray::lock::acquire(&record(&t).with_file_name("tray.lock"))
        .unwrap()
        .unwrap();
    let mut sys = Fake::default();
    let out = run(&cfg(&t), &opts(), &mut sys);
    assert_eq!(sys.verbs(), [format!("{} add", reg())]);
    assert!(out.contains("already running"), "{out}");
}

#[test]
fn dry_run_prints_the_commands_and_runs_nothing() {
    let t = Tmp::new();
    let mut sys = Fake::default();
    let o = InstallOpts {
        dry_run: true,
        ..opts()
    };
    let out = run(&cfg(&t), &o, &mut sys);
    assert!(sys.calls.is_empty());
    assert!(!t.path().join("local").exists());
    assert!(
        out.contains(&format!("{} add {KEY} /v bar-tray /t REG_SZ /d ", reg())),
        "{out}"
    );
    assert!(
        out.contains(&format!(
            "start {} --dirs {}",
            tray(&t),
            record(&t).display()
        )),
        "{out}"
    );
}

#[test]
fn a_bad_command_line_is_refused_before_writing() {
    let t = Tmp::new();
    let mut c = cfg(&t);
    let long = t.path().join("d".repeat(250));
    t.write(&format!("{}/bar-tray.exe", "d".repeat(250)), "");
    c.tray = long.join("bar-tray.exe");
    let mut sys = Fake::default();
    let e = install(
        &c,
        &opts(),
        false,
        &parse("").unwrap(),
        &mut sys,
        &mut Vec::new(),
    )
    .unwrap_err();
    assert!(e.to_string().contains("260"), "{e}");
    assert!(sys.calls.is_empty());
    assert!(!t.path().join("local").exists());
}

#[test]
fn a_failed_reg_add_fails_install() {
    let t = Tmp::new();
    let mut sys = Fake {
        fail: vec!["add"],
        ..Fake::default()
    };
    let mut out = Vec::new();
    let e = install(
        &cfg(&t),
        &opts(),
        false,
        &parse("").unwrap(),
        &mut sys,
        &mut out,
    )
    .unwrap_err();
    assert!(e.to_string().starts_with("reg: "), "{e}");
    assert_eq!(sys.calls.len(), 1, "not started");
}

#[test]
fn no_autostart_writes_no_value() {
    let t = Tmp::new();
    let mut sys = Fake::default();
    let o = InstallOpts {
        autostart: false,
        ..opts()
    };
    let out = run(&cfg(&t), &o, &mut sys);
    assert!(sys.calls.is_empty());
    assert!(out.contains("start it with: bar tray start"), "{out}");
}

#[test]
fn status_reports_the_run_value() {
    let t = Tmp::new();
    let l = layout(&t);
    let now = std::time::SystemTime::now();
    let mut sys = Fake::default();
    assert_eq!(process::status_with(&l, now, &mut sys).autostart, None);
    assert_eq!(sys.verbs(), [format!("{} query", reg())]);
    run(&cfg(&t), &opts(), &mut sys);
    let v = process::status_with(&l, now, &mut sys).to_json();
    assert_eq!(v["autostart"], format!("{KEY}\\bar-tray"));
    assert_eq!(v["installed"], true);
}

#[test]
fn the_pid_comes_from_tray_pid_when_the_lock_file_says_nothing() {
    let t = Tmp::new();
    let lock = record(&t).with_file_name("tray.lock");
    let held = bar_tray::lock::acquire(&lock).unwrap().unwrap();
    // as on Windows, where the holder's lock keeps the file unreadable
    held.set_len(0).unwrap();
    assert_eq!(
        process::running(&lock),
        Running::Yes(Some(std::process::id()))
    );
    std::fs::remove_file(lock.with_file_name("tray.pid")).unwrap();
    assert_eq!(process::running(&lock), Running::Yes(None));
}

#[test]
fn stop_asks_with_taskkill_then_forces() {
    let t = Tmp::new();
    let l = layout(&t);
    let _held = bar_tray::lock::acquire(&l.lock()).unwrap().unwrap();
    let pid = std::process::id().to_string();
    let kill = "/win/System32/taskkill.exe".to_string();
    let mut sys = Fake::default();
    assert!(process::stop(&l, &mut sys, &mut Vec::new()).unwrap());
    assert_eq!(sys.calls, [vec![kill.clone(), "/PID".into(), pid.clone()]]);
    let mut sys = Fake {
        fail: vec!["/PID"],
        ..Fake::default()
    };
    // both fail here (the fake fails on "/PID"): the error is the forced one
    let e = process::stop(&l, &mut sys, &mut Vec::new()).unwrap_err();
    assert!(e.contains(&format!("stop pid {pid}")), "{e}");
    assert_eq!(
        sys.calls,
        [
            vec![kill.clone(), "/PID".into(), pid.clone()],
            vec![kill, "/PID".into(), pid, "/F".into()],
        ]
    );
}

#[test]
fn uninstall_removes_the_run_value_and_the_files() {
    let t = Tmp::new();
    let mut sys = Fake::default();
    run(&cfg(&t), &opts(), &mut sys);
    t.write("local/bar/tray.pid", "4242\n");
    sys.calls.clear();
    let mut out = Vec::new();
    uninstall(
        &layout(&t),
        &t.path().join("claude/settings.json"),
        false,
        &mut sys,
        &mut out,
    )
    .unwrap();
    assert_eq!(
        sys.verbs(),
        [format!("{} query", reg()), format!("{} delete", reg())]
    );
    assert_eq!(sys.run_value, None);
    for f in ["tray-dirs.json", "tray-installed", "tray.pid"] {
        assert!(!t.path().join("local/bar").join(f).exists(), "{f}");
    }
    // nothing to remove: no delete
    sys.calls.clear();
    uninstall(
        &layout(&t),
        &t.path().join("claude/settings.json"),
        false,
        &mut sys,
        &mut Vec::new(),
    )
    .unwrap();
    assert_eq!(sys.verbs(), [format!("{} query", reg())]);
}
