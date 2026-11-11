//! `bar --help --format json`: the command tree as data. Commands, args
//! and flags come from the clap definition; each command's side-effect
//! class and idempotency come from [`CONTRACTS`], which a test keeps in
//! step with the tree. The vocabulary is hop-top kit's: side effect
//! `read`, `write-local`, `write-shared` or `interactive`; idempotent
//! `yes`, `no` or `conditional`.

use clap::{ArgAction, Command};
use hop_top_kit::output::{
    dispatch, register_output_flags, DispatchOptions, RegisterOutputFlagsOptions, EXIT_USAGE,
};
use serde_json::{json, Value};
use std::ffi::OsString;

/// Version of this document's layout.
pub const SCHEMA_VERSION: &str = "1.0";

/// Side effect and idempotency of every command, by path below `bar`.
pub const CONTRACTS: &[(&str, &str, &str)] = &[
    ("render", "read", "yes"),
    ("subagent", "read", "yes"),
    ("check", "read", "yes"),
    ("preview", "read", "yes"),
    ("config export", "write-local", "yes"),
    ("config import", "write-local", "yes"),
    ("edit", "interactive", "no"),
    ("init", "write-local", "yes"),
    ("install", "write-local", "yes"),
    ("update", "read", "yes"),
    ("toggle", "write-local", "no"),
    ("click install", "write-local", "yes"),
    ("click run", "write-shared", "no"),
    ("click action", "write-shared", "no"),
    ("click hook", "write-local", "yes"),
    ("plugin list", "read", "yes"),
    ("plugin info", "read", "yes"),
    ("web plugins", "write-local", "yes"),
    ("prices update", "write-local", "yes"),
    ("refresh", "write-local", "yes"),
    ("help", "read", "yes"),
];

/// Kit's spelling for a command that declares nothing.
const UNKNOWN: &str = "unknown";

fn contract(path: &str) -> (&'static str, &'static str) {
    CONTRACTS
        .iter()
        .find(|(p, ..)| *p == path)
        .map_or((UNKNOWN, UNKNOWN), |(_, se, id)| (*se, *id))
}

/// The output format `args` ask help for, when they ask for help as data:
/// `-h`/`--help` and `--format <f>` other than `table` or `text`. Only
/// arguments before `--` count.
pub fn requested(args: &[OsString]) -> Option<String> {
    let mut help = false;
    let mut format = None;
    let mut it = args.iter().filter_map(|a| a.to_str());
    while let Some(a) = it.next() {
        match a {
            "--" => break,
            "-h" | "--help" => help = true,
            "--format" => format = it.next().map(str::to_string),
            _ => {
                if let Some(f) = a.strip_prefix("--format=") {
                    format = Some(f.to_string());
                }
            }
        }
    }
    format.filter(|f| help && !matches!(f.as_str(), "table" | "text"))
}

/// Prints the document for the command `rest` names in `format`.
pub fn print(mut root: Command, rest: &[OsString], format: &str) {
    root.build();
    let doc = document(&root, target(&root, rest));
    let flags = register_output_flags(Command::new("bar"), RegisterOutputFlagsOptions::default()).0;
    let m = flags
        .try_get_matches_from(["bar", "--format", format])
        .unwrap_or_else(|e| e.exit());
    if let Err(e) = dispatch(&m, &mut std::io::stdout(), &doc, DispatchOptions::default()) {
        eprintln!("bar: {e}");
        std::process::exit(EXIT_USAGE);
    }
}

/// The subcommand `args` name, followed from `root` as far as names match.
pub fn target<'a>(root: &'a Command, args: &[OsString]) -> &'a Command {
    let mut cmd = root;
    for a in args.iter().filter_map(|a| a.to_str()) {
        if a == "--" {
            break;
        }
        if a.starts_with('-') {
            continue;
        }
        // anything else is a flag's value or an operand
        if let Some(sub) = cmd.find_subcommand(a) {
            cmd = sub;
        }
    }
    cmd
}

/// The document for `cmd` (the root or a subcommand of `root`): every
/// runnable command at or under it, in definition order.
pub fn document(root: &Command, cmd: &Command) -> Value {
    let mut prefix = Vec::new();
    path_to(root, cmd, &mut prefix);
    let mut commands = Vec::new();
    walk(cmd, &mut prefix, &mut commands);
    json!({
        "tool": root.get_name(),
        "version": env!("CARGO_PKG_VERSION"),
        "schema_version": SCHEMA_VERSION,
        "commands": commands,
    })
}

/// Names from below `root` down to `cmd`; empty for the root.
fn path_to(root: &Command, cmd: &Command, out: &mut Vec<String>) -> bool {
    if std::ptr::eq(root, cmd) {
        return true;
    }
    for sub in root.get_subcommands() {
        out.push(sub.get_name().to_string());
        if path_to(sub, cmd, out) {
            return true;
        }
        out.pop();
    }
    false
}

/// The subcommands to list: clap's `help` of a group repeats the group's
/// tree and is left out; the root's `help` is one command.
fn subcommands<'a>(cmd: &'a Command, path: &[String]) -> Vec<&'a Command> {
    if cmd.get_name() == "help" && path.len() == 1 {
        return Vec::new();
    }
    cmd.get_subcommands()
        .filter(|s| s.get_name() != "help" || path.is_empty())
        .collect()
}

/// Runnable commands: leaves. A group (a command with subcommands) only
/// takes one of them.
fn walk(cmd: &Command, path: &mut Vec<String>, out: &mut Vec<Value>) {
    let subs = subcommands(cmd, path);
    if subs.is_empty() {
        out.push(entry(cmd, path));
        return;
    }
    for sub in subs {
        path.push(sub.get_name().to_string());
        walk(sub, path, out);
        path.pop();
    }
}

fn entry(cmd: &Command, path: &[String]) -> Value {
    let (side_effect, idempotent) = contract(&path.join(" "));
    let mut full = vec!["bar".to_string()];
    full.extend(path.iter().cloned());
    let args: Vec<Value> = cmd
        .get_positionals()
        .map(|a| {
            json!({
                "name": a.get_id().as_str(),
                "required": a.is_required_set(),
                "variadic": a.get_num_args().is_some_and(|n| n.max_values() > 1),
            })
        })
        .collect();
    let flags: Vec<Value> = cmd
        .get_arguments()
        .filter(|a| !a.is_positional() && !a.is_hide_set() && !a.is_global_set())
        .filter_map(flag)
        .collect();
    let dry_run = cmd.get_arguments().any(|a| a.get_long() == Some("dry-run"));
    json!({
        "path": full,
        "short": cmd.get_about().map(|s| s.to_string()).unwrap_or_default(),
        "side_effect": side_effect,
        "idempotent": idempotent,
        "args": args,
        "flags": flags,
        "dry_run_supported": dry_run,
        "hidden": cmd.is_hide_set(),
    })
}

fn flag(a: &clap::Arg) -> Option<Value> {
    let name = a.get_long()?;
    if matches!(name, "help" | "version") {
        return None;
    }
    let ty = match a.get_action() {
        ArgAction::SetTrue | ArgAction::SetFalse => "bool",
        ArgAction::Count => "count",
        ArgAction::Append => "list",
        _ => "string",
    };
    let mut v = json!({"name": name, "type": ty});
    if let Some(s) = a.get_short() {
        v["short"] = json!(s.to_string());
    }
    if let Some(h) = a.get_help() {
        v["description"] = json!(h.to_string());
    }
    let defaults: Vec<String> = a
        .get_default_values()
        .iter()
        .map(|d| d.to_string_lossy().into_owned())
        .collect();
    if ty == "string" && !defaults.is_empty() {
        v["default"] = json!(defaults.join(","));
    }
    if a.is_required_set() {
        v["required"] = json!(true);
    }
    let choices: Vec<String> = a
        .get_possible_values()
        .iter()
        .map(|p| p.get_name().to_string())
        .collect();
    if ty == "string" && !choices.is_empty() {
        v["choices"] = json!(choices);
    }
    Some(v)
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    fn tree() -> Command {
        let (mut cmd, _) =
            hop_top_kit::output::register_output_flags(crate::Cli::command(), Default::default());
        cmd.build();
        cmd
    }

    fn leaves(cmd: &Command, path: &mut Vec<String>, out: &mut Vec<String>) {
        let subs = subcommands(cmd, path);
        if subs.is_empty() && !path.is_empty() {
            out.push(path.join(" "));
        }
        for sub in subs {
            path.push(sub.get_name().to_string());
            leaves(sub, path, out);
            path.pop();
        }
    }

    #[test]
    fn every_command_declares_its_contract() {
        let mut found = Vec::new();
        leaves(&tree(), &mut Vec::new(), &mut found);
        assert!(found.len() > 10, "{found:?}");
        let undeclared: Vec<&String> = found.iter().filter(|p| contract(p).0 == UNKNOWN).collect();
        assert!(undeclared.is_empty(), "add to CONTRACTS: {undeclared:?}");
        // and no entry names a command that is gone
        let stale: Vec<&str> = CONTRACTS
            .iter()
            .map(|(p, ..)| *p)
            .filter(|p| !found.iter().any(|f| f == p))
            .filter(|p| cfg!(feature = "edit") || *p != "edit")
            .collect();
        assert!(stale.is_empty(), "not in the CLI: {stale:?}");
    }

    #[test]
    fn contracts_use_kit_vocabulary() {
        for (p, se, id) in CONTRACTS {
            assert!(
                ["read", "write-local", "write-shared", "interactive"].contains(se),
                "{p}: {se}"
            );
            assert!(["yes", "no", "conditional"].contains(id), "{p}: {id}");
        }
    }

    #[test]
    fn requested_needs_help_and_a_data_format() {
        let a = |v: &[&str]| v.iter().map(OsString::from).collect::<Vec<_>>();
        assert_eq!(
            requested(&a(&["--help", "--format", "json"])),
            Some("json".into())
        );
        assert_eq!(
            requested(&a(&["config", "-h", "--format=yaml"])),
            Some("yaml".into())
        );
        assert_eq!(requested(&a(&["--help"])), None);
        assert_eq!(requested(&a(&["--help", "--format", "table"])), None);
        assert_eq!(requested(&a(&["check", "--format", "json"])), None);
        assert_eq!(requested(&a(&["x", "--", "--help", "--format=json"])), None);
    }

    #[test]
    fn target_follows_subcommand_names() {
        let root = tree();
        let a = |v: &[&str]| v.iter().map(OsString::from).collect::<Vec<_>>();
        let t = target(&root, &a(&["--format", "json", "config", "--help"]));
        assert_eq!(t.get_name(), "config");
        let t = target(&root, &a(&["init", "--preset", "minimal", "-h"]));
        assert_eq!(t.get_name(), "init");
        let t = target(&root, &a(&["frobnicate", "-h"]));
        assert_eq!(t.get_name(), "bar");
    }
}
