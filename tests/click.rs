#[path = "common/fake_env.rs"]
mod fake_env;

use assert_cmd::Command;
use fake_env::{fixture, url, Fake, SID, TTY};
use hop_bar::click::{check, hook, link, run, toggle, Pane, Refusal, Term, COMMANDS};
use hop_bar::conf::Config;
use hop_bar::options::Options;
use hop_bar::placeholders::{ctx as ctxp, model, Ctx};
use hop_bar::render::ColdRunner;
use hop_bar::session::Session;
use predicates::prelude::*;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

static N: AtomicUsize = AtomicUsize::new(0);

struct Tmp(PathBuf);
impl Tmp {
    fn new() -> Tmp {
        let p = std::env::temp_dir().join(format!(
            "bar-click-{}-{}",
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

#[test]
fn toggle_flips_state_file() {
    let t = Tmp::new();
    assert_eq!(toggle(t.path(), "5h"), Ok(true));
    let f = t.path().join("pct-5h");
    assert!(f.exists());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(f.metadata().unwrap().permissions().mode() & 0o777, 0o600);
    }
    assert_eq!(toggle(t.path(), "5h"), Ok(false));
    assert!(!f.exists());
    assert_eq!(toggle(t.path(), "7d"), Ok(true));
    assert!(t.path().join("pct-7d").exists());
}

#[test]
fn toggle_rejects_unknown_key() {
    let t = Tmp::new();
    for k in ["", "x", "../pct-5h", "5H", "d", "5h/../.."] {
        assert!(toggle(t.path(), k).is_err(), "{k}");
    }
    assert_eq!(std::fs::read_dir(t.path()).unwrap().count(), 0);
}

#[test]
fn toggle_cli_uses_state_dir_and_rejects_bad_key() {
    let t = Tmp::new();
    Command::cargo_bin("bar")
        .unwrap()
        .args(["toggle", "5h"])
        .env("BAR_STATE_DIR", t.path())
        .assert()
        .success();
    assert!(t.path().join("pct-5h").exists());
    Command::cargo_bin("bar")
        .unwrap()
        .args(["toggle", "../x"])
        .env("BAR_STATE_DIR", t.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("unknown key"));
}

// ── Links ──────────────────────────────────────────────────────────────────

fn mac() -> Term {
    Term {
        macos: true,
        tty: Some(TTY.into()),
    }
}

fn opts(pairs: &[(&str, &str)]) -> Options {
    let mut o = Options::default();
    for (k, v) in pairs {
        o.set(k, v);
    }
    o
}

fn session() -> Session {
    Session {
        model: "Claude Opus".into(),
        effort: "high".into(),
        session_id: SID.into(),
        context_used: Some(38.0),
        ..Session::default()
    }
}

/// Renders model, effort and ctx; returns the three outputs.
fn render_three(state: &Path, o: &Options, term: &Term) -> Vec<String> {
    let s = session();
    let cfg = Config::default();
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
    vec![
        model::model_with(&ctx, o, state, || term.clone()).unwrap(),
        model::effort_with(&ctx, o, state, || term.clone()).unwrap(),
        ctxp::render_with(&ctx, o, state, || term.clone()).unwrap(),
    ]
}

fn installed() -> Tmp {
    let t = Tmp::new();
    std::fs::write(t.path().join("click-installed"), "cmd").unwrap();
    t
}

#[test]
fn link_needs_ttys_and_clean_sid() {
    assert_eq!(link("model", SID, &mac()), Some(url("model")));
    assert_eq!(link("compact", SID, &mac()), Some(url("compact")));
    for tty in [
        None,
        Some("pts/0"),
        Some("ttys"),
        Some("ttys4x"),
        Some("console"),
    ] {
        let term = Term {
            macos: true,
            tty: tty.map(String::from),
        };
        assert_eq!(link("model", SID, &term), None, "{tty:?}");
    }
    for sid in ["", "ABC", "abc;rm", "a/b", "a&b=c", "a b", "a\nb", "abc%2f"] {
        assert_eq!(link("model", sid, &mac()), None, "{sid:?}");
    }
    assert_eq!(link("clear", SID, &mac()), None);
}

#[test]
fn click_none_emits_no_link() {
    let t = installed();
    for out in render_three(t.path(), &opts(&[("click", "none")]), &mac()) {
        assert!(!out.contains("\x1b]8;;"), "{out:?}");
    }
    let on = render_three(t.path(), &Options::default(), &mac());
    for (out, cmd) in on.iter().zip(["model", "effort", "compact"]) {
        assert!(
            out.contains(&format!("\x1b]8;;{}\x1b\\", url(cmd))),
            "{out:?}"
        );
    }
}

#[test]
fn handler_missing_emits_no_link() {
    let t = Tmp::new();
    for out in render_three(t.path(), &Options::default(), &mac()) {
        assert!(!out.contains("\x1b]8;;"), "{out:?}");
    }
}

#[test]
fn toggle_only_marker_emits_no_command_link() {
    // A marker from a toggle-only install: usage toggles yes, commands no.
    let t = Tmp::new();
    std::fs::write(t.path().join("click-installed"), "").unwrap();
    for out in render_three(t.path(), &Options::default(), &mac()) {
        assert!(!out.contains("\x1b]8;;"), "{out:?}");
    }
    assert!(hop_bar::click::handler_installed(t.path()));
    assert!(!hop_bar::click::cmd_handler_installed(t.path()));
    std::fs::write(t.path().join("click-installed"), "cmd").unwrap();
    assert!(hop_bar::click::cmd_handler_installed(t.path()));
}

#[test]
fn non_macos_emits_no_command_link() {
    let t = installed();
    let linux = Term {
        macos: false,
        tty: Some(TTY.into()),
    };
    assert_eq!(link("model", SID, &linux), None);
    for out in render_three(t.path(), &Options::default(), &linux) {
        assert!(!out.contains("\x1b]8;;"), "{out:?}");
    }
}

#[test]
fn plugin_cannot_extend_allowlist() {
    assert_eq!(COMMANDS, ["model", "effort", "compact"]);
    // Options (config defaults or inline) cannot pick the command.
    let t = installed();
    let o = opts(&[("cmd", "clear"), ("command", "clear")]);
    let out = render_three(t.path(), &o, &mac());
    assert!(out[0].contains(&url("model")));
    assert!(!out.iter().any(|s| s.contains("clear")));
    // A link a plugin prints is still refused by the handler.
    let f = Fake::new();
    assert_eq!(run(&f, &url("clear")), Err(Refusal::NotAllowed));
    assert!(f.sent().is_empty());
}

// ── Handler refusals ───────────────────────────────────────────────────────

fn refused(f: &Fake, u: &str, want: Refusal) {
    assert_eq!(run(f, u), Err(want), "{u:?}");
    assert!(f.sent().is_empty(), "sent on refusal: {u:?}");
}

#[test]
fn malformed_url_refused() {
    let f = Fake::new();
    let tail = format!("?tty={TTY}&sid={SID}");
    for u in [
        String::new(),
        "bar://cmd/model".into(),
        format!("bar://cmd/model?tty={TTY}"),
        format!("bar://cmd/model?sid={SID}"),
        format!("BAR://cmd/model{tail}"),
        format!("bar:/cmd/model{tail}"),
        format!("http://cmd/model{tail}"),
        format!("bar://toggle/model{tail}"),
        format!("bar://cmd//model{tail}"),
        format!("bar://cmd/model/{tail}"),
        format!("bar://cmd/{tail}"),
        format!("bar://cmd/Model{tail}"),
        format!("bar://cmd/model?tty=&sid={SID}"),
        format!("bar://cmd/model?tty={TTY}&sid="),
        format!("bar://cmd/model?tty=pts/1&sid={SID}"),
        format!("bar://cmd/model{tail}#x"),
        format!(" bar://cmd/model{tail}"),
    ] {
        refused(&f, &u, Refusal::Malformed);
    }
    assert!(
        f.calls.borrow().is_empty(),
        "malformed links never reach tmux"
    );
}

#[test]
fn url_injection_refused() {
    let f = Fake::new();
    for u in [
        format!("{}&x=1", url("model")),
        format!("{}&tty=ttys005", url("model")),
        format!("{}&sid={SID}", url("model")),
        format!("bar://cmd/model?tty={TTY}&sid={SID};rm"),
        format!("bar://cmd/model;rm?tty={TTY}&sid={SID}"),
        format!("bar://cmd/model%2F..%2Fx?tty={TTY}&sid={SID}"),
        format!("bar://cmd/model?tty={TTY}%2F..&sid={SID}"),
        format!("bar://cmd/model?tty={TTY}&sid={SID}%0a"),
        format!("bar://cmd/model?tty={TTY}&sid={SID}\n"),
        format!("bar://cmd/model\n?tty={TTY}&sid={SID}"),
        format!("bar://cmd/model?tty={TTY}\nEnter&sid={SID}"),
        format!("bar://cmd/model?tty={TTY}&&sid={SID}"),
        format!("bar://cmd/model?tty={TTY}&sid={SID}=x"),
        format!("bar://cmd/model?tty%3D{TTY}&sid={SID}"),
        format!("bar://cmd/model?tty={TTY}&sid={SID}&"),
    ] {
        refused(&f, &u, Refusal::Malformed);
    }
    // Parameter order is free; each still once.
    let swapped = format!("bar://cmd/model?sid={SID}&tty={TTY}");
    assert!(run(&f, &swapped).is_ok());
}

#[test]
fn command_off_allowlist_refused() {
    let f = Fake::new();
    for cmd in [
        "clear", "exit", "logout", "login", "bashes", "modelx", "compactt",
    ] {
        refused(&f, &url(cmd), Refusal::NotAllowed);
    }
    assert!(f.calls.borrow().is_empty());
}

#[test]
fn missing_session_refused() {
    let f = Fake::new();
    std::fs::remove_file(f.session_file()).unwrap();
    refused(&f, &url("model"), Refusal::Missing);
    let other = "0000aaaa-0b4d-4e6f-8a7b-1c2d3e4f5a6b";
    let f = Fake::new();
    refused(
        &f,
        &format!("bar://cmd/model?tty={TTY}&sid={other}"),
        Refusal::Missing,
    );
}

#[test]
fn dead_pid_refused() {
    let f = Fake::new();
    f.alive.set(false);
    refused(&f, &url("model"), Refusal::Dead);
}

#[test]
fn other_tty_refused() {
    let f = Fake::new();
    *f.tty.borrow_mut() = Some("ttys009".into());
    refused(&f, &url("model"), Refusal::OtherTty);
    *f.tty.borrow_mut() = None;
    refused(&f, &url("model"), Refusal::OtherTty);
}

#[test]
fn busy_refused() {
    let f = Fake::new();
    f.set_status("busy");
    refused(&f, &url("compact"), Refusal::Busy);
}

#[test]
fn waiting_refused() {
    let f = Fake::new();
    let ev = format!(r#"{{"session_id":"{SID}","hook_event_name":"PermissionRequest"}}"#);
    hook::run_hook(&f.state(), &ev);
    refused(&f, &url("model"), Refusal::Waiting);
}

#[test]
fn draft_in_box_refused() {
    let f = Fake::new();
    f.screen("%1", &fixture("draft"));
    refused(&f, &url("model"), Refusal::NotEmpty);
}

#[test]
fn open_menu_refused() {
    let f = Fake::new();
    f.screen("%1", &fixture("menu"));
    refused(&f, &url("effort"), Refusal::NotEmpty);
}

#[test]
fn no_pane_refused() {
    let f = Fake::new();
    f.panes.borrow_mut().clear();
    refused(&f, &url("model"), Refusal::NoPane);
    let f = Fake::new();
    *f.panes.borrow_mut() = vec![("/dev/ttys001".into(), "%7".into())];
    f.screen("%7", "no status line here\n");
    refused(&f, &url("model"), Refusal::NoPane);
}

#[test]
fn own_pane_preferred_over_sid_match() {
    let f = Fake::new();
    *f.panes.borrow_mut() = vec![
        ("/dev/ttys001".into(), "%1".into()),
        (format!("/dev/{TTY}"), "%2".into()),
    ];
    f.screen("%1", &fixture("empty"));
    f.screen("%2", &fixture("empty"));
    assert_eq!(check(&f, SID, TTY), Ok(Pane("%2".into())));
}

#[test]
fn background_session_found_by_sid() {
    let f = Fake::new();
    *f.panes.borrow_mut() = vec![
        ("/dev/ttys001".into(), "%1".into()),
        ("/dev/ttys002".into(), "%3".into()),
    ];
    // %1 shows a longer id sharing the prefix: not this session.
    f.screen("%1", &fixture("empty").replace(SID, &format!("{SID}0")));
    f.screen("%3", &fixture("empty"));
    assert_eq!(check(&f, SID, TTY), Ok(Pane("%3".into())));
    assert!(run(&f, &url("model")).is_ok());
    assert!(f.sent().iter().all(|c| c[2] == "%3"));
}

#[test]
fn sid_fallback_pane_rechecked_on_second_pass() {
    let f = Fake::new();
    *f.panes.borrow_mut() = vec![("/dev/ttys002".into(), "%3".into())];
    f.screen("%3", &fixture("empty"));
    // The displayed client switches to another session during the wait.
    let other = fixture("empty").replace(SID, "0000aaaa-0b4d-4e6f-8a7b-1c2d3e4f5a6b");
    *f.on_sleep.borrow_mut() = Some(Box::new(move |f: &Fake| f.screen("%3", &other)));
    refused(&f, &url("model"), Refusal::NoPane);
}

#[test]
fn checks_twice_second_check_can_refuse() {
    let f = Fake::new();
    *f.on_sleep.borrow_mut() = Some(Box::new(|f: &Fake| f.set_status("busy")));
    refused(&f, &url("model"), Refusal::Busy);
    assert_eq!(*f.sleeps.borrow(), vec![Duration::from_secs(1)]);

    let f = Fake::new();
    *f.on_sleep.borrow_mut() = Some(Box::new(|f: &Fake| f.screen("%1", &fixture("menu"))));
    refused(&f, &url("model"), Refusal::NotEmpty);

    let f = Fake::new();
    *f.on_sleep.borrow_mut() = Some(Box::new(|f: &Fake| {
        let ev = format!(
            r#"{{"session_id":"{SID}","hook_event_name":"Notification","notification_type":"permission_prompt"}}"#
        );
        hook::run_hook(&f.state(), &ev);
    }));
    refused(&f, &url("model"), Refusal::Waiting);
}

#[test]
fn refusal_notifies_logs_never_sends() {
    let f = Fake::new();
    f.set_status("busy");
    assert_eq!(run(&f, &url("model")), Err(Refusal::Busy));
    let notes = f.notes.borrow();
    assert_eq!(notes.len(), 1);
    assert!(notes[0].0.contains("model") && notes[0].0.contains("not sent"));
    assert_eq!(notes[0].1, "session is busy");
    let logs = f.logs.borrow();
    assert_eq!(logs.len(), 1);
    assert!(
        logs[0].contains("refused model: session is busy"),
        "{logs:?}"
    );
    assert!(f.sent().is_empty());

    // A malformed link logs `?` for the command and still never sends.
    let f = Fake::new();
    let _ = run(&f, "bar://cmd/model?tty=x");
    assert_eq!(f.notes.borrow().len(), 1);
    assert!(f.logs.borrow()[0].contains("refused ?: malformed link"));
    assert!(f.calls.borrow().is_empty());
}

#[test]
fn send_is_literal_then_enter() {
    let f = Fake::new();
    assert_eq!(run(&f, &url("model")), Ok(Pane("%1".into())));
    assert_eq!(
        f.sent(),
        vec![
            vec!["send-keys", "-t", "%1", "-l", "/model"],
            vec!["send-keys", "-t", "%1", "Enter"],
        ]
    );
    assert_eq!(f.sleeps.borrow().len(), 1);
    assert!(f.notes.borrow().is_empty());
    assert!(f.logs.borrow()[0].contains("sent model to %1"));
}
