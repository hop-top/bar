// Unix fixtures (POSIX paths, git CLI); Windows coverage: tests/windows.rs
#![cfg(unix)]

mod common;
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

#[test]
fn reftable_refs_are_unsupported() {
    let t = Tmp::new();
    let d = repo(&t, "p");
    let head = out(&d, &["rev-parse", "HEAD"]);
    // a `reftable/` directory in the common dir
    std::fs::create_dir(d.join(".git/reftable")).unwrap();
    let r = open(&d);
    assert_eq!(r.resolve("refs/stash"), Err(Unsupported));
    assert_eq!(r.stash_count(), Err(Unsupported));
    assert_eq!(r.tags_at(&head), Err(Unsupported));
    std::fs::remove_dir(d.join(".git/reftable")).unwrap();
    assert_eq!(open(&d).stash_count(), Ok(0));
    // or the config extension alone
    let cfg = d.join(".git/config");
    let mut body = std::fs::read_to_string(&cfg).unwrap();
    body.push_str("[extensions]\n\trefStorage = reftable\n");
    std::fs::write(&cfg, body).unwrap();
    let r = open(&d);
    assert_eq!(r.stash_count(), Err(Unsupported));
    assert_eq!(r.tags_at(&head), Err(Unsupported));
}

#[test]
fn real_reftable_repository_is_unsupported() {
    let t = Tmp::new();
    let d = t.path().join("rt");
    std::fs::create_dir_all(&d).unwrap();
    if !cmd(&d, &["init", "-q", "--ref-format=reftable"])
        .status()
        .unwrap()
        .success()
    {
        return; // git older than 2.45
    }
    git(&d, &["commit", "-q", "--allow-empty", "-m", "x"]);
    git(&d, &["tag", "v1"]);
    std::fs::write(d.join("f"), "x\n").unwrap();
    git(&d, &["add", "f"]);
    git(&d, &["stash", "-q"]);
    let r = open(&d);
    assert_eq!(r.head_commit(), Err(Unsupported));
    assert_eq!(r.stash_count(), Err(Unsupported));
    assert_eq!(r.tags_at(&"0".repeat(40)), Err(Unsupported));
}

#[test]
fn more_than_256_loose_tags_go_to_git() {
    let t = Tmp::new();
    let d = repo(&t, "p");
    let head = out(&d, &["rev-parse", "HEAD"]);
    let tags = d.join(".git/refs/tags");
    std::fs::create_dir_all(&tags).unwrap();
    for i in 0..256 {
        std::fs::write(tags.join(format!("t{i:03}")), format!("{head}\n")).unwrap();
    }
    let scan = open(&d).tags_at(&head).unwrap();
    assert_eq!(scan.at.len(), 256);
    // one more, in a subdirectory
    std::fs::create_dir(tags.join("rel")).unwrap();
    std::fs::write(tags.join("rel/x"), format!("{head}\n")).unwrap();
    assert_eq!(open(&d).tags_at(&head), Err(Unsupported));
}

#[test]
fn packed_refs_with_thousands_of_tags() {
    let t = Tmp::new();
    let d = repo(&t, "p");
    let head = out(&d, &["rev-parse", "HEAD"]);
    git(&d, &["tag", "-a", "-m", "r", "v2"]);
    git(&d, &["pack-refs", "--all"]);
    let packed = d.join(".git/packed-refs");
    let mut body = std::fs::read_to_string(&packed).unwrap();
    for i in 0..15_000 {
        body.push_str(&format!("{} refs/tags/p{i:05}\n", "1".repeat(40)));
    }
    std::fs::write(&packed, body).unwrap();
    let r = open(&d);
    assert_eq!(r.head_commit(), Ok(Some(head.clone())));
    assert_eq!(r.resolve("refs/tags/p14999"), Ok(Some("1".repeat(40))));
    assert_eq!(r.resolve("refs/tags/nope"), Ok(None));
    let scan = r.tags_at(&head).unwrap();
    assert_eq!(scan.at, vec![("v2".to_string(), true)]);
    assert_eq!(r.stash_count(), Ok(0));
}
