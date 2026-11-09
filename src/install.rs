//! `bar install`: sets bar as Claude Code's `statusLine` in one of its
//! settings files, showing the diff and asking first; `--uninstall`
//! puts back the status line bar replaced, or removes bar's.

use crate::click::settings::{self, add_hooks, command_for, load, remove_hooks, write_settings};
use serde_json::{json, Value};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

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

/// Whether `cmd` runs `bar render` (any path to a `bar` binary, quoted
/// or not).
pub fn is_bar_command(cmd: &str) -> bool {
    settings::runs_bar(cmd, "render")
}

fn is_bar_status_line(v: Option<&Value>) -> bool {
    v.and_then(|s| s.get("command"))
        .and_then(Value::as_str)
        .is_some_and(is_bar_command)
}

/// Where install keeps the status line it replaced in `settings`.
pub fn record_path(state_dir: &Path, settings: &Path) -> PathBuf {
    let key = settings.display().to_string();
    state_dir
        .join("install")
        .join(format!("{}.json", crate::cache::hash_hex(&key)))
}

fn read_record(path: &Path) -> Option<Value> {
    let v: Value = serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
    v.get("statusLine").cloned()
}

/// What to do with the record of the replaced status line.
enum Record {
    Keep,
    Write(Value),
    Remove,
}

struct Plan {
    path: PathBuf,
    text: Option<String>,
    after: Value,
    record: Record,
    notes: Vec<String>,
}

impl Plan {
    fn changed(&self) -> bool {
        let before: Value = self
            .text
            .as_deref()
            .and_then(|t| serde_json::from_str(t).ok())
            .unwrap_or_else(|| json!({}));
        before != self.after
    }

    fn diff(&self) -> String {
        let shown = self.path.display().to_string();
        let old_name = if self.text.is_some() {
            &shown
        } else {
            "/dev/null"
        };
        crate::diff::unified(
            self.text.as_deref().unwrap_or(""),
            &settings::render(&self.after),
            old_name,
            &shown,
        )
    }
}

fn plan_install(env: &Env, o: &Opts) -> Result<Plan, String> {
    let l = load(&env.settings)?;
    let shown = l.path.display().to_string();
    let rec = record_path(&env.state_dir, &l.path);
    let mut root = l.root.clone();
    let obj = root.as_object_mut().ok_or("not a JSON object")?;
    let bar = env.bar.display().to_string();
    let cur = obj.get("statusLine").cloned();
    let record = match &cur {
        Some(c) if !is_bar_status_line(Some(c)) => Record::Write(c.clone()),
        _ => Record::Keep,
    };
    let mut line = match &cur {
        Some(Value::Object(m)) => Value::Object(m.clone()),
        _ => json!({}),
    };
    line["type"] = json!("command");
    line["command"] = json!(render_command(&bar, env.windows));
    if let Some(n) = o.refresh {
        line["refreshInterval"] = json!(n);
    }
    obj.insert("statusLine".into(), line);
    let mut notes = Vec::new();
    if matches!(record, Record::Write(_)) {
        notes.push(format!(
            "the current status line is kept in {}; bar install --uninstall puts it back",
            rec.display()
        ));
    }
    if o.hooks {
        add_hooks(
            &mut root,
            &command_for(&bar, "click hook", env.windows),
            &shown,
        )?;
    }
    Ok(Plan {
        path: l.path,
        text: l.text,
        after: root,
        record,
        notes,
    })
}

fn plan_uninstall(env: &Env, o: &Opts) -> Result<Plan, String> {
    let l = load(&env.settings)?;
    let rec = record_path(&env.state_dir, &l.path);
    let mut root = l.root.clone();
    let obj = root.as_object_mut().ok_or("not a JSON object")?;
    let mut notes = Vec::new();
    let mut record = Record::Keep;
    if is_bar_status_line(obj.get("statusLine")) {
        match read_record(&rec) {
            Some(prev) => {
                obj.insert("statusLine".into(), prev);
                notes.push("restores the status line bar replaced".into());
            }
            None => {
                obj.shift_remove("statusLine");
            }
        }
        record = Record::Remove;
    } else if obj.contains_key("statusLine") {
        notes.push("statusLine does not run bar; left as is".into());
    }
    if o.hooks {
        remove_hooks(&mut root);
    }
    Ok(Plan {
        path: l.path,
        text: l.text,
        after: root,
        record,
        notes,
    })
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
        return Ok(());
    }
    write!(out, "{}", plan.diff()).map_err(io)?;
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
    if let Record::Write(prev) = &plan.record {
        let body = json!({"settings": plan.path.display().to_string(), "statusLine": prev});
        crate::click::write_atomic(&rec, &(body.to_string() + "\n")).map_err(io)?;
    }
    let backup = write_settings(&plan.path, &plan.after, &env.state_dir).map_err(io)?;
    if let Record::Remove = plan.record {
        let _ = std::fs::remove_file(&rec);
    }
    if let Some(b) = backup {
        writeln!(out, "backup {}", b.display()).map_err(io)?;
    }
    writeln!(out, "wrote {}", plan.path.display()).map_err(io)
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
