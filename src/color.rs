//! Colour values, output depth and quantising: a 256-colour code or an
//! RGB value, drawn at the depth the terminal takes (`set color-depth`).

/// A colour as configured: a 256-colour code or `#rrggbb`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Color {
    Idx(u8),
    Rgb([u8; 3]),
}

impl From<u8> for Color {
    fn from(n: u8) -> Color {
        Color::Idx(n)
    }
}

impl PartialEq<u8> for Color {
    fn eq(&self, n: &u8) -> bool {
        *self == Color::Idx(*n)
    }
}

/// As written in a config: the code, or `#rrggbb`.
impl std::fmt::Display for Color {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Color::Idx(n) => write!(f, "{n}"),
            Color::Rgb([r, g, b]) => write!(f, "#{r:02x}{g:02x}{b:02x}"),
        }
    }
}

/// How many colours the output uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Depth {
    /// No colour at all (`NO_COLOR`): SGR colours are left out.
    None,
    Ansi16,
    Ansi256,
    True,
}

/// `set color-depth`: a fixed depth, or `auto` from the environment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorDepth {
    Auto,
    Fixed(Depth),
}

impl Default for ColorDepth {
    fn default() -> Self {
        ColorDepth::Fixed(Depth::Ansi256)
    }
}

impl ColorDepth {
    pub const NAMES: [&'static str; 5] = ["auto", "none", "16", "256", "truecolor"];

    pub fn parse(s: &str) -> Option<ColorDepth> {
        Some(match s {
            "auto" => ColorDepth::Auto,
            "none" => ColorDepth::Fixed(Depth::None),
            "16" => ColorDepth::Fixed(Depth::Ansi16),
            "256" => ColorDepth::Fixed(Depth::Ansi256),
            "truecolor" => ColorDepth::Fixed(Depth::True),
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            ColorDepth::Auto => "auto",
            ColorDepth::Fixed(Depth::None) => "none",
            ColorDepth::Fixed(Depth::Ansi16) => "16",
            ColorDepth::Fixed(Depth::Ansi256) => "256",
            ColorDepth::Fixed(Depth::True) => "truecolor",
        }
    }

    /// The depth to draw at; `auto` reads `env`.
    pub fn resolve(self, env: impl Fn(&str) -> Option<String>) -> Depth {
        match self {
            ColorDepth::Fixed(d) => d,
            ColorDepth::Auto => detect(env),
        }
    }
}

/// `FORCE_COLOR` (`0`/`false` off, `1` 16, `2` 256, `3` truecolor, any
/// other value on) beats `NO_COLOR`; otherwise `COLORTERM` `truecolor` or
/// `24bit` gives truecolor, anything else 256.
fn detect(env: impl Fn(&str) -> Option<String>) -> Depth {
    let set = |k: &str| env(k).filter(|v| !v.is_empty());
    let rich = || match set("COLORTERM").as_deref() {
        Some("truecolor" | "24bit") => Depth::True,
        _ => Depth::Ansi256,
    };
    match set("FORCE_COLOR").as_deref() {
        Some("0" | "false") => Depth::None,
        Some("1") => Depth::Ansi16,
        Some("2") => Depth::Ansi256,
        Some("3") => Depth::True,
        Some(_) => rich(),
        None if set("NO_COLOR").is_some() => Depth::None,
        None => rich(),
    }
}

/// The 16 ANSI colours as xterm draws them by default; terminal themes
/// change them, so they only guide quantising and gradients.
const ANSI: [[u8; 3]; 16] = [
    [0, 0, 0],
    [205, 0, 0],
    [0, 205, 0],
    [205, 205, 0],
    [0, 0, 238],
    [205, 0, 205],
    [0, 205, 205],
    [229, 229, 229],
    [127, 127, 127],
    [255, 0, 0],
    [0, 255, 0],
    [255, 255, 0],
    [92, 92, 255],
    [255, 0, 255],
    [0, 255, 255],
    [255, 255, 255],
];

const NAMES: [&str; 16] = [
    "black",
    "red",
    "green",
    "yellow",
    "blue",
    "magenta",
    "cyan",
    "white",
    "bright-black",
    "bright-red",
    "bright-green",
    "bright-yellow",
    "bright-blue",
    "bright-magenta",
    "bright-cyan",
    "bright-white",
];

const CUBE: [u8; 6] = [0, 95, 135, 175, 215, 255];

impl Color {
    /// `0`-`255`, `#rrggbb`, or an ANSI name (`red`, `bright-blue`, ...).
    pub fn parse(s: &str) -> Option<Color> {
        let s = s.trim();
        if let Some(hex) = s.strip_prefix('#') {
            if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
                return None;
            }
            let ch = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
            return Some(Color::Rgb([ch(0)?, ch(2)?, ch(4)?]));
        }
        if let Ok(n) = s.parse::<u8>() {
            return Some(Color::Idx(n));
        }
        let n = NAMES.iter().position(|&n| n == s)?;
        Some(Color::Idx(n as u8))
    }

    /// The colour's RGB value; codes 0-15 as xterm draws them.
    pub fn rgb(self) -> [u8; 3] {
        match self {
            Color::Rgb(c) => c,
            Color::Idx(n) if n < 16 => ANSI[n as usize],
            Color::Idx(n) if n < 232 => {
                let i = n - 16;
                [
                    CUBE[(i / 36) as usize],
                    CUBE[(i / 6 % 6) as usize],
                    CUBE[(i % 6) as usize],
                ]
            }
            Color::Idx(n) => {
                let v = 8 + 10 * (n - 232);
                [v, v, v]
            }
        }
    }

    /// The colour as drawn at `depth`: RGB becomes a 256-colour code below
    /// truecolor, and every code one of the 16 at `Ansi16`.
    pub fn at(self, depth: Depth) -> Color {
        match (self, depth) {
            (Color::Idx(n), Depth::Ansi16) if n >= 16 => Color::Idx(nearest16(self.rgb())),
            (Color::Rgb(c), Depth::Ansi16) => Color::Idx(nearest16(c)),
            (Color::Rgb(c), Depth::Ansi256 | Depth::None) => Color::Idx(nearest256(c)),
            _ => self,
        }
    }
}

/// Squared "redmean" distance: cheap and close to how far apart two
/// colours look.
fn distance(a: [u8; 3], b: [u8; 3]) -> i32 {
    let r = (a[0] as i32 + b[0] as i32) / 2;
    let d = |i: usize| a[i] as i32 - b[i] as i32;
    (((512 + r) * d(0) * d(0)) >> 8) + 4 * d(1) * d(1) + (((767 - r) * d(2) * d(2)) >> 8)
}

/// Nearest of the 16 ANSI colours.
pub fn nearest16(c: [u8; 3]) -> u8 {
    (0..16u8)
        .min_by_key(|&i| distance(c, ANSI[i as usize]))
        .unwrap_or(0)
}

/// Nearest 256-colour code from the colour cube (16-231) or the grey ramp
/// (232-255); the 16 theme colours are never picked.
pub fn nearest256(c: [u8; 3]) -> u8 {
    let level = |v: u8| match v {
        0..=47 => 0,
        48..=114 => 1,
        _ => (v - 35) / 40,
    };
    let (r, g, b) = (level(c[0]), level(c[1]), level(c[2]));
    let cube = 16 + 36 * r + 6 * g + b;
    let avg = (c[0] as u16 + c[1] as u16 + c[2] as u16) / 3;
    let grey = 232 + (avg.saturating_sub(3) / 10).min(23) as u8;
    if distance(c, Color::Idx(grey).rgb()) < distance(c, Color::Idx(cube).rgb()) {
        grey
    } else {
        cube
    }
}
