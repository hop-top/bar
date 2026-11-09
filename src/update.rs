//! Opt-in update check (`set update-check on`): a background job asks
//! GitHub for bar's releases once a day and caches the newest tag of the
//! running version's channel; `#{update}` and `bar update` read it. bar
//! never downloads or replaces itself.

use serde_json::Value;
use std::cmp::Ordering;
use std::path::Path;
use std::time::Duration;

pub const REPO_URL: &str = "https://github.com/hop-top/bar";
const API_URL: &str = "https://api.github.com/repos/hop-top/bar/releases?per_page=30";
/// The check runs at most this often, seconds.
pub const TTL: u64 = 86_400;
/// Larger than 30 releases' JSON by a wide margin.
const MAX_BYTES: usize = 4 << 20;

/// The running version.
pub fn current() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Cache key of the check for one channel.
pub fn cache_key(prerelease: bool) -> &'static str {
    if prerelease {
        "update:prerelease"
    } else {
        "update:stable"
    }
}

/// A prerelease build follows prereleases too; a stable build only
/// stable releases.
pub fn channel_is_prerelease(version: &str) -> bool {
    parse(version).is_some_and(|v| !v.pre.is_empty())
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Ident {
    Num(u64),
    Alnum(String),
}

impl Ord for Ident {
    fn cmp(&self, o: &Self) -> Ordering {
        match (self, o) {
            (Ident::Num(a), Ident::Num(b)) => a.cmp(b),
            (Ident::Num(_), Ident::Alnum(_)) => Ordering::Less,
            (Ident::Alnum(_), Ident::Num(_)) => Ordering::Greater,
            (Ident::Alnum(a), Ident::Alnum(b)) => a.cmp(b),
        }
    }
}

impl PartialOrd for Ident {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}

/// A semantic version; build metadata is dropped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    core: [u64; 3],
    pre: Vec<Ident>,
}

impl Ord for Version {
    /// SemVer precedence: a prerelease sorts before its release.
    fn cmp(&self, o: &Self) -> Ordering {
        self.core
            .cmp(&o.core)
            .then_with(|| match (self.pre.is_empty(), o.pre.is_empty()) {
                (true, true) => Ordering::Equal,
                (true, false) => Ordering::Greater,
                (false, true) => Ordering::Less,
                (false, false) => self.pre.cmp(&o.pre),
            })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}

/// `1.2.3`, `1.2.3-rc.1`, with any leading text before the first digit
/// (`v1.2.3`, `bar-v1.2.3`); `None` otherwise.
pub fn parse(s: &str) -> Option<Version> {
    let s = &s[s.find(|c: char| c.is_ascii_digit())?..];
    let s = s.split('+').next().unwrap_or_default();
    let (core, pre) = match s.split_once('-') {
        Some((c, p)) => (c, Some(p)),
        None => (s, None),
    };
    let nums: Vec<u64> = core
        .split('.')
        .map(|n| n.parse().ok())
        .collect::<Option<_>>()?;
    let core: [u64; 3] = nums.try_into().ok()?;
    let pre = match pre {
        None => Vec::new(),
        Some(p) => p
            .split('.')
            .map(|i| match i.parse() {
                Ok(n) => Some(Ident::Num(n)),
                Err(_) if !i.is_empty() => Some(Ident::Alnum(i.to_string())),
                Err(_) => None,
            })
            .collect::<Option<_>>()?,
    };
    Some(Version { core, pre })
}

/// The tag with the highest version among the published releases in a
/// GitHub releases list; prereleases only when `prerelease`.
pub fn latest(body: &[u8], prerelease: bool) -> Option<String> {
    let v: Value = serde_json::from_slice(body).ok()?;
    v.as_array()?
        .iter()
        .filter(|r| r["draft"] != Value::Bool(true))
        .filter(|r| prerelease || r["prerelease"] != Value::Bool(true))
        .filter_map(|r| {
            let tag = r["tag_name"].as_str()?;
            let ver = parse(tag)?;
            (prerelease || ver.pre.is_empty()).then(|| (ver, tag.to_string()))
        })
        .max_by(|a, b| a.0.cmp(&b.0))
        .map(|(_, t)| t)
}

/// The version of `tag` when it is newer than `current`.
pub fn newer(current: &str, tag: &str) -> Option<String> {
    let (c, t) = (parse(current)?, parse(tag)?);
    let shown = &tag[tag.find(|c: char| c.is_ascii_digit())?..];
    (t > c).then(|| shown.to_string())
}

/// Body of the background job: the newest tag, or empty when the check
/// failed (stored too, so a failure waits for the next day).
pub fn check(prerelease: bool, limit: Duration) -> String {
    crate::net::get(
        API_URL,
        &["Accept: application/vnd.github+json"],
        limit.as_secs(),
        MAX_BYTES,
    )
    .ok()
    .and_then(|b| latest(&b, prerelease))
    .unwrap_or_default()
}

/// How to upgrade the binary at `exe` to `tag`: Homebrew, cargo, or a
/// release archive, by where it is installed.
pub fn upgrade_hint(exe: &Path, tag: Option<&str>) -> String {
    let p = exe.display().to_string().replace('\\', "/");
    if ["/Cellar/", "/homebrew/", "/linuxbrew/"]
        .iter()
        .any(|d| p.contains(d))
    {
        return "brew upgrade bar".into();
    }
    if p.contains("/.cargo/bin/") {
        let tag = tag.map_or(String::new(), |t| format!(" --tag {t}"));
        return format!("cargo install --git {REPO_URL}.git{tag} --locked --force");
    }
    let page = tag.map_or(format!("{REPO_URL}/releases"), |t| {
        format!("{REPO_URL}/releases/tag/{t}")
    });
    format!("download the archive for your platform from {page} and replace {p}")
}

/// What `bar update` prints: the running version, what the last check
/// found (`cached`: `None` when the check is off, `Some(None)` before a
/// first result, an empty tag after a failed check) and how to upgrade.
pub fn report(current: &str, cached: Option<Option<&str>>, exe: &Path) -> String {
    let mut out = format!("bar {current}\n");
    let tag = cached.flatten().map(str::trim).filter(|t| !t.is_empty());
    let channel = if channel_is_prerelease(current) {
        "prerelease"
    } else {
        "stable"
    };
    match cached {
        None => {
            out += "update check off: `set update-check on` in bar.conf checks GitHub once a day\n";
        }
        Some(None) => out += "no check result yet; it runs in the background\n",
        Some(Some(_)) if tag.is_none() => out += "the last check failed; it runs again tomorrow\n",
        Some(Some(_)) => {}
    }
    let newer = tag.and_then(|t| newer(current, t));
    match (&newer, tag) {
        (Some(v), _) => out += &format!("newer {channel} release: {v}\n"),
        (None, Some(t)) => out += &format!("up to date (latest {channel} release: {t})\n"),
        (None, None) => {}
    }
    let target = if newer.is_some() { tag } else { None };
    out += &format!("upgrade: {}\n", upgrade_hint(exe, target));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semver_precedence() {
        let ordered = [
            "0.1.0-alpha.1",
            "0.1.0-alpha.2",
            "0.1.0-alpha.10",
            "0.1.0-alpha.beta",
            "0.1.0-beta.0",
            "0.1.0-rc.1",
            "0.1.0",
            "0.1.1",
            "0.2.0",
            "1.0.0",
        ];
        for w in ordered.windows(2) {
            assert!(parse(w[0]).unwrap() < parse(w[1]).unwrap(), "{w:?}");
        }
        assert_eq!(parse("v1.2.3"), parse("bar-v1.2.3+build.5"));
        for bad in ["", "v", "1.2", "1.2.3.4", "1.2.x", "1.2.3-", "1.2.3-a..b"] {
            assert!(parse(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn newer_only_when_greater() {
        assert_eq!(newer("0.1.0", "v0.2.0").as_deref(), Some("0.2.0"));
        assert_eq!(newer("0.2.0", "v0.2.0"), None);
        assert_eq!(newer("0.2.0", "v0.1.9"), None);
        assert_eq!(newer("0.2.0-rc.1", "v0.2.0").as_deref(), Some("0.2.0"));
        assert_eq!(newer("0.2.0", ""), None);
    }

    #[test]
    fn channels() {
        assert!(channel_is_prerelease("0.2.0-alpha.3"));
        assert!(!channel_is_prerelease("0.2.0"));
        let body = br#"[
            {"tag_name": "v0.3.0-alpha.1", "prerelease": true, "draft": false},
            {"tag_name": "v0.4.0", "prerelease": false, "draft": true},
            {"tag_name": "v0.2.1", "prerelease": false, "draft": false},
            {"tag_name": "v0.2.0", "prerelease": false, "draft": false},
            {"tag_name": "not-a-version", "prerelease": false, "draft": false}
        ]"#;
        assert_eq!(latest(body, false).as_deref(), Some("v0.2.1"));
        assert_eq!(latest(body, true).as_deref(), Some("v0.3.0-alpha.1"));
        assert_eq!(latest(b"{}", false), None);
        assert_eq!(latest(b"[]", false), None);
    }

    #[test]
    fn report_lines() {
        let exe = Path::new("/usr/local/bin/bar");
        let off = report("0.1.0", None, exe);
        assert!(off.starts_with("bar 0.1.0\nupdate check off"), "{off}");
        assert!(
            off.ends_with("/releases and replace /usr/local/bin/bar\n"),
            "{off}"
        );
        let r = report("0.1.0", Some(Some("v0.2.0\n")), exe);
        assert!(r.contains("newer stable release: 0.2.0\n"), "{r}");
        assert!(r.contains("/releases/tag/v0.2.0 "), "{r}");
        let r = report("0.2.0", Some(Some("v0.2.0")), exe);
        assert!(
            r.contains("up to date (latest stable release: v0.2.0)"),
            "{r}"
        );
        assert!(report("0.2.0", Some(Some("")), exe).contains("last check failed"));
        assert!(report("0.2.0", Some(None), exe).contains("no check result yet"));
        let r = report("0.3.0-alpha.1", Some(Some("v0.3.0-alpha.2")), exe);
        assert!(r.contains("newer prerelease release: 0.3.0-alpha.2"), "{r}");
    }

    #[test]
    fn hints_follow_the_install() {
        let h = |p: &str| upgrade_hint(Path::new(p), Some("v0.2.0"));
        assert_eq!(
            h("/opt/homebrew/Cellar/bar/0.1.0/bin/bar"),
            "brew upgrade bar"
        );
        assert_eq!(
            h("/home/me/.cargo/bin/bar"),
            "cargo install --git https://github.com/hop-top/bar.git --tag v0.2.0 --locked --force"
        );
        assert_eq!(
            h(r"C:\Users\me\.cargo\bin\bar.exe"),
            "cargo install --git https://github.com/hop-top/bar.git --tag v0.2.0 --locked --force"
        );
        assert!(h("/usr/local/bin/bar").contains("/releases/tag/v0.2.0"));
        assert!(upgrade_hint(Path::new("/x/bar"), None).contains("/releases and"));
    }
}
