//! `bar config export` / `bar config import`: the effective config as one
//! file, and merging or replacing the user config with a file.

use crate::conf::{layout_key, tokenize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// What a line sets, when a later line with the same key replaces it:
/// a `set` key (`left` and `left@1` are one), a `set price` prefix, a
/// `plugin` name, a `rule` target or an `account` glob. `default` lines
/// merge and have none.
fn line_key(raw: &str) -> Option<String> {
    let t = tokenize(raw).ok()?;
    let (head, arg) = (t.first()?.as_str(), t.get(1)?.as_str());
    Some(match head {
        "set" if arg == "price" => format!("price {}", t.get(2)?),
        "set" => match layout_key(arg) {
            Some(Ok((group, line))) => format!("set {group}@{line}"),
            _ => format!("set {arg}"),
        },
        "plugin" | "rule" | "account" => format!("{head} {arg}"),
        _ => return None,
    })
}

fn with_newline(s: &str) -> String {
    if s.is_empty() || s.ends_with('\n') {
        s.to_string()
    } else {
        format!("{s}\n")
    }
}

/// `over` applied over `base`, as one file: the lines of `base` that a
/// line of `over` replaces are dropped, then `over` follows. Reads the same
/// as `base` then `over` layered.
pub fn merge(base: &str, over: &str) -> String {
    let keys: HashSet<String> = over.lines().filter_map(line_key).collect();
    let kept: String = base
        .lines()
        .filter(|l| line_key(l).is_none_or(|k| !keys.contains(&k)))
        .map(|l| format!("{l}\n"))
        .collect();
    format!("{kept}{}", with_newline(over))
}

/// `text` without its lines numbered (1-based) in `skip`.
pub fn without_lines(text: &str, skip: &[usize]) -> String {
    text.lines()
        .enumerate()
        .filter(|(i, _)| !skip.contains(&(i + 1)))
        .map(|(_, l)| format!("{l}\n"))
        .collect()
}

/// The effective config as one file: the user config (`None`: the
/// built-in layout) and, when given, the project config merged over it,
/// each under a comment naming its source.
pub fn effective(user: Option<(&Path, &str)>, project: Option<(&Path, &str)>) -> String {
    let base = match user {
        Some((p, text)) => format!("# bar config: {}\n{}", p.display(), with_newline(text)),
        None => format!(
            "# bar config: built-in layout\n{}",
            crate::render::BUILTIN_CONF
        ),
    };
    match project {
        None => base,
        Some((p, text)) => merge(
            &base,
            &format!("# project config: {}\n{}", p.display(), with_newline(text)),
        ),
    }
}

/// Lines of `text` worth a look before importing it, with their 1-based
/// numbers and what they do: `%…%` values (run a command or read an
/// environment variable), `plugin` lines and `set project-config`. Each
/// description is shown through [`crate::diff::visible`].
pub fn notable(text: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    for (i, raw) in text.lines().enumerate() {
        let Ok(t) = tokenize(raw) else { continue };
        let what = match t.first().map(String::as_str) {
            Some("plugin") => Some(format!("{} (runs a plugin)", t.join(" "))),
            Some("set") if t.get(1).map(String::as_str) == Some("project-config") => {
                Some(format!("{} (what project configs may do)", t.join(" ")))
            }
            _ => {
                let vals: Vec<&str> = t
                    .iter()
                    .skip(1)
                    .map(String::as_str)
                    .filter(|v| crate::expand::has_expansion(v))
                    .collect();
                (!vals.is_empty()).then(|| {
                    format!(
                        "{} (runs a command or reads the environment)",
                        vals.join(" ")
                    )
                })
            }
        };
        if let Some(w) = what {
            out.push((i + 1, crate::diff::visible(&w)));
        }
    }
    out
}

/// How an import changes the user config.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Merge,
    Replace,
}

/// The user config after importing `text`: `old` is the current file,
/// `None` when there is none.
pub fn imported(old: Option<&str>, text: &str, mode: Mode) -> String {
    match (mode, old) {
        (Mode::Merge, Some(old)) => merge(old, text),
        _ => with_newline(text),
    }
}

/// Writes `body` to `path` through [`crate::rewrite`]: through a symlink,
/// old file backed up, atomic, mode kept. `expected` is the text the file
/// was read as (`None`: missing); a file changed since is refused.
/// Returns the backup path when there was a file.
pub fn write_config(
    path: &Path,
    expected: Option<&str>,
    body: &str,
) -> Result<Option<PathBuf>, String> {
    let target = crate::rewrite::target(path)?;
    crate::rewrite::unchanged(&target, expected)?;
    crate::rewrite::replace(&target, body, crate::rewrite::Backups::Beside)
        .map_err(|e| format!("{}: {e}", path.display()))
}
