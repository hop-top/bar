//! The optional `check` capability: `bar-<name> --check` prints what is
//! wrong with the plugin's own setup, for `bar check` to show.

use serde_json::Value;

use super::Found;
use crate::cache::run_child;
use crate::style::sanitize;
use std::time::Duration;

/// Capability a plugin declares to be asked.
pub const CHECK_CAPABILITY: &str = "check";
/// Limit for one `--check` run.
pub const CHECK_TIMEOUT: Duration = Duration::from_secs(5);
/// Most warnings taken from one plugin, and their longest length.
pub const MAX_WARNINGS: usize = 8;
const MAX_LEN: usize = 200;

/// Warnings from stdout: a JSON array of strings, or an object with a
/// `warnings` array of strings. Each is sanitised and cut; anything else
/// is ignored.
pub fn parse_warnings(raw: &str) -> Vec<String> {
    let v: Value = serde_json::from_str(raw.trim()).unwrap_or(Value::Null);
    let list = match &v {
        Value::Array(a) => a.as_slice(),
        Value::Object(_) => v["warnings"].as_array().map_or(&[][..], Vec::as_slice),
        _ => &[],
    };
    list.iter()
        .filter_map(Value::as_str)
        .map(|s| sanitize(s).trim().chars().take(MAX_LEN).collect::<String>())
        .filter(|s| !s.is_empty())
        .take(MAX_WARNINGS)
        .collect()
}

/// Runs `--check` now (5 s limit, no stdin). Empty when it fails or says
/// nothing; a plugin whose check cannot run has nothing to report.
pub fn run(found: &Found) -> Vec<String> {
    let mut c = std::process::Command::new(&found.path);
    c.arg("--check");
    run_child(c, None, CHECK_TIMEOUT).map_or_else(Vec::new, |o| parse_warnings(&o))
}
