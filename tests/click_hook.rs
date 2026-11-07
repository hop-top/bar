mod common;

use assert_cmd::Command;
use hop_bar::click::hook::{is_waiting, run_hook};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

static N: AtomicUsize = AtomicUsize::new(0);
const SID: &str = "3f2a9c1e-0b4d-4e6f-8a7b-1c2d3e4f5a6b";

struct Tmp(PathBuf);
impl Tmp {
    fn new() -> Tmp {
        let p = std::env::temp_dir().join(format!(
            "bar-click-hook-{}-{}",
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

fn ev(event: &str, extra: &str) -> String {
    format!(r#"{{"session_id":"{SID}","hook_event_name":"{event}"{extra}}}"#)
}

#[test]
fn hook_sets_and_clears_waiting_marker() {
    let t = Tmp::new();
    let s = t.path();
    run_hook(s, &ev("PermissionRequest", ""));
    assert!(is_waiting(s, SID));
    let marker = s.join("sessions").join(SID);
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            marker.metadata().unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    // A subagent finishing a tool says nothing about the main prompt.
    run_hook(s, &ev("PostToolUse", r#","agent_id":"a1""#));
    assert!(is_waiting(s, SID));
    run_hook(s, &ev("PostToolUse", ""));
    assert!(!is_waiting(s, SID));

    let cases = [
        (
            ev(
                "Notification",
                r#","notification_type":"permission_prompt""#,
            ),
            true,
        ),
        (ev("UserPromptSubmit", ""), false),
        (
            ev(
                "Notification",
                r#","notification_type":"elicitation_dialog""#,
            ),
            true,
        ),
        (ev("Stop", ""), false),
        (ev("PreToolUse", r#","tool_name":"AskUserQuestion""#), true),
        (ev("SessionEnd", ""), false),
        (ev("StopFailure", ""), true),
        (ev("PermissionDenied", ""), false),
    ];
    for (input, want) in cases {
        run_hook(s, &input);
        assert_eq!(is_waiting(s, SID), want, "{input}");
    }
    // Events that do not change the state leave it alone.
    run_hook(s, &ev("PreToolUse", r#","tool_name":"Bash""#));
    run_hook(
        s,
        &ev("Notification", r#","notification_type":"idle_prompt""#),
    );
    run_hook(s, &ev("Stop", r#","agent_id":"a1""#));
    assert!(!is_waiting(s, SID));
    run_hook(s, &ev("PermissionRequest", ""));
    run_hook(s, &ev("Stop", r#","agent_id":"a1""#));
    assert!(is_waiting(s, SID));
}

#[test]
fn hook_ignores_bad_session_id_and_garbage() {
    let t = Tmp::new();
    for input in [
        r#"{"session_id":"../../x","hook_event_name":"PermissionRequest"}"#,
        r#"{"session_id":"","hook_event_name":"PermissionRequest"}"#,
        r#"{"session_id":"ABC","hook_event_name":"PermissionRequest"}"#,
        r#"{"hook_event_name":"PermissionRequest"}"#,
        "not json",
        "",
    ] {
        run_hook(t.path(), input);
    }
    assert_eq!(std::fs::read_dir(t.path()).unwrap().count(), 0);
    assert!(!is_waiting(t.path(), "../../x"));
}

#[test]
fn hook_cli_reads_stdin_and_exits_ok() {
    let t = Tmp::new();
    let state = t.path().join("state");
    Command::cargo_bin("bar")
        .unwrap()
        .args(["click", "hook"])
        .env("BAR_STATE_DIR", &state)
        .write_stdin(ev("PermissionRequest", ""))
        .assert()
        .success()
        .stdout("");
    assert!(is_waiting(&state, SID));
    Command::cargo_bin("bar")
        .unwrap()
        .args(["click", "hook"])
        .env("BAR_STATE_DIR", &state)
        .write_stdin("garbage")
        .assert()
        .success();
}

/// `bar click run` end to end with fakes first on PATH: the refusal goes to
/// the fake notifier and the log, and tmux is never called.
#[test]
fn click_run_cli_refuses_through_fakes() {
    let t = Tmp::new();
    let (env, cwd) = common::isolation(t.path());
    let bin = t.path().join("fake-bin");
    let calls = t.path().join("calls.log");
    for name in ["osascript", "tmux", "ps"] {
        let p = bin.join(name);
        std::fs::write(
            &p,
            format!(
                "#!/bin/sh\necho \"{name} $*\" >> '{}'\nexit 1\n",
                calls.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&p, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    }
    let url = format!("bar://cmd/clear?tty=ttys004&sid={SID}");
    Command::cargo_bin("bar")
        .unwrap()
        .args(["click", "run", &url])
        .envs(env)
        .current_dir(cwd)
        .assert()
        .failure()
        .stderr(predicates::str::contains("command is not allowed"));
    let log = std::fs::read_to_string(&calls).unwrap();
    assert!(log.starts_with("osascript "), "{log}");
    assert!(!log.contains("tmux"), "{log}");
    let clog = std::fs::read_to_string(t.path().join("state/click.log")).unwrap();
    assert!(
        clog.contains("refused clear: command is not allowed"),
        "{clog}"
    );
}

#[test]
fn hook_marks_done_on_main_stop_and_clears_on_prompt() {
    use hop_bar::click::hook::mark;
    let t = Tmp::new();
    let s = t.path();
    run_hook(s, &ev("Stop", r#","agent_id":"a1""#));
    assert_eq!(mark(s, SID), None);
    run_hook(s, &ev("Stop", ""));
    assert_eq!(mark(s, SID).as_deref(), Some("done"));
    assert!(!is_waiting(s, SID));
    run_hook(s, &ev("PermissionRequest", ""));
    assert_eq!(mark(s, SID).as_deref(), Some("waiting"));
    run_hook(s, &ev("Stop", ""));
    assert_eq!(mark(s, SID).as_deref(), Some("done"));
    run_hook(s, &ev("UserPromptSubmit", ""));
    assert_eq!(mark(s, SID), None);
    run_hook(s, &ev("Stop", ""));
    run_hook(s, &ev("SessionEnd", ""));
    assert_eq!(mark(s, SID), None);
}
