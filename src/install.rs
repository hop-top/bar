//! `bar install`: sets bar as Claude Code's `statusLine` (and with
//! `--subagent` its `subagentStatusLine`) in one of its settings files,
//! showing the diff and asking first; `--uninstall` puts back what bar
//! replaced, or removes bar's.

use crate::click::settings::{self, add_hooks, command_for, load, remove_hooks, write_settings};
use serde_json::{json, Value};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

mod preview;
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

/// The settings keys install writes: the key, the `bar` arguments its
/// command runs, and what it is called in messages.
struct Key {
    name: &'static str,
    args: &'static str,
    what: &'static str,
    flag: &'static str,
}

const STATUS: Key = Key {
    name: "statusLine",
    args: "render",
    what: "status line",
    flag: "",
};

const SUBAGENT: Key = Key {
    name: "subagentStatusLine",
    args: "subagent",
    what: "subagent status line",
    flag: " --subagent",
};

fn runs_bar(v: Option<&Value>, key: &Key) -> bool {
    v.and_then(|s| s.get("command"))
        .and_then(Value::as_str)
        .is_some_and(|c| settings::runs_bar(c, key.args))
}

/// Where install keeps the status line it replaced in `settings`.
pub fn record_path(state_dir: &Path, settings: &Path) -> PathBuf {
    let key = settings.display().to_string();
    state_dir
        .join("install")
        .join(format!("{}.json", crate::cache::hash_hex(&key)))
}

/// The record's settings values, by key; empty when there is none.
fn read_record(path: &Path) -> serde_json::Map<String, Value> {
    let v: Option<Value> = std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok());
    let mut m = match v {
        Some(Value::Object(m)) => m,
        _ => serde_json::Map::new(),
    };
    m.retain(|k, _| k == STATUS.name || k == SUBAGENT.name);
    m
}

/// What to do with the record of the replaced status lines.
enum Record {
    Keep,
    /// Write these keys (`statusLine`, `subagentStatusLine`).
    Write(serde_json::Map<String, Value>),
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

    /// Whether writing also changes the file's layout (one-line JSON,
    /// arrays kept on one line, ...), which the diff then leaves out.
    fn reformats(&self) -> bool {
        self.text
            .as_deref()
            .is_some_and(|t| !settings::same_layout(t))
    }

    /// The diff of the file as written; for a file whose layout changes,
    /// of its content in the written layout, so only real changes show.
    fn diff(&self) -> String {
        let shown = self.path.display().to_string();
        let old_name = if self.text.is_some() {
            &shown
        } else {
            "/dev/null"
        };
        let orig = self.text.as_deref();
        let before = match orig {
            Some(t) if self.reformats() => serde_json::from_str::<Value>(t)
                .map(|v| settings::render_like(&v, orig))
                .unwrap_or_else(|_| t.to_string()),
            Some(t) => t.to_string(),
            None => String::new(),
        };
        crate::diff::unified(
            &before,
            &settings::render_like(&self.after, orig),
            old_name,
            &shown,
        )
    }
}

/// Sets `key` in `obj` to run `bar <key.args>` (other fields of the
/// current value kept); a current value that does not run bar goes into
/// `kept`.
fn set_line(
    obj: &mut serde_json::Map<String, Value>,
    key: &Key,
    command: String,
    kept: &mut serde_json::Map<String, Value>,
) {
    let cur = obj.get(key.name).cloned();
    if let Some(c) = cur.as_ref().filter(|c| !runs_bar(Some(c), key)) {
        kept.insert(key.name.into(), c.clone());
    }
    let mut line = match cur {
        Some(Value::Object(m)) => Value::Object(m),
        _ => json!({}),
    };
    line["type"] = json!("command");
    line["command"] = json!(command);
    obj.insert(key.name.into(), line);
}

/// A binary run through `npx` (or `npm exec`): under a `_npx` directory
/// of npm's cache.
fn in_npx_cache(bar: &Path) -> bool {
    bar.components().any(|c| c.as_os_str() == "_npx")
}

fn plan_install(env: &Env, o: &Opts) -> Result<Plan, String> {
    let l = load(&env.settings)?;
    let shown = l.path.display().to_string();
    let rec = record_path(&env.state_dir, &l.path);
    let mut root = l.root.clone();
    let obj = root.as_object_mut().ok_or("not a JSON object")?;
    let bar = env.bar.display().to_string();
    let mut kept = serde_json::Map::new();
    if !o.subagent_only {
        set_line(obj, &STATUS, render_command(&bar, env.windows), &mut kept);
        if let Some(n) = o.refresh {
            obj[STATUS.name]["refreshInterval"] = json!(n);
        }
    }
    if o.subagent || o.subagent_only {
        set_line(
            obj,
            &SUBAGENT,
            subagent_command(&bar, env.windows),
            &mut kept,
        );
    }
    let mut notes = Vec::new();
    if in_npx_cache(&env.bar) {
        notes.push(format!(
            "warning: {bar} is in the npx cache, which npm may clear; \
             install it with npm install -g @hop-top/bar and run bar install again"
        ));
    }
    for key in [&STATUS, &SUBAGENT] {
        if kept.contains_key(key.name) {
            notes.push(format!(
                "the current {} is kept in {}; bar install --uninstall{} puts it back",
                key.what,
                rec.display(),
                key.flag
            ));
        }
    }
    let record = if kept.is_empty() {
        Record::Keep
    } else {
        let mut all = read_record(&rec);
        all.extend(kept);
        Record::Write(all)
    };
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
    let mut saved = read_record(&rec);
    let mut touched = false;
    let keys = [
        (&STATUS, !o.subagent_only, !o.subagent_only),
        (&SUBAGENT, o.subagent || o.subagent_only, true),
    ];
    for (key, on, remind) in keys {
        let bars = runs_bar(obj.get(key.name), key);
        if !on {
            if bars && remind {
                notes.push(format!(
                    "{} still runs bar; bar install --uninstall{} removes it",
                    key.name, key.flag
                ));
            }
            continue;
        }
        if bars {
            touched = true;
            match saved.remove(key.name) {
                Some(prev) => {
                    obj.insert(key.name.into(), prev);
                    notes.push(format!("restores the {} bar replaced", key.what));
                }
                None => {
                    obj.shift_remove(key.name);
                }
            }
        } else if obj.contains_key(key.name) {
            notes.push(format!("{} does not run bar; left as is", key.name));
        }
    }
    let record = match (touched, saved.is_empty()) {
        (false, _) => Record::Keep,
        (true, true) => Record::Remove,
        (true, false) => Record::Write(saved),
    };
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
