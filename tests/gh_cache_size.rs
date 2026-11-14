//! `prs` on repositories whose PRs carry many check runs: the `gh` list is
//! reduced before it is cached, and nothing over the cache's size cap is
//! stored cut.

// Unix fixtures (shell scripts, modes, POSIX paths); Windows coverage: tests/windows.rs
#![cfg(unix)]

mod common;
#[path = "common/gh_fx.rs"]
mod gh_fx;

use gh_fx::{opts, strip, Fx};
use hop_bar::cache::{Request, MAX_VALUE};
use hop_bar::placeholders::gh::{plan, Kind};
use std::time::Duration;

/// Only the fixture's `gh` and the system's `sh` tools, whatever the
/// caller's PATH holds.
fn fx() -> Fx {
    let f = Fx::github();
    std::env::set_var(
        "PATH",
        format!("{}:/usr/bin:/bin", f.root.join("bin").display()),
    );
    f
}

/// A check run as `gh pr list --json statusCheckRollup` prints it.
fn run(pr: u32, i: u32, status: &str, conclusion: &str) -> String {
    format!(
        r#"{{"__typename":"CheckRun","completedAt":"2026-11-01T10:00:00Z","conclusion":"{conclusion}","detailsUrl":"https://github.com/o/r/actions/runs/1923456789{pr}/job/549876{i:05}","name":"test (ubuntu-latest, stable, shard {i})","startedAt":"2026-11-01T09:00:00Z","status":"{status}","workflowName":"CI"}}"#
    )
}

/// PR `n` with `count` runs, all passing but the last, `last`.
fn pr(n: u32, review: &str, last: &str, count: u32) -> String {
    let mut runs: Vec<String> = (1..count)
        .map(|i| run(n, i, "COMPLETED", "SUCCESS"))
        .collect();
    runs.push(last.to_string());
    format!(
        r#"{{"mergeable":"MERGEABLE","number":{n},"reviewDecision":"{review}","statusCheckRollup":[{}],"url":"https://github.com/o/r/pull/{n}"}}"#,
        runs.join(",")
    )
}

/// 5 PRs x `count` runs: 5 fails, 4 pends, 3 passes, 2 fails by status
/// context, 1 passes but has changes requested.
fn list(count: u32) -> String {
    let body = [
        pr(5, "APPROVED", &run(5, 0, "COMPLETED", "FAILURE"), count),
        pr(4, "APPROVED", &run(4, 0, "IN_PROGRESS", ""), count),
        pr(3, "APPROVED", &run(3, 0, "COMPLETED", "SUCCESS"), count),
        pr(
            2,
            "APPROVED",
            r#"{"__typename":"StatusContext","state":"ERROR"}"#,
            count,
        ),
        pr(
            1,
            "CHANGES_REQUESTED",
            &run(1, 0, "COMPLETED", "SUCCESS"),
            count,
        ),
    ]
    .join(",");
    format!("[{body}]")
}

/// 47 runs per PR: a reply just over the cache's cap, as a real
/// repository's five open PRs give.
fn big() -> String {
    list(47)
}

fn key() -> String {
    plan(Kind::Pr, "https://github.com/o/r", &opts(&[]))
        .unwrap()
        .jobs[0]
        .0
        .clone()
}

const ALL: [(&str, &str); 1] = [("checks", "fail,pending,pass")];

#[test]
fn fixture_is_over_the_cap() {
    let n = big().len();
    assert!(n > MAX_VALUE && n < MAX_VALUE + 8 * 1024, "{n}");
}

#[test]
fn many_check_runs_show_every_pr() {
    let f = fx();
    f.out("out.json", &big());
    f.prime(Kind::Pr, &[]);
    let out = f.show(Kind::Pr, &ALL);
    assert_eq!(strip(&out), "#5✗ #4• #3✓ #2✗ #1✓");
    for (n, color) in [(5, 203), (4, 179), (3, 114), (2, 203), (1, 203)] {
        assert!(
            out.contains(&format!("\x1b[38;5;{color}m#{n}\x1b[39m")),
            "#{n}: {out:?}"
        );
    }
}

/// The cached list is small and does not grow with the number of runs.
#[test]
fn cached_list_is_compact() {
    let f = fx();
    let cached = |count| {
        f.out("out.json", &list(count));
        f.prime(Kind::Pr, &[]);
        std::fs::read_to_string(f.cache().path_for(&key())).unwrap()
    };
    let (few, many) = (cached(5), cached(47));
    assert_eq!(few, many);
    assert!(many.len() < 1024, "{} bytes: {many}", many.len());
    assert!(!many.contains("statusCheckRollup"), "{many}");
    let v: serde_json::Value = serde_json::from_str(&many).unwrap();
    assert_eq!(v.as_array().unwrap().len(), 5);
}

/// A small list renders the same from the old raw cache entry and from
/// the reduced one.
#[test]
fn small_list_renders_as_before() {
    let f = fx();
    let small = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/pr-failure.json"),
    )
    .unwrap();
    for o in [&[][..], &ALL[..], &[("link", "off")][..]] {
        f.cache().store(&key(), &small).unwrap();
        let old = f.show(Kind::Pr, o);
        f.out("out.json", &small);
        f.prime(Kind::Pr, &[]);
        let new = std::fs::read_to_string(f.cache().path_for(&key())).unwrap();
        assert_ne!(new, small, "not reduced");
        assert_eq!(f.show(Kind::Pr, o), old, "{o:?}");
        assert!(!old.is_empty());
    }
}

#[test]
fn cut_gh_output_keeps_last_value_and_backs_off() {
    let f = fx();
    f.out("out.json", &big());
    f.prime(Kind::Pr, &[]);
    let good = std::fs::read_to_string(f.cache().path_for(&key())).unwrap();
    let cut = &big()[..big().len() / 2];
    f.out("out.json", cut);
    f.prime(Kind::Pr, &[]);
    let c = f.cache();
    assert_eq!(std::fs::read_to_string(c.path_for(&key())).unwrap(), good);
    assert!(c.fail_path(&key()).exists(), "no backoff");
    assert_eq!(strip(&f.show(Kind::Pr, &ALL)), "#5✗ #4• #3✓ #2✗ #1✓");
}

#[test]
fn oversize_value_is_never_stored() {
    let f = fx();
    let c = f.cache();
    c.store("k", "old").unwrap();
    let big = format!("[{}]", "1,".repeat(MAX_VALUE) + "1");
    assert!(c.store("k", &big).is_err());
    assert_eq!(std::fs::read_to_string(c.path_for("k")).unwrap(), "old");
}

#[test]
fn oversize_job_result_keeps_last_value_and_backs_off() {
    let f = fx();
    let c = f.cache();
    c.store("k", "old").unwrap();
    // one JSON array of 1000 PRs, reduced still over the cap
    let body: Vec<String> = (1..=1000)
        .map(|n| {
            format!(
                r#"{{"number":{n},"url":"https://github.com/o/r/pull/{n}","statusCheckRollup":[]}}"#
            )
        })
        .collect();
    f.out("out.json", &format!("[{}]", body.join(",")));
    let (_, job) = plan(
        Kind::Pr,
        "https://github.com/o/r",
        &opts(&[("limit", "1000")]),
    )
    .unwrap()
    .jobs
    .remove(0);
    let req = Request {
        key: "k".into(),
        job,
        lock: String::new(),
    };
    c.refresh(&req, Duration::from_secs(10));
    assert_eq!(std::fs::read_to_string(c.path_for("k")).unwrap(), "old");
    assert!(c.fail_path("k").exists(), "no backoff");
}
