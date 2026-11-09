use super::abbrev::Abbrev;
use super::{work_dir, Ctx, Placeholder};
use crate::gitfs::{self, Found};
use crate::options::Options;
use crate::style::{dim, sanitize};
use std::path::Path;

/// Session directory, home shown as `~`; `base=1` for the last component;
/// `open` links it to the file manager or an editor.
pub struct Cwd;
/// Git top-level directory of the session directory (a Jujutsu workspace
/// root when that is nearer), no process spawned; hidden outside a
/// repository.
pub struct Root;

/// Path separators: `/`, and `\` on Windows.
fn is_sep(c: char) -> bool {
    c == '/' || (cfg!(windows) && c == '\\')
}

/// `path` with a leading `home` replaced by `~` (whole components only;
/// ASCII case ignored on Windows, as its file names do).
pub fn abbrev_home(path: &str, home: &str) -> String {
    let home = home.trim_end_matches(is_sep);
    if home.is_empty() {
        return path.to_owned();
    }
    let rest = match path.get(..home.len()) {
        Some(head) if head == home || (cfg!(windows) && head.eq_ignore_ascii_case(home)) => {
            &path[home.len()..]
        }
        _ => return path.to_owned(),
    };
    match rest.chars().next() {
        None => "~".into(),
        Some(c) if is_sep(c) => format!("~{rest}"),
        _ => path.to_owned(),
    }
}

fn base_on(opts: &Options) -> bool {
    opts.get("base") == Some("1")
}

/// `path` as `cwd` / `root` show it, given the home directory: `base=1`
/// the last component only, else `depth` and `fish` applied.
pub fn shown(path: &str, opts: &Options, home: &str) -> Option<String> {
    let full = abbrev_home(path, home);
    let out = if base_on(opts) {
        match full.as_str() {
            "~" | "/" => full,
            _ => full
                .trim_end_matches(is_sep)
                .rsplit(is_sep)
                .next()
                .unwrap_or_default()
                .to_owned(),
        }
    } else {
        Abbrev::from(opts).apply(&full, is_sep)
    };
    let out = sanitize(&out);
    (!out.is_empty()).then(|| dim(&out))
}

fn home() -> String {
    crate::platform::home()
        .and_then(|h| h.into_os_string().into_string().ok())
        .unwrap_or_default()
}

impl Placeholder for Cwd {
    fn render(&self, ctx: &Ctx, opts: &Options) -> Option<String> {
        let text = shown(&ctx.session.cwd, opts, &home())?;
        Some(super::open::wrap(text, &ctx.session.cwd, opts))
    }
}

impl Placeholder for Root {
    fn render(&self, ctx: &Ctx, opts: &Options) -> Option<String> {
        let dir = work_dir(ctx);
        let top = match gitfs::discover(&dir) {
            Found::Repo(r) => Some(r.top),
            _ => None,
        };
        let root = super::jj::nearer_than(&dir, top.as_deref()).or(top)?;
        let root = path_str(&root)?;
        let text = shown(&root, opts, &home())?;
        Some(super::open::wrap(text, &root, opts))
    }
}

fn path_str(p: &Path) -> Option<String> {
    p.to_str().map(str::to_owned)
}
