//! `update`: `↑ <version>` when a newer bar release is out. Off unless
//! `set update-check on`; the check is a daily background job.

use super::{Ctx, Placeholder};
use crate::cache::Job;
use crate::options::Options;
use crate::style::fg;
use crate::update;

pub struct Update;

/// `↑ 0.2.0` in the warn colour.
pub fn fmt(ctx: &Ctx, version: &str) -> String {
    fg(ctx.config.colors.warn, &format!("↑ {version}"))
}

impl Placeholder for Update {
    fn render(&self, ctx: &Ctx, _: &Options) -> Option<String> {
        if !ctx.config.update_check {
            return None;
        }
        let cache = ctx.cache?;
        let prerelease = update::channel_is_prerelease(update::current());
        let job = Job::Update { prerelease };
        let tag = cache.read_or_refresh(update::cache_key(prerelease), update::TTL, &job)?;
        let v = update::newer(update::current(), tag.trim())?;
        Some(fmt(ctx, &v))
    }
}
