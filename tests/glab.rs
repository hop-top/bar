// Unix fixtures (shell scripts, modes, POSIX paths)
#![cfg(unix)]
//! GitLab remotes for `prs` and `issues`, with a fake `glab` on PATH.

#[path = "common/gh_fx.rs"]
mod gh_fx;

use gh_fx::{opts, strip, Fx};
use hop_bar::cache::{Job, Request};
use hop_bar::conf::{self, Config};
use hop_bar::placeholders::gh::Kind;
use hop_bar::placeholders::glab::{gitlab_repo, parse_hosts, plan};

const REMOTE: &str = "https://gitlab.com/g/sub/p";

fn gitlab() -> Fx {
    Fx::new(Some("git@gitlab.com:g/sub/p.git"))
}

fn mr(n: u32, extra: &str) -> String {
    format!(
        r#"{{"iid":{n},"web_url":"https://gitlab.com/g/sub/p/-/merge_requests/{n}","state":"opened","draft":false,"has_conflicts":false,"detailed_merge_status":"mergeable","description":"long text"{extra}}}"#
    )
}

fn view(n: u32, pipeline: &str, extra: &str) -> String {
    mr(
        n,
        &format!(r#","head_pipeline":{{"id":1,"status":"{pipeline}"}}{extra}"#),
    )
}

fn args(p: &hop_bar::placeholders::gh::Plan, i: usize) -> String {
    match &p.jobs[i].1 {
        Job::Glab { args, .. } => args.join(" "),
        _ => unreachable!(),
    }
}

/// One MR whose list entry carries `list_extra` and whose view reports
/// `pipeline` plus `view_extra`; returns the render and its plain text.
fn one_mr(list_extra: &str, pipeline: Option<&str>, view_extra: &str) -> (String, String) {
    let f = gitlab();
    f.out("glab-mr.json", &format!("[{}]", mr(12, list_extra)));
    if let Some(p) = pipeline {
        f.out("glab-view-12.json", &view(12, p, view_extra));
    }
    f.prime_gitlab(Kind::Pr, REMOTE, &[], &[("checks", "fail,pending,pass")]);
    let out = f.show(Kind::Pr, &[("checks", "fail,pending,pass")]);
    (strip(&out), out)
}

fn colored(out: &str, color: u8, text: &str) -> bool {
    out.contains(&format!("\x1b[38;5;{color}m{text}\x1b[39m"))
}

#[test]
fn mr_renders_with_bang_link_and_pipeline_mark() {
    let (plain, out) = one_mr("", Some("success"), "");
    assert_eq!(plain, "!12✓");
    assert!(colored(&out, 114, "!12"), "{out:?}");
    let url = "\x1b]8;;https://gitlab.com/g/sub/p/-/merge_requests/12\x1b\\";
    assert!(out.contains(url), "{out:?}");
}

#[test]
fn pipeline_states_map_to_check_marks() {
    for (status, shown, color) in [
        ("failed", "!12✗", 203),
        ("canceled", "!12✗", 203),
        ("running", "!12•", 179),
        ("pending", "!12•", 179),
        ("manual", "!12•", 179),
        ("skipped", "!12✓", 114),
    ] {
        let (plain, out) = one_mr("", Some(status), "");
        assert_eq!(plain, shown, "{status}");
        assert!(colored(&out, color, "!12"), "{status}: {out:?}");
    }
}

#[test]
fn approvals_and_conflicts_colour_the_number() {
    for (extra, color) in [
        (r#","detailed_merge_status":"not_approved""#, 179),
        (r#","detailed_merge_status":"requested_changes""#, 203),
        (r#","has_conflicts":true"#, 203),
        (r#","draft":true"#, 179),
    ] {
        let (plain, out) = one_mr("", Some("success"), extra);
        assert_eq!(plain, "!12✓", "{extra}");
        assert!(colored(&out, color, "!12"), "{extra}: {out:?}");
    }
}

#[test]
fn failed_view_falls_back_to_merge_status() {
    let (plain, _) = one_mr(r#","detailed_merge_status":"ci_must_pass""#, None, "");
    assert_eq!(plain, "!12✗");
    let (plain, _) = one_mr(r#","detailed_merge_status":"ci_still_running""#, None, "");
    assert_eq!(plain, "!12•");
    let (plain, out) = one_mr("", None, "");
    assert_eq!(plain, "!12");
    assert!(colored(&out, 114, "!12"), "{out:?}");
}

#[test]
fn list_and_views_run_in_one_job_without_a_shell() {
    let f = gitlab();
    f.out("glab-mr.json", &format!("[{},{}]", mr(5, ""), mr(3, "")));
    f.out("glab-view-5.json", &view(5, "success", ""));
    f.out("glab-view-3.json", &view(3, "failed", ""));
    f.prime_gitlab(Kind::Pr, REMOTE, &[], &[]);
    assert_eq!(
        f.glab_calls(),
        [
            format!("mr list -R {REMOTE} --per-page 5 --output json"),
            format!("mr view 5 -R {REMOTE} --output json"),
            format!("mr view 3 -R {REMOTE} --output json"),
        ]
    );
    assert_eq!(strip(&f.show(Kind::Pr, &[])), "!5 !3✗");
    assert!(f.calls().is_empty(), "gh not run for a GitLab remote");
}

#[test]
fn glab_runs_outside_the_project() {
    let f = gitlab();
    f.out("glab-mr.json", &format!("[{}]", mr(5, "")));
    f.out("glab-view-5.json", &view(5, "success", ""));
    let pwd = f.root.join("glab-pwd");
    let bin = f.root.join("bin/glab");
    let body = std::fs::read_to_string(&bin).unwrap();
    let body = body.replacen(
        "#!/bin/sh\n",
        &format!("#!/bin/sh\npwd -P >> '{}'\n", pwd.display()),
        1,
    );
    std::fs::write(&bin, body).unwrap();
    std::env::set_var("BAR_STATE_DIR", &f.state);
    f.prime_gitlab(Kind::Pr, REMOTE, &[], &[]);
    std::env::remove_var("BAR_STATE_DIR");
    let dirs = std::fs::read_to_string(&pwd).unwrap();
    let state = f.state.canonicalize().unwrap();
    assert_eq!(dirs.lines().count(), 2, "{dirs:?}");
    for d in dirs.lines() {
        assert_eq!(std::path::Path::new(d), state, "{dirs:?}");
    }
}

#[test]
fn pipeline_views_capped_at_ten() {
    let f = gitlab();
    let list: Vec<String> = (1..=15).rev().map(|n| mr(n, "")).collect();
    f.out("glab-mr.json", &format!("[{}]", list.join(",")));
    for n in 1..=15 {
        f.out(&format!("glab-view-{n}.json"), &view(n, "failed", ""));
    }
    f.prime_gitlab(Kind::Pr, REMOTE, &[], &[("limit", "15")]);
    let views: Vec<String> = f
        .glab_calls()
        .into_iter()
        .filter(|c| c.starts_with("mr view"))
        .collect();
    let want: Vec<String> = (6..=15)
        .rev()
        .map(|n| format!("mr view {n} -R {REMOTE} --output json"))
        .collect();
    assert_eq!(views, want);
    // past the cap, the list entry alone: no pipeline mark
    let shown = strip(&f.show(Kind::Pr, &[("limit", "15")]));
    assert!(shown.starts_with("!15✗ "), "{shown:?}");
    assert!(shown.ends_with(" !5 !4 !3 !2 !1"), "{shown:?}");
}

#[test]
fn stored_value_is_reduced() {
    let f = gitlab();
    f.out("glab-mr.json", &format!("[{}]", mr(5, "")));
    f.out("glab-view-5.json", &view(5, "success", ""));
    f.prime_gitlab(Kind::Pr, REMOTE, &[], &[]);
    let key = &plan(Kind::Pr, REMOTE, &[], &opts(&[])).unwrap().jobs[0].0;
    let stored = std::fs::read_to_string(f.cache().path_for(key)).unwrap();
    assert!(!stored.contains("long text"), "{stored}");
    assert!(stored.contains("\"number\":5"), "{stored}");
}

#[test]
fn issues_render_with_hash_and_bug_label() {
    let f = gitlab();
    f.out(
        "glab-issue.json",
        r#"[{"iid":7,"web_url":"https://gitlab.com/g/sub/p/-/issues/7","labels":["bug"]},
            {"iid":4,"web_url":"https://gitlab.com/g/sub/p/-/issues/4","labels":["docs"]}]"#,
    );
    f.prime_gitlab(Kind::Issue, REMOTE, &[], &[]);
    let out = f.show(Kind::Issue, &[]);
    assert_eq!(strip(&out), "#7 #4");
    assert!(colored(&out, 203, "#7"), "{out:?}");
    assert!(colored(&out, 250, "#4"), "{out:?}");
    assert_eq!(f.glab_calls().len(), 1, "no view for issues");
}

#[test]
fn self_hosted_needs_gitlab_hosts() {
    let f = Fx::new(Some("ssh://git@git.example.com:2222/team/app.git"));
    let remote = "https://git.example.com/team/app";
    f.out("glab-mr.json", &format!("[{}]", mr(9, "")));
    assert!(plan(Kind::Pr, remote, &[], &opts(&[])).is_none());
    assert_eq!(f.show(Kind::Pr, &[]), "");
    let hosts = vec!["git.example.com".to_string()];
    f.prime_gitlab(Kind::Pr, remote, &hosts, &[]);
    let cfg = conf::parse("set gitlab-hosts Git.Example.com\n").unwrap();
    assert_eq!(cfg.gitlab_hosts, hosts);
    assert_eq!(strip(&f.show_with(&cfg, Kind::Pr, &[])), "!9");
    assert!(f.glab_calls()[0].contains("-R https://git.example.com/team/app"));
}

#[test]
fn github_remote_never_runs_glab() {
    let f = Fx::github();
    f.out("out.json", "[]");
    f.prime(Kind::Pr, &[]);
    let cfg = conf::parse("set gitlab-hosts github.com\n").unwrap();
    f.show_with(&cfg, Kind::Pr, &[]);
    assert!(f.glab_calls().is_empty());
    assert_eq!(f.calls().len(), 1);
}

#[test]
fn failure_keeps_last_value_and_backs_off() {
    let f = gitlab();
    f.out("glab-mr.json", &format!("[{}]", mr(3, "")));
    f.prime_gitlab(Kind::Pr, REMOTE, &[], &[]);
    std::env::set_var("FAKE_GLAB_FAIL", "1");
    f.prime_gitlab(Kind::Pr, REMOTE, &[], &[]);
    assert_eq!(strip(&f.show(Kind::Pr, &[])), "!3");
    let key = &plan(Kind::Pr, REMOTE, &[], &opts(&[])).unwrap().jobs[0].0;
    assert!(f.cache().fail_path(key).is_file());
}

#[test]
fn filters_compile_to_glab_flags() {
    let p = plan(Kind::Issue, REMOTE, &[], &opts(&[("label", "a|b")])).unwrap();
    assert_eq!(p.jobs.len(), 2, "GitLab labels AND, so OR splits");
    assert!(
        args(&p, 0).contains("--label=a --output"),
        "{}",
        args(&p, 0)
    );
    assert!(
        args(&p, 1).contains("--label=b --output"),
        "{}",
        args(&p, 1)
    );
    let p = plan(Kind::Issue, REMOTE, &[], &opts(&[("label", "a&!b")])).unwrap();
    assert!(args(&p, 0).contains("--label=a --not-label=b"));
    let p = plan(Kind::Pr, REMOTE, &[], &opts(&[("author", "@me")])).unwrap();
    assert!(args(&p, 0).contains("--author=@me"));
    let o = opts(&[("author", "!bot"), ("assignee", "@me")]);
    let p = plan(Kind::Issue, REMOTE, &[], &o).unwrap();
    assert!(
        args(&p, 0).contains("--assignee=@me --not-author=bot")
            || args(&p, 0).contains("--not-author=bot --assignee=@me")
    );
    // no `--not-author` on `glab mr list`; two authors at once match nothing
    assert!(plan(Kind::Pr, REMOTE, &[], &opts(&[("author", "!bot")])).is_none());
    let p = plan(Kind::Pr, REMOTE, &[], &opts(&[("author", "a&b")])).unwrap();
    assert!(p.jobs.is_empty());
}

#[test]
fn status_maps_to_state_flags() {
    let a = |kind, s| {
        args(
            &plan(kind, REMOTE, &[], &opts(&[("status", s)])).unwrap(),
            0,
        )
    };
    assert!(!a(Kind::Pr, "open").contains("--closed"));
    assert!(a(Kind::Pr, "closed").contains("--closed"));
    assert!(a(Kind::Pr, "merged").contains("--merged"));
    assert!(a(Kind::Issue, "closed").contains("--closed"));
    assert!(!a(Kind::Issue, "merged").contains("--merged"));
    assert!(a(Kind::Issue, "open").starts_with("issue list -R"));
    let p = plan(Kind::Pr, REMOTE, &[], &opts(&[("limit", "500")])).unwrap();
    assert!(args(&p, 0).contains("--per-page 100"));
}

#[test]
fn hosts_and_repos_are_validated() {
    assert_eq!(
        parse_hosts(" A.example.com,,b.example.com,a.example.com").unwrap(),
        ["a.example.com", "b.example.com"]
    );
    assert!(parse_hosts("").unwrap().is_empty());
    for bad in ["-x.com", "a b", "x.com/p", "user@x.com"] {
        assert!(parse_hosts(bad).is_err(), "{bad}");
    }
    assert!(conf::parse("set gitlab-hosts a/b\n").is_err());
    let none: &[String] = &[];
    assert_eq!(
        gitlab_repo("https://GitLab.com/g/p", none).as_deref(),
        Some("https://gitlab.com/g/p")
    );
    // a self-hosted instance on its own port: matched by host name
    let hosts = vec!["git.example.com".to_string()];
    assert_eq!(
        gitlab_repo("https://git.example.com:8443/g/p", &hosts).as_deref(),
        Some("https://git.example.com:8443/g/p")
    );
    assert!(gitlab_repo("https://git.example.com:x/g/p", &hosts).is_none());
    for bad in [
        "https://gitlab.com/p",
        "https://gitlab.com/-g/p",
        "https://gitlab.com.evil.io/g/p",
        "http://gitlab.com/g/p",
        "https://gitlab.com/g/p?x",
    ] {
        assert!(gitlab_repo(bad, none).is_none(), "{bad}");
    }
}

#[test]
fn job_round_trips_and_rejects_bad_fields() {
    let p = plan(Kind::Pr, REMOTE, &[], &opts(&[])).unwrap();
    let (key, job) = p.jobs[0].clone();
    let req = Request {
        key,
        job,
        lock: "t".into(),
    };
    assert_eq!(Request::from_json(&req.to_json()).unwrap(), req);
    let json = req.to_json();
    for (from, to) in [
        (r#""glab":"mr""#, r#""glab":"sh""#),
        ("\"repo\":\"https://", "\"repo\":\"file://"),
    ] {
        let bad = json.replace(from, to);
        assert_ne!(bad, json, "{from}");
        assert!(Request::from_json(&bad).is_err(), "{from}");
    }
}

#[test]
fn default_config_has_no_hosts() {
    assert!(Config::default().gitlab_hosts.is_empty());
}
