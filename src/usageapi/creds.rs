//! Claude Code's OAuth access token, read where Claude Code stores it:
//! the macOS login keychain, else `<claude dir>/.credentials.json`.
//! Only the background job calls this; the token is returned to the
//! caller and never written anywhere.

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

/// `claudeAiOauth.accessToken` of a credentials JSON, unless
/// `claudeAiOauth.expiresAt` (unix milliseconds) has passed at `now_ms`.
/// bar never refreshes the token: Claude Code does when it next runs.
pub fn from_json(text: &str, now_ms: i64) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(text).ok()?;
    let o = v.get("claudeAiOauth")?;
    if let Some(exp) = o.get("expiresAt").and_then(serde_json::Value::as_i64) {
        if exp <= now_ms {
            return None;
        }
    }
    let t = o.get("accessToken")?.as_str()?;
    plausible(t).then(|| t.to_string())
}

/// `<dir>/.credentials.json`.
fn file(dir: &Path, now_ms: i64) -> Option<String> {
    use std::io::Read;
    let mut s = String::new();
    std::fs::File::open(dir.join(".credentials.json"))
        .ok()?
        .take(MAX_FILE)
        .read_to_string(&mut s)
        .ok()?;
    from_json(&s, now_ms)
}

/// `security find-generic-password -s <service> [-a <account>] -w`:
/// the item's secret on stdout, read from a pipe. The token is never an
/// argument of any process.
#[cfg(target_os = "macos")]
fn keychain(service: &str, account: Option<&str>, now_ms: i64, limit: Duration) -> Option<String> {
    let mut c = std::process::Command::new(crate::platform::system_program("security"));
    c.args(["find-generic-password", "-s", service]);
    if let Some(a) = account {
        c.args(["-a", a]);
    }
    c.arg("-w");
    let out = crate::cache::run_child(c, None, limit)?;
    from_json(out.trim(), now_ms)
}

/// The access token for Claude Code config dir `dir`: on macOS the
/// keychain entry (with the account Claude Code writes, then any
/// account), else the credentials file. `None` when none is found, it
/// has expired or it does not look like a token.
pub fn access_token(dir: &Path, hashed: bool, now_ms: i64, limit: Duration) -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        let svc = service(&dir.to_string_lossy(), hashed);
        let user = account(std::env::var("USER").ok().as_deref());
        // both lookups together take at most 5 s of `limit`
        let end = std::time::Instant::now() + limit.min(Duration::from_secs(5));
        let left = || end.saturating_duration_since(std::time::Instant::now());
        if let Some(t) = keychain(&svc, Some(&user), now_ms, left())
            .or_else(|| keychain(&svc, None, now_ms, left()))
        {
            return Some(t);
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (hashed, limit);
    file(dir, now_ms)
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
        assert_eq!(from_json(&j(2_000, tok), 1_000).as_deref(), Some(tok));
        assert_eq!(from_json(&j(1_000, tok), 1_000), None, "expired");
        assert_eq!(from_json(&j(2_000, "short"), 1_000), None);
        assert_eq!(
            from_json(&j(2_000, "sk-ant-oat01-AAAA\\\"BBBBCCCC"), 1_000),
            None
        );
        assert_eq!(
            from_json(&j(2_000, "sk-ant-oat01-AAAA BBBBCCCC"), 1_000),
            None
        );
        assert_eq!(from_json("{}", 0), None);
        assert_eq!(from_json("not json", 0), None);
        let no_exp = format!(r#"{{"claudeAiOauth":{{"accessToken":"{tok}"}}}}"#);
        assert_eq!(from_json(&no_exp, 1).as_deref(), Some(tok));
    }
}
