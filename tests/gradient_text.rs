//! Recolouring text with `fg` and gradients: grapheme clusters, escape
//! sequences and the gradient's end stops.

use hop_bar::color::Color;
use hop_bar::conf::parse;
use hop_bar::gradient::{recolor, span, Ink};
use hop_bar::options::Options;
use hop_bar::placeholders::Ctx;
use hop_bar::render::render_sample;
use hop_bar::session::Session;
use hop_bar::style::{fg, strip_codes, visible_width};

const FAMILY: &str = "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}";
const FLAG: &str = "\u{1F1EB}\u{1F1F7}";
const HEART: &str = "\u{2764}\u{FE0F}";

fn sample(name: &str, _: &Options, _: &Ctx, _: bool) -> Option<String> {
    Some(match name {
        "zwj" => format!("a{FAMILY}b"),
        "flag" => format!("a{FLAG}b"),
        "vs" => format!("a{HEART}b"),
        "wide" => "ab\u{6F22}".to_string(),
        "inner" => format!("{} {}", fg(203, "red"), fg(114, "green")),
        _ => name.to_string(),
    })
}

fn render(conf: &str) -> String {
    let c = parse(conf).unwrap_or_else(|e| panic!("{conf}: {e:?}"));
    render_sample(&Session::default(), &c, Some(200), 0, &sample)
}

/// The SGR sequences of `text` in order.
fn sgrs(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(at) = rest.find("\x1b[") {
        let tail = &rest[at..];
        let end = tail[2..].find('m').map_or(tail.len(), |e| e + 3);
        out.push(&tail[..end]);
        rest = &tail[end..];
    }
    out
}

#[test]
fn span_counts_clusters_as_the_layout_does() {
    for s in [FAMILY, FLAG, HEART, "e\u{301}", "ab\u{6F22}"] {
        assert_eq!(span(s, 1), visible_width(s), "{s:?}");
    }
    assert_eq!(span(&format!("a{FAMILY}b"), 1), 4);
    assert_eq!(span("\u{E0A0}x", 2), 3);
}

#[test]
fn gradient_draws_each_cluster_in_one_colour() {
    for (name, cluster) in [("zwj", FAMILY), ("flag", FLAG), ("vs", HEART)] {
        let out = render(&format!(
            "set color-depth truecolor\nset left \"#{{{name}[gradient=#ff0000:#0000ff]}}\""
        ));
        // no escape sequence inside the cluster
        assert!(out.contains(cluster), "{name}: {out:?}");
        assert_eq!(strip_codes(&out), format!("a{cluster}b"));
        // three clusters, three colours: red, the middle, blue
        let colours: Vec<_> = sgrs(&out)
            .into_iter()
            .filter(|s| s.starts_with("\x1b[38;2;"))
            .collect();
        assert_eq!(colours.len(), 3, "{name}: {out:?}");
        assert_eq!(colours[0], "\x1b[38;2;255;0;0m");
        assert_eq!(colours[2], "\x1b[38;2;0;0;255m");
    }
}

#[test]
fn gradient_reaches_its_end_stop_on_a_wide_last_cluster() {
    let out = render("set color-depth truecolor\nset left \"#{wide[gradient=#ff0000:#0000ff]}\"");
    let colours: Vec<_> = sgrs(&out)
        .into_iter()
        .filter(|s| s.starts_with("\x1b[38;2;"))
        .collect();
    assert_eq!(colours.first(), Some(&"\x1b[38;2;255;0;0m"), "{out:?}");
    assert_eq!(colours.last(), Some(&"\x1b[38;2;0;0;255m"), "{out:?}");
}

#[test]
fn fg_drops_the_colours_it_overrides() {
    let out = render("set left \"#{inner[fg=75]}\"");
    assert_eq!(strip_codes(&out), "red green");
    assert!(!out.contains("38;5;203"), "{out:?}");
    assert!(!out.contains("38;5;114"), "{out:?}");
    assert_eq!(out, "\x1b[38;5;75mred green\x1b[39m");
}

#[test]
fn escape_with_one_byte_stays_whole() {
    let ink = Ink::Solid(Color::Idx(75));
    let out = recolor("a\x1b7b\x1b\\c", Some(&ink), false);
    assert!(out.contains("\x1b7"), "{out:?}");
    assert!(out.contains("\x1b\\"), "{out:?}");
    assert_eq!(span("a\x1b7b", 1), 2);
    // a multi-byte character after ESC stays intact
    let out = recolor("\x1b\u{e9}x", Some(&ink), false);
    assert!(out.contains("\x1b\u{e9}"), "{out:?}");
}
