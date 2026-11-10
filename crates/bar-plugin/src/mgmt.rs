//! One management request to a proxy: the key travels in curl's stdin
//! config, never argv or environment, and a call refuses an empty key (the
//! proxy counts a missing key as a failed attempt). The reply body comes
//! back as a `Secret`, wiped on drop, because a management listing can
//! carry credentials of its own.

use crate::http::{self, CurlConfig, Opts, Scheme};
use crate::secret::Secret;
use crate::url::Origin;

/// Why a management call gave no data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MErr {
    /// 401 or 403: missing or wrong key, remote management off, or the
    /// client banned. Never retried with the same key.
    Auth(u16),
    /// 404: the route does not exist (management disabled, or the item
    /// is not there).
    NotFound,
    /// Timeout, 5xx, bad JSON, over the size cap.
    Transient(String),
}

pub struct Client<'a> {
    /// The management base URL, no trailing slash.
    base: String,
    key: &'a Secret,
    loopback: bool,
    user_agent: String,
}

impl<'a> Client<'a> {
    pub fn new(base: &str, key: &'a Secret, user_agent: &str) -> Result<Client<'a>, String> {
        if key.as_bytes().is_empty() {
            return Err("empty key".into());
        }
        let origin = Origin::parse(base).ok_or("bad management URL")?;
        Ok(Client {
            base: base.trim_end_matches('/').into(),
            key,
            loopback: origin.is_loopback(),
            user_agent: user_agent.into(),
        })
    }

    pub fn base(&self) -> &str {
        &self.base
    }

    /// `method` on `<base>/<path>`; `body` is JSON. A 2xx reply's body.
    pub fn request(
        &self,
        method: &str,
        path: &str,
        body: Option<&str>,
        max_time: u64,
        cap: usize,
    ) -> Result<Secret, MErr> {
        let mut cfg = CurlConfig::new();
        let mut bearer = Secret::with_capacity(self.key.as_bytes().len() + 8);
        bearer.push(b"Bearer ");
        bearer.push(self.key.as_bytes());
        let bad = |_| MErr::Transient("bad request".into());
        cfg.header("Authorization", bearer.as_bytes())
            .map_err(bad)?;
        let mut headers = vec!["Accept: application/json"];
        if method != "GET" {
            cfg.method(method).map_err(bad)?;
        }
        if let Some(body) = body {
            cfg.body(body.as_bytes()).map_err(bad)?;
            headers.push("Content-Type: application/json");
        }
        let url = format!("{}/{path}", self.base);
        let opts = Opts {
            scheme: Scheme::LoopbackHttp,
            // a loopback proxy is talked to directly, whatever the
            // environment's proxy settings say
            noproxy: self.loopback,
            user_agent: self.user_agent.clone(),
            ..Opts::new(max_time, cap)
        };
        let r = http::request(&url, &headers, cfg.as_bytes(), &opts).map_err(MErr::Transient)?;
        match r.status {
            200..=299 => Ok(Secret::new(r.body)),
            401 | 403 => Err(MErr::Auth(r.status)),
            404 => Err(MErr::NotFound),
            s => Err(MErr::Transient(format!("status {s}"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_key_never_goes_out() {
        assert!(Client::new("http://127.0.0.1:1/x", &Secret::new(vec![]), "t").is_err());
    }
}
