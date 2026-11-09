//! The network: `curl` with a fixed argv, HTTPS only, rather than an HTTP
//! client dependency. Only background jobs and explicit commands call it,
//! never the render path.

use std::process::{Command, Stdio};

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
    c.args(["-fsS", "--max-time"])
        .arg(max_time.max(1).to_string())
        .args(["--proto", "=https"])
        .args(["--proto-redir", "=https", "--max-filesize"])
        .arg(cap.to_string());
    for h in headers {
        c.arg("-H").arg(h);
    }
    let mut child = c
        .arg(url)
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
