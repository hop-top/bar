use hop_bar::conf::{parse, LoadResult};
use hop_bar::palette;
use hop_bar::style::{bg_code, fg, use_ansi};

fn preview(conf: &str, cols: usize) -> String {
    let c = parse(conf).unwrap_or_else(|e| panic!("{conf}: {e:?}"));
    hop_bar::preview::render("early", LoadResult::Ok(c), Some(cols), 0)
}

#[test]
fn terminal_palette_is_ansi16_with_three_step_ramp() {
    let p = palette::by_name("terminal").unwrap();
    assert!(palette::NAMES.contains(&"terminal"));
    assert_eq!((p.muted, p.good, p.warn, p.bad), (8, 2, 3, 1));
    assert_eq!(p.ramp, [1, 1, 1, 3, 3, 3, 3, 2, 2, 2, 2]);
    assert!(p.bgs.iter().all(|&b| b < 16 && b != p.muted));
    let c = parse("set palette terminal").unwrap();
    assert_eq!(
        (c.muted, c.ramp, c.bgs),
        (p.muted, p.ramp.map(Into::into), p.bgs)
    );
}

#[test]
fn other_palettes_stay_256_colour() {
    for p in palette::PALETTES.iter().filter(|p| p.name != "terminal") {
        assert!(!p.ansi, "{}", p.name);
    }
}

#[test]
fn sgr_codes_for_ansi_mode() {
    let _g = use_ansi(true);
    assert_eq!(fg(8, "x"), "\x1b[90mx\x1b[39m");
    assert_eq!(fg(1, "x"), "\x1b[31mx\x1b[39m");
    assert_eq!(fg(7, "x"), "\x1b[37mx\x1b[39m");
    assert_eq!(fg(15, "x"), "\x1b[97mx\x1b[39m");
    assert_eq!(bg_code(0), "\x1b[40m");
    assert_eq!(bg_code(4), "\x1b[44m");
    assert_eq!(bg_code(8), "\x1b[100m");
    assert_eq!(bg_code(15), "\x1b[107m");
    // codes outside the 16 keep the 256-colour form
    assert_eq!(fg(114, "x"), "\x1b[38;5;114mx\x1b[39m");
    assert_eq!(bg_code(200), "\x1b[48;5;200m");
}

#[test]
fn ansi_mode_is_off_by_default_and_restored() {
    assert_eq!(fg(1, "x"), "\x1b[38;5;1mx\x1b[39m");
    {
        let _g = use_ansi(true);
    }
    assert_eq!(bg_code(4), "\x1b[48;5;4m");
}

#[test]
fn terminal_preview_has_no_256_colour_codes_for_state_colours() {
    let out = preview(
        "set palette terminal\nset left \"#{usage} #{usage[type=7d]} #{ctx}\"\n",
        120,
    );
    assert!(out.contains("\x1b[90m"), "{out:?}");
    assert!(
        out.contains("\x1b[31m") || out.contains("\x1b[32m") || out.contains("\x1b[33m"),
        "{out:?}"
    );
    assert!(!out.contains("38;5;"), "{out:?}");
}

#[test]
fn terminal_skins_use_40_47_and_100_107_backgrounds() {
    let p = palette::by_name("terminal").unwrap();
    let left = "#{model} │ #{ctx} │ #{repo} │ #{account} │ #{effort}";
    for skin in ["powerline", "capsule"] {
        let out = preview(
            &format!("set skin {skin}\nset palette terminal\nset left \"{left}\"\n"),
            200,
        );
        assert!(!out.contains("48;5;") && !out.contains("38;5;"), "{out:?}");
        for &b in p.bgs {
            let sgr = if b < 8 {
                40 + b as u32
            } else {
                100 + b as u32 - 8
            };
            assert!(out.contains(&format!("\x1b[{sgr}m")), "{skin} {b}: {out:?}");
        }
    }
}

#[test]
fn default_palette_output_is_unchanged() {
    let conf = "set skin powerline\nset left \"#{model} │ #{usage}\"\n";
    let a = preview(conf, 120);
    let b = preview(&format!("{conf}set palette default\n"), 120);
    assert_eq!(a, b);
    assert!(a.contains("48;5;238") && a.contains("38;5;250"), "{a:?}");
}
