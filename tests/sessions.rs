// Unix fixtures (shell scripts, modes, POSIX paths); Windows coverage: tests/windows.rs
#![cfg(unix)]

mod common;

use assert_cmd::Command;
use hop_bar::conf::Config;
use hop_bar::options::Options;
use hop_bar::placeholders::sessions::render_with;
use hop_bar::placeholders::Ctx;
use hop_bar::render::ColdRunner;
use hop_bar::session::Session;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

static N: AtomicUsize = AtomicUsize::new(0);

struct Tmp(PathBuf);
impl Tmp {
    fn new() -> Tmp {
        let p = std::env::temp_dir().join(format!(
            "bar-sessions-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(p.join("claude/sessions")).unwrap();
        std::fs::create_dir_all(p.join("state/sessions")).unwrap();
        Tmp(p)
    }
    fn claude(&self) -> PathBuf {
        self.0.join("claude")
    }
    fn state(&self) -> PathBuf {
        self.0.join("state")
    }
    fn session(&self, file: &str, pid: &str, sid: &str, status: &str) {
        let body = format!(r#"{{"pid":{pid},"sessionId":"{sid}","status":"{status}","x":1}}"#);
        std::fs::write(self.claude().join("sessions").join(file), body).unwrap();
    }
    fn mark(&self, sid: &str, what: &str) {
        std::fs::write(self.state().join("sessions").join(sid), what).unwrap();
    }
}
impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn live() -> String {
    std::process::id().to_string()
}

fn dead() -> String {
    let mut c = std::process::Command::new("true").spawn().unwrap();
    let pid = c.id();
    c.wait().unwrap();
    pid.to_string()
}

fn go(t: &Tmp) -> Option<String> {
    let cfg = Config::default();
    let s = Session::default();
    let runner = ColdRunner;
    let ctx = Ctx {
        session: &s,
        config: &cfg,
        now: 0,
        runner: &runner,
        cache: None,
        columns: None,
        plugin_dirs: &[],
    };
    render_with(&ctx, &Options::default(), &t.state(), &t.claude())
}

#[test]
fn counts_order_and_colours() {
    let t = Tmp::new();
    t.session("1.json", &live(), "a", "busy");
    t.session("2.json", &live(), "b", "busy");
    t.session("3.json", &live(), "c", "idle");
    t.session("4.json", &live(), "d", "idle");
    t.session("5.json", &live(), "e", "idle");
    t.mark("c", "waiting");
    t.mark("d", "done");
    t.mark("e", "done");
    let out = go(&t).unwrap();
    assert_eq!(
        out,
        "\x1b[38;5;179m>2\x1b[39m \x1b[1m\x1b[38;5;203m?1\x1b[39m\x1b[22m \x1b[38;5;114mv2\x1b[39m"
    );
}

#[test]
fn waiting_wins_over_busy_and_busy_over_done() {
    let t = Tmp::new();
    t.session("1.json", &live(), "a", "busy");
    t.session("2.json", &live(), "b", "busy");
    t.mark("a", "waiting");
    t.mark("b", "done");
    let out = go(&t).unwrap();
    assert!(
        out.contains("?1") && out.contains(">1") && !out.contains('v'),
        "{out:?}"
    );
}

#[test]
fn dead_and_malformed_pids_skipped() {
    let t = Tmp::new();
    t.session("1.json", &dead(), "a", "busy");
    t.session("2.json", "\"abc\"", "b", "busy");
    t.session("3.json", "null", "c", "busy");
    std::fs::write(t.claude().join("sessions/4.json"), "not json").unwrap();
    assert_eq!(go(&t), None);
    t.session("5.json", &live(), "e", "busy");
    assert!(go(&t).unwrap().contains(">1"));
}

#[test]
fn invalid_session_id_ignores_marker() {
    let t = Tmp::new();
    t.session("1.json", &live(), "../x", "idle");
    t.mark("x", "waiting");
    assert_eq!(go(&t), None);
}

#[test]
fn all_zero_or_no_dir_omitted() {
    let t = Tmp::new();
    t.session("1.json", &live(), "a", "idle");
    assert_eq!(go(&t), None);
    std::fs::remove_dir_all(t.claude().join("sessions")).unwrap();
    assert_eq!(go(&t), None);
}

#[test]
fn colours_follow_config() {
    let t = Tmp::new();
    t.session("1.json", &live(), "a", "busy");
    let mut cfg = Config::default();
    cfg.colors.warn = 99;
    let s = Session::default();
    let runner = ColdRunner;
    let ctx = Ctx {
        session: &s,
        config: &cfg,
        now: 0,
        runner: &runner,
        cache: None,
        columns: None,
        plugin_dirs: &[],
    };
    let out = render_with(&ctx, &Options::default(), &t.state(), &t.claude()).unwrap();
    assert!(out.contains("38;5;99m"), "{out:?}");
}

#[test]
fn bar_render_wires_sessions_as_core_name() {
    let t = Tmp::new();
    t.session("1.json", &live(), "a", "busy");
    let root = t.0.clone();
    let (env, cwd) = common::isolation(&root);
    let conf = root.join("xdg/bar");
    std::fs::create_dir_all(&conf).unwrap();
    std::fs::write(conf.join("bar.conf"), "set left \"#{sessions} #{spend}\"\n").unwrap();
    let out = Command::cargo_bin("bar")
        .unwrap()
        .arg("render")
        .envs(env)
        .env("COLUMNS", "80")
        .current_dir(cwd)
        .write_stdin("{}")
        .output()
        .unwrap();
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains(">1"), "{text:?}");
}
