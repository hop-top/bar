//! `modes`: `voice`, `sandbox` and `remote` flags, each shown only while
//! on. The statusline JSON carries none of them, so they come from the
//! Claude Code settings files: the user's `settings.json`, then the
//! project's `.claude/settings.json` and `.claude/settings.local.json`,
//! a later file overriding an earlier one.

use super::{work_dir, Ctx, Placeholder};
use crate::click;
use crate::options::Options;
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

fn read(path: &Path) -> Option<Value> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

/// Per flag, the last file that sets it decides.
fn states(files: &[PathBuf]) -> [bool; 3] {
    let mut out = [false; 3];
    for v in files.iter().filter_map(|f| read(f)) {
        for (i, flag) in out.iter_mut().enumerate() {
            if let Some(on) = setting(&v, i) {
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

/// Flags on in the settings files under `claude` and `project`.
pub fn render_with(claude: Option<&Path>, project: &Path, opts: &Options) -> Option<String> {
    let mut files = Vec::new();
    if let Some(d) = claude {
        files.push(d.join("settings.json"));
    }
    files.push(project.join(".claude/settings.json"));
    files.push(project.join(".claude/settings.local.json"));
    let f = flags(states(&files), opts);
    (!f.is_empty()).then(|| dim(&f.join(" ")))
}

impl Placeholder for Modes {
    fn render(&self, ctx: &Ctx, opts: &Options) -> Option<String> {
        render_with(click::claude_dir().as_deref(), &work_dir(ctx), opts)
    }
}

/// Stand-in flags for `bar preview`: all on.
pub fn sample(opts: &Options) -> Vec<&'static str> {
    flags([true; 3], opts)
}
