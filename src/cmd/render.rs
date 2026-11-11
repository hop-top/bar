//! The status line paths Claude Code runs on every update: `bar render`,
//! `bar subagent` and the background `bar refresh <json>`.

use std::io::Write;

use hop_bar::cache::{refresh_timeout, Cache, CachedRunner, Request};
use hop_bar::{click, conf, session};

use super::cwd;

pub fn render() {
    let mut buf = Vec::new();
    // Non-UTF-8 or unreadable stdin degrades to an empty session.
    let _ = std::io::Read::read_to_end(&mut std::io::stdin(), &mut buf);
    let s = session::parse(&String::from_utf8_lossy(&buf));
    let columns = std::env::var("COLUMNS")
        .ok()
        .and_then(|c| c.trim().parse().ok());
    let cache = Cache::new(&click::state_dir());
    let runner = CachedRunner::new(cache.clone());
    // the session's directory picks the project config; Claude Code runs
    // the status line command there too
    let dir = Some(std::path::PathBuf::from(&s.cwd))
        .filter(|d| d.is_absolute())
        .or_else(cwd);
    let load = conf::load(dir.as_deref());
    let out = hop_bar::render::run(&s, load, columns, &runner, Some(&cache));
    if !out.is_empty() {
        // a closed pipe must not panic; render always exits 0
        let mut so = std::io::stdout().lock();
        let _ = writeln!(so, "{out}");
        let _ = so.flush();
    }
    // after the output: publish the state dir for the bar:// handler and the tray
    if let Some(d) = click::pointer::default_dir() {
        click::pointer::sync(cache.state_dir(), &d, std::time::SystemTime::now());
    }
}

/// Largest `subagentStatusLine` input read.
const MAX_SUBAGENT_INPUT: u64 = 4 * 1024 * 1024;

pub fn subagent() {
    use std::io::Read as _;
    let mut buf = Vec::new();
    let _ = std::io::stdin()
        .take(MAX_SUBAGENT_INPUT)
        .read_to_end(&mut buf);
    let cache = Cache::new(&click::state_dir());
    let runner = CachedRunner::new(cache.clone());
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    // the payload's directory picks the project config, as for render
    let load = |p: &hop_bar::subagent::Payload| {
        let dir = Some(std::path::PathBuf::from(&p.cwd))
            .filter(|d| d.is_absolute())
            .or_else(cwd);
        conf::load(dir.as_deref())
    };
    let input = String::from_utf8_lossy(&buf);
    let out = hop_bar::subagent::run(&input, load, now, &runner, Some(&cache));
    if !out.is_empty() {
        let mut so = std::io::stdout().lock();
        let _ = so.write_all(out.as_bytes());
        let _ = so.flush();
    }
}

pub fn refresh(raw: &str) {
    if let Ok(req) = Request::from_json(raw) {
        let limit = refresh_timeout(std::env::var("BAR_REFRESH_TIMEOUT").ok().as_deref());
        Cache::new(&click::state_dir()).refresh(&req, limit);
    }
}
