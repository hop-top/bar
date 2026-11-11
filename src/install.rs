//! `bar install`: sets bar as Claude Code's `statusLine` (and with
//! `--subagent` its `subagentStatusLine`) in one of its settings files,
//! showing the diff and asking first; `--uninstall` puts back what bar
//! replaced, or removes bar's.

use crate::click::settings::{self, command_for, write_settings};
use serde_json::{json, Value};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

mod plan;
mod preview;
use plan::{plan_install, plan_uninstall, Record};
pub use preview::{preview, Preview};

/// Which Claude Code settings file to edit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// `<claude dir>/settings.json`
    User,
    /// `.claude/settings.json` in the current directory
    Project,
    /// `.claude/settings.local.json` in the current directory
    Local,
}

/// The settings file of `scope`; `claude` is Claude Code's config dir
/// (absolute or `None`), `cwd` the working directory.
pub fn settings_path(scope: Scope, claude: Option<PathBuf>, cwd: &Path) -> Result<PathBuf, String> {
    match scope {
        Scope::User => claude.map(|d| d.join("settings.json")).ok_or_else(|| {
            "HOME must be an absolute path (or CLAUDE_CONFIG_DIR, when set, must be absolute)"
                .into()
        }),
        Scope::Project => Ok(cwd.join(".claude").join("settings.json")),
        Scope::Local => Ok(cwd.join(".claude").join("settings.local.json")),
    }
}

/// A path to `exe` that outlives an upgrade: the first `bar` on `path`
/// (the `PATH` value) that is the same file, else for a Homebrew keg
/// (`<prefix>/Cellar/bar/<version>/bin/bar`) `<prefix>/bin/bar` when it is
/// the same file, else `exe`.
pub fn stable_exe(exe: &Path, path: Option<&std::ffi::OsStr>) -> PathBuf {
    let Ok(real) = crate::platform::canonicalize(exe) else {
        return exe.to_path_buf();
    };
    let same = |p: &Path| crate::platform::canonicalize(p).is_ok_and(|c| c == real);
    let name = exe.file_name().unwrap_or_default();
    let on_path = crate::plugin::path_dirs(path)
        .into_iter()
        .map(|d| d.join(name))
        .find(|p| same(p));
    if let Some(p) = on_path {
        return p;
    }
    let keg = exe.ancestors().nth(4).filter(|cellar| {
        cellar.file_name() == Some("Cellar".as_ref())
            && exe.ancestors().nth(3).and_then(Path::file_name) == Some("bar".as_ref())
            && exe.parent().and_then(Path::file_name) == Some("bin".as_ref())
    });
    match keg
        .and_then(Path::parent)
        .map(|pfx| pfx.join("bin").join(name))
    {
        Some(p) if same(&p) => p,
        _ => exe.to_path_buf(),
    }
}

/// The `bar` a command written into Claude Code's settings runs:
/// [`stable_exe`], except on Windows a path that would need quoting
/// becomes plain `bar` when that path is the `bar` found on `path`.
/// Claude Code runs commands through Git Bash or PowerShell, and no
/// quoting works in both; a bare `bar` does.
pub fn command_bar(exe: &Path, path: Option<&std::ffi::OsStr>, windows: bool) -> PathBuf {
    let stable = stable_exe(exe, path);
    let shown = stable.display().to_string();
    let quoted = command_for(&shown, "", windows).starts_with('\'');
    let first_on_path = || {
        let name = stable.file_name().unwrap_or_default();
        crate::plugin::path_dirs(path)
            .into_iter()
            .map(|d| d.join(name))
            .find(|p| p.is_file())
            .is_some_and(|p| p == stable)
    };
    if windows && quoted && first_on_path() {
        PathBuf::from("bar")
    } else {
        stable
    }
}

/// The `bar` that `scope`'s settings run: plain `bar` (found on `PATH`)
/// for the project settings, which are shared, else this binary.
pub fn bar_for(scope: Scope, exe: Option<PathBuf>) -> PathBuf {
    match (scope, exe) {
        (Scope::Project, _) | (_, None) => PathBuf::from("bar"),
        (_, Some(exe)) => exe,
    }
}

pub struct Opts {
    pub uninstall: bool,
    /// `statusLine.refreshInterval`, seconds (at least 1).
    pub refresh: Option<u64>,
    /// Also add (or with `uninstall` remove) the `bar click hook` entries.
    pub hooks: bool,
    /// Also set (or with `uninstall` remove) `subagentStatusLine`.
    pub subagent: bool,
    /// Set (or with `uninstall` remove) only `subagentStatusLine`;
    /// `statusLine` is left as it is.
    pub subagent_only: bool,
    /// Write without asking.
    pub yes: bool,
}

pub struct Env {
    /// The binary the commands run; plain `bar` is written unquoted.
    pub bar: PathBuf,
    pub settings: PathBuf,
    pub state_dir: PathBuf,
    /// Write commands for Claude Code on Windows.
    pub windows: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    /// No terminal to ask on and no `--yes`; nothing written (exit 2).
    NeedsConfirm,
    /// The answer was not yes; nothing written (exit 1).
    Declined,
    Failed(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NeedsConfirm => f.write_str("not a terminal; rerun with --yes to write"),
            Self::Declined => f.write_str("nothing written"),
            Self::Failed(e) => f.write_str(e),
        }
    }
}

impl From<String> for Error {
    fn from(e: String) -> Self {
        Self::Failed(e)
    }
}

/// The status line command: `'<bar>' render`.
pub fn render_command(bar: &str, windows: bool) -> String {
    command_for(bar, "render", windows)
}

/// The subagent status line command: `'<bar>' subagent`.
pub fn subagent_command(bar: &str, windows: bool) -> String {
    command_for(bar, "subagent", windows)
}

/// Whether `cmd` runs `bar render` (any path to a `bar` binary, quoted
/// or not).
pub fn is_bar_command(cmd: &str) -> bool {
    settings::runs_bar(cmd, "render")
}

/// Where install keeps the status line it replaced in `settings`.
pub fn record_path(state_dir: &Path, settings: &Path) -> PathBuf {
    let key = settings.display().to_string();
    state_dir
        .join("install")
        .join(format!("{}.json", crate::cache::hash_hex(&key)))
}

/// Plans the change, prints the diff, asks (unless `yes`; refuses
/// without a terminal), then backs up and writes. `tty` says whether
/// `input` is a terminal.
pub fn run(
    env: &Env,
    o: &Opts,
    tty: bool,
    input: &mut dyn BufRead,
    out: &mut dyn Write,
) -> Result<(), Error> {
    run_outcome(env, o, tty, input, out).map(|_| ())
}

/// What [`run_outcome`] did: the settings file, whether it was written,
/// and the backup of its old content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub path: PathBuf,
    pub written: bool,
    pub backup: Option<PathBuf>,
}

/// [`run`], returning what it did.
pub fn run_outcome(
    env: &Env,
    o: &Opts,
    tty: bool,
    input: &mut dyn BufRead,
    out: &mut dyn Write,
) -> Result<Outcome, Error> {
    let io = |e: std::io::Error| Error::Failed(e.to_string());
    let plan = if o.uninstall {
        plan_uninstall(env, o)?
    } else {
        plan_install(env, o)?
    };
    for n in &plan.notes {
        writeln!(out, "{n}").map_err(io)?;
    }
    if !plan.changed() {
        let what = if o.uninstall {
            "nothing to remove"
        } else {
            "already up to date"
        };
        writeln!(out, "{}: {what}", plan.path.display()).map_err(io)?;
        return Ok(Outcome {
            path: plan.path,
            written: false,
            backup: None,
        });
    }
    write!(out, "{}", plan.diff()).map_err(io)?;
    if plan.reformats() {
        writeln!(
            out,
            "{} is also reformatted (indent kept); the diff shows its content",
            plan.path.display()
        )
        .map_err(io)?;
    }
    if !o.yes {
        if !tty {
            return Err(Error::NeedsConfirm);
        }
        write!(out, "write {}? [y/N] ", plan.path.display()).map_err(io)?;
        out.flush().map_err(io)?;
        let mut answer = String::new();
        input.read_line(&mut answer).map_err(io)?;
        if !matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
            return Err(Error::Declined);
        }
    }
    crate::rewrite::unchanged(&plan.path, plan.text.as_deref())
        .map_err(|e| Error::Failed(format!("{e}; run bar install again")))?;
    let rec = record_path(&env.state_dir, &plan.path);
    if let Record::Write(keys) = &plan.record {
        let mut body = serde_json::Map::new();
        body.insert("settings".into(), json!(plan.path.display().to_string()));
        body.extend(keys.clone());
        let body = Value::Object(body).to_string() + "\n";
        crate::click::write_atomic(&rec, &body).map_err(io)?;
    }
    let backup = write_settings(
        &plan.path,
        plan.text.as_deref(),
        &plan.after,
        &env.state_dir,
    )
    .map_err(io)?;
    if let Record::Remove = plan.record {
        let _ = std::fs::remove_file(&rec);
    }
    if let Some(b) = &backup {
        writeln!(out, "backup {}", b.display()).map_err(io)?;
    }
    writeln!(out, "wrote {}", plan.path.display()).map_err(io)?;
    Ok(Outcome {
        path: plan.path,
        written: true,
        backup,
    })
}

#[cfg(test)]
mod tests {
    use super::{is_bar_command, render_command};

    #[test]
    fn bar_commands_are_recognised() {
        for c in [
            "bar render",
            "'/opt/my bar/bar' render",
            "/usr/local/bin/bar render",
            "C:/Users/me/bin/bar.exe render",
            r"'/o/it'\''s/bar' render",
        ] {
            assert!(is_bar_command(c), "{c}");
        }
        for c in [
            "bar",
            "bar check",
            "~/.claude/statusline.sh",
            "foobar render",
            "/bin/bar-x render",
        ] {
            assert!(!is_bar_command(c), "{c}");
        }
    }

    #[test]
    fn render_command_quotes_per_platform() {
        assert_eq!(
            render_command("/opt/my bar/bar", false),
            "'/opt/my bar/bar' render"
        );
        assert_eq!(
            render_command(r"C:\Users\me\bin\bar.exe", true),
            "C:/Users/me/bin/bar.exe render"
        );
        assert!(is_bar_command(&render_command(
            r"C:\Users\Jane Doe\bar.exe",
            true
        )));
    }
}
