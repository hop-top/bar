//! Options on every placeholder that style its value: `bg`, `fg`, `bold`
//! and `gradient`. bar applies them; placeholders and plugins never see them.

use crate::color::Color;
use crate::conf::Config;
use crate::gradient::{parse_stops, recolor, Extent, Gradient, Ink};
use crate::options::Options;
use crate::style::{accent, muted};

/// The styling option keys.
pub const KEYS: [&str; 4] = ["bg", "fg", "bold", "gradient"];

/// A colour value that may name a role the palette fills in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorSpec {
    Fixed(Color),
    Good,
    Warn,
    Bad,
    Muted,
    Accent,
}

impl ColorSpec {
    /// A `Color` value, or `good`, `warn`, `bad`, `muted`, `accent`.
    pub fn parse(s: &str) -> Option<ColorSpec> {
        Some(match s.trim() {
            "good" => ColorSpec::Good,
            "warn" => ColorSpec::Warn,
            "bad" => ColorSpec::Bad,
            "muted" => ColorSpec::Muted,
            "accent" => ColorSpec::Accent,
            other => ColorSpec::Fixed(Color::parse(other)?),
        })
    }

    pub fn resolve(self, config: &Config) -> Color {
        match self {
            ColorSpec::Fixed(c) => c,
            ColorSpec::Good => config.colors.good,
            ColorSpec::Warn => config.colors.warn,
            ColorSpec::Bad => config.colors.bad,
            ColorSpec::Muted => muted(),
            ColorSpec::Accent => accent().into(),
        }
    }
}

/// Stops of a gradient value: a preset, or colours split by `:`, roles
/// included.
fn gradient_stops(s: &str, config: &Config) -> Option<Vec<Color>> {
    parse_stops(s).or_else(|| {
        let stops: Option<Vec<Color>> = s
            .split(':')
            .map(|p| ColorSpec::parse(p).map(|c| c.resolve(config)))
            .collect();
        stops.filter(|v| (2..=crate::gradient::MAX_STOPS).contains(&v.len()))
    })
}

/// Whether `s` is a valid gradient value (`set gradient`, `gradient=`).
pub fn valid_gradient(s: &str) -> bool {
    gradient_stops(s, &Config::default()).is_some()
}

fn on(v: &str) -> Option<bool> {
    match v.trim() {
        "" | "on" | "1" | "true" | "yes" => Some(true),
        "off" | "0" | "false" | "no" => Some(false),
        _ => None,
    }
}

/// Why a styling option's value is invalid; `None` when it is valid.
pub fn invalid(key: &str, v: &str) -> Option<String> {
    let ok = match key {
        "bg" | "fg" => ColorSpec::parse(v).is_some(),
        "bold" => on(v).is_some(),
        "gradient" => valid_gradient(v),
        _ => true,
    };
    (!ok).then(|| match key {
        "bg" | "fg" => {
            format!("invalid {key} `{v}` (0-255, #rrggbb, an ANSI name, good, warn, bad, muted or accent)")
        }
        "bold" => format!("invalid bold `{v}` (on|off)"),
        _ => format!(
            "invalid gradient `{v}` ({}, or 2-{} colours split by `:`)",
            crate::gradient::preset_names().collect::<Vec<_>>().join("|"),
            crate::gradient::MAX_STOPS
        ),
    })
}

/// A placeholder's styling, values resolved against the config. Bad values
/// are dropped.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Look {
    pub bg: Option<Color>,
    pub fg: Option<Color>,
    pub bold: bool,
    pub gradient: Option<Vec<Color>>,
}

impl Look {
    /// The styling options split off `opts`.
    pub fn split(opts: &Options, config: &Config) -> (Look, Options) {
        let mut look = Look::default();
        let mut rest = Options::default();
        for (k, v) in opts.iter() {
            match k.as_str() {
                "bg" => look.bg = ColorSpec::parse(v).map(|c| c.resolve(config)),
                "fg" => look.fg = ColorSpec::parse(v).map(|c| c.resolve(config)),
                "bold" => look.bold = on(v).unwrap_or(false),
                "gradient" => look.gradient = gradient_stops(v, config),
                _ => rest.set(k, v),
            }
        }
        (look, rest)
    }

    /// `text` in the foreground colour or gradient, and bold; a gradient
    /// beats `fg`. Private-use glyphs are `icon_width` cells.
    pub fn paint(&self, text: String, icon_width: usize) -> String {
        if text.is_empty() {
            return text;
        }
        match (&self.gradient, self.fg) {
            (Some(stops), _) => {
                let g = Gradient::new(stops);
                let ink = Ink::Ramp {
                    gradient: &g,
                    start: 0,
                    extent: Extent::of([text.as_str()], icon_width),
                    icon_width,
                };
                recolor(&text, Some(&ink), self.bold)
            }
            (None, Some(c)) => recolor(&text, Some(&Ink::Solid(c)), self.bold),
            (None, None) if self.bold => recolor(&text, None, true),
            (None, None) => text,
        }
    }
}

/// The `set gradient` stops; `None` when it is off.
pub fn line_gradient(config: &Config) -> Option<Gradient> {
    let s = config.gradient.as_str();
    if s.is_empty() || s == "off" {
        return None;
    }
    gradient_stops(s, config).map(|v| Gradient::new(&v))
}

/// The options every placeholder takes, plugins included, as JSON in the
/// `check::option_schema` shape: `icon` and the styling options.
pub fn schema() -> serde_json::Value {
    let presets: Vec<&str> = crate::gradient::preset_names().collect();
    serde_json::json!([
        {"key": "icon", "kind": "any", "values": [], "default": "",
         "help": "a glyph (at most 2 cells), or none to hide it"},
        {"key": "bg", "kind": "any", "values": [], "default": "",
         "help": "segment background under powerline or capsule: 0-255, #rrggbb or a name"},
        {"key": "fg", "kind": "any", "values": ["good", "warn", "bad", "muted", "accent"], "default": "",
         "help": "text colour: 0-255, #rrggbb, an ANSI name or a palette role"},
        {"key": "bold", "kind": "one", "values": ["on", "off"], "default": "off",
         "help": "bold text"},
        {"key": "gradient", "kind": "any", "values": presets, "default": "",
         "help": "colour the text along a preset or from:to colours; beats fg"},
    ])
}
