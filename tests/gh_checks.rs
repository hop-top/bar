//! CI check state of `prs`: rollup mapping, marks, option and old caches.

// Unix fixtures (shell scripts, modes, POSIX paths); Windows coverage: tests/windows.rs
#![cfg(unix)]

mod common;
#[path = "common/gh_fx.rs"]
mod gh_fx;

use gh_fx::{strip, Fx};
use hop_bar::conf::{Config, Icons};
use hop_bar::placeholders::gh::{checks, Checks, Kind};
use serde_json::{json, Value};

fn rollup(items: Value) -> Value {
    json!({"number": 1, "statusCheckRollup": items})
}

fn run(status: &str, conclusion: &str) -> Value {
    json!({"__typename": "CheckRun", "status": status, "conclusion": conclusion})
}

fn ctx_state(state: &str) -> Value {
    json!({"__typename": "StatusContext", "state": state})
}

#[test]
fn failed_conclusions_and_states_fail() {
    for c in [
        "FAILURE",
        "TIMED_OUT",
        "CANCELLED",
        "ACTION_REQUIRED",
        "STARTUP_FAILURE",
    ] {
        let pr = rollup(json!([run("COMPLETED", "SUCCESS"), run("COMPLETED", c)]));
        assert_eq!(checks(&pr), Checks::Fail, "{c}");
    }
    for s in ["FAILURE", "ERROR"] {
        assert_eq!(checks(&rollup(json!([ctx_state(s)]))), Checks::Fail, "{s}");
    }
}

#[test]
fn failure_beats_pending() {
    let pr = rollup(json!([run("IN_PROGRESS", ""), run("COMPLETED", "FAILURE")]));
    assert_eq!(checks(&pr), Checks::Fail);
}

#[test]
fn unfinished_runs_and_contexts_pend() {
    for s in ["IN_PROGRESS", "QUEUED", "WAITING", "PENDING"] {
        let pr = rollup(json!([run("COMPLETED", "SUCCESS"), run(s, "")]));
        assert_eq!(checks(&pr), Checks::Pending, "{s}");
    }
    for s in ["PENDING", "EXPECTED"] {
        assert_eq!(
            checks(&rollup(json!([ctx_state(s)]))),
            Checks::Pending,
            "{s}"
        );
    }
}

#[test]
fn finished_without_failure_passes() {
    let pr = rollup(json!([
        run("COMPLETED", "SUCCESS"),
        run("COMPLETED", "SKIPPED"),
        run("COMPLETED", "NEUTRAL"),
        ctx_state("SUCCESS"),
    ]));
    assert_eq!(checks(&pr), Checks::Pass);
}

#[test]
fn no_checks_or_no_field_is_none() {
    assert_eq!(checks(&rollup(json!([]))), Checks::None);
    assert_eq!(checks(&json!({"number": 1})), Checks::None);
    assert_eq!(checks(&rollup(json!(null))), Checks::None);
}

fn pr_with(n: u32, checks: &str) -> String {
    format!(
        r#"{{"number":{n},"url":"https://github.com/o/r/pull/{n}","reviewDecision":"APPROVED","mergeable":"MERGEABLE","statusCheckRollup":{checks}}}"#
    )
}

const FAIL: &str = r#"[{"__typename":"CheckRun","status":"COMPLETED","conclusion":"FAILURE"}]"#;
const PEND: &str = r#"[{"__typename":"CheckRun","status":"IN_PROGRESS","conclusion":""}]"#;
const PASS: &str = r#"[{"__typename":"CheckRun","status":"COMPLETED","conclusion":"SUCCESS"}]"#;

/// PRs 4 fail, 3 pending, 2 pass, 1 no checks.
fn four() -> Fx {
    let f = Fx::github();
    let body = [
        pr_with(4, FAIL),
        pr_with(3, PEND),
        pr_with(2, PASS),
        pr_with(1, "[]"),
    ]
    .join(",");
    f.out("out.json", &format!("[{body}]"));
    f.prime(Kind::Pr, &[]);
    f
}

#[test]
fn default_marks_failing_only() {
    let f = four();
    let out = f.show(Kind::Pr, &[]);
    assert_eq!(strip(&out), "#4✗ #3 #2 #1");
    assert!(out.contains("\x1b[38;5;203m✗\x1b[39m"), "{out:?}");
}

#[test]
fn checks_option_picks_marked_states() {
    let f = four();
    let show = |v: &str| strip(&f.show(Kind::Pr, &[("checks", v)]));
    assert_eq!(show("fail,pending"), "#4✗ #3• #2 #1");
    assert_eq!(show("fail,pending,pass"), "#4✗ #3• #2✓ #1");
    assert_eq!(show("pending"), "#4 #3• #2 #1");
    assert_eq!(show("none"), "#4 #3 #2 #1");
    let out = f.show(Kind::Pr, &[("checks", "pending,pass")]);
    assert!(out.contains("\x1b[38;5;179m•\x1b[39m"), "{out:?}");
    assert!(out.contains("\x1b[38;5;114m✓\x1b[39m"), "{out:?}");
}

#[test]
fn mark_sits_after_the_link() {
    let f = four();
    let out = f.show(Kind::Pr, &[]);
    let close = "\x1b]8;;\x1b\\";
    let i = out.find("\x1b]8;;https://github.com/o/r/pull/4").unwrap();
    let after = &out[i..];
    let end = after.find(close).unwrap() + close.len();
    assert!(!after[..end].contains('✗'), "{out:?}");
    assert!(after[end..].starts_with("\x1b[38;5;203m✗"), "{out:?}");
    let plain = f.show(Kind::Pr, &[("link", "off")]);
    assert!(!plain.contains("\x1b]8") && strip(&plain).starts_with("#4✗"));
}

#[test]
fn icon_set_picks_mark_glyph() {
    let f = four();
    let o = [("checks", "fail,pending,pass")];
    let mut cfg = Config {
        icons: Icons::Nerdfont,
        ..Config::default()
    };
    assert_eq!(
        strip(&f.show_with(&cfg, Kind::Pr, &o)),
        "#4\u{f467} #3\u{f444} #2\u{f42e} #1"
    );
    cfg.icons = Icons::Emoji;
    assert_eq!(strip(&f.show_with(&cfg, Kind::Pr, &o)), "#4❌ #3🟡 #2✅ #1");
}

#[test]
fn palette_colours_the_mark() {
    let f = four();
    let mut cfg = Config::default();
    cfg.colors.bad = 9.into();
    let out = f.show_with(&cfg, Kind::Pr, &[]);
    assert!(out.contains("\x1b[38;5;9m✗\x1b[39m"), "{out:?}");
}

#[test]
fn old_cache_without_check_data_renders_as_before() {
    let f = Fx::github();
    let old = r#"[{"number":7,"url":"https://github.com/o/r/pull/7","reviewDecision":"APPROVED","mergeable":"MERGEABLE"}]"#;
    f.out("out.json", old);
    f.prime(Kind::Pr, &[]);
    let o = [("checks", "fail,pending,pass")];
    let out = f.show(Kind::Pr, &o);
    assert_eq!(strip(&out), "#7");
    assert!(out.contains("\x1b[38;5;114m#7\x1b[39m"), "{out:?}");
}

#[test]
fn issues_never_get_marks() {
    let f = Fx::github();
    f.out(
        "out.json",
        r#"[{"number":5,"url":"https://github.com/o/r/issues/5","labels":[],"statusCheckRollup":[{"state":"FAILURE"}]}]"#,
    );
    f.prime(Kind::Issue, &[]);
    assert_eq!(strip(&f.show(Kind::Issue, &[("checks", "fail")])), "!5");
}

fn preview_prs(opts: &str, icons: &str) -> String {
    let src = format!("set icons {icons}\nset left \"#{{prs{opts}}}\"\n");
    let c = hop_bar::conf::parse(&src).unwrap();
    let out = hop_bar::preview::render("early", hop_bar::conf::LoadResult::Ok(c), Some(80), 0);
    strip(&out).trim().to_string()
}

#[test]
fn preview_samples_check_states() {
    assert_eq!(preview_prs("", "none"), "#131✗ #129 #128");
    let all = "[checks=fail,pending,pass;icon=none]";
    assert_eq!(preview_prs(all, "none"), "#131✗ #129• #128✓");
    assert_eq!(preview_prs(all, "emoji"), "#131❌ #129🟡 #128✅");
    assert_eq!(preview_prs("[checks=none]", "none"), "#131 #129 #128");
}
