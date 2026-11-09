//! The config file as text, edited in place: a change rewrites only the
//! value of the line it touches, or appends one line, so comments, blank
//! lines, line endings and lines the editor does not know stay as they are.

use std::ops::Range;

/// A token of a config line: its value as `conf` reads it and the bytes it
/// spans in the line (quotes included).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tok {
    pub value: String,
    pub span: Range<usize>,
    pub quoted: bool,
}

/// Same rules as the config tokenizer (whitespace splits, double quotes
/// group, `#` outside quotes ends the line), keeping byte spans. `None` on
/// an unterminated quote.
pub fn tokens(line: &str) -> Option<Vec<Tok>> {
    let mut out = Vec::new();
    let mut cur = String::new();
    // start and end of the token being read, and whether it had quotes
    let mut tok: Option<(usize, usize, bool)> = None;
    let mut in_q = false;
    for (i, ch) in line.char_indices() {
        let end = i + ch.len_utf8();
        match ch {
            '"' => {
                in_q = !in_q;
                let t = tok.get_or_insert((i, end, true));
                t.1 = end;
                t.2 = true;
            }
            '#' if !in_q => break,
            c if c.is_whitespace() && !in_q => {
                if let Some((s, e, quoted)) = tok.take() {
                    out.push(Tok {
                        value: std::mem::take(&mut cur),
                        span: s..e,
                        quoted,
                    });
                }
            }
            c => {
                cur.push(c);
                tok.get_or_insert((i, end, false)).1 = end;
            }
        }
    }
    if in_q {
        return None;
    }
    if let Some((s, e, quoted)) = tok {
        out.push(Tok {
            value: cur,
            span: s..e,
            quoted,
        });
    }
    Some(out)
}

/// `value` written as one config token: bare when it is a plain word and
/// `bare` allows it, else in double quotes.
pub fn quote(value: &str, bare: bool) -> String {
    let plain = !value.is_empty() && !value.contains(|c: char| c.is_whitespace() || c == '#');
    if bare && plain {
        value.to_string()
    } else {
        format!("\"{value}\"")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Doc {
    lines: Vec<String>,
    eol: &'static str,
}

impl Doc {
    pub fn new(text: &str) -> Doc {
        let eol = if text.contains("\r\n") { "\r\n" } else { "\n" };
        Doc {
            lines: text.split_inclusive('\n').map(str::to_string).collect(),
            eol,
        }
    }

    pub fn text(&self) -> String {
        self.lines.concat()
    }

    /// Tokens of every `set <key> <value>` line (exactly one value), with
    /// the line index.
    fn sets(&self) -> impl Iterator<Item = (usize, Vec<Tok>)> + '_ {
        self.lines.iter().enumerate().filter_map(|(i, l)| {
            let t = tokens(l)?;
            (t.len() == 3 && t[0].value == "set").then_some((i, t))
        })
    }

    /// The last line setting a key `matches` accepts: the one that wins.
    fn last_set(&self, matches: &dyn Fn(&str) -> bool) -> Option<(usize, Vec<Tok>)> {
        self.sets().filter(|(_, t)| matches(&t[1].value)).last()
    }

    /// The value of the winning `set` line for a matching key.
    pub fn get(&self, matches: &dyn Fn(&str) -> bool) -> Option<String> {
        self.last_set(matches).map(|(_, t)| t[2].value.clone())
    }

    /// Rewrites the value of the winning line for a matching key, keeping
    /// its quoting style and everything around it; with no such line, one
    /// `set <key> <value>` line is appended. False when nothing changed.
    pub fn set(&mut self, matches: &dyn Fn(&str) -> bool, key: &str, value: &str) -> bool {
        let before = self.lines.clone();
        match self.last_set(matches) {
            Some((i, t)) => {
                let v = &t[2];
                let line = &mut self.lines[i];
                line.replace_range(v.span.clone(), &quote(value, !v.quoted));
            }
            None => {
                if let Some(last) = self.lines.last_mut() {
                    if !last.ends_with('\n') {
                        last.push_str(self.eol);
                    }
                }
                let bare = !value.contains("#{");
                self.lines
                    .push(format!("set {key} {}{}", quote(value, bare), self.eol));
            }
        }
        self.lines != before
    }

    /// Drops every `set` line for a matching key. False when there was none.
    pub fn remove(&mut self, matches: &dyn Fn(&str) -> bool) -> bool {
        let gone: Vec<usize> = self
            .sets()
            .filter(|(_, t)| matches(&t[1].value))
            .map(|(i, _)| i)
            .collect();
        for i in gone.iter().rev() {
            self.lines.remove(*i);
        }
        !gone.is_empty()
    }
}
