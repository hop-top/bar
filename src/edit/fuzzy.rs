//! Subsequence matching for the placeholder picker.

/// Score of `query` against `text` (case-insensitive): `None` unless every
/// query char appears in order. Higher is better: a match at the start, at
/// a word start and runs of adjacent chars score more; gaps cost.
pub fn score(query: &str, text: &str) -> Option<i32> {
    let text: Vec<char> = text.to_lowercase().chars().collect();
    let mut score = 0;
    let mut at = 0;
    let mut prev: Option<usize> = None;
    for q in query.to_lowercase().chars().filter(|c| !c.is_whitespace()) {
        let i = at + text[at..].iter().position(|&c| c == q)?;
        score += match prev {
            Some(p) if p + 1 == i => 8,
            _ if i == 0 => 10,
            _ if !text[i - 1].is_alphanumeric() => 6,
            _ => 1,
        };
        score -= (i - at).min(5) as i32;
        prev = Some(i);
        at = i + 1;
    }
    Some(score)
}

/// Indices of `items` matching `query`, best first; ties keep item order.
/// Each item is matched by its name, then (worth less) its description.
pub fn rank(query: &str, items: &[(String, String)]) -> Vec<usize> {
    let mut hits: Vec<(i32, usize)> = items
        .iter()
        .enumerate()
        .filter_map(|(i, (name, desc))| {
            let s = score(query, name)
                .map(|s| s + 100)
                .or_else(|| score(query, desc))?;
            Some((s, i))
        })
        .collect();
    hits.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    hits.into_iter().map(|(_, i)| i).collect()
}
