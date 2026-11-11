//! Children of the tray: `bar tray state --json` (output captured) and
//! `bar tray run <id>` / `bar toggle <key>` (output dropped). Argument
//! arrays only, never a shell; `bar` is found beside the tray binary,
//! never on `PATH`. A tray built for the Windows GUI subsystem has no
//! console, so every child gets null stdio and no console window.

use std::ffi::OsString;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Output of `bar tray state --json` beyond this is an error.
pub const MAX_CAPTURE: u64 = 1 << 20;

/// File name of the `bar` binary on this OS.
pub const BAR_NAME: &str = if cfg!(windows) { "bar.exe" } else { "bar" };

/// `bar` next to the tray binary at `tray_exe` (an absolute path).
pub fn bar_path(tray_exe: &Path) -> PathBuf {
    tray_exe.with_file_name(BAR_NAME)
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

fn command(bar: &Path, args: &[OsString], capture: bool) -> Command {
    let mut c = Command::new(bar);
    c.args(args)
        .stdin(Stdio::null())
        .stdout(if capture {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        c.creation_flags(CREATE_NO_WINDOW);
    }
    c
}

/// Runs `bar args`, waits at most `limit` (then kills and reaps it) and
/// returns what it printed when `capture` is set (empty otherwise).
pub fn run(
    bar: &Path,
    args: &[OsString],
    capture: bool,
    limit: Duration,
) -> Result<String, String> {
    let mut child = command(bar, args, capture)
        .spawn()
        .map_err(|e| format!("{}: {e}", bar.display()))?;
    let reader = child.stdout.take().map(|out| {
        std::thread::spawn(move || {
            let mut s = Vec::new();
            let _ = out.take(MAX_CAPTURE + 1).read_to_end(&mut s);
            s
        })
    });
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(st)) => break Ok(st),
            Ok(None) if start.elapsed() >= limit => {
                let _ = child.kill();
                let _ = child.wait();
                break Err("timed out".to_string());
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(e) => break Err(e.to_string()),
        }
    };
    let bytes = reader.map_or_else(Vec::new, |h| h.join().unwrap_or_default());
    if bytes.len() as u64 > MAX_CAPTURE {
        return Err("output too large".to_string());
    }
    let status = status?;
    if !status.success() {
        return Err(format!("exit {}", status.code().unwrap_or(-1)));
    }
    String::from_utf8(bytes).map_err(|_| "output not UTF-8".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strs(a: Vec<OsString>) -> Vec<String> {
        a.into_iter()
            .map(|s| s.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn argv() {
        assert_eq!(strs(state_args()), ["tray", "state", "--json"]);
        assert_eq!(strs(toggle_args("5h")), ["toggle", "5h"]);
        assert_eq!(strs(run_args("p.g.i", false, "")), ["tray", "run", "p.g.i"]);
        assert_eq!(
            strs(run_args("p.g.i", true, "ab12")),
            [
                "tray",
                "run",
                "p.g.i",
                "--confirmed",
                "--generation",
                "ab12"
            ]
        );
    }

    #[test]
    fn id_stays_one_argument() {
        let a = run_args("a b; rm -rf /", false, "g");
        assert_eq!(a[2], "a b; rm -rf /");
        assert_eq!(a.len(), 5);
    }

    #[test]
    fn bar_beside_tray() {
        let p = bar_path(Path::new("/opt/x/bar-tray"));
        assert_eq!(p.parent(), Some(Path::new("/opt/x")));
        assert_eq!(p.file_name().unwrap(), BAR_NAME);
    }

    #[cfg(unix)]
    mod unix {
        use super::*;
        const SH: &str = "/bin/sh";
        fn sh(script: &str) -> Vec<OsString> {
            vec!["-c".into(), script.into()]
        }

        #[test]
        fn captures_stdout() {
            let out = run(
                Path::new(SH),
                &sh("printf hi"),
                true,
                Duration::from_secs(5),
            );
            assert_eq!(out.unwrap(), "hi");
        }

        #[test]
        fn nonzero_exit_is_error() {
            let e = run(Path::new(SH), &sh("exit 3"), false, Duration::from_secs(5));
            assert_eq!(e.unwrap_err(), "exit 3");
        }

        #[test]
        fn limit_kills() {
            let t = Instant::now();
            let e = run(
                Path::new(SH),
                &sh("sleep 30"),
                false,
                Duration::from_millis(100),
            );
            assert_eq!(e.unwrap_err(), "timed out");
            assert!(t.elapsed() < Duration::from_secs(5));
        }

        #[test]
        fn oversize_refused() {
            let s = format!("head -c {} /dev/zero", MAX_CAPTURE + 10);
            let e = run(Path::new(SH), &sh(&s), true, Duration::from_secs(10));
            assert_eq!(e.unwrap_err(), "output too large");
        }

        #[test]
        fn missing_binary() {
            assert!(run(
                Path::new("/nonexistent/bar"),
                &[],
                false,
                Duration::from_secs(1)
            )
            .is_err());
        }
    }
}
