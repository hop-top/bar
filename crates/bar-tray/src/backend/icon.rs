//! The tray glyph as raw RGBA, drawn in code so no image asset or decoder
//! is needed: three bars of unequal length on a transparent square. One
//! variant per taskbar theme, so the glyph stays legible when the theme
//! flips.

pub const SIZE: u32 = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Variant {
    /// Dark glyph, for a light taskbar.
    Dark,
    /// Light glyph, for a dark taskbar.
    Light,
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
        }
    }
}

/// Bars as `(top row, height, width)`; all start at column 6.
const BARS: [(u32, u32, u32); 3] = [(7, 5, 20), (14, 5, 14), (21, 5, 17)];
const LEFT: u32 = 6;

/// `SIZE` x `SIZE` RGBA pixels, row-major.
pub fn rgba(v: Variant) -> Vec<u8> {
    let mut px = vec![0u8; (SIZE * SIZE * 4) as usize];
    let c = v.colour();
    for (top, h, w) in BARS {
        for y in top..top + h {
            for x in LEFT..LEFT + w {
                let i = ((y * SIZE + x) * 4) as usize;
                px[i..i + 4].copy_from_slice(&c);
            }
        }
    }
    px
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn size_and_variants() {
        let (d, l) = (rgba(Variant::Dark), rgba(Variant::Light));
        assert_eq!(d.len(), (SIZE * SIZE * 4) as usize);
        assert_ne!(d, l);
        let opaque = |p: &[u8]| p.chunks(4).filter(|c| c[3] == 255).count();
        assert_eq!(opaque(&d), opaque(&l));
        assert_eq!(opaque(&d), (20 + 14 + 17) * 5);
        assert_eq!(&d[..4], &[0, 0, 0, 0], "corner transparent");
    }

    #[test]
    fn contrasts_with_taskbar() {
        assert_eq!(Variant::for_taskbar(true), Variant::Dark);
        assert_eq!(Variant::for_taskbar(false), Variant::Light);
    }
}
