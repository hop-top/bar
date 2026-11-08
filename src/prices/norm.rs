//! Model-id normaliser: the fixed forms a gateway or cloud wraps around a
//! vendor's model id. Nothing here guesses; every step is one named form.

/// One spelling to try, with the vendor a prefix named, if any.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cand {
    pub id: String,
    pub vendor: Option<String>,
}

/// Bedrock cross-region inference prefixes (`us.anthropic.claude-…`).
const REGIONS: &[&str] = &["us", "eu", "apac", "au", "jp", "ca", "global", "us-gov"];

/// Bedrock `<vendor>.<model>` prefixes.
const VENDORS: &[&str] = &[
    "anthropic",
    "amazon",
    "meta",
    "mistral",
    "cohere",
    "ai21",
    "deepseek",
    "openai",
    "qwen",
    "writer",
];

/// Spellings of `model`, most specific first: as given; without region,
/// vendor (`vendor.` or `vendor/`) prefixes; without a Vertex `@…` or
/// Bedrock `-vN:M` suffix; without a `-YYYYMMDD` date.
pub fn candidates(model: &str) -> Vec<Cand> {
    let mut out = vec![Cand {
        id: model.to_string(),
        vendor: None,
    }];
    let (mut s, mut vendor) = (model, None::<&str>);
    if let Some((r, rest)) = s.split_once('.') {
        if REGIONS.contains(&r) && !rest.is_empty() {
            s = rest;
        }
    }
    if let Some((v, rest)) = s.split_once('.') {
        if VENDORS.contains(&v) && !rest.is_empty() {
            (s, vendor) = (rest, Some(v));
        }
    }
    if let Some((v, rest)) = s.split_once('/') {
        if !v.is_empty() && !rest.is_empty() {
            (s, vendor) = (rest, Some(v));
        }
    }
    let mut push = |s: &str| {
        let last = out.last().is_some_and(|c| c.id == s);
        if !last && !s.is_empty() {
            out.push(Cand {
                id: s.to_string(),
                vendor: vendor.map(str::to_string),
            });
        }
    };
    push(s);
    let s = s.split_once('@').map_or(s, |(a, _)| a);
    let s = strip_version(s);
    push(s);
    push(strip_date(s));
    out
}

/// `claude-x-v1:0` to `claude-x`.
fn strip_version(s: &str) -> &str {
    s.rsplit_once('-')
        .filter(|(_, tail)| {
            tail.strip_prefix('v')
                .and_then(|t| t.split_once(':'))
                .is_some_and(|(a, b)| digits(a) && digits(b))
        })
        .map_or(s, |(head, _)| head)
}

/// `claude-x-20250929` to `claude-x`.
pub fn strip_date(s: &str) -> &str {
    s.rsplit_once('-')
        .filter(|(_, d)| d.len() == 8 && digits(d))
        .map_or(s, |(head, _)| head)
}

fn digits(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}
