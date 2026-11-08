use std::io::Write;

use clap::{ArgMatches, Command, CommandFactory, Parser, Subcommand};
use hop_bar::cache::{refresh_timeout, Cache, CachedRunner, Request};
use hop_bar::{check, click, conf, plugin, session};
use hop_top_kit::output::{
    dispatch, register_output_flags, DispatchOptions, RegisterOutputFlagsOptions, EXIT_USAGE,
};
use serde_json::{json, Value};

#[derive(Parser)]
#[command(
    name = "bar",
    version,
    about = "Fast, configurable and extensible status line for Claude Code"
)]
struct Cli {
    #[command(subcommand)]
    cmd: SubCmd,
}

#[derive(Subcommand)]
enum SubCmd {
    /// Print the status line.
    Render,
    /// Lint the config file; exit 2 on a config error.
    Check,
    /// Show the layout with built-in sample data.
    Preview {
        /// Usage pace to show.
        #[arg(long, value_parser = hop_bar::preview::STATES, default_value = "early")]
        state: String,
        /// Render a built-in preset instead of your config.
        #[arg(long, value_parser = hop_bar::presets::NAMES)]
        preset: Option<String>,
    },
    /// Write a preset as your config; refuses when a config exists.
    Init {
        /// Preset to write.
        #[arg(long, value_parser = hop_bar::presets::NAMES, default_value = hop_bar::presets::DEFAULT)]
        preset: String,
    },
    /// Flip a usage view between bar and percentage.
    Toggle {
        /// 5h or 7d
        key: String,
    },
    /// Click handler app and clickable commands.
    Click {
        #[command(subcommand)]
        cmd: ClickCmd,
    },
    /// Discovered `bar-<name>` plugins.
    Plugin {
        #[command(subcommand)]
        cmd: PluginCmd,
    },
    /// Model price data (models.dev).
    Prices {
        #[command(subcommand)]
        cmd: PricesCmd,
    },
    /// Run one background cache refresh (spawned by render).
    #[command(hide = true)]
    Refresh {
        /// Refresh request as JSON.
        request: String,
    },
}

#[derive(Subcommand)]
enum PricesCmd {
    /// Fetch models.dev prices into bar's data dir; the only network use.
    Update {
        /// Write this file instead of the data dir's models.json.
        #[arg(long, value_name = "FILE")]
        out: Option<std::path::PathBuf>,
    },
}

#[derive(Subcommand)]
enum PluginCmd {
    /// List plugins on PATH.
    List,
    /// Show one plugin's `--ext-info`.
    Info {
        /// Placeholder name (file name without `bar-`).
        name: String,
    },
}

#[derive(Subcommand)]
enum ClickCmd {
    /// Build and register the macOS `bar://` URL handler.
    Install {
        /// Print the script and commands without running them.
        #[arg(long)]
        dry_run: bool,
        /// Only register the Claude Code hooks (any platform); no URL handler app.
        #[arg(long)]
        hooks_only: bool,
    },
    /// Type an allow-listed slash command into its session (handler entry).
    Run {
        /// bar://cmd/<command>?tty=<ttysN>&sid=<session id>
        url: String,
    },
    /// Claude Code hook: keep the session's waiting marker (stdin JSON).
    Hook,
}

fn main() {
    // `bar render`, run on every status line update, skips building the
    // clap command; it takes no options, so clap would do the same.
    let mut args = std::env::args_os().skip(1);
    if let (Some(a), None) = (args.next(), args.next()) {
        if a == "render" {
            return render();
        }
    }
    let cli_cmd: Command = Cli::command();
    let (cli_cmd, _ctx) = register_output_flags(cli_cmd, RegisterOutputFlagsOptions::default());
    let matches = cli_cmd.get_matches();
    match matches.subcommand() {
        Some(("render", _)) => render(),
        Some(("check", m)) => check(m),
        Some(("preview", m)) => preview(m),
        Some(("init", m)) => init(m),
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
        Some(("click", m)) => click_cmd(m),
        Some(("plugin", m)) => plugin_cmd(m),
        Some(("prices", m)) => prices_cmd(m),
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

fn prices_cmd(m: &ArgMatches) {
    if let Some(("update", u)) = m.subcommand() {
        match hop_bar::prices::update::run(
            u.get_one::<std::path::PathBuf>("out").map(|p| p.as_path()),
        ) {
            Ok((path, n)) => println!("{n} models written to {}", path.display()),
            Err(e) => {
                eprintln!("bar prices update: {e}");
                std::process::exit(2);
            }
        }
    }
}

fn check(m: &ArgMatches) {
    let dirs = plugin::path_dirs(std::env::var_os("PATH").as_deref());
    let report = match conf::find_path_default() {
        Some(p) => check::check_file(&p, &dirs),
        None => check::check_builtin(&dirs),
    };
    let mut out = std::io::stdout();
    if m.get_one::<String>("format").map(String::as_str) == Some("table") {
        let _ = out.write_all(report.to_text().as_bytes());
    } else if let Err(e) = dispatch(m, &mut out, &report.to_json(), DispatchOptions::default()) {
        eprintln!("bar check: {e}");
        std::process::exit(EXIT_USAGE);
    }
    if report.has_errors() {
        std::process::exit(EXIT_USAGE);
    }
}

fn preview(m: &ArgMatches) {
    let state = m.get_one::<String>("state").map_or("early", String::as_str);
    let columns = std::env::var("COLUMNS")
        .ok()
        .and_then(|c| c.trim().parse().ok());
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    let load = match m.get_one::<String>("preset") {
        // names come from the value parser, so the preset exists and parses
        Some(p) => match conf::parse(hop_bar::presets::get(p).unwrap_or_default()) {
            Ok(c) => conf::LoadResult::Ok(c),
            Err(e) => conf::LoadResult::Invalid(e),
        },
        None => conf::load(),
    };
    let out = hop_bar::preview::render(state, load, columns, now);
    let mut so = std::io::stdout().lock();
    let _ = writeln!(so, "{out}");
}

fn init(m: &ArgMatches) {
    let name = m
        .get_one::<String>("preset")
        .map_or(hop_bar::presets::DEFAULT, String::as_str);
    let home = std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_default();
    let r = hop_bar::presets::init(
        name,
        &|k| std::env::var(k).ok(),
        &home,
        &hop_top_kit::xdg::raw_config_dir("bar"),
    );
    match r {
        Ok(p) => println!("wrote {} (preset {name})", p.display()),
        Err(e) => {
            eprintln!("bar init: {e}");
            std::process::exit(EXIT_USAGE);
        }
    }
}

fn click_cmd(m: &ArgMatches) {
    match m.subcommand() {
        Some(("install", i)) => {
            let cfg = click::InstallCfg::from_env().unwrap_or_else(|e| {
                eprintln!("bar click install: {e}");
                std::process::exit(EXIT_USAGE);
            });
            if !i.get_flag("hooks_only") && !cfg.app.is_absolute() {
                eprintln!("bar click install: HOME (or BAR_APP_DIR) must be an absolute path");
                std::process::exit(EXIT_USAGE);
            }
            let r = if i.get_flag("hooks_only") {
                click::run_hooks_only(&cfg, i.get_flag("dry_run"), &mut std::io::stdout())
                    .map_err(click::InstallError::Failed)
            } else {
                click::run_install(
                    &cfg,
                    cfg!(target_os = "macos"),
                    i.get_flag("dry_run"),
                    &mut click::exec,
                    &mut std::io::stdout(),
                )
            };
            if let Err(e) = r {
                eprintln!("bar click install: {e}");
                std::process::exit(if matches!(e, click::InstallError::Unsupported) {
                    EXIT_USAGE
                } else {
                    1
                });
            }
        }
        Some(("run", r)) => {
            let url = r.get_one::<String>("url").map_or("", String::as_str);
            if let Err(e) = click::run(&click::sys::System::from_env(), url) {
                eprintln!("bar click run: refused: {e}");
                std::process::exit(1);
            }
        }
        Some(("hook", _)) => {
            let mut buf = Vec::new();
            let _ = std::io::Read::read_to_end(&mut std::io::stdin(), &mut buf);
            click::hook::run_hook(&click::state_dir(), &String::from_utf8_lossy(&buf));
        }
        _ => {}
    }
}

fn plugin_cmd(m: &ArgMatches) {
    let dirs = plugin::path_dirs(std::env::var_os("PATH").as_deref());
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
                eprintln!("bar plugin info: no plugin `{name}` on PATH");
                std::process::exit(1);
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

fn render() {
    let mut buf = Vec::new();
    // Non-UTF-8 or unreadable stdin degrades to an empty session.
    let _ = std::io::Read::read_to_end(&mut std::io::stdin(), &mut buf);
    let s = session::parse(&String::from_utf8_lossy(&buf));
    let columns = std::env::var("COLUMNS")
        .ok()
        .and_then(|c| c.trim().parse().ok());
    let cache = Cache::new(&click::state_dir());
    let runner = CachedRunner::new(cache.clone());
    let out = hop_bar::render::run(&s, conf::load(), columns, &runner, Some(&cache));
    if !out.is_empty() {
        // a closed pipe must not panic; render always exits 0
        let mut so = std::io::stdout().lock();
        let _ = writeln!(so, "{out}");
        let _ = so.flush();
    }
}
