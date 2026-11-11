//! The tray glyph as raw RGBA, drawn in code so no image asset or decoder
//! is needed: three bars of unequal length on a transparent square. One
//! variant per taskbar theme, so the glyph stays legible when the theme
//! flips; a template variant (black on transparent) for hosts that tint
//! it themselves, with a dot in the corner when something went wrong.

pub const SIZE: u32 = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Variant {
    /// Dark glyph, for a light taskbar.
    Dark,
    /// Light glyph, for a dark taskbar.
    Light,
    /// Black on transparent; the host recolours it (macOS template image).
    Template,
    /// `Template` with a dot: the last action was refused or `bar` failed.
    TemplateBad,
}

impl Variant {
    /// The glyph that contrasts with the taskbar.
    pub fn for_taskbar(light_taskbar: bool) -> Variant {
        if light_taskbar {
            Variant::Dark
        } else {
            Variant::Light
        }
    }

    fn colour(self) -> [u8; 4] {
        match self {
            Variant::Dark => [0x20, 0x20, 0x20, 0xff],
            Variant::Light => [0xf2, 0xf2, 0xf2, 0xff],
            Variant::Template | Variant::TemplateBad => [0, 0, 0, 0xff],
        }
    }
}

/// Bars as `(top row, height, width)`; all start at column 6.
const BARS: [(u32, u32, u32); 3] = [(7, 5, 20), (14, 5, 14), (21, 5, 17)];
const LEFT: u32 = 6;
/// The dot of `TemplateBad`: centre and radius, with the gap around it.
const DOT: (f64, f64, f64) = (26.5, 5.5, 3.5);

/// `SIZE` x `SIZE` RGBA pixels, row-major.
pub fn rgba(v: Variant) -> Vec<u8> {
    rgba_scaled(v, 1)
}

/// The glyph at `scale` times [`SIZE`] per side, for hosts that show it
/// at `SIZE` points on a denser display.
pub fn rgba_scaled(v: Variant, scale: u32) -> Vec<u8> {
    let size = SIZE * scale;
    let mut px = vec![0u8; (size * size * 4) as usize];
    let c = v.colour();
    for (top, h, w) in BARS {
        for y in top * scale..(top + h) * scale {
            for x in LEFT * scale..(LEFT + w) * scale {
                let i = ((y * size + x) * 4) as usize;
                px[i..i + 4].copy_from_slice(&c);
            }
        }
    }
    if v == Variant::TemplateBad {
        let (cx, cy, r) = DOT;
        let s = f64::from(scale);
        for y in 0..size {
            for x in 0..size {
                let (dx, dy) = ((f64::from(x) + 0.5) / s - cx, (f64::from(y) + 0.5) / s - cy);
                let d = dx * dx + dy * dy;
                let i = ((y * size + x) * 4) as usize;
                if d <= r * r {
                    px[i..i + 4].copy_from_slice(&c);
                } else if d <= (r + 2.0) * (r + 2.0) {
                    px[i..i + 4].copy_from_slice(&[0, 0, 0, 0]);
                }
            }
        }
    }
    px
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opaque(p: &[u8]) -> usize {
        p.chunks(4).filter(|c| c[3] == 255).count()
    }

    #[test]
    fn size_and_variants() {
        let (d, l) = (rgba(Variant::Dark), rgba(Variant::Light));
        assert_eq!(d.len(), (SIZE * SIZE * 4) as usize);
        assert_ne!(d, l);
        assert_eq!(opaque(&d), opaque(&l));
        assert_eq!(opaque(&d), (20 + 14 + 17) * 5);
        assert_eq!(&d[..4], &[0, 0, 0, 0], "corner transparent");
    }

    #[test]
    fn contrasts_with_taskbar() {
        assert_eq!(Variant::for_taskbar(true), Variant::Dark);
        assert_eq!(Variant::for_taskbar(false), Variant::Light);
    }

    #[test]
    fn template_is_black_alpha_only() {
        let t = rgba(Variant::Template);
        assert_eq!(opaque(&t), opaque(&rgba(Variant::Dark)));
        assert!(t
            .chunks(4)
            .all(|p| p[..3] == [0, 0, 0] && (p[3] == 0 || p[3] == 255)));
    }

    #[test]
    fn bad_adds_a_dot_in_the_corner() {
        let (t, b) = (rgba(Variant::Template), rgba(Variant::TemplateBad));
        assert_ne!(opaque(&t), opaque(&b));
        let at = |x: u32, y: u32| ((y * SIZE + x) * 4 + 3) as usize;
        assert_eq!(t[at(26, 5)], 0);
        assert_eq!(b[at(26, 5)], 255);
        // the gap keeps the dot off the top bar
        assert_eq!(t[at(24, 8)], 255);
        assert_eq!(b[at(24, 8)], 0);
    }

    #[test]
    fn scaled_keeps_the_shape() {
        let s = rgba_scaled(Variant::Template, 2);
        assert_eq!(s.len(), (SIZE * SIZE * 16) as usize);
        assert_eq!(opaque(&s), opaque(&rgba(Variant::Template)) * 4);
    }
}
