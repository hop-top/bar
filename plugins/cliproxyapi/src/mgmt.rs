//! The proxy's management API over `bar_plugin::http`. The key travels in
//! curl's stdin config, never argv or environment. Calls refuse an empty
//! key (the proxy counts a missing key as a failed attempt).

use bar_plugin::secret::Secret;
use serde_json::Value;

/// Largest `credentials` listing read.
pub const CAP_LIST: usize = 256 * 1024;
/// Largest `api-call` wrapper read (the proxy does not bound upstream bodies).
pub const CAP_CALL: usize = 128 * 1024;
/// Time for the local listing.
pub const T_LIST: u64 = 3;
/// Time for an upstream call through the proxy.
pub const T_CALL: u64 = 8;
/// Largest reply to a field change read.
pub const CAP_PATCH: usize = 8 * 1024;
/// Time for a field change.
pub const T_PATCH: u64 = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Api {
    V8,
    V0,
}

impl Api {
    pub fn name(self) -> &'static str {
        match self {
            Api::V8 => "v8",
            Api::V0 => "v0",
        }
    }
    pub fn from_name(s: &str) -> Api {
        if s == "v0" {
            Api::V0
        } else {
            Api::V8
        }
    }
    fn list_path(self) -> &'static str {
        match self {
            Api::V8 => "credentials",
            Api::V0 => "auth-files",
        }
    }
    fn fields_path(self) -> &'static str {
        match self {
            Api::V8 => "credentials/fields",
            Api::V0 => "auth-files/fields",
        }
    }
    fn call_path(self) -> &'static str {
        match self {
            Api::V8 => "requests/api-call",
            Api::V0 => "api-call",
        }
    }
}

pub use bar_plugin::mgmt::MErr;

pub struct Client<'a> {
    /// `<origin>/v8/management` or `/v0/management`, no trailing slash.
    inner: bar_plugin::mgmt::Client<'a>,
}

impl<'a> Client<'a> {
    /// `base` must end in `/v8/management` or `/v0/management`.
    pub fn new(base: &str, key: &'a Secret) -> Result<Client<'a>, String> {
        let ua = format!("bar-cliproxyapi/{}", crate::VERSION);
        Ok(Client {
            inner: bar_plugin::mgmt::Client::new(base, key, &ua)?,
        })
    }

    /// The same client for the other API generation, when `base` names one.
    pub fn base_for(base: &str, api: Api) -> String {
        let (from, to) = match api {
            Api::V8 => ("/v0/management", "/v8/management"),
            Api::V0 => ("/v8/management", "/v0/management"),
        };
        match base.strip_suffix(from) {
            Some(head) => format!("{head}{to}"),
            None => base.to_string(),
        }
    }

    /// The API generation `base` names.
    pub fn api_of(base: &str) -> Api {
        if base.ends_with("/v0/management") {
            Api::V0
        } else {
            Api::V8
        }
    }

    fn send(
        &self,
        path: &str,
        post: Option<&str>,
        max_time: u64,
        cap: usize,
    ) -> Result<Value, MErr> {
        self.send_as(post.map(|_| "POST"), path, post, max_time, cap)
    }

    fn send_as(
        &self,
        method: Option<&str>,
        path: &str,
        post: Option<&str>,
        max_time: u64,
        cap: usize,
    ) -> Result<Value, MErr> {
        let body = self
            .inner
            .request(method.unwrap_or("GET"), path, post, max_time, cap)?;
        serde_json::from_slice(body.as_bytes())
            .map_err(|_| MErr::Transient("response is not JSON".into()))
    }

    /// The account listing.
    pub fn credentials(&self) -> Result<Value, MErr> {
        let api = Client::api_of(self.inner.base());
        self.send(api.list_path(), None, T_LIST, CAP_LIST)
    }

    /// Sets an account's routing priority (`name` is the proxy's own id of
    /// the credential, as the listing gives it). The proxy persists it with
    /// the credential; it changes only which account new selections prefer.
    pub fn set_priority(&self, name: &str, priority: i64) -> Result<(), MErr> {
        let body = serde_json::json!({ "name": name, "priority": priority }).to_string();
        let api = Client::api_of(self.inner.base());
        self.send_as(
            Some("PATCH"),
            api.fields_path(),
            Some(&body),
            T_PATCH,
            CAP_PATCH,
        )
        .map(drop)
    }

    /// One call through the proxy with the account's own token
    /// (`$TOKEN$` stands for it in `headers`). The URL is the caller's: the
    /// plugin passes compiled-in ones only. The proxy answers 200 with
    /// `{status_code, header, body}` whatever the upstream said.
    pub fn api_call(
        &self,
        auth_index: &str,
        method: &str,
        url: &str,
        headers: &[(&str, &str)],
    ) -> Result<ApiReply, MErr> {
        let h: serde_json::Map<String, Value> = headers
            .iter()
            .map(|(k, v)| (k.to_string(), Value::from(*v)))
            .collect();
        let body = serde_json::json!({
            "auth_index": auth_index, "method": method, "url": url, "header": h,
        })
        .to_string();
        let api = Client::api_of(self.inner.base());
        let v = self.send(api.call_path(), Some(&body), T_CALL, CAP_CALL)?;
        let status = v["status_code"]
            .as_u64()
            .ok_or_else(|| MErr::Transient("no status_code".into()))?;
        let retry_after = v["header"]
            .as_object()
            .and_then(|m| {
                m.iter()
                    .find(|(k, _)| k.eq_ignore_ascii_case("retry-after"))
            })
            .and_then(|(_, v)| v.get(0).and_then(Value::as_str).or(v.as_str()))
            .and_then(|s| s.trim().parse().ok());
        Ok(ApiReply {
            status: status as u16,
            retry_after,
            body: serde_json::from_str(v["body"].as_str().unwrap_or("null")).unwrap_or(Value::Null),
        })
    }
}

/// What an upstream call through the proxy returned.
#[derive(Debug, Clone, PartialEq)]
pub struct ApiReply {
    pub status: u16,
    pub retry_after: Option<u64>,
    pub body: Value,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_generation_follows_the_base() {
        let b = "http://127.0.0.1:1/v8/management";
        assert_eq!(Client::api_of(b), Api::V8);
        let v0 = Client::base_for(b, Api::V0);
        assert_eq!(v0, "http://127.0.0.1:1/v0/management");
        assert_eq!(Client::api_of(&v0), Api::V0);
        assert_eq!(Client::base_for(&v0, Api::V8), b);
    }

    #[test]
    fn an_empty_key_never_goes_out() {
        assert!(Client::new("http://127.0.0.1:1/v8/management", &Secret::new(vec![])).is_err());
    }
}
