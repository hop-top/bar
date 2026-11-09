//! `bar click run`: fail-closed checks, then the literal keys. A refusal
//! notifies and logs, and never touches the terminal.

use super::url::{parse, parse_syntax};
use serde_json::Value;
use std::path::PathBuf;
use std::time::Duration;

/// Everything `bar click run` touches outside the process.
pub trait Env {
    /// Claude Code's `sessions/` dir (`<pid>.json` per live session).
    fn sessions_dir(&self) -> PathBuf;
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

/// Pid in the session file whose `sessionId` is `sid`.
fn session(env: &dyn Env, sid: &str) -> Option<u32> {
    let dir = std::fs::read_dir(env.sessions_dir()).ok()?;
    dir.flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .filter_map(|p| serde_json::from_str::<Value>(&std::fs::read_to_string(p).ok()?).ok())
        .find(|v| v.get("sessionId").and_then(Value::as_str) == Some(sid))
        .and_then(|v| u32::try_from(v.get("pid")?.as_u64()?).ok())
}

fn valid_pane(id: &str) -> bool {
    id.strip_prefix('%')
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

/// Target of every OSC 8 link that opens on `line`; closing links (empty
/// target) and unterminated ones are skipped.
fn link_targets(line: &str) -> impl Iterator<Item = &str> {
    line.split("\x1b]8;").skip(1).filter_map(|rest| {
        let (_, rest) = rest.split_once(';')?;
        let end = rest.find(['\x1b', '\x07'])?;
        Some(&rest[..end]).filter(|t| !t.is_empty())
    })
}

/// The session's own status line is on screen: below the last box border,
/// a link that parses as a `bar://cmd/` link for exactly this `tty` and
/// `sid`. Text above the box (a transcript quoting a link, or the id in
/// plain text) never counts.
fn shows_session(screen: &str, sid: &str, tty: &str) -> bool {
    let lines: Vec<&str> = screen.lines().collect();
    let Some(bottom) = lines.iter().rposition(|l| border_width(l) >= MIN_BORDER) else {
        return false;
    };
    lines[bottom + 1..]
        .iter()
        .flat_map(|l| link_targets(l))
        .any(|t| parse(t).is_ok_and(|u| u.sid == sid && u.tty == tty))
}

fn capture(env: &dyn Env, pane: &str) -> Option<String> {
    env.tmux(&["capture-pane", "-p", "-e", "-t", pane])
}

/// The session's own pane, else a pane showing its status line: a
/// background session has a terminal of its own and is shown through a
/// client (`claude attach`, the agents view) in another pane. Every such
/// client forwards keys to the one session, so any of them will do; the
/// first in tmux order is taken. `true` when found by status line, so it
/// is checked again on every pass.
fn find_pane(env: &dyn Env, sid: &str, tty: &str) -> Option<(Pane, bool)> {
    let list = env.tmux(&["list-panes", "-a", "-F", "#{pane_tty} #{pane_id}"])?;
    let panes: Vec<(&str, &str)> = list
        .lines()
        .filter_map(|l| l.split_once(' '))
        .filter(|(_, id)| valid_pane(id))
        .collect();
    let dev = format!("/dev/{tty}");
    if let Some((_, id)) = panes.iter().find(|(t, _)| *t == dev) {
        return Some((Pane(id.to_string()), false));
    }
    panes
        .iter()
        .find(|(_, id)| capture(env, id).is_some_and(|s| shows_session(&s, sid, tty)))
        .map(|(_, id)| (Pane(id.to_string()), true))
}

/// Visible text is allowed only inside dim spans that are not reverse
/// video; elsewhere nothing but whitespace and SGR codes. Any other
/// escape, or an SGR parameter that does not parse, refuses.
fn only_dim_text(s: &str) -> bool {
    let (mut dim, mut reverse) = (false, false);
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c != '\x1b' {
            let placeholder = dim && !reverse;
            if !(c.is_whitespace() || c == '\u{a0}' || placeholder) {
                return false;
            }
            continue;
        }
        if it.next() != Some('[') {
            return false;
        }
        let mut raw = String::new();
        loop {
            match it.next() {
                Some('m') => break,
                Some(p) if p.is_ascii_digit() || p == ';' => raw.push(p),
                _ => return false,
            }
        }
        // Empty or zero-padded parameters are numbers too: `\e[m`, `\e[00m`.
        let Ok(params) = raw
            .split(';')
            .map(|p| {
                if p.is_empty() {
                    Ok(0)
                } else {
                    p.parse::<u32>()
                }
            })
            .collect::<Result<Vec<u32>, _>>()
        else {
            return false;
        };
        let mut i = 0;
        while i < params.len() {
            match params[i] {
                0 => (dim, reverse) = (false, false),
                2 => dim = true,
                22 => dim = false,
                7 => reverse = true,
                27 => reverse = false,
                // 38;5;n and 38;2;r;g;b (also 48, 58) carry arguments.
                38 | 48 | 58 => match params.get(i + 1) {
                    Some(5) => i += 2,
                    Some(2) => i += 4,
                    _ => {}
                },
                _ => {}
            }
            i += 1;
        }
    }
    true
}

/// Cells of a box border line (only `─` once codes are removed), else 0.
fn border_width(line: &str) -> usize {
    let plain = crate::style::strip_codes(line);
    let t = plain.trim();
    if t.chars().all(|c| c == '─') {
        t.chars().count()
    } else {
        0
    }
}

/// Narrower than this is not taken for the input box.
const MIN_BORDER: usize = 20;

/// Empty box. A box opens where a border (only `─`, at least
/// `MIN_BORDER` cells) sits right above a line holding the `❯` marker; the
/// real box always opens that way, so a draft that types a fake box adds a
/// second opening. Ambiguity refuses, hence all of:
/// - exactly one opening on screen, and no `❯` line below its marker;
/// - the very next line is the bottom border: same width, and the last such
///   line on screen (a draft of more than one line refuses);
/// - after the marker only dim placeholder text, never reverse video.
pub fn box_is_empty(screen: &str) -> bool {
    let lines: Vec<&str> = screen.lines().collect();
    let openings: Vec<usize> = (1..lines.len())
        .filter(|&k| lines[k].contains('❯') && border_width(lines[k - 1]) >= MIN_BORDER)
        .collect();
    let [i] = openings[..] else {
        return false;
    };
    if lines[i + 1..].iter().any(|l| l.contains('❯')) {
        return false;
    }
    let width = border_width(lines[i - 1]);
    let bottom = lines.iter().rposition(|l| border_width(l) == width);
    let (_, rest) = lines[i].split_once('❯').unwrap_or_default();
    bottom == Some(i + 1) && only_dim_text(rest)
}

/// Busy or not, only the box gates the keys: a dialog (permission prompt,
/// question, picker) replaces the box, so it never parses as empty.
fn empty_box(
    env: &dyn Env,
    sid: &str,
    tty: &str,
    pane: &Pane,
    by_sid: bool,
) -> Result<(), Refusal> {
    session(env, sid).ok_or(Refusal::Missing)?;
    let screen = capture(env, &pane.0).ok_or(Refusal::NotEmpty)?;
    if by_sid && !shows_session(&screen, sid, tty) {
        return Err(Refusal::NoPane);
    }
    if box_is_empty(&screen) {
        Ok(())
    } else {
        Err(Refusal::NotEmpty)
    }
}

/// Every check, the empty box twice a second apart: a dialog can open at
/// any moment.
pub fn check(env: &dyn Env, sid: &str, tty: &str) -> Result<Pane, Refusal> {
    let pid = session(env, sid).ok_or(Refusal::Missing)?;
    if !env.alive(pid) {
        return Err(Refusal::Dead);
    }
    if env.tty_of(pid).as_deref() != Some(tty) {
        return Err(Refusal::OtherTty);
    }
    let (pane, by_sid) = find_pane(env, sid, tty).ok_or(Refusal::NoPane)?;
    empty_box(env, sid, tty, &pane, by_sid)?;
    env.sleep(Duration::from_secs(1));
    empty_box(env, sid, tty, &pane, by_sid)?;
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
