//! Named colour palettes: muted text, state colours, usage ramp and the
//! background list skins cycle through. 256-colour codes, except `terminal`:
//! codes 0-15 there are drawn as the ANSI 16-colour SGR, so the terminal
//! theme decides the actual colours.

pub struct Palette {
    pub name: &'static str,
    pub muted: u8,
    pub good: u8,
    pub warn: u8,
    pub bad: u8,
    pub ramp: [u8; 11],
    pub bgs: &'static [u8],
    /// Codes 0-15 are drawn as SGR 30-37/90-97 (fg) and 40-47/100-107 (bg).
    pub ansi: bool,
}

/// Ramp of a non-default palette: its own bad, warn and good, low to high.
const fn ramp(bad: u8, warn: u8, good: u8) -> [u8; 11] {
    [
        bad, bad, bad, warn, warn, warn, warn, good, good, good, good,
    ]
}

pub const PALETTES: [Palette; 7] = [
    Palette {
        name: "default",
        muted: 250,
        good: 114,
        warn: 179,
        bad: 203,
        ramp: [196, 202, 208, 214, 220, 226, 190, 154, 118, 82, 46],
        bgs: &[238, 24, 236, 60, 23],
        ansi: false,
    },
    Palette {
        name: "nord",
        muted: 248,
        good: 144,
        warn: 222,
        bad: 167,
        ramp: ramp(167, 222, 144),
        bgs: &[237, 24, 239, 67, 60],
        ansi: false,
    },
    Palette {
        name: "gruvbox",
        muted: 246,
        good: 142,
        warn: 214,
        bad: 167,
        ramp: ramp(167, 214, 142),
        bgs: &[237, 239, 66, 241, 95],
        ansi: false,
    },
    Palette {
        name: "dracula",
        muted: 146,
        good: 84,
        warn: 228,
        bad: 203,
        ramp: ramp(203, 228, 84),
        bgs: &[236, 61, 238, 97, 239],
        ansi: false,
    },
    Palette {
        name: "solarized",
        muted: 245,
        good: 106,
        warn: 136,
        bad: 160,
        ramp: ramp(160, 136, 106),
        bgs: &[23, 235, 24, 30, 66],
        ansi: false,
    },
    Palette {
        name: "mono",
        muted: 245,
        good: 252,
        warn: 248,
        bad: 255,
        ramp: ramp(255, 248, 252),
        bgs: &[236, 238, 240, 242],
        ansi: false,
    },
    // black and blue only: ok/warn/bad text stays readable on both
    Palette {
        name: "terminal",
        muted: 8,
        good: 2,
        warn: 3,
        bad: 1,
        ramp: ramp(1, 3, 2),
        bgs: &[0, 4],
        ansi: true,
    },
];

pub const NAMES: [&str; 7] = [
    "default",
    "nord",
    "gruvbox",
    "dracula",
    "solarized",
    "mono",
    "terminal",
];

pub fn by_name(name: &str) -> Option<&'static Palette> {
    PALETTES.iter().find(|p| p.name == name)
}

pub fn default() -> &'static Palette {
    &PALETTES[0]
}
