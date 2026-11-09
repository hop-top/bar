// Unix fixtures (POSIX paths, git CLI); Windows coverage: tests/windows.rs
#![cfg(unix)]

mod common;
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

/// Version 4 index: `(path, stage)` entries with prefix-compressed names,
/// no extensions, zeroed checksum.
fn index_v4(entries: &[(&str, u16)]) -> Vec<u8> {
    let mut d = b"DIRC\0\0\0\x04".to_vec();
    d.extend((entries.len() as u32).to_be_bytes());
    let mut prev = "";
    for (path, stage) in entries {
        d.extend([0u8; 40 + 20]);
        let flags = (stage << 12) | (path.len().min(0xfff) as u16);
        d.extend(flags.to_be_bytes());
        let common = prev
            .bytes()
            .zip(path.bytes())
            .take_while(|(a, b)| a == b)
            .count();
        let strip = prev.len() - common;
        assert!(strip < 0x80);
        d.push(strip as u8);
        d.extend(&path.as_bytes()[common..]);
        d.push(0);
        prev = path;
    }
    d.extend([0u8; 20]);
    d
}

#[test]
fn v4_reconstructed_paths_drive_conflict_dedup() {
    // stages of one path share it whole (strip 0, empty suffix); a
    // neighbour extends it; a later path strips back
    let idx = index_v4(&[
        ("dir/a", 1),
        ("dir/a", 2),
        ("dir/a", 3),
        ("dir/ab", 2),
        ("dir/ab", 3),
        ("dir/b", 0),
        ("dir/c", 1),
        ("e", 0),
    ]);
    assert_eq!(unmerged(&idx, 20), Some(3));
    // same entries, none conflicted past the first path
    let idx = index_v4(&[("dir/a", 2), ("dir/a", 3), ("dir/ab", 0), ("dir/b", 0)]);
    assert_eq!(unmerged(&idx, 20), Some(1));
    // a strip past the previous path's length is refused
    let mut bad = index_v4(&[("ab", 1)]);
    let at = 12 + 40 + 20 + 2;
    bad[at] = 3;
    assert_eq!(unmerged(&bad, 20), None);
}

#[test]
fn extension_overrunning_the_index_is_refused() {
    let ext = |size: u32, body: usize| {
        let mut d = b"DIRC\0\0\0\x02\0\0\0\0".to_vec();
        d.extend(b"TREE");
        d.extend(size.to_be_bytes());
        d.extend(vec![0u8; body]);
        d.extend([0u8; 20]);
        d
    };
    assert_eq!(unmerged(&ext(4, 4), 20), Some(0));
    // a size near u32::MAX must not wrap the offset, on any target
    assert_eq!(unmerged(&ext(0xFFFF_FFF8, 4), 20), None);
    assert_eq!(unmerged(&ext(u32::MAX, 4), 20), None);
    // one byte past the end, into the checksum
    assert_eq!(unmerged(&ext(5, 4), 20), None);
}

#[test]
fn sequencer_todo_read_as_git_prompt_reads_it() {
    let t = Tmp::new();
    let d = repo(&t, "p");
    let op = |todo: &str| {
        std::fs::create_dir_all(d.join(".git/sequencer")).unwrap();
        std::fs::write(d.join(".git/sequencer/todo"), todo).unwrap();
        open(&d).operation().unwrap().map(|(op, _)| op)
    };
    assert_eq!(op("pick 0123abc subject\n"), Some(Op::CherryPick));
    assert_eq!(op("pick\t0123abc\n"), Some(Op::CherryPick));
    assert_eq!(op("revert 0123abc subject\n"), Some(Op::Revert));
    assert_eq!(op("revert\t0123abc\n"), Some(Op::Revert));
    // `p[ \t]` in git-prompt.sh: the bare verb and one blank only
    assert_eq!(op("p \n"), Some(Op::CherryPick));
    assert_eq!(op("p 0123abc subject\n"), None);
    // no whitespace trimmed before the verb
    assert_eq!(op(" pick 0123abc\n"), None);
    assert_eq!(op("pick\n"), None);
    assert_eq!(op("picked 0123abc\n"), None);
    assert_eq!(op("reverted x\n"), None);
    assert_eq!(op("pick x\r\n"), Some(Op::CherryPick));
}
