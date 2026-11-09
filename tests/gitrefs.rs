// Unix fixtures (POSIX paths, git CLI); Windows coverage: tests/windows.rs
#![cfg(unix)]

#[path = "common/git_fx.rs"]
mod git_fx;
use git_fx::*;

use hop_bar::gitfs::{best_tag, Unsupported};
use hop_bar::placeholders::gitref::parse_tags;

#[test]
fn head_commit_from_loose_ref() {
    let t = Tmp::new();
    let d = repo(&t, "p");
    let r = open(&d);
    assert_eq!(r.head_commit(), Ok(Some(out(&d, &["rev-parse", "HEAD"]))));
}

#[test]
fn head_commit_from_packed_refs() {
    let t = Tmp::new();
    let d = repo(&t, "p");
    git(&d, &["checkout", "-q", "-b", "feat/x"]);
    git(&d, &["commit", "-q", "--allow-empty", "-m", "y"]);
    git(&d, &["pack-refs", "--all"]);
    assert!(!d.join(".git/refs/heads/feat/x").exists());
    assert_eq!(
        open(&d).head_commit(),
        Ok(Some(out(&d, &["rev-parse", "HEAD"])))
    );
    // a loose ref written after packing wins over the packed one
    git(&d, &["commit", "-q", "--allow-empty", "-m", "z"]);
    assert!(d.join(".git/refs/heads/feat/x").exists());
    assert_eq!(
        open(&d).head_commit(),
        Ok(Some(out(&d, &["rev-parse", "HEAD"])))
    );
}

#[test]
fn head_commit_detached_unborn_and_symbolic() {
    let t = Tmp::new();
    let d = repo(&t, "p");
    let first = out(&d, &["rev-parse", "HEAD"]);
    git(&d, &["commit", "-q", "--allow-empty", "-m", "y"]);
    git(&d, &["checkout", "-q", "--detach", &first]);
    assert_eq!(open(&d).head_commit(), Ok(Some(first.clone())));
    // a symbolic ref chain: HEAD -> refs/heads/alias -> refs/heads/main
    git(&d, &["checkout", "-q", "main"]);
    git(&d, &["symbolic-ref", "refs/heads/alias", "refs/heads/main"]);
    git(&d, &["symbolic-ref", "HEAD", "refs/heads/alias"]);
    assert_eq!(
        open(&d).head_commit(),
        Ok(Some(out(&d, &["rev-parse", "main"])))
    );
    git(&d, &["checkout", "-q", "--orphan", "fresh"]);
    assert_eq!(open(&d).head_commit(), Ok(None));
}

#[test]
fn head_commit_in_linked_worktree() {
    let t = Tmp::new();
    let d = repo(&t, "p");
    git(&d, &["worktree", "add", "-q", ".wt/x", "-b", "feat"]);
    let wt = d.join(".wt/x");
    git(&wt, &["commit", "-q", "--allow-empty", "-m", "wt"]);
    git(&d, &["pack-refs", "--all"]);
    let want = out(&wt, &["rev-parse", "HEAD"]);
    assert_ne!(want, out(&d, &["rev-parse", "HEAD"]));
    assert_eq!(open(&wt).head_commit(), Ok(Some(want)));
    assert_eq!(
        open(&d).head_commit(),
        Ok(Some(out(&d, &["rev-parse", "HEAD"])))
    );
}

#[test]
fn reftable_head_is_unsupported() {
    let t = Tmp::new();
    let d = repo(&t, "p");
    std::fs::write(d.join(".git/HEAD"), "ref: refs/heads/.invalid\n").unwrap();
    assert_eq!(open(&d).head_commit(), Err(Unsupported));
    std::fs::write(d.join(".git/HEAD"), "ref: refs/heads/../../x\n").unwrap();
    assert_eq!(open(&d).head_commit(), Err(Unsupported));
}

#[test]
fn lightweight_loose_tag_at_head() {
    let t = Tmp::new();
    let d = repo(&t, "p");
    git(&d, &["tag", "v1"]);
    let r = open(&d);
    let head = r.head_commit().unwrap().unwrap();
    let scan = r.tags_at(&head).unwrap();
    assert_eq!(scan.at, vec![("v1".to_string(), false)]);
    assert!(!scan.unknown);
}

#[test]
fn annotated_tags_peeled_from_packed_refs() {
    let t = Tmp::new();
    let d = repo(&t, "p");
    git(&d, &["tag", "old"]);
    git(&d, &["tag", "-a", "-m", "old", "old-a"]);
    git(&d, &["commit", "-q", "--allow-empty", "-m", "y"]);
    git(&d, &["tag", "-a", "-m", "v", "v1.9"]);
    git(&d, &["tag", "-a", "-m", "v", "v1.10"]);
    git(&d, &["tag", "-a", "-m", "v", "rel/x"]);
    git(&d, &["tag", "lw"]);
    git(&d, &["pack-refs", "--all"]);
    let r = open(&d);
    let head = r.head_commit().unwrap().unwrap();
    let mut scan = r.tags_at(&head).unwrap();
    assert!(!scan.unknown, "{scan:?}");
    scan.at.sort();
    assert_eq!(
        scan.at,
        vec![
            ("lw".to_string(), false),
            ("rel/x".to_string(), true),
            ("v1.10".to_string(), true),
            ("v1.9".to_string(), true),
        ]
    );
    let mut want: Vec<String> = out(&d, &["tag", "--points-at", "HEAD"])
        .lines()
        .map(String::from)
        .collect();
    want.sort();
    let mut got: Vec<String> = scan.at.iter().map(|t| t.0.clone()).collect();
    got.sort();
    assert_eq!(got, want);
    assert_eq!(best_tag(&scan.at), Some("v1.10"));
}

#[test]
fn loose_annotated_tag_is_unknown_and_git_job_tells() {
    let t = Tmp::new();
    let d = repo(&t, "p");
    git(&d, &["tag", "-a", "-m", "v", "v2"]);
    let r = open(&d);
    let head = r.head_commit().unwrap().unwrap();
    let scan = r.tags_at(&head).unwrap();
    assert!(scan.at.is_empty());
    assert!(scan.unknown);
    let job = hop_bar::cache::Job::GitTags {
        dir: d.to_str().unwrap().into(),
        commit: head.clone(),
    };
    let raw = job.execute(std::time::Duration::from_secs(10)).unwrap();
    let tags = parse_tags(&raw, &head).unwrap();
    assert_eq!(tags, vec![("v2".to_string(), true)]);
    assert_eq!(parse_tags(&raw, &"0".repeat(40)), None);
}

#[test]
fn loose_tag_overrides_packed_tag_of_same_name() {
    let t = Tmp::new();
    let d = repo(&t, "p");
    let first = out(&d, &["rev-parse", "HEAD"]);
    git(&d, &["tag", "moved"]);
    git(&d, &["commit", "-q", "--allow-empty", "-m", "y"]);
    git(&d, &["tag", "-a", "-m", "g", "gone"]);
    git(&d, &["pack-refs", "--all"]);
    // rewritten after packing: loose refs shadow the packed lines
    git(&d, &["tag", "-f", "moved"]);
    git(&d, &["tag", "-f", "gone", &first]);
    assert!(d.join(".git/refs/tags/moved").exists());
    assert!(d.join(".git/refs/tags/gone").exists());
    let r = open(&d);
    let head = r.head_commit().unwrap().unwrap();
    let scan = r.tags_at(&head).unwrap();
    assert_eq!(scan.at, vec![("moved".to_string(), false)]);
    assert_eq!(out(&d, &["tag", "--points-at", "HEAD"]), "moved");
    // the loose `gone` names another commit: only git can rule it out
    assert!(scan.unknown);
}

#[test]
fn best_tag_prefers_annotated_then_version() {
    let t = |n: &str, a| (n.to_string(), a);
    assert_eq!(best_tag(&[]), None);
    assert_eq!(best_tag(&[t("v9", false), t("v1", true)]), Some("v1"));
    assert_eq!(
        best_tag(&[t("v1.9", true), t("v1.10", true), t("v1.010", true)]),
        Some("v1.10")
    );
    assert_eq!(best_tag(&[t("a", false), t("b", false)]), Some("b"));
}

#[test]
fn stash_count_from_reflog() {
    let t = Tmp::new();
    let d = repo(&t, "p");
    commit_file(&d, "f", "1\n", "f");
    assert_eq!(open(&d).stash_count(), Ok(0));
    for i in 0..3 {
        std::fs::write(d.join("f"), format!("{}\n", i + 10)).unwrap();
        git(&d, &["stash", "-q"]);
    }
    let n = out(&d, &["stash", "list"]).lines().count();
    assert_eq!(n, 3);
    assert_eq!(open(&d).stash_count(), Ok(n));
    // shared with linked worktrees
    git(&d, &["worktree", "add", "-q", ".wt/x", "-b", "feat"]);
    assert_eq!(open(&d.join(".wt/x")).stash_count(), Ok(3));
    git(&d, &["stash", "drop", "-q"]);
    assert_eq!(open(&d).stash_count(), Ok(2));
    git(&d, &["stash", "clear"]);
    assert_eq!(open(&d).stash_count(), Ok(0));
}
