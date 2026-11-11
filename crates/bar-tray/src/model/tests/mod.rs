use super::*;
use serde_json::{json, Value};

mod diffs;
mod parse;
mod state;

/// The example snapshot of the contract.
pub const FIXTURE: &str = include_str!("../../../tests/fixtures/snapshot.json");
/// Its `at`, so `Last:` ages are stable.
pub const NOW: u64 = 1_762_800_000;

pub fn fixture() -> Snapshot {
    parse(FIXTURE).expect("fixture parses")
}

pub fn fixture_json() -> Value {
    serde_json::from_str(FIXTURE).expect("fixture is JSON")
}

/// A one-plugin snapshot with the given groups.
pub fn with_groups(groups: Value) -> Value {
    json!({"version": 1, "generation": "g1", "at": NOW,
           "plugins": [{"name": "p", "groups": groups}]})
}

pub fn parse_value(v: &Value) -> Result<Snapshot, Error> {
    parse(&v.to_string())
}

/// Menu of the fixture at its own `at`.
pub fn menu() -> Menu {
    Menu::build(&fixture(), NOW)
}

/// Labels of every row in display order, indented by depth.
pub fn labels(m: &Menu) -> Vec<String> {
    m.flatten()
        .into_iter()
        .map(|(r, d)| format!("{}{}", "  ".repeat(d), r.label))
        .collect()
}
