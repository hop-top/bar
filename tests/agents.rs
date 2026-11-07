mod common;

use assert_cmd::Command;
use hop_bar::click::hook::run_hook;
use hop_bar::conf::Config;
use hop_bar::options::Options;
use hop_bar::placeholders::agents::{render_with, STALE_SECS};
use hop_bar::placeholders::Ctx;
use hop_bar::render::ColdRunner;
use hop_bar::session::Session;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, SystemTime};

static N: AtomicUsize = AtomicUsize::new(0);
const SID: &str = "3f2a9c1e-0b4d-4e6f-8a7b-1c2d3e4f5a6b";

struct Tmp(PathBuf);
impl Tmp {
    fn new() -> Tmp {
        let p = std::env::temp_dir().join(format!(
            "bar-agents-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&p).unwrap();
        Tmp(p)
    }
    fn state(&self) -> PathBuf {
        self.0.join("state")
    }
}
impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn start(state: &Path, id: &str, kind: &str) {
    run_hook(
        state,
        &format!(
            r#"{{"session_id":"{SID}","hook_event_name":"SubagentStart","agent_id":"{id}","agent_type":"{kind}"}}"#
        ),
    );
}

fn stop(state: &Path, id: &str) {
    run_hook(
        state,
        &format!(
            r#"{{"session_id":"{SID}","hook_event_name":"SubagentStop","agent_id":"{id}","agent_type":"x"}}"#
        ),
    );
}

fn go(state: &Path, sid: &str, opts: &Options, now: i64) -> Option<String> {
    let cfg = Config::default();
    let s = Session {
        session_id: sid.into(),
        ..Session::default()
    };
    let runner = ColdRunner;
    let ctx = Ctx {
        session: &s,
        config: &cfg,
        now,
        runner: &runner,
        cache: None,
        columns: None,
        plugin_dirs: &[],
    };
    render_with(&ctx, opts, state)
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

fn plain(s: &str) -> String {
    let mut out = String::new();
    let mut esc = false;
    for c in s.chars() {
        match (esc, c) {
            (_, '\x1b') => esc = true,
            (true, 'm') => esc = false,
            (false, c) => out.push(c),
            _ => {}
        }
    }
    out
}

#[test]
fn hidden_without_agents() {
    let t = Tmp::new();
    assert_eq!(go(&t.state(), SID, &Options::default(), now()), None);
}

#[test]
fn counts_running_and_stop_removes() {
    let t = Tmp::new();
    let s = t.state();
    start(&s, "agent-a1", "Explore");
    start(&s, "agent-b2", "code-reviewer");
    let out = go(&s, SID, &Options::default(), now()).unwrap();
    assert_eq!(plain(&out), "agents 2");
    assert!(out.contains("38;5;250m"), "label muted: {out:?}");
    stop(&s, "agent-a1");
    assert_eq!(
        plain(&go(&s, SID, &Options::default(), now()).unwrap()),
        "agents 1"
    );
    stop(&s, "agent-b2");
    assert_eq!(go(&s, SID, &Options::default(), now()), None);
}

#[test]
fn start_is_idempotent_and_files_are_private() {
    let t = Tmp::new();
    let s = t.state();
    start(&s, "agent-a1", "Explore");
    start(&s, "agent-a1", "Explore");
    assert_eq!(
        plain(&go(&s, SID, &Options::default(), now()).unwrap()),
        "agents 1"
    );
    use std::os::unix::fs::PermissionsExt;
    let f = s.join("agents").join(SID).join("agent-a1");
    assert_eq!(f.metadata().unwrap().permissions().mode() & 0o777, 0o600);
}

#[test]
fn sessions_are_isolated_and_session_end_clears() {
    let t = Tmp::new();
    let s = t.state();
    start(&s, "agent-a1", "Explore");
    let other = "aaaa-bbbb";
    assert_eq!(go(&s, other, &Options::default(), now()), None);
    run_hook(
        &s,
        &format!(r#"{{"session_id":"{SID}","hook_event_name":"SessionEnd"}}"#),
    );
    assert_eq!(go(&s, SID, &Options::default(), now()), None);
}

#[test]
fn stale_entries_do_not_count() {
    let t = Tmp::new();
    let s = t.state();
    start(&s, "agent-old", "Explore");
    start(&s, "agent-new", "Explore");
    let f = std::fs::File::options()
        .write(true)
        .open(s.join("agents").join(SID).join("agent-old"))
        .unwrap();
    f.set_modified(SystemTime::now() - Duration::from_secs(STALE_SECS as u64 + 60))
        .unwrap();
    assert_eq!(
        plain(&go(&s, SID, &Options::default(), now()).unwrap()),
        "agents 1"
    );
}

#[test]
fn names_option_lists_types_and_max_truncates() {
    let t = Tmp::new();
    let s = t.state();
    start(&s, "agent-a1", "Explore");
    start(&s, "agent-b2", "code-reviewer-with-long-name");
    let mut o = Options::default();
    o.set("names", "1");
    let out = plain(&go(&s, SID, &o, now()).unwrap());
    assert!(out.starts_with("agents 2 "), "{out:?}");
    assert!(
        out.contains("Explore") && out.contains("code-reviewer"),
        "{out:?}"
    );
    o.set("max", "8");
    let out = plain(&go(&s, SID, &o, now()).unwrap());
    assert!(out.contains('…'), "{out:?}");
    assert!(out.chars().count() <= "agents 2 ".len() + 8, "{out:?}");
}

#[test]
fn hostile_ids_and_types_are_contained() {
    let t = Tmp::new();
    let s = t.state();
    start(&s, "../../escape", "Explore");
    start(&s, "", "Explore");
    assert_eq!(go(&s, SID, &Options::default(), now()), None);
    assert!(!t.0.join("escape").exists());
    start(&s, "agent-a1", r"bad\u001b[31m\nname");
    let mut o = Options::default();
    o.set("names", "1");
    let out = go(&s, SID, &o, now()).unwrap();
    assert!(!plain(&out).contains('\x1b') && !plain(&out).contains('\n'));
}

#[test]
fn malformed_hook_input_is_ignored() {
    let t = Tmp::new();
    let s = t.state();
    run_hook(&s, "not json");
    run_hook(
        &s,
        r#"{"session_id":"x/y","hook_event_name":"SubagentStart","agent_id":"a"}"#,
    );
    run_hook(
        &s,
        &format!(r#"{{"session_id":"{SID}","hook_event_name":"SubagentStart"}}"#),
    );
    assert_eq!(go(&s, SID, &Options::default(), now()), None);
}

#[test]
fn bar_render_wires_agents_as_core_name() {
    let t = Tmp::new();
    start(&t.state(), "agent-a1", "Explore");
    let root = t.0.clone();
    let (env, cwd) = common::isolation(&root);
    let conf = root.join("xdg/bar");
    std::fs::create_dir_all(&conf).unwrap();
    std::fs::write(conf.join("bar.conf"), "set left \"#{agents}\"\n").unwrap();
    let out = Command::cargo_bin("bar")
        .unwrap()
        .arg("render")
        .envs(env)
        .env("COLUMNS", "80")
        .current_dir(cwd)
        .write_stdin(format!(r#"{{"session_id":"{SID}"}}"#))
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(plain(&String::from_utf8_lossy(&out.stdout)).contains("agents 1"));
}
