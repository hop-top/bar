//! `bar tray`: the desktop tray's subcommands. `state` writes the menu
//! snapshot the tray shows; `run` runs one of its items. More (installing
//! the app) dispatch from the same place.

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
    /// Run one item of the menu snapshot: its plugin, action and tokens come
    /// from tray-menu.json, never from the command line.
    Run {
        /// The item: <plugin>.<group>.<item>, as the snapshot names it.
        id: String,
        /// The item asks for confirmation and the user gave it.
        #[arg(long)]
        confirmed: bool,
        /// The snapshot generation the menu was built from; a different
        /// one on disk means the menu changed and the run is refused.
        #[arg(long, value_name = "ID")]
        generation: Option<String>,
    },
}

pub fn run(m: &ArgMatches) {
    match m.subcommand() {
        Some(("state", s)) => state(s.get_flag("json")),
        Some(("run", r)) => run_item(&tray::run::Pick {
            id: r.get_one::<String>("id").map_or("", String::as_str),
            generation: r.get_one::<String>("generation").map(String::as_str),
            confirmed: r.get_flag("confirmed"),
        }),
        _ => {}
    }
}

/// The user's config only (a project file never grants a plugin); a
/// broken one is a usage error.
fn config(cmd: &str) -> conf::Config {
    match conf::load(None) {
        conf::LoadResult::Ok(c) => c,
        conf::LoadResult::Missing => conf::Config::default(),
        conf::LoadResult::Invalid(_) => {
            eprintln!("bar tray {cmd}: config error, run bar check");
            std::process::exit(EXIT_USAGE);
        }
    }
}

/// Plugins from `PATH`, the dir of `bar` and the usual install dirs;
/// the state dir the click handler uses.
fn state(json: bool) {
    let config = config("state");
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

/// Exit 1 with one short line on stderr when nothing ran or the plugin
/// failed (the tray shows it); the action's first line on stdout when it
/// ran.
fn run_item(pick: &tray::run::Pick) {
    let config = config("run");
    let dirs = tray::handler_plugin_dirs(std::env::var_os("PATH").as_deref());
    let state = click::handler_state_dir();
    match tray::run::run(&state, &config, &dirs, pick) {
        Ok(note) => {
            if !note.is_empty() {
                println!("{note}");
            }
        }
        Err(e) => {
            eprintln!("bar tray run: refused: {e}");
            std::process::exit(1);
        }
    }
}
