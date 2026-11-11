use clap::{ArgMatches, Subcommand};
use hop_bar::plugin;
use hop_top_kit::output::{dispatch, CliError, DispatchOptions};
use serde_json::{json, Value};

use crate::cli_out;

#[derive(Subcommand)]
pub enum PluginCmd {
    /// List plugins on PATH.
    List,
    /// Show one plugin's `--ext-info`.
    Info {
        /// Placeholder name (file name without `bar-`).
        name: String,
    },
}

pub fn run(m: &ArgMatches) {
    let dirs = plugin::plugin_dirs(std::env::var_os("PATH").as_deref());
    let (leaf, data) = match m.subcommand() {
        Some(("list", l)) => {
            let rows = plugin::discover(&dirs)
                .iter()
                .map(|f| {
                    let i = plugin::info(f);
                    json!({
                        "name": f.name,
                        "version": i.version,
                        "description": i.description,
                        "path": f.path.display().to_string(),
                    })
                })
                .collect();
            (l, Value::Array(rows))
        }
        Some(("info", i)) => {
            let name = i.get_one::<String>("name").map_or("", String::as_str);
            let Some(f) = plugin::find(&dirs, name) else {
                let err = CliError::not_found(format!("no plugin `{name}` on PATH"));
                let fix = "`bar plugin list` shows the plugins found";
                cli_out::fail(i, "plugin info", cli_out::with_fix(err, fix));
            };
            let mut v = plugin::info(&f).to_json();
            v["path"] = json!(f.path.display().to_string());
            (i, v)
        }
        _ => return,
    };
    let mut out = std::io::stdout();
    if let Err(e) = dispatch(leaf, &mut out, &data, DispatchOptions::default()) {
        eprintln!("bar plugin: {e}");
        std::process::exit(2);
    }
}
