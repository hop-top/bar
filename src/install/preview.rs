//! `bar install --dry-run`: the change [`super::run_outcome`] would make,
//! worked out the same way, with nothing written.

use super::{plan_install, plan_uninstall, record_path, Env, Opts, Record};
use std::path::PathBuf;

/// What [`super::run_outcome`] would do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preview {
    /// The settings file.
    pub path: PathBuf,
    /// The files it would write or remove, settings first:
    /// `(action, path, backed up)`, action `create`, `update` or
    /// `remove`. Empty when the settings are up to date.
    pub changes: Vec<(&'static str, PathBuf, bool)>,
    /// The diff of the settings file; empty without changes.
    pub diff: String,
    /// What the run would print besides the diff.
    pub notes: Vec<String>,
}

/// Plans the install (or with `o.uninstall` the uninstall) of `env`.
pub fn preview(env: &Env, o: &Opts) -> Result<Preview, String> {
    let plan = if o.uninstall {
        plan_uninstall(env, o)?
    } else {
        plan_install(env, o)?
    };
    let mut notes = plan.notes.clone();
    if !plan.changed() {
        return Ok(Preview {
            path: plan.path,
            changes: Vec::new(),
            diff: String::new(),
            notes,
        });
    }
    if plan.reformats() {
        notes.push(format!(
            "{} is also reformatted (indent kept); the diff shows its content",
            plan.path.display()
        ));
    }
    let exists = plan.text.is_some();
    let mut changes = vec![(
        if exists { "update" } else { "create" },
        plan.path.clone(),
        exists,
    )];
    let rec = record_path(&env.state_dir, &plan.path);
    match &plan.record {
        Record::Write(_) => {
            let action = if rec.exists() { "update" } else { "create" };
            changes.push((action, rec, false));
        }
        Record::Remove if rec.exists() => changes.push(("remove", rec, false)),
        _ => {}
    }
    Ok(Preview {
        diff: plan.diff(),
        path: plan.path,
        changes,
        notes,
    })
}
