use hop_bar::cache::Cache;
use hop_bar::conf::Config;
use hop_bar::options::Options;
use hop_bar::placeholders::{lookup, Ctx};
use hop_bar::render::ColdRunner;
use hop_bar::session::Session;
use hop_bar::transcript::LOADS;
use std::sync::atomic::Ordering;

#[test]
fn three_placeholders_share_one_load() {
    let dir = std::env::temp_dir().join(format!("bar-memo-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("t.jsonl");
    std::fs::write(
        &file,
        "{\"timestamp\":\"2026-01-02T03:04:05.000Z\",\"message\":{\"id\":\"a\",\"usage\":{\"input_tokens\":5,\"output_tokens\":6,\"cache_read_input_tokens\":7}}}\n",
    )
    .unwrap();
    let cache = Cache::with_exe(&dir.join("state"), "/nonexistent/bar".into());
    let cfg = Config::default();
    let runner = ColdRunner;
    let s = Session {
        transcript_path: file.to_string_lossy().into_owned(),
        ..Session::default()
    };
    let ctx = Ctx {
        session: &s,
        config: &cfg,
        now: 0,
        runner: &runner,
        cache: Some(&cache),
        columns: None,
        plugin_dirs: &[],
        memo: Default::default(),
    };
    let before = LOADS.load(Ordering::Relaxed);
    for n in ["tokens", "cache", "cachettl"] {
        let out = lookup(n).unwrap().render(&ctx, &Options::default());
        assert!(out.is_some(), "{n}");
    }
    assert_eq!(LOADS.load(Ordering::Relaxed) - before, 1);
    let _ = std::fs::remove_dir_all(&dir);
}
