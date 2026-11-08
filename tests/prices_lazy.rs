use hop_bar::prices::{self, models, Price};

// One test per binary: the load counter is process-wide.
#[test]
fn snapshot_loads_only_when_override_and_builtin_miss() {
    let o = [Price::parse("gpt-4o", "1", "1", "1", "1").unwrap()];
    assert!(prices::lookup("claude-opus-4-5-20251101", &[]).is_some());
    assert!(prices::lookup("us.anthropic.claude-opus-4-5-20251101-v1:0", &[]).is_some());
    assert!(prices::lookup("gpt-4o", &o).is_some());
    assert_eq!(models::loads(), 0);
    prices::lookup("gpt-4o", &[]);
    assert!(models::loads() >= 1);
}
