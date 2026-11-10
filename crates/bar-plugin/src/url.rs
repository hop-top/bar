//! The part of a URL a proxy plugin needs: the normalised origin.

/// `scheme://host:port` with a lower-case scheme and host and an explicit
/// port; no user info, path, query or fragment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Origin {
    pub scheme: String,
    /// Host as written in a URL: IPv6 literals keep their brackets.
    pub host: String,
    pub port: u16,
}

impl Origin {
    /// `None` for anything but a plain `http`/`https` URL with a host:
    /// user info, an empty host, a bad port or control characters refuse.
    pub fn parse(url: &str) -> Option<Origin> {
        let url = url.trim();
        if url.bytes().any(|b| b.is_ascii_control() || b == b' ') {
            return None;
        }
        let (scheme, rest) = url.split_once("://")?;
        let scheme = scheme.to_ascii_lowercase();
        let default = match scheme.as_str() {
            "http" => 80,
            "https" => 443,
            _ => return None,
        };
        let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
        let auth = &rest[..end];
        if auth.contains('@') {
            return None;
        }
        let (host, port) = if let Some(v6) = auth.strip_prefix('[') {
            let (h, tail) = v6.split_once(']')?;
            let port = match tail {
                "" => default,
                t => t.strip_prefix(':')?.parse().ok()?,
            };
            (format!("[{}]", h.to_ascii_lowercase()), port)
        } else {
            match auth.rsplit_once(':') {
                Some((h, p)) => (h.to_ascii_lowercase(), p.parse().ok()?),
                None => (auth.to_ascii_lowercase(), default),
            }
        };
        let host_ok = !host.is_empty()
            && host
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-._:[]".contains(&b));
        (host_ok && port != 0).then_some(Origin { scheme, host, port })
    }

    pub fn is_loopback(&self) -> bool {
        is_loopback_host(&self.host)
    }
}

impl std::fmt::Display for Origin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}://{}:{}", self.scheme, self.host, self.port)
    }
}

/// `localhost`, `127.0.0.1` and `::1` (with or without brackets).
pub fn is_loopback_host(host: &str) -> bool {
    matches!(
        host.to_ascii_lowercase().as_str(),
        "localhost" | "127.0.0.1" | "::1" | "[::1]"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn norm(s: &str) -> Option<String> {
        Origin::parse(s).map(|o| o.to_string())
    }

    #[test]
    fn origin_is_normalised() {
        assert_eq!(
            norm("HTTP://Localhost:8318/v1/x?y#z").unwrap(),
            "http://localhost:8318"
        );
        assert_eq!(
            norm("http://127.0.0.1:8318/").unwrap(),
            "http://127.0.0.1:8318"
        );
        assert_eq!(norm("http://127.0.0.1").unwrap(), "http://127.0.0.1:80");
        assert_eq!(
            norm("https://Proxy.Example").unwrap(),
            "https://proxy.example:443"
        );
        assert_eq!(norm("http://[::1]:9/p").unwrap(), "http://[::1]:9");
        assert_eq!(norm("http://[::1]").unwrap(), "http://[::1]:80");
    }

    #[test]
    fn odd_urls_are_refused() {
        for bad in [
            "",
            "ftp://x",
            "http://",
            "http://u:p@host",
            "http://host:0",
            "http://host:x",
            "http://ho st",
            "host:80",
            "http://:80",
            "http://a\nb",
        ] {
            assert_eq!(norm(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn loopback_hosts() {
        for h in ["localhost", "LOCALHOST", "127.0.0.1", "::1", "[::1]"] {
            assert!(is_loopback_host(h), "{h}");
        }
        for h in ["127.0.0.2", "example.com", "localhost.evil.com", "0.0.0.0"] {
            assert!(!is_loopback_host(h), "{h}");
        }
    }
}
