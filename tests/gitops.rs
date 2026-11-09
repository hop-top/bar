// Unix fixtures (POSIX paths, git CLI); Windows coverage: tests/windows.rs
#![cfg(unix)]

#[path = "common/git_fx.rs"]
mod git_fx;
use git_fx::*;

use hop_bar::gitfs::{unmerged, Op, Unsupported};

#[test]
fn clean_repo_has_no_operation_or_conflicts() {
    let t = Tmp::new();
    let d = repo(&t, "p");
    let r = open(&d);
    assert_eq!(r.operation(), Ok(None));
    assert_eq!(r.conflicts(), Ok(0));
}

#[test]
fn conflicted_merge() {
    let t = Tmp::new();
    let d = diverged(&t, &["a", "dir/b", "dir/c"], 0);
    commit_file(&d, "clean", "x\n", "unrelated");
    git_fails(&d, &["merge", "-q", "feat"]);
    let r = open(&d);
    assert_eq!(r.operation(), Ok(Some((Op::Merge, None))));
    assert_eq!(git_unmerged(&d), 3);
    assert_eq!(r.conflicts(), Ok(3));
    // resolving one path drops it from the count
    git(&d, &["add", "a"]);
    assert_eq!(open(&d).conflicts(), Ok(2));
}

#[test]
fn mid_rebase_with_step() {
    let t = Tmp::new();
    let d = diverged(&t, &["a"], 2);
    git(&d, &["checkout", "-q", "feat"]);
    git_fails(&d, &["rebase", "main"]);
    assert!(d.join(".git/rebase-merge").is_dir());
    let r = open(&d);
    assert_eq!(r.operation(), Ok(Some((Op::Rebase, Some((1, 3))))));
    assert_eq!(r.conflicts(), Ok(git_unmerged(&d)));
    assert_eq!(r.conflicts(), Ok(1));
}

#[test]
fn apply_rebase_and_am() {
    let t = Tmp::new();
    let d = diverged(&t, &["a"], 1);
    git(&d, &["checkout", "-q", "feat"]);
    git_fails(&d, &["rebase", "--apply", "main"]);
    assert_eq!(open(&d).operation(), Ok(Some((Op::Rebase, Some((1, 2))))));
    git(&d, &["rebase", "--abort"]);

    let patches = t.path().join("patches");
    git(
        &d,
        &[
            "format-patch",
            "-q",
            "-o",
            patches.to_str().unwrap(),
            "main..feat",
        ],
    );
    git(&d, &["checkout", "-q", "main"]);
    let mut files: Vec<String> = std::fs::read_dir(&patches)
        .unwrap()
        .map(|e| e.unwrap().path().to_str().unwrap().to_string())
        .collect();
    files.sort();
    let mut args = vec!["am", "-q"];
    args.extend(files.iter().map(String::as_str));
    git_fails(&d, &args);
    assert_eq!(open(&d).operation(), Ok(Some((Op::Am, Some((1, 2))))));
}

#[test]
fn cherry_pick_revert_and_bisect() {
    let t = Tmp::new();
    let d = diverged(&t, &["a"], 0);
    git_fails(&d, &["cherry-pick", "feat"]);
    assert_eq!(open(&d).operation(), Ok(Some((Op::CherryPick, None))));
    assert_eq!(open(&d).conflicts(), Ok(1));
    git(&d, &["cherry-pick", "--abort"]);

    // reverting the base commit of `a` under main's change conflicts
    let base = out(&d, &["rev-parse", "main~1"]);
    git_fails(&d, &["revert", "--no-edit", &base]);
    assert_eq!(open(&d).operation(), Ok(Some((Op::Revert, None))));
    git(&d, &["revert", "--abort"]);

    git(&d, &["bisect", "start"]);
    assert_eq!(open(&d).operation(), Ok(Some((Op::Bisect, None))));
    git(&d, &["bisect", "reset"]);
    assert_eq!(open(&d).operation(), Ok(None));
}

#[test]
fn operation_is_per_worktree() {
    let t = Tmp::new();
    let d = diverged(&t, &["a"], 0);
    git(&d, &["worktree", "add", "-q", ".wt/x", "-b", "other"]);
    git_fails(&d, &["merge", "-q", "feat"]);
    assert_eq!(open(&d).operation(), Ok(Some((Op::Merge, None))));
    let wt = open(&d.join(".wt/x"));
    assert_eq!(wt.operation(), Ok(None));
    assert_eq!(wt.conflicts(), Ok(0));
}

#[test]
fn index_v3_and_v4() {
    let t = Tmp::new();
    // shared prefixes exercise v4 path compression
    let files = ["dir/a1", "dir/a2", "dir/sub/b", "dir/sub/bb", "z"];
    let d = diverged(&t, &files, 0);
    for i in 0..20 {
        commit_file(&d, &format!("dir/sub/clean{i:02}"), "x\n", "c");
    }
    git_fails(&d, &["merge", "-q", "feat"]);
    // intent-to-add sets the extended flag, which needs version 3
    std::fs::write(d.join("dir/new"), "n\n").unwrap();
    git(&d, &["add", "-N", "dir/new"]);
    let want = git_unmerged(&d);
    assert_eq!(want, files.len());
    let version = |d: &std::path::Path| {
        let b = std::fs::read(d.join(".git/index")).unwrap();
        u32::from_be_bytes(b[4..8].try_into().unwrap())
    };
    assert_eq!(version(&d), 3);
    assert_eq!(open(&d).conflicts(), Ok(want));
    git(&d, &["update-index", "--index-version", "4"]);
    assert_eq!(version(&d), 4);
    assert_eq!(git_unmerged(&d), want);
    assert_eq!(open(&d).conflicts(), Ok(want));
    git(&d, &["add", "dir/sub/b"]);
    assert_eq!(version(&d), 4);
    assert_eq!(open(&d).conflicts(), Ok(want - 1));
}

#[test]
fn split_index_and_env_are_unsupported() {
    let t = Tmp::new();
    let d = repo(&t, "p");
    commit_file(&d, "f", "1\n", "f");
    git(&d, &["update-index", "--split-index"]);
    assert_eq!(open(&d).conflicts(), Err(Unsupported));
}

#[test]
fn unmerged_rejects_malformed_input() {
    assert_eq!(unmerged(b"", 20), None);
    assert_eq!(unmerged(b"XXXX\0\0\0\x02\0\0\0\0", 20), None);
    // version 5 is unknown
    assert_eq!(unmerged(b"DIRC\0\0\0\x05\0\0\0\0", 20), None);
    // claims an entry it does not hold
    assert_eq!(unmerged(b"DIRC\0\0\0\x02\0\0\0\x01", 20), None);
    // empty v2 index with its checksum
    let mut empty = b"DIRC\0\0\0\x02\0\0\0\0".to_vec();
    empty.extend([0u8; 20]);
    assert_eq!(unmerged(&empty, 20), Some(0));
}

#[test]
fn unmerged_handles_real_index_truncated_anywhere() {
    let t = Tmp::new();
    let d = diverged(&t, &["a", "b"], 0);
    git_fails(&d, &["merge", "-q", "feat"]);
    let data = std::fs::read(d.join(".git/index")).unwrap();
    assert_eq!(unmerged(&data, 20), Some(2));
    // no cut panics; cuts into the entries are refused
    for n in 0..data.len() {
        let _ = unmerged(&data[..n], 20);
    }
    assert_eq!(unmerged(&data[..40], 20), None);
}
