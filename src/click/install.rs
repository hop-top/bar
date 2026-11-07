//! `bar click install`: the macOS `bar://` URL handler app and the
//! waiting-state hook in Claude Code's `settings.json`.

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
}

impl InstallCfg {
    pub fn from_env() -> InstallCfg {
        let apps = std::env::var_os("BAR_APP_DIR")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join("Applications")
            });
        InstallCfg {
            bar: std::env::current_exe().unwrap_or_else(|_| PathBuf::from("bar")),
            state_dir: state_dir(),
            app: apps.join("bar-click.app"),
            settings: claude_dir()
                .unwrap_or_else(|| PathBuf::from(".claude"))
                .join("settings.json"),
        }
    }

    fn script_path(&self) -> PathBuf {
        self.state_dir.join("click-handler.applescript")
    }
}

pub struct InstallPlan {
    pub script: String,
    pub commands: Vec<Vec<String>>,
}

fn as_literal(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Handler for `bar://toggle/<key>` and `bar://cmd/...`; `bar` validates
/// both. A refusal exits non-zero, so `try` keeps the applet quiet.
pub fn applescript(bar: &Path) -> String {
    format!(
        "on open location theURL\n\
         \tset thePrefix to \"bar://toggle/\"\n\
         \tset theBar to quoted form of \"{}\"\n\
         \tif (count of theURL) > (count of thePrefix) and theURL starts with thePrefix then\n\
         \t\tset theKey to text ((count of thePrefix) + 1) thru -1 of theURL\n\
         \t\ttry\n\
         \t\t\tdo shell script theBar & \" toggle \" & quoted form of theKey\n\
         \t\tend try\n\
         \telse if theURL starts with \"bar://cmd/\" then\n\
         \t\ttry\n\
         \t\t\tdo shell script theBar & \" click run \" & quoted form of theURL\n\
         \t\tend try\n\
         \tend if\n\
         end open location\n",
        as_literal(&bar.display().to_string())
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
        script: applescript(&cfg.bar),
        commands,
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
) -> Result<(), String> {
    let io = |e: std::io::Error| e.to_string();
    if !macos {
        writeln!(out, "bar click install: macOS only, nothing to do").map_err(io)?;
        return Ok(());
    }
    let settings = plan_settings(&cfg.settings, &cfg.bar)?;
    let plan = install_plan(cfg);
    if dry_run {
        writeln!(out, "{}", plan.script).map_err(io)?;
        for c in &plan.commands {
            writeln!(out, "{}", c.join(" ")).map_err(io)?;
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
    for c in &plan.commands {
        run(c).map_err(|e| format!("{}: {e}", c[0]))?;
    }
    // Re-read right before writing: settings.json may have changed meanwhile.
    let fresh = plan_settings(&cfg.settings, &cfg.bar)?;
    if fresh.describe() != settings.describe() {
        for l in fresh.describe() {
            writeln!(out, "{l}").map_err(io)?;
        }
    }
    if let Some(backup) = apply_settings(&fresh).map_err(io)? {
        writeln!(out, "backup {}", backup.display()).map_err(io)?;
    }
    write_private(&cfg.state_dir.join(MARKER), MARKER_CMD).map_err(io)?;
    writeln!(out, "installed {}", cfg.app.display()).map_err(io)
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
    if let Some(backup) = apply_settings(&plan).map_err(io)? {
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
