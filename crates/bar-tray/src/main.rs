//! `bar-tray [--snapshot <file>] [--bar <path>] [--poll <seconds>] [--lock <file>] [--print]`
//!
//! Shows bar's menu in the menu bar or notification area. The menu comes
//! from `bar tray state --json` (`bar` beside this binary, or `--bar`),
//! or from a snapshot file with `--snapshot`, for showing a menu without
//! `bar`. `--print` prints the menu once instead of showing it. With
//! `--lock`, a tray that finds the file locked by another one exits 0
//! at once (`bar tray install` and `bar tray start` pass it).
//! `--backend` prints the tray backend this build has and exits, 1 when
//! it has none (Linux without the `linux-sni` feature).

// A tray has no console: the GUI subsystem keeps one from flashing up.
#![cfg_attr(windows, windows_subsystem = "windows")]

use bar_tray::backend::source::Source;
use bar_tray::backend::spawn;
use bar_tray::model::{Kind, Menu, MenuItem};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

const USAGE: &str =
    "usage: bar-tray [--snapshot <file>] [--bar <path>] [--poll <seconds>] [--lock <file>] [--print] [--backend]";
/// Seconds between idle polls unless `--poll` says otherwise.
const DEFAULT_POLL: u64 = 30;

#[derive(Debug, Default, PartialEq, Eq)]
struct Args {
    snapshot: Option<PathBuf>,
    bar: Option<PathBuf>,
    poll: Option<u64>,
    lock: Option<PathBuf>,
    print: bool,
    backend: bool,
}

fn parse(argv: impl IntoIterator<Item = String>) -> Result<Args, String> {
    let mut a = Args::default();
    let mut it = argv.into_iter();
    while let Some(arg) = it.next() {
        let mut value = |flag: &str| it.next().ok_or_else(|| format!("{flag} needs a value"));
        match arg.as_str() {
            "--snapshot" => a.snapshot = Some(PathBuf::from(value("--snapshot")?)),
            "--bar" => a.bar = Some(PathBuf::from(value("--bar")?)),
            "--poll" => {
                let v = value("--poll")?;
                a.poll = Some(
                    v.parse()
                        .ok()
                        .filter(|s| (10..=600).contains(s))
                        .ok_or_else(|| format!("invalid --poll `{v}` (10..600 seconds)"))?,
                );
            }
            "--lock" => a.lock = Some(PathBuf::from(value("--lock")?)),
            "--print" => a.print = true,
            "--backend" => a.backend = true,
            other => return Err(format!("unknown argument `{other}`")),
        }
    }
    Ok(a)
}

fn main() -> ExitCode {
    let args = match parse(std::env::args().skip(1)) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("bar-tray: {e}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    if args.backend {
        return backend();
    }
    // `bar` is needed for clicks, and for the snapshot without `--snapshot`
    let bar = match (&args.snapshot, &args.bar) {
        (Some(_), None) => None,
        (_, explicit) => match spawn::locate(explicit.as_deref()) {
            Ok(p) => Some(p),
            Err(e) => {
                eprintln!("bar-tray: {e}");
                return ExitCode::FAILURE;
            }
        },
    };
    let source = match (&args.snapshot, &bar) {
        (Some(p), _) => Source::File(p.clone()),
        (None, Some(b)) => Source::Bar(b.clone()),
        (None, None) => unreachable!("no snapshot means a located bar"),
    };
    if args.print {
        return print(&source);
    }
    // held until the process ends; the OS releases it however that happens
    let _lock = match args.lock.as_deref().map(bar_tray::lock::acquire) {
        None => None,
        Some(Ok(Some(f))) => Some(f),
        Some(Ok(None)) => {
            eprintln!("bar-tray: already running");
            return ExitCode::SUCCESS;
        }
        Some(Err(e)) => {
            eprintln!("bar-tray: lock: {e}");
            return ExitCode::FAILURE;
        }
    };
    show(
        source,
        bar,
        Duration::from_secs(args.poll.unwrap_or(DEFAULT_POLL)),
    )
}

#[cfg(target_os = "macos")]
fn show(source: Source, bar: Option<PathBuf>, poll: Duration) -> ExitCode {
    bar_tray::backend::macos::run(bar_tray::backend::Options { source, bar, poll })
}

#[cfg(windows)]
fn show(source: Source, bar: Option<PathBuf>, poll: Duration) -> ExitCode {
    bar_tray::backend::windows::run(bar_tray::backend::Options { source, bar, poll })
}

#[cfg(all(target_os = "linux", feature = "linux-sni"))]
fn show(source: Source, bar: Option<PathBuf>, poll: Duration) -> ExitCode {
    bar_tray::backend::linux::run(bar_tray::backend::Options { source, bar, poll })
}

#[cfg(not(any(
    target_os = "macos",
    windows,
    all(target_os = "linux", feature = "linux-sni")
)))]
fn show(_source: Source, _bar: Option<PathBuf>, _poll: Duration) -> ExitCode {
    eprintln!("bar-tray: no tray backend for this platform yet; --print shows the menu");
    ExitCode::FAILURE
}

/// The backend `show` runs in this build.
const BACKEND: Option<&str> = if cfg!(target_os = "macos") {
    Some("macos")
} else if cfg!(windows) {
    Some("windows")
} else if cfg!(all(target_os = "linux", feature = "linux-sni")) {
    Some("sni")
} else {
    None
};

fn backend() -> ExitCode {
    match BACKEND {
        Some(b) => {
            println!("{b}");
            ExitCode::SUCCESS
        }
        None => {
            eprintln!("bar-tray: built without a tray backend for this platform");
            ExitCode::FAILURE
        }
    }
}

fn print(source: &Source) -> ExitCode {
    let snapshot = match source.fetch() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("bar-tray: {e}");
            return ExitCode::FAILURE;
        }
    };
    for w in &snapshot.warnings {
        eprintln!("bar-tray: warning: {w}");
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    print!("{}", tree(&Menu::build(&snapshot, now)));
    ExitCode::SUCCESS
}

/// One line per row: `[x]`/`[ ]` for toggles and choices, `>` for a
/// submenu, `---` for a separator, then the label, detail and flags.
fn tree(m: &Menu) -> String {
    let mut out = String::new();
    for (row, depth) in m.flatten() {
        out.push_str(&"    ".repeat(depth));
        out.push_str(&line(row));
        out.push('\n');
    }
    out
}

fn line(row: &MenuItem) -> String {
    let mark = match row.kind {
        Kind::Separator => return "---".to_string(),
        Kind::Toggle | Kind::Choice if row.checked => "[x] ",
        Kind::Toggle | Kind::Choice => "[ ] ",
        Kind::Submenu(_) => "> ",
        Kind::Info | Kind::Action => "",
    };
    let mut s = format!("{mark}{}", row.label);
    if let Some(d) = &row.detail {
        s.push_str(&format!(" — {d}"));
    }
    let mut flags = Vec::new();
    if row.confirm {
        flags.push("confirm".to_string());
    }
    if let Some(st) = row.style {
        flags.push(format!("{st:?}").to_lowercase());
    }
    if !row.enabled && !matches!(row.kind, Kind::Info | Kind::Submenu(_)) {
        flags.push("disabled".to_string());
    }
    if !flags.is_empty() {
        s.push_str(&format!("  ({})", flags.join(", ")));
    }
    format!("{s}  #{}", row.id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bar_tray::model;

    fn args(a: &[&str]) -> Result<Args, String> {
        parse(a.iter().map(|s| s.to_string()))
    }

    #[test]
    fn flags() {
        assert_eq!(args(&[]).unwrap(), Args::default());
        let a = args(&[
            "--snapshot",
            "s.json",
            "--bar",
            "/x/bar",
            "--poll",
            "45",
            "--lock",
            "/s/tray.lock",
            "--print",
        ])
        .unwrap();
        assert_eq!(
            a,
            Args {
                snapshot: Some("s.json".into()),
                bar: Some("/x/bar".into()),
                poll: Some(45),
                lock: Some("/s/tray.lock".into()),
                print: true,
                backend: false,
            }
        );
        assert!(args(&["--backend"]).unwrap().backend);
        assert_eq!(args(&["--poll"]).unwrap_err(), "--poll needs a value");
        assert_eq!(args(&["--lock"]).unwrap_err(), "--lock needs a value");
        assert_eq!(
            args(&["--poll", "5"]).unwrap_err(),
            "invalid --poll `5` (10..600 seconds)"
        );
        assert!(args(&["--poll", "x"]).is_err());
        assert_eq!(args(&["s.json"]).unwrap_err(), "unknown argument `s.json`");
    }

    #[test]
    fn tree_lines() {
        let s = model::parse(include_str!("../tests/fixtures/snapshot.json")).unwrap();
        let t = tree(&Menu::build(&s, 1_762_800_000));
        let lines: Vec<&str> = t.lines().collect();
        assert_eq!(lines[0], "> Claude · :8317  #cliproxyapi.claude@8317");
        assert_eq!(
            lines[1],
            "    [x] alice — 5h 59% ↻2h10m · 7d 71%  (good)  #cliproxyapi.claude@8317.3"
        );
        assert_eq!(
            lines[5],
            "Use soonest reset  (confirm)  #cliproxyapi.next@8317.next-soon"
        );
        assert_eq!(
            lines[6],
            "management refused the key  (bad)  #cliproxyapi.info.auth"
        );
        assert_eq!(lines[7], "---");
        assert_eq!(lines[9], "[ ] Routing :8318 — not enabled: tray routellm.toggle  (disabled)  #routellm.rtellm@8317.toggle@8318");
        assert_eq!(lines[lines.len() - 1], "Quit  #tray.quit");
    }
}
