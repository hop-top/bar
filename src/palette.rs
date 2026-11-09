//! Named colour palettes: muted text, state colours, usage ramp and the
//! background list skins cycle through. Built-in palettes use 256-colour
//! codes or `#rrggbb` values (drawn as the nearest 256-colour code below
//! truecolor), except `terminal`: codes 0-15 there are drawn as the ANSI
//! 16-colour SGR, so the terminal theme decides the actual colours. Users
//! add their own in `palettes/<name>.conf` beside `bar.conf` (see [`file`]).

use crate::color::Color;
use std::borrow::Cow;

pub mod file;

pub use file::{find, use_dir, DirGuard};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Palette {
    pub name: Cow<'static, str>,
    pub muted: Color,
    pub good: Color,
    pub warn: Color,
    pub bad: Color,
    pub ramp: [Color; 11],
    pub bgs: Cow<'static, [Color]>,
    /// Codes 0-15 are drawn as SGR 30-37/90-97 (fg) and 40-47/100-107 (bg).
    pub ansi: bool,
}

const fn idx(n: u8) -> Color {
    Color::Idx(n)
}

/// `0xrrggbb` as an RGB colour.
const fn rgb(v: u32) -> Color {
    Color::Rgb([(v >> 16) as u8, (v >> 8) as u8, v as u8])
}

/// Ramp of a non-default palette: its own bad, warn and good, low to high.
pub const fn ramp(bad: Color, warn: Color, good: Color) -> [Color; 11] {
    [
        bad, bad, bad, warn, warn, warn, warn, good, good, good, good,
    ]
}

pub static PALETTES: [Palette; 13] = [
    Palette {
        name: Cow::Borrowed("default"),
        muted: idx(250),
        good: idx(114),
        warn: idx(179),
        bad: idx(203),
        ramp: [
            idx(196),
            idx(202),
            idx(208),
            idx(214),
            idx(220),
            idx(226),
            idx(190),
            idx(154),
            idx(118),
            idx(82),
            idx(46),
        ],
        bgs: Cow::Borrowed(&[idx(238), idx(24), idx(236), idx(60), idx(23)]),
        ansi: false,
    },
    Palette {
        name: Cow::Borrowed("nord"),
        muted: idx(249),
        good: idx(144),
        warn: idx(222),
        bad: idx(210),
        ramp: ramp(idx(210), idx(222), idx(144)),
        bgs: Cow::Borrowed(&[idx(237), idx(24), idx(239), idx(23), idx(240)]),
        ansi: false,
    },
    Palette {
        name: Cow::Borrowed("gruvbox"),
        muted: idx(246),
        good: idx(142),
        warn: idx(214),
        bad: idx(167),
        ramp: ramp(idx(167), idx(214), idx(142)),
        bgs: Cow::Borrowed(&[idx(237), idx(239), idx(66), idx(241), idx(95)]),
        ansi: false,
    },
    Palette {
        name: Cow::Borrowed("dracula"),
        muted: idx(146),
        good: idx(84),
        warn: idx(228),
        bad: idx(203),
        ramp: ramp(idx(203), idx(228), idx(84)),
        bgs: Cow::Borrowed(&[idx(236), idx(61), idx(238), idx(97), idx(239)]),
        ansi: false,
    },
    Palette {
        name: Cow::Borrowed("solarized"),
        muted: idx(245),
        good: idx(106),
        warn: idx(136),
        bad: idx(160),
        ramp: ramp(idx(160), idx(136), idx(106)),
        bgs: Cow::Borrowed(&[idx(23), idx(235), idx(24), idx(30), idx(66)]),
        ansi: false,
    },
    Palette {
        name: Cow::Borrowed("mono"),
        muted: idx(245),
        good: idx(252),
        warn: idx(248),
        bad: idx(255),
        ramp: ramp(idx(255), idx(248), idx(252)),
        bgs: Cow::Borrowed(&[idx(236), idx(238), idx(240), idx(242)]),
        ansi: false,
    },
    Palette {
        name: Cow::Borrowed("tokyo-night"),
        muted: rgb(0xa9b1d6),
        good: rgb(0x9ece6a),
        warn: rgb(0xe0af68),
        bad: rgb(0xf7768e),
        ramp: ramp(rgb(0xf7768e), rgb(0xe0af68), rgb(0x9ece6a)),
        bgs: Cow::Borrowed(&[
            rgb(0x24283b),
            rgb(0x1a1b26),
            rgb(0x394b70),
            rgb(0x292e42),
            rgb(0x16161e),
        ]),
        ansi: false,
    },
    Palette {
        name: Cow::Borrowed("rose-pine"),
        muted: rgb(0x908caa),
        good: rgb(0x9ccfd8),
        warn: rgb(0xf6c177),
        bad: rgb(0xeb6f92),
        ramp: ramp(rgb(0xeb6f92), rgb(0xf6c177), rgb(0x9ccfd8)),
        bgs: Cow::Borrowed(&[rgb(0x1f1d2e), rgb(0x26233a), rgb(0x191724)]),
        ansi: false,
    },
    Palette {
        name: Cow::Borrowed("catppuccin"),
        muted: rgb(0xa6adc8),
        good: rgb(0xa6e3a1),
        warn: rgb(0xf9e2af),
        bad: rgb(0xf38ba8),
        ramp: ramp(rgb(0xf38ba8), rgb(0xf9e2af), rgb(0xa6e3a1)),
        bgs: Cow::Borrowed(&[
            rgb(0x313244),
            rgb(0x1e1e2e),
            rgb(0x45475a),
            rgb(0x181825),
            rgb(0x11111b),
        ]),
        ansi: false,
    },
    Palette {
        name: Cow::Borrowed("one-dark"),
        muted: rgb(0x828997),
        good: rgb(0x98c379),
        warn: rgb(0xe5c07b),
        bad: rgb(0xe06c75),
        ramp: ramp(rgb(0xe06c75), rgb(0xe5c07b), rgb(0x98c379)),
        bgs: Cow::Borrowed(&[rgb(0x282c34), rgb(0x31363f), rgb(0x21252b)]),
        ansi: false,
    },
    Palette {
        name: Cow::Borrowed("everforest"),
        muted: rgb(0x9da9a0),
        good: rgb(0xa7c080),
        warn: rgb(0xdbbc7f),
        bad: rgb(0xe67e80),
        ramp: ramp(rgb(0xe67e80), rgb(0xdbbc7f), rgb(0xa7c080)),
        bgs: Cow::Borrowed(&[
            rgb(0x343f44),
            rgb(0x2d353b),
            rgb(0x3a515d),
            rgb(0x232a2e),
            rgb(0x425047),
        ]),
        ansi: false,
    },
    Palette {
        name: Cow::Borrowed("kanagawa"),
        muted: rgb(0x938aa9),
        good: rgb(0x98bb6c),
        warn: rgb(0xe6c384),
        bad: rgb(0xe46876),
        ramp: ramp(rgb(0xe46876), rgb(0xe6c384), rgb(0x98bb6c)),
        bgs: Cow::Borrowed(&[
            rgb(0x1f1f28),
            rgb(0x2a2a37),
            rgb(0x16161d),
            rgb(0x363646),
            rgb(0x223249),
        ]),
        ansi: false,
    },
    // black and blue only: ok/warn/bad text stays readable on both
    Palette {
        name: Cow::Borrowed("terminal"),
        muted: idx(8),
        good: idx(2),
        warn: idx(3),
        bad: idx(1),
        ramp: ramp(idx(1), idx(3), idx(2)),
        bgs: Cow::Borrowed(&[idx(0), idx(4)]),
        ansi: true,
    },
];

pub const NAMES: [&str; 13] = [
    "default",
    "nord",
    "gruvbox",
    "dracula",
    "solarized",
    "mono",
    "tokyo-night",
    "rose-pine",
    "catppuccin",
    "one-dark",
    "everforest",
    "kanagawa",
    "terminal",
];

/// The built-in palette called `name`.
pub fn by_name(name: &str) -> Option<&'static Palette> {
    PALETTES.iter().find(|p| p.name == name)
}

pub fn default() -> &'static Palette {
    &PALETTES[0]
}
