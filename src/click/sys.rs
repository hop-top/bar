//! The real `Env`: `ps`, `tmux`, `osascript` and the log file. The URL
//! handler starts `bar` with a minimal `PATH`, so tools are looked up on
//! `PATH` first, then in the usual install dirs.

use super::{claude_dir, handler_state_dir, Env};
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

const FALLBACK_DIRS: [&str; 4] = ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin", "/bin"];

fn tool(name: &str) -> PathBuf {
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path)
        .chain(FALLBACK_DIRS.iter().map(PathBuf::from))
        .map(|d| d.join(name))
        .find(|p| p.is_file())
        .unwrap_or_else(|| PathBuf::from(name))
}

fn output(name: &str, args: &[&str]) -> Option<String> {
    let o = Command::new(tool(name))
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    o.status
        .success()
        .then(|| String::from_utf8_lossy(&o.stdout).into_owned())
}

fn tty_of(pid: u32) -> Option<String> {
    let t = output("ps", &["-o", "tty=", "-p", &pid.to_string()])?;
    let t = t.trim();
    (!t.is_empty()).then(|| t.to_string())
}

/// Controlling terminal of the parent process (Claude Code).
pub(crate) fn parent_tty() -> Option<String> {
    tty_of(crate::platform::parent_pid()?)
}

pub struct System {
    pub sessions: PathBuf,
    pub state: PathBuf,
}

impl System {
    pub fn from_env() -> System {
        System {
            sessions: claude_dir()
                .unwrap_or_else(|| PathBuf::from(".claude"))
                .join("sessions"),
            state: handler_state_dir(),
        }
    }
}

impl Env for System {
    fn sessions_dir(&self) -> PathBuf {
        self.sessions.clone()
    }
    fn alive(&self, pid: u32) -> bool {
        output("ps", &["-o", "pid=", "-p", &pid.to_string()])
            .is_some_and(|s| s.trim() == pid.to_string())
    }
    fn tty_of(&self, pid: u32) -> Option<String> {
        tty_of(pid)
    }
    fn tmux(&self, args: &[&str]) -> Option<String> {
        output("tmux", args)
    }
    fn sleep(&self, d: Duration) {
        std::thread::sleep(d);
    }
    fn now(&self) -> String {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs())
            .to_string()
    }
    /// Message and title go in as `argv`, never into the script text.
    fn notify(&self, title: &str, message: &str) {
        let _ = output(
            "osascript",
            &[
                "-e",
                "on run argv",
                "-e",
                "display notification (item 1 of argv) with title (item 2 of argv)",
                "-e",
                "end run",
                message,
                title,
            ],
        );
    }
    fn log(&self, line: &str) {
        let _ = std::fs::create_dir_all(&self.state);
        let mut o = std::fs::OpenOptions::new();
        crate::platform::private_file(o.create(true).append(true));
        if let Ok(mut f) = o.open(self.state.join("click.log")) {
            let _ = writeln!(f, "{line}");
        }
    }
}
