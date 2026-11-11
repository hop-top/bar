//! Output and errors of the CLI subcommands. Under the default format a
//! command prints text and its errors go to stderr. Under any other
//! `--format` stdout carries one document: the result through the
//! formatter, or, with `json` or `yaml`, the error envelope (kit's
//! `code`, `message`, `cause`, `suggested_fix`, `exit_code`,
//! `transience`); text meant for a person then goes to stderr. The exit
//! code is the envelope's.

use clap::{ArgMatches, Command};
use hop_top_kit::output::{
    default_registry, dispatch, render_error, CliError, DispatchOptions, EXIT_USAGE,
};
use serde_json::Value;
use std::ffi::OsString;
use std::io::Write;

/// Format of the default, human output.
const TEXT: &str = "table";

/// The `--format` asked for.
pub fn format(m: &ArgMatches) -> &str {
    m.get_one::<String>("format").map_or(TEXT, String::as_str)
}

/// Whether stdout carries data rather than text.
pub fn data(m: &ArgMatches) -> bool {
    format(m) != TEXT
}

/// Where text meant for a person goes: stdout, or stderr when stdout
/// carries data.
pub fn human(m: &ArgMatches) -> Box<dyn Write> {
    if data(m) {
        Box::new(std::io::stderr())
    } else {
        Box::new(std::io::stdout())
    }
}

/// Exits with a usage error unless `--format` names a known format, so a
/// command never acts and then fails to report.
pub fn check_format(m: &ArgMatches, cmd: &str) {
    let keys = default_registry().keys();
    let f = format(m);
    if !keys.contains(&f) {
        let mut keys = keys;
        keys.sort_unstable();
        let msg = format!("unknown output format '{f}' (valid: {})", keys.join(", "));
        fail_as(TEXT, cmd, CliError::usage(msg));
    }
}

/// Prints a command's result: `text` under the default format, `value`
/// through the formatter otherwise.
pub fn emit(m: &ArgMatches, cmd: &str, text: &str, value: &Value) {
    let mut out = std::io::stdout();
    if !data(m) {
        let _ = out.write_all(text.as_bytes());
    } else if let Err(e) = dispatch(m, &mut out, value, DispatchOptions::default()) {
        fail(m, cmd, CliError::usage(e.to_string()));
    }
}

/// Reports `err` per `--format` and exits with its code.
pub fn fail(m: &ArgMatches, cmd: &str, err: CliError) -> ! {
    fail_as(format(m), cmd, err)
}

/// Reports `err` for format `fmt` and exits with its code. In text the
/// cause comes first, then `bar <cmd>: <message>`, then the fix when the
/// message does not already say it.
pub fn fail_as(fmt: &str, cmd: &str, err: CliError) -> ! {
    if matches!(fmt, "json" | "yaml") {
        let mut out = std::io::stdout();
        let _ = render_error(&mut out, fmt, &err);
        let _ = out.flush();
    } else {
        let mut e = std::io::stderr().lock();
        if !err.cause.is_empty() {
            let _ = writeln!(e, "{}", err.cause.trim_end());
        }
        let name = if cmd.is_empty() {
            "bar".into()
        } else {
            format!("bar {cmd}")
        };
        let _ = writeln!(e, "{name}: {}", err.message);
        if !err.suggested_fix.is_empty() && !err.message.contains(&err.suggested_fix) {
            let _ = writeln!(e, "{}", err.suggested_fix);
        }
    }
    std::process::exit(err.exit_code)
}

/// `err` with a suggested fix.
pub fn with_fix(mut err: CliError, fix: &str) -> CliError {
    err.suggested_fix = fix.into();
    err
}

/// The `--format` value in raw arguments, for the paths clap did not
/// parse (an unknown command, a usage error).
pub fn format_in(args: &[OsString]) -> &str {
    let mut it = args.iter().filter_map(|a| a.to_str());
    while let Some(a) = it.next() {
        if a == "--" {
            break;
        }
        if let Some(v) = a.strip_prefix("--format=") {
            return v;
        }
        if a == "--format" {
            return it.next().unwrap_or(TEXT);
        }
    }
    TEXT
}

/// Parses the arguments. A usage error under `--format json|yaml` becomes
/// the error envelope; help, version and text-format errors are clap's.
pub fn matches(cmd: Command) -> ArgMatches {
    let args: Vec<OsString> = std::env::args_os().collect();
    match cmd.try_get_matches_from(&args) {
        Ok(m) => m,
        Err(e) => {
            let fmt = format_in(args.get(1..).unwrap_or_default());
            if !e.use_stderr() || !matches!(fmt, "json" | "yaml") {
                e.exit()
            }
            let text = e.render().to_string();
            let msg = text
                .lines()
                .next()
                .unwrap_or_default()
                .trim_start_matches("error: ");
            let mut err = CliError::usage(msg);
            err.cause = text.trim_end().into();
            err.exit_code = EXIT_USAGE;
            fail_as(
                fmt,
                "",
                with_fix(err, "run the command with --help for its usage"),
            )
        }
    }
}
