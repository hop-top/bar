use clap::ArgMatches;
use hop_bar::click;
use hop_top_kit::output::CliError;
use serde_json::json;

use crate::{cli_out, dry_run};

pub fn run(m: &ArgMatches) {
    use hop_bar::install::{self, Error, Scope};
    cli_out::check_format(m, "install");
    let scope = if m.get_flag("project") {
        Scope::Project
    } else if m.get_flag("local") {
        Scope::Local
    } else {
        Scope::User
    };
    let cwd = std::env::current_dir().unwrap_or_default();
    let settings = install::settings_path(scope, click::claude_dir(), &cwd)
        .unwrap_or_else(|e| cli_out::fail(m, "install", CliError::usage(e)));
    let env = install::Env {
        bar: install::bar_for(
            scope,
            std::env::current_exe().ok().map(|e| {
                install::command_bar(&e, std::env::var_os("PATH").as_deref(), cfg!(windows))
            }),
        ),
        settings,
        state_dir: click::state_dir(),
        windows: cfg!(windows),
    };
    let opts = install::Opts {
        uninstall: m.get_flag("uninstall"),
        refresh: m.get_one::<u64>("refresh").copied(),
        hooks: m.get_flag("hooks"),
        subagent: m.get_flag("subagent"),
        subagent_only: m.get_flag("subagent_only"),
        yes: m.get_flag("yes"),
    };
    if dry_run::on(m) {
        return dry_run::install(m, &env, &opts);
    }
    let tty = std::io::IsTerminal::is_terminal(&std::io::stdin());
    let r = install::run_outcome(
        &env,
        &opts,
        tty,
        &mut std::io::stdin().lock(),
        &mut cli_out::human(m),
    );
    match r {
        Ok(o) => {
            let data = json!({
                "settings": o.path.display().to_string(),
                "written": o.written,
                "backup": o.backup.map(|b| b.display().to_string()),
            });
            cli_out::emit(m, "install", "", &data);
        }
        Err(e) => {
            let err = match e {
                Error::NeedsConfirm | Error::Declined => cli_out::with_fix(
                    CliError::consent_refused(e.to_string()),
                    "rerun with --yes to write",
                ),
                Error::Failed(_) => CliError::generic(e.to_string()),
            };
            cli_out::fail(m, "install", err);
        }
    }
}
