//! `set color-depth auto` reading the environment of `bar render`, and
//! `bar check` on the styling options.

mod common;
#[path = "common/render_fx.rs"]
mod render_fx;
#[allow(unused_imports)]
use render_fx::*;

use hop_bar::check::{check_text, Level};

const CONF: &str = "set color-depth auto\nset left \"#{model[fg=#ff8800,bold]}\"";
const STDIN: &str = r#"{"model":{"display_name":"Opus"}}"#;

fn render(vars: &[(&'static str, &str)]) -> String {
    let t = Tmp::new();
    let mut r = run(&t).conf(CONF).stdin(STDIN);
    for k in ["COLORTERM", "NO_COLOR", "FORCE_COLOR"] {
        r = match vars.iter().find(|(n, _)| *n == k) {
            Some((k, v)) => r.env(k, v),
            None => r.unset(k),
        };
    }
    r.go().raw
}

#[test]
fn auto_depth_reads_colorterm_no_color_and_force_color() {
    assert!(render(&[]).contains("\x1b[38;5;208m"));
    assert!(render(&[("COLORTERM", "truecolor")]).contains("\x1b[38;2;255;136;0m"));
    let none = render(&[("NO_COLOR", "1"), ("COLORTERM", "truecolor")]);
    assert_eq!(none.trim_end(), "\x1b[1mOpus\x1b[22m");
    let sixteen = render(&[("NO_COLOR", "1"), ("FORCE_COLOR", "1")]);
    // only 16-colour foreground SGR (3x, 9x), besides bold
    let codes: Vec<&str> = sixteen
        .split("\x1b[")
        .skip(1)
        .map(|c| &c[..c.find('m').unwrap()])
        .filter(|c| !matches!(*c, "1" | "22" | "39"))
        .collect();
    assert!(!codes.is_empty(), "{sixteen:?}");
    assert!(
        codes
            .iter()
            .all(|c| c.len() == 2 && matches!(&c[..1], "3" | "9")),
        "{sixteen:?}"
    );
}

#[test]
fn fixed_depth_ignores_no_color() {
    let t = Tmp::new();
    let o = run(&t)
        .conf("set left \"#{model[fg=75]}\"")
        .stdin(STDIN)
        .env("NO_COLOR", "1")
        .go();
    assert!(o.raw.contains("\x1b[38;5;75m"), "{:?}", o.raw);
}

fn errors(conf: &str) -> Vec<String> {
    check_text(conf, None, &[])
        .issues
        .into_iter()
        .filter(|i| i.level == Level::Error)
        .map(|i| i.message)
        .collect()
}

#[test]
fn check_validates_styling_options() {
    let ok = "set left \"#{model[fg=#ff8800,bold,gradient=ember]} #{ctx[bg=good,fg=bright-cyan,gradient=1:#00ff00:blue]}\"\ndefault repo fg=accent bold=off\n";
    assert_eq!(errors(ok), Vec::<String>::new());
    for (opt, want) in [
        ("fg=pink", "invalid fg `pink`"),
        ("bg=#12", "invalid bg `#12`"),
        ("bg=300", "invalid bg `300`"),
        ("bold=maybe", "invalid bold `maybe`"),
        ("gradient=nope", "invalid gradient `nope`"),
        ("gradient=#ff0000", "invalid gradient `#ff0000`"),
    ] {
        let e = errors(&format!("set left \"#{{model[{opt}]}}\""));
        assert_eq!(e.len(), 1, "{opt}: {e:?}");
        assert!(e[0].contains(want), "{opt}: {e:?}");
    }
    assert_eq!(
        errors("set left \"#{model[fg=%$C%]}\""),
        Vec::<String>::new()
    );
}

#[test]
fn styling_options_are_reserved_from_plugins() {
    let raw = r#"{"name":"n","version":"1","capabilities":[],"options":{"gradient":{"type":"string"},"fg":{"type":"string"},"k":{"type":"int"}}}"#;
    let i = hop_bar::plugin::parse_info(raw).unwrap();
    assert_eq!(i.declares_look_option, Some("fg"));
    for k in ["fg", "gradient"] {
        assert!(!i.options.contains_key(k), "{k}");
    }
    assert!(i.options.contains_key("k"));
}

#[test]
fn web_schema_lists_every_placeholder_option_with_help() {
    let s = hop_bar::web::schema();
    let keys: Vec<&str> = s["generic"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| {
            assert!(o["help"].as_str().is_some_and(|h| !h.is_empty()), "{o}");
            o["key"].as_str().unwrap()
        })
        .collect();
    assert_eq!(keys, ["icon", "bg", "fg", "bold", "gradient"]);
    let settings = hop_bar::web::inspect("set color-depth truecolor\nset gradient dusk\n");
    assert_eq!(settings["set"]["color-depth"], "truecolor");
    assert_eq!(settings["set"]["gradient"], "dusk");
    assert_eq!(hop_bar::web::inspect("")["set"]["gradient"], "off");
}
