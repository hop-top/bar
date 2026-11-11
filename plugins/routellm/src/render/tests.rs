use super::*;
use crate::cache::{Alias, Entry, Row};

fn inst() -> Instance {
    crate::config::parse(
        "instance http://127.0.0.1:8318\n routellm http://127.0.0.1:6060\n",
        None,
    )
    .instances
    .remove(0)
}

fn snap(disabled: bool, up: Option<bool>, at: i64) -> Snapshot {
    Snapshot {
        status: Status::Ok,
        cache: Cache {
            entry: Some(Entry {
                disabled,
                priority: 1000,
                aliases: vec![
                    Alias {
                        alias: "route-auto".into(),
                        tier: "auto".into(),
                    },
                    Alias {
                        alias: "route-private".into(),
                        tier: "private".into(),
                    },
                ],
            }),
            rows: vec![Row {
                alias: "route-auto".into(),
                model: "claude-sonnet-5-5".into(),
            }],
            up,
            at,
            ..Default::default()
        },
    }
}

fn opts(s: &str) -> Opts {
    let o: serde_json::Map<String, serde_json::Value> = serde_json::from_str(s).unwrap();
    Opts::from_input(&RenderInput {
        options: o,
        ..Default::default()
    })
}

fn texts(o: &Output) -> Vec<(String, Option<String>)> {
    o.items
        .iter()
        .map(|i| (i.text.clone().unwrap(), i.style.clone()))
        .collect()
}

fn t(text: &str, style: Option<&str>) -> (String, Option<String>) {
    (text.to_string(), style.map(String::from))
}

#[test]
fn a_session_off_an_alias_shows_the_state_alone() {
    let i = inst();
    let go = |s: &Snapshot, now: i64, o: &Opts| output(s, &i, now, o, "claude-opus-4");
    let o = Opts::default();
    let on = go(&snap(false, Some(true), 100), 110, &o);
    assert_eq!(texts(&on), vec![t("on", Some("good"))]);
    assert_eq!(on.items[0].tokens["to"], "off");
    assert_eq!(on.items[0].tokens["instance"], instance_id(&i.origin));
    assert_eq!(on.tokens, on.items[0].tokens);
    // the mark leads, the label stands in for it with icons off
    assert_eq!(on.items[0].icon.as_deref(), Some(ICON));
    assert_eq!(on.items[0].label.as_deref(), Some(LABEL));
    let off = go(&snap(true, None, 100), 110, &o);
    assert_eq!(texts(&off), vec![t("off", Some("dim"))]);
    assert_eq!(off.items[0].tokens["to"], "on");
    let down = go(&snap(false, Some(false), 100), 110, &o);
    assert_eq!(texts(&down), vec![t("on \u{2717}", Some("bad"))]);
    let old = go(&snap(false, Some(true), 100), 100 + STALE_AFTER + 1, &o);
    assert_eq!(texts(&old), vec![t("on ~", Some("good"))]);
    let dim = go(
        &snap(false, Some(true), 100),
        100 + STALE_AFTER + 1,
        &opts(r#"{"stale":"dim"}"#),
    );
    assert_eq!(texts(&dim), vec![t("on", Some("dim"))]);
    let hidden = go(
        &snap(false, Some(true), 100),
        1000,
        &opts(r#"{"stale":"hide"}"#),
    );
    assert_eq!(hidden.text.as_deref(), Some(""));
}

#[test]
fn a_session_model_names_its_alias() {
    assert_eq!(alias_of_model("route-auto").as_deref(), Some("route-auto"));
    assert_eq!(
        alias_of_model("team/Route-Vision").as_deref(),
        Some("route-vision")
    );
    for m in [
        "",
        "route-",
        "router-random-0.5",
        "claude-opus-4",
        "myroute-auto",
    ] {
        assert_eq!(alias_of_model(m), None, "{m}");
    }
}

#[test]
fn on_shows_alias_in_front_of_its_bracketed_tier() {
    let i = inst();
    let on = snap(false, Some(true), 100);
    let go = |s: &Snapshot, model: &str, o: &str| output(s, &i, 110, &opts(o), model);
    // one item: the alias, the tier in brackets, green
    let v = go(&on, "route-auto", "{}");
    assert_eq!(texts(&v), vec![t("route-auto[auto]", Some("good"))]);
    assert_eq!(v.items[0].icon.as_deref(), Some(ICON));
    assert_eq!(v.items[0].label.as_deref(), Some(LABEL));
    assert_eq!(v.items[0].tokens["to"], "off");
    assert_eq!(
        texts(&go(&on, "route-private", "{}")),
        vec![t("route-private[private]", Some("good"))]
    );
    // compact and `label=none` leave the alias out
    for o in [r#"{"compact":true}"#, r#"{"label":"none"}"#] {
        assert_eq!(
            texts(&go(&on, "route-auto", o)),
            vec![t("auto", Some("good"))],
            "{o}"
        );
    }
    assert_eq!(
        texts(&go(&on, "route-auto", r#"{"show":"alias"}"#)),
        vec![t("route-auto", Some("good"))]
    );
    // `show` brings back the separate items
    assert_eq!(
        texts(&go(
            &on,
            "route-auto",
            r#"{"show":"state,alias,tier,marker"}"#
        )),
        vec![
            t("on", Some("good")),
            t("route-auto[auto]", Some("good")),
            t("\u{2601}", None)
        ]
    );
    assert_eq!(
        texts(&go(&on, "route-private", r#"{"show":"tier,marker"}"#)),
        vec![t("private", Some("good")), t("\u{2302}", None)]
    );
    // an alias the provider does not expose, or a plain model: state only
    for m in ["route-hard", "claude-opus-4", ""] {
        assert_eq!(texts(&go(&on, m, "{}")), vec![t("on", Some("good"))], "{m}");
    }
    // `routed` hides a session that is not on an alias
    assert_eq!(
        go(&on, "claude-opus-4", r#"{"routed":true}"#)
            .text
            .as_deref(),
        Some("")
    );
    assert_eq!(go(&on, "route-auto", r#"{"routed":true}"#).items.len(), 1);
}

#[test]
fn off_shows_the_tier_and_the_full_model_and_down_stays_red() {
    let i = inst();
    let go = |s: &Snapshot, model: &str, o: &str| texts(&output(s, &i, 110, &opts(o), model));
    let off = snap(true, None, 100);
    // the model the alias maps to, as the proxy knows it
    assert_eq!(
        go(&off, "route-auto", "{}"),
        vec![t("auto[claude-sonnet-5-5]", Some("dim"))]
    );
    assert_eq!(
        go(&off, "route-auto", r#"{"show":"tier"}"#),
        vec![t("auto[claude-sonnet-5-5]", Some("dim"))]
    );
    // no row: the proxy refuses, and the item says so
    assert_eq!(
        go(&off, "route-private", "{}"),
        vec![t("private[\u{2717}]", Some("bad"))]
    );
    // routing on but RouteLLM down: the ON text, in red
    let down = snap(false, Some(false), 100);
    assert_eq!(
        go(&down, "route-auto", "{}"),
        vec![t("route-auto[auto]", Some("bad"))]
    );
    assert_eq!(
        go(&down, "route-private", "{}"),
        vec![t("route-private[private]", Some("bad"))]
    );
    // old data: marked, or dim
    let old = |o: &str| {
        texts(&output(
            &snap(false, Some(true), 100),
            &i,
            100 + STALE_AFTER + 1,
            &opts(o),
            "route-auto",
        ))
    };
    assert_eq!(old("{}"), vec![t("route-auto[auto] ~", Some("good"))]);
    assert_eq!(
        old(r#"{"stale":"dim"}"#),
        vec![t("route-auto[auto]", Some("dim"))]
    );
}

#[test]
fn caps_and_links_are_opt_in() {
    let i = inst();
    let mut s = snap(false, Some(true), 100);
    s.cache.tiers = vec![crate::cache::Tier {
        id: "auto".into(),
        kind: "tier".into(),
        vision: Some(true),
        tools: Some(true),
        context: Some(200_000),
    }];
    let o = output(&s, &i, 110, &opts(r#"{"show":"state,caps"}"#), "route-auto");
    assert_eq!(
        texts(&o),
        vec![t("on", Some("good")), t("vis tools 200k", Some("dim"))]
    );
    let o = output(
        &s,
        &i,
        110,
        &opts(r#"{"link":"x://{instance}/{alias}/{tier}?q={tier}%"}"#),
        "route-auto",
    );
    assert_eq!(
        o.items[0].link.as_deref(),
        Some(format!("x://{}/route-auto/auto?q=auto%", instance_id(&i.origin)).as_str())
    );
    // the state item, when shown, stays clickable for the toggle
    let o = output(
        &s,
        &i,
        110,
        &opts(r#"{"show":"state,alias,tier","link":"x://{alias}"}"#),
        "route-auto",
    );
    assert_eq!(o.items[0].link, None);
    assert!(o.items[1].link.is_some());
    let none = output(&s, &i, 110, &opts("{}"), "route-auto");
    assert!(none.items.iter().all(|i| i.link.is_none()));
}

#[test]
fn the_action_timeout_covers_the_worst_toggle_with_a_margin() {
    // two passes, each waiting for the proxy's lock and the instance's
    // (8 s each); the entry read twice, the change and three read-backs
    // (3 s each); RouteLLM's health and models (3 s each); the pauses
    let lock_waits = 2 * 2 * 8;
    let calls = (2 + 1 + 3) * 3;
    let probes = 2 * 3;
    let pauses = 2;
    let need = lock_waits + calls + probes + pauses;
    for (n, a) in actions() {
        assert!(
            a.timeout >= need + 10 && a.timeout <= 120,
            "{n}: {} for {need}",
            a.timeout
        );
    }
}

#[test]
fn an_undrivable_instance_is_hidden() {
    let mut s = snap(false, Some(true), 100);
    s.cache.problem = Some(Problem::Mismatch);
    let o = Opts::default();
    assert_eq!(
        output(&s, &inst(), 101, &o, "route-auto").text.as_deref(),
        Some("")
    );
    let s = Snapshot {
        status: Status::AuthStopped(401),
        cache: Cache::default(),
    };
    let k = output(&s, &inst(), 101, &o, "route-auto");
    assert_eq!(texts(&k), vec![t("RL key?", Some("bad"))]);
}

#[test]
fn setup_prints_a_snippet_without_a_private_row_and_without_secrets() {
    let s = snap(false, Some(true), 1);
    let text = setup_snippet(&inst(), &s.cache, Layout::Legacy).join("\n");
    assert!(text.contains("alias: route-auto, fork: true"), "{text}");
    assert!(text.contains("priority: 1000") && text.contains("disable-cooling: true"));
    assert!(!text.contains("route-private, fork"), "{text}");
    assert!(text.contains("name: private, alias: route-private"));
}

fn yaml(l: Layout) -> serde_norway::Value {
    let s = snap(false, Some(true), 1);
    serde_norway::from_str(&setup_snippet(&inst(), &s.cache, l).join("\n")).expect("valid YAML")
}

fn at<'a>(v: &'a serde_norway::Value, path: &[&str]) -> &'a serde_norway::Value {
    path.iter().fold(v, |v, k| &v[*k])
}

#[test]
fn v8_snippet_is_a_complete_v8_block() {
    let v = yaml(Layout::V8);
    let p = &at(&v, &["api-keys", "openai-compatibility"])[0];
    assert_eq!(p["name"], "routellm");
    assert_eq!(p["disabled"], true);
    assert_eq!(p["priority"], 1000);
    assert_eq!(p["disable-cooling"], true);
    assert!(p["keys"][0]["api-key"].is_string(), "{p:?}");
    assert!(p["api-key-entries"].is_null());
    assert_eq!(p["models"][0]["alias"], "route-auto");
    let rows = at(&v, &["oauth", "model-alias", "claude"]);
    assert_eq!(rows[0]["alias"], "route-auto");
    assert_eq!(rows[0]["fork"], true);
    assert!(v["openai-compatibility"].is_null() && v["oauth-model-alias"].is_null());
}

#[test]
fn legacy_snippet_is_a_complete_legacy_block() {
    let v = yaml(Layout::Legacy);
    let p = &v["openai-compatibility"][0];
    assert_eq!(p["name"], "routellm");
    assert_eq!(p["disabled"], true);
    assert_eq!(p["disable-cooling"], true);
    assert!(p["api-key-entries"][0]["api-key"].is_string(), "{p:?}");
    assert!(p["keys"].is_null());
    assert_eq!(v["oauth-model-alias"]["claude"][0]["alias"], "route-auto");
    assert!(v["api-keys"].is_null() && v["oauth"].is_null());
}

#[test]
fn snippets_note_off_until_on_and_the_v8_rewrite() {
    let s = snap(false, Some(true), 1);
    let v8 = setup_snippet(&inst(), &s.cache, Layout::V8).join("\n");
    assert!(
        v8.contains("OFF until `bar rtellm on`") && v8.contains("keep a backup"),
        "{v8}"
    );
    let lg = setup_snippet(&inst(), &s.cache, Layout::Legacy).join("\n");
    assert!(
        lg.contains("OFF until `bar rtellm on`") && !lg.contains("backup"),
        "{lg}"
    );
}

#[test]
fn status_warns_when_the_fallback_is_not_armed() {
    let mut s = snap(false, Some(true), 1);
    s.cache.rows.clear();
    let t = status_lines(&s, &inst(), 2).join("\n");
    assert!(t.contains("no claude row"), "{t}");
    let t = status_lines(&snap(false, Some(true), 1), &inst(), 2).join("\n");
    assert!(!t.contains("warning"), "private needs no row: {t}");
}
