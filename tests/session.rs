mod common;

use assert_cmd::Command;
use hop_bar::session::{self, Window};

const FULL: &str = include_str!("fixtures/session-full.json");

#[test]
fn parses_full_payload() {
    let s = session::parse(FULL);
    assert_eq!(s.cwd, "/work/proj");
    assert_eq!(s.model, "Opus 4");
    assert_eq!(s.effort, "high");
    assert_eq!(s.session_id, "abc-123");
    assert_eq!(s.context_used, Some(42.5));
    assert_eq!(
        s.five_hour,
        Some(Window {
            used_percentage: 12.5,
            resets_at: 1760000000
        })
    );
    assert_eq!(
        s.seven_day,
        Some(Window {
            used_percentage: 60.0,
            resets_at: 1760500000
        })
    );
}

#[test]
fn missing_rate_limits_are_none() {
    let s = session::parse(r#"{"model":{"display_name":"X"}}"#);
    assert_eq!(s.model, "X");
    assert_eq!(s.five_hour, None);
    assert_eq!(s.seven_day, None);
    assert_eq!(s.context_used, None);
}

#[test]
fn empty_stdin_gives_default_session() {
    assert_eq!(session::parse(""), session::Session::default());
}

#[test]
fn workspace_current_dir_wins_over_cwd() {
    let s = session::parse(r#"{"cwd":"/a","workspace":{"current_dir":"/b"}}"#);
    assert_eq!(s.cwd, "/b");
    let s = session::parse(r#"{"cwd":"/a"}"#);
    assert_eq!(s.cwd, "/a");
}

#[test]
fn render_exits_zero_on_garbage_stdin() {
    for input in [&b"\xff\xfe{{{"[..], b"null", b"[1,2]", b"{\"cwd\":5}", b""] {
        let root = std::env::temp_dir().join(format!("bar-session-{}", std::process::id()));
        let (iso, cwd) = common::isolation(&root);
        Command::cargo_bin("bar")
            .unwrap()
            .envs(iso)
            .current_dir(cwd)
            .arg("render")
            .write_stdin(input)
            .assert()
            .success();
    }
}

#[test]
fn transcript_path_parses() {
    let s = session::parse(r#"{"transcript_path":"/t/x.jsonl"}"#);
    assert_eq!(s.transcript_path, "/t/x.jsonl");
    assert_eq!(session::parse("{}").transcript_path, "");
}
