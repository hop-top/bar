//! `bar check`: lints the config file without rendering anything.

use crate::budget;
use crate::conf::{layers, project, Layer};
use crate::options::Options;
use crate::render::builtin_config;
use std::cell::Cell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

mod lines;
mod options;
mod placeholders;
mod report;
pub mod state;

pub use options::{core_options, option_help, option_schema, Val};
pub use report::{Issue, Level, ProjectInfo, Report};

struct Checker<'a> {
    dirs: &'a [PathBuf],
    icon_width: usize,
    /// `set max-rows` of the config as applied.
    max_rows: usize,
    defaults: BTreeMap<String, Options>,
    gh_missing: bool,
    gh_warned: Cell<bool>,
    budgets: budget::Budgets,
    /// `set usage-api on`.
    usage_api: bool,
    /// `set account-source`.
    account_source: Option<String>,
    /// Plugins whose `--check` already ran.
    checked: std::collections::BTreeSet<String>,
    /// `glab` is not on PATH while a GitLab remote is in play.
    glab_missing: bool,
    jj_missing: bool,
    issues: Vec<Issue>,
    /// File of the lines being linted.
    layer: Layer,
    /// Linting the `subagent` layout.
    row: bool,
}

impl Checker<'_> {
    fn add(&mut self, level: Level, line: usize, message: String) {
        self.issues.push(Issue {
            level,
            line,
            message,
            layer: self.layer,
        });
    }

    fn error(&mut self, line: usize, message: String) {
        self.add(Level::Error, line, message);
    }
}

/// A config file and its contents, or why it could not be read.
pub type Source = (PathBuf, Result<String, String>);

/// Reads `path` for `check_layers`.
pub fn source(path: &Path) -> Source {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()));
    (path.to_path_buf(), text)
}

/// Lints the user config (`None`: the built-in layout) and the project
/// config over it, as `bar render` would apply them. An unreadable file
/// is one error without a line.
pub fn check_layers(user: Option<Source>, project: Option<Source>, dirs: &[PathBuf]) -> Report {
    let path = user.as_ref().map(|(p, _)| p.clone());
    let mut c = Checker {
        dirs,
        icon_width: 1,
        max_rows: crate::conf::DEFAULT_MAX_ROWS,
        defaults: BTreeMap::new(),
        gh_missing: !dirs
            .iter()
            .any(|d| crate::platform::executable_in(d, "gh").is_some()),
        gh_warned: Cell::new(false),
        budgets: Default::default(),
        usage_api: false,
        account_source: None,
        checked: Default::default(),
        glab_missing: false,
        jj_missing: !dirs
            .iter()
            .any(|d| crate::platform::executable_in(d, "jj").is_some()),
        issues: Vec::new(),
        layer: Layer::User,
        row: false,
    };
    let user_text = match user.map(|(_, t)| t) {
        Some(Err(e)) => {
            c.error(0, e);
            return Report {
                path,
                project: None,
                issues: c.issues,
                notes: Vec::new(),
            };
        }
        Some(Ok(t)) => Some(t),
        None => None,
    };
    let mut applied = None;
    let mut mode = project::ProjectMode::Off;
    let parsed = layers(user_text.as_deref(), |m| {
        mode = m;
        let text = project.as_ref()?.1.clone();
        applied = text.as_ref().ok().cloned();
        Some(text)
    });
    if let Ok(cfg) = &parsed.config {
        c.icon_width = cfg.icon_width;
        c.max_rows = cfg.max_rows;
        c.defaults = cfg.defaults.clone();
        c.budgets = cfg.budgets;
        c.usage_api = cfg.usage_api;
        c.account_source = cfg.account_source.clone();
        for (k, v) in [
            ("usage-source", &cfg.usage_source),
            ("account-source", &cfg.account_source),
        ] {
            if let Some(n) = v {
                c.source(k, n);
            }
        }
        let gitlab = !cfg.gitlab_hosts.is_empty() || cwd_on_gitlab(&cfg.gitlab_hosts);
        c.glab_missing = gitlab
            && !dirs
                .iter()
                .any(|d| crate::platform::executable_in(d, "glab").is_some());
    }
    for e in parsed.config.err().unwrap_or_default() {
        c.layer = e.layer;
        c.error(e.error.line, e.error.message);
    }
    // dropped project lines do not apply: warnings, not linted further
    c.layer = Layer::Project;
    for e in &parsed.dropped {
        let why = if e.line == 0 {
            format!("{}; the project config is ignored", e.message)
        } else {
            format!("{}; the line is ignored", e.message)
        };
        c.add(Level::Warning, e.line, why);
    }
    let applied = applied.map(|text| {
        let skip: Vec<usize> = parsed.dropped.iter().map(|e| e.line).collect();
        text.lines()
            .enumerate()
            .map(|(i, l)| if skip.contains(&(i + 1)) { "" } else { l })
            .collect::<Vec<_>>()
            .join("\n")
    });
    c.layer = Layer::User;
    match &user_text {
        Some(text) => c.lint(text),
        None => {
            let cfg = builtin_config();
            for src in [&cfg.left, &cfg.center, &cfg.right] {
                c.layout(src, 0);
            }
        }
    }
    if let Some(text) = &applied {
        c.layer = Layer::Project;
        c.lint(text);
    }
    c.issues
        .sort_by_key(|i| (i.layer == Layer::Project, i.line));
    Report {
        path,
        project: project.map(|(path, _)| ProjectInfo { path, mode }),
        issues: c.issues,
        notes: Vec::new(),
    }
}

/// The working directory's `origin`, read from the repository files only,
/// is on gitlab.com or one of `hosts`.
fn cwd_on_gitlab(hosts: &[String]) -> bool {
    let Ok(dir) = std::env::current_dir() else {
        return false;
    };
    let crate::gitfs::Found::Repo(repo) = crate::gitfs::discover(&dir) else {
        return false;
    };
    repo.origin_url()
        .ok()
        .flatten()
        .and_then(|u| crate::placeholders::repo::web_url(&u))
        .is_some_and(|u| crate::placeholders::glab::gitlab_repo(&u, hosts).is_some())
}

/// Lints `text`, a user config file's contents, on its own.
pub fn check_text(text: &str, path: Option<PathBuf>, dirs: &[PathBuf]) -> Report {
    let p = path.unwrap_or_default();
    let mut r = check_layers(Some((p, Ok(text.to_string()))), None, dirs);
    r.path = r.path.filter(|p| !p.as_os_str().is_empty());
    r
}

/// Lints the built-in layout, used when no config file exists.
pub fn check_builtin(dirs: &[PathBuf]) -> Report {
    check_layers(None, None, dirs)
}

/// Lints the file at `path`; an unreadable file is one error without a line.
pub fn check_file(path: &Path, dirs: &[PathBuf]) -> Report {
    check_layers(Some(source(path)), None, dirs)
}
