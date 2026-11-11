//! The tray's wait for a `bar` child ends when the child exits, not at
//! the next tick of a polling interval. Own test binary: no other test
//! spawning children runs beside it.
#![cfg(unix)]

use bar_tray::backend::spawn::run;
use std::ffi::OsString;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

/// The fastest run of a trivial child is within a few ms of the fastest
/// blocking `output()` of it. Minima, as host noise only ever adds time;
/// a 20 ms poll never returns before its first sleep.
#[test]
fn a_fast_child_ends_the_wait_promptly() {
    let args: Vec<OsString> = vec!["-c".into(), "exit 0".into()];
    let (mut ours, mut base) = (Duration::MAX, Duration::MAX);
    for _ in 0..15 {
        let t = Instant::now();
        Command::new("/bin/sh").args(&args).output().unwrap();
        base = base.min(t.elapsed());
        let t = Instant::now();
        run(Path::new("/bin/sh"), &args, true, Duration::from_secs(30)).unwrap();
        ours = ours.min(t.elapsed());
    }
    assert!(
        ours < base + Duration::from_millis(5),
        "run {ours:?} vs output() {base:?}"
    );
}
