//! `bar click install`: the macOS `bar://` URL handler app and the
//! waiting-state hook in Claude Code's `settings.json`.

use super::dirs::{follows_pointer, handler_env, shell_command, EnvVar, HandlerDirs};
use super::settings::{apply_settings, plan_settings};
use super::{claude_dir, state_dir, write_private, MARKER, MARKER_CMD};
use std::io::Write;
use std::path::{Path, PathBuf};

const LSREGISTER: &str = "/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister";

pub struct InstallCfg {
    pub bar: PathBuf,
    pub state_dir: PathBuf,
    pub app: PathBuf,
    /// Claude Code `settings.json` that receives the hook.
    pub settings: PathBuf,
    /// The dir variables baked into the handler (`HANDLER_ENV`).
    pub env: Vec<EnvVar>,
}

impl InstallCfg {
    /// Fails without an absolute Claude config dir (`HOME` or
    /// `CLAUDE_CONFIG_DIR`), so nothing lands under the working directory.
    pub fn from_env() -> Result<InstallCfg, String> {
        let claude = claude_dir().ok_or(
            "HOME must be an absolute path (or CLAUDE_CONFIG_DIR, when set, must be absolute)",
        )?;
        let apps = std::env::var_os("BAR_APP_DIR")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join("Applications")
            });
        Ok(InstallCfg {
            bar: std::env::current_exe().map_or_else(
                |_| PathBuf::from("bar"),
                |e| {
                    let path = std::env::var_os("PATH");
                    crate::install::command_bar(&e, path.as_deref(), cfg!(windows))
                },
            ),
            state_dir: state_dir(),
            app: apps.join("bar-click.app"),
            settings: claude.join("settings.json"),
            env: handler_env(&|k| std::env::var(k).ok()),
        })
    }

    fn script_path(&self) -> PathBuf {
        self.state_dir.join("click-handler.applescript")
    }

    /// `handler_warnings` for this handler against this environment and
    /// the renderer's pointer (macOS; none elsewhere).
    pub fn warnings(&self) -> Vec<String> {
        let pointer = cfg!(target_os = "macos")
            .then(super::pointer::default_dir)
            .flatten()
            .and_then(|d| super::pointer::read(&d));
        let claude = self.settings.parent();
        super::dirs::handler_warnings(&self.app, &self.state_dir, claude, pointer.as_deref())
    }

    /// The dirs the handler resolves; the Claude dir is the parent of
    /// `settings`.
    pub fn handler_dirs(&self) -> HandlerDirs {
        HandlerDirs {
            state_dir: self.state_dir.clone(),
            claude_dir: self.settings.parent().map(Path::to_path_buf),
            follows_pointer: follows_pointer(&self.env),
        }
    }
}

pub struct InstallPlan {
    pub script: String,
    pub commands: Vec<Vec<String>>,
}

fn as_literal(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\r', "\\r")
}

/// Handler for `bar://toggle/<key>`, `bar://cmd/...` and
/// `bar://action/...`; `bar` validates all three. A refusal exits non-zero, so `try` keeps the applet quiet. Every
/// call runs `bar` under `env` (see `shell_command`): LaunchServices
/// starts the applet without the shell's environment.
pub fn applescript(bar: &Path, env: &[EnvVar]) -> String {
    format!(
        "on open location theURL\n\
         \tset thePrefix to \"bar://toggle/\"\n\
         \tset theBar to \"{}\"\n\
         \tif (count of theURL) > (count of thePrefix) and theURL starts with thePrefix then\n\
         \t\tset theKey to text ((count of thePrefix) + 1) thru -1 of theURL\n\
         \t\ttry\n\
         \t\t\tdo shell script theBar & \" toggle \" & quoted form of theKey\n\
         \t\tend try\n\
         \telse if theURL starts with \"bar://cmd/\" then\n\
         \t\ttry\n\
         \t\t\tdo shell script theBar & \" click run \" & quoted form of theURL\n\
         \t\tend try\n\
         \telse if theURL starts with \"bar://action/\" then\n\
         \t\ttry\n\
         \t\t\tdo shell script theBar & \" click action \" & quoted form of theURL\n\
         \t\tend try\n\
         \tend if\n\
         end open location\n",
        as_literal(&shell_command(bar, env))
    )
}

pub fn info_plist_commands(app: &Path) -> Vec<Vec<String>> {
    let plist = app.join("Contents/Info.plist").display().to_string();
    let edit = |key: &str, kind: &str, val: &str| {
        ["plutil", "-replace", key, kind, val, &plist]
            .map(String::from)
            .to_vec()
    };
    vec![
        edit("LSUIElement", "-bool", "true"),
        edit("CFBundleIdentifier", "-string", "top.hop.bar-click"),
        edit(
            "CFBundleURLTypes",
            "-json",
            r#"[{"CFBundleURLName":"bar","CFBundleURLSchemes":["bar"]}]"#,
        ),
    ]
}

pub fn install_plan(cfg: &InstallCfg) -> InstallPlan {
    let app = cfg.app.display().to_string();
    let mut commands = vec![vec![
        "osacompile".to_string(),
        "-o".into(),
        app.clone(),
        cfg.script_path().display().to_string(),
    ]];
    commands.extend(info_plist_commands(&cfg.app));
    commands.push(
        ["codesign", "--force", "--deep", "--sign", "-", &app]
            .map(String::from)
            .to_vec(),
    );
    commands.push(vec![LSREGISTER.into(), "-f".into(), app]);
    InstallPlan {
        script: applescript(&cfg.bar, &cfg.env),
        commands,
    }
}

/// Error from `run_install`; the CLI exits 2 on `Unsupported`, 1 otherwise.
#[derive(Debug, PartialEq, Eq)]
pub enum InstallError {
    /// The app bundle needs macOS; `--hooks-only` works anywhere.
    Unsupported,
    Failed(String),
}

impl std::fmt::Display for InstallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unsupported => f.write_str(
                "the bar:// URL handler needs macOS (not supported on this platform); \
                 use --hooks-only to register the Claude Code hooks only",
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

/// `macos` and `dry_run` are parameters so tests cover every path without
/// touching the machine; `run` executes one external command.
pub fn run_install(
    cfg: &InstallCfg,
    macos: bool,
    dry_run: bool,
    run: &mut dyn FnMut(&[String]) -> Result<(), String>,
    out: &mut dyn Write,
) -> Result<(), InstallError> {
    let io = |e: std::io::Error| e.to_string();
    if !macos {
        return Err(InstallError::Unsupported);
    }
    let settings = plan_settings(&cfg.settings, &cfg.bar)?;
    let plan = install_plan(cfg);
    if dry_run {
        writeln!(out, "{}", plan.script).map_err(io)?;
        for (i, c) in plan.commands.iter().enumerate() {
            writeln!(out, "{}", c.join(" ")).map_err(io)?;
            if i == 0 {
                let record = HandlerDirs::path(&cfg.app);
                write!(
                    out,
                    "write {}: {}",
                    record.display(),
                    cfg.handler_dirs().to_json()
                )
                .map_err(io)?;
            }
        }
        for l in settings.describe() {
            writeln!(out, "{l}").map_err(io)?;
        }
        return Ok(());
    }
    for l in settings.describe() {
        writeln!(out, "{l}").map_err(io)?;
    }
    if let Some(parent) = cfg.app.parent() {
        std::fs::create_dir_all(parent).map_err(io)?;
    }
    write_private(&cfg.script_path(), &plan.script).map_err(io)?;
    for (i, c) in plan.commands.iter().enumerate() {
        run(c).map_err(|e| format!("{}: {e}", c[0]))?;
        // into the bundle osacompile made, before codesign seals it
        if i == 0 {
            let record = HandlerDirs::path(&cfg.app);
            write_private(&record, &cfg.handler_dirs().to_json()).map_err(io)?;
        }
    }
    // Re-read right before writing: settings.json may have changed meanwhile.
    let fresh = plan_settings(&cfg.settings, &cfg.bar)?;
    if fresh.describe() != settings.describe() {
        for l in fresh.describe() {
            writeln!(out, "{l}").map_err(io)?;
        }
    }
    if let Some(backup) = apply_settings(&fresh, &cfg.state_dir).map_err(io)? {
        writeln!(out, "backup {}", backup.display()).map_err(io)?;
    }
    write_private(&cfg.state_dir.join(MARKER), MARKER_CMD).map_err(io)?;
    // the secret that makes `bar://action/` links unforgeable by pages
    super::action::ensure_nonce(&cfg.state_dir)?;
    writeln!(out, "installed {}", cfg.app.display()).map_err(io)?;
    writeln!(out, "state dir {}", cfg.state_dir.display()).map_err(io)?;
    for w in &cfg.warnings() {
        writeln!(out, "warning: {w}").map_err(io)?;
    }
    Ok(())
}

/// `--hooks-only`: wires the `bar click hook` entries into `settings.json`
/// and nothing else (no app bundle, any platform).
pub fn run_hooks_only(cfg: &InstallCfg, dry_run: bool, out: &mut dyn Write) -> Result<(), String> {
    let io = |e: std::io::Error| e.to_string();
    let plan = plan_settings(&cfg.settings, &cfg.bar)?;
    for l in plan.describe() {
        writeln!(out, "{l}").map_err(io)?;
    }
    if dry_run {
        return Ok(());
    }
    if let Some(backup) = apply_settings(&plan, &cfg.state_dir).map_err(io)? {
        writeln!(out, "backup {}", backup.display()).map_err(io)?;
    }
    Ok(())
}

/// Runs a real command; fails on spawn error or non-zero exit.
pub fn exec(cmd: &[String]) -> Result<(), String> {
    let st = std::process::Command::new(&cmd[0])
        .args(&cmd[1..])
        .status()
        .map_err(|e| e.to_string())?;
    st.success().then_some(()).ok_or_else(|| st.to_string())
}
