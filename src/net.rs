//! The network: `curl` with a fixed argv, HTTPS only, rather than an HTTP
//! client dependency. Only background jobs and explicit commands call it,
//! never the render path. The transfer itself is `bar_plugin::http`.

use bar_plugin::http::{self, Opts, Scheme};

pub(crate) use bar_plugin::http::Response;

fn opts(program: std::path::PathBuf, max_time: u64, cap: usize) -> Opts {
    Opts {
        program,
        // say who is asking, rather than curl's own version string
        user_agent: concat!("bar/", env!("CARGO_PKG_VERSION")).into(),
        max_time,
        cap,
        scheme: Scheme::Https,
        noproxy: false,
    }
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
    http::get(
        url,
        headers,
        &opts(crate::platform::program("curl"), max_time, cap),
    )
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
    http::request(
        url,
        headers,
        config,
        &opts(crate::platform::system_program("curl"), max_time, cap),
    )
}
