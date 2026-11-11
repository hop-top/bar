//! Windows creation flags: set in one place per crate, since
//! `CommandExt::creation_flags` replaces the flags set before it, and a
//! later call elsewhere would silently drop a flag (the process group, or
//! the hidden console). The tray and bar agree on the private console
//! mark.

use std::path::{Path, PathBuf};

/// The files that may call `creation_flags`, with how many calls each:
/// bar's child helper and its refresh spawn (with the retry without
/// breakaway), `bar tray start` and its retry, the tray's child helper,
/// and the plugin crate's `curl`.
const ALLOWED: [(&str, usize); 4] = [
    ("src/platform/windows.rs", 3),
    ("src/tray/process.rs", 2),
    ("crates/bar-tray/src/backend/spawn/tree.rs", 1),
    ("crates/bar-plugin/src/http.rs", 1),
];

fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        let name = e.file_name();
        if p.is_dir() {
            if name != "target" && name != ".git" {
                sources(&p, out);
            }
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

/// Calls outside comments (split here, so this file has none).
fn calls(text: &str) -> usize {
    let needle = concat!("creation", "_flags(");
    text.lines()
        .map(str::trim_start)
        .filter(|l| !l.starts_with("//"))
        .map(|l| l.matches(needle).count())
        .sum()
}

#[test]
fn creation_flags_are_set_only_by_the_helpers() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    for d in ["src", "crates", "plugins", "tests"] {
        sources(&root.join(d), &mut files);
    }
    assert!(files.len() > 50, "found {} sources", files.len());
    let mut found = Vec::new();
    for f in &files {
        let n = calls(&std::fs::read_to_string(f).unwrap());
        if n > 0 {
            let rel = f
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            found.push((rel, n));
        }
    }
    found.sort();
    let mut want: Vec<_> = ALLOWED.iter().map(|(f, n)| (f.to_string(), *n)).collect();
    want.sort();
    assert_eq!(found, want);
}

#[test]
fn the_tray_marks_the_console_as_bar_reads_it() {
    use bar_tray::backend::spawn;
    use hop_bar::platform::flags;
    assert_eq!(spawn::PRIVATE_CONSOLE_ENV, flags::PRIVATE_CONSOLE_ENV);
    assert_eq!(spawn::PRIVATE_CONSOLE, flags::PRIVATE_CONSOLE);
    // the mark reaches bar through the tray's environment filter only
    // because it is set after it; it is no handler variable
    assert!(!spawn::handler_var(spawn::PRIVATE_CONSOLE_ENV));
}

/// A background refresh gets a console (hidden, its own) and the mark;
/// the children of a process that claimed its console do not get the
/// mark, and still have a console.
#[cfg(windows)]
#[test]
fn a_refresh_has_a_console_and_its_children_lose_the_mark() {
    use hop_bar::platform::{self, flags::PRIVATE_CONSOLE_ENV};
    use std::process::Stdio;
    use std::time::{Duration, Instant};

    let dir = std::env::temp_dir().join(format!("bar-flags-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let out = dir.join("refresh.txt");
    let _ = std::fs::remove_file(&out);
    // `chcp` prints the console's code page, and fails without a console
    let mut c = platform::shell(&format!(
        r#"(chcp && echo mark=%{PRIVATE_CONSOLE_ENV}%) > "{}.tmp" & move /Y "{0}.tmp" "{0}" >NUL"#,
        out.display()
    ));
    c.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    platform::spawn_detached(c).unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    let text = loop {
        if let Ok(t) = std::fs::read_to_string(&out) {
            break t;
        }
        assert!(Instant::now() < deadline, "refresh child wrote nothing");
        std::thread::sleep(Duration::from_millis(50));
    };
    assert!(text.contains(':'), "no console code page: {text:?}");
    assert!(text.contains("mark=1"), "{text:?}");

    std::env::set_var(PRIVATE_CONSOLE_ENV, "1");
    platform::claim_private_console();
    let mut c = platform::shell(&format!("chcp && echo mark=%{PRIVATE_CONSOLE_ENV}%"));
    let o = platform::own_group(&mut c)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&o.stdout);
    assert!(o.status.success(), "{text:?}");
    assert!(text.contains(':'), "no console code page: {text:?}");
    assert!(!text.contains("mark=1"), "{text:?}");
    std::env::remove_var(PRIVATE_CONSOLE_ENV);
    let _ = std::fs::remove_dir_all(&dir);
}
