//! The network: system `curl` with a fixed argv rather than an HTTP client
//! dependency. A secret (a bearer key, a request body) goes to curl as
//! config lines on stdin (`-K -`), so it never appears in argv or the
//! environment. Only background jobs and explicit commands call this,
//! never a render path.

use crate::fsio::read_capped;
use crate::secret::Secret;
use crate::url::Origin;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

/// Which URLs a request may use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scheme {
    /// HTTPS only, redirects included.
    Https,
    /// HTTPS, or plain HTTP to a loopback host (`localhost`, `127.0.0.1`,
    /// `::1`) only; checked before curl starts. Redirects stay HTTPS only.
    LoopbackHttp,
}

/// How one request runs.
#[derive(Debug, Clone)]
pub struct Opts {
    /// The curl to run.
    pub program: PathBuf,
    pub user_agent: String,
    /// Whole-request limit in seconds (at least 1).
    pub max_time: u64,
    /// Largest body in bytes; a longer one is an error.
    pub cap: usize,
    pub scheme: Scheme,
    /// `--noproxy '*'`: talk to the host directly, whatever `HTTPS_PROXY`
    /// and friends say.
    pub noproxy: bool,
}

impl Opts {
    /// The system curl, HTTPS only, proxies honoured.
    pub fn new(max_time: u64, cap: usize) -> Opts {
        Opts {
            program: system_curl(),
            user_agent: concat!("bar-plugin/", env!("CARGO_PKG_VERSION")).into(),
            max_time,
            cap,
            scheme: Scheme::Https,
            noproxy: false,
        }
    }
}

/// A curl a request carrying a secret may trust: macOS `/usr/bin/curl`,
/// Windows `%SystemRoot%\System32\curl.exe`, elsewhere the first
/// absolute `PATH` entry holding one (else `/usr/bin/curl`).
pub fn system_curl() -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        PathBuf::from("/usr/bin/curl")
    }
    #[cfg(windows)]
    {
        let root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
        PathBuf::from(root).join("System32").join("curl.exe")
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        let path = std::env::var_os("PATH").unwrap_or_default();
        std::env::split_paths(&path)
            .filter(|d| d.is_absolute())
            .map(|d| d.join("curl"))
            .find(|p| p.is_file())
            .unwrap_or_else(|| PathBuf::from("/usr/bin/curl"))
    }
}

/// curl's argv: `-q` first so no `.curlrc` changes it, and `--` before
/// `url` so it is never read as an option.
fn args(url: &str, headers: &[&str], o: &Opts) -> Vec<String> {
    let mut a: Vec<String> = ["-q", "-fsS", "--max-time"].map(String::from).to_vec();
    a.push(o.max_time.max(1).to_string());
    let proto = match o.scheme {
        Scheme::Https => "=https",
        Scheme::LoopbackHttp => "=http,https",
    };
    a.extend(
        [
            "--proto",
            proto,
            "--proto-redir",
            "=https",
            "--max-filesize",
        ]
        .map(String::from),
    );
    a.push(o.cap.to_string());
    // a plain-http request is never sent through a proxy: a bearer token
    // would cross it in the clear
    if o.noproxy || url.starts_with("http://") {
        a.extend(["--noproxy", "*"].map(String::from));
    }
    // say who is asking, rather than curl's own version string
    a.push("--user-agent".into());
    a.push(o.user_agent.clone());
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
fn curl(program: PathBuf, args: Vec<String>) -> Command {
    let mut c = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW: no console window from a background refresh
        c.creation_flags(0x0800_0000);
    }
    c.args(args).env_remove("SSLKEYLOGFILE");
    c
}

/// Plain HTTP only to loopback; checked on the parsed URL, so curl never
/// sees a plain-HTTP request to anywhere else.
fn check_scheme(url: &str, scheme: Scheme) -> Result<(), String> {
    if scheme == Scheme::Https {
        return Ok(());
    }
    match Origin::parse(url) {
        Some(o) if o.scheme == "https" || o.is_loopback() => Ok(()),
        Some(_) => Err("plain http is allowed to loopback hosts only".into()),
        None => Err("not an http(s) URL".into()),
    }
}

/// Body of `url` with extra `headers` (`Name: value`, not secret) cut off
/// after `max_time` seconds; a body over `cap` bytes or a non-2xx status
/// is an error. No stdin, no secrets.
pub fn get(url: &str, headers: &[&str], o: &Opts) -> Result<Vec<u8>, String> {
    check_scheme(url, o.scheme)?;
    let mut c = curl(o.program.clone(), args(url, headers, o));
    let mut child = c
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("curl: {e}"))?;
    let out = child.stdout.take().ok_or("curl: no output")?;
    let body = read_capped(out, o.cap);
    if body.is_err() {
        let _ = child.kill();
    }
    let out = child.wait_with_output().map_err(|e| format!("curl: {e}"))?;
    let body = body?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(format!("curl failed ({}): {}", out.status, err.trim()));
    }
    Ok(body)
}

/// What `request` got back: the status, a `Retry-After` in seconds when
/// the server sent one as a number, and the body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    pub status: u16,
    pub retry_after: Option<u64>,
    pub body: Vec<u8>,
}

/// After the body: a newline, the status and the `Retry-After` header.
const TRAILER: &str = "\n%{http_code}\n%header{retry-after}";

/// `args` without `-f`, so any status comes back, plus the trailer.
fn request_args(url: &str, headers: &[&str], o: &Opts) -> Vec<String> {
    let mut a = args(url, headers, o);
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

/// Requests `url` like `get`, with `config` (curl config-file lines: a
/// secret header, the method, a body; see `CurlConfig`) written to curl's
/// stdin and read with `-K -`, so it never appears in argv or the
/// environment. Any HTTP status is returned.
pub fn request(url: &str, headers: &[&str], config: &[u8], o: &Opts) -> Result<Response, String> {
    check_scheme(url, o.scheme)?;
    let mut c = curl(o.program.clone(), request_args(url, headers, o));
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
    let body = read_capped(out, o.cap + 64);
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

/// Builds curl config-file lines. The result is a `Secret`: it holds the
/// credential, and is wiped when dropped.
#[derive(Default)]
pub struct CurlConfig(Secret);

/// A value curl's config syntax cannot carry safely.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BadValue;

/// `"` + `v` with curl's config escapes + `"`; `None` for control
/// characters other than tab, line feed, carriage return and vertical tab.
fn quote(v: &[u8]) -> Option<Vec<u8>> {
    let mut q = Vec::with_capacity(v.len() + 8);
    q.push(b'"');
    for &b in v {
        match b {
            b'\\' => q.extend_from_slice(b"\\\\"),
            b'"' => q.extend_from_slice(b"\\\""),
            b'\t' => q.extend_from_slice(b"\\t"),
            b'\n' => q.extend_from_slice(b"\\n"),
            b'\r' => q.extend_from_slice(b"\\r"),
            0x0b => q.extend_from_slice(b"\\v"),
            0..=0x1f | 0x7f => return None,
            _ => q.push(b),
        }
    }
    q.push(b'"');
    Some(q)
}

impl CurlConfig {
    pub fn new() -> CurlConfig {
        CurlConfig(Secret::with_capacity(512))
    }

    fn line(&mut self, key: &str, value: &[u8]) -> Result<&mut Self, BadValue> {
        let mut q = quote(value).ok_or(BadValue)?;
        self.0.push(key.as_bytes());
        self.0.push(b" = ");
        self.0.push(&q);
        self.0.push(b"\n");
        crate::secret::wipe(&mut q);
        Ok(self)
    }

    /// `request = "<method>"`.
    pub fn method(&mut self, method: &str) -> Result<&mut Self, BadValue> {
        if !method.bytes().all(|b| b.is_ascii_uppercase()) || method.is_empty() {
            return Err(BadValue);
        }
        self.line("request", method.as_bytes())
    }

    /// `header = "<name>: <value>"`; neither may hold control characters.
    pub fn header(&mut self, name: &str, value: &[u8]) -> Result<&mut Self, BadValue> {
        if name.is_empty() || name.bytes().any(|b| b == b':' || b <= b' ' || b >= 0x7f) {
            return Err(BadValue);
        }
        let mut h = Secret::with_capacity(name.len() + 2 + value.len());
        h.push(name.as_bytes());
        h.push(b": ");
        h.push(value);
        self.line("header", h.as_bytes())
    }

    /// `data-binary = "<body>"`.
    pub fn body(&mut self, body: &[u8]) -> Result<&mut Self, BadValue> {
        self.line("data-binary", body)
    }

    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn o(max: u64, cap: usize) -> Opts {
        Opts {
            program: "curl".into(),
            ..Opts::new(max, cap)
        }
    }

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
        let mut op = o(0, 10);
        op.user_agent = "bar/1.2.3".into();
        let a = args("-K/etc/passwd", &["Accept: x"], &op);
        assert_eq!(a[0], "-q");
        assert_eq!(a[a.len() - 2..], ["--", "-K/etc/passwd"]);
        assert!(a.windows(2).any(|w| w == ["--max-time", "1"]));
        assert!(a.windows(2).any(|w| w == ["-H", "Accept: x"]));
        assert!(a.windows(2).any(|w| w == ["--proto", "=https"]));
        assert!(a.windows(2).any(|w| w == ["--user-agent", "bar/1.2.3"]));
        assert!(!a.iter().any(|x| x == "--noproxy"));
        let r = request_args("https://x", &[], &op);
        assert!(r.windows(2).any(|w| w == ["--user-agent", "bar/1.2.3"]));
    }

    #[test]
    fn plain_http_implies_noproxy() {
        let mut op = o(5, 10);
        op.scheme = Scheme::LoopbackHttp;
        assert!(!op.noproxy);
        let a = args("http://127.0.0.1:1", &[], &op);
        assert!(a.windows(2).any(|w| w == ["--noproxy", "*"]));
        let a = args("https://proxy.example.test", &[], &op);
        assert!(!a.iter().any(|x| x == "--noproxy"));
    }

    #[test]
    fn loopback_http_and_noproxy_shape_the_argv() {
        let mut op = o(5, 10);
        op.scheme = Scheme::LoopbackHttp;
        op.noproxy = true;
        let a = args("http://127.0.0.1:1", &[], &op);
        assert!(a.windows(2).any(|w| w == ["--proto", "=http,https"]));
        assert!(a.windows(2).any(|w| w == ["--proto-redir", "=https"]));
        assert!(a.windows(2).any(|w| w == ["--noproxy", "*"]));
    }

    #[test]
    fn plain_http_is_for_loopback_only() {
        assert!(check_scheme("http://127.0.0.1:8318/x", Scheme::LoopbackHttp).is_ok());
        assert!(check_scheme("http://localhost/x", Scheme::LoopbackHttp).is_ok());
        assert!(check_scheme("http://[::1]:1/x", Scheme::LoopbackHttp).is_ok());
        assert!(check_scheme("https://proxy.example/x", Scheme::LoopbackHttp).is_ok());
        assert!(check_scheme("http://proxy.example/x", Scheme::LoopbackHttp).is_err());
        assert!(check_scheme("http://127.0.0.1@evil.example/x", Scheme::LoopbackHttp).is_err());
        assert!(check_scheme("http://localhost.evil.example/x", Scheme::LoopbackHttp).is_err());
        assert!(check_scheme("ftp://127.0.0.1/", Scheme::LoopbackHttp).is_err());
    }

    #[test]
    fn request_reads_its_config_from_stdin() {
        let a = request_args("https://x", &["A: b"], &o(5, 10));
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

    #[test]
    fn config_lines_quote_values() {
        let mut c = CurlConfig::new();
        c.method("POST").unwrap();
        c.header("Authorization", b"Bearer a\"b\\c").unwrap();
        c.body(b"{\"k\":\"v\\n\"}").unwrap();
        let t = String::from_utf8(c.as_bytes().to_vec()).unwrap();
        assert_eq!(
            t,
            "request = \"POST\"\nheader = \"Authorization: Bearer a\\\"b\\\\c\"\n\
             data-binary = \"{\\\"k\\\":\\\"v\\\\n\\\"}\"\n"
        );
    }

    #[test]
    fn config_refuses_what_could_inject_a_line() {
        let mut c = CurlConfig::new();
        assert!(c.header("X", b"a\0b").is_err());
        assert!(c.header("X", b"a\x1bb").is_err());
        assert!(c.header("X: y", b"v").is_err());
        assert!(c.header("", b"v").is_err());
        assert!(c.method("po st").is_err());
        // a line feed is escaped, never emitted raw
        c.header("X", b"a\nb\nurl = \"http://evil\"").unwrap();
        assert_eq!(c.as_bytes().iter().filter(|&&b| b == b'\n').count(), 1);
    }
}
