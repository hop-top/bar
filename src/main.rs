use std::io::Write;

use clap::{Command, CommandFactory, Parser, Subcommand};
use hop_bar::cache::{refresh_timeout, Cache, CachedRunner, Request};
use hop_bar::{click, conf, session};
use hop_top_kit::output::{register_output_flags, RegisterOutputFlagsOptions};

#[derive(Parser)]
#[command(
    name = "bar",
    version,
    about = "Configurable status line for Claude Code: tmux-style format, placeholders, plugins"
)]
struct Cli {
    #[command(subcommand)]
    cmd: SubCmd,
}

#[derive(Subcommand)]
enum SubCmd {
    /// Print the status line.
    Render,
    /// Flip a usage view between bar and percentage.
    Toggle {
        /// 5h or 7d
        key: String,
    },
    /// Click handler app.
    Click {
        #[command(subcommand)]
        cmd: ClickCmd,
    },
    /// Run one background cache refresh (spawned by render).
    #[command(hide = true)]
    Refresh {
        /// Refresh request as JSON.
        request: String,
    },
}

#[derive(Subcommand)]
enum ClickCmd {
    /// Build and register the macOS `bar://` URL handler.
    Install {
        /// Print the script and commands without running them.
        #[arg(long)]
        dry_run: bool,
    },
}

fn main() {
    let cli_cmd: Command = Cli::command();
    let (cli_cmd, _ctx) = register_output_flags(cli_cmd, RegisterOutputFlagsOptions::default());
    let matches = cli_cmd.get_matches();
    match matches.subcommand() {
        Some(("render", _)) => render(),
        Some(("toggle", m)) => {
            let key = m.get_one::<String>("key").map_or("", String::as_str);
            match click::toggle(&click::state_dir(), key) {
                Ok(on) => println!("{key}: {}", if on { "pct" } else { "bar" }),
                Err(e) => {
                    eprintln!("bar toggle: {e}");
                    std::process::exit(2);
                }
            }
        }
        Some(("click", m)) => {
            if let Some(("install", i)) = m.subcommand() {
                let cfg = click::InstallCfg::from_env();
                let r = click::run_install(
                    &cfg,
                    cfg!(target_os = "macos"),
                    i.get_flag("dry_run"),
                    &mut click::exec,
                    &mut std::io::stdout(),
                );
                if let Err(e) = r {
                    eprintln!("bar click install: {e}");
                    std::process::exit(1);
                }
            }
        }
        Some(("refresh", m)) => {
            let raw = m.get_one::<String>("request").map_or("", String::as_str);
            if let Ok(req) = Request::from_json(raw) {
                let limit = refresh_timeout(std::env::var("BAR_REFRESH_TIMEOUT").ok().as_deref());
                Cache::new(&click::state_dir()).refresh(&req, limit);
            }
        }
        _ => {}
    }
}

fn render() {
    let mut buf = Vec::new();
    // Non-UTF-8 or unreadable stdin degrades to an empty session.
    let _ = std::io::Read::read_to_end(&mut std::io::stdin(), &mut buf);
    let s = session::parse(&String::from_utf8_lossy(&buf));
    let columns = std::env::var("COLUMNS")
        .ok()
        .and_then(|c| c.trim().parse().ok());
    let cache = Cache::new(&click::state_dir());
    let runner = CachedRunner::new(Cache::new(&click::state_dir()));
    let out = hop_bar::render::run(&s, conf::load(), columns, &runner, Some(&cache));
    if !out.is_empty() {
        // a closed pipe must not panic; render always exits 0
        let mut so = std::io::stdout().lock();
        let _ = writeln!(so, "{out}");
        let _ = so.flush();
    }
}
