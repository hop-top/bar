//! `bar check` results and how they print.

use crate::conf::{Layer, ProjectMode};
use crate::prices::models;
use serde_json::{json, Value};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Error,
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    pub level: Level,
    /// 1-based; 0 when the problem has no line (I/O, built-in layout).
    pub line: usize,
    pub message: String,
    /// The file the line is in.
    pub layer: Layer,
}

/// The project config found for the working directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectInfo {
    pub path: PathBuf,
    /// The user config's `project-config`; `off` means it was not applied.
    pub mode: ProjectMode,
}

#[derive(Debug)]
pub struct Report {
    /// The config file in use; `None` means the built-in layout.
    pub path: Option<PathBuf>,
    pub project: Option<ProjectInfo>,
    pub issues: Vec<Issue>,
    /// Warnings about the setup rather than a config line (the click
    /// handler's dirs).
    pub notes: Vec<String>,
}

impl Report {
    pub fn has_errors(&self) -> bool {
        self.issues.iter().any(|i| i.level == Level::Error)
    }

    /// The file `layer` stands for, `None` for the built-in layout.
    fn file(&self, layer: Layer) -> Option<&PathBuf> {
        match layer {
            Layer::User => self.path.as_ref(),
            Layer::Project => self.project.as_ref().map(|p| &p.path),
        }
    }

    pub fn to_json(&self) -> Value {
        let issues: Vec<Value> = self
            .issues
            .iter()
            .map(|i| {
                json!({
                    "level": level_name(i.level),
                    "file": self.file(i.layer).map(|p| p.display().to_string()),
                    "line": i.line,
                    "message": i.message,
                })
            })
            .chain(
                self.notes
                    .iter()
                    .map(|n| json!({"level": "warning", "file": null, "line": 0, "message": n})),
            )
            .collect();
        let a = models::active();
        json!({
            "path": self.path.as_ref().map(|p| p.display().to_string()),
            "project": self.project.as_ref().map(|p| json!({
                "path": p.path.display().to_string(),
                "mode": p.mode.name(),
                "applied": p.mode != ProjectMode::Off,
            })),
            "prices": {
                "path": models::user_path().display().to_string(),
                "fetched": a.user_file.as_ref().map(|_| a.snapshot.fetched()),
                "ignored": a.ignored,
            },
            "ok": !self.has_errors(),
            "issues": issues,
        })
    }

    pub fn to_text(&self) -> String {
        let mut out = match &self.path {
            Some(p) => format!("config: {}\n", p.display()),
            None => "config: none found, using the built-in layout\n".to_string(),
        };
        if let Some(p) = &self.project {
            out.push_str(&match p.mode {
                ProjectMode::Off => format!(
                    "project: {} (not applied: project-config off)\n",
                    p.path.display()
                ),
                m => format!("project: {} ({})\n", p.path.display(), m.name()),
            });
        }
        let a = models::active();
        let path = models::user_path();
        out.push_str(&match (&a.user_file, &a.ignored) {
            (Some(_), _) => format!(
                "prices: models.dev snapshot {} ({})\n",
                a.snapshot.fetched(),
                path.display()
            ),
            (None, Some(why)) => format!("prices: {} ignored, {why}\n", path.display()),
            (None, None) => format!("prices: models.dev not fetched ({})\n", path.display()),
        });
        for i in &self.issues {
            out.push_str(&format!(
                "{}{}: {}\n",
                self.at(i),
                level_name(i.level),
                i.message
            ));
        }
        for n in &self.notes {
            out.push_str(&format!("warning: {n}\n"));
        }
        let count = |l| self.issues.iter().filter(|i| i.level == l).count();
        let (e, w) = (
            count(Level::Error),
            count(Level::Warning) + self.notes.len(),
        );
        out.push_str(&if e + w == 0 {
            "ok\n".to_string()
        } else {
            format!("{e} error(s), {w} warning(s)\n")
        });
        out
    }

    /// `line 3: `; with a project config in play, `config line 3: ` or
    /// `project line 3: ` (`project: ` without a line).
    fn at(&self, i: &Issue) -> String {
        let label = match (&self.project, i.layer) {
            (None, _) => None,
            (Some(_), Layer::User) => Some("config"),
            (Some(_), Layer::Project) => Some("project"),
        };
        match (label, i.line) {
            (None, 0) => String::new(),
            (None, n) => format!("line {n}: "),
            (Some(l), 0) => format!("{l}: "),
            (Some(l), n) => format!("{l} line {n}: "),
        }
    }
}

fn level_name(l: Level) -> &'static str {
    match l {
        Level::Error => "error",
        Level::Warning => "warning",
    }
}
