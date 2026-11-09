//! The network: `curl` with a fixed argv, HTTPS only, rather than an HTTP
//! client dependency. Only background jobs and explicit commands call it,
//! never the render path.

use std::process::{Command, Stdio};

/// curl's argv: `-q` first so no `.curlrc` changes it, and `--` before
/// `url` so it is never read as an option.
fn args(url: &str, headers: &[&str], max_time: u64, cap: usize) -> Vec<String> {
    let mut a: Vec<String> = ["-q", "-fsS", "--max-time"].map(String::from).to_vec();
    a.push(max_time.max(1).to_string());
    a.extend(
        [
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            "--max-filesize",
        ]
        .map(String::from),
    );
    a.push(cap.to_string());
    for h in headers {
        a.push("-H".into());
        a.push(h.to_string());
    }
    a.push("--".into());
    a.push(url.into());
    a
}

/// Body of `url` (HTTPS, redirects HTTPS only) with extra `headers`
/// (`Name: value`), cut off after `max_time` seconds; a body over `cap`
/// bytes or a non-2xx status is an error.
pub(crate) fn get(
    url: &str,
    headers: &[&str],
    max_time: u64,
    cap: usize,
) -> Result<Vec<u8>, String> {
    let mut c = Command::new(crate::platform::program("curl"));
    // no console window when run from a background refresh on Windows
    crate::platform::hide_window(&mut c);
    c.args(args(url, headers, max_time, cap));
    let mut child = c
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("curl: {e}"))?;
    let out = child.stdout.take().ok_or("curl: no output")?;
    let body = crate::prices::update::read_capped(out, cap);
    if body.is_err() {
        let _ = child.kill();
    }
    let o = child.wait_with_output().map_err(|e| format!("curl: {e}"))?;
    let body = body?;
    if !o.status.success() {
        let err = String::from_utf8_lossy(&o.stderr);
        return Err(format!("curl failed ({}): {}", o.status, err.trim()));
    }
    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::args;

    #[test]
    fn argv_ignores_curlrc_and_ends_options_before_the_url() {
        let a = args("-K/etc/passwd", &["Accept: x"], 0, 10);
        assert_eq!(a[0], "-q");
        assert_eq!(a[a.len() - 2..], ["--", "-K/etc/passwd"]);
        assert!(a.windows(2).any(|w| w == ["--max-time", "1"]));
        assert!(a.windows(2).any(|w| w == ["-H", "Accept: x"]));
        assert!(a.windows(2).any(|w| w == ["--proto", "=https"]));
    }
}
