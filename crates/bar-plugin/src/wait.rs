//! Waiting for a child with a deadline, woken by its exit instead of a
//! polling interval: a sleep between `try_wait` calls makes every run
//! last at least one interval, and longer where sleeps overshoot (busy
//! or low-priority hosts).
//!
//! Unix: a helper thread blocks in `waitid(WNOWAIT)`, which reports the
//! exit without reaping, so the pid (and the group it leads) stays the
//! caller's until it reaps the child itself; a kill at the deadline can
//! never reach a reused pid. Windows: `WaitForSingleObject` on the
//! child's handle.
#![cfg_attr(any(unix, windows), allow(unsafe_code))] // waitid / WaitForSingleObject: std has no timed wait

use std::io;
use std::process::{Child, ExitStatus};
use std::time::{Duration, Instant};

/// Waits for `child` until `deadline`. `Ok(Some)`: it exited and is
/// reaped. `Ok(None)`: still running at the deadline and not reaped;
/// the caller kills it and reaps it with `wait`, which also ends the
/// helper thread.
pub fn wait_until(child: &mut Child, deadline: Instant) -> io::Result<Option<ExitStatus>> {
    if let Some(s) = child.try_wait()? {
        return Ok(Some(s));
    }
    imp::wait_until(child, deadline)
}

/// `try_wait` every 10 ms: where no blocking wait is available.
#[allow(dead_code)]
fn poll(child: &mut Child, deadline: Instant) -> io::Result<Option<ExitStatus>> {
    loop {
        match child.try_wait()? {
            Some(s) => return Ok(Some(s)),
            None if Instant::now() >= deadline => return Ok(None),
            None => std::thread::sleep(Duration::from_millis(10)),
        }
    }
}

#[cfg(unix)]
mod imp {
    use super::poll;
    use std::io;
    use std::process::{Child, ExitStatus};
    use std::sync::mpsc::{self, RecvTimeoutError};
    use std::time::Instant;

    pub fn wait_until(child: &mut Child, deadline: Instant) -> io::Result<Option<ExitStatus>> {
        let pid = libc::id_t::from(child.id());
        let (tx, rx) = mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("child-wait".into())
            .spawn(move || {
                let _ = tx.send(exited(pid));
            });
        if spawned.is_err() {
            return poll(child, deadline);
        }
        match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(Ok(())) => child.wait().map(Some),
            Ok(Err(e)) => child.try_wait()?.map(Some).ok_or(e),
            // one last look, as a poll at the deadline would
            Err(RecvTimeoutError::Timeout) => child.try_wait(),
            Err(RecvTimeoutError::Disconnected) => poll(child, deadline),
        }
    }

    /// Blocks until `pid` has exited, leaving it unreaped.
    fn exited(pid: libc::id_t) -> io::Result<()> {
        loop {
            // SAFETY: `info` is plain data for which all-zero is valid and
            // `waitid` writes only into it; WNOWAIT leaves the child, and
            // so its pid, to its owner.
            let r = unsafe {
                let mut info: libc::siginfo_t = std::mem::zeroed();
                libc::waitid(libc::P_PID, pid, &mut info, libc::WEXITED | libc::WNOWAIT)
            };
            if r == 0 {
                return Ok(());
            }
            let e = io::Error::last_os_error();
            if e.kind() != io::ErrorKind::Interrupted {
                return Err(e);
            }
        }
    }
}

#[cfg(windows)]
mod imp {
    use std::io;
    use std::os::windows::io::AsRawHandle;
    use std::process::{Child, ExitStatus};
    use std::time::Instant;
    use windows_sys::Win32::Foundation::{WAIT_OBJECT_0, WAIT_TIMEOUT};
    use windows_sys::Win32::System::Threading::WaitForSingleObject;

    pub fn wait_until(child: &mut Child, deadline: Instant) -> io::Result<Option<ExitStatus>> {
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            // below INFINITE; a longer limit is waited in turns
            let ms = u32::try_from(left.as_millis())
                .unwrap_or(u32::MAX - 1)
                .min(u32::MAX - 1);
            // SAFETY: the handle is the child's own, open while `child`
            // is borrowed.
            let r = unsafe { WaitForSingleObject(child.as_raw_handle(), ms) };
            match r {
                WAIT_OBJECT_0 => return child.wait().map(Some),
                WAIT_TIMEOUT if Instant::now() >= deadline => return child.try_wait(),
                WAIT_TIMEOUT => {}
                _ => return Err(io::Error::last_os_error()),
            }
        }
    }
}

#[cfg(not(any(unix, windows)))]
mod imp {
    use std::io;
    use std::process::{Child, ExitStatus};
    use std::time::Instant;

    pub fn wait_until(child: &mut Child, deadline: Instant) -> io::Result<Option<ExitStatus>> {
        super::poll(child, deadline)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::{Command, Stdio};

    fn shell(script: &str) -> Command {
        #[cfg(windows)]
        let mut c = {
            let mut c = Command::new("cmd");
            c.args(["/C", script]);
            c
        };
        #[cfg(not(windows))]
        let mut c = {
            let mut c = Command::new("/bin/sh");
            c.args(["-c", script]);
            c
        };
        c.stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        c
    }

    fn far() -> Instant {
        Instant::now() + Duration::from_secs(30)
    }

    #[test]
    fn exit_status_is_returned_and_reaped() {
        let mut c = shell("exit 3").spawn().unwrap();
        let st = wait_until(&mut c, far()).unwrap().unwrap();
        assert_eq!(st.code(), Some(3));
        // reaped: a second wait reports the same status at once
        assert_eq!(c.try_wait().unwrap().map(|s| s.code()), Some(Some(3)));
    }

    #[cfg(unix)]
    #[test]
    fn deadline_leaves_the_child_running_and_unreaped() {
        let mut c = Command::new("/bin/sleep").arg("5").spawn().unwrap();
        let start = Instant::now();
        let r = wait_until(&mut c, start + Duration::from_millis(100)).unwrap();
        assert!(r.is_none());
        assert!(start.elapsed() < Duration::from_secs(3));
        assert!(
            c.try_wait().unwrap().is_none(),
            "reaped or gone at the deadline"
        );
        c.kill().unwrap();
        // killing ends the wait, helper thread included
        assert!(c.wait().is_ok());
    }

    /// An exit is noticed when it happens, not at the next tick of a
    /// polling interval: the fastest wait for a trivial child is within a
    /// few ms of the fastest blocking `output()` for it. Minima, as noise
    /// only ever adds time; a 10 ms poll never returns before its first
    /// sleep, so it fails here on any host.
    #[test]
    fn a_fast_child_is_reaped_promptly() {
        let mut ours = Duration::MAX;
        let mut base = Duration::MAX;
        for _ in 0..15 {
            let t = Instant::now();
            shell("exit 0").output().unwrap();
            base = base.min(t.elapsed());
            let t = Instant::now();
            let mut c = shell("exit 0").spawn().unwrap();
            wait_until(&mut c, far()).unwrap().unwrap();
            ours = ours.min(t.elapsed());
        }
        assert!(
            ours < base + Duration::from_millis(5),
            "wait {ours:?} vs output() {base:?}"
        );
    }
}
