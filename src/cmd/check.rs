use std::io::Write;

use clap::ArgMatches;
use hop_bar::{check, click, conf, plugin};
use hop_top_kit::output::{dispatch, DispatchOptions, EXIT_USAGE};
use serde_json::Value;

use super::cwd;

pub fn run(m: &ArgMatches) {
    let _palettes = hop_bar::palette::use_dir(conf::palette_dir_default());
    let dirs = plugin::plugin_dirs(std::env::var_os("PATH").as_deref());
    let user = conf::find_path_default().map(|p| check::source(&p));
    let project = cwd().and_then(|d| conf::project::find(&d)).map(|p| {
        let text = conf::project::read(&p);
        (p, text)
    });
    let mut report = check::check_layers(user, project, &dirs);
    if cfg!(target_os = "macos") {
        if let Ok(cfg) = click::InstallCfg::from_env() {
            report.notes.extend(cfg.warnings());
        }
    }
    let mut out = std::io::stdout();
    if m.get_one::<String>("format").map(String::as_str) == Some("table") {
        let _ = out.write_all(report.to_text().as_bytes());
    } else if let Err(e) = dispatch(
        m,
        &mut out,
        &check_json(&report),
        DispatchOptions::default(),
    ) {
        eprintln!("bar check: {e}");
        std::process::exit(EXIT_USAGE);
    }
    if report.has_errors() {
        std::process::exit(EXIT_USAGE);
    }
}

/// The report with the `state` it renders from.
fn check_json(report: &check::Report) -> Value {
    let on = matches!(conf::load(cwd().as_deref()), conf::LoadResult::Ok(c) if c.update_check);
    let mut v = report.to_json();
    v["state"] = check::state::snapshot(&click::handler_state_dir(), on);
    v
}
