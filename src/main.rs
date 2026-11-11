mod cli_out;
mod cmd;
mod config_cmd;
mod dry_run;
mod help_json;
mod tray_cmd;
mod waybar_cmd;

use std::ffi::OsString;

use clap::{Command, CommandFactory, Parser, Subcommand};
use cmd::render::{refresh, render, subagent};
use hop_top_kit::output::{register_output_flags, RegisterOutputFlagsOptions};

#[derive(Parser)]
#[command(
    name = "bar",
    version,
    about = "Fast, configurable and extensible status line for Claude Code",
    after_help = "Plugins: `bar <name> <args>` runs the `bar-<name>` plugin found on PATH, \
                  e.g. `bar cliproxyapi use`."
)]
struct Cli {
    #[command(subcommand)]
    cmd: SubCmd,
}

#[derive(Subcommand)]
enum SubCmd {
    /// Print the status line.
    Render,
    /// Print the subagent rows (Claude Code's subagentStatusLine command).
    Subagent,
    /// Lint the config file; exit 2 on a config error.
    Check,
    /// Show the layout with built-in sample data.
    Preview {
        /// Usage pace to show.
        #[arg(long, value_parser = hop_bar::preview::STATES, default_value = "early")]
        state: String,
        /// Render a built-in preset instead of your config; bare, every preset in turn.
        #[arg(long, num_args = 0..=1, value_name = "NAME", value_parser = hop_bar::presets::NAMES)]
        preset: Option<Option<String>>,
        /// Show the subagent rows (`set subagent`) for sample subagents instead.
        #[arg(long)]
        subagent: bool,
    },
    /// Export the config, or import a file into it.
    Config {
        #[command(subcommand)]
        cmd: config_cmd::ConfigCmd,
    },
    /// Edit the config in a full-screen terminal editor with a live preview.
    #[cfg(feature = "edit")]
    Edit,
    /// Write a preset as your config; refuses when another config exists.
    Init {
        /// Preset to write.
        #[arg(long, value_parser = hop_bar::presets::NAMES, default_value = hop_bar::presets::DEFAULT)]
        preset: String,
        /// Print what would be written, as data with --format; write nothing.
        #[arg(long)]
        dry_run: bool,
    },
    /// Set bar as Claude Code's status line in its settings.json (shows the diff, asks first).
    Install {
        /// Edit .claude/settings.json in the current directory instead of your user settings.
        #[arg(long, conflicts_with = "local")]
        project: bool,
        /// Edit .claude/settings.local.json in the current directory instead of your user settings.
        #[arg(long)]
        local: bool,
        /// Put back the status line bar replaced, or remove bar's.
        #[arg(long)]
        uninstall: bool,
        /// Also re-run the status line every SECS seconds (refreshInterval).
        #[arg(long, value_name = "SECS", value_parser = clap::value_parser!(u64).range(1..), conflicts_with = "uninstall")]
        refresh: Option<u64>,
        /// Also add the `bar click hook` hooks (with --uninstall, remove them).
        #[arg(long)]
        hooks: bool,
        /// Also set bar as the subagentStatusLine (with --uninstall, remove it).
        #[arg(long)]
        subagent: bool,
        /// Set (with --uninstall, remove) only the subagentStatusLine; statusLine is left as is.
        #[arg(long, conflicts_with_all = ["subagent", "refresh"])]
        subagent_only: bool,
        /// Write without asking.
        #[arg(short, long)]
        yes: bool,
        /// Print what would be written, as data with --format; write nothing.
        #[arg(long)]
        dry_run: bool,
    },
    /// Show the newest release found by the update check and how to upgrade (never downloads).
    Update,
    /// Flip a usage view between bar and percentage, or a plugin between
    /// its text and alternate text; with a value, set it instead.
    Toggle {
        /// 5h, 7d, an installed plugin's name or <plugin>.<toggle>
        key: String,
        /// on or off, or the view: pct or bar (5h, 7d), alt or show (plugins)
        value: Option<String>,
        /// Print what would be written, as data with --format; write nothing.
        #[arg(long)]
        dry_run: bool,
    },
    /// Click handler app and clickable commands.
    Click {
        #[command(subcommand)]
        cmd: cmd::click::ClickCmd,
    },
    /// Desktop tray: its menu snapshot, install and autostart.
    Tray {
        #[command(subcommand)]
        cmd: tray_cmd::TrayCmd,
    },
    /// One line of waybar custom-module JSON ({text, tooltip, class, alt}) from the tray snapshot.
    Waybar,
    /// Discovered `bar-<name>` plugins (`bar <name> <args>` runs one).
    Plugin {
        #[command(subcommand)]
        cmd: cmd::plugin::PluginCmd,
    },
    /// Web configurator support.
    Web {
        #[command(subcommand)]
        cmd: cmd::web::WebCmd,
    },
    /// Model price data (models.dev).
    Prices {
        #[command(subcommand)]
        cmd: cmd::prices::PricesCmd,
    },
    /// Run one background cache refresh (spawned by render).
    #[command(hide = true)]
    Refresh {
        /// Refresh request as JSON.
        request: String,
    },
    /// Any other name runs the `bar-<name>` plugin with the remaining arguments.
    #[command(external_subcommand)]
    #[allow(dead_code)] // read through the matches
    External(Vec<OsString>),
}

fn main() {
    // `bar render` (every status line update) and `bar refresh <json>`
    // (its background jobs) skip building the clap command; for these
    // argument lists clap would dispatch the same way.
    let args: Vec<_> = std::env::args_os().skip(1).take(3).collect();
    match args.as_slice() {
        [a] if a == "render" => return render(),
        [a] if a == "subagent" => return subagent(),
        [a, req] if a == "refresh" => {
            if let Some(req) = req.to_str().filter(|r| !r.starts_with('-')) {
                return refresh(req);
            }
        }
        _ => {}
    }
    let cli_cmd: Command = Cli::command();
    let (cli_cmd, _ctx) = register_output_flags(cli_cmd, RegisterOutputFlagsOptions::default());
    let rest: Vec<OsString> = std::env::args_os().skip(1).collect();
    if let Some(format) = help_json::requested(&rest) {
        return help_json::print(cli_cmd, &rest, &format);
    }
    let matches = cli_out::matches(cli_cmd);
    match matches.subcommand() {
        Some(("render", _)) => render(),
        Some(("subagent", _)) => subagent(),
        Some(("check", m)) => cmd::check::run(m),
        Some(("preview", m)) => cmd::preview::run(m),
        Some(("init", m)) => cmd::init::run(m),
        Some(("install", m)) => cmd::install::run(m),
        Some(("update", m)) => cmd::update::run(m),
        Some(("config", m)) => config_cmd::run(m),
        #[cfg(feature = "edit")]
        Some(("edit", _)) => {
            if let Err(e) = hop_bar::edit::run() {
                eprintln!("bar edit: {e}");
                std::process::exit(hop_top_kit::output::EXIT_USAGE);
            }
        }
        Some(("toggle", m)) => cmd::toggle::run(m),
        Some(("click", m)) => cmd::click::run(m),
        Some(("tray", m)) => {
            // started by the tray with a hidden console of its own
            hop_bar::platform::claim_private_console();
            tray_cmd::run(m);
        }
        Some(("waybar", _)) => waybar_cmd::run(),
        Some(("plugin", m)) => cmd::plugin::run(m),
        Some(("prices", m)) => cmd::prices::run(m),
        Some(("web", m)) => cmd::web::run(m),
        Some(("refresh", m)) => {
            refresh(m.get_one::<String>("request").map_or("", String::as_str));
        }
        Some((name, m)) => {
            let args: Vec<OsString> = m
                .get_many::<OsString>("")
                .map(|v| v.cloned().collect())
                .unwrap_or_default();
            cmd::external::run(name, &args, cli_out::format(&matches));
        }
        _ => {}
    }
}
