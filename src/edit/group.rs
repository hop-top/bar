//! One layout group (`set left "..."`) as segments of raw text, and the
//! placeholders inside a segment, so edits touch only what they change.

use crate::format::{self, Token};
use crate::options::Options;
use std::ops::Range;

/// Index of the `}` closing a placeholder body, ignoring any in quotes.
fn close(s: &str) -> Option<usize> {
    let mut quoted = false;
    for (i, c) in s.char_indices() {
        match c {
            '"' => quoted = !quoted,
            '}' if !quoted => return Some(i),
            _ => {}
        }
    }
    None
}

/// Byte ranges of every `#{...}` in `src`, braces included. An unclosed
/// one runs to the end.
pub fn placeholder_spans(src: &str) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(i) = src[at..].find("#{") {
        let start = at + i;
        let body = start + 2;
        let end = close(&src[body..]).map_or(src.len(), |e| body + e + 1);
        out.push(start..end);
        at = end;
    }
    out
}

/// The segments of a group: split at the trimmed `separator` outside
/// placeholders, each trimmed; empty ones dropped (as the renderer does).
pub fn split(src: &str, separator: &str) -> Vec<String> {
    let sep = separator.trim();
    let spans = placeholder_spans(src);
    let mut out = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while i < src.len() {
        if let Some(sp) = spans.iter().find(|s| s.start == i) {
            i = sp.end;
            continue;
        }
        if !sep.is_empty() && src[i..].starts_with(sep) {
            out.push(src[start..i].trim().to_string());
            i += sep.len();
            start = i;
            continue;
        }
        i += src[i..].chars().next().map_or(1, char::len_utf8);
    }
    out.push(src[start..].trim().to_string());
    out.retain(|s| !s.is_empty());
    out
}

/// Segments joined back with ` <separator> `.
pub fn join(segs: &[String], separator: &str) -> String {
    segs.join(&format!(" {} ", separator.trim()))
}

/// A placeholder of a segment: where it sits, its name and inline options.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ph {
    pub span: Range<usize>,
    pub name: String,
    pub options: Options,
}

/// Placeholders of `seg` that parse, in order.
pub fn placeholders(seg: &str) -> Vec<Ph> {
    placeholder_spans(seg)
        .into_iter()
        .filter_map(
            |span| match format::parse(&seg[span.clone()]).ok()?.pop()? {
                Token::Placeholder { name, options } => Some(Ph {
                    span,
                    name,
                    options,
                }),
                Token::Text(_) => None,
            },
        )
        .collect()
}

/// Why `value` cannot be written as option `key`: the config file has no
/// way to escape a double quote, and `;`, `]` and `}` end a value (a list
/// separates its items with `,`).
pub fn value_error(key: &str, value: &str) -> Option<String> {
    if value.contains('"') {
        return Some("a value cannot hold \"".into());
    }
    if key.is_empty() || key.contains(|c: char| "=;[]{}\" ".contains(c)) {
        return Some(format!("invalid option name `{key}`"));
    }
    let bad: &[char] = &[';', ']', '}'];
    value.contains(bad).then(|| {
        format!(
            "`{key}` cannot hold {}",
            bad.iter()
                .map(|c| format!("`{c}`"))
                .collect::<Vec<_>>()
                .join(" ")
        )
    })
}

/// `#{name}` or `#{name[k=v;...]}`; an empty value is written as the bare key.
pub fn write_placeholder(name: &str, options: &Options) -> String {
    if options.is_empty() {
        return format!("#{{{name}}}");
    }
    let mut parts: Vec<String> = Vec::new();
    for (k, v) in options.iter() {
        parts.push(if v.is_empty() {
            k.clone()
        } else {
            format!("{k}={v}")
        });
    }
    format!("#{{{name}[{}]}}", parts.join(";"))
}

/// `seg` with its placeholder at `ph` given new options.
pub fn with_options(seg: &str, ph: &Ph, options: &Options) -> String {
    let mut out = seg.to_string();
    out.replace_range(ph.span.clone(), &write_placeholder(&ph.name, options));
    out
}

/// `opts` with `key` set to `value`, or removed for `None`.
pub fn set_option(opts: &Options, key: &str, value: Option<&str>) -> Options {
    let mut out = Options::default();
    for (k, v) in opts.iter() {
        if k != key {
            out.set(k, v);
        } else if let Some(v) = value {
            out.set(k, v);
        }
    }
    if let (Some(v), None) = (value, opts.get(key)) {
        out.set(key, v);
    }
    out
}
