#![allow(dead_code, unused_imports)]
use hop_bar::cache::{Cache, Request};
use hop_bar::conf::{parse, Config};
use hop_bar::options::Options;
use hop_bar::placeholders::Ctx;
use hop_bar::plugin::{self, cached_info, find, info_job, parse_output, Found};
use hop_bar::render::ColdRunner;
use hop_bar::session::{Session, Window};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

pub static N: AtomicUsize = AtomicUsize::new(0);
pub const NOW: i64 = 1_800_000_000;
pub const DAY: i64 = 86_400;

/// Temp root with fixture plugins copied into `bin` (mode 0755) and a
/// private state dir.
pub struct Fx {
    pub root: PathBuf,
    pub bin: PathBuf,
    pub state: PathBuf,
}

impl Fx {
    pub fn new() -> Fx {
        let root = std::env::temp_dir().join(format!(
            "bar-plugin-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        let (bin, state) = (root.join("bin"), root.join("state"));
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::create_dir_all(&state).unwrap();
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/plugins");
        for e in std::fs::read_dir(src).unwrap().flatten() {
            install(&e.path(), &bin.join(e.file_name()));
        }
        Fx { root, bin, state }
    }

    pub fn dirs(&self) -> Vec<PathBuf> {
        vec![self.bin.clone()]
    }

    pub fn found(&self, name: &str) -> Found {
        find(&self.dirs(), name).unwrap()
    }

    /// Never spawns: a cold read stays cold.
    pub fn cache(&self) -> Cache {
        Cache::with_exe(&self.state, PathBuf::from("/nonexistent/bar"))
    }

    pub fn file(&self, rel: &str) -> String {
        std::fs::read_to_string(self.bin.join(rel)).unwrap_or_default()
    }

    pub fn put(&self, rel: &str, body: &str) {
        std::fs::write(self.bin.join(rel), body).unwrap();
    }

    pub fn info_calls(&self) -> usize {
        self.file("info.log").lines().count()
    }

    /// Stores the interrogation result, as a finished refresh would.
    pub fn prime_info(&self, name: &str) {
        let (key, job) = info_job(&self.found(name));
        refresh(&self.cache(), key, job);
    }

    /// Primes the info, then runs the render job once.
    pub fn prime(&self, name: &str, cfg: &Config, o: &[(&str, &str)]) {
        self.prime_in(&session(), name, cfg, o);
    }

    pub fn prime_in(&self, s: &Session, name: &str, cfg: &Config, o: &[(&str, &str)]) {
        self.prime_info(name);
        let s = s.clone();
        let cache = self.cache();
        let dirs = self.dirs();
        let ctx = ctx(&s, cfg, &cache, &dirs);
        let found = self.found(name);
        let inf = cached_info(&cache, &found).unwrap();
        let p = plugin::plan(&ctx, &found, &inf, &opts(o)).unwrap();
        refresh(&cache, p.key, p.job);
    }

    pub fn show(&self, name: &str, cfg: &Config, o: &[(&str, &str)]) -> String {
        self.show_in(&session(), name, cfg, o)
    }

    pub fn show_in(&self, s: &Session, name: &str, cfg: &Config, o: &[(&str, &str)]) -> String {
        let s = s.clone();
        let cache = self.cache();
        let dirs = self.dirs();
        plugin::render(&ctx(&s, cfg, &cache, &dirs), name, &opts(o)).unwrap_or_default()
    }
}

impl Drop for Fx {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

pub fn install(from: &Path, to: &Path) {
    std::fs::copy(from, to).unwrap();
    std::fs::set_permissions(to, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
}

pub fn refresh(cache: &Cache, key: String, job: hop_bar::cache::Job) {
    cache.refresh(
        &Request {
            key,
            job,
            lock: String::new(),
        },
        Duration::from_secs(20),
    );
}

pub fn opts(o: &[(&str, &str)]) -> Options {
    let mut out = Options::default();
    for (k, v) in o {
        out.set(k, v);
    }
    out
}

pub fn session() -> Session {
    Session {
        cwd: "/tmp/work".into(),
        model: "Opus".into(),
        session_id: "s-1".into(),
        five_hour: Some(Window {
            used_percentage: 10.0,
            resets_at: NOW + 3600,
        }),
        ..Session::default()
    }
}

pub fn ctx<'a>(s: &'a Session, cfg: &'a Config, cache: &'a Cache, dirs: &'a [PathBuf]) -> Ctx<'a> {
    Ctx {
        session: s,
        config: cfg,
        now: NOW,
        runner: &ColdRunner,
        cache: Some(cache),
        columns: Some(120),
        plugin_dirs: dirs,
    }
}

pub fn cfg(src: &str) -> Config {
    parse(src).unwrap()
}

pub fn paint(out: &str, name: &str, cfg: &Config) -> String {
    plugin::paint(&parse_output(out).unwrap(), name, cfg, NOW).unwrap_or_default()
}
