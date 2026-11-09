//! Option `open`: an OSC 8 link that opens a directory in the file manager
//! (a `file://` URL naming this host) or in an editor's URL scheme.

use crate::options::Options;
use crate::style::link;
use std::fmt::Write;
use std::sync::OnceLock;

/// Values of option `open`; `none` (the default) adds no link.
pub const VALUES: [&str; 5] = ["none", "files", "vscode", "cursor", "zed"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Opener {
    Files,
    Vscode,
    Cursor,
    Zed,
}

/// The opener option `open` names; `None` for `none`, unset or unknown.
pub fn opener(opts: &Options) -> Option<Opener> {
    Some(match opts.get("open")? {
        "files" => Opener::Files,
        "vscode" => Opener::Vscode,
        "cursor" => Opener::Cursor,
        "zed" => Opener::Zed,
        _ => return None,
    })
}

/// A directory path split into what a URL needs: the host of a UNC path
/// (empty otherwise) and the path with `/` separators, starting with `/`.
fn url_path(path: &str, windows: bool) -> Option<(String, String)> {
    if !windows {
        return path
            .starts_with('/')
            .then(|| (String::new(), path.to_owned()));
    }
    let p = path.replace('\\', "/");
    // `\\?\` and `\\.\` (device namespace) prefixes: the path after them
    let p = match p.strip_prefix("//?/").or_else(|| p.strip_prefix("//./")) {
        Some(rest) => match rest.strip_prefix("UNC/") {
            Some(unc) => format!("//{unc}"),
            None => rest.to_owned(),
        },
        None => p,
    };
    let b = p.as_bytes();
    if b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':' {
        if b.len() > 2 && b[2] != b'/' {
            return None;
        }
        return Some((String::new(), format!("/{p}")));
    }
    let unc = p.strip_prefix("//")?;
    let (host, rest) = unc.split_once('/')?;
    (valid_host(host) && !rest.is_empty()).then(|| (host.to_owned(), format!("/{rest}")))
}

/// Host names go into the URL as they are: letters, digits, `-`, `.`, `_`,
/// not dots alone.
fn valid_host(h: &str) -> bool {
    h.bytes().any(|b| b != b'.')
        && h.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_'))
}

/// `path` percent-encoded as UTF-8: unreserved characters and `/` stay,
/// and the colon of a leading drive (`/C:`).
fn encode(path: &str) -> String {
    let drive = {
        let b = path.as_bytes();
        b.len() >= 3 && b[0] == b'/' && b[1].is_ascii_alphabetic() && b[2] == b':'
    };
    let mut out = String::with_capacity(path.len() + 8);
    for (i, b) in path.bytes().enumerate() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~' | b'/') {
            out.push(b as char);
        } else if drive && i == 2 {
            out.push(':');
        } else {
            let _ = write!(out, "%{b:02X}");
        }
    }
    out
}

/// The URL that opens directory `path` with `opener`: `file://<host>/...`,
/// `vscode://file/...`, `cursor://file/...` or `zed://file/...`, ending in
/// `/`. `host` names this machine in a `file://` URL (empty leaves it
/// out); `windows` reads `path` as a Windows path (drive or UNC). `None`
/// for a relative path, or a UNC path in an editor.
pub fn url(opener: Opener, path: &str, host: &str, windows: bool) -> Option<String> {
    let (unc, p) = url_path(path, windows)?;
    let mut p = encode(&p);
    if !p.ends_with('/') {
        p.push('/');
    }
    Some(match opener {
        Opener::Files => {
            let host = if unc.is_empty() { host } else { &unc };
            let host = if valid_host(host) { host } else { "" };
            format!("file://{host}{p}")
        }
        _ if !unc.is_empty() => return None,
        Opener::Vscode => format!("vscode://file{p}"),
        Opener::Cursor => format!("cursor://file{p}"),
        Opener::Zed => format!("zed://file{p}"),
    })
}

/// This machine's host name, looked up once per process.
fn local_host() -> &'static str {
    static HOST: OnceLock<String> = OnceLock::new();
    HOST.get_or_init(|| crate::platform::hostname().unwrap_or_default())
}

/// `text` linked to directory `path` per option `open`; as it is when
/// `open` is unset or `none`, or the path cannot be linked.
pub fn wrap(text: String, path: &str, opts: &Options) -> String {
    let Some(o) = opener(opts) else {
        return text;
    };
    let host = if o == Opener::Files { local_host() } else { "" };
    match url(o, path, host, cfg!(windows)) {
        Some(u) => link(&u, &text),
        None => text,
    }
}

/// As [`wrap`] for preview samples: a Unix `path`, no host name looked up.
pub fn sample(text: String, path: &str, opts: &Options) -> String {
    match opener(opts).and_then(|o| url(o, path, "", false)) {
        Some(u) => link(&u, &text),
        None => text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Opener::*;

    fn with_open(v: &str) -> Options {
        let mut o = Options::default();
        o.set("open", v);
        o
    }

    #[test]
    fn unix_paths_encode_spaces_and_unicode() {
        assert_eq!(
            url(Files, "/home/me/My Project", "box", false).as_deref(),
            Some("file://box/home/me/My%20Project/")
        );
        assert_eq!(
            url(Vscode, "/Users/me/café/日本", "", false).as_deref(),
            Some("vscode://file/Users/me/caf%C3%A9/%E6%97%A5%E6%9C%AC/")
        );
        assert_eq!(
            url(Cursor, "/a/b%c#d?e", "", false).as_deref(),
            Some("cursor://file/a/b%25c%23d%3Fe/")
        );
        assert_eq!(
            url(Zed, "/srv/x:12/", "", false).as_deref(),
            Some("zed://file/srv/x%3A12/")
        );
        assert_eq!(
            url(Files, "/", "box", false).as_deref(),
            Some("file://box/")
        );
    }

    #[test]
    fn unix_backslash_is_a_file_name_character() {
        assert_eq!(
            url(Files, r"/tmp/a\b", "", false).as_deref(),
            Some("file:///tmp/a%5Cb/")
        );
    }

    #[test]
    fn relative_paths_get_no_url() {
        assert_eq!(url(Files, "src/bar", "box", false), None);
        assert_eq!(url(Vscode, "", "", false), None);
        assert_eq!(url(Vscode, r"src\bar", "", true), None);
        assert_eq!(url(Vscode, "C:relative", "", true), None);
    }

    #[test]
    fn windows_device_paths() {
        assert_eq!(
            url(Vscode, r"\\.\C:\src\bar", "", true).as_deref(),
            Some("vscode://file/C:/src/bar/")
        );
        assert_eq!(
            url(Files, r"\\.\UNC\srv\share\d", "box", true).as_deref(),
            Some("file://srv/share/d/")
        );
        assert_eq!(url(Files, r"\\.\PhysicalDrive0", "box", true), None);
        assert_eq!(url(Files, r"\\.\pipe\x", "box", true), None);
    }

    #[test]
    fn hosts_of_only_dots_are_not_hosts() {
        assert_eq!(url(Files, r"\\..\share\d", "", true), None);
        assert_eq!(url(Files, r"\\.\share", "", true), None);
        assert_eq!(
            url(Files, "/tmp/x", "..", false).as_deref(),
            Some("file:///tmp/x/")
        );
        assert_eq!(
            url(Files, "/tmp/x", ".", false).as_deref(),
            Some("file:///tmp/x/")
        );
    }

    #[test]
    fn windows_drive_paths() {
        let p = r"C:\Users\Me\My Project";
        assert_eq!(
            url(Files, p, "DESKTOP-1", true).as_deref(),
            Some("file://DESKTOP-1/C:/Users/Me/My%20Project/")
        );
        assert_eq!(
            url(Vscode, p, "", true).as_deref(),
            Some("vscode://file/C:/Users/Me/My%20Project/")
        );
        assert_eq!(
            url(Zed, r"d:\", "", true).as_deref(),
            Some("zed://file/d:/")
        );
        assert_eq!(
            url(Cursor, r"\\?\C:\src\ü", "", true).as_deref(),
            Some("cursor://file/C:/src/%C3%BC/")
        );
    }

    #[test]
    fn windows_unc_paths_open_in_the_file_manager_only() {
        assert_eq!(
            url(Files, r"\\server\share\dir", "me", true).as_deref(),
            Some("file://server/share/dir/")
        );
        assert_eq!(
            url(Files, r"\\?\UNC\server\share", "me", true).as_deref(),
            Some("file://server/share/")
        );
        assert_eq!(url(Vscode, r"\\server\share\dir", "", true), None);
        assert_eq!(url(Files, r"\\bad host\share", "", true), None);
    }

    #[test]
    fn an_unusable_host_name_is_left_out() {
        assert_eq!(
            url(Files, "/x", "a b/c", false).as_deref(),
            Some("file:///x/")
        );
        assert_eq!(url(Files, "/x", "", false).as_deref(), Some("file:///x/"));
    }

    #[test]
    fn option_values() {
        let o = |v: &str| opener(&with_open(v));
        assert_eq!(o("files"), Some(Files));
        assert_eq!(o("vscode"), Some(Vscode));
        assert_eq!(o("cursor"), Some(Cursor));
        assert_eq!(o("zed"), Some(Zed));
        assert_eq!(o("none"), None);
        assert_eq!(o("finder"), None);
        assert_eq!(opener(&Options::default()), None);
    }

    #[test]
    fn wrap_without_open_is_unchanged() {
        assert_eq!(wrap("x".into(), "/a", &Options::default()), "x");
        let opts = with_open("vscode");
        assert_eq!(
            wrap("x".into(), "/a b", &opts),
            if cfg!(windows) {
                "x".to_string()
            } else {
                "\x1b]8;;vscode://file/a%20b/\x1b\\x\x1b]8;;\x1b\\".to_string()
            }
        );
    }
}
