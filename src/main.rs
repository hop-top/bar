use std::io::Write;

mod cli_out;
mod config_cmd;

use std::ffi::OsString;

use clap::{ArgMatches, Command, CommandFactory, Parser, Subcommand};
use hop_bar::cache::{refresh_timeout, Cache, CachedRunner, Request};
use hop_bar::{check, click, conf, plugin, session};
use hop_top_kit::output::{
    dispatch, register_output_flags, CliError, DispatchOptions, RegisterOutputFlagsOptions,
    EXIT_USAGE,
};
use serde_json::{json, Value};

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
    /// Write a preset as your config; refuses when a config exists.
    Init {
        /// Preset to write.
        #[arg(long, value_parser = hop_bar::presets::NAMES, default_value = hop_bar::presets::DEFAULT)]
        preset: String,
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
    },
    /// Show the newest release found by the update check and how to upgrade (never downloads).
    Update,
    /// Flip a usage view between bar and percentage, or a plugin between
    /// its text and alternate text.
    Toggle {
        /// 5h, 7d or a plugin name
        key: String,
    },
    /// Click handler app and clickable commands.
    Click {
        #[command(subcommand)]
        cmd: ClickCmd,
    },
    /// Discovered `bar-<name>` plugins (`bar <name> <args>` runs one).
    Plugin {
        #[command(subcommand)]
        cmd: PluginCmd,
    },
    /// Web configurator support.
    Web {
        #[command(subcommand)]
        cmd: WebCmd,
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
    /// Any other name runs the `bar-<name>` plugin with the remaining arguments.
    #[command(external_subcommand)]
    #[allow(dead_code)] // read through the matches
    External(Vec<OsString>),
}

#[derive(Subcommand)]
enum PricesCmd {
    /// Fetch models.dev prices into bar's data dir (spendhist also does, daily, for models without a built-in price).
    Update {
        /// Write this file instead of the data dir's models.json.
        #[arg(long, value_name = "FILE")]
        out: Option<std::path::PathBuf>,
    },
}

#[derive(Subcommand)]
enum WebCmd {
    /// Print plugins.json for the web configurator: the `--ext-info` of
    /// every plugin found on PATH (or in --dir), including its `web`
    /// object. `-o FILE` writes it to FILE.
    Plugins {
        /// Look for plugins in this directory instead of PATH; repeatable.
        #[arg(long, value_name = "DIR")]
        dir: Vec<std::path::PathBuf>,
        /// Overwrite the `-o` file when it exists.
        #[arg(long)]
        force: bool,
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
    /// Run a plugin's declared action (handler entry).
    Action {
        /// bar://action/<plugin>.<action>?<token>=<value>&k=<entry>&n=<nonce>
        url: String,
    },
    /// Claude Code hook (stdin JSON): waiting marker, subagents, plugin records.
    Hook,
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
    let matches = cli_out::matches(cli_cmd);
    match matches.subcommand() {
        Some(("render", _)) => render(),
        Some(("subagent", _)) => subagent(),
        Some(("check", m)) => check(m),
        Some(("preview", m)) => preview(m),
        Some(("init", m)) => init(m),
        Some(("install", m)) => install(m),
        Some(("update", m)) => update(m),
        Some(("config", m)) => config_cmd::run(m),
        #[cfg(feature = "edit")]
        Some(("edit", _)) => {
            if let Err(e) = hop_bar::edit::run() {
                eprintln!("bar edit: {e}");
                std::process::exit(EXIT_USAGE);
            }
        }
        Some(("toggle", m)) => {
            cli_out::check_format(m, "toggle");
            let arg = m.get_one::<String>("key").map_or("", String::as_str);
            let flipped = click::toggle_target(arg).and_then(|(key, sid)| {
                click::toggle_in(&click::handler_state_dir(), key, sid).map(|on| (key, on))
            });
            match flipped {
                Ok((key, on)) => {
                    let view = match (click::is_usage_key(key), on) {
                        (true, true) => "pct",
                        (true, false) => "bar",
                        (false, true) => "alt",
                        (false, false) => "show",
                    };
                    let data = json!({"key": key, "view": view});
                    cli_out::emit(m, "toggle", &format!("{key}: {view}\n"), &data);
                }
                Err(e) => cli_out::fail(m, "toggle", CliError::usage(e)),
            }
        }
        Some(("click", m)) => click_cmd(m),
        Some(("plugin", m)) => plugin_cmd(m),
        Some(("prices", m)) => prices_cmd(m),
        Some(("web", m)) => web_cmd(m),
        Some(("refresh", m)) => {
            refresh(m.get_one::<String>("request").map_or("", String::as_str));
        }
        Some((name, m)) => {
            let args: Vec<OsString> = m
                .get_many::<OsString>("")
                .map(|v| v.cloned().collect())
                .unwrap_or_default();
            external(name, &args, cli_out::format(&matches));
        }
        _ => {}
    }
}

/// `bar <name> <args>`: runs the plugin `bar-<name>` (found as a render
/// finds one) with `args`, stdio inherited, and exits with its status. A
/// name that is no plugin but is an alias a plugin declares runs that
/// plugin; any other name is a usage error, as any unknown command,
/// reported per the `--format` before `name` or among `args`.
fn external(name: &str, args: &[OsString], format: &str) -> ! {
    let dirs = plugin::plugin_dirs(std::env::var_os("PATH").as_deref());
    let cache = Cache::new(&click::state_dir());
    let found = match plugin::resolve(&dirs, name, &|f| plugin::info_now(&cache, f).aliases) {
        plugin::Resolved::One(f) => f,
        plugin::Resolved::Ambiguous(names) => {
            eprintln!(
                "bar: `{name}` is an alias of several plugins ({}); run one as `bar <plugin> ...`",
                names.join(", ")
            );
            std::process::exit(EXIT_USAGE);
        }
        plugin::Resolved::Unknown => {
            let err = CliError::usage(format!("unrecognized command `{name}`"));
            let fix = "see `bar --help`; `bar plugin list` shows the plugins found";
            let fmt = match cli_out::format_in(args) {
                "table" => format,
                f => f,
            };
            cli_out::fail_as(fmt, "", cli_out::with_fix(err, fix));
        }
    };
    let mut c = std::process::Command::new(&found.path);
    c.args(args);
    // the user's `account` rules, for a plugin that declared it wants them
    if plugin::info_now(&cache, &found).inputs.accounts {
        let rules = plugin::load_account_rules(None);
        if rules.as_array().is_some_and(|a| !a.is_empty()) {
            c.env(plugin::ACCOUNTS_ENV, rules.to_string());
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let e = c.exec();
        eprintln!("bar {name}: {}: {e}", found.path.display());
        std::process::exit(126);
    }
    #[cfg(not(unix))]
    match c.status() {
        Ok(st) => std::process::exit(st.code().unwrap_or(1)),
        Err(e) => {
            eprintln!("bar {name}: {}: {e}", found.path.display());
            std::process::exit(126);
        }
    }
}

fn refresh(raw: &str) {
    if let Ok(req) = Request::from_json(raw) {
        let limit = refresh_timeout(std::env::var("BAR_REFRESH_TIMEOUT").ok().as_deref());
        Cache::new(&click::state_dir()).refresh(&req, limit);
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

fn init(m: &ArgMatches) {
    cli_out::check_format(m, "init");
    let name = m
        .get_one::<String>("preset")
        .map_or(hop_bar::presets::DEFAULT, String::as_str);
    let home = hop_bar::platform::home().unwrap_or_default();
    let r = hop_bar::presets::init(
        name,
        &|k| std::env::var(k).ok(),
        &home,
        &hop_bar::platform::dirs::config(),
    );
    match r {
        Ok(p) => {
            let text = format!("wrote {} (preset {name})\n", p.display());
            let data = json!({"path": p.display().to_string(), "preset": name});
            cli_out::emit(m, "init", &text, &data);
        }
        Err(e) if e.ends_with(hop_bar::presets::EXISTS) => cli_out::fail(
            m,
            "init",
            cli_out::with_fix(
                CliError::conflict(e),
                "edit that file, or move it aside and rerun bar init",
            ),
        ),
        Err(e) => cli_out::fail(m, "init", CliError::usage(e)),
    }
}

fn update(m: &ArgMatches) {
    use hop_bar::{cache::Job, update};
    cli_out::check_format(m, "update");
    let on = matches!(conf::load(cwd().as_deref()), conf::LoadResult::Ok(c) if c.update_check);
    let cache = Cache::new(&click::state_dir());
    let key = update::cache_key(update::channel_is_prerelease(update::current()));
    let cached = on.then(|| {
        let pre = update::channel_is_prerelease(update::current());
        // a stale result also starts the daily check in the background
        cache.read_or_refresh(key, update::TTL, &Job::Update { prerelease: pre })
    });
    let exe = std::env::current_exe().unwrap_or_else(|_| "bar".into());
    let cached = cached.as_ref().map(|c| c.as_deref());
    let text = update::report(update::current(), cached, &exe);
    let age = std::fs::metadata(cache.path_for(key))
        .and_then(|md| md.modified())
        .ok()
        .and_then(|t| t.elapsed().ok())
        .map(|d| d.as_secs());
    let data = update::report_json(update::current(), cached, age, &exe);
    cli_out::emit(m, "update", &text, &data);
}

fn install(m: &ArgMatches) {
    use hop_bar::install::{self, Error, Scope};
    cli_out::check_format(m, "install");
    let scope = if m.get_flag("project") {
        Scope::Project
    } else if m.get_flag("local") {
        Scope::Local
    } else {
        Scope::User
    };
    let cwd = std::env::current_dir().unwrap_or_default();
    let settings = install::settings_path(scope, click::claude_dir(), &cwd)
        .unwrap_or_else(|e| cli_out::fail(m, "install", CliError::usage(e)));
    let env = install::Env {
        bar: install::bar_for(
            scope,
            std::env::current_exe().ok().map(|e| {
                install::command_bar(&e, std::env::var_os("PATH").as_deref(), cfg!(windows))
            }),
        ),
        settings,
        state_dir: click::state_dir(),
        windows: cfg!(windows),
    };
    let opts = install::Opts {
        uninstall: m.get_flag("uninstall"),
        refresh: m.get_one::<u64>("refresh").copied(),
        hooks: m.get_flag("hooks"),
        subagent: m.get_flag("subagent"),
        subagent_only: m.get_flag("subagent_only"),
        yes: m.get_flag("yes"),
    };
    let tty = std::io::IsTerminal::is_terminal(&std::io::stdin());
    let r = install::run_outcome(
        &env,
        &opts,
        tty,
        &mut std::io::stdin().lock(),
        &mut cli_out::human(m),
    );
    match r {
        Ok(o) => {
            let data = json!({
                "settings": o.path.display().to_string(),
                "written": o.written,
                "backup": o.backup.map(|b| b.display().to_string()),
            });
            cli_out::emit(m, "install", "", &data);
        }
        Err(e) => {
            let err = match e {
                Error::NeedsConfirm | Error::Declined => cli_out::with_fix(
                    CliError::consent_refused(e.to_string()),
                    "rerun with --yes to write",
                ),
                Error::Failed(_) => CliError::generic(e.to_string()),
            };
            cli_out::fail(m, "install", err);
        }
    }
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

fn click_cmd(m: &ArgMatches) {
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

fn plugin_cmd(m: &ArgMatches) {
    let dirs = plugin::plugin_dirs(std::env::var_os("PATH").as_deref());
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
                let err = CliError::not_found(format!("no plugin `{name}` on PATH"));
                let fix = "`bar plugin list` shows the plugins found";
                cli_out::fail(i, "plugin info", cli_out::with_fix(err, fix));
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

fn web_cmd(m: &ArgMatches) {
    let Some(("plugins", p)) = m.subcommand() else {
        return;
    };
    let given: Vec<std::path::PathBuf> = p
        .get_many::<std::path::PathBuf>("dir")
        .into_iter()
        .flatten()
        .map(|d| std::path::absolute(d).unwrap_or_else(|_| d.clone()))
        .collect();
    let dirs = if given.is_empty() {
        plugin::plugin_dirs(std::env::var_os("PATH").as_deref())
    } else {
        given
    };
    let cache = Cache::new(&click::state_dir());
    let infos: Vec<_> = plugin::discover(&dirs)
        .iter()
        .map(|f| plugin::info_now(&cache, f))
        .collect();
    let body = format!(
        "{}\n",
        serde_json::to_string_pretty(&plugin::web::manifest(&infos)).unwrap_or_default()
    );
    // the global `-o/--output` (hop-top-kit output flags); `-` is stdout
    let out = p.get_one::<String>("output").map_or("", String::as_str);
    let force = p.get_flag("force");
    let leaf = p;
    let written = match out {
        "" | "-" => std::io::stdout()
            .write_all(body.as_bytes())
            .map_err(|e| e.to_string()),
        path => {
            let p = std::path::Path::new(path);
            if p.exists() && !force {
                let err = CliError::conflict(format!("{path} exists; pass --force to overwrite"));
                cli_out::fail(leaf, "web plugins", err);
            }
            hop_bar::rewrite::target(p).and_then(|t| {
                hop_bar::rewrite::write(&t, &body).map_err(|e| format!("{path}: {e}"))
            })
        }
    };
    if let Err(e) = written {
        eprintln!("bar web plugins: {e}");
        std::process::exit(1);
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
    // the session's directory picks the project config; Claude Code runs
    // the status line command there too
    let dir = Some(std::path::PathBuf::from(&s.cwd))
        .filter(|d| d.is_absolute())
        .or_else(cwd);
    let load = conf::load(dir.as_deref());
    let out = hop_bar::render::run(&s, load, columns, &runner, Some(&cache));
    if !out.is_empty() {
        // a closed pipe must not panic; render always exits 0
        let mut so = std::io::stdout().lock();
        let _ = writeln!(so, "{out}");
        let _ = so.flush();
    }
    // after the output: publish the state dir for the bar:// handler
    if cfg!(target_os = "macos") {
        if let Some(d) = click::pointer::default_dir() {
            click::pointer::sync(cache.state_dir(), &d, std::time::SystemTime::now());
        }
    }
}

/// Largest `subagentStatusLine` input read.
const MAX_SUBAGENT_INPUT: u64 = 4 * 1024 * 1024;

fn subagent() {
    use std::io::Read as _;
    let mut buf = Vec::new();
    let _ = std::io::stdin()
        .take(MAX_SUBAGENT_INPUT)
        .read_to_end(&mut buf);
    let cache = Cache::new(&click::state_dir());
    let runner = CachedRunner::new(cache.clone());
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    // the payload's directory picks the project config, as for render
    let load = |p: &hop_bar::subagent::Payload| {
        let dir = Some(std::path::PathBuf::from(&p.cwd))
            .filter(|d| d.is_absolute())
            .or_else(cwd);
        conf::load(dir.as_deref())
    };
    let input = String::from_utf8_lossy(&buf);
    let out = hop_bar::subagent::run(&input, load, now, &runner, Some(&cache));
    if !out.is_empty() {
        let mut so = std::io::stdout().lock();
        let _ = so.write_all(out.as_bytes());
        let _ = so.flush();
    }
}

fn cwd() -> Option<std::path::PathBuf> {
    std::env::current_dir().ok()
}
