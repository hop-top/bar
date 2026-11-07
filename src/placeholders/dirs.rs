use super::{work_dir, Ctx, Placeholder};
use crate::gitfs::{self, Found};
use crate::options::Options;
use crate::style::{dim, sanitize};
use std::path::Path;

/// Session directory, home shown as `~`; `base=1` for the last component.
pub struct Cwd;
/// Git top-level directory of the session directory, no process spawned;
/// hidden outside a repository.
pub struct Root;

/// `path` with a leading `home` replaced by `~` (whole components only).
pub fn abbrev_home(path: &str, home: &str) -> String {
    let home = home.trim_end_matches('/');
    if home.is_empty() {
        return path.to_owned();
    }
    match path.strip_prefix(home) {
        Some("") => "~".into(),
        Some(rest) if rest.starts_with('/') => format!("~{rest}"),
        _ => path.to_owned(),
    }
}

fn base_on(opts: &Options) -> bool {
    opts.get("base") == Some("1")
}

/// `path` as `cwd` / `root` show it, given the home directory.
pub fn shown(path: &str, opts: &Options, home: &str) -> Option<String> {
    let full = abbrev_home(path, home);
    let out = if base_on(opts) {
        match full.as_str() {
            "~" | "/" => full,
            _ => full
                .trim_end_matches('/')
                .rsplit('/')
                .next()
                .unwrap_or_default()
                .to_owned(),
        }
    } else {
        full
    };
    let out = sanitize(&out);
    (!out.is_empty()).then(|| dim(&out))
}

fn home() -> String {
    std::env::var("HOME").unwrap_or_default()
}

impl Placeholder for Cwd {
    fn render(&self, ctx: &Ctx, opts: &Options) -> Option<String> {
        shown(&ctx.session.cwd, opts, &home())
    }
}

impl Placeholder for Root {
    fn render(&self, ctx: &Ctx, opts: &Options) -> Option<String> {
        let Found::Repo(r) = gitfs::discover(&work_dir(ctx)) else {
            return None;
        };
        shown(&path_str(&r.top)?, opts, &home())
    }
}

fn path_str(p: &Path) -> Option<String> {
    p.to_str().map(str::to_owned)
}
