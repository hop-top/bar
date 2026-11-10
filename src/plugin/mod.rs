//! External placeholders: `bar-<name>` executables on `PATH`, interrogated
//! with `--ext-info` (kit ext-discover, extended) and rendered with JSON on
//! stdin. Both runs go through the cache; render never waits on a plugin.

mod alias;
pub mod check;
mod discover;
mod info;
mod render;
pub mod web;

use std::time::Duration;

pub use alias::{resolve, valid_alias, Resolved, MAX_ALIASES};
pub use discover::{discover, find, path_dirs, plugin_dirs, plugin_dirs_with, Found};
pub use info::{
    cached_info, env_secret, info, info_job, info_now, parse_info, validate, ActionSpec, ExtInfo,
    Inputs, OptSpec, OptType, MAX_ENV_INPUTS, MAX_ENV_VALUE, MAX_TOKENS, RESERVED, SESSION_INPUTS,
};
pub use render::{
    cached_output, default_output, link_account, paint, paint_with, parse_output, plan, render,
    render_found, render_found_with, Account, Metric, Output, Plan, Usage, MAX_ITEMS,
};

pub const PREFIX: &str = "bar-";
/// Capability a plugin must declare to be rendered.
pub const CAPABILITY: &str = "placeholder";
pub const INFO_TIMEOUT: Duration = Duration::from_secs(5);
pub const RENDER_TIMEOUT: Duration = Duration::from_secs(3);
/// Longest render limit a plugin can ask for in `--ext-info`; below the
/// refresh budget of a detached job.
pub const MAX_RENDER_TIMEOUT: Duration = Duration::from_secs(15);
/// Render results stay fresh this long unless the plugin says otherwise.
pub const DEFAULT_TTL: u64 = 60;
/// `--ext-info` results stay fresh a day; the cache key carries the
/// binary's mtime, so an upgrade asks again at once.
pub const INFO_TTL: u64 = 86_400;
