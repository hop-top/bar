//! `bar tray start|stop|status`: the tray process. A running tray holds
//! an exclusive lock on `tray.lock` in the state dir and wrote its pid
//! there (`bar-tray --lock`); whether the lock is held says whether it
//! runs, whatever the file holds. With the LaunchAgent installed, start
//! and stop go through `launchctl`, so launchd does not restart a tray
//! stopped on purpose.

use super::agent;
use serde_json::{json, Value};
use std::fs::{File, TryLockError};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// The single-instance lock a running tray holds.
pub const LOCK_FILE: &str = "tray.lock";

/// Where things are: the state dir, the LaunchAgents dir (macOS) and
/// the user id launchd's GUI domain is named after.
#[derive(Debug, Clone)]
pub struct Layout {
    pub state_dir: PathBuf,
    pub agents: Option<PathBuf>,
    pub uid: Option<u32>,
}

impl Layout {
    /// This environment's: `click::state_dir()`, `~/Library/LaunchAgents`
    /// on macOS (none without an absolute home), the effective user id.
    pub fn from_env() -> Layout {
        let home = crate::platform::home();
        Layout {
            state_dir: crate::click::state_dir(),
            agents: if cfg!(target_os = "macos") {
                agent::agents_dir(home.as_deref())
            } else {
                None
            },
            uid: crate::platform::current_uid(),
        }
    }

    pub fn lock(&self) -> PathBuf {
        self.state_dir.join(LOCK_FILE)
    }

    /// The LaunchAgent plist path, on macOS.
    pub fn plist(&self) -> Option<PathBuf> {
        self.agents.as_deref().map(agent::plist_path)
    }

    /// The installed plist and launchd's user id, when both exist.
    fn agent(&self) -> Option<(PathBuf, u32)> {
        let p = self.plist().filter(|p| p.is_file())?;
        Some((p, self.uid?))
    }
}

/// What starting and stopping need from the machine; tests record calls.
pub trait Sys {
    /// Runs `cmd` to the end; fails on a spawn error or non-zero exit.
    fn exec(&mut self, cmd: &[String]) -> Result<(), String>;
    /// Starts `cmd` in the background, detached from this terminal.
    fn spawn(&mut self, cmd: &[String]) -> Result<u32, String>;
}

/// The real machine.
pub struct System;

impl Sys for System {
    fn exec(&mut self, cmd: &[String]) -> Result<(), String> {
        crate::click::exec(cmd)
    }

    fn spawn(&mut self, cmd: &[String]) -> Result<u32, String> {
        use std::process::{Command, Stdio};
        let mut c = Command::new(&cmd[0]);
        c.args(&cmd[1..])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        // its own process group: a Ctrl-C in this terminal does not reach it
        #[cfg(unix)]
        std::os::unix::process::CommandExt::process_group(&mut c, 0);
        c.spawn().map(|ch| ch.id()).map_err(|e| e.to_string())
    }
}

/// Whether a tray holds the lock at `lock`, and its pid when readable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Running {
    No,
    Yes(Option<u32>),
}

/// Probes the lock without creating the file: a free lock is taken and
/// let go at once.
pub fn running(lock: &Path) -> Running {
    let Ok(f) = File::open(lock) else {
        return Running::No;
    };
    match f.try_lock_shared() {
        Ok(()) => Running::No,
        Err(TryLockError::WouldBlock) => Running::Yes(
            std::fs::read_to_string(lock)
                .ok()
                .and_then(|s| s.lines().next()?.trim().parse().ok()),
        ),
        // a lock that cannot be probed counts as free
        Err(TryLockError::Error(_)) => Running::No,
    }
}

fn kill(pid: u32) -> Vec<String> {
    if cfg!(windows) {
        vec!["taskkill".into(), "/PID".into(), pid.to_string()]
    } else {
        vec!["/bin/kill".into(), "-TERM".into(), pid.to_string()]
    }
}

/// Starts the tray unless one runs: through launchd when the LaunchAgent
/// is installed (load it, or start it again when loaded), else `argv`
/// in the background.
pub fn start(
    l: &Layout,
    argv: &[String],
    sys: &mut dyn Sys,
    out: &mut dyn Write,
) -> Result<(), String> {
    let io = |e: std::io::Error| e.to_string();
    if let Running::Yes(pid) = running(&l.lock()) {
        return writeln!(out, "already running{}", shown(pid)).map_err(io);
    }
    if let Some((plist, uid)) = l.agent() {
        sys.exec(&agent::bootstrap(uid, &plist))
            .or_else(|_| sys.exec(&agent::kickstart(uid)))
            .map_err(|e| format!("launchctl: {e}"))?;
        return writeln!(out, "started {}", agent::LABEL).map_err(io);
    }
    let pid = sys.spawn(argv).map_err(|e| format!("{}: {e}", argv[0]))?;
    writeln!(out, "started pid {pid}").map_err(io)
}

fn shown(pid: Option<u32>) -> String {
    pid.map_or(String::new(), |p| format!(" (pid {p})"))
}

/// Stops the tray: unloads the LaunchAgent when installed (it loads
/// again at the next login), then signals a tray still holding the lock.
/// `Ok(false)` when nothing ran.
pub fn stop(l: &Layout, sys: &mut dyn Sys, out: &mut dyn Write) -> Result<bool, String> {
    let io = |e: std::io::Error| e.to_string();
    let mut stopped = false;
    if let Some((_, uid)) = l.agent() {
        // fails when it is not loaded, which is fine
        if sys.exec(&agent::bootout(uid)).is_ok() {
            writeln!(out, "unloaded {}", agent::LABEL).map_err(io)?;
            stopped = true;
        }
    }
    match running(&l.lock()) {
        Running::No => {}
        Running::Yes(None) => return Err("a tray holds the lock but its pid is unknown".into()),
        Running::Yes(Some(pid)) => {
            sys.exec(&kill(pid))
                .map_err(|e| format!("stop pid {pid}: {e}"))?;
            writeln!(out, "stopped pid {pid}").map_err(io)?;
            stopped = true;
        }
    }
    if !stopped {
        writeln!(out, "not running").map_err(io)?;
    }
    Ok(stopped)
}

/// What `bar tray status` reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    pub installed: bool,
    pub state_dir: PathBuf,
    /// The LaunchAgent plist, when installed.
    pub autostart: Option<PathBuf>,
    pub running: Running,
    /// Seconds since the snapshot was written; `None` without one or
    /// once spent by a run.
    pub snapshot_age: Option<u64>,
}

pub fn status(l: &Layout, now: SystemTime) -> Status {
    let snap = l.state_dir.join(super::SNAPSHOT_FILE);
    let snapshot_age = std::fs::metadata(snap)
        .and_then(|m| m.modified())
        .ok()
        .filter(|t| *t > SystemTime::UNIX_EPOCH)
        .map(|t| now.duration_since(t).map_or(0, |d| d.as_secs()));
    Status {
        installed: l
            .state_dir
            .join(crate::click::pointer::TRAY_MARKER)
            .is_file(),
        state_dir: l.state_dir.clone(),
        autostart: l.plist().filter(|p| p.is_file()),
        running: running(&l.lock()),
        snapshot_age,
    }
}

impl Status {
    pub fn to_json(&self) -> Value {
        let (running, pid) = match self.running {
            Running::No => (false, None),
            Running::Yes(p) => (true, p),
        };
        json!({
            "installed": self.installed,
            "state_dir": self.state_dir.display().to_string(),
            "autostart": self.autostart.as_ref().map(|p| p.display().to_string()),
            "running": running,
            "pid": pid,
            "snapshot_age": self.snapshot_age,
        })
    }

    pub fn lines(&self) -> Vec<String> {
        let yes = |b: bool| if b { "yes" } else { "no" };
        vec![
            format!("installed  {}", yes(self.installed)),
            format!("state dir  {}", self.state_dir.display()),
            format!(
                "autostart  {}",
                self.autostart
                    .as_ref()
                    .map_or("off".into(), |p| p.display().to_string())
            ),
            format!(
                "running    {}",
                match self.running {
                    Running::No => "no".to_string(),
                    Running::Yes(p) => format!("yes{}", shown(p)),
                }
            ),
            format!(
                "menu       {}",
                self.snapshot_age
                    .map_or("none".into(), |s| format!("written {s} s ago"))
            ),
        ]
    }
}
