use clap::ArgMatches;
use hop_bar::click;
use hop_top_kit::output::CliError;
use serde_json::json;

use crate::{cli_out, dry_run};

pub fn run(m: &ArgMatches) {
    cli_out::check_format(m, "toggle");
    let arg = m.get_one::<String>("key").map_or("", String::as_str);
    let value = m.get_one::<String>("value").map(String::as_str);
    if dry_run::on(m) {
        return dry_run::toggle(m, &click::handler_state_dir(), arg, value);
    }
    match click::toggle_cli(&click::handler_state_dir(), arg, value) {
        Ok((key, view)) => {
            let data = json!({"key": key, "view": view});
            cli_out::emit(m, "toggle", &format!("{key}: {view}\n"), &data);
        }
        Err(e) => cli_out::fail(m, "toggle", CliError::usage(e)),
    }
}
