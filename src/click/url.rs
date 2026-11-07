//! `bar://cmd/<command>?tty=<ttysN>&sid=<session id>`: building and strict
//! parsing. Any page or app can open a `bar://` URL, so the allowlist here
//! and the checks in `check` are the security boundary.

use super::{handler_installed, Refusal};
use crate::options::Options;
use std::path::Path;
use std::sync::OnceLock;

/// The only commands a link can type. Fixed in core: neither config nor
/// plugins can add to it.
pub const COMMANDS: [&str; 3] = ["model", "effort", "compact"];

const PREFIX: &str = "bar://cmd/";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClickUrl {
    pub cmd: &'static str,
    pub tty: String,
    pub sid: String,
}

/// The terminal `bar render` runs on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Term {
    /// v0 types commands through macOS + tmux only.
    pub macos: bool,
    /// Controlling terminal of the parent (Claude Code), e.g. `ttys004`.
    pub tty: Option<String>,
}

impl Term {
    /// Probed once per process, and only when a link is wanted.
    pub fn current() -> &'static Term {
        static T: OnceLock<Term> = OnceLock::new();
        T.get_or_init(|| {
            let macos = cfg!(target_os = "macos");
            Term {
                macos,
                tty: macos.then(super::sys::parent_tty).flatten(),
            }
        })
    }
}

pub(crate) fn valid_tty(s: &str) -> bool {
    s.strip_prefix("ttys")
        .is_some_and(|n| (1..=6).contains(&n.len()) && n.bytes().all(|b| b.is_ascii_digit()))
}

/// Session ids are UUIDs: lowercase hex and dashes.
pub(crate) fn valid_sid(s: &str) -> bool {
    (1..=64).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b) || b == b'-')
}

fn allowed(cmd: &str) -> Option<&'static str> {
    COMMANDS.iter().copied().find(|c| *c == cmd)
}

/// The link for `cmd` in this session, or `None` when it cannot be typed.
pub fn link(cmd: &str, sid: &str, term: &Term) -> Option<String> {
    let cmd = allowed(cmd)?;
    let tty = term.tty.as_deref().filter(|t| term.macos && valid_tty(t))?;
    valid_sid(sid).then(|| format!("{PREFIX}{cmd}?tty={tty}&sid={sid}"))
}

/// Wraps `body` in the link for `cmd`, unless `click=none`, the handler is
/// not installed, or there is no usable terminal. `term` runs only when the
/// first two pass.
pub fn with_cmd_link(
    body: String,
    cmd: &str,
    sid: &str,
    opts: &Options,
    state: &Path,
    term: impl FnOnce() -> Term,
) -> String {
    if opts.get("click") == Some("none") || !handler_installed(state) {
        return body;
    }
    match link(cmd, sid, &term()) {
        Some(url) => crate::style::link(&url, &body),
        None => body,
    }
}

/// Syntax only: `(command, tty, sid)` with the command any `[a-z]+`.
pub(crate) fn parse_syntax(url: &str) -> Option<(&str, &str, &str)> {
    let rest = url.strip_prefix(PREFIX)?;
    let (cmd, query) = rest.split_once('?')?;
    if cmd.is_empty() || !cmd.bytes().all(|b| b.is_ascii_lowercase()) {
        return None;
    }
    let (mut tty, mut sid) = (None, None);
    for pair in query.split('&') {
        let (k, v) = pair.split_once('=')?;
        let slot = match k {
            "tty" => &mut tty,
            "sid" => &mut sid,
            _ => return None,
        };
        if slot.replace(v).is_some() {
            return None;
        }
    }
    let (tty, sid) = (tty?, sid?);
    (valid_tty(tty) && valid_sid(sid)).then_some((cmd, tty, sid))
}

/// Strict: exact scheme, host and path; `tty` and `sid` once each and
/// nothing else; the command on the allowlist.
pub fn parse(url: &str) -> Result<ClickUrl, Refusal> {
    let (cmd, tty, sid) = parse_syntax(url).ok_or(Refusal::Malformed)?;
    Ok(ClickUrl {
        cmd: allowed(cmd).ok_or(Refusal::NotAllowed)?,
        tty: tty.into(),
        sid: sid.into(),
    })
}
