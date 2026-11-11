// Unix fixtures (shell scripts, modes, POSIX paths); Windows coverage: tests/windows.rs
#![cfg(unix)]

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
        let p = crate::common::tmp::root().join(format!(
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
        crate::common::tmp::remove(&self.0);
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
        common::exe::write(
            &p,
            format!(
                "#!/bin/sh\necho \"{name} $*\" >> '{}'\nexit 1\n",
                calls.display()
            ),
        );
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

// ── tlc records ────────────────────────────────────────────────────────────

use hop_bar::click::hook::{read_record, record_path};
use hop_bar::click::tlc::{last_touch, Touch};

fn bash(cmd: &str, cwd: &str, stdout: &str, extra: &str) -> String {
    let v = serde_json::json!({
        "session_id": SID,
        "hook_event_name": "PostToolUse",
        "cwd": cwd,
        "tool_name": "Bash",
        "tool_input": {"command": cmd},
        "tool_response": {"stdout": stdout, "stderr": "", "interrupted": false, "isImage": false},
    });
    let mut s = v.to_string();
    s.pop();
    format!("{s}{extra}}}")
}

fn touch(cmd: &str, out: &str) -> Option<(String, String)> {
    last_touch(cmd, Path::new("/w/proj"), out)
        .map(|Touch { task, cwd }| (task, cwd.display().to_string()))
}

fn at(task: &str, cwd: &str) -> Option<(String, String)> {
    Some((task.into(), cwd.into()))
}

#[test]
fn tlc_task_actions_are_found_in_each_form() {
    assert_eq!(touch("tlc task claim T-0042", ""), at("T-0042", "/w/proj"));
    assert_eq!(
        touch("tlc task complete T-0042 --note abc1234", ""),
        at("T-0042", "/w/proj")
    );
    assert_eq!(
        touch("tlc task update T-0042 --status IN_PROGRESS", ""),
        at("T-0042", "/w/proj")
    );
    assert_eq!(
        touch("tlc task update --status=in_progress T-0042", ""),
        at("T-0042", "/w/proj")
    );
    assert_eq!(
        touch("tlc task claim --no-prompt task_01abc", ""),
        at("task_01abc", "/w/proj")
    );
    assert_eq!(
        touch(
            "tlc task create 'Fix it' --track alpha",
            "Created task T-0048: Fix it\n"
        ),
        at("T-0048", "/w/proj")
    );
    assert_eq!(
        touch("/usr/local/bin/tlc task claim T-1", ""),
        at("T-1", "/w/proj")
    );
    assert_eq!(touch("FOO=1 tlc task claim T-1", ""), at("T-1", "/w/proj"));
}

#[test]
fn tlc_non_actions_are_ignored() {
    for cmd in [
        "tlc task list --mine",
        "tlc task show T-0042",
        "tlc task update T-0042 --status DONE",
        "tlc task update T-0042 --title 'IN_PROGRESS'",
        "tlc task unclaim T-0042",
        "tlc task claim 'T-00*'",
        "tlc task claim",
        "echo tlc task claim T-0042",
        "git commit -m 'tlc task claim T-0042'",
        "# tlc task claim T-0042",
        "tlcx task claim T-0042",
        "tlc task create 'no output'",
    ] {
        assert_eq!(touch(cmd, ""), None, "{cmd}");
    }
}

#[test]
fn tlc_chained_commands_last_action_wins() {
    let cmd = "cargo test && git commit -m 'fix: x' && tlc task complete T-0001 --note \"$(git rev-parse --short HEAD)\"; tlc task claim T-0002 | cat";
    assert_eq!(touch(cmd, ""), at("T-0002", "/w/proj"));
    let cmd = "tlc task claim T-0002 && tlc task create a && tlc task create b || true";
    let out = "Claimed T-0002\nCreated task T-0010: a\nCreated task T-0011: b\n";
    assert_eq!(touch(cmd, out), at("T-0011", "/w/proj"));
    let cmd = "tlc task create a\ntlc task claim T-0003";
    assert_eq!(
        touch(cmd, "Created task T-0010: a\n"),
        at("T-0003", "/w/proj")
    );
}

#[test]
fn tlc_directory_follows_cd_and_chdir() {
    assert_eq!(
        touch("cd /w/other && tlc task claim T-1", ""),
        at("T-1", "/w/other")
    );
    assert_eq!(
        touch("cd sub && tlc task claim T-1", ""),
        at("T-1", "/w/proj/sub")
    );
    assert_eq!(
        touch("tlc -C ../b task claim T-1", ""),
        at("T-1", "/w/proj/../b")
    );
    assert_eq!(
        touch("tlc --chdir=/w/c task claim T-1", ""),
        at("T-1", "/w/c")
    );
    // a directory bar cannot know records nothing
    assert_eq!(touch("cd ~/x && tlc task claim T-1", ""), None);
    assert_eq!(touch("cd \"$D\" && tlc task claim T-1", ""), None);
}

#[test]
fn hook_records_tlc_task_per_session() {
    let t = Tmp::new();
    let s = t.path();
    run_hook(s, &bash("tlc task claim T-0042", "/w/proj", "", ""));
    let r = read_record(s, "tlc", SID).unwrap();
    assert_eq!(r["task"], "T-0042");
    assert_eq!(r["cwd"], "/w/proj");
    assert!(r["at"].as_i64().unwrap() > 0);
    let path = record_path(s, "tlc", SID).unwrap();
    assert_eq!(path, s.join("records/tlc").join(SID));
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(path.metadata().unwrap().permissions().mode() & 0o777, 0o600);
    }
    // later commands without a tlc action keep the record
    run_hook(s, &bash("ls", "/w/proj", "", ""));
    run_hook(s, &bash("tlc task list", "/w/proj", "", ""));
    assert_eq!(read_record(s, "tlc", SID).unwrap()["task"], "T-0042");
    // the same task again leaves the record as it was
    std::fs::write(&path, r#"{"task":"T-0042","cwd":"/w/proj","at":7}"#).unwrap();
    run_hook(s, &bash("tlc task complete T-0042", "/w/proj", "", ""));
    assert_eq!(read_record(s, "tlc", SID).unwrap()["at"], 7);
    run_hook(
        s,
        &bash("cd sub && tlc task complete T-0042", "/w/proj", "", ""),
    );
    assert_eq!(read_record(s, "tlc", SID).unwrap()["cwd"], "/w/proj/sub");
    // the latest touched task wins
    run_hook(
        s,
        &bash(
            "tlc task create x",
            "/w/proj",
            "Created task T-0048: x\n",
            "",
        ),
    );
    assert_eq!(read_record(s, "tlc", SID).unwrap()["task"], "T-0048");
    // other tools and events never record
    let other = bash("tlc task claim T-9", "/w/proj", "", "").replace("\"Bash\"", "\"Read\"");
    run_hook(s, &other);
    let pre = bash("tlc task claim T-9", "/w/proj", "", "").replace("PostToolUse", "PreToolUse");
    run_hook(s, &pre);
    assert_eq!(read_record(s, "tlc", SID).unwrap()["task"], "T-0048");
    // a plain-string tool_response works too
    let plain = serde_json::json!({
        "session_id": SID, "hook_event_name": "PostToolUse", "cwd": "/w/proj",
        "tool_name": "Bash", "tool_input": {"command": "tlc task create y"},
        "tool_response": "Created task T-0050: y",
    });
    run_hook(s, &plain.to_string());
    assert_eq!(read_record(s, "tlc", SID).unwrap()["task"], "T-0050");
    // no temp files left behind
    let names: Vec<String> = std::fs::read_dir(s.join("records/tlc"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, [SID]);
}

#[test]
fn hook_subagent_tool_call_records_for_main_session() {
    let t = Tmp::new();
    let s = t.path();
    run_hook(s, &ev("PermissionRequest", ""));
    let sub = bash(
        "tlc task claim T-0007",
        "/w/proj",
        "",
        r#","agent_id":"a1b2","agent_type":"general-purpose""#,
    );
    run_hook(s, &sub);
    assert_eq!(read_record(s, "tlc", SID).unwrap()["task"], "T-0007");
    // and leaves the main session's waiting marker alone
    assert!(is_waiting(s, SID));
    run_hook(s, &bash("tlc task claim T-0008", "/w/proj", "", ""));
    assert!(!is_waiting(s, SID));
}

#[test]
fn hook_session_end_drops_records_of_that_session_only() {
    let t = Tmp::new();
    let s = t.path();
    const OTHER: &str = "0a1b2c3d-0000-4000-8000-0000000000ff";
    run_hook(s, &bash("tlc task claim T-1", "/w/proj", "", ""));
    let other = bash("tlc task claim T-2", "/w/proj", "", "").replace(SID, OTHER);
    run_hook(s, &other);
    run_hook(s, &ev("SessionEnd", ""));
    assert!(read_record(s, "tlc", SID).is_none());
    assert_eq!(read_record(s, "tlc", OTHER).unwrap()["task"], "T-2");
}

#[test]
fn hook_tlc_record_refuses_bad_input() {
    let t = Tmp::new();
    let s = t.path();
    for (cmd, cwd) in [
        ("tlc task claim T-1", "relative/dir"),
        ("tlc task claim T-1", ""),
    ] {
        run_hook(s, &bash(cmd, cwd, "", ""));
    }
    let bad_sid = bash("tlc task claim T-1", "/w/proj", "", "").replace(SID, "../../x");
    run_hook(s, &bad_sid);
    assert!(!s.join("records").exists());
    // a planted symlink is never written through
    let target = s.join("elsewhere");
    std::fs::create_dir_all(&target).unwrap();
    std::fs::create_dir_all(s.join("records")).unwrap();
    std::os::unix::fs::symlink(&target, s.join("records/tlc")).unwrap();
    run_hook(s, &bash("tlc task claim T-1", "/w/proj", "", ""));
    assert_eq!(std::fs::read_dir(&target).unwrap().count(), 0);
}
