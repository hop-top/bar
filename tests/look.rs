//! `fg`, `bold`, `gradient` and `set gradient`, `set color-depth` on the
//! rendered line.

use hop_bar::conf::parse;
use hop_bar::options::Options;
use hop_bar::placeholders::Ctx;
use hop_bar::render::render_sample;
use hop_bar::session::Session;
use hop_bar::style::{fg, link, strip_codes, visible_width};

/// Values with colours and a link of their own, as placeholders draw them.
fn sample(name: &str, _: &Options, _: &Ctx, _: bool) -> Option<String> {
    Some(match name {
        "a" => format!("{} {}", fg(203, "red"), fg(114, "green")),
        "b" => link("https://example.com/x", "linked"),
        "c" => "e\u{301}x".to_string(),
        "d" => "longer value".to_string(),
        "e" => String::new(),
        _ => name.to_string(),
    })
}

fn render(conf: &str) -> String {
    let c = parse(conf).unwrap_or_else(|e| panic!("{conf}: {e:?}"));
    render_sample(&Session::default(), &c, Some(200), 0, &sample)
}

/// The foreground SGR parameters in effect at each visible character.
fn inks(text: &str) -> Vec<(char, String)> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut it = text.char_indices().peekable();
    while let Some((i, c)) = it.next() {
        if c != '\x1b' {
            out.push((c, cur.clone()));
            continue;
        }
        let rest = &text[i + 1..];
        if let Some(csi) = rest.strip_prefix('[') {
            let end = 1 + csi.find(|c: char| ('\x40'..='\x7e').contains(&c)).unwrap();
            let params = &rest[1..end];
            if rest.as_bytes()[end] == b'm' {
                let p: Vec<&str> = params.split(';').collect();
                let mut k = 0;
                while k < p.len() {
                    match p[k] {
                        "38" => {
                            let n = if p[k + 1] == "5" { 3 } else { 5 };
                            cur = p[k..k + n].join(";");
                            k += n;
                            continue;
                        }
                        "39" | "0" | "" => cur.clear(),
                        x if x.len() == 2 && (x.starts_with('3') || x.starts_with('9')) => {
                            cur = x.to_string()
                        }
                        _ => {}
                    }
                    k += 1;
                }
            }
            for _ in 0..=end {
                it.next();
            }
        } else {
            // OSC: up to ST
            let end = rest.find("\x1b\\").unwrap() + 2;
            for _ in 0..end {
                it.next();
            }
        }
    }
    out
}

fn ink_of(text: &str, ch: char) -> String {
    inks(text).into_iter().find(|(c, _)| *c == ch).unwrap().1
}

#[test]
fn fg_overrides_the_colours_inside_a_value() {
    let out = render("set left \"#{a[fg=75]}\"");
    assert_eq!(strip_codes(&out), "red green");
    assert!(
        inks(&out).iter().all(|(_, ink)| ink == "38;5;75"),
        "{out:?}"
    );
    let plain = render("set left \"#{a}\"");
    assert_eq!(ink_of(&plain, 'r'), "38;5;203");
}

#[test]
fn fg_takes_roles_hex_and_names() {
    let c = parse("set color.good 33").unwrap();
    let good = render_sample(
        &Session::default(),
        &c.clone(),
        Some(200),
        0,
        &|_: &str, _: &Options, _: &Ctx, _: bool| Some("v".into()),
    );
    assert!(!good.contains("38;5;33"), "no fg, no role colour: {good:?}");
    let conf = "set color.good 33\nset left \"#{x[fg=good]} #{y[fg=#ff8800]} #{z[fg=bright-red]}\"";
    let out = render(conf);
    assert_eq!(ink_of(&out, 'x'), "38;5;33");
    assert_eq!(ink_of(&out, 'y'), "38;5;208");
    assert_eq!(ink_of(&out, 'z'), "38;5;9");
    let tc = render(&format!("set color-depth truecolor\n{conf}"));
    assert_eq!(ink_of(&tc, 'y'), "38;2;255;136;0");
}

#[test]
fn bold_wraps_the_value() {
    let out = render("set left \"#{d[bold]} #{x[bold=off]}\"");
    assert!(out.starts_with("\x1b[1ml"), "{out:?}");
    assert!(out.contains("value\x1b[22m"), "{out:?}");
    assert_eq!(strip_codes(&out), "longer value x");
    assert_eq!(out.matches("\x1b[1m").count(), 1);
}

#[test]
fn gradient_runs_across_the_value() {
    let out = render("set color-depth truecolor\nset left \"#{d[gradient=#ff0000:#0000ff]}\"");
    let cells = inks(&out);
    assert_eq!(strip_codes(&out), "longer value");
    assert_eq!(cells.first().unwrap().1, "38;2;255;0;0");
    assert_eq!(cells.last().unwrap().1, "38;2;0;0;255");
    // every cell between is its own colour
    let distinct: std::collections::BTreeSet<_> = cells.iter().map(|(_, i)| i).collect();
    assert!(distinct.len() > 8, "{distinct:?}");
    assert!(out.ends_with("\x1b[39m"), "{out:?}");
}

#[test]
fn gradient_quantises_to_the_depth() {
    let conf = "set left \"#{d[gradient=ember]}\"";
    for (depth, sgr) in [("", "38;5;"), ("set color-depth 16\n", "")] {
        let out = render(&format!("{depth}{conf}"));
        assert!(!out.contains("38;2;"), "{out:?}");
        for (_, ink) in inks(&out) {
            assert!(ink.starts_with(sgr) && !ink.is_empty(), "{depth}: {ink}");
        }
        assert_eq!(strip_codes(&out), "longer value");
    }
    // neighbouring cells with one 256-colour code share one SGR
    let out = render(conf);
    assert!(out.matches("\x1b[38;5;").count() < "longer value".len());
}

#[test]
fn gradient_keeps_links_width_and_combining_marks() {
    let conf = "set color-depth truecolor\nset left \"#{b[gradient=frost]} #{c[gradient=dusk]}\"";
    let out = render(conf);
    assert!(
        out.contains("\x1b]8;;https://example.com/x\x1b\\"),
        "{out:?}"
    );
    assert!(out.contains("\x1b]8;;\x1b\\"), "{out:?}");
    assert_eq!(strip_codes(&out), "linked e\u{301}x");
    assert_eq!(visible_width(&out), visible_width("linked e\u{301}x"));
    // the combining mark follows its letter directly
    assert!(out.contains("e\u{301}"), "{out:?}");
}

#[test]
fn gradient_beats_fg_and_keeps_truncation() {
    let out =
        render("set color-depth truecolor\nset left \"#{d[fg=75;gradient=#000000:#ffffff]}\"");
    assert_eq!(ink_of(&out, 'l'), "38;2;0;0;0");
    assert!(!out.contains("38;5;75"));
    // values are cut before they are painted
    let c = parse("set color-depth truecolor\nset left \"#{name[max=4;gradient=moss]}\"").unwrap();
    let s = Session {
        name: "a long session name".into(),
        ..Session::default()
    };
    let out = hop_bar::render::render(&s, &c, Some(200), &hop_bar::render::ColdRunner);
    let plain = parse("set left \"#{name[max=4]}\"").unwrap();
    let want = hop_bar::render::render(&s, &plain, Some(200), &hop_bar::render::ColdRunner);
    assert_eq!(strip_codes(&out), strip_codes(&want));
    assert!(out.contains("38;2;"), "{out:?}");
}

#[test]
fn empty_values_stay_empty() {
    let out = render("set left \"#{e[fg=75;bold;gradient=ash]} │ #{x}\"");
    assert_eq!(strip_codes(&out), "x");
}

#[test]
fn line_gradient_spans_all_segments() {
    let conf = "set color-depth truecolor\nset gradient \"#ff0000:#0000ff\"\nset left \"#{x} │ #{a}\"\nset right \"#{d}\"";
    let out = render(conf);
    let cells: Vec<_> = inks(&out)
        .into_iter()
        .filter(|(c, _)| !c.is_whitespace())
        .collect();
    assert_eq!(cells.first().unwrap(), &('x', "38;2;255;0;0".to_string()));
    assert_eq!(cells.last().unwrap(), &('e', "38;2;0;0;255".to_string()));
    // the joiner keeps its muted colour
    assert_eq!(ink_of(&out, '│'), "38;5;250");
    let plain = render(&conf.replace("set gradient \"#ff0000:#0000ff\"\n", ""));
    assert_eq!(strip_codes(&out), strip_codes(&plain));
    assert_eq!(visible_width(&out), visible_width(&plain));
}

#[test]
fn line_gradient_leaves_skin_glyphs_alone() {
    let base = "set skin powerline\nset skin-glyphs ascii\nset left \"#{x} │ #{d}\"\n";
    let plain = render(base);
    let out = render(&format!("{base}set gradient lagoon\n"));
    assert_eq!(strip_codes(&out), strip_codes(&plain));
    // the arrows keep the colours of the backgrounds they join
    let arrows = |s: &str| -> Vec<String> {
        inks(s)
            .into_iter()
            .filter(|(c, _)| *c == '>')
            .map(|(_, i)| i)
            .collect()
    };
    assert_eq!(arrows(&out), arrows(&plain));
    assert_eq!(render(&format!("{base}set gradient off\n")), plain);
}

#[test]
fn line_gradient_values() {
    assert!(parse("set gradient ember").is_ok());
    assert!(parse("set gradient \"1:2:3\"").is_ok());
    assert!(parse("set gradient off").unwrap().gradient.is_empty());
    for bad in [
        "nope",
        "\"#ff0000\"",
        "\"1:2:3:4:5:6:7:8:9\"",
        "\"#ff0000:#zz0000\"",
        // `:` is the only separator
        "\"#ff0000,#0000ff\"",
        "\"1:2,3\"",
    ] {
        let e = parse(&format!("set gradient {bad}")).unwrap_err();
        assert!(e[0].message.contains("invalid gradient"), "{bad}: {e:?}");
    }
}

#[test]
fn depth_16_and_none_on_a_full_line() {
    let conf = "set skin powerline\nset left \"#{a} │ #{b[fg=#123456]}\"";
    let sixteen = render(&format!("set color-depth 16\n{conf}"));
    assert!(
        !sixteen.contains("38;5;") && !sixteen.contains("48;5;"),
        "{sixteen:?}"
    );
    assert!(!sixteen.contains(";2;"), "{sixteen:?}");
    let none = render(&format!("set color-depth none\n{conf}\nset gradient ember"));
    // the plain 256-colour line without its colours
    let plain = conf.replace("set skin powerline\n", "");
    assert_eq!(none, hop_bar::style::strip_colors(&render(&plain)));
    assert!(
        none.contains("\x1b]8;;https://example.com/x"),
        "links stay: {none:?}"
    );
    for code in ["\x1b[3", "\x1b[4", "\x1b[9", "\x1b[10", ";5;", ";2;"] {
        assert!(!none.contains(code), "{code:?} in {none:?}");
    }
}

#[test]
fn backgrounds_that_quantise_alike_join_like_one() {
    let conf =
        "set skin powerline\nset skin-glyphs ascii\nset left \"#{x[bg=235]} │ #{y[bg=236]}\"";
    let at16 = strip_codes(&render(&format!("set color-depth 16\n{conf}")));
    assert_eq!(at16.trim(), "x | y >", "{at16:?}");
    let at256 = strip_codes(&render(conf));
    assert_eq!(at256.trim(), "x > y >", "{at256:?}");
}

#[test]
fn depth_none_draws_skins_as_plain() {
    let body = "set color-depth none\nset left \"#{x} │ #{d}\"\nset right \"#{a}\"\n";
    let plain = render(body);
    assert!(plain.contains(" │ "), "{plain:?}");
    for skin in ["powerline", "capsule"] {
        for glyphs in ["nerd", "ascii"] {
            let out = render(&format!(
                "set skin {skin}\nset skin-glyphs {glyphs}\n{body}"
            ));
            assert_eq!(out, plain, "{skin} {glyphs}");
        }
    }
    // other depths keep the skin
    let out = render(&format!(
        "set skin powerline\n{}",
        body.replace("none", "16")
    ));
    assert_ne!(strip_codes(&out), strip_codes(&plain));
}
