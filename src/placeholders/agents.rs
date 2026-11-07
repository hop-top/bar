use super::{truncate, Ctx, Placeholder};
use crate::click::{self, hook};
use crate::options::Options;
use crate::style::dim;
use std::path::Path;
use std::time::SystemTime;

/// Entries older than this are ignored: a subagent that died without a
/// stop event must not stay counted forever.
pub const STALE_SECS: i64 = 6 * 3600;
/// Live agents counted per render, and directory entries scanned.
const MAX_ENTRIES: usize = 64;
const MAX_SCAN: usize = 256;

/// `agents 2`: subagents running in this session, from the registry
/// `bar click hook` keeps. Hidden at 0. Option `names=1` appends the types.
pub struct Agents;

impl Placeholder for Agents {
    fn render(&self, ctx: &Ctx, opts: &Options) -> Option<String> {
        render_with(ctx, opts, &click::state_dir())
    }
}

pub fn render_with(ctx: &Ctx, opts: &Options, state: &Path) -> Option<String> {
    let dir = hook::agents_dir(state, &ctx.session.session_id).filter(|d| hook::is_real_dir(d))?;
    let mut kinds: Vec<String> = Vec::new();
    // Invalid, stale and temp entries are skipped before the cap, so junk
    // cannot hide live agents; the scan itself is bounded.
    for e in std::fs::read_dir(dir).ok()?.flatten().take(MAX_SCAN) {
        if kinds.len() >= MAX_ENTRIES {
            break;
        }
        let name = e.file_name();
        if !name.to_str().is_some_and(hook::valid_agent_id) {
            continue;
        }
        let Ok(meta) = e.metadata() else { continue };
        let mtime = meta
            .modified()
            .ok()
            .and_then(|m| m.duration_since(SystemTime::UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_secs() as i64);
        if !meta.is_file() || ctx.now - mtime >= STALE_SECS {
            continue;
        }
        let kind = std::fs::read_to_string(e.path()).unwrap_or_default();
        kinds.push(kind.trim().to_owned());
    }
    if kinds.is_empty() {
        return None;
    }
    let head = format!("{} {}", dim("agents"), kinds.len());
    if opts.get("names") != Some("1") {
        return Some(head);
    }
    kinds.sort();
    let list: Vec<String> = kinds.into_iter().filter(|k| !k.is_empty()).collect();
    if list.is_empty() {
        return Some(head);
    }
    Some(format!("{head} {}", truncate(list.join(" "), opts)))
}
