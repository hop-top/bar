//! The plugin's own configuration file (`routellm.conf`): which proxy
//! instances it may change, where each one's RouteLLM is reached and where
//! the management key comes from. A separate file, owner-only, because
//! this plugin *writes* the proxy's configuration: the grant is explicit,
//! separately removable, and nothing that decides where a key or a toggle
//! goes is settable from `bar.conf`.
//!
//! ```text
//! instance http://127.0.0.1:8318
//!   routellm http://127.0.0.1:6060
//!   provider routellm
//!   key-from cliproxyapi
//!   local-tiers private,default,local_text
//!   on-priority 1000
//!   off-map route-auto=claude-sonnet-5-5
//! ```

use bar_plugin::fsio::{self, FileError};
use bar_plugin::keysource::{split_argv, KeySource};
use bar_plugin::url::Origin;
use std::path::{Path, PathBuf};

/// Largest config file accepted, in bytes.
pub const MAX_FILE: usize = 16 * 1024;
pub const DEFAULT_PROVIDER: &str = "routellm";
pub const DEFAULT_LOCAL: [&str; 3] = ["private", "default", "local_text"];
pub const DEFAULT_OFF: [(&str, &str); 2] = [
    ("route-auto", "claude-sonnet-5-5"),
    ("route-vision", "claude-sonnet-5-5"),
];
pub const DEFAULT_ON_PRIORITY: i64 = 1000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyRef {
    Own(KeySource),
    /// The key source of the same instance in `cliproxyapi.conf`.
    FromCpa,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Instance {
    /// The proxy's origin, as Claude Code's `ANTHROPIC_BASE_URL` is
    /// matched against.
    pub origin: Origin,
    /// Where this machine reaches RouteLLM.
    pub routellm: Origin,
    /// The proxy's `openai-compatibility` entry that carries RouteLLM.
    pub provider: String,
    pub key: KeyRef,
    /// `<origin>/v0/management`: the field-level routes live there.
    pub management: String,
    /// Tiers declared to stay on this machine (a marker, not a proof).
    pub local_tiers: Vec<String>,
    /// Priority the provider gets when switched on (when lower).
    pub on_priority: i64,
    /// `(alias, model)`: what routing OFF maps an alias to (`setup`).
    pub off_map: Vec<(String, String)>,
    /// Non-loopback hosts need `remote yes` and https.
    pub remote: bool,
}

#[derive(Debug, Default, PartialEq)]
pub struct Config {
    pub instances: Vec<Instance>,
    /// Problems found while parsing; the instances that parsed are kept.
    pub errors: Vec<String>,
}

#[derive(Debug, PartialEq)]
pub enum LoadError {
    /// No file: the plugin is not set up, which is not a problem.
    Missing,
    Unsafe(FileError),
    NotUtf8,
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoadError::Missing => write!(f, "no configuration file"),
            LoadError::Unsafe(e) => write!(f, "configuration file refused: {e}"),
            LoadError::NotUtf8 => write!(f, "configuration file is not UTF-8 text"),
        }
    }
}

/// `BAR_ROUTELLM_CONFIG`, else `routellm.conf` in bar's config dir.
pub fn path() -> PathBuf {
    match std::env::var_os("BAR_ROUTELLM_CONFIG").map(PathBuf::from) {
        Some(p) if p.is_absolute() => p,
        _ => bar_plugin::dirs::config("bar").join("routellm.conf"),
    }
}

/// `BAR_CLIPROXYAPI_CONFIG`, else `cliproxyapi.conf` in bar's config dir.
pub fn cpa_path() -> PathBuf {
    match std::env::var_os("BAR_CLIPROXYAPI_CONFIG").map(PathBuf::from) {
        Some(p) if p.is_absolute() => p,
        _ => bar_plugin::dirs::config("bar").join("cliproxyapi.conf"),
    }
}

pub fn load(path: &Path) -> Result<Config, LoadError> {
    let raw = fsio::read_private(path, MAX_FILE).map_err(|e| match e {
        FileError::Missing => LoadError::Missing,
        e => LoadError::Unsafe(e),
    })?;
    let text = std::str::from_utf8(raw.as_bytes()).map_err(|_| LoadError::NotUtf8)?;
    Ok(parse(text, bar_plugin::dirs::home().as_deref()))
}

#[derive(Default)]
struct Draft {
    line: usize,
    errors_before: usize,
    url: String,
    routellm: Option<String>,
    provider: Option<String>,
    key: Option<KeyRef>,
    management: Option<String>,
    local: Option<Vec<String>>,
    on_priority: Option<i64>,
    off_map: Vec<(String, String)>,
    remote: bool,
}

fn token_ok(s: &str, max: usize) -> bool {
    (1..=max).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-/:".contains(&b))
}

/// Parses config text; `home` expands a leading `~/` in `key-file`.
pub fn parse(text: &str, home: Option<&Path>) -> Config {
    let mut cfg = Config::default();
    let mut cur: Option<Draft> = None;
    let finish = |d: Option<Draft>, cfg: &mut Config| {
        if let Some(d) = d {
            if cfg.errors.len() > d.errors_before {
                return;
            }
            match build(&d) {
                Ok(i) => cfg.instances.push(i),
                Err(e) => cfg.errors.push(format!("line {}: {e}", d.line)),
            }
        }
    };
    for (n, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some(tok) = split_argv(line) else {
            cfg.errors.push(format!("line {}: unbalanced quote", n + 1));
            continue;
        };
        let (key, args) = (tok[0].as_str(), &tok[1..]);
        if key == "instance" {
            finish(cur.take(), &mut cfg);
            match args {
                [url] => {
                    cur = Some(Draft {
                        line: n + 1,
                        errors_before: cfg.errors.len(),
                        url: url.clone(),
                        ..Draft::default()
                    })
                }
                _ => cfg
                    .errors
                    .push(format!("line {}: instance takes one URL", n + 1)),
            }
            continue;
        }
        let Some(d) = cur.as_mut() else {
            cfg.errors
                .push(format!("line {}: `{key}` outside an instance", n + 1));
            continue;
        };
        if let Err(e) = d.set(key, args, home) {
            cfg.errors.push(format!("line {}: {e}", n + 1));
        }
    }
    finish(cur.take(), &mut cfg);
    cfg
}

impl Draft {
    /// One directive inside an instance.
    fn set(&mut self, key: &str, args: &[String], home: Option<&Path>) -> Result<(), String> {
        match (key, args) {
            ("routellm", [u]) => self.routellm = Some(u.clone()),
            ("provider", [p]) if token_ok(p, 64) => self.provider = Some(p.clone()),
            ("management", [u]) => self.management = Some(u.clone()),
            ("key-from", [w]) if w == "cliproxyapi" => self.set_key(KeyRef::FromCpa)?,
            ("key-file", [p]) => match fsio::expand_home(p, home) {
                Some(p) if p.is_absolute() => self.set_key(KeyRef::Own(KeySource::File(p)))?,
                _ => return Err("key-file must be an absolute path".into()),
            },
            ("key-keychain", [s]) => self.set_key(KeyRef::Own(KeySource::Keychain(s.clone())))?,
            ("key-command", argv) if !argv.is_empty() => {
                if !Path::new(&argv[0]).is_absolute() {
                    return Err("key-command must start with an absolute path".into());
                }
                self.set_key(KeyRef::Own(KeySource::Command(argv.to_vec())))?;
            }
            ("local-tiers", [list]) => self.local = Some(local_tiers(list)?),
            ("on-priority", [p]) => match p.parse::<i64>() {
                Ok(v) if (-1_000_000..=1_000_000).contains(&v) => self.on_priority = Some(v),
                _ => return Err("on-priority must be an integer".into()),
            },
            ("off-map", [kv]) => match kv.split_once('=') {
                Some((a, m)) if token_ok(a, 64) && token_ok(m, 128) => {
                    self.off_map.push((a.to_string(), m.to_string()));
                }
                _ => return Err("off-map takes alias=model".into()),
            },
            ("remote", [v]) => self.remote = matches!(v.as_str(), "yes" | "true" | "on" | "1"),
            _ => return Err(format!("unknown or malformed `{key}`")),
        }
        Ok(())
    }

    fn set_key(&mut self, k: KeyRef) -> Result<(), String> {
        if self.key.is_some() {
            return Err("more than one key source".into());
        }
        self.key = Some(k);
        Ok(())
    }
}

/// `local-tiers a,b,c`: the names, trimmed, empty ones dropped.
fn local_tiers(list: &str) -> Result<Vec<String>, String> {
    let v: Vec<String> = list
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if v.iter().all(|t| token_ok(t, 64)) {
        Ok(v)
    } else {
        Err("local-tiers: tier names are letters, digits and ._-".into())
    }
}

/// Origin and path-tail checks shared by the URLs that may carry a key.
fn private_ok(o: &Origin, remote: bool, what: &str) -> Result<(), String> {
    if !o.is_loopback() && !(remote && o.scheme == "https") {
        return Err(format!(
            "{what}: a non-loopback host needs `remote yes` and https"
        ));
    }
    Ok(())
}

fn build(d: &Draft) -> Result<Instance, String> {
    let origin = Origin::parse(&d.url).ok_or("instance: not an http(s) URL")?;
    let routellm = Origin::parse(d.routellm.as_deref().ok_or("no `routellm` URL")?)
        .ok_or("routellm: not an http(s) URL")?;
    private_ok(&routellm, d.remote, "routellm")?;
    let management = match &d.management {
        Some(m) => {
            let m = m.trim_end_matches('/');
            let tail = m
                .split_once("://")
                .and_then(|(_, r)| r.find('/').map(|i| &r[i..]))
                .unwrap_or("");
            let head = m.strip_suffix(tail).unwrap_or(m);
            if tail != "/v0/management" && tail != "/v8/management" {
                return Err("management: URL must end in /v0/management or /v8/management".into());
            }
            // field-level changes live in the v0 routes
            format!("{head}/v0/management")
        }
        None => format!("{origin}/v0/management"),
    };
    let m = Origin::parse(&management).ok_or("management: not an http(s) URL")?;
    private_ok(&m, d.remote, "management")?;
    Ok(Instance {
        origin,
        routellm,
        provider: d
            .provider
            .clone()
            .unwrap_or_else(|| DEFAULT_PROVIDER.into()),
        key: d.key.clone().unwrap_or(KeyRef::FromCpa),
        management,
        local_tiers: d
            .local
            .clone()
            .unwrap_or_else(|| DEFAULT_LOCAL.iter().map(|s| s.to_string()).collect()),
        on_priority: d.on_priority.unwrap_or(DEFAULT_ON_PRIORITY),
        off_map: if d.off_map.is_empty() {
            DEFAULT_OFF
                .iter()
                .map(|(a, m)| (a.to_string(), m.to_string()))
                .collect()
        } else {
            d.off_map.clone()
        },
        remote: d.remote,
    })
}

impl Config {
    pub fn instance_for(&self, origin: &Origin) -> Option<&Instance> {
        self.instances.iter().find(|i| &i.origin == origin)
    }
}

impl Instance {
    /// Where the management key comes from. `key-from cliproxyapi` takes
    /// the key line of the same instance in `cliproxyapi.conf` (the file
    /// is held to the same owner-only rule).
    pub fn key_source(&self) -> Result<KeySource, String> {
        match &self.key {
            KeyRef::Own(k) => Ok(k.clone()),
            KeyRef::FromCpa => key_from_cpa(&cpa_path(), &self.origin),
        }
    }
}

/// The key source of the `instance` block for `origin` in the
/// `cliproxyapi.conf` at `path`.
pub fn key_from_cpa(path: &Path, origin: &Origin) -> Result<KeySource, String> {
    let raw = fsio::read_private(path, MAX_FILE).map_err(|e| format!("cliproxyapi.conf: {e}"))?;
    let text = std::str::from_utf8(raw.as_bytes()).map_err(|_| "cliproxyapi.conf: not UTF-8")?;
    let home = bar_plugin::dirs::home();
    let mut inside = false;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some(tok) = split_argv(line) else {
            continue;
        };
        let (key, args) = (tok[0].as_str(), &tok[1..]);
        if key == "instance" {
            inside = args.first().and_then(|u| Origin::parse(u)).as_ref() == Some(origin);
            continue;
        }
        if !inside {
            continue;
        }
        match (key, args) {
            ("key-file", [p]) => {
                if let Some(p) = fsio::expand_home(p, home.as_deref()).filter(|p| p.is_absolute()) {
                    return Ok(KeySource::File(p));
                }
            }
            ("key-keychain", [s]) => return Ok(KeySource::Keychain(s.clone())),
            ("key-command", argv) if !argv.is_empty() && Path::new(&argv[0]).is_absolute() => {
                return Ok(KeySource::Command(argv.to_vec()))
            }
            _ => {}
        }
    }
    Err(format!(
        "key-from cliproxyapi: no usable key source for {origin} in cliproxyapi.conf"
    ))
}

/// The short stable id of an instance, as the `instance` token carries it.
pub fn instance_id(origin: &Origin) -> String {
    bar_plugin::sha256::hex(origin.to_string().as_bytes())[..16].to_string()
}

/// Tests only: a `/`-rooted path made absolute on this platform too
/// (`/k` is relative on Windows; `C:/k` is not).
#[cfg(test)]
fn abs(p: &str) -> String {
    if cfg!(windows) {
        format!("C:{p}")
    } else {
        p.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = "# c\ninstance HTTP://127.0.0.1:8318/\n  routellm http://localhost:6060\n  off-map route-auto=claude-opus-5-5\n  local-tiers private\n  on-priority 500\ninstance http://127.0.0.1:9\n  routellm http://127.0.0.1:6061\n  key-file ~/k\n  management http://127.0.0.1:9/v8/management\n  provider rl2\n";

    #[test]
    fn parses_instances_with_defaults() {
        let home = abs("/h");
        let c = parse(GOOD, Some(Path::new(&home)));
        assert!(c.errors.is_empty(), "{:?}", c.errors);
        let a = &c.instances[0];
        assert_eq!(a.origin.to_string(), "http://127.0.0.1:8318");
        assert_eq!(a.management, "http://127.0.0.1:8318/v0/management");
        assert_eq!(a.key, KeyRef::FromCpa);
        assert_eq!(a.provider, "routellm");
        assert_eq!(a.on_priority, 500);
        assert_eq!(a.local_tiers, ["private"]);
        assert_eq!(
            a.off_map,
            [("route-auto".to_string(), "claude-opus-5-5".to_string())]
        );
        let b = &c.instances[1];
        // a v8 base is turned into the v0 one: field changes live there
        assert_eq!(b.management, "http://127.0.0.1:9/v0/management");
        assert_eq!(b.key, KeyRef::Own(KeySource::File(abs("/h/k").into())));
        assert_eq!(b.provider, "rl2");
        assert_eq!(b.on_priority, 1000);
        assert_eq!(b.off_map.len(), 2);
        assert_eq!(b.local_tiers, ["private", "default", "local_text"]);
    }

    #[test]
    fn refuses_unsafe_or_incomplete_instances() {
        let two_keys = format!(
            "instance http://127.0.0.1:1\n routellm http://127.0.0.1:2\n key-file {}\n key-keychain s\n",
            abs("/k")
        );
        let cases = [
            ("instance http://127.0.0.1:1\n", "no `routellm`"),
            (
                "instance http://127.0.0.1:1\n routellm http://rl.example:6060\n",
                "remote yes",
            ),
            (
                "instance http://127.0.0.1:1\n routellm http://127.0.0.1:2\n management http://proxy.example/v0/management\n",
                "remote yes",
            ),
            (
                "instance http://127.0.0.1:1\n routellm http://127.0.0.1:2\n remote yes\n management http://proxy.example/v0/management\n",
                "remote yes",
            ),
            (two_keys.as_str(), "more than one"),
            (
                "instance http://127.0.0.1:1\n routellm http://127.0.0.1:2\n key-command bin/x\n",
                "absolute",
            ),
            (
                "instance http://127.0.0.1:1\n routellm http://127.0.0.1:2\n management http://127.0.0.1:1/x\n",
                "must end",
            ),
            (
                "instance http://127.0.0.1:1\n routellm http://127.0.0.1:2\n off-map nonsense\n",
                "alias=model",
            ),
            (
                "instance http://127.0.0.1:1\n routellm http://127.0.0.1:2\n on-priority x\n",
                "integer",
            ),
            ("routellm http://127.0.0.1:2\n", "outside an instance"),
        ];
        for (text, want) in cases {
            let c = parse(text, None);
            assert!(c.instances.is_empty(), "{text:?}");
            assert!(
                c.errors.iter().any(|e| e.contains(want)),
                "{text:?}: {:?}",
                c.errors
            );
        }
    }

    #[test]
    fn a_remote_instance_needs_https_on_both_urls() {
        let t = "instance https://p.example\n routellm https://rl.example:6060\n remote yes\n key-keychain s\n";
        let c = parse(t, None);
        assert!(c.errors.is_empty(), "{:?}", c.errors);
        assert_eq!(
            c.instances[0].management,
            "https://p.example:443/v0/management"
        );
    }

    #[cfg(unix)]
    #[test]
    fn key_from_cliproxyapi_takes_the_same_instances_key_line() {
        use std::os::unix::fs::PermissionsExt;
        let p = std::env::temp_dir().join(format!("rl-cpa-conf-{}", std::process::id()));
        let text = "instance http://127.0.0.1:1\n key-file /other\ninstance http://127.0.0.1:8318\n management http://127.0.0.1:8318/v8/management\n key-command /usr/bin/pass show cpa\n providers claude\n";
        std::fs::write(&p, text).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600)).unwrap();
        let o = Origin::parse("http://127.0.0.1:8318").unwrap();
        assert_eq!(
            key_from_cpa(&p, &o).unwrap(),
            KeySource::Command(vec!["/usr/bin/pass".into(), "show".into(), "cpa".into()])
        );
        let none = Origin::parse("http://127.0.0.1:2").unwrap();
        assert!(key_from_cpa(&p, &none).is_err());
        // the file must be owner-only, like the plugin's own
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(key_from_cpa(&p, &o).unwrap_err().contains("mode"));
        let _ = std::fs::remove_file(&p);
    }

    #[cfg(unix)]
    #[test]
    fn load_refuses_a_group_readable_file() {
        use std::os::unix::fs::PermissionsExt;
        let p = std::env::temp_dir().join(format!("rl-conf-{}", std::process::id()));
        std::fs::write(&p, GOOD).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o640)).unwrap();
        assert_eq!(
            load(&p).unwrap_err(),
            LoadError::Unsafe(FileError::Mode(0o640))
        );
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(load(&p).is_ok());
        let _ = std::fs::remove_file(&p);
        assert_eq!(load(&p).unwrap_err(), LoadError::Missing);
    }
}
