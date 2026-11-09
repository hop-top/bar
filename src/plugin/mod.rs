//! External placeholders: `bar-<name>` executables on `PATH`, interrogated
//! with `--ext-info` (kit ext-discover, extended) and rendered with JSON on
//! stdin. Both runs go through the cache; render never waits on a plugin.

mod discover;
mod info;
mod render;
pub mod web;

use std::time::Duration;

pub use discover::{discover, find, path_dirs, Found};
pub use info::{
    cached_info, info, info_job, info_now, parse_info, validate, ExtInfo, OptSpec, OptType,
    RESERVED,
};
pub use render::{
    paint, paint_with, parse_output, plan, render, render_found, render_found_with, Metric, Output,
    Plan,
};

pub const PREFIX: &str = "bar-";
/// Capability a plugin must declare to be rendered.
pub const CAPABILITY: &str = "placeholder";
pub const INFO_TIMEOUT: Duration = Duration::from_secs(5);
pub const RENDER_TIMEOUT: Duration = Duration::from_secs(3);
/// Render results stay fresh this long unless the plugin says otherwise.
pub const DEFAULT_TTL: u64 = 60;
/// `--ext-info` results stay fresh a day; the cache key carries the
/// binary's mtime, so an upgrade asks again at once.
pub const INFO_TTL: u64 = 86_400;
