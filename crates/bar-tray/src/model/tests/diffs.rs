use super::*;

#[test]
fn fixture_menu_tree() {
    let m = menu();
    assert_eq!(
        labels(&m),
        [
            "Claude · :8317",
            "  alice",
            "  bob",
            "  carol",
            "Use most quota left",
            "Use soonest reset",
            "management refused the key",
            "",
            "Routing",
            "Routing :8318",
            "",
            "Last: ok: preferred alice · 12 s ago",
            "Sessions: 1 waiting · 2 done",
            "",
            "5h usage",
            "7d usage",
            "",
            "Quit",
        ]
    );
    let sub = &m.items[0];
    assert!(matches!(sub.kind, Kind::Submenu(_)));
    assert_eq!(sub.id, "cliproxyapi.claude@8317");
    let alice = m.find("cliproxyapi.claude@8317.3").unwrap();
    assert_eq!(
        (&alice.kind, alice.checked, alice.enabled, alice.style),
        (&Kind::Choice, true, true, Some(Style::Good))
    );
    assert_eq!(alice.detail.as_deref(), Some("5h 59% ↻2h10m · 7d 71%"));
    let soon = m.find("cliproxyapi.next@8317.next-soon").unwrap();
    assert_eq!(
        (&soon.kind, soon.confirm, soon.clickable()),
        (&Kind::Action, true, true)
    );
    let auth = m.find("cliproxyapi.info.auth").unwrap();
    assert_eq!(
        (&auth.kind, auth.enabled, auth.clickable()),
        (&Kind::Info, false, false)
    );
    let toggle = m.find("routellm.rtellm@8317.toggle").unwrap();
    assert_eq!(
        (&toggle.kind, toggle.checked, toggle.clickable()),
        (&Kind::Toggle, true, true)
    );
    let off = m.find("routellm.rtellm@8317.toggle@8318").unwrap();
    assert!(!off.enabled && !off.clickable());
    let last = m.find(LAST_ID).unwrap();
    assert_eq!((&last.kind, last.style), (&Kind::Info, Some(Style::Dim)));
    let view = m.find("tray.view.5h").unwrap();
    assert_eq!(
        (&view.kind, view.checked, view.clickable()),
        (&Kind::Toggle, true, true)
    );
    assert!(m.find(QUIT_ID).unwrap().clickable());
    let seps: Vec<&str> = m
        .flatten()
        .iter()
        .filter(|(r, _)| r.kind == Kind::Separator)
        .map(|(r, _)| r.id.as_str())
        .collect();
    assert_eq!(
        seps,
        ["tray.sep.0", "tray.sep.1", "tray.sep.2", "tray.sep.3"]
    );
}

#[test]
fn empty_snapshot_is_just_quit() {
    let s = parse(r#"{"version":1,"generation":"a","at":0}"#).unwrap();
    assert_eq!(labels(&Menu::build(&s, 0)), ["Quit"]);
}

#[test]
fn rows_without_action_are_disabled_and_never_confirm() {
    let s = parse_value(&with_groups(
        json!([{"id": "g", "kind": "buttons", "items": [
            {"id": "a", "label": "a", "confirm": true},
            {"id": "b", "label": "b", "action": "x", "confirm": true}
        ]}]),
    ))
    .unwrap();
    let m = Menu::build(&s, 0);
    let a = m.find("p.g.a").unwrap();
    assert!(!a.enabled && !a.confirm);
    let b = m.find("p.g.b").unwrap();
    assert!(b.enabled && b.confirm);
}

#[test]
fn checked_only_on_choice_and_toggle() {
    let s = parse_value(&with_groups(json!([
        {"id": "b", "kind": "buttons", "items": [{"id": "x", "label": "x", "checked": true, "action": "a"}]},
        {"id": "i", "kind": "info", "items": [{"id": "y", "label": "y", "checked": true}]}
    ])))
    .unwrap();
    let m = Menu::build(&s, 0);
    assert!(!m.find("p.b.x").unwrap().checked);
    assert!(!m.find("p.i.y").unwrap().checked);
}

#[test]
fn empty_radio_group_and_empty_plugin_leave_no_rows() {
    let s = parse_value(&json!({"version": 1, "generation": "g", "at": 0, "plugins": [
        {"name": "a", "groups": [{"id": "r", "kind": "radio", "label": "R", "items": []}]},
        {"name": "b", "groups": [{"id": "i", "kind": "info", "items": [{"id": "x", "label": "x"}]}]}
    ]}))
    .unwrap();
    assert_eq!(labels(&Menu::build(&s, 0)), ["x", "", "Quit"]);
}

#[test]
fn radio_submenu_falls_back_to_group_id() {
    let s = parse_value(&with_groups(json!([{"id": "claude@1", "kind": "radio",
        "items": [{"id": "1", "label": "one", "action": "use"}]}])))
    .unwrap();
    assert_eq!(labels(&Menu::build(&s, 0))[0], "claude@1");
}

#[test]
fn last_action_label() {
    let mut v = fixture_json();
    v["last_action"] = json!({"at": NOW - 125, "ok": false, "note": "needs confirmation"});
    let m = Menu::build(&parse_value(&v).unwrap(), NOW);
    let last = m.find(LAST_ID).unwrap();
    assert_eq!(last.label, "Last: refused: needs confirmation · 2 m ago");
    assert_eq!(last.style, Some(Style::Bad));
    v["last_action"] = json!({"at": NOW + 5, "ok": true, "note": ""});
    let m = Menu::build(&parse_value(&v).unwrap(), NOW);
    assert_eq!(m.find(LAST_ID).unwrap().label, "Last: ok · 0 s ago");
}

#[test]
fn age_units() {
    assert_eq!(age(59), "59 s ago");
    assert_eq!(age(60), "1 m ago");
    assert_eq!(age(3599), "59 m ago");
    assert_eq!(age(7200), "2 h ago");
    assert_eq!(age(86_400 * 3), "3 d ago");
}

#[test]
fn same_menu_no_diff() {
    assert_eq!(diff(&menu(), &menu()), Diff::Same);
}

#[test]
fn property_changes_update_rows_in_place() {
    let old = menu();
    let mut v = fixture_json();
    v["plugins"][0]["groups"][0]["items"][0]["checked"] = json!(false);
    v["plugins"][0]["groups"][0]["items"][1]["checked"] = json!(true);
    v["plugins"][0]["groups"][0]["items"][1]["detail"] = json!("5h 13%");
    v["plugins"][1]["groups"][0]["items"][1]["enabled"] = json!(true);
    v["last_action"] = json!({"at": NOW - 1, "ok": true, "note": "preferred bob"});
    let new = Menu::build(&parse_value(&v).unwrap(), NOW);
    let d = diff(&old, &new);
    let Diff::Update(changes) = &d else {
        panic!("{d:?}")
    };
    let ids: Vec<&str> = changes.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "cliproxyapi.claude@8317.3",
            "cliproxyapi.claude@8317.4",
            "routellm.rtellm@8317.toggle@8318",
            LAST_ID
        ]
    );
    assert_eq!(
        (changes[1].checked, changes[1].detail.as_deref()),
        (true, Some("5h 13%"))
    );
    assert!(changes[2].enabled);
    assert_eq!(changes[3].label, "Last: ok: preferred bob · 1 s ago");
    let mut applied = old.clone();
    applied.apply(&d).unwrap();
    assert_eq!(applied, new);
}

#[test]
fn shape_changes_rebuild() {
    let old = menu();
    let mut v = fixture_json();
    // a row removed
    v["plugins"][0]["groups"][0]["items"]
        .as_array_mut()
        .unwrap()
        .pop();
    let new = Menu::build(&parse_value(&v).unwrap(), NOW);
    assert_eq!(diff(&old, &new), Diff::Rebuild(new.clone()));
    // a row moved
    let mut v = fixture_json();
    v["plugins"].as_array_mut().unwrap().reverse();
    let new = Menu::build(&parse_value(&v).unwrap(), NOW);
    assert!(matches!(diff(&old, &new), Diff::Rebuild(_)));
    // same ids, other kind
    let mut v = fixture_json();
    v["plugins"][1]["groups"][0]["kind"] = json!("buttons");
    let new = Menu::build(&parse_value(&v).unwrap(), NOW);
    assert!(matches!(diff(&old, &new), Diff::Rebuild(_)));
    // same ids, other nesting
    let mut v = fixture_json();
    v["plugins"][0]["groups"][0]["kind"] = json!("check");
    let new = Menu::build(&parse_value(&v).unwrap(), NOW);
    assert!(matches!(diff(&old, &new), Diff::Rebuild(_)));
    let mut applied = old.clone();
    applied.apply(&diff(&old, &new)).unwrap();
    assert_eq!(applied, new);
}

#[test]
fn apply_refuses_unknown_row() {
    let mut m = menu();
    let d = Diff::Update(vec![Change {
        id: "nope".into(),
        label: "x".into(),
        detail: None,
        checked: false,
        enabled: true,
        style: None,
        confirm: false,
    }]);
    assert_eq!(m.apply(&d), Err("no row nope".into()));
    assert!(m.apply(&Diff::Same).is_ok());
}

#[test]
fn find_mut_reaches_submenu_rows() {
    let mut m = menu();
    m.find_mut("cliproxyapi.claude@8317.4").unwrap().checked = true;
    assert!(m.find("cliproxyapi.claude@8317.4").unwrap().checked);
    assert!(m.find_mut("missing").is_none());
}

fn row(id: &str, kind: Kind) -> MenuItem {
    MenuItem {
        id: id.into(),
        kind,
        label: id.into(),
        detail: None,
        checked: false,
        enabled: true,
        style: None,
        confirm: false,
    }
}

#[test]
fn same_rows_other_depth_rebuild() {
    let nested = Menu {
        items: vec![
            row("a", Kind::Submenu(vec![row("b", Kind::Info)])),
            row("c", Kind::Info),
        ],
    };
    let flat = Menu {
        items: vec![
            row("a", Kind::Submenu(vec![])),
            row("b", Kind::Info),
            row("c", Kind::Info),
        ],
    };
    assert!(matches!(diff(&nested, &flat), Diff::Rebuild(_)));
    assert!(matches!(diff(&flat, &nested), Diff::Rebuild(_)));
}

#[test]
fn prefix_menus_rebuild() {
    let two = Menu {
        items: vec![row("a", Kind::Info), row("b", Kind::Info)],
    };
    let one = Menu {
        items: vec![row("a", Kind::Info)],
    };
    assert_eq!(diff(&two, &one), Diff::Rebuild(one.clone()));
    assert_eq!(diff(&one, &two), Diff::Rebuild(two));
}
