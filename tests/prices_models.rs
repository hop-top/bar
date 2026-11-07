use hop_bar::prices::{self, models::Snapshot, Price};

const FIX: &str = r#"{"source":"https://models.dev/api.json","fetched":"2026-11-01","providers":{
"amazon-bedrock":{"us.anthropic.claude-haiku-9-20260101-v1:0":[1,5,1.25,0.1,200000]},
"anthropic":{"claude-haiku-9":[1,5,1.25,0.1,200000],"claude-opus-4-5":[99,99,99,99,200000]},
"azure":{"gpt-4o":[2.5,10,null,1.25,128000],"dup":[3,3,null,null,1000]},
"google-vertex-anthropic":{"claude-haiku-8@20260101":[2,6,2.5,0.2,200000]},
"mistral":{"mistral-small":[0.1,0.3,null,null,32000],"dup":[1,1,null,null,1000]},
"openai":{"gpt-4o":[2.5,10,null,1.25,128000],"gpt-4o-mini":[0.15,0.6,null,0.075,128000],"dup":[3,3,null,null,1000]},
"openrouter":{"openai/gpt-5":[1.25,10,null,0.125,400000]}
}}"#;

fn snap() -> Snapshot {
    Snapshot::parse(FIX).unwrap()
}

fn mu(v: f64) -> u64 {
    (v * 1e6).round() as u64
}

#[test]
fn parses_header_and_counts() {
    let s = snap();
    assert_eq!(s.fetched(), "2026-11-01");
    assert_eq!(s.len(), 12);
}

#[test]
fn rejects_malformed_snapshots() {
    for bad in [
        "",
        "{}",
        r#"{"fetched":"x"}"#,
        r#"{"fetched":"x","providers":[]}"#,
    ] {
        assert!(Snapshot::parse(bad).is_none(), "{bad}");
    }
}

#[test]
fn exact_id_and_null_cache_rates_fall_back_to_input() {
    let r = snap().lookup("mistral-small", None).unwrap();
    assert_eq!(r, [mu(0.1), mu(0.3), mu(0.1), mu(0.1)]);
    let r = snap().lookup("gpt-4o", None).unwrap();
    assert_eq!(r, [mu(2.5), mu(10.0), mu(2.5), mu(1.25)]);
}

#[test]
fn strips_trailing_date_suffix() {
    let s = snap();
    assert_eq!(
        s.lookup("gpt-4o-mini-20240718", None),
        s.lookup("gpt-4o-mini", None)
    );
    assert!(s.lookup("gpt-4o-mini-20240718", None).is_some());
}

#[test]
fn bedrock_region_vendor_and_version_forms() {
    let s = snap();
    let want = s.lookup("claude-haiku-9", None).unwrap();
    for id in [
        "us.anthropic.claude-haiku-9-20260101-v1:0",
        "eu.anthropic.claude-haiku-9-20260101-v1:0",
        "anthropic.claude-haiku-9-20260101-v1:0",
        "global.anthropic.claude-haiku-9-v1:0",
        "anthropic.claude-haiku-9-v2:0",
    ] {
        assert_eq!(s.lookup(id, None), Some(want), "{id}");
    }
}

#[test]
fn openrouter_vendor_slash_model() {
    let s = snap();
    assert_eq!(s.lookup("openai/gpt-5", None).unwrap()[0], mu(1.25));
    assert_eq!(
        s.lookup("openai/gpt-4o-mini", None),
        s.lookup("gpt-4o-mini", None)
    );
    assert!(s.lookup("openai/gpt-4o-mini", None).is_some());
    assert_eq!(
        s.lookup("mistralai/mistral-small", None).unwrap()[0],
        mu(0.1)
    );
    assert_eq!(
        s.lookup("anthropic/claude-haiku-9", None).unwrap()[0],
        mu(1.0)
    );
}

#[test]
fn vertex_at_date_form() {
    let s = snap();
    assert_eq!(
        s.lookup("claude-haiku-8@20260101", None).unwrap()[0],
        mu(2.0)
    );
    assert_eq!(s.lookup("claude-haiku-8@default", None), None);
    assert_eq!(s.lookup("claude-haiku-8-20260101", None), None);
}

#[test]
fn same_price_across_providers_is_not_ambiguous() {
    assert_eq!(snap().lookup("gpt-4o", None).unwrap()[0], mu(2.5));
}

#[test]
fn differing_prices_are_unknown_unless_provider_given() {
    let s = snap();
    assert_eq!(s.lookup("dup", None), None);
    assert_eq!(s.lookup("dup", Some("mistral")).unwrap()[0], mu(1.0));
    assert_eq!(s.lookup("dup", Some("openai")).unwrap()[0], mu(3.0));
    assert_eq!(s.lookup("dup", Some("nowhere")), None);
}

#[test]
fn never_fuzzy() {
    let s = snap();
    for id in [
        "gpt-4",
        "gpt-4o-mini-x",
        "gpt",
        "mistral",
        "claude-haiku",
        "GPT-4O",
        "",
    ] {
        assert_eq!(s.lookup(id, None), None, "{id}");
    }
}

#[test]
fn precedence_override_then_builtin_then_snapshot() {
    let s = snap();
    let none: &[Price] = &[];
    // builtin beats the snapshot's claude-opus-4-5
    assert_eq!(
        prices::lookup_in("claude-opus-4-5", None, none, &s).unwrap()[0],
        mu(5.0)
    );
    // snapshot fills what neither has
    assert_eq!(
        prices::lookup_in("gpt-4o", None, none, &s).unwrap()[0],
        mu(2.5)
    );
    // override beats both
    let o = [Price::parse("claude-opus", "7", "7", "7", "7").unwrap()];
    assert_eq!(
        prices::lookup_in("claude-opus-4-5", None, &o, &s).unwrap()[0],
        mu(7.0)
    );
    let o = [Price::parse("gpt-4o", "1", "1", "1", "1").unwrap()];
    assert_eq!(
        prices::lookup_in("gpt-4o", None, &o, &s).unwrap()[0],
        mu(1.0)
    );
    // an ambiguous snapshot id stays unknown
    assert_eq!(prices::lookup_in("dup", None, none, &s), None);
}

#[test]
fn builtin_table_reads_through_the_normaliser() {
    let none: &[Price] = &[];
    let s = snap();
    let want = prices::lookup_in("claude-opus-4-5", None, none, &s);
    assert_eq!(
        prices::lookup_in("us.anthropic.claude-opus-4-5-20251101-v1:0", None, none, &s),
        want
    );
    assert_eq!(
        prices::lookup_in("anthropic/claude-opus-4-5", None, none, &s),
        want
    );
    assert_eq!(
        prices::lookup_in("claude-opus-4-5@20251101", None, none, &s),
        want
    );
}

#[test]
fn embedded_snapshot_parses_and_is_dated() {
    let s = prices::models::embedded();
    assert!(s.len() > 500, "{}", s.len());
    let f = s.fetched();
    assert!(f.len() == 10 && f.as_bytes()[4] == b'-', "{f}");
    assert!(s.lookup("gpt-4o", Some("openai")).is_some());
}
