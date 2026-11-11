use std::io::Write;

use clap::{ArgMatches, Subcommand};
use hop_bar::cache::Cache;
use hop_bar::{click, plugin};
use hop_top_kit::output::CliError;

use crate::{cli_out, dry_run};

#[derive(Subcommand)]
pub enum WebCmd {
    /// Print plugins.json for the web configurator: the `--ext-info` of
    /// every plugin found on PATH (or in --dir), including its `web`
    /// object. `-o FILE` writes it to FILE.
    Plugins {
        /// Look for plugins in this directory instead of PATH; repeatable.
        #[arg(long, value_name = "DIR")]
        dir: Vec<std::path::PathBuf>,
        /// Overwrite the `-o` file when it exists.
        #[arg(long)]
        force: bool,
        /// Print what would be written, as data with --format; write nothing.
        #[arg(long)]
        dry_run: bool,
    },
}

pub fn run(m: &ArgMatches) {
    let Some(("plugins", p)) = m.subcommand() else {
        return;
    };
    let given: Vec<std::path::PathBuf> = p
        .get_many::<std::path::PathBuf>("dir")
        .into_iter()
        .flatten()
        .map(|d| std::path::absolute(d).unwrap_or_else(|_| d.clone()))
        .collect();
    let dirs = if given.is_empty() {
        plugin::plugin_dirs(std::env::var_os("PATH").as_deref())
    } else {
        given
    };
    // the global `-o/--output` (hop-top-kit output flags); `-` is stdout
    let out = p.get_one::<String>("output").map_or("", String::as_str);
    let force = p.get_flag("force");
    if dry_run::on(p) {
        return dry_run::web_plugins(p, out, force);
    }
    let cache = Cache::new(&click::state_dir());
    let infos: Vec<_> = plugin::discover(&dirs)
        .iter()
        .map(|f| plugin::info_now(&cache, f))
        .collect();
    let body = format!(
        "{}\n",
        serde_json::to_string_pretty(&plugin::web::manifest(&infos)).unwrap_or_default()
    );
    let leaf = p;
    let written = match out {
        "" | "-" => std::io::stdout()
            .write_all(body.as_bytes())
            .map_err(|e| e.to_string()),
        path => {
            let p = std::path::Path::new(path);
            if p.exists() && !force {
                let err = CliError::conflict(format!("{path} exists; pass --force to overwrite"));
                cli_out::fail(leaf, "web plugins", err);
            }
            hop_bar::rewrite::target(p).and_then(|t| {
                hop_bar::rewrite::write(&t, &body).map_err(|e| format!("{path}: {e}"))
            })
        }
    };
    if let Err(e) = written {
        eprintln!("bar web plugins: {e}");
        std::process::exit(1);
    }
}
