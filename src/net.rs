//! The network: `curl` with a fixed argv, HTTPS only, rather than an HTTP
//! client dependency. Only background jobs and explicit commands call it,
//! never the render path.

use std::io::Write;
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
    // say who is asking, rather than curl's own version string
    a.push("--user-agent".into());
    a.push(concat!("bar/", env!("CARGO_PKG_VERSION")).into());
    for h in headers {
        a.push("-H".into());
        a.push(h.to_string());
    }
    a.push("--".into());
    a.push(url.into());
    a
}

/// `program` with `args`, no console window, and no `SSLKEYLOGFILE`: an
/// inherited one would make curl write the TLS session keys (and with
/// them the request, token included) to that file. The CA variables
/// (`CURL_CA_BUNDLE`, `SSL_CERT_FILE`, `SSL_CERT_DIR`) and proxy
/// variables are kept: networks with their own CA or proxy need them.
fn curl(program: std::path::PathBuf, args: Vec<String>) -> Command {
    let mut c = Command::new(program);
    // no console window when run from a background refresh on Windows
    crate::platform::hide_window(&mut c);
    c.args(args).env_remove("SSLKEYLOGFILE");
    c
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
    let mut c = curl(
        crate::platform::program("curl"),
        args(url, headers, max_time, cap),
    );
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

/// What `request` got back: the status, a `Retry-After` in seconds when
/// the server sent one as a number, and the body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Response {
    pub status: u16,
    pub retry_after: Option<u64>,
    pub body: Vec<u8>,
}

/// After the body: a newline, the status and the `Retry-After` header.
const TRAILER: &str = "\n%{http_code}\n%header{retry-after}";

/// `args` without `-f`, so any status comes back, plus the trailer.
fn request_args(url: &str, headers: &[&str], max_time: u64, cap: usize) -> Vec<String> {
    let mut a = args(url, headers, max_time, cap);
    a[1] = "-sS".into();
    let at = a.len() - 2;
    a.splice(
        at..at,
        ["-K".to_string(), "-".into(), "-w".into(), TRAILER.into()],
    );
    a
}

/// Splits curl's output into body and trailer.
fn split_trailer(out: &[u8]) -> Option<Response> {
    let text_end = out.len();
    let nl2 = out.iter().rposition(|&b| b == b'\n')?;
    let nl1 = out[..nl2].iter().rposition(|&b| b == b'\n')?;
    let status = std::str::from_utf8(&out[nl1 + 1..nl2])
        .ok()?
        .trim()
        .parse()
        .ok()?;
    let retry_after = std::str::from_utf8(&out[nl2 + 1..text_end])
        .ok()
        .and_then(|r| r.trim().parse().ok());
    Some(Response {
        status,
        retry_after,
        body: out[..nl1].to_vec(),
    })
}

/// GET `url` like `get`, with `config` (curl config-file lines, e.g. a
/// secret header) written to curl's stdin and read with `-K -`, so it
/// never appears in argv or the environment. Any HTTP status is returned.
/// The system curl runs (`platform::system_program`), never one found
/// on `PATH`.
pub(crate) fn request(
    url: &str,
    headers: &[&str],
    config: &[u8],
    max_time: u64,
    cap: usize,
) -> Result<Response, String> {
    // a fixed path: this request carries a secret
    let mut c = curl(
        crate::platform::system_program("curl"),
        request_args(url, headers, max_time, cap),
    );
    let mut child = c
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("curl: {e}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        // curl reads all of its config before it connects; dropped = EOF
        let _ = stdin.write_all(config);
    }
    let out = child.stdout.take().ok_or("curl: no output")?;
    // body plus the short trailer
    let body = crate::prices::update::read_capped(out, cap + 64);
    if body.is_err() {
        let _ = child.kill();
    }
    let st = child.wait().map_err(|e| format!("curl: {e}"))?;
    let body = body?;
    if !st.success() {
        return Err(format!("curl failed ({st})"));
    }
    split_trailer(&body).ok_or_else(|| "curl: no status".into())
}

#[cfg(test)]
mod tests {
    use super::{args, curl, request_args, split_trailer};

    #[test]
    fn curl_never_logs_tls_keys() {
        for c in [
            curl("curl".into(), vec![]),
            curl("/usr/bin/curl".into(), vec![]),
        ] {
            let removed = c
                .get_envs()
                .any(|(k, v)| k == "SSLKEYLOGFILE" && v.is_none());
            assert!(removed);
        }
    }

    #[test]
    fn argv_ignores_curlrc_and_ends_options_before_the_url() {
        let a = args("-K/etc/passwd", &["Accept: x"], 0, 10);
        assert_eq!(a[0], "-q");
        assert_eq!(a[a.len() - 2..], ["--", "-K/etc/passwd"]);
        assert!(a.windows(2).any(|w| w == ["--max-time", "1"]));
        assert!(a.windows(2).any(|w| w == ["-H", "Accept: x"]));
        assert!(a.windows(2).any(|w| w == ["--proto", "=https"]));
        let ua = format!("bar/{}", env!("CARGO_PKG_VERSION"));
        assert!(a.windows(2).any(|w| w == ["--user-agent", ua.as_str()]));
        let r = request_args("https://x", &[], 5, 10);
        assert!(r.windows(2).any(|w| w == ["--user-agent", ua.as_str()]));
    }

    #[test]
    fn request_reads_its_config_from_stdin() {
        let a = request_args("https://x", &["A: b"], 5, 10);
        assert_eq!(a[0], "-q");
        assert_eq!(a[1], "-sS");
        assert!(!a.iter().any(|x| x == "-fsS"));
        assert!(a.windows(2).any(|w| w == ["-K", "-"]));
        assert_eq!(a[a.len() - 2..], ["--", "https://x"]);
    }

    #[test]
    fn trailer_splits_off_status_and_retry_after() {
        let r = split_trailer(b"{\"a\":1}\n429\n120").unwrap();
        assert_eq!((r.status, r.retry_after), (429, Some(120)));
        assert_eq!(r.body, b"{\"a\":1}");
        let r = split_trailer(b"x\ny\n200\n").unwrap();
        assert_eq!(
            (r.status, r.retry_after, r.body.as_slice()),
            (200, None, &b"x\ny"[..])
        );
        assert!(split_trailer(b"no trailer").is_none());
    }
}
