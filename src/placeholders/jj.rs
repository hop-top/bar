//! `jj`: the Jujutsu working-copy commit. Read from one `jj log` call,
//! cached per workspace root and refreshed in the background with
//! `--ignore-working-copy`, so bar never snapshots the working copy and
//! render never runs jj. Render only looks for a `.jj` directory.

use super::{gitst, truncate, work_dir, Ctx, Placeholder};
use crate::cache::{Cache, Job};
use crate::options::Options;
use crate::style::{accent, bold, dim, fg, sanitize};
use std::path::{Path, PathBuf};

/// Parts accepted by `show`, in display order.
pub const SHOW: &[&str] = &[
    "change",
    "commit",
    "bookmarks",
    "workspace",
    "state",
    "desc",
];
/// Parts shown when `show` is unset.
pub const DEFAULT_SHOW: &[&str] = &["change", "bookmarks", "state"];
/// Workspace name jj gives the first workspace; not shown.
const DEFAULT_WORKSPACE: &str = "default";

/// `jj log` template: one field per line, in the order `parse` reads.
/// Ids as shortest unique prefix (at least 8 characters) and the rest,
/// tab-separated; bookmarks on `@`, then on its parents; workspaces whose
/// working copy is `@`; state words; first line of the description.
pub const TEMPLATE: &str = concat!(
    "change_id.shortest(8).prefix() ++ \"\\t\" ++ change_id.shortest(8).rest() ++ \"\\n\"",
    " ++ commit_id.shortest(8).prefix() ++ \"\\t\" ++ commit_id.shortest(8).rest() ++ \"\\n\"",
    " ++ local_bookmarks.map(|b| b.name()).join(\" \") ++ \"\\n\"",
    " ++ parents.map(|p| p.local_bookmarks().map(|b| b.name()).join(\" \")).join(\" \") ++ \"\\n\"",
    " ++ working_copies.map(|w| w.name()).join(\" \") ++ \"\\n\"",
    " ++ separate(\" \", if(empty, \"empty\"), if(conflict, \"conflict\"), if(divergent, \"divergent\")) ++ \"\\n\"",
    " ++ description.first_line() ++ \"\\n\"",
);

/// Arguments of the refresh's `jj` call for the workspace at `dir`.
pub fn args(dir: &str) -> Vec<String> {
    [
        "--ignore-working-copy",
        "--no-pager",
        "--color=never",
        "-R",
        dir,
        "log",
        "-r",
        "@",
        "--no-graph",
        "-T",
        TEMPLATE,
    ]
    .map(String::from)
    .into()
}

pub fn cache_key(root: &Path) -> String {
    format!("jj:{}", root.display())
}

/// Nearest ancestor of `dir` holding a `.jj` directory: the workspace root.
pub fn discover(dir: &Path) -> Option<PathBuf> {
    dir.ancestors()
        .find(|d| d.join(".jj").is_dir())
        .map(Path::to_path_buf)
}

/// The workspace root when it is nearer to `dir` than any git work tree
/// root `git_top` (or there is none): the directory is jj's, not git's.
pub fn nearer_than(dir: &Path, git_top: Option<&Path>) -> Option<PathBuf> {
    let root = discover(dir)?;
    match git_top {
        Some(top) if top.starts_with(&root) => None,
        _ => Some(root),
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Info {
    /// Shortest unique prefix and the rest.
    pub change: (String, String),
    pub commit: (String, String),
    /// Local bookmarks on `@`.
    pub bookmarks: Vec<String>,
    /// Local bookmarks on the parents of `@`.
    pub parent_bookmarks: Vec<String>,
    /// Workspaces whose working-copy commit is `@`.
    pub workspaces: Vec<String>,
    pub empty: bool,
    pub conflict: bool,
    pub divergent: bool,
    /// First line of the description.
    pub desc: String,
}

/// Output of `TEMPLATE`; `None` when it is not that shape.
pub fn parse(text: &str) -> Option<Info> {
    let mut lines = text.split('\n');
    let mut id = || -> Option<(String, String)> {
        let line = lines.next()?;
        let (p, r) = line.split_once('\t')?;
        (!p.is_empty()).then(|| (p.to_string(), r.to_string()))
    };
    let change = id()?;
    let commit = id()?;
    let words = |l: &str| l.split_whitespace().map(String::from).collect::<Vec<_>>();
    let bookmarks = words(lines.next()?);
    let parent_bookmarks = words(lines.next()?);
    let workspaces = words(lines.next()?);
    let state = words(lines.next()?);
    let desc = lines.next()?.to_string();
    let has = |w: &str| state.iter().any(|s| s == w);
    Some(Info {
        change,
        commit,
        bookmarks,
        parent_bookmarks,
        workspaces,
        empty: has("empty"),
        conflict: has("conflict"),
        divergent: has("divergent"),
        desc,
    })
}

impl Info {
    /// Bookmarks on `@`, then those on its parents, each once.
    pub fn all_bookmarks(&self) -> Vec<&str> {
        let mut out: Vec<&str> = Vec::new();
        for b in self.bookmarks.iter().chain(&self.parent_bookmarks) {
            if !out.contains(&b.as_str()) {
                out.push(b);
            }
        }
        out
    }

    /// This workspace's name: the one named after the root directory when
    /// several share `@`, else the first. `None` for `default`.
    pub fn workspace(&self, root: &Path) -> Option<&str> {
        let base = root.file_name().map(|n| n.to_string_lossy());
        let name = self
            .workspaces
            .iter()
            .find(|w| base.as_deref() == Some(w.as_str()))
            .or_else(|| self.workspaces.first())?;
        (name != DEFAULT_WORKSPACE).then_some(name.as_str())
    }
}

/// Styled parts of `info` listed in `show`, in display order.
pub fn parts(ctx: &Ctx, opts: &Options, info: &Info, root: &Path, show: &[&str]) -> Vec<String> {
    let c = &ctx.config.colors;
    let id = |(p, r): &(String, String), lead: &dyn Fn(&str) -> String| {
        format!("{}{}", lead(&sanitize(p)), dim(&sanitize(r)))
    };
    let mut out = Vec::new();
    for part in SHOW.iter().filter(|p| show.contains(p)) {
        let text = match *part {
            "change" => Some(id(&info.change, &|p| bold(&fg(accent(), p)))),
            "commit" => Some(id(&info.commit, &|p| p.to_string())),
            "bookmarks" => {
                let b = sanitize(&info.all_bookmarks().join(" "));
                (!b.is_empty()).then(|| dim(&b))
            }
            "workspace" => info
                .workspace(root)
                .map(sanitize)
                .filter(|w| !w.is_empty())
                .map(|w| dim(&w)),
            "state" => {
                let mut s = Vec::new();
                if info.conflict {
                    s.push(fg(c.bad, "conflict"));
                }
                if info.divergent {
                    s.push(fg(c.warn, "divergent"));
                }
                if info.empty {
                    s.push(dim("empty"));
                }
                (!s.is_empty()).then(|| s.join(" "))
            }
            "desc" => {
                let d = truncate(sanitize(info.desc.trim()), opts);
                (!d.is_empty()).then_some(d)
            }
            _ => None,
        };
        out.extend(text);
    }
    out
}

/// Stand-in for `bar preview`: a change on top of `main` in workspace
/// `bar-feat`; `fresh` is a new, empty change.
pub fn sample(fresh: bool) -> Info {
    let pair = |p: &str, r: &str| (p.to_string(), r.to_string());
    Info {
        change: pair("kp", "qxywon"),
        commit: pair("3f2a", "1b9c"),
        parent_bookmarks: vec!["main".into()],
        workspaces: vec!["bar-feat".into()],
        empty: fresh,
        desc: if fresh { "" } else { "feat: jj status" }.into(),
        ..Info::default()
    }
}

fn ttl(opts: &Options) -> u64 {
    opts.get("ttl")
        .and_then(|t| t.trim().parse::<u64>().ok())
        .map_or(gitst::TTL, |t| t.max(1))
}

pub struct Jj;

impl Placeholder for Jj {
    fn render(&self, ctx: &Ctx, opts: &Options) -> Option<String> {
        let show = match opts.get("show") {
            Some(v) => gitst::parse_show(v, SHOW)?,
            None => DEFAULT_SHOW.to_vec(),
        };
        let cache: &Cache = ctx.cache?;
        let root = discover(&work_dir(ctx))?;
        let root = root.canonicalize().unwrap_or(root);
        let job = Job::Jj {
            dir: root.to_str()?.to_string(),
        };
        let raw = cache.read_or_refresh(&cache_key(&root), ttl(opts), &job)?;
        let info = parse(&raw)?;
        let parts = parts(ctx, opts, &info, &root, &show);
        (!parts.is_empty()).then(|| parts.join(" "))
    }
}
