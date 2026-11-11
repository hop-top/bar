use clap::{ArgMatches, Subcommand};
use hop_bar::{click, conf, plugin};
use hop_top_kit::output::EXIT_USAGE;

#[derive(Subcommand)]
pub enum ClickCmd {
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
    /// Run a plugin's declared action (handler entry).
    Action {
        /// bar://action/<plugin>.<action>?<token>=<value>&k=<entry>&n=<nonce>
        url: String,
    },
    /// Claude Code hook (stdin JSON): waiting marker, subagents, plugin records.
    Hook,
}

/// `bar click action`: the user's config only (a project file never grants
/// an action), plugins from `PATH`, the dir of `bar` and the usual install
/// dirs, since the URL handler starts `bar` with a minimal `PATH`.
fn click_action(url: &str) -> Result<(), String> {
    let config = match conf::load(None) {
        conf::LoadResult::Ok(c) => c,
        conf::LoadResult::Missing => conf::Config::default(),
        conf::LoadResult::Invalid(_) => return Err("config error, run bar check".into()),
    };
    let mut dirs = plugin::plugin_dirs(std::env::var_os("PATH").as_deref());
    for d in ["/opt/homebrew/bin", "/usr/local/bin"] {
        let d = std::path::PathBuf::from(d);
        if !dirs.contains(&d) {
            dirs.push(d);
        }
    }
    click::action::run(&click::handler_state_dir(), url, &config, &dirs)
}

pub fn run(m: &ArgMatches) {
    match m.subcommand() {
        Some(("install", i)) => {
            let cfg = click::InstallCfg::from_env().unwrap_or_else(|e| {
                eprintln!("bar click install: {e}");
                std::process::exit(EXIT_USAGE);
            });
            let macos = cfg!(target_os = "macos");
            if !i.get_flag("hooks_only") && macos && !cfg.app.is_absolute() {
                eprintln!("bar click install: HOME (or BAR_APP_DIR) must be an absolute path");
                std::process::exit(EXIT_USAGE);
            }
            let r = if i.get_flag("hooks_only") {
                click::run_hooks_only(&cfg, i.get_flag("dry_run"), &mut std::io::stdout())
                    .map_err(click::InstallError::Failed)
            } else {
                click::run_install(
                    &cfg,
                    macos,
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
            if cfg!(windows) {
                eprintln!(
                    "bar click run: bar:// links need macOS and tmux (not supported on Windows)"
                );
                std::process::exit(EXIT_USAGE);
            }
            let url = r.get_one::<String>("url").map_or("", String::as_str);
            if let Err(e) = click::run(&click::sys::System::from_env(), url) {
                eprintln!("bar click run: refused: {e}");
                std::process::exit(1);
            }
        }
        Some(("action", a)) => {
            let url = a.get_one::<String>("url").map_or("", String::as_str);
            if let Err(e) = click_action(url) {
                eprintln!("bar click action: refused: {e}");
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
