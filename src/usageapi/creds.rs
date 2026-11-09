//! Claude Code's OAuth access token, read where Claude Code stores it:
//! the macOS login keychain, else `<claude dir>/.credentials.json`.
//! Only the background job calls this; the token is returned to the
//! caller and never written anywhere.

use crate::secret::{wipe, Secret};
use std::path::Path;
use std::time::Duration;

/// Larger than any credentials file by a wide margin.
const MAX_FILE: u64 = 64 * 1024;

/// Keychain service of Claude Code's credentials: `Claude Code-credentials`,
/// with `-<first 8 hex of sha256(dir)>` appended when `CLAUDE_CONFIG_DIR`
/// is set (`hashed`), as Claude Code names it.
pub fn service(dir: &str, hashed: bool) -> String {
    if hashed {
        format!(
            "Claude Code-credentials-{}",
            &super::sha256::hex(dir.as_bytes())[..8]
        )
    } else {
        "Claude Code-credentials".into()
    }
}

/// Keychain account Claude Code writes: `USER` when it is a plain name,
/// else `claude-code-user`.
pub fn account(user: Option<&str>) -> String {
    match user {
        Some(u)
            if !u.is_empty()
                && u.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b)) =>
        {
            u.into()
        }
        _ => "claude-code-user".into(),
    }
}

/// A token as it may appear in a curl config line: no quote, backslash,
/// space or control character can end the value early.
fn plausible(t: &str) -> bool {
    (16..=4096).contains(&t.len())
        && t.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-._~+/=".contains(&b))
}

#[derive(serde::Deserialize)]
struct CredFile<'a> {
    #[serde(rename = "claudeAiOauth", borrow)]
    oauth: Option<OAuth<'a>>,
}

#[derive(serde::Deserialize)]
struct OAuth<'a> {
    // borrowed: the token is never copied into a string serde owns
    #[serde(rename = "accessToken", borrow)]
    access_token: Option<&'a str>,
    #[serde(rename = "expiresAt")]
    expires_at: Option<f64>,
}

/// `claudeAiOauth.accessToken` of a credentials JSON, unless
/// `claudeAiOauth.expiresAt` (unix milliseconds) has passed at `now_ms`.
/// bar never refreshes the token: Claude Code does when it next runs.
pub fn from_json(text: &[u8], now_ms: i64) -> Option<Secret> {
    let f: CredFile = serde_json::from_slice(text).ok()?;
    let o = f.oauth?;
    if o.expires_at.is_some_and(|exp| exp <= now_ms as f64) {
        return None;
    }
    let t = o.access_token?;
    plausible(t).then(|| Secret::new(t.as_bytes().to_vec()))
}

/// Why no token came back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoToken {
    /// None found, or expired.
    Missing,
    /// The keychain did not answer in time (e.g. a permission prompt
    /// nobody answered).
    Blocked,
}

/// `<dir>/.credentials.json`.
fn file(dir: &Path, now_ms: i64) -> Option<Secret> {
    use std::io::Read;
    let mut buf = Secret::with_capacity(MAX_FILE as usize + 1);
    let mut chunk = [0u8; 4096];
    let mut f = std::fs::File::open(dir.join(".credentials.json")).ok()?;
    loop {
        let n = f.read(&mut chunk).ok()?;
        if n == 0 {
            break;
        }
        if buf.as_bytes().len() + n > MAX_FILE as usize {
            wipe(&mut chunk);
            return None;
        }
        buf.push(&chunk[..n]);
    }
    wipe(&mut chunk);
    from_json(buf.as_bytes(), now_ms)
}

/// `/usr/bin/security find-generic-password -s <service> [-a <account>] -w`:
/// the item's secret on stdout, read from a pipe. The token is never an
/// argument of any process.
#[cfg(target_os = "macos")]
fn keychain(
    service: &str,
    account: Option<&str>,
    now_ms: i64,
    limit: Duration,
) -> Result<Secret, NoToken> {
    use crate::cache::{run_child_raw, Child};
    let mut c = std::process::Command::new(crate::platform::system_program("security"));
    c.args(["find-generic-password", "-s", service]);
    if let Some(a) = account {
        c.args(["-a", a]);
    }
    c.arg("-w");
    match run_child_raw(c, None, limit) {
        Child::Out(out) => {
            let out = Secret::new(out);
            from_json(out.as_bytes().trim_ascii(), now_ms).ok_or(NoToken::Missing)
        }
        Child::TimedOut => Err(NoToken::Blocked),
        Child::Failed => Err(NoToken::Missing),
    }
}

/// The access token for Claude Code config dir `dir`. Order: on macOS the
/// keychain entry with the account Claude Code writes, then the
/// credentials file, then (macOS) the keychain entry of any account.
/// `Blocked` when a keychain read timed out and nothing else was found.
pub fn access_token(
    dir: &Path,
    hashed: bool,
    now_ms: i64,
    limit: Duration,
) -> Result<Secret, NoToken> {
    #[cfg(target_os = "macos")]
    {
        let svc = service(&dir.to_string_lossy(), hashed);
        let user = account(std::env::var("USER").ok().as_deref());
        // both lookups together take at most 5 s of `limit`
        let end = std::time::Instant::now() + limit.min(Duration::from_secs(5));
        let left = || end.saturating_duration_since(std::time::Instant::now());
        let first = keychain(&svc, Some(&user), now_ms, left());
        if first.is_ok() {
            return first;
        }
        if let Some(t) = file(dir, now_ms) {
            return Ok(t);
        }
        if matches!(first, Err(NoToken::Blocked)) {
            return first;
        }
        keychain(&svc, None, now_ms, left())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (hashed, limit);
        file(dir, now_ms).ok_or(NoToken::Missing)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_names_follow_the_config_dir() {
        assert_eq!(service("/x", false), "Claude Code-credentials");
        // sha256("/Users/me/.claude-work") starts with these 8 hex digits
        let h = &crate::usageapi::sha256::hex(b"/Users/me/.claude-work")[..8];
        assert_eq!(
            service("/Users/me/.claude-work", true),
            format!("Claude Code-credentials-{h}")
        );
    }

    #[test]
    fn account_falls_back_for_odd_names() {
        assert_eq!(account(Some("jo.e-1_x")), "jo.e-1_x");
        assert_eq!(account(Some("a b")), "claude-code-user");
        assert_eq!(account(Some("")), "claude-code-user");
        assert_eq!(account(None), "claude-code-user");
    }

    #[test]
    fn token_from_json() {
        let tok = "sk-ant-oat01-AAAABBBBCCCCDDDD_eeee";
        let j = |exp: i64, t: &str| {
            format!(r#"{{"claudeAiOauth":{{"accessToken":"{t}","expiresAt":{exp}}}}}"#)
        };
        let got =
            |text: &str, now: i64| from_json(text.as_bytes(), now).map(|s| s.as_bytes().to_vec());
        assert_eq!(got(&j(2_000, tok), 1_000).as_deref(), Some(tok.as_bytes()));
        assert_eq!(got(&j(1_000, tok), 1_000), None, "expired");
        assert_eq!(got(&j(2_000, "short"), 1_000), None);
        assert_eq!(got(&j(2_000, "sk-ant-oat01-AAAA\\\"BBBBCCCC"), 1_000), None);
        assert_eq!(got(&j(2_000, "sk-ant-oat01-AAAA BBBBCCCC"), 1_000), None);
        assert_eq!(got("{}", 0), None);
        assert_eq!(got("not json", 0), None);
        let no_exp = format!(r#"{{"claudeAiOauth":{{"accessToken":"{tok}"}}}}"#);
        assert_eq!(got(&no_exp, 1).as_deref(), Some(tok.as_bytes()));
    }
}
