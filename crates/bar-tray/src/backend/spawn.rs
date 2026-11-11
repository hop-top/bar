//! Children of the tray: `bar tray state --json` (output captured) and
//! `bar tray run <id>` / `bar toggle <key>` (output dropped, first line
//! of stderr kept as the failure note). Argument arrays only, never a
//! shell; `bar` is found beside the tray binary (or where `--bar` says),
//! never on `PATH`. Every child gets null stdin, a scrubbed
//! environment (only the names the `bar://` handler bakes, with the
//! tray's own values), a time limit and, past it, a kill of the child
//! and everything it started.

mod tree;

use std::ffi::OsString;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

/// Output of `bar tray state --json` beyond this is an error.
pub const MAX_CAPTURE: u64 = 1 << 20;

/// File name of the `bar` binary on this OS.
pub const BAR_NAME: &str = if cfg!(windows) { "bar.exe" } else { "bar" };

/// Every variable that moves a dir `bar` reads: the home dir (where its
/// config and state dirs are without an override; the `bar://` handler
/// inherits it from the login session, a child of the tray would not),
/// the state dir, the XDG bases, Claude Code's config dir, tmux's socket
/// dir, and `PATH`, where an action finds its plugin. Nothing else
/// reaches a child.
#[cfg(not(windows))]
pub const HANDLER_ENV: [&str; 8] = [
    "HOME",
    "BAR_STATE_DIR",
    "XDG_STATE_HOME",
    "XDG_CONFIG_HOME",
    "XDG_DATA_HOME",
    "CLAUDE_CONFIG_DIR",
    "TMUX_TMPDIR",
    "PATH",
];

/// Windows: the profile and app-data dirs stand in for the home dir, and
/// `SYSTEMROOT` is needed to start any process at all.
#[cfg(windows)]
pub const HANDLER_ENV: [&str; 11] = [
    "USERPROFILE",
    "APPDATA",
    "LOCALAPPDATA",
    "SYSTEMROOT",
    "BAR_STATE_DIR",
    "XDG_STATE_HOME",
    "XDG_CONFIG_HOME",
    "XDG_DATA_HOME",
    "CLAUDE_CONFIG_DIR",
    "TMUX_TMPDIR",
    "PATH",
];

/// Set to [`PRIVATE_CONSOLE`]: tells a `bar` started by the tray that
/// its console (created hidden, `CREATE_NO_WINDOW`) is its own, so its
/// children may share it.
pub const PRIVATE_CONSOLE_ENV: &str = "BAR_PRIVATE_CONSOLE";
/// The value of [`PRIVATE_CONSOLE_ENV`].
pub const PRIVATE_CONSOLE: &str = "1";

/// Whether `name` is one of [`HANDLER_ENV`]; Windows names are stored
/// in any case (`Path`, `SystemRoot`).
pub fn handler_var(name: &str) -> bool {
    HANDLER_ENV.iter().any(|h| {
        if cfg!(windows) {
            h.eq_ignore_ascii_case(name)
        } else {
            *h == name
        }
    })
}

/// The longest `bar tray run` lets an action run.
pub const MAX_LIMIT: Duration = Duration::from_secs(120);
/// What `bar` gets on top of an action's limit before the tray kills it.
pub const GRACE: Duration = Duration::from_secs(5);
/// How long `bar tray state --json` or `bar toggle` may take: plugins
/// answer `--menu` within 3 s each, in parallel.
pub const QUICK: Duration = Duration::from_secs(15);
/// Longest failure note kept from stderr.
const MAX_NOTE: usize = 80;
/// How long the pipes may stay open after a kill.
const PIPE_GRACE: Duration = Duration::from_millis(500);

/// The deadline for an action: its limit (`None` or 0: the longest
/// `bar` allows) plus [`GRACE`].
pub fn deadline(limit: Option<u64>) -> Duration {
    let limit = match limit {
        Some(s) if s > 0 => Duration::from_secs(s).min(MAX_LIMIT),
        _ => MAX_LIMIT,
    };
    limit + GRACE
}

/// `bar` next to the tray binary at `tray_exe` (an absolute path).
pub fn bar_path(tray_exe: &Path) -> PathBuf {
    tray_exe.with_file_name(BAR_NAME)
}

/// Where `bar` is, resolved once: `explicit` when given (made
/// absolute), else beside this executable. Never `PATH`: a writable
/// entry there could swap in another `bar` for every click.
pub fn locate(explicit: Option<&Path>) -> Result<PathBuf, String> {
    match explicit {
        Some(p) => absolute(p),
        None => beside_exe(std::env::current_exe().ok().as_deref()),
    }
}

/// `bar` beside the tray at `exe`, when there is one.
fn beside_exe(exe: Option<&Path>) -> Result<PathBuf, String> {
    exe.map(bar_path)
        .filter(|p| p.is_file())
        .ok_or_else(|| "no `bar` beside bar-tray; pass --bar <path>".to_string())
}

fn absolute(p: &Path) -> Result<PathBuf, String> {
    let abs = if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::env::current_dir().map_err(|e| e.to_string())?.join(p)
    };
    if abs.is_file() {
        Ok(abs)
    } else {
        Err(format!("{}: not a file", abs.display()))
    }
}

pub fn state_args() -> Vec<OsString> {
    ["tray", "state", "--json"].map(OsString::from).to_vec()
}

/// `generation` ties the click to the snapshot the user saw.
pub fn run_args(id: &str, confirmed: bool, generation: &str) -> Vec<OsString> {
    let mut a: Vec<OsString> = ["tray", "run", id].map(OsString::from).to_vec();
    if confirmed {
        a.push("--confirmed".into());
    }
    if !generation.is_empty() {
        a.push("--generation".into());
        a.push(generation.into());
    }
    a
}

pub fn toggle_args(key: &str) -> Vec<OsString> {
    vec!["toggle".into(), key.into()]
}

/// The [`HANDLER_ENV`] subset of `env`; empty values count as unset.
pub fn scrub(env: impl IntoIterator<Item = (OsString, OsString)>) -> Vec<(OsString, OsString)> {
    env.into_iter()
        .filter(|(k, v)| k.to_str().is_some_and(handler_var) && !v.is_empty())
        .collect()
}

fn command(bar: &Path, args: &[OsString], capture: bool) -> Command {
    let mut c = Command::new(bar);
    c.args(args)
        .env_clear()
        .envs(scrub(std::env::vars_os()))
        .stdin(Stdio::null())
        .stdout(if capture {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stderr(Stdio::piped());
    // its console is hidden and its own: its children may share it
    #[cfg(windows)]
    c.env(PRIVATE_CONSOLE_ENV, PRIVATE_CONSOLE);
    tree::own_group(&mut c);
    c
}

/// Reads a pipe to its end on its own thread, keeping at most `cap`
/// bytes plus one (so the caller can tell an overflow).
fn reader<R: Read + Send + 'static>(pipe: Option<R>, cap: u64) -> Option<Receiver<Vec<u8>>> {
    pipe.map(|p| {
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut s = Vec::new();
            let _ = p.take(cap + 1).read_to_end(&mut s);
            let _ = tx.send(s);
        });
        rx
    })
}

/// What the pipe's reader got, once the pipe closed by `by`; `None` while
/// something still holds it open (its reader is left to finish alone).
fn collected(rx: Option<Receiver<Vec<u8>>>, by: Instant) -> Option<Vec<u8>> {
    let Some(rx) = rx else {
        return Some(Vec::new());
    };
    match rx.recv_timeout(by.saturating_duration_since(Instant::now())) {
        Ok(s) => Some(s),
        Err(RecvTimeoutError::Timeout) => None,
        Err(RecvTimeoutError::Disconnected) => Some(Vec::new()),
    }
}

/// Waits for `child` until `deadline`, then kills it with everything it
/// started and reaps it.
fn wait(child: &mut Child, deadline: Instant) -> Result<ExitStatus, String> {
    match bar_plugin::wait::wait_until(child, deadline) {
        Ok(Some(st)) => Ok(st),
        Ok(None) => {
            tree::kill_tree(child);
            let _ = child.wait();
            Err("timed out".to_string())
        }
        Err(e) => Err(e.to_string()),
    }
}

/// Runs `bar args`, waits at most `limit` (then kills and reaps it with
/// its descendants) and returns what it printed when `capture` is set
/// (empty otherwise). A failure's message is the first line of stderr
/// when there is one. A descendant that escaped the kill, or outlives a
/// child that exited, holds the pipes no longer than the limit.
pub fn run(
    bar: &Path,
    args: &[OsString],
    capture: bool,
    limit: Duration,
) -> Result<String, String> {
    let mut child = command(bar, args, capture)
        .spawn()
        .map_err(|e| format!("{}: {e}", bar.display()))?;
    let out = reader(child.stdout.take(), MAX_CAPTURE);
    let err = reader(child.stderr.take(), 4096);
    let start = Instant::now();
    let status = wait(&mut child, start + limit);
    let by = (start + limit).max(Instant::now() + PIPE_GRACE);
    let bytes = collected(out, by);
    let stderr =
        collected(err, by).map_or_else(String::new, |e| note(&String::from_utf8_lossy(&e)));
    if bytes.as_ref().is_some_and(|b| b.len() as u64 > MAX_CAPTURE) {
        return Err("output too large".to_string());
    }
    let status = status?;
    let bytes = bytes.ok_or_else(|| "timed out".to_string())?;
    if !status.success() {
        return Err(if stderr.is_empty() {
            format!("exit {}", status.code().unwrap_or(-1))
        } else {
            stderr
        });
    }
    String::from_utf8(bytes).map_err(|_| "output not UTF-8".to_string())
}

/// The first line, printable characters only, at most [`MAX_NOTE`].
fn note(out: &str) -> String {
    out.lines()
        .next()
        .unwrap_or("")
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_NOTE)
        .collect::<String>()
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests;
