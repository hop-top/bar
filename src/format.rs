//! `#{name[key=value,...]}` placeholder parsing and segment splitting.

use crate::options::Options;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Token {
    Text(String),
    Placeholder { name: String, options: Options },
}

pub fn parse(input: &str) -> Result<Vec<Token>, String> {
    let mut out = Vec::new();
    let mut text = String::new();
    let mut rest = input;
    while let Some(i) = rest.find("#{") {
        text.push_str(&rest[..i]);
        let body = &rest[i + 2..];
        let end = find_close(body).ok_or_else(|| format!("unterminated placeholder: {rest}"))?;
        if !text.is_empty() {
            out.push(Token::Text(std::mem::take(&mut text)));
        }
        out.push(parse_placeholder(&body[..end])?);
        rest = &body[end + 1..];
    }
    text.push_str(rest);
    if !text.is_empty() {
        out.push(Token::Text(text));
    }
    Ok(out)
}

/// Index of the closing `}`, ignoring any inside double quotes.
fn find_close(s: &str) -> Option<usize> {
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

fn parse_placeholder(body: &str) -> Result<Token, String> {
    let (name, opts) = match body.find('[') {
        Some(i) => {
            let inner = body[i + 1..]
                .strip_suffix(']')
                .ok_or_else(|| format!("missing ] in placeholder: {body}"))?;
            (&body[..i], inner)
        }
        None => (body, ""),
    };
    let name = name.trim();
    if name.is_empty() {
        return Err("empty placeholder name".into());
    }
    let mut options = Options::default();
    for part in split_unquoted(opts, ',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        match part.split_once('=') {
            Some((k, v)) => options.set(k.trim(), v.trim()),
            None => options.set(part, ""),
        }
    }
    Ok(Token::Placeholder {
        name: name.to_string(),
        options,
    })
}

fn split_unquoted(s: &str, sep: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut quoted = false;
    let mut start = 0;
    for (i, c) in s.char_indices() {
        if c == '"' {
            quoted = !quoted;
        } else if c == sep && !quoted {
            parts.push(&s[start..i]);
            start = i + c.len_utf8();
        }
    }
    parts.push(&s[start..]);
    parts
}

/// Split at every occurrence of `separator` found in Text tokens; text
/// next to a split loses its edge whitespace, empty pieces are dropped.
pub fn segments(tokens: &[Token], separator: &str) -> Vec<Vec<Token>> {
    let mut out: Vec<Vec<Token>> = vec![Vec::new()];
    for t in tokens {
        match t {
            Token::Text(s) if !separator.is_empty() && s.contains(separator) => {
                let pieces: Vec<&str> = s.split(separator).collect();
                let last = pieces.len() - 1;
                for (i, p) in pieces.iter().enumerate() {
                    let mut p = *p;
                    if i > 0 {
                        p = p.trim_start();
                    }
                    if i < last {
                        p = p.trim_end();
                    }
                    if !p.is_empty() {
                        out.last_mut().unwrap().push(Token::Text(p.to_string()));
                    }
                    if i < last {
                        out.push(Vec::new());
                    }
                }
            }
            other => out.last_mut().unwrap().push(other.clone()),
        }
    }
    out
}
