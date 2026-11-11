use super::*;

#[test]
fn fixture_is_the_contract() {
    let s = fixture();
    assert_eq!(s.generation, "7c1f0a9e3b2d4f60");
    assert_eq!(s.at, NOW);
    assert_eq!(
        s.last_action.as_ref().map(|a| a.note.as_str()),
        Some("preferred alice")
    );
    assert_eq!(s.sessions.map(|c| (c.waiting, c.done)), Some((1, 2)));
    assert_eq!(s.views.len(), 2);
    assert_eq!(s.views[0].key, "5h");
    assert!(s.views[0].on);
    assert!(s.warnings.is_empty(), "{:?}", s.warnings);
}

#[test]
fn fixture_plugins() {
    let s = fixture();
    let names: Vec<&str> = s.plugins.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["cliproxyapi", "routellm"]);
    let cpa = &s.plugins[0];
    assert_eq!(cpa.label.as_deref(), Some("CLIProxyAPI"));
    let kinds: Vec<GroupKind> = cpa.groups.iter().map(|g| g.kind).collect();
    assert_eq!(
        kinds,
        [GroupKind::Radio, GroupKind::Buttons, GroupKind::Info]
    );
    let alice = &cpa.groups[0].items[0];
    assert_eq!(
        (alice.id.as_str(), alice.checked, alice.style),
        ("3", true, Some(Style::Good))
    );
    assert_eq!(alice.action.as_deref(), Some("use"));
    let soon = &cpa.groups[1].items[1];
    assert!(soon.confirm);
    let auth = &cpa.groups[2].items[0];
    assert_eq!(
        (auth.action.as_deref(), auth.style),
        (None, Some(Style::Bad))
    );
    let off = &s.plugins[1].groups[0].items[1];
    assert!(!off.enabled);
    assert_eq!(
        off.detail.as_deref(),
        Some("not enabled: tray routellm.toggle")
    );
}

#[test]
fn minimal_snapshot() {
    let s = parse(r#"{"version":1,"generation":"a","at":0}"#).unwrap();
    assert!(s.plugins.is_empty() && s.views.is_empty());
    assert!(s.last_action.is_none() && s.sessions.is_none());
}

#[test]
fn unknown_fields_are_ignored() {
    let mut v = fixture_json();
    v["future"] = json!(true);
    v["plugins"][0]["groups"][0]["items"][0]["later"] = json!({"x": 1});
    assert!(parse_value(&v).is_ok());
}

#[test]
fn not_json_or_not_object() {
    assert!(parse("nope").unwrap_err().why.starts_with("not JSON"));
    assert_eq!(parse("[1]").unwrap_err().why, "not an object");
    assert_eq!(parse("{}").unwrap_err().path, "version");
}

#[test]
fn version_must_match() {
    let mut v = fixture_json();
    v["version"] = json!(2);
    let e = parse_value(&v).unwrap_err();
    assert_eq!(e.path, "version");
    assert!(e.why.contains("unsupported version 2"), "{e}");
}

#[test]
fn generation_charset() {
    for bad in ["", "a b", "x/y", &"a".repeat(65)] {
        let mut v = fixture_json();
        v["generation"] = json!(bad);
        assert_eq!(parse_value(&v).unwrap_err().path, "generation", "{bad:?}");
    }
    let mut v = fixture_json();
    v["generation"] = json!("A1z9");
    assert!(parse_value(&v).is_ok());
}

#[test]
fn unknown_group_kind_is_dropped_not_refused() {
    let s = parse_value(&with_groups(json!([
        {"id": "a", "kind": "slider", "items": [{"id": "x", "label": "x"}]},
        {"id": "b", "kind": "check", "items": [{"id": "y", "label": "y", "action": "t"}]}
    ])))
    .unwrap();
    assert_eq!(s.plugins[0].groups.len(), 1);
    assert_eq!(s.plugins[0].groups[0].id, "b");
    assert_eq!(s.warnings.len(), 1);
    assert!(
        s.warnings[0].contains("unknown kind \"slider\""),
        "{:?}",
        s.warnings
    );
}

#[test]
fn ids_are_validated() {
    for (field, bad) in [
        ("id", "a.b"),
        ("id", ""),
        ("id", "a b"),
        ("id", &"a".repeat(33)),
    ] {
        let v = with_groups(
            json!([{"id": "g", "kind": "info", "items": [{field: bad, "label": "x"}]}]),
        );
        let e = parse_value(&v).unwrap_err();
        assert!(e.why.starts_with("id:"), "{bad:?}: {e}");
        let v = with_groups(json!([{"id": bad, "kind": "info", "items": []}]));
        assert!(
            parse_value(&v).unwrap_err().why.starts_with("id:"),
            "{bad:?}"
        );
    }
    for ok in ["claude@8317", "next-soon", "a:b_c", "ABC"] {
        let v =
            with_groups(json!([{"id": ok, "kind": "info", "items": [{"id": ok, "label": "x"}]}]));
        assert!(parse_value(&v).is_ok(), "{ok:?}");
    }
}

#[test]
fn dispatch_id_length() {
    let long = "a".repeat(32);
    let mut v = json!({"version": 1, "generation": "g", "at": 0,
        "plugins": [{"name": "p".repeat(32), "groups": [{"id": long, "kind": "info",
                     "items": [{"id": "b".repeat(30), "label": "x"}]}]}]});
    assert!(parse_value(&v).is_ok());
    v["plugins"][0]["groups"][0]["items"][0]["id"] = json!("b".repeat(31));
    let e = parse_value(&v).unwrap_err();
    assert!(e.why.contains("longer than 96"), "{e}");
}

#[test]
fn plugin_name_and_action_charset() {
    let mut v = fixture_json();
    v["plugins"][1]["name"] = json!("Route LLM");
    assert!(parse_value(&v).unwrap_err().why.starts_with("name:"));
    let mut v = fixture_json();
    v["plugins"][0]["groups"][0]["items"][0]["action"] = json!("Use");
    assert!(parse_value(&v).unwrap_err().why.starts_with("action:"));
}

#[test]
fn duplicates_are_refused() {
    let mut v = fixture_json();
    v["plugins"][1]["name"] = json!("cliproxyapi");
    assert_eq!(parse_value(&v).unwrap_err().why, "duplicate plugin name");
    let mut v = fixture_json();
    v["plugins"][0]["groups"][1]["id"] = json!("claude@8317");
    assert_eq!(parse_value(&v).unwrap_err().why, "duplicate group id");
    let mut v = fixture_json();
    v["plugins"][0]["groups"][0]["items"][1]["id"] = json!("3");
    let e = parse_value(&v).unwrap_err();
    assert_eq!(
        (e.path.as_str(), e.why.as_str()),
        ("plugins[cliproxyapi].claude@8317.3", "duplicate item id")
    );
}

#[test]
fn labels_are_cleaned_and_empty_rows_dropped() {
    let s = parse_value(&with_groups(json!([{"id": "g", "kind": "info", "items": [
        {"id": "a", "label": " a\tb\u{202E}c\n ", "detail": "\u{200B}"},
        {"id": "b", "label": "\u{0007}"},
        {"id": "c", "label": "x".repeat(100)}
    ]}])))
    .unwrap();
    let g = &s.plugins[0].groups[0];
    assert_eq!(g.items.len(), 2);
    assert_eq!(g.items[0].label, "a bc");
    assert_eq!(g.items[0].detail, None);
    assert_eq!(g.items[1].label.chars().count(), snapshot::MAX_LABEL);
    assert!(
        s.warnings.iter().any(|w| w.contains("empty label")),
        "{:?}",
        s.warnings
    );
}

#[test]
fn defaults_and_styles() {
    let s = parse_value(&with_groups(json!([{"id": "g", "kind": "check", "items": [
        {"id": "a", "label": "a", "style": "neon"},
        {"id": "b", "label": "b", "style": "dim", "enabled": false, "checked": true, "confirm": true,
         "limit": 90},
        {"id": "c", "label": "c", "limit": 0}
    ]}])))
    .unwrap();
    let g = &s.plugins[0].groups[0];
    let a = &g.items[0];
    assert_eq!(
        (
            a.enabled,
            a.checked,
            a.confirm,
            a.style,
            a.action.as_deref(),
            a.limit
        ),
        (true, false, false, None, None, None)
    );
    let b = &g.items[1];
    assert_eq!(
        (b.enabled, b.checked, b.confirm, b.style, b.limit),
        (false, true, true, Some(Style::Dim), Some(90))
    );
    assert_eq!(g.items[2].limit, None, "0 means unknown");
}

#[test]
fn item_caps() {
    let items: Vec<Value> = (0..20)
        .map(|i| json!({"id": format!("i{i}"), "label": "x"}))
        .collect();
    let s = parse_value(&with_groups(
        json!([{"id": "g", "kind": "info", "items": items}]),
    ))
    .unwrap();
    assert_eq!(s.plugins[0].groups[0].items.len(), snapshot::MAX_ITEMS);
    assert!(
        s.warnings[0].contains("more than 16 items"),
        "{:?}",
        s.warnings
    );
    let groups: Vec<Value> = (0..20)
        .map(|i| json!({"id": format!("g{i}"), "kind": "info", "items": []}))
        .collect();
    let s = parse_value(&with_groups(json!(groups))).unwrap();
    assert_eq!(s.plugins[0].groups.len(), snapshot::MAX_ITEMS);
    let plugins: Vec<Value> = (0..20).map(|i| json!({"name": format!("p{i}")})).collect();
    let s = parse_value(&json!({"version": 1, "generation": "g", "at": 0, "plugins": plugins}))
        .unwrap();
    assert_eq!(s.plugins.len(), snapshot::MAX_ITEMS);
}

#[test]
fn views_are_validated() {
    let s = parse_value(&json!({"version": 1, "generation": "g", "at": 0, "views": [
        {"key": "5h", "label": "5h", "on": true},
        {"key": "Bad Key", "label": "x"},
        {"key": "ok", "label": ""}
    ]}))
    .unwrap();
    assert_eq!(s.views.len(), 1);
    assert_eq!(s.warnings.len(), 2);
}

#[test]
fn error_display() {
    let e = Error {
        path: "plugins[p].g".into(),
        why: "duplicate group id".into(),
    };
    assert_eq!(e.to_string(), "plugins[p].g: duplicate group id");
}
