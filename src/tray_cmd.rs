//! `bar tray`: the desktop tray's subcommands. `state` writes the menu
//! snapshot the tray shows; `run` runs one of its items; `install`,
//! `uninstall`, `start`, `stop` and `status` manage the app.

use clap::{ArgMatches, Subcommand};
use hop_bar::tray::install::{InstallCfg, InstallError, InstallOpts};
use hop_bar::tray::process::{self, Layout};
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
    /// Set the tray up: record its dirs, register the Claude Code hooks,
    /// start it at login (a LaunchAgent on macOS) and now.
    Install {
        /// Print what would be written and run, change nothing.
        #[arg(long)]
        dry_run: bool,
        /// No autostart: start it with `bar tray start`.
        #[arg(long)]
        no_autostart: bool,
        /// Leave the Claude Code hooks alone (the session rows need them).
        #[arg(long)]
        no_hooks: bool,
    },
    /// Stop the tray and remove what install wrote; the hooks stay while
    /// `bar click install` is installed.
    Uninstall {
        /// Keep the Claude Code hooks.
        #[arg(long)]
        keep_hooks: bool,
    },
    /// Start the tray unless it runs.
    Start,
    /// Stop the tray (until `bar tray start` or the next login).
    Stop,
    /// Whether the tray is installed and running, and the menu's age.
    Status {
        /// Print the status as JSON.
        #[arg(long)]
        json: bool,
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
        Some(("install", i)) => install(&InstallOpts {
            dry_run: i.get_flag("dry_run"),
            autostart: !i.get_flag("no_autostart"),
            hooks: !i.get_flag("no_hooks"),
        }),
        Some(("uninstall", u)) => uninstall(u.get_flag("keep_hooks")),
        Some(("start", _)) => start(),
        Some(("stop", _)) => {
            let r = process::stop(
                &Layout::from_env(),
                &mut process::System,
                &mut std::io::stdout(),
            );
            fail("stop", r.map(drop));
        }
        Some(("status", s)) => status(s.get_flag("json")),
        _ => {}
    }
}

fn fail(cmd: &str, r: Result<(), String>) {
    if let Err(e) = r {
        eprintln!("bar tray {cmd}: {e}");
        std::process::exit(1);
    }
}

fn install(o: &InstallOpts) {
    let cfg = InstallCfg::from_env().unwrap_or_else(|e| {
        eprintln!("bar tray install: {e}");
        std::process::exit(EXIT_USAGE);
    });
    let config = config("install");
    let macos = cfg!(target_os = "macos");
    let r = tray::install::install(
        &cfg,
        o,
        macos,
        &config,
        &mut process::System,
        &mut std::io::stdout(),
    );
    if let Err(e) = r {
        eprintln!("bar tray install: {e}");
        std::process::exit(if e == InstallError::Unsupported {
            EXIT_USAGE
        } else {
            1
        });
    }
}

fn uninstall(keep_hooks: bool) {
    let Some(claude) = click::claude_dir() else {
        eprintln!("bar tray uninstall: HOME must be an absolute path (or CLAUDE_CONFIG_DIR, when set, must be absolute)");
        std::process::exit(EXIT_USAGE);
    };
    let r = tray::install::uninstall(
        &Layout::from_env(),
        &claude.join("settings.json"),
        keep_hooks,
        &mut process::System,
        &mut std::io::stdout(),
    );
    fail("uninstall", r);
}

/// The command line install recorded, else this `bar` and the
/// `bar-tray` beside it.
fn start() {
    let l = Layout::from_env();
    let argv = tray::install::recorded_argv(&l.state_dir).or_else(|| {
        InstallCfg::from_env()
            .ok()
            .filter(|c| std::path::Path::new(&c.tray).is_file())
            .map(|c| c.argv())
    });
    let Some(argv) = argv else {
        eprintln!("bar tray start: no bar-tray beside bar; run bar tray install");
        std::process::exit(1);
    };
    fail(
        "start",
        process::start(&l, &argv, &mut process::System, &mut std::io::stdout()),
    );
}

fn status(json: bool) {
    let st = process::status(&Layout::from_env(), std::time::SystemTime::now());
    let mut so = std::io::stdout().lock();
    if json {
        let _ = writeln!(so, "{}", st.to_json());
    } else {
        for l in st.lines() {
            let _ = writeln!(so, "{l}");
        }
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
