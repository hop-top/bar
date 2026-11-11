use std::io::Write;

use clap::ArgMatches;
use hop_top_kit::output::CliError;
use serde_json::json;

use crate::{cli_out, dry_run};

pub fn run(m: &ArgMatches) {
    cli_out::check_format(m, "init");
    let name = m
        .get_one::<String>("preset")
        .map_or(hop_bar::presets::DEFAULT, String::as_str);
    let home = hop_bar::platform::home().unwrap_or_default();
    let plan = if dry_run::on(m) {
        hop_bar::presets::plan
    } else {
        hop_bar::presets::init
    };
    let r = plan(
        name,
        &|k| std::env::var(k).ok(),
        &home,
        &hop_bar::platform::dirs::config(),
    );
    match r {
        Ok(hop_bar::presets::Init::Wrote(p)) if dry_run::on(m) => {
            let c = vec![dry_run::change("create", &p)];
            dry_run::emit(m, "init", c, json!({"preset": name}), "");
        }
        Ok(hop_bar::presets::Init::Same(_)) if dry_run::on(m) => {
            dry_run::emit(m, "init", Vec::new(), json!({"preset": name}), "");
        }
        Ok(hop_bar::presets::Init::Wrote(p)) => {
            let text = format!("wrote {} (preset {name})\n", p.display());
            let data = json!({"path": p.display().to_string(), "preset": name, "written": true});
            cli_out::emit(m, "init", &text, &data);
        }
        Ok(hop_bar::presets::Init::Same(p)) => {
            let _ = writeln!(
                std::io::stderr(),
                "bar init: {} exists; not overwritten (already preset {name})",
                p.display()
            );
            let data = json!({"path": p.display().to_string(), "preset": name, "written": false});
            cli_out::emit(m, "init", "", &data);
        }
        Err(e @ hop_bar::presets::InitError::Exists(_)) => cli_out::fail(
            m,
            "init",
            cli_out::with_fix(
                CliError::conflict(e.to_string()),
                "edit that file, or move it aside and rerun bar init",
            ),
        ),
        Err(e) => cli_out::fail(m, "init", CliError::usage(e.to_string())),
    }
}
