//! `set` directives: one key and value each, plus `set price` and the
//! palette fill-in applied after the whole file is read.

use super::{
    code, duration, layout_key, Config, Icons, ProjectMode, Skin, SkinGlyphs, Wrap, MAX_LINES,
};
use crate::color::{Color, ColorDepth};
use crate::compact::CompactAt;
use crate::look;
use crate::palette;
use crate::prices::Price;

/// The source string a layout key sets.
fn group_slot<'a>(c: &'a mut Config, group: &str, line: usize) -> &'a mut String {
    if line == 1 {
        return match group {
            "left" => &mut c.left,
            "center" => &mut c.center,
            _ => &mut c.right,
        };
    }
    if c.more_lines.len() < line - 1 {
        c.more_lines.resize(line - 1, Default::default());
    }
    let l = &mut c.more_lines[line - 2];
    match group {
        "left" => &mut l.left,
        "center" => &mut l.center,
        _ => &mut l.right,
    }
}

pub(super) fn apply_set(c: &mut Config, key: &str, val: &str) -> Result<(), String> {
    if let Some(k) = layout_key(key) {
        let (group, line) = k?;
        *group_slot(c, group, line) = val.into();
        return Ok(());
    }
    match key {
        "subagent" => c.subagent = Some(val.into()),
        "subagent-hide" => c.subagent_hide = statuses(val)?,
        "separator" => c.separator = val.into(),
        "gap" => c.gap = val.parse().map_err(|_| format!("invalid gap `{val}`"))?,
        "wrap" => {
            c.wrap = match val {
                // `auto` predates `on`/`off`: today's layout
                "off" | "auto" => Wrap::Off,
                "on" => Wrap::On,
                _ => return Err(format!("invalid wrap `{val}` (off|on)")),
            }
        }
        "max-rows" => {
            c.max_rows = match val.parse::<usize>() {
                Ok(n) if (1..=MAX_LINES).contains(&n) => n,
                _ => return Err(format!("invalid max-rows `{val}` (1-{MAX_LINES})")),
            }
        }
        "refresh" => c.refresh = duration(val)?,
        "icons" => {
            c.icons = match val {
                "none" => Icons::None,
                "nerdfont" => Icons::Nerdfont,
                "emoji" => Icons::Emoji,
                _ => return Err(format!("invalid icons `{val}` (none|nerdfont|emoji)")),
            }
        }
        "icon-width" => {
            c.icon_width = match val {
                "1" => 1,
                "2" => 2,
                _ => return Err(format!("invalid icon-width `{val}` (1|2)")),
            }
        }
        "claude-dir" => c.claude_dir = Some(val.into()),
        "compact-at" => {
            c.compact_at = Some(CompactAt::parse(val).ok_or_else(|| {
                format!("invalid compact-at `{val}` (tokens like 200k, or 1%..100%)")
            })?)
        }
        "update-check" => {
            c.update_check = match val {
                "on" => true,
                "off" => false,
                _ => return Err(format!("invalid update-check `{val}` (on|off)")),
            }
        }
        "service-status" => {
            c.service_status = match val {
                "on" => true,
                "off" => false,
                _ => return Err(format!("invalid service-status `{val}` (on|off)")),
            }
        }
        "project-config" => {
            c.project_config = ProjectMode::parse(val).ok_or_else(|| {
                format!(
                    "invalid project-config `{val}` ({})",
                    ProjectMode::NAMES.join("|")
                )
            })?
        }
        "color.good" => c.colors.good = code(val)?,
        "color.warn" => c.colors.warn = code(val)?,
        "color.bad" => c.colors.bad = code(val)?,
        "skin" => {
            c.skin = match val {
                "plain" => Skin::Plain,
                "powerline" => Skin::Powerline,
                "capsule" => Skin::Capsule,
                _ => return Err(format!("invalid skin `{val}` (plain|powerline|capsule)")),
            }
        }
        "skin-glyphs" => {
            c.skin_glyphs = match val {
                "nerd" => SkinGlyphs::Nerd,
                "ascii" => SkinGlyphs::Ascii,
                _ => return Err(format!("invalid skin-glyphs `{val}` (nerd|ascii)")),
            }
        }
        "palette" => {
            c.palette = palette::by_name(val)
                .ok_or_else(|| format!("invalid palette `{val}` ({})", palette::NAMES.join("|")))?
                .name;
        }
        "color-depth" => {
            c.color_depth = ColorDepth::parse(val).ok_or_else(|| {
                format!(
                    "invalid color-depth `{val}` ({})",
                    ColorDepth::NAMES.join("|")
                )
            })?
        }
        "gradient" => {
            if val != "off" && !look::valid_gradient(val) {
                return Err(look::invalid("gradient", val).unwrap_or_default());
            }
            c.gradient = if val == "off" {
                String::new()
            } else {
                val.into()
            };
        }
        "ramp" => {
            let codes: Result<Vec<Color>, _> = val.split_whitespace().map(code).collect();
            c.ramp = codes?
                .try_into()
                .map_err(|_| "ramp needs exactly 11 codes".to_string())?;
        }
        _ => return Err(format!("unknown set key `{key}`")),
    }
    Ok(())
}

/// `completed,failed`: a comma list of statuses, lower-cased; empty for
/// an empty value.
fn statuses(val: &str) -> Result<Vec<String>, String> {
    if val.trim().is_empty() {
        return Ok(Vec::new());
    }
    val.split(',')
        .map(|s| {
            let s = s.trim().to_ascii_lowercase();
            let ok = !s.is_empty() && s.bytes().all(|b| b.is_ascii_lowercase() || b == b'_');
            ok.then_some(s)
                .ok_or_else(|| format!("invalid subagent-hide `{val}` (comma list of statuses)"))
        })
        .collect()
}

pub(super) fn apply_price(c: &mut Config, a: &[String]) -> Result<(), String> {
    const USAGE: &str = "usage: set price <model-prefix> <in> <out> <cache_write> <cache_read>";
    let [prefix, i, o, w, r] = a else {
        return Err(USAGE.into());
    };
    let p = Price::parse(prefix, i, o, w, r)
        .ok_or("price rates are dollars per million tokens, non-negative numbers")?;
    c.prices.push(p);
    Ok(())
}

/// Palette values for every colour setting the file did not set itself,
/// wherever in the file `set palette` appears.
pub(super) fn apply_palette(c: &mut Config, explicit: &[String]) {
    let p = palette::by_name(c.palette).unwrap_or_else(palette::default);
    let unset = |k: &str| !explicit.iter().any(|e| e == k);
    c.muted = p.muted;
    c.bgs = p.bgs;
    if unset("color.good") {
        c.colors.good = p.good.into();
    }
    if unset("color.warn") {
        c.colors.warn = p.warn.into();
    }
    if unset("color.bad") {
        c.colors.bad = p.bad.into();
    }
    if unset("ramp") {
        c.ramp = p.ramp.map(Color::Idx);
    }
}
