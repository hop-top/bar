use std::ffi::OsString;

use hop_bar::cache::Cache;
use hop_bar::{click, plugin};
use hop_top_kit::output::{CliError, EXIT_USAGE};

use crate::cli_out;

/// `bar <name> <args>`: runs the plugin `bar-<name>` (found as a render
/// finds one) with `args`, stdio inherited, and exits with its status. A
/// name that is no plugin but is an alias a plugin declares runs that
/// plugin; any other name is a usage error, as any unknown command,
/// reported per the `--format` before `name` or among `args`.
pub fn run(name: &str, args: &[OsString], format: &str) -> ! {
    let dirs = plugin::plugin_dirs(std::env::var_os("PATH").as_deref());
    let cache = Cache::new(&click::state_dir());
    let found = match plugin::resolve(&dirs, name, &|f| plugin::info_now(&cache, f).aliases) {
        plugin::Resolved::One(f) => f,
        plugin::Resolved::Ambiguous(names) => {
            eprintln!(
                "bar: `{name}` is an alias of several plugins ({}); run one as `bar <plugin> ...`",
                names.join(", ")
            );
            std::process::exit(EXIT_USAGE);
        }
        plugin::Resolved::Unknown => {
            let err = CliError::usage(format!("unrecognized command `{name}`"));
            let fix = "see `bar --help`; `bar plugin list` shows the plugins found";
            let fmt = match cli_out::format_in(args) {
                "table" => format,
                f => f,
            };
            cli_out::fail_as(fmt, "", cli_out::with_fix(err, fix));
        }
    };
    let mut c = std::process::Command::new(&found.path);
    c.args(args);
    // the user's `account` rules, for a plugin that declared it wants them
    if plugin::info_now(&cache, &found).inputs.accounts {
        let rules = plugin::load_account_rules(None);
        if rules.as_array().is_some_and(|a| !a.is_empty()) {
            c.env(plugin::ACCOUNTS_ENV, rules.to_string());
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let e = c.exec();
        eprintln!("bar {name}: {}: {e}", found.path.display());
        std::process::exit(126);
    }
    #[cfg(not(unix))]
    match c.status() {
        Ok(st) => std::process::exit(st.code().unwrap_or(1)),
        Err(e) => {
            eprintln!("bar {name}: {}: {e}", found.path.display());
            std::process::exit(126);
        }
    }
}
