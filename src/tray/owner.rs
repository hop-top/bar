//! Whose autostart it is. Every tray install of a user (one per state
//! dir) shares one LaunchAgent, one `Run` value, one autostart entry and
//! one user unit, and each names the install that wrote it: the tray's
//! `--lock <state>/tray.lock`, or `--dirs <state>/tray-dirs.json` in the
//! `Run` value. Install does not take another install's over unless
//! forced; uninstall, start, stop and status leave it alone.

use super::process::Sys;
use super::xdg::Dirs;
use crate::click::pointer::TRAY_MARKER;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Owner {
    /// Not there.
    Absent,
    /// This install's.
    This,
    /// Another install's: the state dir it names still holds the tray
    /// marker.
    Other(PathBuf),
    /// No install's: it names a state dir uninstalled or removed, or no
    /// state dir at all. This install may use and remove it.
    Nobody,
}

impl Owner {
    fn of(named: Option<PathBuf>, state: &Path) -> Owner {
        match named {
            Some(d) if d == state => Owner::This,
            Some(d) if d.join(TRAY_MARKER).is_file() => Owner::Other(d),
            _ => Owner::Nobody,
        }
    }

    fn of_file(path: &Path, state: &Path, named: fn(&str) -> Option<PathBuf>) -> Owner {
        if !path.is_file() {
            return Owner::Absent;
        }
        let body = std::fs::read_to_string(path).unwrap_or_default();
        Owner::of(named(&body), state)
    }

    /// There, and this install's to start, stop or remove.
    pub fn ours(&self) -> bool {
        matches!(self, Owner::This | Owner::Nobody)
    }

    /// The other install's state dir.
    pub fn other(&self) -> Option<&Path> {
        match self {
            Owner::Other(d) => Some(d),
            _ => None,
        }
    }
}

/// The autostart entry's owner.
pub fn entry(d: &Dirs, state: &Path) -> Owner {
    Owner::of_file(&d.desktop_entry(), state, entry_state)
}

/// The user unit's owner.
pub fn unit(d: &Dirs, state: &Path) -> Owner {
    Owner::of_file(&d.unit(), state, unit_state)
}

/// The LaunchAgent's owner.
pub fn plist(path: &Path, state: &Path) -> Owner {
    Owner::of_file(path, state, plist_state)
}

/// The `Run` value's owner, from `reg query` in `system`.
pub fn run_value(system: &Path, sys: &mut dyn Sys, state: &Path) -> Owner {
    match sys.output(&super::agent_windows::query(system)) {
        Ok((true, out)) => Owner::of(run_value_state(&out), state),
        _ => Owner::Absent,
    }
}

/// What uninstall says about a file it leaves.
pub fn kept(what: &Path, other: &Path) -> String {
    format!(
        "kept {}: belongs to the tray install in {}",
        what.display(),
        other.display()
    )
}

/// Why install does not overwrite another install's autostart.
pub fn refused(what: &Path, other: &Path) -> String {
    format!(
        "{} belongs to the tray install in {o}: uninstall that one first \
         (bar tray uninstall with BAR_STATE_DIR={o}), or pass --force to take it over",
        what.display(),
        o = other.display()
    )
}

/// The dir of the path after `flag`.
fn dir_after(args: &[String], flag: &str) -> Option<PathBuf> {
    let i = args.iter().position(|a| a == flag)?;
    Path::new(args.get(i + 1)?).parent().map(Path::to_path_buf)
}

/// Words split at blanks outside double quotes; inside them `\` takes
/// the next character as is.
fn words(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let (mut quoted, mut word) = (false, false);
    let mut it = s.chars();
    while let Some(c) = it.next() {
        match c {
            '"' => (quoted, word) = (!quoted, true),
            '\\' if quoted => cur.extend(it.next()),
            ' ' | '\t' if !quoted => {
                if word || !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
                word = false;
            }
            c => cur.push(c),
        }
    }
    if word || !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// The state dir in an autostart entry's `Exec` (as `xdg::desktop_entry`
/// writes it: string escapes, then quoting, then `%%`).
fn entry_state(body: &str) -> Option<PathBuf> {
    let exec = body.lines().find_map(|l| l.strip_prefix("Exec="))?;
    let mut s = String::new();
    let mut it = exec.chars();
    while let Some(c) = it.next() {
        match (c, c == '\\') {
            (_, true) => s.push(match it.next()? {
                's' => ' ',
                't' => '\t',
                n => n,
            }),
            (c, false) => s.push(c),
        }
    }
    let args: Vec<String> = words(&s).iter().map(|w| w.replace("%%", "%")).collect();
    dir_after(&args, "--lock")
}

/// The state dir in a unit's `ExecStart` (as `xdg::unit` writes it).
fn unit_state(body: &str) -> Option<PathBuf> {
    let exec = body.lines().find_map(|l| l.strip_prefix("ExecStart="))?;
    let args: Vec<String> = words(exec)
        .iter()
        .map(|w| w.replace("$$", "$").replace("%%", "%"))
        .collect();
    dir_after(&args, "--lock")
}

/// The state dir in a LaunchAgent's `ProgramArguments`.
fn plist_state(body: &str) -> Option<PathBuf> {
    let (_, rest) = body.split_once("<key>ProgramArguments</key>")?;
    let (array, _) = rest.split_once("</array>")?;
    let args: Vec<String> = array
        .split("<string>")
        .skip(1)
        .filter_map(|s| s.split_once("</string>"))
        .map(|(v, _)| {
            v.replace("&lt;", "<")
                .replace("&gt;", ">")
                .replace("&amp;", "&")
        })
        .collect();
    dir_after(&args, "--lock")
}

/// The state dir in `reg query` output for the `Run` value: the dir of
/// `--dirs "<state>\tray-dirs.json"`.
fn run_value_state(out: &str) -> Option<PathBuf> {
    let (_, rest) = out.split_once("--dirs \"")?;
    let (record, _) = rest.split_once('"')?;
    Path::new(record).parent().map(Path::to_path_buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tray::{agent, agent_windows, xdg};

    fn argv(lock: &str) -> Vec<String> {
        ["/a b/bar-tray", "--bar", "/o/b$r", "--lock", lock]
            .map(String::from)
            .to_vec()
    }

    /// Paths the writers have to quote or escape.
    const LOCKS: &[&str] = &[
        "/s/tray.lock",
        "/s p/tray.lock",
        "/s/100%/tray.lock",
        "/s/$HOME/tray.lock",
        "/s/a\"b\\c`d/tray.lock",
        "/s/<&>/tray.lock",
    ];

    fn dir(lock: &str) -> Option<PathBuf> {
        Path::new(lock).parent().map(Path::to_path_buf)
    }

    #[test]
    fn the_state_dir_reads_back_from_every_autostart_file() {
        let env = [("PATH".to_string(), "/usr/bin:/bin".to_string())];
        for lock in LOCKS {
            let e = xdg::desktop_entry(&argv(lock), &env).unwrap();
            assert_eq!(entry_state(&e), dir(lock), "{e}");
            let p = agent::plist(&argv(lock), &env);
            assert_eq!(plist_state(&p), dir(lock), "{p}");
            #[cfg(unix)]
            {
                let u = xdg::unit(&argv(lock), &env).unwrap();
                assert_eq!(unit_state(&u), dir(lock), "{u}");
            }
        }
    }

    #[test]
    fn the_state_dir_reads_back_from_reg_query() {
        let c = agent_windows::command(
            Path::new(&format!("{}/a b/bar-tray.exe", root())),
            Path::new(&format!("{}/s p/tray-dirs.json", root())),
        )
        .unwrap();
        let out = format!("\r\nHKEY_CURRENT_USER\\x\r\n    bar-tray    REG_SZ    {c}\r\n\r\n");
        assert_eq!(
            run_value_state(&out),
            Some(PathBuf::from(format!("{}/s p", root())))
        );
        assert_eq!(run_value_state("bar-tray REG_SZ x"), None);
    }

    fn root() -> &'static str {
        if cfg!(windows) {
            "C:"
        } else {
            ""
        }
    }

    #[test]
    fn files_of_no_known_shape_name_no_state_dir() {
        for f in [entry_state, unit_state, plist_state] {
            assert_eq!(f("x"), None);
            assert_eq!(f("Exec=/t --bar /b\nExecStart=/t"), None);
        }
    }

    #[test]
    fn an_install_without_its_marker_owns_nothing() {
        let t = std::env::temp_dir().join(format!("bar-owner-{}", std::process::id()));
        let other = t.join("other");
        std::fs::create_dir_all(&other).unwrap();
        let state = t.join("state");
        assert_eq!(Owner::of(Some(state.clone()), &state), Owner::This);
        assert_eq!(Owner::of(Some(other.clone()), &state), Owner::Nobody);
        assert_eq!(Owner::of(None, &state), Owner::Nobody);
        std::fs::write(other.join(TRAY_MARKER), "").unwrap();
        let o = Owner::of(Some(other.clone()), &state);
        assert_eq!(o.other(), Some(other.as_path()));
        assert!(!o.ours() && Owner::Nobody.ours() && !Owner::Absent.ours());
        std::fs::remove_dir_all(&t).unwrap();
    }
}
