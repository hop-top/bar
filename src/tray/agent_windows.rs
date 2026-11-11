//! Windows autostart: the per-user `Run` value `bar-tray`, which starts
//! the tray at every logon. A `Run` value is a bare command line, with no
//! way to set variables, so it names `tray-dirs.json` and the tray takes
//! `bar`, its lock and the recorded variables from there. Set, read and
//! removed with `reg.exe` from the system dir: no shell, an argument
//! array, nothing else in the registry is touched. In particular the
//! notification-area settings (`NotifyIconSettings`, `IsPromoted`) stay
//! the user's: new icons land in the overflow area until the user pins
//! them, and install says how.

use super::process::{self, Layout, Sys};
use std::io::Write;
use std::path::{Path, PathBuf};

/// The per-user `Run` key.
pub const KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
/// The value under [`KEY`].
pub const VALUE: &str = "bar-tray";
/// Longest command line a `Run` value may hold.
pub const MAX_COMMAND: usize = 260;

/// Printed by the first install: Windows hides a new icon in the
/// overflow area, and only the user can pin it.
pub const OVERFLOW_HINT: &str = "\
the bar icon starts in the taskbar overflow (the ^ next to the clock); \
to keep it visible, drag it onto the taskbar, or turn bar-tray on in \
Settings > Personalization > Taskbar > Other system tray icons (ms-settings:taskbar)";

/// Where the value lives, as `status` shows it.
pub fn location() -> PathBuf {
    PathBuf::from(format!(r"{KEY}\{VALUE}"))
}

/// `reg.exe` in `system`, never one found on `PATH`.
fn reg(system: &Path) -> String {
    system.join("reg.exe").display().to_string()
}

/// The tray's logon command line: `"<tray>" --dirs "<record>"`. Both
/// paths absolute and free of `"`; at most [`MAX_COMMAND`] characters.
pub fn command(tray: &Path, record: &Path) -> Result<String, String> {
    for p in [tray, record] {
        let s = p.display().to_string();
        if !p.is_absolute() || s.contains('"') {
            return Err(format!("not an absolute path without quotes: {s}"));
        }
    }
    let c = format!("\"{}\" --dirs \"{}\"", tray.display(), record.display());
    if c.chars().count() > MAX_COMMAND {
        return Err(format!(
            "the autostart command is longer than {MAX_COMMAND} characters; \
             install bar in a shorter path or use --no-autostart"
        ));
    }
    Ok(c)
}

/// The same command as an argument array, for starting the tray now.
pub fn argv(tray: &Path, record: &Path) -> Vec<String> {
    vec![
        tray.display().to_string(),
        "--dirs".into(),
        record.display().to_string(),
    ]
}

/// Sets the value to `command`, replacing an older one.
pub fn add(system: &Path, command: &str) -> Vec<String> {
    vec![
        reg(system),
        "add".into(),
        KEY.into(),
        "/v".into(),
        VALUE.into(),
        "/t".into(),
        "REG_SZ".into(),
        "/d".into(),
        command.into(),
        "/f".into(),
    ]
}

/// Succeeds when the value exists.
pub fn query(system: &Path) -> Vec<String> {
    vec![
        reg(system),
        "query".into(),
        KEY.into(),
        "/v".into(),
        VALUE.into(),
    ]
}

/// Removes the value; fails when it is absent.
pub fn delete(system: &Path) -> Vec<String> {
    vec![
        reg(system),
        "delete".into(),
        KEY.into(),
        "/v".into(),
        VALUE.into(),
        "/f".into(),
    ]
}

/// What install sets up: the value's command and the tray started now.
pub struct Autostart {
    pub system: PathBuf,
    pub command: String,
    pub argv: Vec<String>,
}

impl Autostart {
    pub fn new(system: &Path, tray: &Path, record: &Path) -> Result<Autostart, String> {
        Ok(Autostart {
            system: system.to_path_buf(),
            command: command(tray, record)?,
            argv: argv(tray, record),
        })
    }

    /// Sets the value, then starts the tray unless one runs (a running
    /// one keeps going: the next logon uses the new value). With
    /// `dry_run` only prints the commands.
    pub fn install(
        &self,
        l: &Layout,
        dry_run: bool,
        sys: &mut dyn Sys,
        out: &mut dyn Write,
    ) -> Result<(), String> {
        let io = |e: std::io::Error| e.to_string();
        let add = add(&self.system, &self.command);
        if dry_run {
            writeln!(out, "{}", shown(&add)).map_err(io)?;
            return writeln!(out, "start {}", shown(&self.argv)).map_err(io);
        }
        sys.exec(&add).map_err(|e| format!("reg: {e}"))?;
        writeln!(out, "installed {}", location().display()).map_err(io)?;
        process::start(l, &self.argv, sys, out)
    }
}

/// `cmd` as a line, each argument with spaces in quotes.
fn shown(cmd: &[String]) -> String {
    cmd.iter()
        .map(|a| {
            if a.contains(' ') && !a.starts_with('"') {
                format!("\"{a}\"")
            } else {
                a.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Whether the value exists.
pub fn installed(system: &Path, sys: &mut dyn Sys) -> bool {
    sys.exec(&query(system)).is_ok()
}

/// Removes the value when it exists.
pub fn remove(system: &Path, sys: &mut dyn Sys, out: &mut dyn Write) -> Result<(), String> {
    if !installed(system, sys) {
        return Ok(());
    }
    sys.exec(&delete(system)).map_err(|e| format!("reg: {e}"))?;
    writeln!(out, "removed {}", location().display()).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_quotes_both_paths() {
        assert_eq!(
            command(
                Path::new("/a b/bar-tray.exe"),
                Path::new("/s/tray-dirs.json")
            )
            .unwrap(),
            r#""/a b/bar-tray.exe" --dirs "/s/tray-dirs.json""#
        );
    }

    #[test]
    fn command_refuses_relative_quoted_or_long() {
        assert!(command(Path::new("bar-tray.exe"), Path::new("/s/r.json")).is_err());
        assert!(command(Path::new("/t"), Path::new("r.json")).is_err());
        assert!(command(Path::new("/a\"b/t"), Path::new("/s/r.json")).is_err());
        let long = format!("/{}", "d".repeat(MAX_COMMAND));
        let e = command(Path::new(&long), Path::new("/s/r.json")).unwrap_err();
        assert!(e.contains("260"), "{e}");
        // exactly at the limit is fine
        let fixed = r#""" --dirs "/r""#.len();
        let p = format!("/{}", "x".repeat(MAX_COMMAND - fixed - 1));
        assert_eq!(
            command(Path::new(&p), Path::new("/r")).unwrap().len(),
            MAX_COMMAND
        );
    }

    #[test]
    fn reg_commands_touch_only_the_run_value() {
        let sys = Path::new("/sys32");
        let all = [add(sys, "\"/t\""), query(sys), delete(sys)];
        for c in &all {
            assert_eq!(c[0], "/sys32/reg.exe");
            assert_eq!(c[2], KEY);
            assert_eq!(&c[3..5], ["/v", "bar-tray"]);
            assert!(!c
                .iter()
                .any(|a| a.contains("IsPromoted") || a.contains("NotifyIconSettings")));
        }
        assert_eq!(&all[0][5..], ["/t", "REG_SZ", "/d", "\"/t\"", "/f"]);
        assert_eq!(all[1].len(), 5);
        assert_eq!(all[2].last().unwrap(), "/f");
    }

    #[test]
    fn shown_quotes_spaced_arguments() {
        assert_eq!(
            shown(&["/a b/t".into(), "--dirs".into(), "\"/x y\"".into()]),
            r#""/a b/t" --dirs "/x y""#
        );
    }
}
