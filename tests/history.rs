use hop_bar::history::{sample, CAP};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

static N: AtomicUsize = AtomicUsize::new(0);

struct Tmp(PathBuf);
impl Tmp {
    fn new() -> Tmp {
        let p = std::env::temp_dir().join(format!(
            "bar-hist-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&p).unwrap();
        Tmp(p)
    }
}
impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const R5: i64 = 2_000_000;
const STEP5: i64 = 600;
const STEP7: i64 = 6 * 3600;

#[test]
fn first_sample_is_kept_and_stored_private() {
    let t = Tmp::new();
    assert_eq!(sample(&t.0, "5h", R5, 1000, 12.0), vec![12.0]);
    let file = std::fs::read_dir(&t.0)
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.to_string_lossy().contains("5h"))
        .expect("history file");
    assert!(file.is_file());
    #[cfg(unix)]
    {
        let mode = std::os::unix::fs::PermissionsExt::mode(
            &std::fs::metadata(file).unwrap().permissions(),
        );
        assert_eq!(mode & 0o777, 0o600);
    }
}

#[test]
fn appends_only_after_the_step() {
    let t = Tmp::new();
    sample(&t.0, "5h", R5, 1000, 10.0);
    assert_eq!(sample(&t.0, "5h", R5, 1000 + STEP5 - 1, 11.0), vec![10.0]);
    assert_eq!(sample(&t.0, "5h", R5, 1000 + STEP5, 12.0), vec![10.0, 12.0]);
    // the step counts from the last stored sample
    assert_eq!(
        sample(&t.0, "5h", R5, 1000 + STEP5 + 100, 13.0),
        vec![10.0, 12.0]
    );
}

#[test]
fn seven_day_step_is_six_hours() {
    let t = Tmp::new();
    sample(&t.0, "7d", R5, 0, 1.0);
    assert_eq!(sample(&t.0, "7d", R5, STEP7 - 1, 2.0), vec![1.0]);
    assert_eq!(sample(&t.0, "7d", R5, STEP7, 3.0), vec![1.0, 3.0]);
}

#[test]
fn windows_are_independent() {
    let t = Tmp::new();
    sample(&t.0, "5h", R5, 0, 1.0);
    assert_eq!(sample(&t.0, "7d", R5, 1, 9.0), vec![9.0]);
}

#[test]
fn ring_buffer_is_capped_oldest_dropped() {
    let t = Tmp::new();
    let mut last = vec![];
    for i in 0..(CAP as i64 + 10) {
        last = sample(&t.0, "5h", R5, i * STEP5, i as f64);
    }
    assert_eq!(last.len(), CAP);
    assert_eq!(*last.last().unwrap(), (CAP + 9) as f64);
    assert_eq!(last[0], 10.0);
}

#[test]
fn reset_starts_a_new_series() {
    let t = Tmp::new();
    sample(&t.0, "5h", R5, 0, 50.0);
    sample(&t.0, "5h", R5, STEP5, 60.0);
    assert_eq!(sample(&t.0, "5h", R5 + 18000, STEP5 + 1, 2.0), vec![2.0]);
}

#[test]
fn clock_going_backwards_restarts() {
    let t = Tmp::new();
    sample(&t.0, "5h", R5, 5000, 50.0);
    assert_eq!(sample(&t.0, "5h", R5, 100, 7.0), vec![7.0]);
}

#[test]
fn garbage_file_is_ignored() {
    let t = Tmp::new();
    sample(&t.0, "5h", R5, 0, 1.0);
    for e in std::fs::read_dir(&t.0).unwrap() {
        std::fs::write(e.unwrap().path(), "not\nnumbers here\n\u{0}").unwrap();
    }
    assert_eq!(sample(&t.0, "5h", R5, 10, 4.0), vec![4.0]);
}

#[test]
fn unwritable_state_still_returns_the_sample() {
    let t = Tmp::new();
    let blocker = t.0.join("file");
    std::fs::write(&blocker, "").unwrap();
    assert_eq!(sample(&blocker.join("sub"), "5h", R5, 0, 5.0), vec![5.0]);
}
