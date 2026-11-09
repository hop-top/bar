//! Project config: a `.bar.conf` found from the session's directory and
//! applied over the user config, and what it is allowed to do.

use super::{layout_key, parse_options, Config};
use crate::expand::has_expansion;
use crate::format::{self, Token};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// File name of a project config.
pub const FILE_NAME: &str = ".bar.conf";

/// Most directories climbed looking for the git root.
const MAX_DEPTH: usize = 64;

/// Largest project config read.
pub const MAX_BYTES: u64 = 64 * 1024;

/// `set project-config`: whether a project config is read, and whether it
/// may run commands and plugins.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProjectMode {
    /// Never looked for.
    Off,
    /// Applied, minus `%…%` values, `plugin` lines and plugins the user
    /// config does not use.
    #[default]
    Safe,
    /// Applied as if it were part of the user config.
    Full,
}

impl ProjectMode {
    pub const NAMES: [&'static str; 3] = ["off", "safe", "full"];

    pub fn parse(s: &str) -> Option<ProjectMode> {
        Some(match s {
            "off" => ProjectMode::Off,
            "safe" => ProjectMode::Safe,
            "full" => ProjectMode::Full,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            ProjectMode::Off => "off",
            ProjectMode::Safe => "safe",
            ProjectMode::Full => "full",
        }
    }
}

/// The project config for a session in `start`: the nearest `.bar.conf`
/// from `start` up to the git root (the first directory holding a `.git`
/// entry), both included. Outside a git repository only `start` itself
/// counts. Only stats, at most two per directory climbed.
pub fn find(start: &Path) -> Option<PathBuf> {
    if !start.is_absolute() {
        return None;
    }
    let own = start.join(FILE_NAME);
    if own.is_file() {
        return Some(own);
    }
    let mut found = None;
    for (i, d) in start.ancestors().take(MAX_DEPTH).enumerate() {
        if i > 0 && found.is_none() {
            let p = d.join(FILE_NAME);
            if p.is_file() {
                found = Some(p);
            }
        }
        if std::fs::symlink_metadata(d.join(".git")).is_ok() {
            return found;
        }
    }
    None
}

/// The repository root of a project config at `path`: the first directory
/// up from it holding a `.git` entry, else its own directory.
fn root_of(path: &Path) -> Option<&Path> {
    let dir = path.parent()?;
    let root = dir
        .ancestors()
        .take(MAX_DEPTH)
        .find(|d| std::fs::symlink_metadata(d.join(".git")).is_ok());
    Some(root.unwrap_or(dir))
}

/// Reads a project config found by [`find`] as the current user.
pub fn read(path: &Path) -> Result<String, String> {
    read_as(path, crate::platform::current_uid())
}

/// Reads a project config found by [`find`], refusing one that may not
/// be the repository's: a symlink to a file outside the repository, a
/// file owned by a user other than `uid` (`None`: not checked), or a
/// file over [`MAX_BYTES`].
pub fn read_as(path: &Path, uid: Option<u32>) -> Result<String, String> {
    use std::io::Read;
    let shown = path.display();
    let link = std::fs::symlink_metadata(path).map_err(|e| format!("{shown}: {e}"))?;
    if link.file_type().is_symlink() {
        let real = std::fs::canonicalize(path).map_err(|e| format!("{shown}: {e}"))?;
        let root = root_of(path).and_then(|r| std::fs::canonicalize(r).ok());
        if !root.is_some_and(|r| real.starts_with(r)) {
            return Err(format!(
                "{shown}: a symlink to a file outside the repository"
            ));
        }
    }
    let f = std::fs::File::open(path).map_err(|e| format!("{shown}: {e}"))?;
    let meta = f.metadata().map_err(|e| format!("{shown}: {e}"))?;
    if let (Some(uid), Some(owner)) = (uid, crate::platform::owner(&meta)) {
        if owner != uid {
            return Err(format!("{shown}: owned by another user"));
        }
    }
    let too_big = || format!("{shown}: larger than 64 KiB");
    if meta.len() > MAX_BYTES {
        return Err(too_big());
    }
    let mut buf = Vec::new();
    f.take(MAX_BYTES + 1)
        .read_to_end(&mut buf)
        .map_err(|e| format!("{shown}: {e}"))?;
    if buf.len() as u64 > MAX_BYTES {
        return Err(too_big());
    }
    String::from_utf8(buf).map_err(|_| format!("{shown}: not UTF-8"))
}

/// Plugin placeholders the user config names: in a layout, a `default`
/// line or a `plugin` line.
pub fn user_plugins(c: &Config) -> BTreeSet<String> {
    let mut names: BTreeSet<String> = c.defaults.keys().chain(c.plugins.keys()).cloned().collect();
    for src in c.lines().iter().flatten() {
        for t in format::parse(src).unwrap_or_default() {
            if let Token::Placeholder { name, .. } = t {
                names.insert(name);
            }
        }
    }
    names.retain(|n| crate::placeholders::lookup(n).is_none());
    names
}

const NEEDS_FULL: &str = "in a project config needs `set project-config full` in the user config";

/// Refuses a project config line that `mode` does not allow. `used` is
/// `user_plugins` of the user config.
pub(super) fn guard(
    mode: ProjectMode,
    t: &[String],
    used: &BTreeSet<String>,
) -> Result<(), String> {
    if t[0] == "set" && t.get(1).map(String::as_str) == Some("project-config") {
        return Err("`project-config` is read from the user config only".into());
    }
    if mode == ProjectMode::Full {
        return Ok(());
    }
    let expansion = |v: &str| {
        if has_expansion(v) {
            Err(format!("`{v}`: a `%…%` value {NEEDS_FULL}"))
        } else {
            Ok(())
        }
    };
    match t[0].as_str() {
        "plugin" => Err(format!("a `plugin` line {NEEDS_FULL}")),
        // network use is opted into by the user
        "set" if t.get(1).map(String::as_str) == Some("update-check") => {
            Err(format!("`set update-check` {NEEDS_FULL}"))
        }
        // which transcripts and accounts are read is the user's choice
        "set" if t.get(1).map(String::as_str) == Some("claude-dir") => {
            Err(format!("`set claude-dir` {NEEDS_FULL}"))
        }
        "default" if t.len() >= 2 => {
            let opts = parse_options(&t[2..]).unwrap_or_default();
            let r = opts.iter().try_for_each(|(_, v)| expansion(v));
            r
        }
        "set" if t.len() == 3 && matches!(layout_key(&t[1]), Some(Ok(_))) => {
            for tok in format::parse(&t[2]).unwrap_or_default() {
                let Token::Placeholder { name, options } = tok else {
                    continue;
                };
                if crate::placeholders::lookup(&name).is_none() && !used.contains(&name) {
                    return Err(format!(
                        "plugin `{name}` is not in the user config; using it {NEEDS_FULL}"
                    ));
                }
                options.iter().try_for_each(|(_, v)| expansion(v))?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

/// Drops user `rule` and `account` entries (the first `rules` and
/// `accounts`) that a project entry with the same target or glob
/// replaces; both lists are first-match.
pub(super) fn replace_overridden(c: &mut Config, rules: usize, accounts: usize) {
    let targets: Vec<String> = c.rules[rules..].iter().map(|r| r.target.clone()).collect();
    let mut i = 0;
    c.rules.retain(|r| {
        i += 1;
        i > rules || !targets.contains(&r.target)
    });
    let globs: Vec<String> = c.accounts[accounts..]
        .iter()
        .map(|a| a.glob.clone())
        .collect();
    let mut i = 0;
    c.accounts.retain(|a| {
        i += 1;
        i > accounts || !globs.contains(&a.glob)
    });
}
