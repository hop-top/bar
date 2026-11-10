//! Command aliases: `bar <alias> <args>` runs the plugin that declares
//! `alias` in its `--ext-info` (`aliases`), when no plugin is named so.

use super::{discover, find, Found};
use std::path::PathBuf;

/// Most aliases a plugin can declare.
pub const MAX_ALIASES: usize = 4;
/// Longest alias, in characters.
pub const MAX_ALIAS_LEN: usize = 16;

/// Alias names: `[a-z0-9-]`, 1 to 16.
pub fn valid_alias(s: &str) -> bool {
    (1..=MAX_ALIAS_LEN).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// How a command name resolved.
#[derive(Debug, PartialEq, Eq)]
pub enum Resolved {
    /// The plugin to run.
    One(Found),
    /// Several plugins declare the alias: their names.
    Ambiguous(Vec<String>),
    Unknown,
}

/// The plugin for `bar <name>`: the plugin `bar-<name>` when there is
/// one, else the one plugin whose `aliases_of` holds `name`. `aliases_of`
/// is asked only when no plugin is named `name`.
pub fn resolve(
    dirs: &[PathBuf],
    name: &str,
    aliases_of: &dyn Fn(&Found) -> Vec<String>,
) -> Resolved {
    if let Some(f) = find(dirs, name) {
        return Resolved::One(f);
    }
    if !valid_alias(name) {
        return Resolved::Unknown;
    }
    let mut hits: Vec<Found> = discover(dirs)
        .into_iter()
        .filter(|f| aliases_of(f).iter().any(|a| a == name))
        .collect();
    match hits.len() {
        0 => Resolved::Unknown,
        1 => Resolved::One(hits.remove(0)),
        _ => Resolved::Ambiguous(hits.into_iter().map(|f| f.name).collect()),
    }
}
