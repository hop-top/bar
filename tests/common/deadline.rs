//! Bounded child runs: a plugin that spins or hangs fails its test within
//! a deadline instead of hanging the suite. Shared by the plugin crates'
//! tests too (`#[path]`), so it names no `crate::` item.
#![allow(dead_code)]

use std::io::{Read, Write};
use std::process::{Command, ExitStatus, Output, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

/// Default bound for one plugin run. A run takes milliseconds; the longest
/// limit a plugin declares is 15 s, and a loaded CI host delays a spawn by
/// seconds. Four times that limit fails a stuck plugin in a minute without
/// tripping on a slow but working one.
pub const DEADLINE: Duration = Duration::from_secs(60);

/// [`run_within`] with the default [`DEADLINE`].
pub fn run(cmd: &mut Command, stdin: Option<&[u8]>) -> Output {
    run_within(cmd, stdin, DEADLINE)
}

/// Runs `cmd` to completion as `Command::output` does, `stdin` written to
/// it (`None`: null). Past `limit`, or when its output is still held open
/// by something it started, its whole process tree is killed and the test
/// fails: a looping subshell outlives its parent and keeps stdout open, so
/// killing the plugin alone would leave the read hanging.
pub fn run_within(cmd: &mut Command, stdin: Option<&[u8]>, limit: Duration) -> Output {
    let deadline = Instant::now() + limit;
    hop_bar::platform::own_group(cmd)
        .stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = spawn(cmd);
    if let (Some(bytes), Some(mut pipe)) = (stdin, child.stdin.take()) {
        // a plugin that never reads must not block the test; EPIPE is ignored
        let bytes = bytes.to_vec();
        std::thread::spawn(move || {
            let _ = pipe.write_all(&bytes);
        });
    }
    let out = drain(child.stdout.take());
    let err = drain(child.stderr.take());
    let status = bar_plugin::wait::wait_until(&mut child, deadline).unwrap();
    let left = || deadline.saturating_duration_since(Instant::now());
    let stdout = status.and_then(|_| out.recv_timeout(left()).ok());
    let stderr = stdout.as_ref().and_then(|_| err.recv_timeout(left()).ok());
    if let (Some(status), Some(stdout), Some(stderr)) = (status, stdout, stderr) {
        return Output {
            status,
            stdout,
            stderr,
        };
    }
    hop_bar::platform::kill_tree(&mut child);
    let _ = child.wait();
    stuck(cmd, limit, status, &err)
}

fn stuck(cmd: &Command, limit: Duration, status: Option<ExitStatus>, err: &Receiver<Vec<u8>>) -> ! {
    let stderr = err.recv_timeout(Duration::from_secs(5)).unwrap_or_default();
    let what = match status {
        None => "still running".to_string(),
        Some(s) => format!("exited ({s}) but its output still open"),
    };
    panic!(
        "{:?} {what} after {limit:?}; process tree killed; stderr {:?}",
        cmd.get_program(),
        String::from_utf8_lossy(&stderr)
    )
}

/// Spawns, retrying while the binary is "text file busy": a test thread
/// that just copied a plugin can leak its write handle into another
/// thread's fork until that child execs, so the exec fails for a moment.
fn spawn(cmd: &mut Command) -> std::process::Child {
    for _ in 0..50 {
        match cmd.spawn() {
            Err(e) if e.kind() == std::io::ErrorKind::ExecutableFileBusy => {
                std::thread::sleep(Duration::from_millis(20));
            }
            r => return r.unwrap(),
        }
    }
    cmd.spawn().unwrap()
}

/// Reads `pipe` to its end on a thread, so a chatty child never blocks on
/// a full pipe; the bytes arrive on the channel.
fn drain(pipe: Option<impl Read + Send + 'static>) -> Receiver<Vec<u8>> {
    let (tx, rx) = mpsc::channel();
    if let Some(mut pipe) = pipe {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = pipe.read_to_end(&mut buf);
            let _ = tx.send(buf);
        });
    }
    rx
}
