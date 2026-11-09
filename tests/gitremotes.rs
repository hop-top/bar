// Unix fixtures (POSIX paths, git CLI); Windows coverage: tests/windows.rs
#![cfg(unix)]

#[path = "common/git_fx.rs"]
mod git_fx;
use git_fx::*;

use hop_bar::gitfs::Unsupported;
use hop_bar::placeholders::remotes::parts;
use std::path::Path;

/// The upstream `git status --porcelain=v2 --branch` reports.
fn git_upstream(d: &Path) -> Option<String> {
    out(d, &["status", "--porcelain=v2", "--branch"])
        .lines()
        .find_map(|l| l.strip_prefix("# branch.upstream ").map(str::to_string))
}

fn upstream(d: &Path) -> Option<String> {
    open(d).upstream().unwrap().map(|u| u.name)
}

/// Clone of a repo with `main` and `feat/x`, so remote-tracking refs exist.
fn cloned(t: &Tmp) -> std::path::PathBuf {
    let seed = repo(t, "seed");
    git(&seed, &["branch", "feat/x"]);
    git(t.path(), &["clone", "-q", "seed", "c"]);
    t.path().join("c")
}

#[test]
fn upstream_matches_git_status() {
    let t = Tmp::new();
    let d = cloned(&t);
    assert_eq!(upstream(&d).as_deref(), Some("origin/main"));
    assert_eq!(upstream(&d), git_upstream(&d));
    let u = open(&d).upstream().unwrap().unwrap();
    assert_eq!((u.remote.as_str(), u.branch.as_str()), ("origin", "main"));

    // a branch with a slash, tracking another name
    git(
        &d,
        &["checkout", "-q", "-b", "mine", "--track", "origin/feat/x"],
    );
    assert_eq!(upstream(&d).as_deref(), Some("origin/feat/x"));
    assert_eq!(upstream(&d), git_upstream(&d));

    // no upstream
    git(&d, &["checkout", "-q", "-b", "solo"]);
    assert_eq!(upstream(&d), None);
    assert_eq!(git_upstream(&d), None);

    // tracking a local branch (remote `.`)
    git(&d, &["branch", "-q", "--set-upstream-to=main"]);
    assert_eq!(upstream(&d).as_deref(), Some("main"));
    assert_eq!(upstream(&d), git_upstream(&d));

    // detached HEAD
    git(&d, &["checkout", "-q", "--detach"]);
    assert_eq!(open(&d).upstream(), Ok(None));
}

#[test]
fn upstream_gone_still_named() {
    let t = Tmp::new();
    let d = cloned(&t);
    git(
        &d,
        &["checkout", "-q", "-b", "mine", "--track", "origin/feat/x"],
    );
    git(&d, &["update-ref", "-d", "refs/remotes/origin/feat/x"]);
    assert_eq!(upstream(&d).as_deref(), Some("origin/feat/x"));
    assert_eq!(upstream(&d), git_upstream(&d));
}

#[test]
fn upstream_follows_fetch_refspec() {
    let t = Tmp::new();
    let d = cloned(&t);
    git(
        &d,
        &[
            "config",
            "remote.origin.fetch",
            "+refs/heads/*:refs/remotes/mirror/*",
        ],
    );
    assert_eq!(upstream(&d).as_deref(), Some("mirror/main"));
    assert_eq!(upstream(&d), git_upstream(&d));
    // a remote without a matching refspec tracks nothing
    git(
        &d,
        &["config", "remote.origin.fetch", "refs/heads/other:refs/x"],
    );
    assert_eq!(upstream(&d), None);
    assert_eq!(git_upstream(&d), None);
    // remote missing from the config: nothing either
    git(&d, &["config", "branch.main.remote", "nowhere"]);
    assert_eq!(upstream(&d), None);
    assert_eq!(git_upstream(&d), None);
}

#[test]
fn upstream_in_linked_worktree() {
    let t = Tmp::new();
    let d = cloned(&t);
    git(
        &d,
        &[
            "worktree",
            "add",
            "-q",
            "--track",
            "-b",
            "wt",
            "../wt",
            "origin/feat/x",
        ],
    );
    let wt = t.path().join("wt");
    assert_eq!(upstream(&wt).as_deref(), Some("origin/feat/x"));
    assert_eq!(upstream(&wt), git_upstream(&wt));
}

#[test]
fn ambiguous_short_name_left_to_git() {
    let t = Tmp::new();
    let d = cloned(&t);
    // a local branch named like the remote-tracking one
    git(&d, &["branch", "origin/main"]);
    assert_eq!(open(&d).upstream(), Err(Unsupported));
    assert_eq!(git_upstream(&d).as_deref(), Some("remotes/origin/main"));
}

#[test]
fn upstream_config_git_cannot_be_read_from_files() {
    let t = Tmp::new();
    let d = cloned(&t);
    let cfg = d.join(".git/config");
    let body = std::fs::read_to_string(&cfg).unwrap();
    std::fs::write(&cfg, format!("{body}[include]\n\tpath = extra\n")).unwrap();
    assert_eq!(open(&d).upstream(), Err(Unsupported));
    // URL rewrites do not change the upstream
    std::fs::write(
        &cfg,
        format!("{body}[url \"https://m/\"]\n\tinsteadOf = x:\n"),
    )
    .unwrap();
    assert_eq!(upstream(&d).as_deref(), Some("origin/main"));
    assert_eq!(open(&d).origin_url(), Err(Unsupported));
}

#[test]
fn remote_urls_by_name() {
    let t = Tmp::new();
    let d = repo(&t, "p");
    git(&d, &["remote", "add", "origin", "git@github.com:me/p.git"]);
    git(
        &d,
        &["remote", "add", "upstream", "https://github.com/org/p"],
    );
    let r = open(&d);
    for name in ["origin", "upstream"] {
        assert_eq!(
            r.remote_url(name),
            Ok(Some(out(&d, &["remote", "get-url", name])))
        );
    }
    assert_eq!(r.remote_url("nope"), Ok(None));
}

#[test]
fn remote_parts() {
    let p = |s: &str| parts(s).map(|(h, o, r)| format!("{h}|{o}|{r}"));
    assert_eq!(
        p("git@github.com:o/r.git").as_deref(),
        Some("github.com|o|r")
    );
    assert_eq!(
        p("https://gitlab.com/g/sub/p").as_deref(),
        Some("gitlab.com|g/sub|p")
    );
    assert_eq!(p("ssh://git@h.io:2222/o/r/").as_deref(), Some("h.io|o|r"));
    assert_eq!(p("/srv/git/o/r.git").as_deref(), Some("|o|r"));
    assert_eq!(p("../r").as_deref(), Some("||r"));
    assert_eq!(p("r").as_deref(), Some("||r"));
    assert_eq!(p(""), None);
}

#[test]
fn remote_job_names_the_remote() {
    use hop_bar::cache::{Job, Request};
    let t = Tmp::new();
    let d = repo(&t, "p");
    git(&d, &["config", "url.https://mirror.io/.insteadOf", "gh:"]);
    git(&d, &["remote", "add", "upstream", "gh:org/p.git"]);
    let job = |name: &str| Job::GitRemote {
        dir: d.to_str().unwrap().into(),
        name: name.into(),
    };
    let limit = std::time::Duration::from_secs(10);
    assert_eq!(
        job("upstream").execute(limit).as_deref().map(str::trim),
        Some("https://mirror.io/org/p.git")
    );
    // a missing remote is known absent; a missing origin stays a failure
    assert_eq!(job("other").execute(limit).as_deref(), Some(""));
    assert_eq!(job("origin").execute(limit), None);

    let req = |job: Job| Request {
        key: "k".into(),
        job,
        lock: "l".into(),
    };
    let r = req(job("upstream"));
    assert_eq!(Request::from_json(&r.to_json()).unwrap(), r);
    // requests without a name are for origin
    let json = r.to_json().replace(",\"name\":\"upstream\"", "");
    assert!(!json.contains("upstream"), "{json}");
    assert_eq!(Request::from_json(&json).unwrap(), req(job("origin")));
    for bad in ["-x", "", "a b", "a;b"] {
        assert!(
            Request::from_json(&req(job(bad)).to_json()).is_err(),
            "{bad:?}"
        );
    }
}
