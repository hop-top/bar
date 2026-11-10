use hop_bar::color::{nearest16, nearest256, Color, ColorDepth, Depth};
use hop_bar::conf::parse;
use hop_bar::style::{self, bg_code, fg, fg_code, strip_colors};

fn env<'a>(vars: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
    move |k| {
        vars.iter()
            .find(|(n, _)| *n == k)
            .map(|(_, v)| v.to_string())
    }
}

#[test]
fn colour_values_parse() {
    assert_eq!(Color::parse("75"), Some(Color::Idx(75)));
    assert_eq!(Color::parse("#ff8800"), Some(Color::Rgb([255, 136, 0])));
    assert_eq!(Color::parse("#FF8800"), Some(Color::Rgb([255, 136, 0])));
    assert_eq!(Color::parse("red"), Some(Color::Idx(1)));
    assert_eq!(Color::parse("bright-white"), Some(Color::Idx(15)));
    for bad in ["256", "-1", "#fff", "#gggggg", "pink", "", "#ff88001"] {
        assert_eq!(Color::parse(bad), None, "{bad}");
    }
    assert_eq!(Color::Rgb([255, 136, 0]).to_string(), "#ff8800");
    assert_eq!(Color::Idx(7).to_string(), "7");
}

#[test]
fn rgb_quantises_to_the_nearest_256_colour() {
    assert_eq!(nearest256([255, 0, 0]), 196);
    assert_eq!(nearest256([0, 0, 0]), 16);
    assert_eq!(nearest256([255, 255, 255]), 231);
    // exact cube and grey-ramp entries map to themselves
    assert_eq!(nearest256([95, 135, 175]), 67);
    assert_eq!(nearest256([128, 128, 128]), 244);
    assert_eq!(nearest256([255, 136, 0]), 208);
    for n in 16..=255u8 {
        assert_eq!(nearest256(Color::Idx(n).rgb()), n, "code {n}");
    }
}

#[test]
fn codes_quantise_to_the_nearest_of_16() {
    assert_eq!(nearest16(Color::Idx(196).rgb()), 9);
    assert_eq!(nearest16(Color::Idx(250).rgb()), 7);
    assert_eq!(nearest16(Color::Idx(16).rgb()), 0);
    assert_eq!(nearest16([0, 0, 238]), 4);
    assert_eq!(Color::Idx(3).at(Depth::Ansi16), Color::Idx(3));
    assert_eq!(Color::Idx(231).at(Depth::Ansi16), Color::Idx(15));
    assert_eq!(Color::Rgb([255, 0, 0]).at(Depth::Ansi256), Color::Idx(196));
    assert_eq!(
        Color::Rgb([255, 0, 0]).at(Depth::True),
        Color::Rgb([255, 0, 0])
    );
    assert_eq!(Color::Idx(196).at(Depth::True), Color::Idx(196));
}

#[test]
#[allow(clippy::cognitive_complexity)] // over the threshold; split when next touched
fn auto_depth_follows_the_environment() {
    let auto = |vars: &[(&str, &str)]| ColorDepth::Auto.resolve(env(vars));
    assert_eq!(auto(&[]), Depth::Ansi256);
    assert_eq!(auto(&[("COLORTERM", "truecolor")]), Depth::True);
    assert_eq!(auto(&[("COLORTERM", "24bit")]), Depth::True);
    assert_eq!(auto(&[("COLORTERM", "yes")]), Depth::Ansi256);
    assert_eq!(auto(&[("NO_COLOR", "1")]), Depth::None);
    assert_eq!(
        auto(&[("NO_COLOR", "")]),
        Depth::Ansi256,
        "empty NO_COLOR is unset"
    );
    assert_eq!(auto(&[("FORCE_COLOR", "0")]), Depth::None);
    assert_eq!(auto(&[("FORCE_COLOR", "false")]), Depth::None);
    assert_eq!(
        auto(&[("FORCE_COLOR", "1"), ("NO_COLOR", "1")]),
        Depth::Ansi16
    );
    // FORCE_COLOR is a floor: it never lowers what the terminal reports
    assert_eq!(
        auto(&[("FORCE_COLOR", "2"), ("COLORTERM", "truecolor")]),
        Depth::True
    );
    assert_eq!(auto(&[("FORCE_COLOR", "1")]), Depth::Ansi256);
    assert_eq!(
        auto(&[("FORCE_COLOR", "2"), ("NO_COLOR", "1")]),
        Depth::Ansi256
    );
    // TERM=dumb: no colour unless forced
    assert_eq!(
        auto(&[("TERM", "dumb"), ("COLORTERM", "truecolor")]),
        Depth::None
    );
    assert_eq!(
        auto(&[("TERM", "dumb"), ("FORCE_COLOR", "1")]),
        Depth::Ansi16
    );
    assert_eq!(auto(&[("TERM", "xterm")]), Depth::Ansi256);
    assert_eq!(auto(&[("FORCE_COLOR", "3")]), Depth::True);
    assert_eq!(
        auto(&[
            ("FORCE_COLOR", "true"),
            ("NO_COLOR", "1"),
            ("COLORTERM", "truecolor")
        ]),
        Depth::True
    );
    assert_eq!(auto(&[("FORCE_COLOR", "true")]), Depth::Ansi256);
    // a fixed depth ignores the environment
    let fixed = ColorDepth::Fixed(Depth::Ansi256);
    assert_eq!(fixed.resolve(env(&[("NO_COLOR", "1")])), Depth::Ansi256);
    assert_eq!(fixed.resolve(env(&[("TERM", "dumb")])), Depth::Ansi256);
}

#[test]
fn unset_depth_is_256_unless_colour_is_off() {
    let unset = |vars: &[(&str, &str)]| ColorDepth::default().resolve(env(vars));
    assert_eq!(unset(&[]), Depth::Ansi256);
    // never above 256 on its own
    assert_eq!(unset(&[("COLORTERM", "truecolor")]), Depth::Ansi256);
    assert_eq!(unset(&[("FORCE_COLOR", "3")]), Depth::Ansi256);
    // the environment turns colour off
    assert_eq!(unset(&[("NO_COLOR", "1")]), Depth::None);
    assert_eq!(unset(&[("NO_COLOR", "")]), Depth::Ansi256);
    assert_eq!(unset(&[("TERM", "dumb")]), Depth::None);
    assert_eq!(unset(&[("FORCE_COLOR", "0")]), Depth::None);
    // and FORCE_COLOR back on
    assert_eq!(
        unset(&[("NO_COLOR", "1"), ("FORCE_COLOR", "1")]),
        Depth::Ansi16
    );
    assert_eq!(
        unset(&[("TERM", "dumb"), ("FORCE_COLOR", "2")]),
        Depth::Ansi256
    );
}

#[test]
fn color_depth_key_parses() {
    assert_eq!(parse("").unwrap().color_depth, ColorDepth::default());
    assert_ne!(ColorDepth::default(), ColorDepth::Fixed(Depth::Ansi256));
    assert_eq!(ColorDepth::default().name(), "256");
    for (v, want) in [
        ("auto", ColorDepth::Auto),
        ("none", ColorDepth::Fixed(Depth::None)),
        ("16", ColorDepth::Fixed(Depth::Ansi16)),
        ("256", ColorDepth::Fixed(Depth::Ansi256)),
        ("truecolor", ColorDepth::Fixed(Depth::True)),
    ] {
        let c = parse(&format!("set color-depth {v}")).unwrap();
        assert_eq!(c.color_depth, want);
        assert_eq!(want.name(), v);
    }
    let e = parse("set color-depth 24bit").unwrap_err();
    assert!(e[0].message.contains("invalid color-depth"), "{e:?}");
}

#[test]
fn hex_values_in_colour_settings() {
    let c = parse(
        "set color.good \"#a3be8c\"\nset color.bad red\nset ramp \"#ff0000 1 2 3 4 5 6 7 8 9 #00ff00\"\naccount \"*\" work \"#5e81ac\"\n",
    )
    .unwrap();
    assert_eq!(c.colors.good, Color::Rgb([0xa3, 0xbe, 0x8c]));
    assert_eq!(c.colors.bad, Color::Idx(1));
    assert_eq!(c.ramp[0], Color::Rgb([255, 0, 0]));
    assert_eq!(c.ramp[10], Color::Rgb([0, 255, 0]));
    assert_eq!(c.accounts[0].color, Color::Rgb([0x5e, 0x81, 0xac]));
    let e = parse("set color.good \"#abc\"").unwrap_err();
    assert!(e[0].message.contains("invalid colour"), "{e:?}");
}

#[test]
fn sgr_at_each_depth() {
    // the default depth draws codes exactly as before
    assert_eq!(fg_code(196), "\x1b[38;5;196m");
    assert_eq!(bg_code(24), "\x1b[48;5;24m");
    assert_eq!(fg_code(Color::Rgb([255, 136, 0])), "\x1b[38;5;208m");
    {
        let _d = style::use_depth(Depth::True);
        assert_eq!(fg_code(Color::Rgb([255, 136, 0])), "\x1b[38;2;255;136;0m");
        assert_eq!(bg_code(Color::Rgb([1, 2, 3])), "\x1b[48;2;1;2;3m");
        assert_eq!(fg_code(196), "\x1b[38;5;196m");
    }
    {
        let _d = style::use_depth(Depth::Ansi16);
        assert_eq!(fg_code(196), "\x1b[91m");
        assert_eq!(fg_code(2), "\x1b[32m");
        assert_eq!(bg_code(250), "\x1b[47m");
        assert_eq!(fg_code(Color::Rgb([0, 0, 0])), "\x1b[30m");
    }
    assert_eq!(style::depth(), Depth::Ansi256, "guards restore the depth");
}

#[test]
fn no_color_strips_colours_only() {
    let s = format!(
        "\x1b[1;38;5;196mX\x1b[39;22m{}",
        fg(Color::Rgb([1, 2, 3]), "y")
    );
    assert_eq!(strip_colors(&s), "\x1b[1mX\x1b[22my");
    assert_eq!(strip_colors("\x1b[0ma\x1b[m"), "\x1b[0ma\x1b[m");
    assert_eq!(strip_colors("\x1b[48;5;24m\x1b[91ma\x1b[49m"), "a");
    let link = "\x1b]8;;https://example.com\x1b\\t\x1b]8;;\x1b\\";
    assert_eq!(strip_colors(link), link);
    assert_eq!(strip_colors("\x1b[2Ka"), "\x1b[2Ka");
    // colon-form colours (ITU T.416) go too
    assert_eq!(strip_colors("\x1b[38:5:203ma"), "a");
    assert_eq!(strip_colors("\x1b[1;38:2::255:0:0;48:5:24mb"), "\x1b[1mb");
    assert_eq!(strip_colors("\x1b[58:2::1:2:3;4mc"), "\x1b[4mc");
    assert_eq!(strip_colors("\x1b[4:3md"), "\x1b[4:3md");
}
