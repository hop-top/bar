//! `bar click run`: fail-closed checks, then the literal keys. A refusal
//! notifies and logs, and never touches the terminal.

use super::hook::is_waiting;
use super::url::{parse, parse_syntax};
use serde_json::Value;
use std::path::PathBuf;
use std::time::Duration;

/// Everything `bar click run` touches outside the process.
pub trait Env {
    /// Claude Code's `sessions/` dir (`<pid>.json` per live session).
    fn sessions_dir(&self) -> PathBuf;
    /// bar's state dir, where the hook writes waiting markers.
    fn state_dir(&self) -> PathBuf;
    fn alive(&self, pid: u32) -> bool;
    /// Controlling terminal of `pid`, e.g. `ttys004`.
    fn tty_of(&self, pid: u32) -> Option<String>;
    /// Runs `tmux <args>`; stdout on success.
    fn tmux(&self, args: &[&str]) -> Option<String>;
    fn sleep(&self, d: Duration);
    /// Timestamp for log lines.
    fn now(&self) -> String;
    fn notify(&self, title: &str, message: &str);
    fn log(&self, line: &str);
}

/// A tmux pane id, e.g. `%3`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pane(pub String);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    Malformed,
    NotAllowed,
    Missing,
    Dead,
    OtherTty,
    NoPane,
    Busy,
    Waiting,
    NotEmpty,
    SendFailed,
}

impl Refusal {
    pub fn message(self) -> &'static str {
        match self {
            Refusal::Malformed => "malformed link",
            Refusal::NotAllowed => "command is not allowed",
            Refusal::Missing => "session not found",
            Refusal::Dead => "session is not running",
            Refusal::OtherTty => "session is on another terminal",
            Refusal::NoPane => "session is not on screen in tmux",
            Refusal::Busy => "session is busy",
            Refusal::Waiting => "session is waiting for you",
            Refusal::NotEmpty => "the input box is not empty",
            Refusal::SendFailed => "tmux send-keys failed",
        }
    }
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str(self.message())
    }
}

/// `(pid, status)` of the session file whose `sessionId` is `sid`.
fn session(env: &dyn Env, sid: &str) -> Option<(u32, String)> {
    let dir = std::fs::read_dir(env.sessions_dir()).ok()?;
    dir.flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .filter_map(|p| serde_json::from_str::<Value>(&std::fs::read_to_string(p).ok()?).ok())
        .find(|v| v.get("sessionId").and_then(Value::as_str) == Some(sid))
        .and_then(|v| {
            let pid = u32::try_from(v.get("pid")?.as_u64()?).ok()?;
            let status = v.get("status")?.as_str()?.to_string();
            Some((pid, status))
        })
}

fn valid_pane(id: &str) -> bool {
    id.strip_prefix('%')
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

/// `sid=<sid>` not followed by another id character, so a longer id that
/// shares the prefix does not match.
fn shows_sid(screen: &str, sid: &str) -> bool {
    let needle = format!("sid={sid}");
    screen.match_indices(&needle).any(|(i, _)| {
        !screen[i + needle.len()..]
            .bytes()
            .next()
            .is_some_and(|b| b.is_ascii_hexdigit() || b == b'-')
    })
}

fn capture(env: &dyn Env, pane: &str) -> Option<String> {
    env.tmux(&["capture-pane", "-p", "-e", "-t", pane])
}

/// The session's own pane, else the pane whose status line carries its id:
/// a background session is shown through another client.
fn find_pane(env: &dyn Env, sid: &str, tty: &str) -> Option<Pane> {
    let list = env.tmux(&["list-panes", "-a", "-F", "#{pane_tty} #{pane_id}"])?;
    let panes: Vec<(&str, &str)> = list
        .lines()
        .filter_map(|l| l.split_once(' '))
        .filter(|(_, id)| valid_pane(id))
        .collect();
    let dev = format!("/dev/{tty}");
    if let Some((_, id)) = panes.iter().find(|(t, _)| *t == dev) {
        return Some(Pane(id.to_string()));
    }
    panes
        .iter()
        .find(|(_, id)| capture(env, id).is_some_and(|s| shows_sid(&s, sid)))
        .map(|(_, id)| Pane(id.to_string()))
}

/// Empty box: the last prompt marker line holds nothing, or only dim
/// placeholder text. Drafts and menu selections render in normal text.
pub fn box_is_empty(screen: &str) -> bool {
    let Some(line) = screen.lines().rfind(|l| l.contains('❯')) else {
        return false;
    };
    let (_, mut rest) = line.split_once('❯').unwrap_or_default();
    loop {
        let t = rest.trim_start_matches(|c: char| c.is_whitespace() || c == '\u{a0}');
        match t
            .strip_prefix("\x1b[0m")
            .or_else(|| t.strip_prefix("\x1b[m"))
        {
            Some(r) => rest = r,
            None => {
                rest = t;
                break;
            }
        }
    }
    let rest = rest.trim_end();
    rest.is_empty() || rest.starts_with("\x1b[2m")
}

fn idle_and_empty(env: &dyn Env, sid: &str, pane: &Pane) -> Result<(), Refusal> {
    let (_, status) = session(env, sid).ok_or(Refusal::Missing)?;
    if status != "idle" {
        return Err(Refusal::Busy);
    }
    if is_waiting(&env.state_dir(), sid) {
        return Err(Refusal::Waiting);
    }
    match capture(env, &pane.0) {
        Some(s) if box_is_empty(&s) => Ok(()),
        _ => Err(Refusal::NotEmpty),
    }
}

/// Every check, the idle/empty pair twice a second apart: dialogs can open
/// just after a turn ends.
pub fn check(env: &dyn Env, sid: &str, tty: &str) -> Result<Pane, Refusal> {
    let (pid, _) = session(env, sid).ok_or(Refusal::Missing)?;
    if !env.alive(pid) {
        return Err(Refusal::Dead);
    }
    if env.tty_of(pid).as_deref() != Some(tty) {
        return Err(Refusal::OtherTty);
    }
    let pane = find_pane(env, sid, tty).ok_or(Refusal::NoPane)?;
    idle_and_empty(env, sid, &pane)?;
    env.sleep(Duration::from_secs(1));
    idle_and_empty(env, sid, &pane)?;
    Ok(pane)
}

fn send(env: &dyn Env, pane: &Pane, cmd: &str) -> Result<(), Refusal> {
    let text = format!("/{cmd}");
    env.tmux(&["send-keys", "-t", &pane.0, "-l", &text])
        .and_then(|_| env.tmux(&["send-keys", "-t", &pane.0, "Enter"]))
        .map(drop)
        .ok_or(Refusal::SendFailed)
}

/// Parses, checks, then types `/<command>` and Enter into the pane.
pub fn run(env: &dyn Env, url: &str) -> Result<Pane, Refusal> {
    let label = parse_syntax(url).map_or("?", |(c, _, _)| c);
    let r = parse(url).and_then(|u| {
        let pane = check(env, &u.sid, &u.tty)?;
        send(env, &pane, u.cmd)?;
        Ok(pane)
    });
    match &r {
        Ok(p) => env.log(&format!("{} sent {label} to {}", env.now(), p.0)),
        Err(e) => {
            env.log(&format!("{} refused {label}: {e}", env.now()));
            env.notify(&format!("bar: {label} not sent"), e.message());
        }
    }
    r
}
