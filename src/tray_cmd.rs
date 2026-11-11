//! `bar tray`: the desktop tray's subcommands. `state` writes the menu
//! snapshot the tray shows; more (running an item, installing the app)
//! dispatch from the same place.

use clap::{ArgMatches, Subcommand};
use hop_bar::{click, conf, tray};
use hop_top_kit::output::EXIT_USAGE;
use std::io::Write;

#[derive(Subcommand)]
pub enum TrayCmd {
    /// Write the menu snapshot the tray shows to the state dir (tray-menu.json):
    /// view toggles, last action, session counts and the granted plugins' menus.
    State {
        /// Print the snapshot as JSON.
        #[arg(long)]
        json: bool,
    },
}

pub fn run(m: &ArgMatches) {
    if let Some(("state", s)) = m.subcommand() {
        state(s.get_flag("json"));
    }
}

/// The user's config only (a project file never grants a plugin),
/// plugins from `PATH`, the dir of `bar` and the usual install dirs,
/// the state dir the click handler uses.
fn state(json: bool) {
    let config = match conf::load(None) {
        conf::LoadResult::Ok(c) => c,
        conf::LoadResult::Missing => conf::Config::default(),
        conf::LoadResult::Invalid(_) => {
            eprintln!("bar tray state: config error, run bar check");
            std::process::exit(EXIT_USAGE);
        }
    };
    let dirs = tray::handler_plugin_dirs(std::env::var_os("PATH").as_deref());
    let state = click::handler_state_dir();
    match tray::state::run(&state, &config, &dirs) {
        Ok(v) => {
            let mut so = std::io::stdout().lock();
            let line = if json {
                v.to_string()
            } else {
                tray::state::summary(&v, &state)
            };
            let _ = writeln!(so, "{line}");
        }
        Err(e) => {
            eprintln!("bar tray state: {e}");
            std::process::exit(1);
        }
    }
}
