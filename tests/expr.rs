use hop_bar::expr::{combine, parse, to_queries};

fn q(key: &str, s: &str) -> Vec<String> {
    to_queries(key, &parse(s).unwrap())
}

fn label(s: &str) -> Vec<String> {
    q("label", s)
}

#[test]
fn or_collapses() {
    assert_eq!(label("a|b"), ["label:a,b"]);
}

#[test]
fn and_with_quoted() {
    assert_eq!(label(r#"a&"b c""#), [r#"label:a label:"b c""#]);
}

#[test]
fn or_inside_and() {
    assert_eq!(label("(a|b)&c"), ["label:a,b label:c"]);
}

#[test]
fn negation() {
    assert_eq!(label("!a"), ["-label:a"]);
}

#[test]
fn and_binds_tighter() {
    assert_eq!(label("a&b|c"), ["label:a label:b", "label:c"]);
}

#[test]
fn author_or_splits_queries() {
    assert_eq!(
        q("author", "@me|dependabot"),
        ["author:@me", "author:dependabot"]
    );
}

#[test]
fn negated_or() {
    assert_eq!(label("!(a|b)"), ["-label:a -label:b"]);
}

#[test]
fn negated_and() {
    assert_eq!(label("!(a&b)"), ["-label:a", "-label:b"]);
}

#[test]
fn and_of_ors() {
    assert_eq!(label("(a|b)&(c|d)"), ["label:a,b label:c,d"]);
}

#[test]
fn not_or_label() {
    assert_eq!(label("!a|b"), ["-label:a", "label:b"]);
}

#[test]
fn assignee_negation() {
    assert_eq!(q("assignee", "!x"), ["-assignee:x"]);
}

#[test]
fn quoted_value_in_or_group() {
    assert_eq!(label(r#"a|"b c""#), [r#"label:a,"b c""#]);
}

#[test]
fn double_negation() {
    assert_eq!(label("!!a"), ["label:a"]);
}

#[test]
fn duplicates_removed() {
    assert_eq!(label("a|a"), ["label:a"]);
    assert_eq!(label("a&b|b&a"), ["label:a label:b", "label:b label:a"]);
    assert_eq!(label("a&b|a&b"), ["label:a label:b"]);
}

#[test]
fn spaces_around_operators() {
    assert_eq!(label(" a | b & c "), ["label:a", "label:b label:c"]);
}

#[test]
fn cross_key_product() {
    let l = label("a&b|c");
    let a = q("author", "x|y");
    let got = combine(&[l, a]);
    assert_eq!(
        got,
        [
            "label:a label:b author:x",
            "label:a label:b author:y",
            "label:c author:x",
            "label:c author:y",
        ]
    );
}

#[test]
fn combine_skips_empty_and_dedupes() {
    assert_eq!(combine(&[vec![], vec!["a".into(), "a".into()]]), ["a"]);
    assert!(combine(&[]).is_empty());
}

#[test]
fn unbalanced_paren_errors() {
    assert!(parse("(a|b").is_err());
    assert!(parse("a|b)").is_err());
}

#[test]
fn empty_value_errors() {
    assert!(parse("").is_err());
    assert!(parse("  ").is_err());
    assert!(parse("a|").is_err());
    assert!(parse("&a").is_err());
    assert!(parse(r#""""#).is_err());
    assert!(parse("()").is_err());
    assert!(parse("!").is_err());
}

#[test]
fn unterminated_quote_errors() {
    assert!(parse(r#"a&"b c"#).is_err());
}

#[test]
fn missing_operator_errors() {
    assert!(parse(r#"a"b""#).is_err());
}

#[test]
fn dnf_blowup_errors() {
    // 6 OR pairs = 64 conjuncts: allowed; 7 = 128: rejected
    let pair = |i: usize| format!("(a{i}|b{i})");
    let ok: Vec<_> = (0..6).map(pair).collect();
    assert!(parse(&ok.join("&")).is_ok());
    let bad: Vec<_> = (0..7).map(pair).collect();
    assert!(parse(&bad.join("&")).is_err());
}
