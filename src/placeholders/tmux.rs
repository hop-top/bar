//! `tmux`: `session:window` of the pane bar runs in. Names come from a
//! background `tmux display-message`; render only reads the cache.

use super::{truncate, Ctx, Placeholder};
use crate::cache::Job;
use crate::options::Options;
use crate::style::{dim, sanitize};

/// Names change on rename; look again this often, seconds.
const TTL: u64 = 30;

pub struct Tmux;

/// `%12`: what tmux puts in `$TMUX_PANE`. Anything else never reaches argv.
pub fn valid_pane(p: &str) -> bool {
    p.strip_prefix('%')
        .is_some_and(|d| (1..=10).contains(&d.len()) && d.bytes().all(|b| b.is_ascii_digit()))
}

/// Cache key: one entry per tmux server and pane.
pub fn cache_key(tmux: &str, pane: &str) -> String {
    format!("tmux:{tmux}\0{pane}")
}

impl Placeholder for Tmux {
    fn render(&self, ctx: &Ctx, opts: &Options) -> Option<String> {
        let tmux = std::env::var("TMUX").ok().filter(|t| !t.is_empty())?;
        let pane = std::env::var("TMUX_PANE").ok().filter(|p| valid_pane(p))?;
        let job = Job::Tmux { pane: pane.clone() };
        let stored = ctx
            .cache?
            .read_or_refresh(&cache_key(&tmux, &pane), TTL, &job)?;
        let name = sanitize(stored.lines().next().unwrap_or_default().trim());
        (!name.is_empty()).then(|| dim(&truncate(name, opts)))
    }
}
