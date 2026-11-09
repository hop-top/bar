//! Repository config: the `remote` and `branch` sections, parsed the way
//! git reads them for the forms this parser models. Includes, old-style
//! `[remote.x]` / `[branch.x]` headers, `extensions` and line
//! continuations are `Unsupported`; `url.*` sections are noted, since they
//! rewrite remote URLs.

use super::Unsupported;

/// One `key = value` line.
#[derive(Debug)]
struct Entry<'a> {
    /// Lower case.
    section: String,
    /// Case kept, quotes and escapes removed.
    sub: Option<String>,
    /// Lower case.
    key: String,
    /// As written after `=`; decoded on use.
    raw: &'a str,
}

#[derive(Debug)]
pub(super) struct Config<'a> {
    entries: Vec<Entry<'a>>,
    /// A `[url ...]` section: remote URLs may be rewritten (`insteadOf`).
    pub rewrites: bool,
}

/// `"name"` with git's subsection escapes (`\"`, `\\`, any other `\x` is
/// `x`); `None` when it is not one quoted string.
fn subsection(s: &str) -> Option<String> {
    let inner = s.trim().strip_prefix('"')?.strip_suffix('"')?;
    let mut out = String::new();
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => out.push(chars.next()?),
            '"' => return None,
            c => out.push(c),
        }
    }
    Some(out)
}

/// A line that ends in a backslash git reads as a line continuation: not
/// an escaped one (`\\`), not inside a comment.
fn continues(line: &str) -> bool {
    let mut quoted = false;
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                if chars.next().is_none() {
                    return true;
                }
            }
            '"' => quoted = !quoted,
            '#' | ';' if !quoted => return false,
            _ => {}
        }
    }
    false
}

/// `body` with each continued line joined to the next (backslash and line
/// break dropped), for scanning a config whose values may span lines.
pub(super) fn join_continued(body: &str) -> String {
    let mut out = String::with_capacity(body.len());
    for line in body.lines() {
        if continues(line) {
            out.push_str(&line[..line.len() - 1]);
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

/// Byte offset of the `]` closing a section header (`rest` follows the
/// `[`), skipping any inside the quoted subsection name.
fn header_end(rest: &str) -> Option<usize> {
    let mut quoted = false;
    let mut chars = rest.char_indices();
    while let Some((i, c)) = chars.next() {
        match c {
            '\\' if quoted => {
                chars.next()?;
            }
            '"' => quoted = !quoted,
            ']' if !quoted => return Some(i),
            _ => {}
        }
    }
    None
}

pub(super) fn parse(body: &str) -> Result<Config<'_>, Unsupported> {
    let mut cfg = Config {
        entries: Vec::new(),
        rewrites: false,
    };
    let (mut section, mut sub) = (String::new(), None::<String>);
    for raw in body.lines() {
        if continues(raw) {
            return Err(Unsupported);
        }
        let mut line = raw.trim();
        while let Some(rest) = line.strip_prefix('[') {
            let end = header_end(rest).ok_or(Unsupported)?;
            let (head, tail) = (&rest[..end], &rest[end + 1..]);
            let lower = head.to_ascii_lowercase();
            if lower.starts_with("include")
                || lower.starts_with("remote.")
                || lower.starts_with("branch.")
                || lower.starts_with("extensions")
            {
                return Err(Unsupported);
            }
            if lower.starts_with("url") {
                cfg.rewrites = true;
            }
            (section, sub) = match head.split_once(char::is_whitespace) {
                Some((s, q)) => (s.to_ascii_lowercase(), subsection(q)),
                None => (lower, None),
            };
            line = tail.trim();
        }
        if line.is_empty() || line.starts_with(['#', ';']) {
            continue;
        }
        let Some((key, val)) = line.split_once('=') else {
            continue;
        };
        cfg.entries.push(Entry {
            section: section.clone(),
            sub: sub.clone(),
            key: key.trim().to_ascii_lowercase(),
            raw: val,
        });
    }
    Ok(cfg)
}

impl Config<'_> {
    /// Any entry in `section` (in a subsection only, with `sub`).
    pub fn has_entries(&self, section: &str, sub: bool) -> bool {
        self.entries
            .iter()
            .any(|e| e.section == section && (!sub || e.sub.is_some()))
    }

    /// Values of `section "sub".key`, in file order, decoded.
    fn values<'s>(
        &'s self,
        section: &'s str,
        sub: &'s str,
        key: &'s str,
    ) -> impl Iterator<Item = Result<String, Unsupported>> + 's {
        self.entries
            .iter()
            .filter(move |e| e.section == section && e.sub.as_deref() == Some(sub) && e.key == key)
            .map(|e| value(e.raw).ok_or(Unsupported))
    }

    /// First value (multi-valued keys: `url`, `merge`, `fetch`).
    pub fn first(
        &self,
        section: &str,
        sub: &str,
        key: &str,
    ) -> Result<Option<String>, Unsupported> {
        self.values(section, sub, key).next().transpose()
    }

    /// Last value (single-valued keys: the last one wins).
    pub fn last(&self, section: &str, sub: &str, key: &str) -> Result<Option<String>, Unsupported> {
        let mut out = None;
        for v in self.values(section, sub, key) {
            out = Some(v?);
        }
        Ok(out)
    }

    /// Every value, in file order.
    pub fn all(&self, section: &str, sub: &str, key: &str) -> Result<Vec<String>, Unsupported> {
        self.values(section, sub, key).collect()
    }
}

/// Config value: trimmed (whitespace inside quotes kept), quotes removed, `#`/`;` outside quotes start a
/// comment, `\\ \" \n \t \b` escapes.
fn value(v: &str) -> Option<String> {
    let mut out = String::new();
    // length up to the last character that is kept at the end: anything
    // but whitespace outside quotes
    let mut keep = 0;
    let mut quoted = false;
    let mut chars = v.trim().chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => quoted = !quoted,
            '\\' => {
                out.push(match chars.next()? {
                    'n' => '\n',
                    't' => '\t',
                    'b' => '\u{8}',
                    c @ ('\\' | '"') => c,
                    _ => return None,
                });
                keep = out.len();
            }
            '#' | ';' if !quoted => break,
            c => {
                out.push(c);
                if quoted || !c.is_whitespace() {
                    keep = out.len();
                }
            }
        }
    }
    out.truncate(keep);
    (!quoted).then_some(out)
}

/// Where a fetch refspec maps `src` (a full ref name), as git's
/// `remote_find_tracking` does: the first refspec that matches wins; a
/// refspec without a destination tracks nothing. Negative refspecs are
/// `Unsupported`.
pub(super) fn track(refspecs: &[String], src: &str) -> Result<Option<String>, Unsupported> {
    for spec in refspecs {
        let spec = spec.strip_prefix('+').unwrap_or(spec);
        if spec.starts_with('^') {
            return Err(Unsupported);
        }
        let Some((from, to)) = spec.split_once(':') else {
            continue;
        };
        if to.is_empty() {
            continue;
        }
        match (from.split_once('*'), to.split_once('*')) {
            (None, None) if from == src => return Ok(Some(to.to_string())),
            (Some((pre, suf)), Some((tpre, tsuf))) => {
                if suf.contains('*') || tsuf.contains('*') {
                    return Err(Unsupported);
                }
                if src.len() >= pre.len() + suf.len() && src.starts_with(pre) && src.ends_with(suf)
                {
                    let mid = &src[pre.len()..src.len() - suf.len()];
                    return Ok(Some(format!("{tpre}{mid}{tsuf}")));
                }
            }
            (None, None) => {}
            _ => return Err(Unsupported),
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sections_keys_and_last_wins() {
        let c = parse(
            "[Branch \"Feat/X\"]\n\tRemote = a\n\tremote = \"b\" ; c\n\tmerge = refs/heads/m\n\
             \tmerge = refs/heads/n\n[branch \"feat/x\"]\n\tremote = lower\n",
        )
        .unwrap();
        assert_eq!(c.last("branch", "Feat/X", "remote"), Ok(Some("b".into())));
        assert_eq!(
            c.first("branch", "Feat/X", "merge"),
            Ok(Some("refs/heads/m".into()))
        );
        assert_eq!(
            c.last("branch", "feat/x", "remote"),
            Ok(Some("lower".into()))
        );
        assert_eq!(c.last("branch", "nope", "remote"), Ok(None));
        assert!(!c.rewrites);
    }

    #[test]
    fn escaped_subsection() {
        let c = parse("[remote \"a\\\"b\\\\c\"]\n url = u\n").unwrap();
        assert_eq!(c.first("remote", "a\"b\\c", "url"), Ok(Some("u".into())));
    }

    #[test]
    fn unsupported_forms() {
        for body in [
            "[include]\npath = x\n",
            "[includeIf \"gitdir:/x\"]\npath = x\n",
            "[remote.origin]\nurl = x\n",
            "[branch.main]\nremote = x\n",
            "[extensions]\nworktreeConfig = true\n",
            "[remote \"o\"]\nurl = a\\\n b\n",
            "[remote \"o\"\nurl = x\n",
        ] {
            assert!(parse(body).is_err(), "{body:?}");
        }
        assert!(
            parse("[url \"https://m/\"]\ninsteadOf = gh:\n")
                .unwrap()
                .rewrites
        );
    }

    #[test]
    fn bracket_inside_a_quoted_subsection() {
        let c = parse("[remote \"a]b\"]\n url = u\n").unwrap();
        assert_eq!(c.first("remote", "a]b", "url"), Ok(Some("u".into())));
        let c = parse("[remote \"a\\\"]b\"] url = v\n").unwrap();
        assert_eq!(c.first("remote", "a\"]b", "url"), Ok(Some("v".into())));
        assert!(parse("[remote \"a]b\n url = u\n").is_err());
    }

    #[test]
    fn line_end_backslash_that_does_not_continue() {
        let c = parse(
            "[remote \"o\"]\n url = C:\\\\\n # a comment \\\n pushurl = p ; note \\\n fetch = f\n",
        )
        .unwrap();
        assert_eq!(c.first("remote", "o", "url"), Ok(Some("C:\\".into())));
        assert_eq!(c.first("remote", "o", "pushurl"), Ok(Some("p".into())));
        assert_eq!(c.first("remote", "o", "fetch"), Ok(Some("f".into())));
        assert!(parse("[remote \"o\"]\n url = a\\\\\\\n b\n").is_err());
        assert!(parse("[remote \"o\"]\n url = \"a ; b\\\n c\"\n").is_err());
    }

    #[test]
    fn spaces_inside_quotes_are_kept() {
        let c =
            parse("[branch \"m\"]\n a = \"x  \"  \n b = y  \n c = \" \"z\n d = x\\t\n").unwrap();
        assert_eq!(c.last("branch", "m", "a"), Ok(Some("x  ".into())));
        assert_eq!(c.last("branch", "m", "b"), Ok(Some("y".into())));
        assert_eq!(c.last("branch", "m", "c"), Ok(Some(" z".into())));
        assert_eq!(c.last("branch", "m", "d"), Ok(Some("x\t".into())));
    }

    #[test]
    fn refspec_mapping() {
        let s = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let std = s(&["+refs/heads/*:refs/remotes/origin/*"]);
        assert_eq!(
            track(&std, "refs/heads/feat/x"),
            Ok(Some("refs/remotes/origin/feat/x".into()))
        );
        assert_eq!(track(&std, "refs/tags/v1"), Ok(None));
        let exact = s(&[
            "refs/heads/main:refs/remotes/o/trunk",
            "refs/heads/*:refs/x/*",
        ]);
        assert_eq!(
            track(&exact, "refs/heads/main"),
            Ok(Some("refs/remotes/o/trunk".into()))
        );
        assert_eq!(
            track(&exact, "refs/heads/dev"),
            Ok(Some("refs/x/dev".into()))
        );
        assert_eq!(track(&s(&["refs/heads/main"]), "refs/heads/main"), Ok(None));
        assert_eq!(
            track(&s(&["^refs/heads/x"]), "refs/heads/main"),
            Err(Unsupported)
        );
        let mid = s(&["refs/heads/pre-*-suf:refs/remotes/o/*"]);
        assert_eq!(
            track(&mid, "refs/heads/pre-a-suf"),
            Ok(Some("refs/remotes/o/a".into()))
        );
        assert_eq!(track(&mid, "refs/heads/pre-suf"), Ok(None));
    }
}
