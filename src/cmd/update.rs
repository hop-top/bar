use clap::ArgMatches;
use hop_bar::cache::Cache;
use hop_bar::{check, click, conf};

use super::cwd;
use crate::cli_out;

pub fn run(m: &ArgMatches) {
    use hop_bar::{cache::Job, update};
    cli_out::check_format(m, "update");
    let on = matches!(conf::load(cwd().as_deref()), conf::LoadResult::Ok(c) if c.update_check);
    let cache = Cache::new(&click::state_dir());
    let pre = update::channel_is_prerelease(update::current());
    // a stale result also starts the daily check in the background
    let cached = on.then(|| {
        cache.read_or_refresh(
            update::cache_key(pre),
            update::TTL,
            &Job::Update { prerelease: pre },
        )
    });
    let exe = std::env::current_exe().unwrap_or_else(|_| "bar".into());
    let text = update::report(
        update::current(),
        cached.as_ref().map(|c| c.as_deref()),
        &exe,
    );
    let data = check::state::update_state(&cache, update::current(), on, &exe);
    cli_out::emit(m, "update", &text, &data);
}
