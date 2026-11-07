//! Label logic: parse `&` / `|` / `!` / parentheses / quotes and compile to
//! GitHub search queries via disjunctive normal form. Pure; no I/O.
//!
//! `&` binds tighter than `|`. Each DNF conjunct becomes one query; the
//! caller runs the queries and merges the results.

/// Upper bound on DNF conjuncts, so `(a|b)&(c|d)&...` cannot explode.
pub const MAX_CONJUNCTS: usize = 64;
/// Nesting limit for `(` and `!`, so hostile input cannot overflow the stack.
const MAX_DEPTH: usize = 128;

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Atom(String),
    Not(Box<Expr>),
    And(Vec<Expr>),
    Or(Vec<Expr>),
}

#[derive(Debug, PartialEq)]
enum Tok {
    Or,
    And,
    Not,
    Open,
    Close,
    Val(String),
}

fn lex(s: &str) -> Result<Vec<Tok>, String> {
    let mut out = Vec::new();
    let mut it = s.char_indices().peekable();
    while let Some(&(i, c)) = it.peek() {
        match c {
            c if c.is_whitespace() => {
                it.next();
            }
            '|' | '&' | '!' | '(' | ')' => {
                it.next();
                out.push(match c {
                    '|' => Tok::Or,
                    '&' => Tok::And,
                    '!' => Tok::Not,
                    '(' => Tok::Open,
                    _ => Tok::Close,
                });
            }
            '"' => {
                it.next();
                let start = i + 1;
                let end = loop {
                    match it.next() {
                        Some((j, '"')) => break j,
                        Some(_) => {}
                        None => return Err("unterminated quote".into()),
                    }
                };
                out.push(Tok::Val(s[start..end].trim().to_string()));
            }
            _ => {
                let mut end = i;
                while let Some(&(j, c)) = it.peek() {
                    if matches!(c, '|' | '&' | '!' | '(' | ')' | '"') {
                        break;
                    }
                    end = j + c.len_utf8();
                    it.next();
                }
                out.push(Tok::Val(s[i..end].trim_end().to_string()));
            }
        }
    }
    Ok(out)
}

struct Parser {
    toks: Vec<Tok>,
    pos: usize,
    depth: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }

    fn or(&mut self) -> Result<Expr, String> {
        let mut v = vec![self.and()?];
        while self.peek() == Some(&Tok::Or) {
            self.pos += 1;
            v.push(self.and()?);
        }
        Ok(if v.len() == 1 {
            v.remove(0)
        } else {
            Expr::Or(v)
        })
    }

    fn and(&mut self) -> Result<Expr, String> {
        let mut v = vec![self.unary()?];
        while self.peek() == Some(&Tok::And) {
            self.pos += 1;
            v.push(self.unary()?);
        }
        Ok(if v.len() == 1 {
            v.remove(0)
        } else {
            Expr::And(v)
        })
    }

    fn unary(&mut self) -> Result<Expr, String> {
        if self.depth >= MAX_DEPTH {
            return Err("expression nested too deeply".into());
        }
        self.depth += 1;
        let r = self.unary_inner();
        self.depth -= 1;
        r
    }

    fn unary_inner(&mut self) -> Result<Expr, String> {
        match self.toks.get(self.pos) {
            Some(Tok::Not) => {
                self.pos += 1;
                Ok(Expr::Not(Box::new(self.unary()?)))
            }
            Some(Tok::Open) => {
                self.pos += 1;
                let e = self.or()?;
                if self.peek() != Some(&Tok::Close) {
                    return Err("unbalanced parenthesis: missing ')'".into());
                }
                self.pos += 1;
                Ok(e)
            }
            Some(Tok::Val(v)) if v.is_empty() => Err("empty value".into()),
            Some(Tok::Val(v)) => {
                let e = Expr::Atom(v.clone());
                self.pos += 1;
                Ok(e)
            }
            Some(Tok::Close) => Err("unexpected ')'".into()),
            Some(_) => Err("expected a value".into()),
            None => Err("empty value: expression ends early".into()),
        }
    }
}

pub fn parse(s: &str) -> Result<Expr, String> {
    let mut p = Parser {
        toks: lex(s)?,
        pos: 0,
        depth: 0,
    };
    let e = p.or()?;
    match p.peek() {
        None => {}
        Some(Tok::Close) => return Err("unbalanced parenthesis: unexpected ')'".into()),
        Some(_) => return Err("expected '&' or '|' between values".into()),
    }
    // Worst case (every OR split): bounds every key's query count.
    if count(&e, false) > MAX_CONJUNCTS {
        return Err(format!(
            "expression expands to more than {MAX_CONJUNCTS} queries"
        ));
    }
    Ok(e)
}

/// DNF conjunct count with negation pushed inward; saturating.
fn count(e: &Expr, neg: bool) -> usize {
    match (e, neg) {
        (Expr::Atom(_), _) => 1,
        (Expr::Not(x), n) => count(x, !n),
        (Expr::And(v), false) | (Expr::Or(v), true) => v
            .iter()
            .fold(1usize, |a, x| a.saturating_mul(count(x, neg))),
        (Expr::Or(v), false) | (Expr::And(v), true) => v
            .iter()
            .fold(0usize, |a, x| a.saturating_add(count(x, neg))),
    }
}

/// Negation-normal form; positive-atom OR groups kept whole for `label`.
enum Node {
    Lit(bool, String),
    /// Positive atoms joined by OR; renders `key:a,b`.
    Group(Vec<String>),
    And(Vec<Node>),
    Or(Vec<Node>),
}

fn nnf(e: &Expr, neg: bool, group: bool) -> Node {
    match (e, neg) {
        (Expr::Atom(v), n) => Node::Lit(n, v.clone()),
        (Expr::Not(x), n) => nnf(x, !n, group),
        (Expr::And(v), false) | (Expr::Or(v), true) => {
            Node::And(v.iter().map(|x| nnf(x, neg, group)).collect())
        }
        (Expr::Or(v), false) | (Expr::And(v), true) => {
            let kids: Vec<Node> = v.iter().map(|x| nnf(x, neg, group)).collect();
            let kids = kids.into_iter().flat_map(flatten_or).collect::<Vec<_>>();
            if group && kids.iter().all(|k| matches!(k, Node::Lit(false, _))) {
                Node::Group(
                    kids.into_iter()
                        .filter_map(|k| match k {
                            Node::Lit(_, v) => Some(v),
                            _ => None,
                        })
                        .collect(),
                )
            } else {
                Node::Or(kids)
            }
        }
    }
}

fn flatten_or(n: Node) -> Vec<Node> {
    match n {
        Node::Or(v) => v,
        Node::Group(v) => v.into_iter().map(|s| Node::Lit(false, s)).collect(),
        other => vec![other],
    }
}

#[derive(Clone)]
enum Term {
    Lit(bool, String),
    Group(Vec<String>),
}

fn dnf(n: &Node) -> Vec<Vec<Term>> {
    match n {
        Node::Lit(neg, v) => vec![vec![Term::Lit(*neg, v.clone())]],
        Node::Group(v) => vec![vec![Term::Group(v.clone())]],
        Node::Or(v) => v.iter().flat_map(dnf).collect(),
        Node::And(v) => v.iter().fold(vec![vec![]], |acc, x| {
            let rhs = dnf(x);
            acc.iter()
                .flat_map(|a| {
                    rhs.iter().map(move |b| {
                        let mut c = a.clone();
                        c.extend(b.iter().cloned());
                        c
                    })
                })
                .collect()
        }),
    }
}

fn value(v: &str) -> String {
    if v.contains(|c: char| c.is_whitespace() || c == ',') {
        format!("\"{v}\"")
    } else {
        v.to_string()
    }
}

fn render(key: &str, t: &Term) -> String {
    match t {
        Term::Lit(neg, v) => format!("{}{key}:{}", if *neg { "-" } else { "" }, value(v)),
        Term::Group(g) => format!(
            "{key}:{}",
            dedup(g.iter().map(|v| value(v)).collect()).join(",")
        ),
    }
}

fn dedup(v: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    v.into_iter().filter(|s| seen.insert(s.clone())).collect()
}

/// One query per DNF conjunct. Only `label` collapses OR into `label:a,b`;
/// other keys split, since repeated `author:` terms match nothing.
pub fn to_queries(key: &str, e: &Expr) -> Vec<String> {
    let node = nnf(e, false, key == "label");
    dedup(
        dnf(&node)
            .iter()
            .map(|conj| dedup(conj.iter().map(|t| render(key, t)).collect()).join(" "))
            .collect(),
    )
}

/// Cartesian product of per-key query lists, space-joined, order-stable,
/// deduplicated. Empty lists (key unset) are skipped. Errors when the
/// product would exceed `MAX_CONJUNCTS`, checked before building.
pub fn combine(parts: &[Vec<String>]) -> Result<Vec<String>, String> {
    let mut acc: Vec<String> = Vec::new();
    for p in parts.iter().filter(|p| !p.is_empty()) {
        if acc.is_empty() {
            acc = p.clone();
        } else {
            if acc.len().saturating_mul(p.len()) > MAX_CONJUNCTS {
                return Err(format!("queries expand to more than {MAX_CONJUNCTS}"));
            }
            acc = acc
                .iter()
                .flat_map(|a| p.iter().map(move |b| format!("{a} {b}")))
                .collect();
        }
        if acc.len() > MAX_CONJUNCTS {
            return Err(format!("queries expand to more than {MAX_CONJUNCTS}"));
        }
        acc = dedup(acc);
    }
    Ok(acc)
}
