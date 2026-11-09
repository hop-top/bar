//! `modes`: `voice`, `sandbox` and `remote` flags, each shown only while
//! on. The statusline JSON carries none of them, so they come from the
//! Claude Code settings files: the user's `settings.json`, then the
//! project's `.claude/settings.json` and `.claude/settings.local.json`,
//! a later file overriding an earlier one. The project is the launch
//! directory (`workspace.project_dir`), where Claude Code reads them,
//! else the working directory.

use super::{work_dir, Ctx, Placeholder};
use crate::options::Options;
use crate::settings;
use crate::style::dim;
use serde_json::Value;
use std::path::{Path, PathBuf};

pub const SHOW: &[&str] = &["voice", "sandbox", "remote"];

/// `voice`, `sandbox`, `remote` while on; hidden when none is.
pub struct Modes;

fn bool_at(v: &Value, path: &[&str]) -> Option<bool> {
    path.iter().try_fold(v, |v, k| v.get(k))?.as_bool()
}

/// The setting behind `SHOW[i]` in one settings value, when it is set.
/// `voice.enabled` wins over the older `voiceEnabled`.
fn setting(v: &Value, i: usize) -> Option<bool> {
    match i {
        0 => bool_at(v, &["voice", "enabled"]).or_else(|| bool_at(v, &["voiceEnabled"])),
        1 => bool_at(v, &["sandbox", "enabled"]),
        _ => bool_at(v, &["remoteControlAtStartup"]),
    }
}

/// Per flag, the last file that sets it decides.
fn states(files: &[Option<&Value>]) -> [bool; 3] {
    let mut out = [false; 3];
    for v in files.iter().flatten() {
        for (i, flag) in out.iter_mut().enumerate() {
            if let Some(on) = setting(v, i) {
                *flag = on;
            }
        }
    }
    out
}

/// The flags in `show` (all when unset) that are on, in `SHOW` order.
fn flags(on: [bool; 3], opts: &Options) -> Vec<&'static str> {
    let want: Option<Vec<&str>> = opts
        .get("show")
        .map(|s| s.split(',').map(str::trim).collect());
    SHOW.iter()
        .zip(on)
        .filter(|(n, on)| *on && want.as_ref().is_none_or(|w| w.contains(n)))
        .map(|(n, _)| *n)
        .collect()
}

/// Flags on in the `user` settings and the settings files of `project`.
pub fn render_with(user: Option<&Value>, project: &Path, opts: &Options) -> Option<String> {
    let shared = settings::read(&project.join(".claude/settings.json"));
    let local = settings::read(&project.join(".claude/settings.local.json"));
    let f = flags(states(&[user, shared.as_ref(), local.as_ref()]), opts);
    (!f.is_empty()).then(|| dim(&f.join(" ")))
}

impl Placeholder for Modes {
    fn render(&self, ctx: &Ctx, opts: &Options) -> Option<String> {
        let launch = PathBuf::from(&ctx.session.project_dir);
        let project = if !ctx.session.project_dir.is_empty() && launch.is_dir() {
            launch
        } else {
            work_dir(ctx)
        };
        render_with(settings::user(), &project, opts)
    }
}

/// Stand-in flags for `bar preview`: all on.
pub fn sample(opts: &Options) -> Vec<&'static str> {
    flags([true; 3], opts)
}
