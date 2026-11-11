use std::io::Write;

use clap::ArgMatches;
use hop_bar::conf;

use super::cwd;

pub fn run(m: &ArgMatches) {
    let state = m.get_one::<String>("state").map_or("early", String::as_str);
    let columns = std::env::var("COLUMNS")
        .ok()
        .and_then(|c| c.trim().parse().ok());
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    let preset = |p: &str| {
        // names come from the value parser, so the preset exists and parses
        match conf::parse(hop_bar::presets::get(p).unwrap_or_default()) {
            Ok(c) => conf::LoadResult::Ok(c),
            Err(e) => conf::LoadResult::Invalid(e),
        }
    };
    let mut so = std::io::stdout().lock();
    let draw = |state: &str, load: conf::LoadResult| {
        if m.get_flag("subagent") {
            hop_bar::preview::subagent(load, columns, now)
        } else {
            hop_bar::preview::render(state, load, columns, now)
        }
    };
    match m.get_one::<String>("preset") {
        Some(p) => {
            let _ = writeln!(so, "{}", draw(state, preset(p)));
        }
        // bare `--preset`: every preset, each under a muted header
        None if m.value_source("preset").is_some() => {
            for p in hop_bar::presets::NAMES {
                let out = draw(state, preset(p));
                let _ = writeln!(so, "{}\n{out}", hop_bar::style::dim(&format!("── {p} ──")));
            }
        }
        None => {
            let _ = writeln!(so, "{}", draw(state, conf::load(cwd().as_deref())));
        }
    }
}
