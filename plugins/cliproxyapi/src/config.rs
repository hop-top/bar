//! The plugin's own configuration file: which proxy instances it may talk
//! to and where each one's management key comes from. It is a separate
//! file, owned by the user with no group or other access, because anything
//! that steers where a key is sent (or which command runs) must not be
//! settable from `bar.conf`, where a project's `.bar.conf` can replace
//! plugin lines.
//!
//! ```text
//! instance http://127.0.0.1:8318
//!   management http://127.0.0.1:8318/v8/management
//!   key-file ~/.config/cliproxyapi/management-key
//!   providers claude,codex
//!   alias "*@example.test" work
//! ```

use bar_plugin::fsio::{self, FileError};
use bar_plugin::keysource::{split_argv, KeySource};
use bar_plugin::url::Origin;
use std::path::{Path, PathBuf};

/// Largest config file accepted, in bytes.
pub const MAX_FILE: usize = 16 * 1024;
/// Providers the plugin can read quota for.
pub const PROVIDERS: [&str; 2] = ["claude", "codex"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Instance {
    /// The base URL's origin, as Claude Code's `ANTHROPIC_BASE_URL` is
    /// matched against.
    pub origin: Origin,
    /// `<scheme>://<host>:<port>/v8/management` (or `/v0/...`).
    pub management: String,
    pub key: KeySource,
    pub providers: Vec<String>,
    /// Plain `http` is for loopback hosts only; any other host needs
    /// `remote yes` and `https`.
    pub remote: bool,
    /// `(glob on the email, label)`, first match wins.
    pub aliases: Vec<(String, String)>,
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

/// `BAR_CLIPROXYAPI_CONFIG`, else `cliproxyapi.conf` in bar's config dir.
pub fn path() -> PathBuf {
    match std::env::var_os("BAR_CLIPROXYAPI_CONFIG").map(PathBuf::from) {
        Some(p) if p.is_absolute() => p,
        _ => bar_plugin::dirs::config("bar").join("cliproxyapi.conf"),
    }
}

/// Reads and parses the file at `path`.
pub fn load(path: &Path) -> Result<Config, LoadError> {
    let raw = fsio::read_private(path, MAX_FILE).map_err(|e| match e {
        FileError::Missing => LoadError::Missing,
        e => LoadError::Unsafe(e),
    })?;
    let text = std::str::from_utf8(raw.as_bytes()).map_err(|_| LoadError::NotUtf8)?;
    let home = std::env::var_os("HOME").map(PathBuf::from);
    Ok(parse(text, home.as_deref()))
}

#[derive(Default)]
struct Draft {
    line: usize,
    /// Errors already found when the block began: more means a line in
    /// it was refused, and the instance is dropped.
    errors_before: usize,
    url: String,
    management: Option<String>,
    key: Option<KeySource>,
    providers: Option<Vec<String>>,
    remote: bool,
    aliases: Vec<(String, String)>,
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
        let mut err = |e: &str| cfg.errors.push(format!("line {}: {e}", n + 1));
        let set_key = |k: KeySource, d: &mut Draft, err: &mut dyn FnMut(&str)| {
            if d.key.is_some() {
                err("more than one key source");
            } else {
                d.key = Some(k);
            }
        };
        match (key, args) {
            ("management", [u]) => d.management = Some(u.clone()),
            ("key-file", [p]) => match fsio::expand_home(p, home) {
                Some(p) if p.is_absolute() => set_key(KeySource::File(p), d, &mut err),
                _ => err("key-file must be an absolute path"),
            },
            ("key-keychain", [s]) => set_key(KeySource::Keychain(s.clone()), d, &mut err),
            ("key-command", argv) if !argv.is_empty() => {
                if Path::new(&argv[0]).is_absolute() {
                    set_key(KeySource::Command(argv.to_vec()), d, &mut err)
                } else {
                    err("key-command must start with an absolute path");
                }
            }
            ("providers", [list]) => {
                d.providers = Some(
                    list.split(',')
                        .map(|s| s.trim().to_ascii_lowercase())
                        .collect(),
                )
            }
            ("remote", [v]) => d.remote = matches!(v.as_str(), "yes" | "true" | "on" | "1"),
            ("alias", [glob, label]) => d.aliases.push((glob.to_ascii_lowercase(), label.clone())),
            _ => err(&format!("unknown or malformed `{key}`")),
        }
    }
    finish(cur.take(), &mut cfg);
    cfg
}

fn build(d: &Draft) -> Result<Instance, String> {
    let origin = Origin::parse(&d.url).ok_or("instance: not an http(s) URL")?;
    let management = match &d.management {
        Some(m) => m.trim_end_matches('/').to_string(),
        None => format!("{origin}/v8/management"),
    };
    let m = Origin::parse(&management).ok_or("management: not an http(s) URL")?;
    let tail = management
        .split_once("://")
        .and_then(|(_, r)| r.find('/').map(|i| &r[i..]))
        .unwrap_or("");
    if tail != "/v8/management" && tail != "/v0/management" {
        return Err("management: URL must end in /v8/management or /v0/management".into());
    }
    if !m.is_loopback() && !(d.remote && m.scheme == "https") {
        return Err("management: a non-loopback host needs `remote yes` and https".into());
    }
    let providers = d
        .providers
        .clone()
        .unwrap_or_else(|| PROVIDERS.iter().map(|s| s.to_string()).collect());
    if let Some(bad) = providers.iter().find(|p| !PROVIDERS.contains(&p.as_str())) {
        return Err(format!(
            "providers: `{bad}` is not supported (claude, codex)"
        ));
    }
    Ok(Instance {
        origin,
        management,
        key: d
            .key
            .clone()
            .ok_or("no key source (key-file, key-keychain or key-command)")?,
        providers,
        remote: d.remote,
        aliases: d.aliases.clone(),
    })
}

impl Config {
    pub fn instance_for(&self, origin: &Origin) -> Option<&Instance> {
        self.instances.iter().find(|i| &i.origin == origin)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = "# c\ninstance HTTP://127.0.0.1:8318/\n  key-file ~/k\n  alias \"*@a.test\" work\ninstance https://p.example\n  remote yes\n  key-keychain svc\n  providers codex\n";

    #[test]
    fn parses_instances() {
        let c = parse(GOOD, Some(Path::new("/h")));
        assert!(c.errors.is_empty(), "{:?}", c.errors);
        assert_eq!(c.instances.len(), 2);
        let a = &c.instances[0];
        assert_eq!(a.origin.to_string(), "http://127.0.0.1:8318");
        assert_eq!(a.management, "http://127.0.0.1:8318/v8/management");
        assert_eq!(a.key, KeySource::File("/h/k".into()));
        assert_eq!(a.providers, ["claude", "codex"]);
        assert_eq!(a.aliases, [("*@a.test".to_string(), "work".to_string())]);
        let b = &c.instances[1];
        assert_eq!(b.management, "https://p.example:443/v8/management");
        assert_eq!(b.providers, ["codex"]);
        let o = Origin::parse("http://127.0.0.1:8318/anything").unwrap();
        assert!(c.instance_for(&o).is_some());
        assert!(c
            .instance_for(&Origin::parse("http://127.0.0.1:1").unwrap())
            .is_none());
    }

    #[test]
    fn refuses_unsafe_instances() {
        let cases = [
            (
                "instance http://proxy.example\n key-file /k\n",
                "remote yes",
            ),
            (
                "instance http://proxy.example\n remote yes\n key-file /k\n",
                "remote yes",
            ),
            ("instance http://127.0.0.1:1\n", "no key source"),
            (
                "instance http://127.0.0.1:1\n key-file /k\n key-keychain s\n",
                "more than one",
            ),
            (
                "instance http://127.0.0.1:1\n key-command bin/x\n",
                "absolute",
            ),
            ("instance http://127.0.0.1:1\n key-file k\n", "absolute"),
            (
                "instance http://127.0.0.1:1\n key-file /k\n providers xai\n",
                "not supported",
            ),
            (
                "instance http://127.0.0.1:1\n key-file /k\n management http://127.0.0.1:1/x\n",
                "must end",
            ),
            ("instance ftp://x\n key-file /k\n", "not an http"),
            ("key-file /k\n", "outside an instance"),
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
    fn key_command_keeps_its_arguments() {
        let c = parse(
            "instance http://localhost:1\n key-command \"/usr/bin/pass\" show \"a b\"\n",
            None,
        );
        assert_eq!(
            c.instances[0].key,
            KeySource::Command(vec!["/usr/bin/pass".into(), "show".into(), "a b".into()])
        );
    }

    #[cfg(unix)]
    #[test]
    fn load_refuses_a_group_readable_file() {
        use std::os::unix::fs::PermissionsExt;
        let p = std::env::temp_dir().join(format!("cpa-conf-{}", std::process::id()));
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
