use hop_bar::cache::{Job, Request};
use hop_bar::gitfs::{discover, Found};
use hop_bar::placeholders::gitst::{cache_key, parse, Status};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

static N: AtomicUsize = AtomicUsize::new(0);

fn tmp() -> PathBuf {
    let p = std::env::temp_dir().join(format!(
        "bar-gitst-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir_all(&p).unwrap();
    p.canonicalize().unwrap()
}

fn git(dir: &Path, args: &[&str]) {
    let st = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@t", "-C"])
        .arg(dir)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .status()
        .unwrap();
    assert!(st.success(), "git {args:?}");
}

const CLEAN: &str =
    "# branch.oid 1234\n# branch.head main\n# branch.upstream origin/main\n# branch.ab +0 -0\n";
const MIXED: &str = "# branch.oid 1234
# branch.head main
# branch.upstream origin/main
# branch.ab +2 -1
1 M. N... 100644 100644 100644 aaa bbb staged.rs
1 MM N... 100644 100644 100644 aaa bbb both.rs
1 .M N... 100644 100644 100644 aaa bbb work.rs
1 .D N... 100644 100644 000000 aaa bbb gone.rs
? new.txt
? dir/other.txt
! ignored.log
";
const RENAMED: &str = "# branch.head main
2 R. N... 100644 100644 100644 aaa bbb R100 new name.rs\told name.rs
2 RM N... 100644 100644 100644 aaa bbb R90 b.rs\ta.rs
";
const CONFLICT: &str = "# branch.head main
u UU N... 100644 100644 100644 100644 aaa bbb ccc clash.rs
1 A. N... 000000 100644 100644 000 bbb added.rs
";
const NO_UPSTREAM: &str = "# branch.oid 1234\n# branch.head topic\n? a\n";
const DETACHED: &str =
    "# branch.oid 1234\n# branch.head (detached)\n1 .M N... 100644 100644 100644 a b f\n";

fn counts(s: &Status) -> (usize, usize, usize) {
    (s.staged, s.modified, s.untracked)
}

#[test]
fn parse_clean() {
    let s = parse(CLEAN);
    assert_eq!(counts(&s), (0, 0, 0));
    assert_eq!(s.ahead_behind, Some((0, 0)));
}

#[test]
fn parse_mixed() {
    let s = parse(MIXED);
    assert_eq!(counts(&s), (2, 3, 2));
    assert_eq!(s.ahead_behind, Some((2, 1)));
}

#[test]
fn parse_renamed() {
    assert_eq!(counts(&parse(RENAMED)), (2, 1, 0));
}

#[test]
fn parse_conflicted_counts_as_modified() {
    assert_eq!(counts(&parse(CONFLICT)), (1, 1, 0));
}

#[test]
fn parse_no_upstream_and_detached() {
    let s = parse(NO_UPSTREAM);
    assert_eq!((counts(&s), s.ahead_behind), ((0, 0, 1), None));
    let s = parse(DETACHED);
    assert_eq!((counts(&s), s.ahead_behind), ((0, 1, 0), None));
}

#[test]
fn parse_garbage_is_empty() {
    let s = parse("\u{0}junk\n# branch.ab nonsense\n");
    assert_eq!((counts(&s), s.ahead_behind), ((0, 0, 0), None));
}

#[test]
fn linked_worktree_detected() {
    let root = tmp();
    let main = root.join("main");
    std::fs::create_dir_all(&main).unwrap();
    git(&main, &["init", "-q", "-b", "main"]);
    git(&main, &["commit", "-q", "--allow-empty", "-m", "x"]);
    let wt = root.join("wt-feature");
    git(
        &main,
        &["worktree", "add", "-q", wt.to_str().unwrap(), "-b", "f"],
    );
    let Found::Repo(m) = discover(&main) else {
        panic!()
    };
    let Found::Repo(w) = discover(&wt) else {
        panic!()
    };
    assert!(!m.linked());
    assert!(w.linked());
    assert_eq!(w.top, wt);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn submodule_style_gitfile_is_not_linked() {
    let root = tmp();
    let real = root.join("real");
    std::fs::create_dir_all(&real).unwrap();
    git(&real, &["init", "-q", "-b", "main"]);
    let sub = root.join("sub");
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::write(
        sub.join(".git"),
        format!("gitdir: {}\n", real.join(".git").display()),
    )
    .unwrap();
    let Found::Repo(r) = discover(&sub) else {
        panic!()
    };
    assert!(!r.linked());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn job_runs_status_in_repo_root() {
    let root = tmp();
    git(&root, &["init", "-q", "-b", "main"]);
    std::fs::write(root.join("u.txt"), "x").unwrap();
    let job = Job::GitStatus {
        dir: root.to_string_lossy().into(),
    };
    let out = job.execute(Duration::from_secs(10)).expect("status output");
    assert!(out.contains("# branch.head main"), "{out}");
    assert!(out.contains("? u.txt"), "{out}");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn job_fails_outside_repo_and_for_relative_dir() {
    let root = tmp();
    let job = Job::GitStatus {
        dir: root.to_string_lossy().into(),
    };
    assert_eq!(job.execute(Duration::from_secs(5)), None);
    let req = Request {
        key: "k".into(),
        job: Job::GitStatus { dir: "rel".into() },
        lock: String::new(),
    };
    assert!(Request::from_json(&req.to_json()).is_err());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn job_roundtrips() {
    let req = Request {
        key: cache_key(Path::new("/r")),
        job: Job::GitStatus { dir: "/r".into() },
        lock: "t".into(),
    };
    assert_eq!(Request::from_json(&req.to_json()).unwrap(), req);
}

#[test]
fn show_list_forms() {
    use hop_bar::placeholders::gitst::{parse_show, SHOW};
    assert_eq!(
        parse_show("\"staged,untracked\"", SHOW),
        Some(vec!["staged", "untracked"])
    );
    assert_eq!(
        parse_show("modified+staged", SHOW),
        Some(vec!["modified", "staged"])
    );
    assert_eq!(parse_show("staged,nope", SHOW), None);
}
