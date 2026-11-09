//! The test temp-dir helper: dirs removed on drop, stale ones swept,
//! a failing test's dir kept only when asked.
mod common;

use common::tmp::{self, Dir};
use std::time::{Duration, SystemTime};

#[test]
fn a_dir_is_removed_when_it_drops() {
    let d = Dir::new(&format!("tmp-drop-{}", std::process::id()));
    std::fs::write(d.path().join("f"), "x").unwrap();
    let p = d.path().to_path_buf();
    assert!(p.starts_with(tmp::root()));
    drop(d);
    assert!(!p.exists());
}

// a directory opened as a file to set its mtime: Unix only
#[cfg(unix)]
#[test]
fn sweep_removes_old_dirs_but_not_new_or_kept_ones() {
    let base = Dir::new(&format!("tmp-sweep-{}", std::process::id()));
    let (old, new, kept) = (
        base.path().join("old"),
        base.path().join("new"),
        base.path().join("kept"),
    );
    for d in [&old, &new, &kept] {
        std::fs::create_dir_all(d).unwrap();
    }
    std::fs::write(kept.join(".keep"), "").unwrap();
    // as seen from an hour later, `old` and `kept` are stale
    let later = SystemTime::now() + Duration::from_secs(3600);
    std::fs::File::open(&new)
        .unwrap()
        .set_modified(later)
        .unwrap();
    tmp::sweep(base.path(), later);
    assert!(!old.exists());
    assert!(new.exists());
    assert!(kept.exists());
}

#[test]
fn a_failing_tests_dir_is_kept_only_when_asked() {
    let run = |keep: Option<&str>| {
        let name = format!("tmp-fail-{}-{}", std::process::id(), keep.unwrap_or("-"));
        let p = tmp::root().join(&name);
        let mut c = std::process::Command::new(std::env::current_exe().unwrap());
        c.args(["--exact", "panics_holding_a_dir", "--include-ignored"])
            .env("BAR_TMP_NAME", &name)
            .env_remove("BAR_TEST_KEEP");
        if let Some(v) = keep {
            c.env("BAR_TEST_KEEP", v);
        }
        let out = c.output().unwrap();
        assert!(!out.status.success());
        let kept = p.join(".keep").exists();
        let _ = std::fs::remove_dir_all(&p);
        kept
    };
    assert!(!run(None));
    assert!(!run(Some("0")));
    assert!(run(Some("1")));
}

/// Run by the test above in a child process.
#[test]
#[ignore = "fails on purpose; run by a_failing_tests_dir_is_kept_only_when_asked"]
fn panics_holding_a_dir() {
    let _d = Dir::new(&std::env::var("BAR_TMP_NAME").unwrap());
    panic!("failing on purpose");
}
