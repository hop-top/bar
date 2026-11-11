//! `bar tray install|uninstall`. Install records the dirs, the `bar` and
//! the variables the tray runs with (`tray-dirs.json`), writes the
//! `tray-installed` marker, registers the Claude Code hooks the session
//! rows read, prints `tray` lines to consider (the config is left alone)
//! and sets up autostart: on macOS a LaunchAgent, loaded at once.
//! Independent of `bar click install`: either works without the other,
//! and uninstalling one keeps the hooks while the other is installed.

use super::process::{self, Layout, Sys};
use super::{agent, SNAPSHOT_FILE};
use crate::click::pointer::TRAY_MARKER;
use crate::click::{settings, EnvVar, HandlerDirs};
use crate::conf::Config;
use serde_json::{json, Map, Value};
use std::io::Write;
use std::path::{Path, PathBuf};

/// What install records for the tray and `bar tray start`.
pub const DIRS_FILE: &str = "tray-dirs.json";
/// Marker contents when install added the hooks, so uninstall may take
/// them out again.
const MARKER_HOOKS: &str = "hooks";
const TRAY_NAME: &str = if cfg!(windows) {
    "bar-tray.exe"
} else {
    "bar-tray"
};

pub struct InstallCfg {
    /// Absolute `bar` the tray runs.
    pub bar: PathBuf,
    /// The `bar` the hook entries run (see `install::command_bar`).
    pub hook_bar: PathBuf,
    /// Absolute `bar-tray`.
    pub tray: PathBuf,
    /// Claude Code `settings.json` that receives the hooks.
    pub settings: PathBuf,
    /// The dir variables baked into the tray (`HANDLER_ENV`).
    pub env: Vec<EnvVar>,
    pub layout: Layout,
}

/// The `bar-tray` installed beside `bar`.
pub fn tray_beside(bar: &Path) -> PathBuf {
    bar.with_file_name(TRAY_NAME)
}

impl InstallCfg {
    /// Fails without an absolute Claude config dir or home, so nothing
    /// lands under the working directory.
    pub fn from_env() -> Result<InstallCfg, String> {
        let claude = crate::click::claude_dir().ok_or(
            "HOME must be an absolute path (or CLAUDE_CONFIG_DIR, when set, must be absolute)",
        )?;
        let layout = Layout::from_env();
        if cfg!(target_os = "macos") && layout.agents.is_none() {
            return Err("HOME must be an absolute path".into());
        }
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let path = std::env::var_os("PATH");
        let bar = crate::install::stable_exe(&exe, path.as_deref());
        let tray = Some(tray_beside(&bar))
            .filter(|t| t.is_file())
            .unwrap_or_else(|| tray_beside(&exe));
        Ok(InstallCfg {
            hook_bar: crate::install::command_bar(&exe, path.as_deref(), cfg!(windows)),
            bar,
            tray,
            settings: claude.join("settings.json"),
            env: crate::click::handler_env(&|k| std::env::var(k).ok()),
            layout,
        })
    }

    /// The tray's command line: absolute paths, the lock that keeps it
    /// single.
    pub fn argv(&self) -> Vec<String> {
        argv(&self.tray, &self.bar, &self.layout.lock())
    }

    /// Variables set at install, as the LaunchAgent sets them.
    fn baked_env(&self) -> Vec<(String, String)> {
        self.env
            .iter()
            .filter_map(|(k, v)| Some((k.clone(), v.clone()?)))
            .collect()
    }

    /// `tray-dirs.json`: the paths, the dirs they resolve and the
    /// variables (`null` when unset at install).
    pub fn record(&self) -> String {
        let dirs = HandlerDirs {
            state_dir: self.layout.state_dir.clone(),
            claude_dir: self.settings.parent().map(Path::to_path_buf),
            follows_pointer: crate::click::follows_pointer(&self.env),
        };
        let env: Map<String, Value> = self
            .env
            .iter()
            .map(|(k, v)| (k.clone(), v.as_deref().map_or(Value::Null, Value::from)))
            .collect();
        let v = json!({
            "bar": self.bar.display().to_string(),
            "tray": self.tray.display().to_string(),
            "state_dir": dirs.state_dir.display().to_string(),
            "claude_dir": dirs.claude_dir.as_ref().map(|p| p.display().to_string()),
            "follows_pointer": dirs.follows_pointer,
            "env": env,
        });
        format!("{v}\n")
    }
}

fn argv(tray: &Path, bar: &Path, lock: &Path) -> Vec<String> {
    [tray, Path::new("--bar"), bar, Path::new("--lock"), lock]
        .map(|p| p.display().to_string())
        .to_vec()
}

/// The tray command line from `tray-dirs.json` in `state`, when there is
/// one with both paths.
pub fn recorded_argv(state: &Path) -> Option<Vec<String>> {
    let s = std::fs::read_to_string(state.join(DIRS_FILE)).ok()?;
    let v: Value = serde_json::from_str(&s).ok()?;
    let path = |k: &str| v.get(k)?.as_str().map(PathBuf::from);
    Some(argv(
        &path("tray")?,
        &path("bar")?,
        &state.join(process::LOCK_FILE),
    ))
}

pub struct InstallOpts {
    pub dry_run: bool,
    /// Set up autostart (the LaunchAgent on macOS).
    pub autostart: bool,
    /// Register the Claude Code hooks.
    pub hooks: bool,
}

/// Error from `install`; the CLI exits 2 on `Unsupported`, 1 otherwise.
#[derive(Debug, PartialEq, Eq)]
pub enum InstallError {
    /// No autostart for this platform yet; `--no-autostart` works.
    Unsupported,
    Failed(String),
}

impl std::fmt::Display for InstallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unsupported => f.write_str(
                "tray autostart is not supported on this platform yet; \
                 use --no-autostart and start it with bar tray start",
            ),
            Self::Failed(e) => f.write_str(e),
        }
    }
}

impl From<String> for InstallError {
    fn from(e: String) -> Self {
        Self::Failed(e)
    }
}

/// `tray <plugin>.<action>` for every `action` line whose action no
/// `tray` line covers: `action` lines do not grant the tray, so these
/// are only offered.
pub fn suggestions(c: &Config) -> Vec<String> {
    let covered = |p: &str, a: &str| {
        c.tray
            .iter()
            .any(|g| g.plugin == p && g.action.as_deref().is_none_or(|x| x == a))
    };
    let mut out: Vec<String> = Vec::new();
    for b in c.actions.iter().filter(|b| !covered(&b.plugin, &b.action)) {
        let line = format!("tray {}.{}", b.plugin, b.action);
        if !out.contains(&line) {
            out.push(line);
        }
    }
    out
}

fn print_suggestions(c: &Config, out: &mut dyn Write) -> std::io::Result<()> {
    let lines = suggestions(c);
    if lines.is_empty() {
        if c.tray.is_empty() {
            writeln!(
                out,
                "no plugin menu is granted: add `tray <plugin>` lines to your config"
            )?;
        }
        return Ok(());
    }
    writeln!(
        out,
        "your `action` lines do not grant the tray; to show them there, add to your config:"
    )?;
    for l in lines {
        writeln!(out, "  {l}")?;
    }
    Ok(())
}

/// `macos` and `dry_run` are parameters so tests cover every path without
/// touching the machine; `sys` runs `launchctl`.
pub fn install(
    cfg: &InstallCfg,
    o: &InstallOpts,
    macos: bool,
    config: &Config,
    sys: &mut dyn Sys,
    out: &mut dyn Write,
) -> Result<(), InstallError> {
    let io = |e: std::io::Error| e.to_string();
    let agent = match (o.autostart, macos, cfg.layout.plist()) {
        (false, ..) => None,
        (true, true, Some(p)) => Some((
            p,
            cfg.layout
                .uid
                .ok_or_else(|| "no user id for launchctl".to_string())?,
        )),
        (true, ..) => return Err(InstallError::Unsupported),
    };
    if !cfg.tray.is_file() {
        let msg = format!("no bar-tray at {}", cfg.tray.display());
        if !o.dry_run {
            return Err(InstallError::Failed(msg));
        }
        writeln!(out, "warning: {msg}").map_err(io)?;
    }
    let state = &cfg.layout.state_dir;
    // hooks already there (another install) stay that install's; a
    // reinstall keeps what the first one added
    let ours = added_hooks(state)
        || o.hooks
            && !settings::plan_settings(&cfg.settings, &cfg.hook_bar)?
                .added
                .is_empty();
    let plist = agent
        .as_ref()
        .map(|(p, _)| (p, agent::plist(&cfg.argv(), &cfg.baked_env())));
    if o.dry_run {
        writeln!(
            out,
            "write {}: {}",
            state.join(DIRS_FILE).display(),
            cfg.record().trim_end()
        )
        .map_err(io)?;
        writeln!(out, "write {}", state.join(TRAY_MARKER).display()).map_err(io)?;
    } else {
        crate::click::write_private(&state.join(DIRS_FILE), &cfg.record()).map_err(io)?;
        let marker = if ours { MARKER_HOOKS } else { "" };
        crate::click::write_private(&state.join(TRAY_MARKER), marker).map_err(io)?;
    }
    if o.hooks {
        crate::click::register_hooks(&cfg.settings, &cfg.hook_bar, state, o.dry_run, out)?;
    }
    if let (Some((path, body)), Some((_, uid))) = (&plist, &agent) {
        let load = agent::bootstrap(*uid, path);
        if o.dry_run {
            write!(out, "write {}:\n{body}", path.display()).map_err(io)?;
            writeln!(out, "{}", agent::bootout(*uid).join(" ")).map_err(io)?;
            writeln!(out, "{}", load.join(" ")).map_err(io)?;
        } else {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir).map_err(io)?;
            }
            // a loaded older agent would keep its old arguments
            let _ = sys.exec(&agent::bootout(*uid));
            crate::click::write_private(path, body).map_err(io)?;
            sys.exec(&load).map_err(|e| format!("launchctl: {e}"))?;
            writeln!(out, "installed {}", path.display()).map_err(io)?;
        }
    }
    print_suggestions(config, out).map_err(io)?;
    if !o.dry_run {
        writeln!(out, "state dir {}", state.display()).map_err(io)?;
        if agent.is_none() {
            writeln!(out, "start it with: bar tray start").map_err(io)?;
        }
    }
    Ok(())
}

/// Whether the tray install in `state` added the hooks.
fn added_hooks(state: &Path) -> bool {
    std::fs::read_to_string(state.join(TRAY_MARKER)).is_ok_and(|s| s.trim() == MARKER_HOOKS)
}

fn remove(path: &Path, out: &mut dyn Write) -> Result<(), String> {
    match std::fs::remove_file(path) {
        Ok(()) => writeln!(out, "removed {}", path.display()).map_err(|e| e.to_string()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

/// Takes out the hook entries running `bar click hook` (backup first).
fn remove_hooks(path: &Path, state: &Path, out: &mut dyn Write) -> Result<(), String> {
    let l = settings::load(path)?;
    let mut root = l.root.clone();
    let events = settings::remove_hooks(&mut root);
    if events.is_empty() {
        return Ok(());
    }
    let io = |e: std::io::Error| e.to_string();
    if let Some(b) =
        settings::write_settings(&l.path, l.text.as_deref(), &root, state).map_err(io)?
    {
        writeln!(out, "backup {}", b.display()).map_err(io)?;
    }
    writeln!(
        out,
        "removed hooks from {}: {}",
        l.path.display(),
        events.join(", ")
    )
    .map_err(io)
}

/// Stops the tray, removes the LaunchAgent, the snapshot, the record,
/// the lock and the marker, and the hooks when this install added them,
/// `bar click install` is not installed and `keep_hooks` is off.
pub fn uninstall(
    l: &Layout,
    settings_path: &Path,
    keep_hooks: bool,
    sys: &mut dyn Sys,
    out: &mut dyn Write,
) -> Result<(), String> {
    let state = &l.state_dir;
    let added_hooks = added_hooks(state);
    process::stop(l, sys, out)?;
    if let Some(p) = l.plist() {
        remove(&p, out)?;
    }
    for f in [SNAPSHOT_FILE, DIRS_FILE, process::LOCK_FILE, TRAY_MARKER] {
        remove(&state.join(f), out)?;
    }
    let io = |e: std::io::Error| e.to_string();
    if !added_hooks {
        return Ok(());
    }
    if keep_hooks {
        writeln!(out, "hooks kept (--keep-hooks)").map_err(io)
    } else if crate::click::handler_installed(state) {
        writeln!(out, "hooks kept: bar click install uses them").map_err(io)
    } else {
        remove_hooks(settings_path, state, out)
    }
}
