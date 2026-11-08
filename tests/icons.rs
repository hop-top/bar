// Unix fixtures (shell scripts, modes, POSIX paths); Windows coverage: tests/windows.rs
#![cfg(unix)]

mod common;
#[path = "common/plugin_fx.rs"]
mod plugin_fx;
#[path = "common/render_fx.rs"]
mod render_fx;

use hop_bar::conf::{Config, Icons};
use hop_bar::icons;
use hop_bar::layout::compose_with;
use hop_bar::options::Options;
use hop_bar::render::{render_in, ColdRunner};
use hop_bar::style::{dim, strip_codes, visible_width, visible_width_with};
use plugin_fx::Fx;
use render_fx::{run, Tmp};

const CTX_STDIN: &str = r#"{"model":{"display_name":"Opus"},"effort":{"level":"high"},"context_window":{"used_percentage":38}}"#;

fn conf(extra: &str, left: &str) -> String {
    format!("{extra}\nset left \"{left}\"")
}

fn line(t: &Tmp, c: &str, stdin: &str) -> render_fx::Out {
    run(t).conf(c).stdin(stdin).go()
}

// --- config ---

#[test]
fn set_icons_and_width_parse() {
    let c = hop_bar::conf::parse("set icons nerdfont\nset icon-width 2\n").unwrap();
    assert_eq!((c.icons, c.icon_width), (Icons::Nerdfont, 2));
    let d = Config::default();
    assert_eq!((d.icons, d.icon_width), (Icons::None, 1));
    assert!(hop_bar::conf::parse("set icons fancy").is_err());
    assert!(hop_bar::conf::parse("set icon-width 3").is_err());
}

// --- none is byte-identical ---

#[test]
fn none_set_output_unchanged() {
    let t = Tmp::new();
    let l = "#{model}#{effort} │ #{ctx}";
    let plain = line(&t, &conf("", l), CTX_STDIN);
    let none = line(&t, &conf("set icons none", l), CTX_STDIN);
    assert_eq!(plain.raw, none.raw);
    assert_eq!(plain.plain.trim(), "Opus[high] │ ctx 38%");
}

// --- sets ---

#[test]
fn builtin_table_covers_core_placeholders() {
    let o = Options::default();
    for set in [Icons::Nerdfont, Icons::Emoji] {
        for n in hop_bar::placeholders::names() {
            let g = icons::builtin(set, n, &o).unwrap_or_else(|| panic!("{n}"));
            assert!(visible_width_with(g, 2) <= 2, "{n} {g:?}");
        }
    }
    let mut seven = Options::default();
    seven.set("type", "7d");
    for set in [Icons::Nerdfont, Icons::Emoji] {
        assert_ne!(
            icons::builtin(set, "usage", &o),
            icons::builtin(set, "usage", &seven)
        );
    }
    assert!(icons::builtin(Icons::None, "model", &o).is_none());
}

#[test]
fn builtin_icons_are_distinct_and_documented() {
    let o = Options::default();
    let docs =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/docs/config.md")).unwrap();
    let table = docs
        .split("## Icons")
        .nth(1)
        .unwrap()
        .split("## ")
        .next()
        .unwrap();
    for set in [Icons::Nerdfont, Icons::Emoji] {
        let mut seen = std::collections::HashMap::new();
        for n in hop_bar::placeholders::names() {
            let g = icons::builtin(set, n, &o).unwrap();
            if let Some(prev) = seen.insert(g, n) {
                panic!("{n} and {prev} share {g:?}");
            }
        }
    }
    for n in hop_bar::placeholders::names() {
        let row = table
            .lines()
            .find(|l| l.starts_with(&format!("| `{n}`")))
            .unwrap_or_else(|| panic!("no icon row for {n}"));
        let nf = icons::builtin(Icons::Nerdfont, n, &o).unwrap();
        let code = format!("U+{:04X}", nf.chars().next().unwrap() as u32);
        assert!(row.contains(&code), "{n}: {row}");
        assert!(
            row.contains(icons::builtin(Icons::Emoji, n, &o).unwrap()),
            "{n}: {row}"
        );
    }
}

#[test]
fn nerdfont_and_emoji_prefix_values() {
    let t = Tmp::new();
    let nf = line(
        &t,
        &conf("set icons nerdfont", "#{model} #{effort}"),
        CTX_STDIN,
    );
    assert_eq!(nf.plain.trim(), "\u{f2db} Opus \u{f0e7} [high]");
    let em = line(&t, &conf("set icons emoji", "#{model}"), CTX_STDIN);
    assert_eq!(em.plain.trim(), "🤖 Opus");
    assert!(em.raw.starts_with(&dim("🤖")), "{:?}", em.raw);
}

#[test]
fn usage_windows_get_distinct_icons() {
    let t = Tmp::new();
    let far = 9_999_999_999i64;
    let s = format!(
        r#"{{"rate_limits":{{"five_hour":{{"used_percentage":10,"resets_at":{far}}},"seven_day":{{"used_percentage":10,"resets_at":{far}}}}}}}"#
    );
    let o = line(
        &t,
        &conf(
            "set icons emoji",
            "#{usage[type=5h,view=pct]}|#{usage[type=7d,view=pct]}",
        ),
        &s,
    );
    assert!(
        o.plain.contains("⏳") && o.plain.contains("📅"),
        "{}",
        o.plain
    );
}

#[test]
fn account_and_repo_get_icons() {
    let t = Tmp::new();
    t.write(".claude.json", &render_fx::account_json("a@b.test"));
    let r = render_fx::repo(&t, "proj");
    let o = run(&t)
        .conf(&conf("set icons emoji", "#{account} #{branch}"))
        .cwd(&r)
        .go();
    assert_eq!(o.plain.trim(), "👤 a@b.test 🌿 main");
}

// --- override ---

#[test]
fn explicit_icon_beats_set_and_none_hides() {
    let t = Tmp::new();
    let o = line(
        &t,
        &conf("set icons emoji", "#{model[icon=X]} #{effort[icon=none]}"),
        CTX_STDIN,
    );
    assert_eq!(o.plain.trim(), "X Opus [high]");
    // explicit icon works with the set off
    let o = line(&t, &conf("", "#{model[icon=X]}"), CTX_STDIN);
    assert_eq!(o.plain.trim(), "X Opus");
}

#[test]
fn default_line_icon_applies() {
    let t = Tmp::new();
    let o = line(
        &t,
        &conf("set icons emoji\ndefault model icon=M", "#{model}"),
        CTX_STDIN,
    );
    assert_eq!(o.plain.trim(), "M Opus");
}

#[test]
fn icon_replaces_ctx_label() {
    let t = Tmp::new();
    let o = line(&t, &conf("set icons nerdfont", "#{ctx}"), CTX_STDIN);
    assert_eq!(o.plain.trim(), "\u{f0e4} 38%");
    let o = line(&t, &conf("set icons emoji", "#{ctx[icon=none]}"), CTX_STDIN);
    assert_eq!(o.plain.trim(), "ctx 38%", "icon=none keeps the label");
}

#[test]
fn user_icon_is_sanitized() {
    let t = Tmp::new();
    let o = line(&t, &conf("", "#{model[icon=\x1bX]}"), CTX_STDIN);
    assert!(!o.raw.contains("\x1bX"), "{:?}", o.raw);
    assert_eq!(o.plain.trim(), "X Opus");
}

#[test]
fn icon_sits_outside_link() {
    let t = Tmp::new();
    let r = render_fx::repo(&t, "proj");
    git_origin(&r);
    let o = run(&t)
        .conf(&conf("set icons emoji", "#{repo}"))
        .cwd(&r)
        .go();
    let osc = o.raw.find("\x1b]8;;http").expect("link");
    let icon = o.raw.find("📁").expect("icon");
    assert!(icon < osc, "{:?}", o.raw);
    assert!(o.raw.starts_with(&dim("📁")), "{:?}", o.raw);
}

fn git_origin(d: &std::path::Path) {
    render_fx::git(
        d,
        &["remote", "add", "origin", "https://github.com/o/r.git"],
    );
}

// --- width ---

#[test]
fn icon_width_used_for_private_use() {
    let s = "\u{f2db} a";
    assert_eq!(visible_width(s), 3);
    assert_eq!(visible_width_with(s, 1), 3);
    assert_eq!(visible_width_with(s, 2), 4);
    assert_eq!(visible_width_with("\u{f8ff}", 2), 2);
    assert_eq!(visible_width_with("\u{d7ff}", 2), 1, "outside range");
    assert_eq!(visible_width_with("\u{f0000}", 2), 2);
    assert_eq!(visible_width_with("🤖 a", 2), 4, "emoji unchanged");
}

#[test]
fn layout_pads_by_icon_width() {
    let l = vec!["\u{f2db} a".to_string()];
    let r = vec!["z".to_string()];
    let w = |iw| {
        let out = compose_with(&l, &[], &r, Some(30), 3, iw);
        visible_width_with(&out, iw)
    };
    assert_eq!(w(1), 26);
    assert_eq!(w(2), 26);
    let pad = |iw| {
        let out = compose_with(&l, &[], &r, Some(30), 3, iw);
        out.matches(' ').count()
    };
    assert_eq!(pad(1), pad(2) + 1);
}

// --- plugins ---

const PLUGIN: &str = r#"#!/bin/sh
here=$(dirname "$0")
if [ "$1" = "--ext-info" ]; then
  echo '{"name":"echo","version":"1","icon":"I","capabilities":["placeholder"],"options":{}}'
  exit 0
fi
cat > "$here/stdin.json"
cat "$here/out.json"
"#;

fn plugin_line(set: &str, out: &str, layout: &str) -> String {
    plugin_line_with(PLUGIN, set, out, layout)
}

fn plugin_line_with(script: &str, set: &str, out: &str, layout: &str) -> String {
    let fx = Fx::new();
    fx.put("bar-echo", script);
    fx.put("out.json", out);
    let c = plugin_fx::cfg(&format!("{set}\nset left \"{layout}\"\n"));
    fx.prime("echo", &c, &[]);
    let (s, cache, dirs) = (plugin_fx::session(), fx.cache(), fx.dirs());
    strip_codes(&render_in(
        &s,
        &c,
        Some(120),
        &ColdRunner,
        Some(&cache),
        &dirs,
    ))
    .trim()
    .to_string()
}

#[test]
fn plugin_icon_replaces_label() {
    let out = r#"{"text":"v","label":"lbl"}"#;
    assert_eq!(
        plugin_line("", out, "#{echo}"),
        "lbl v",
        "set none keeps label"
    );
    assert_eq!(plugin_line("set icons emoji", out, "#{echo}"), "I v");
    let o2 = r#"{"text":"v","label":"lbl","icon":"O"}"#;
    assert_eq!(
        plugin_line("set icons emoji", o2, "#{echo}"),
        "O v",
        "output beats info"
    );
    assert_eq!(
        plugin_line("set icons emoji", o2, "#{echo[icon=none]}"),
        "lbl v"
    );
    assert_eq!(plugin_line("set icons emoji", o2, "#{echo[icon=Z]}"), "Z v");
    assert_eq!(plugin_line("", out, "#{echo[icon=Z]}"), "Z v");
}

#[test]
fn plugin_info_icon_parsed_and_sanitized() {
    let i = hop_bar::plugin::parse_info(
        r#"{"name":"n","version":"1","icon":"\u001b[31mX","capabilities":[]}"#,
    )
    .unwrap();
    assert_eq!(i.icon.as_deref(), Some("[31mX"));
}

#[test]
fn oversized_plugin_icon_dropped() {
    let out = r#"{"text":"v","label":"lbl","icon":"toolong"}"#;
    assert_eq!(
        plugin_line("set icons emoji", out, "#{echo}"),
        "I v",
        "falls back to the ext-info icon"
    );
    let bare = PLUGIN.replace(r#""icon":"I","#, r#""icon":"toolong","#);
    assert_eq!(
        plugin_line_with(&bare, "set icons emoji", out, "#{echo}"),
        "lbl v",
        "then to the label"
    );
}

#[test]
fn declared_icon_option_ignored() {
    let i = hop_bar::plugin::parse_info(
        r#"{"name":"n","version":"1","capabilities":[],"options":{"icon":{"type":"string"},"k":{"type":"int"}}}"#,
    )
    .unwrap();
    assert!(i.declares_icon_option);
    assert!(!i.options.contains_key("icon") && i.options.contains_key("k"));
}

#[test]
fn icon_replaces_ctx_label_in_every_set() {
    let t = Tmp::new();
    for set in ["nerdfont", "emoji"] {
        let o = line(&t, &conf(&format!("set icons {set}"), "#{ctx}"), CTX_STDIN);
        assert!(!o.plain.contains("ctx"), "{}", o.plain);
        assert!(o.plain.trim().ends_with("38%"));
    }
}

fn preview(t: &Tmp, conf: &str, cols: &str) -> String {
    use assert_cmd::Command;
    let (iso, cwd) = common::isolation(t.path());
    t.write("xdg/bar/bar.conf", conf);
    let o = Command::cargo_bin("bar")
        .unwrap()
        .arg("preview")
        .envs(iso)
        .env("COLUMNS", cols)
        .current_dir(cwd)
        .output()
        .unwrap();
    String::from_utf8(o.stdout).unwrap()
}

#[test]
fn preview_unchanged_with_icons_none() {
    let t = Tmp::new();
    for cols in ["60", "200"] {
        let base = preview(&t, "set gap 3", cols);
        let none = preview(&t, "set icons none\nset icon-width 2", cols);
        assert_eq!(base, none, "COLUMNS={cols}");
    }
}

#[test]
fn preview_shows_gh_icons() {
    let t = Tmp::new();
    let o = preview(
        &t,
        "set icons emoji\nset left \"#{prs} #{issues} #{ctx}\"",
        "200",
    );
    let o = strip_codes(&o);
    assert!(o.contains("🔀 #131") && o.contains("🐛 !57"), "{o}");
    assert!(!o.contains("ctx"), "{o}");
}
