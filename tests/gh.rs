#[path = "common/gh_fx.rs"]
mod gh_fx;

use gh_fx::{opts, strip, Fx};
use hop_bar::cache::Request;
use hop_bar::placeholders::gh::{plan, Kind};
use std::time::Duration;

fn pr_status(name: &str, color: u8) {
    pr_status_shown(name, color, "#1");
}

/// `shown`: the plain text, check mark included.
fn pr_status_shown(name: &str, color: u8, shown: &str) {
    let f = Fx::github();
    f.fixture(name);
    f.prime(Kind::Pr, &[]);
    let out = f.show(Kind::Pr, &[]);
    assert_eq!(strip(&out), shown, "{name}");
    assert!(
        out.contains(&format!("\x1b[38;5;{color}m#1\x1b[39m")),
        "{name}: {out:?}"
    );
}

#[test]
fn pr_approved_success_is_good() {
    pr_status("pr-approved-success.json", 114);
}
#[test]
fn pr_failure_is_bad() {
    pr_status_shown("pr-failure.json", 203, "#1✗");
}
#[test]
fn pr_in_progress_is_warn() {
    pr_status("pr-in-progress.json", 179);
}
#[test]
fn pr_review_required_is_warn() {
    pr_status("pr-review-required.json", 179);
}
#[test]
fn pr_changes_requested_is_bad() {
    pr_status("pr-changes-requested.json", 203);
}
#[test]
fn pr_conflicting_is_bad() {
    pr_status("pr-conflicting.json", 203);
}
#[test]
fn pr_status_error_is_bad() {
    pr_status_shown("pr-status-error.json", 203, "#1✗");
}
#[test]
fn pr_status_pending_is_warn() {
    pr_status("pr-status-pending.json", 179);
}
#[test]
fn pr_skipped_is_good() {
    pr_status("pr-skipped.json", 114);
}

fn pr(n: u32) -> String {
    format!(
        r#"{{"number":{n},"url":"https://github.com/o/r/pull/{n}","reviewDecision":"APPROVED","mergeable":"MERGEABLE","statusCheckRollup":[]}}"#
    )
}

#[test]
fn limit_applies_after_merge() {
    let f = Fx::github();
    f.out("out-author_a.json", &format!("[{},{}]", pr(5), pr(3)));
    f.out("out-author_b.json", &format!("[{},{}]", pr(4), pr(2)));
    let o = [("author", "a|b"), ("limit", "3")];
    f.prime(Kind::Pr, &o);
    assert_eq!(strip(&f.show(Kind::Pr, &o)), "#5 #4 #3");
}

#[test]
fn merged_queries_dedupe_by_number() {
    let f = Fx::github();
    f.out("out-author_a.json", &format!("[{},{}]", pr(7), pr(6)));
    f.out("out-author_b.json", &format!("[{},{}]", pr(7), pr(8)));
    let o = [("author", "a|b")];
    f.prime(Kind::Pr, &o);
    assert_eq!(strip(&f.show(Kind::Pr, &o)), "#8 #7 #6");
}

fn issue(n: u32, label: &str) -> String {
    format!(
        r#"{{"number":{n},"url":"https://github.com/o/r/issues/{n}","labels":[{{"name":"{label}"}}]}}"#
    )
}

#[test]
fn issue_with_bug_label_is_bad() {
    let f = Fx::github();
    f.out("out.json", &format!("[{}]", issue(42, "bug")));
    f.prime(Kind::Issue, &[]);
    let out = f.show(Kind::Issue, &[]);
    assert!(out.contains("\x1b[38;5;203m!42\x1b[39m"), "{out:?}");
}

#[test]
fn issue_otherwise_muted() {
    let f = Fx::github();
    f.out("out.json", &format!("[{}]", issue(42, "docs")));
    f.prime(Kind::Issue, &[]);
    let out = f.show(Kind::Issue, &[]);
    assert!(out.contains("\x1b[38;5;250m!42\x1b[39m"), "{out:?}");
    assert!(!out.replace("38;5;250m", "").contains("38;5;"), "{out:?}");
}

#[test]
fn issues_render_with_bang() {
    let f = Fx::github();
    f.out(
        "out.json",
        &format!("[{},{}]", issue(9, "x"), issue(12, "x")),
    );
    f.prime(Kind::Issue, &[]);
    assert_eq!(strip(&f.show(Kind::Issue, &[])), "!12 !9");
}

#[test]
fn each_number_links_to_its_url() {
    let f = Fx::github();
    f.out("out.json", &format!("[{},{}]", pr(2), pr(1)));
    f.prime(Kind::Pr, &[]);
    let out = f.show(Kind::Pr, &[]);
    for n in [1, 2] {
        let want = format!("\x1b]8;;https://github.com/o/r/pull/{n}\x1b\\");
        assert!(out.contains(&want), "{out:?}");
    }
    assert!(!f.show(Kind::Pr, &[("link", "false")]).contains("\x1b]8"));
}

#[test]
fn hostile_url_not_linked() {
    let f = Fx::github();
    let bad = r#"[{"number":1,"url":"https://x/\u001b]0;pwn\u0007","statusCheckRollup":[]}]"#;
    f.out("out.json", bad);
    f.prime(Kind::Pr, &[]);
    let out = f.show(Kind::Pr, &[]);
    assert!(!out.contains("\x1b]0;"), "{out:?}");
    assert!(!out.contains('\x07'), "{out:?}");
}

#[test]
fn non_github_remote_omitted() {
    for remote in [
        Some("https://gitlab.com/o/r.git"),
        Some("https://github.com.evil.io/o/r"),
        None,
    ] {
        let f = Fx::new(remote);
        f.fixture("pr-skipped.json");
        assert!(plan(Kind::Pr, "https://gitlab.com/o/r", &opts(&[])).is_none());
        assert_eq!(f.show(Kind::Pr, &[]), "", "{remote:?}");
        assert!(f.calls().is_empty(), "{remote:?}");
    }
}

#[test]
fn gh_missing_omitted() {
    let f = Fx::github();
    let jobs = plan(Kind::Pr, "https://github.com/o/r", &opts(&[]))
        .unwrap()
        .jobs;
    std::fs::remove_file(f.root.join("bin/gh")).unwrap();
    let cache = f.cache();
    for (key, job) in jobs {
        let req = Request {
            key,
            job,
            lock: String::new(),
        };
        cache.refresh(&req, Duration::from_secs(5));
    }
    assert_eq!(f.show(Kind::Pr, &[]), "");
}

#[test]
fn rate_limit_keeps_last_value() {
    let f = Fx::github();
    f.out("out.json", &format!("[{}]", pr(3)));
    f.prime(Kind::Pr, &[]);
    std::env::set_var("FAKE_GH_FAIL", "1");
    f.prime(Kind::Pr, &[]);
    assert_eq!(f.calls().len(), 2);
    assert_eq!(strip(&f.show(Kind::Pr, &[])), "#3");
}

#[test]
fn limit_zero_prints_nothing() {
    let f = Fx::github();
    f.out("out.json", &format!("[{}]", pr(3)));
    f.prime(Kind::Pr, &[]);
    assert_eq!(f.show(Kind::Pr, &[("limit", "0")]), "");
    assert!(f.calls().len() == 1, "no fetch for limit=0");
}

#[test]
fn status_maps_to_state_flag() {
    let f = Fx::github();
    let args = |kind, status: &str| {
        let p = plan(kind, "https://github.com/o/r", &opts(&[("status", status)])).unwrap();
        match &p.jobs[0].1 {
            hop_bar::cache::Job::Gh { args } => args.join(" "),
            _ => unreachable!(),
        }
    };
    for s in ["open", "closed", "merged"] {
        assert!(args(Kind::Pr, s).contains(&format!("--state {s}")), "{s}");
    }
    assert!(args(Kind::Issue, "closed").contains("--state closed"));
    assert!(args(Kind::Issue, "merged").contains("--state open"));
    assert!(args(Kind::Pr, "closed").starts_with("pr list -R o/r"));
    f.prime(Kind::Pr, &[("status", "merged")]);
    assert!(f.calls()[0].contains("--state merged"), "{:?}", f.calls());
}

#[test]
fn search_terms_compile_from_options() {
    let p = plan(
        Kind::Issue,
        "https://github.com/o/r",
        &opts(&[("assignee", "@me"), ("label", "a|b")]),
    )
    .unwrap();
    let hop_bar::cache::Job::Gh { args } = &p.jobs[0].1 else {
        unreachable!()
    };
    let i = args.iter().position(|a| a == "--search").unwrap();
    assert_eq!(args[i + 1], "assignee:@me label:a,b");
    assert!(p.jobs[0]
        .0
        .starts_with("gh:issue:o/r:assignee:@me label:a,b:open:5"));
}

#[test]
fn query_cap_8_truncates_and_check_warns() {
    let nine = "a1|a2|a3|a4|a5|a6|a7|a8|a9";
    let p = plan(
        Kind::Pr,
        "https://github.com/o/r",
        &opts(&[("author", nine)]),
    )
    .unwrap();
    assert_eq!(p.jobs.len(), 8);
    assert!(p.warning.as_deref().unwrap().contains("9 queries"));
    let eight = "a1|a2|a3|a4|a5|a6|a7|a8";
    let p = plan(
        Kind::Pr,
        "https://github.com/o/r",
        &opts(&[("author", eight)]),
    )
    .unwrap();
    assert_eq!((p.jobs.len(), p.warning), (8, None));
}
