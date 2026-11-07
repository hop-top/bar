use hop_bar::cache::{Cache, Request};
use hop_bar::conf::Config;
use hop_bar::options::Options;
use hop_bar::placeholders::gh::{plan, Issues, Kind, Prs};
use hop_bar::placeholders::{Ctx, Placeholder};
use hop_bar::render::ColdRunner;
use hop_bar::session::Session;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

static N: AtomicUsize = AtomicUsize::new(0);
static ENV: Mutex<()> = Mutex::new(());

const FAKE_GH: &str = r#"#!/bin/sh
printf '%s\n' "$*" >> "$FAKE_GH_LOG"
if [ -n "$FAKE_GH_FAIL" ]; then echo "API rate limit exceeded" >&2; exit 1; fi
q=""; prev=""
for a in "$@"; do [ "$prev" = "--search" ] && q=$a; prev=$a; done
k=$(printf %s "$q" | tr -c 'A-Za-z0-9' _)
f="$FAKE_GH_DIR/out-$k.json"
[ -f "$f" ] || f="$FAKE_GH_DIR/out.json"
cat "$f"
"#;

/// Temp dir, fake `gh` on PATH and a git repo; holds the env lock.
struct Fx {
    root: PathBuf,
    state: PathBuf,
    repo: PathBuf,
    old_path: Option<std::ffi::OsString>,
    _lock: MutexGuard<'static, ()>,
}

impl Fx {
    fn new(remote: Option<&str>) -> Fx {
        let lock = ENV.lock().unwrap_or_else(|e| e.into_inner());
        let root = std::env::temp_dir().join(format!(
            "bar-gh-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        let (bin, data, state, repo) = (
            root.join("bin"),
            root.join("data"),
            root.join("state"),
            root.join("repo"),
        );
        for d in [&bin, &data, &state, &repo] {
            std::fs::create_dir_all(d).unwrap();
        }
        let root = root.canonicalize().unwrap();
        let (bin, data, state, repo) = (
            root.join("bin"),
            root.join("data"),
            root.join("state"),
            root.join("repo"),
        );
        let gh = bin.join("gh");
        std::fs::write(&gh, FAKE_GH).unwrap();
        std::os::unix::fs::PermissionsExt::set_mode(
            &mut std::fs::metadata(&gh).unwrap().permissions(),
            0o755,
        );
        std::fs::set_permissions(&gh, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        git(&repo, &["init", "-q"]);
        if let Some(r) = remote {
            git(&repo, &["remote", "add", "origin", r]);
        }
        let old_path = std::env::var_os("PATH");
        let mut path = bin.into_os_string();
        path.push(":");
        path.push(old_path.clone().unwrap_or_default());
        std::env::set_var("PATH", path);
        std::env::set_var("FAKE_GH_DIR", &data);
        std::env::set_var("FAKE_GH_LOG", root.join("gh.log"));
        std::env::remove_var("FAKE_GH_FAIL");
        Fx {
            root,
            state,
            repo,
            old_path,
            _lock: lock,
        }
    }

    fn github() -> Fx {
        Fx::new(Some("https://github.com/o/r.git"))
    }

    fn out(&self, rel: &str, body: &str) {
        std::fs::write(self.root.join("data").join(rel), body).unwrap();
    }

    fn fixture(&self, name: &str) {
        let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        self.out("out.json", &std::fs::read_to_string(p.join(name)).unwrap());
    }

    fn calls(&self) -> Vec<String> {
        std::fs::read_to_string(self.root.join("gh.log"))
            .unwrap_or_default()
            .lines()
            .map(String::from)
            .collect()
    }

    fn cache(&self) -> Cache {
        Cache::with_exe(&self.state, PathBuf::from("/nonexistent/bar"))
    }

    /// Runs every query's job now, as a finished background refresh would.
    fn prime(&self, kind: Kind, o: &[(&str, &str)]) {
        let cache = self.cache();
        for (key, job) in plan(kind, "https://github.com/o/r", &opts(o)).unwrap().jobs {
            cache.refresh(
                &Request {
                    key,
                    job,
                    lock: String::new(),
                },
                Duration::from_secs(10),
            );
        }
    }

    fn show(&self, kind: Kind, o: &[(&str, &str)]) -> String {
        let session = Session {
            cwd: self.repo.display().to_string(),
            ..Session::default()
        };
        let cfg = Config::default();
        let cache = self.cache();
        let ctx = Ctx {
            session: &session,
            config: &cfg,
            now: 0,
            runner: &ColdRunner,
            cache: Some(&cache),
        };
        let p: &dyn Placeholder = match kind {
            Kind::Pr => &Prs,
            Kind::Issue => &Issues,
        };
        p.render(&ctx, &opts(o)).unwrap_or_default()
    }
}

impl Drop for Fx {
    fn drop(&mut self) {
        match &self.old_path {
            Some(p) => std::env::set_var("PATH", p),
            None => std::env::remove_var("PATH"),
        }
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn git(dir: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .status()
        .unwrap()
        .success();
    assert!(ok, "git {args:?}");
}

fn opts(o: &[(&str, &str)]) -> Options {
    let mut out = Options::default();
    for (k, v) in o {
        out.set(k, v);
    }
    out
}

fn strip(s: &str) -> String {
    let mut out = String::new();
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c != '\x1b' {
            out.push(c);
            continue;
        }
        match it.next() {
            Some('[') => {
                for n in it.by_ref() {
                    if ('\x40'..='\x7e').contains(&n) {
                        break;
                    }
                }
            }
            Some(']') => {
                while let Some(n) = it.next() {
                    if n == '\x1b' {
                        it.next();
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    out
}

fn pr_status(name: &str, color: u8) {
    let f = Fx::github();
    f.fixture(name);
    f.prime(Kind::Pr, &[]);
    let out = f.show(Kind::Pr, &[]);
    assert_eq!(strip(&out), "#1", "{name}");
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
    pr_status("pr-failure.json", 203);
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
    pr_status("pr-status-error.json", 203);
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
    assert!(out.contains("\x1b[2m!42\x1b[22m"), "{out:?}");
    assert!(!out.contains("38;5;"), "{out:?}");
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
