//! How bar runs a command it waits for (`%command%` refreshes, plugin
//! and action calls share the runner): prompt end, time limit, cap.

use hop_bar::cache::Job;
use std::time::{Duration, Instant};

fn job(cmd: &str, timeout: u64) -> Job {
    Job::Command {
        cmd: cmd.to_string(),
        timeout,
    }
}

/// The runner returns when the command exits, not at the next tick of a
/// polling interval: its fastest run of a trivial command is within a
/// few ms of the fastest blocking `output()` of the same command. Minima,
/// as host noise only ever adds time; a 10 ms poll never returns before
/// its first sleep.
#[test]
fn a_fast_command_ends_the_wait_promptly() {
    let j = job("echo ok", 30);
    let mut ours = Duration::MAX;
    let mut base = Duration::MAX;
    for _ in 0..15 {
        let t = Instant::now();
        let out = hop_bar::platform::shell("echo ok").output().unwrap();
        base = base.min(t.elapsed());
        assert!(out.status.success());
        let t = Instant::now();
        let got = j.execute(Duration::from_secs(30));
        ours = ours.min(t.elapsed());
        assert_eq!(got.as_deref().map(str::trim), Some("ok"));
    }
    assert!(
        ours < base + Duration::from_millis(5),
        "runner {ours:?} vs output() {base:?}"
    );
}

#[test]
fn a_failing_command_yields_nothing() {
    assert_eq!(job("exit 3", 30).execute(Duration::from_secs(30)), None);
}

#[cfg(unix)]
#[test]
fn the_limit_kills_the_command_and_its_children() {
    let start = Instant::now();
    // a grandchild holding stdout must not keep the runner waiting
    let got = job("sleep 7 & sleep 7; echo late", 1).execute(Duration::from_secs(30));
    assert_eq!(got, None);
    let took = start.elapsed();
    assert!(took >= Duration::from_secs(1), "took {took:?}");
    assert!(took < Duration::from_secs(5), "took {took:?}");
}

#[cfg(unix)]
#[test]
fn output_is_capped() {
    let got = job("head -c 100000 /dev/zero | tr '\\0' a", 30)
        .execute(Duration::from_secs(30))
        .unwrap();
    assert_eq!(got.len(), hop_bar::cache::MAX_VALUE);
    assert!(got.bytes().all(|b| b == b'a'));
}
