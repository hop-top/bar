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
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let f = s.join("agents").join(SID).join("agent-a1");
        assert_eq!(f.metadata().unwrap().permissions().mode() & 0o777, 0o600);
    }
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
    let go = |c: &str| {
        std::fs::write(conf.join("bar.conf"), c).unwrap();
        let out = Command::cargo_bin("bar")
            .unwrap()
            .arg("render")
            .envs(env.clone())
            .env("COLUMNS", "80")
            .current_dir(&cwd)
            .write_stdin(format!(r#"{{"session_id":"{SID}"}}"#))
            .output()
            .unwrap();
        assert!(out.status.success());
        plain(&String::from_utf8_lossy(&out.stdout))
    };
    assert!(go("set left \"#{agents}\"\n").contains("agents 1"));
    // the icon takes the label's place
    let out = go("set icons emoji\nset left \"#{agents}\"\n");
    assert_eq!(out.trim(), "👥 1", "{out:?}");
}

#[test]
fn session_end_leaves_other_sessions_alone() {
    let t = Tmp::new();
    let s = t.state();
    let other = "aaaa-bbbb";
    start(&s, "agent-a1", "Explore");
    run_hook(
        &s,
        &format!(
            r#"{{"session_id":"{other}","hook_event_name":"SubagentStart","agent_id":"agent-z","agent_type":"X"}}"#
        ),
    );
    run_hook(
        &s,
        &format!(r#"{{"session_id":"{SID}","hook_event_name":"SessionEnd"}}"#),
    );
    assert_eq!(go(&s, SID, &Options::default(), now()), None);
    assert_eq!(
        plain(&go(&s, other, &Options::default(), now()).unwrap()),
        "agents 1"
    );
}

#[cfg(unix)]
#[test]
fn long_ids_and_symlinked_agents_dir_are_rejected() {
    let t = Tmp::new();
    let s = t.state();
    start(&s, &"a".repeat(65), "Explore");
    assert_eq!(go(&s, SID, &Options::default(), now()), None);
    let target = t.0.join("elsewhere");
    std::fs::create_dir_all(&target).unwrap();
    std::fs::create_dir_all(s.join("agents")).unwrap();
    std::os::unix::fs::symlink(&target, s.join("agents").join(SID)).unwrap();
    start(&s, "agent-a1", "Explore");
    assert!(std::fs::read_dir(&target).unwrap().next().is_none());
    std::fs::write(target.join("agent-x"), "Explore").unwrap();
    assert_eq!(go(&s, SID, &Options::default(), now()), None);
}

#[test]
fn junk_does_not_crowd_out_live_agents() {
    let t = Tmp::new();
    let s = t.state();
    let dir = s.join("agents").join(SID);
    std::fs::create_dir_all(&dir).unwrap();
    for i in 0..100 {
        std::fs::write(dir.join(format!(".junk{i}.tmp")), "").unwrap();
    }
    start(&s, "agent-a1", "Explore");
    assert_eq!(
        plain(&go(&s, SID, &Options::default(), now()).unwrap()),
        "agents 1"
    );
}

#[test]
fn start_prunes_expired_and_orphan_tmp_files() {
    let t = Tmp::new();
    let s = t.state();
    start(&s, "agent-old", "Explore");
    let dir = s.join("agents").join(SID);
    let old = SystemTime::now() - Duration::from_secs(STALE_SECS as u64 + 60);
    let f = std::fs::File::options()
        .write(true)
        .open(dir.join("agent-old"))
        .unwrap();
    f.set_modified(old).unwrap();
    std::fs::write(dir.join(".agent-x.1.tmp"), "").unwrap();
    let f = std::fs::File::options()
        .write(true)
        .open(dir.join(".agent-x.1.tmp"))
        .unwrap();
    f.set_modified(old).unwrap();
    start(&s, "agent-new", "Explore");
    assert!(!dir.join("agent-old").exists());
    assert!(!dir.join(".agent-x.1.tmp").exists());
    assert!(dir.join("agent-new").exists());
}

const BIDI_LS: [char; 2] = ['\u{202E}', '\u{2028}'];

#[test]
fn hook_write_strips_bidi_and_line_separators() {
    let t = Tmp::new();
    let s = t.state();
    // JSON escapes, decoded by the hook
    start(&s, "agent-a1", r"evil\u202Egnp.exe\u2028x");
    let body = std::fs::read_to_string(s.join("agents").join(SID).join("agent-a1")).unwrap();
    assert!(!body.contains(BIDI_LS), "{body:?}");
    assert_eq!(body, "evilgnp.exex");
}

#[test]
fn names_read_sanitized_and_bounded() {
    let t = Tmp::new();
    let s = t.state();
    let dir = s.join("agents").join(SID);
    std::fs::create_dir_all(&dir).unwrap();
    // written by something other than the hook
    std::fs::write(dir.join("agent-a1"), "a\u{202E}b\u{2028}c").unwrap();
    for i in 0..40 {
        std::fs::write(dir.join(format!("agent-big{i}")), "x".repeat(1 << 16)).unwrap();
    }
    let mut o = Options::default();
    o.set("names", "1");
    let out = go(&s, SID, &o, now()).unwrap();
    assert!(!out.contains(BIDI_LS), "{out:?}");
    assert!(plain(&out).contains("abc"), "{out:?}");
    assert!(out.len() <= 2048, "{}", out.len());
}
