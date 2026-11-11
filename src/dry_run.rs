//! `--dry-run` on the writing subcommands: the command runs its checks
//! and works out what it would write, prints that plan and exits 0,
//! writing nothing. A refusal (an existing file, a broken config) is
//! reported as without `--dry-run`.
//!
//! The plan, under `--format json|yaml`, is one document:
//! `{"dry_run": true, "command": "<path below bar>", "changes": [...]}`
//! plus fields of the command's own (`diff`, `key`, ...). Each change is
//! `{"action": "create" | "update" | "remove", "path": "<file>"}`, with
//! `backup: true` when the old file would be backed up first and
//! `current` / `proposed` when the change switches a value. No changes:
//! nothing would be written. In text the plan is a `would <action>
//! <path>` line per change, then `dry run: nothing written`. The plan
//! goes to stdout, `-o` or not: a dry run writes no file.

use crate::cli_out;
use clap::{ArgMatches, Command};
use hop_top_kit::output::{register_output_flags, CliError, RegisterOutputFlagsOptions};
use serde_json::{json, Value};
use std::path::Path;

/// Whether `--dry-run` was given.
pub fn on(m: &ArgMatches) -> bool {
    m.try_get_one::<bool>("dry_run")
        .ok()
        .flatten()
        .copied()
        .unwrap_or(false)
}

/// A change of `action` to `path`.
pub fn change(action: &str, path: &Path) -> Value {
    json!({"action": action, "path": path.display().to_string()})
}

/// Writing `path` whole: `create` when it is missing, else `update`.
pub fn write(path: &Path) -> Value {
    change(if path.exists() { "update" } else { "create" }, path)
}

/// Writing `path` whole (`create` or `update`); an existing one is
/// backed up beside it first, which removes its oldest backups (listed
/// after the write).
pub fn write_backed_up(path: &Path) -> Vec<Value> {
    let mut c = write(path);
    if !path.exists() {
        return vec![c];
    }
    c["backup"] = json!(true);
    let target = hop_bar::rewrite::target(path).unwrap_or_else(|_| path.to_path_buf());
    let old = hop_bar::rewrite::rotated_out(&target, hop_bar::rewrite::Backups::Beside);
    std::iter::once(c)
        .chain(old.iter().map(|p| change("remove", p)))
        .collect()
}

/// Prints the plan of `cmd`: `changes`, and the fields of `extra` (an
/// object). `pre` is text shown before the change lines in text, such
/// as a diff.
pub fn emit(m: &ArgMatches, cmd: &str, changes: Vec<Value>, extra: Value, pre: &str) {
    let mut text = String::from(pre);
    for c in &changes {
        let s = |k: &str| c[k].as_str().unwrap_or_default().to_string();
        text.push_str(&format!("would {} {}", s("action"), s("path")));
        if c["current"].is_string() {
            text.push_str(&format!(" ({} -> {})", s("current"), s("proposed")));
        }
        if c["backup"] == true {
            text.push_str(" (old file backed up)");
        }
        text.push('\n');
    }
    if changes.is_empty() {
        text.push_str("nothing to change\n");
    }
    text.push_str("dry run: nothing written\n");
    let mut v = json!({"dry_run": true, "command": cmd, "changes": changes});
    if let (Some(o), Value::Object(extra)) = (v.as_object_mut(), extra) {
        o.extend(extra);
    }
    // stdout even with `-o`: a dry run writes no file
    let flags = register_output_flags(Command::new("bar"), RegisterOutputFlagsOptions::default()).0;
    let to_stdout = flags
        .try_get_matches_from(["bar", "--format", cli_out::format(m)])
        .unwrap_or_else(|e| cli_out::fail(m, cmd, CliError::usage(e.to_string())));
    cli_out::emit(&to_stdout, cmd, &text, &v);
}

/// `bar install --dry-run`: the settings change and the diff; refusals
/// as without `--dry-run`, and no consent asked.
pub fn install(m: &ArgMatches, env: &hop_bar::install::Env, o: &hop_bar::install::Opts) {
    let p = hop_bar::install::preview(env, o)
        .unwrap_or_else(|e| cli_out::fail(m, "install", CliError::generic(e)));
    let changes = p
        .changes
        .iter()
        .map(|(action, path, backup)| {
            let mut c = change(action, path);
            if *backup {
                c["backup"] = json!(true);
            }
            c
        })
        .collect();
    let mut pre = String::new();
    for n in &p.notes {
        pre.push_str(n);
        pre.push('\n');
    }
    pre.push_str(&p.diff);
    let extra = json!({
        "settings": p.path.display().to_string(),
        "diff": p.diff,
        "notes": p.notes,
    });
    emit(m, "install", changes, extra, &pre);
}

/// `bar toggle --dry-run`: the view now and after, the state file and,
/// for a session, the idle sessions' toggle dirs it prunes.
pub fn toggle(m: &ArgMatches, dir: &Path, arg: &str, value: Option<&str>) {
    use hop_bar::click::{toggle_plan, view_name};
    let p = toggle_plan(dir, arg, value)
        .unwrap_or_else(|e| cli_out::fail(m, "toggle", CliError::usage(e)));
    // the real run prunes first, then writes
    let pruned = p.prune.iter().map(|d| change("remove", d));
    let changes = pruned
        .chain(p.change.iter().map(|(action, path)| {
            let mut c = change(action, path);
            c["current"] = json!(view_name(p.key, p.current));
            c["proposed"] = json!(view_name(p.key, p.proposed));
            c
        }))
        .collect();
    let extra = json!({"key": p.key, "view": view_name(p.key, p.proposed)});
    emit(m, "toggle", changes, extra, "");
}

/// `bar web plugins --dry-run`: the `-o` file (`out`; empty or `-` is
/// stdout, which writes no file). Plugins are not run.
pub fn web_plugins(m: &ArgMatches, out: &str, force: bool) {
    let changes = match out {
        "" | "-" => Vec::new(),
        path => {
            let p = Path::new(path);
            if p.exists() && !force {
                let err = CliError::conflict(format!("{path} exists; pass --force to overwrite"));
                cli_out::fail(m, "web plugins", err);
            }
            vec![write(p)]
        }
    };
    emit(m, "web plugins", changes, json!({}), "");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_is_create_or_update() {
        let dir = std::env::temp_dir();
        assert_eq!(write(&dir)["action"], "update");
        let missing = dir.join("bar-dry-run-no-such-file");
        assert_eq!(write(&missing)["action"], "create");
        assert_eq!(write(&missing)["path"], missing.display().to_string());
    }
}
