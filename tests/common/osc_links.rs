//! OSC 8 links in rendered output: each `bar://` link's URL and text, and
//! the plain text with every escape removed.
#![allow(dead_code)]

const CLOSE: &str = "\x1b]8;;\x1b\\";

/// Each `bar://` link in `s`: its URL and the plain text inside.
pub fn links(s: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut rest = s;
    while let Some(i) = rest.find("\x1b]8;;bar://") {
        let after = &rest[i + "\x1b]8;;".len()..];
        let end = after.find("\x1b\\").unwrap();
        let body = &after[end + 2..];
        let close = body.find(CLOSE).unwrap();
        out.push((after[..end].to_string(), strip(&body[..close])));
        rest = &body[close + CLOSE.len()..];
    }
    out
}

pub fn strip(s: &str) -> String {
    let mut out = String::new();
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c == '\x1b' {
            // OSC runs to ST (ESC \), SGR to its `m`
            let end = if it.next() == Some(']') { '\\' } else { 'm' };
            for d in it.by_ref() {
                if d == end {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}
