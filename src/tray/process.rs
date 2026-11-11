//! `bar tray start|stop|status`: the tray process. A running tray holds
//! an exclusive lock on `tray.lock` in the state dir and wrote its pid
//! there (`bar-tray --lock`); whether the lock is held says whether it
//! runs, whatever the file holds. With the LaunchAgent installed, start
//! and stop go through `launchctl`, so launchd does not restart a tray
//! stopped on purpose; with the systemd user unit, through `systemctl`.
//! Windows keeps a locked file unreadable to other processes, so the
//! tray also writes its pid to `tray.pid`.

use super::agent;
use super::xdg::{self, UNIT};
use serde_json::{json, Value};
use std::fs::{File, TryLockError};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// The single-instance lock a running tray holds.
pub const LOCK_FILE: &str = "tray.lock";
/// The running tray's pid, beside the lock (`bar-tray --lock`).
pub const PID_FILE: &str = "tray.pid";

/// Where things are: the state dir, the LaunchAgents dir (macOS), the
/// user id launchd's GUI domain is named after, the XDG dirs (Linux)
/// and, on Windows, the system dir whose `reg.exe` and `taskkill.exe`
/// autostart and stop use.
#[derive(Debug, Clone)]
pub struct Layout {
    pub state_dir: PathBuf,
    pub agents: Option<PathBuf>,
    pub uid: Option<u32>,
    pub xdg: Option<xdg::Dirs>,
    pub system_dir: Option<PathBuf>,
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
            xdg: if cfg!(target_os = "linux") {
                xdg::Dirs::from_env(home.as_deref(), &|k| std::env::var(k).ok())
            } else {
                None
            },
            system_dir: system_dir(),
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

    /// The installed systemd user unit (Linux).
    fn unit(&self) -> Option<PathBuf> {
        self.xdg
            .as_ref()
            .map(xdg::Dirs::unit)
            .filter(|p| p.is_file())
    }
}

#[cfg(windows)]
fn system_dir() -> Option<PathBuf> {
    Some(crate::platform::system_dir())
}

#[cfg(not(windows))]
fn system_dir() -> Option<PathBuf> {
    None
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
    /// Quiet: stdout is dropped (`reg` and `taskkill` report success
    /// there), the first line of stderr joins the error.
    fn exec(&mut self, cmd: &[String]) -> Result<(), String> {
        use std::process::{Command, Stdio};
        let mut c = Command::new(&cmd[0]);
        c.args(&cmd[1..]).stdin(Stdio::null());
        crate::platform::hide_window(&mut c);
        let o = c.output().map_err(|e| e.to_string())?;
        if o.status.success() {
            return Ok(());
        }
        let err = String::from_utf8_lossy(&o.stderr);
        Err(match err.lines().map(str::trim).find(|l| !l.is_empty()) {
            Some(l) => format!("{}: {l}", o.status),
            None => o.status.to_string(),
        })
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
        #[cfg(windows)]
        return spawn_detached(c);
        #[cfg(not(windows))]
        c.spawn().map(|ch| ch.id()).map_err(|e| e.to_string())
    }
}

/// No console, its own process group and, when the job allows it, out
/// of the caller's job object, so closing this terminal leaves it be.
#[cfg(windows)]
fn spawn_detached(mut c: std::process::Command) -> Result<u32, String> {
    use std::os::windows::process::CommandExt;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
    let flags = DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP;
    match c.creation_flags(flags | CREATE_BREAKAWAY_FROM_JOB).spawn() {
        Ok(ch) => Ok(ch.id()),
        // a job without breakaway rights refuses the flag
        Err(_) => c
            .creation_flags(flags)
            .spawn()
            .map(|ch| ch.id())
            .map_err(|e| e.to_string()),
    }
}

/// Whether a tray holds the lock at `lock`, and its pid when readable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Running {
    No,
    Yes(Option<u32>),
}

/// Probes the lock without creating the file: a free lock is taken and
/// let go at once. The pid comes from the lock file, else (Windows, where
/// the holder's lock keeps others from reading it) from `tray.pid`.
pub fn running(lock: &Path) -> Running {
    let Ok(f) = File::open(lock) else {
        return Running::No;
    };
    let pid = |p: &Path| {
        std::fs::read_to_string(p)
            .ok()
            .and_then(|s| s.lines().next()?.trim().parse().ok())
    };
    match f.try_lock_shared() {
        Ok(()) => Running::No,
        Err(TryLockError::WouldBlock) => {
            Running::Yes(pid(lock).or_else(|| pid(&lock.with_file_name(PID_FILE))))
        }
        // a lock that cannot be probed counts as free
        Err(TryLockError::Error(_)) => Running::No,
    }
}

/// `taskkill` asks the tray to close (it quits on `WM_CLOSE`), with
/// `force` it ends it; elsewhere SIGTERM.
fn kill(l: &Layout, pid: u32, force: bool) -> Vec<String> {
    match &l.system_dir {
        Some(sys) => {
            let mut c = vec![
                sys.join("taskkill.exe").display().to_string(),
                "/PID".into(),
                pid.to_string(),
            ];
            if force {
                c.push("/F".into());
            }
            c
        }
        None => vec!["/bin/kill".into(), "-TERM".into(), pid.to_string()],
    }
}

/// Starts the tray unless one runs: through launchd when the LaunchAgent
/// is installed (load it, or start it again when loaded), through
/// `systemctl --user` when the unit is, else `argv` in the background.
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
    if l.unit().is_some() {
        let cmd = xdg::systemctl(&["start", UNIT]);
        sys.exec(&cmd).map_err(|e| format!("systemctl: {e}"))?;
        return writeln!(out, "started {UNIT}").map_err(io);
    }
    let pid = sys.spawn(argv).map_err(|e| format!("{}: {e}", argv[0]))?;
    writeln!(out, "started pid {pid}").map_err(io)
}

fn shown(pid: Option<u32>) -> String {
    pid.map_or(String::new(), |p| format!(" (pid {p})"))
}

/// Stops the tray: unloads the LaunchAgent when installed (it loads
/// again at the next login) or stops the unit, then signals a tray still
/// holding the lock: SIGTERM, or `taskkill` on Windows (`/F` when no
/// window takes the close request).
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
    if l.unit().is_some() {
        let was = running(&l.lock()) != Running::No;
        // systemd then neither restarts it nor counts it as failed
        if sys.exec(&xdg::systemctl(&["stop", UNIT])).is_ok() && was {
            writeln!(out, "stopped {UNIT}").map_err(io)?;
            stopped = true;
        }
    }
    match running(&l.lock()) {
        Running::No => {}
        Running::Yes(None) => return Err("a tray holds the lock but its pid is unknown".into()),
        Running::Yes(Some(pid)) => {
            sys.exec(&kill(l, pid, false))
                .or_else(|e| match l.system_dir {
                    // no window took the request: end it
                    Some(_) => sys.exec(&kill(l, pid, true)),
                    None => Err(e),
                })
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
    /// The LaunchAgent plist, the Windows `Run` value, the user unit or
    /// the autostart entry, when installed.
    pub autostart: Option<PathBuf>,
    pub running: Running,
    /// Seconds since the snapshot was written; `None` without one or
    /// once spent by a run.
    pub snapshot_age: Option<u64>,
}

pub fn status(l: &Layout, now: SystemTime) -> Status {
    status_with(l, now, &mut System)
}

/// [`status`] with `sys` asking `reg.exe` about the Windows `Run` value.
pub fn status_with(l: &Layout, now: SystemTime, sys: &mut dyn Sys) -> Status {
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
        autostart: l
            .plist()
            .filter(|p| p.is_file())
            .or_else(|| {
                let system = l.system_dir.as_deref()?;
                super::agent_windows::installed(system, sys).then(super::agent_windows::location)
            })
            .or_else(|| l.xdg.as_ref()?.autostart()),
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
