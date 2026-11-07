//! Toggle state and the macOS URL-handler app (`bar click install`).

use std::io::Write;
use std::path::{Path, PathBuf};

const KEYS: [&str; 2] = ["5h", "7d"];
const MARKER: &str = "click-installed";
const LSREGISTER: &str = "/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister";

/// `BAR_STATE_DIR` overrides the kit's state dir (tests, relocation).
pub fn state_dir() -> PathBuf {
    std::env::var_os("BAR_STATE_DIR")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| hop_top_kit::xdg::raw_state_dir("bar"))
}

pub fn is_toggled(dir: &Path, key: &str) -> bool {
    dir.join(format!("pct-{key}")).exists()
}

/// The install step writes this; links need a handler to open them.
pub fn handler_installed(dir: &Path) -> bool {
    dir.join(MARKER).exists()
}

pub(crate) fn write_private(path: &Path, body: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut o, 0o600);
    o.open(path)?.write_all(body.as_bytes())
}

/// Flips `pct-<key>`; `Ok(true)` when the percentage view is now on.
pub fn toggle(dir: &Path, key: &str) -> Result<bool, String> {
    if !KEYS.contains(&key) {
        return Err(format!("unknown key `{key}` (expected 5h or 7d)"));
    }
    let f = dir.join(format!("pct-{key}"));
    if f.exists() {
        std::fs::remove_file(&f).map_err(|e| e.to_string())?;
        return Ok(false);
    }
    write_private(&f, "").map_err(|e| e.to_string())?;
    Ok(true)
}

pub struct InstallCfg {
    pub bar: PathBuf,
    pub state_dir: PathBuf,
    pub app: PathBuf,
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

/// Handler for `bar://toggle/<key>`; `bar toggle` validates the key.
pub fn applescript(bar: &Path) -> String {
    format!(
        "on open location theURL\n\
         \tset thePrefix to \"bar://toggle/\"\n\
         \tif (count of theURL) > (count of thePrefix) and theURL starts with thePrefix then\n\
         \t\tset theKey to text ((count of thePrefix) + 1) thru -1 of theURL\n\
         \t\tdo shell script quoted form of \"{}\" & \" toggle \" & quoted form of theKey\n\
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
    let plan = install_plan(cfg);
    if dry_run {
        writeln!(out, "{}", plan.script).map_err(io)?;
        for c in &plan.commands {
            writeln!(out, "{}", c.join(" ")).map_err(io)?;
        }
        return Ok(());
    }
    if let Some(parent) = cfg.app.parent() {
        std::fs::create_dir_all(parent).map_err(io)?;
    }
    write_private(&cfg.script_path(), &plan.script).map_err(io)?;
    for c in &plan.commands {
        run(c).map_err(|e| format!("{}: {e}", c[0]))?;
    }
    write_private(&cfg.state_dir.join(MARKER), "").map_err(io)?;
    writeln!(out, "installed {}", cfg.app.display()).map_err(io)
}

/// Runs a real command; fails on spawn error or non-zero exit.
pub fn exec(cmd: &[String]) -> Result<(), String> {
    let st = std::process::Command::new(&cmd[0])
        .args(&cmd[1..])
        .status()
        .map_err(|e| e.to_string())?;
    st.success().then_some(()).ok_or_else(|| st.to_string())
}
