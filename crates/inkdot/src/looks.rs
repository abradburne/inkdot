//! How the picture looks: the shades of a DMG game's look, and the colours of a CGB game.
//!
//! The four DMG looks are shades chosen here to suggest each model's screen (they are ours, not
//! measured): the original's green-tinted reflective LCD, the Pocket's grey one, the Light's
//! backlit teal, and plain grey. The LCD texture (the dot grid and the slow pixels) is the
//! shader's business ([`crate::video`]).
//!
//! A CGB game's colours are 15-bit; its screen shows them darker and less saturated than an
//! sRGB monitor would (Pan Docs "RGB Translation by CGBs"). "As on its screen" models that with a
//! gamma curve and a mixing matrix of our own, tuned by eye to the description there: mid
//! intensities brighten, primaries bleed into each other a little, white is a light grey.
//! "As stored" is the plain conversion, `(c << 3) | (c >> 2)`.

/// A DMG game's look.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum Look {
    /// Run on a CGB, coloured by its compatibility palettes.
    Color,
    GameBoy,
    Pocket,
    Light,
    Grey,
}

impl Look {
    pub fn parse(s: &str) -> Option<Look> {
        Some(match s {
            "color" => Look::Color,
            "gameboy" => Look::GameBoy,
            "pocket" => Look::Pocket,
            "light" => Look::Light,
            "grey" => Look::Grey,
            _ => return None,
        })
    }

    /// The four shades, lightest first, for a look that runs as a DMG.
    pub fn shades(self) -> [[u8; 3]; 4] {
        match self {
            Look::GameBoy => [
                [0xc4, 0xcc, 0x92],
                [0x87, 0x97, 0x62],
                [0x4b, 0x5c, 0x41],
                [0x22, 0x30, 0x29],
            ],
            Look::Pocket => [
                [0xdc, 0xdb, 0xcf],
                [0xa2, 0xa1, 0x96],
                [0x5f, 0x5f, 0x58],
                [0x24, 0x25, 0x23],
            ],
            Look::Light => [
                [0x6f, 0xd8, 0xb8],
                [0x3c, 0xa3, 0x89],
                [0x1f, 0x67, 0x58],
                [0x0b, 0x2e, 0x2a],
            ],
            Look::Grey | Look::Color => [[0xff; 3], [0xaa; 3], [0x55; 3], [0x00; 3]],
        }
    }

    /// How slowly the screen's pixels change, for the LCD texture's blur: the share of the
    /// last frame left in each new one.
    pub fn persistence(self) -> f32 {
        match self {
            Look::GameBoy => 0.45,
            Look::Pocket => 0.30,
            Look::Light => 0.30,
            Look::Grey | Look::Color => 0.20,
        }
    }
}

/// A CGB colour (RGB555, red in bits 0-4) as plain 8-bit RGB.
pub fn raw(c: u16) -> [u8; 3] {
    let ch = |shift: u16| {
        let v = ((c >> shift) & 31) as u8;
        (v << 3) | (v >> 2)
    };
    [ch(0), ch(5), ch(10)]
}

/// The nearest RGB555 colour to an RGB one, for writing a look's shades into CGB palettes.
pub fn rgb555(c: [u8; 3]) -> u16 {
    let five = |v: u8| (v as u16 * 31 + 127) / 255;
    five(c[0]) | five(c[1]) << 5 | five(c[2]) << 10
}

/// A CGB colour as its screen shows it (see the module's comment).
pub fn screen(c: u16) -> [u8; 3] {
    let lin = |shift: u16| {
        let v = ((c >> shift) & 31) as f32 / 31.0;
        // The panel's response: mid values come out brighter than a 2.2 display gamma.
        v.powf(1.7)
    };
    let (r, g, b) = (lin(0), lin(5), lin(10));
    // Each primary bleeds into the others a little.
    let mix = [
        0.82 * r + 0.13 * g + 0.05 * b,
        0.10 * r + 0.74 * g + 0.16 * b,
        0.08 * r + 0.14 * g + 0.78 * b,
    ];
    // Unlit, white is a light grey.
    mix.map(|v| ((v.clamp(0.0, 1.0).powf(1.0 / 2.2) * 0.94 + 0.02) * 255.0) as u8)
}

/// A table from every CGB colour to RGBA, for converting a frame quickly.
pub fn color_table(screen_like: bool) -> Vec<[u8; 4]> {
    (0..32768u16)
        .map(|c| {
            let [r, g, b] = if screen_like { screen(c) } else { raw(c) };
            [r, g, b, 255]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shades_survive_rgb555() {
        for look in [Look::GameBoy, Look::Pocket, Look::Light, Look::Grey] {
            for s in look.shades() {
                let back = raw(rgb555(s));
                assert!(
                    s.iter().zip(back).all(|(a, b)| a.abs_diff(b) <= 4),
                    "{s:?} {back:?}"
                );
            }
        }
    }

    #[test]
    fn raw_colours_span_0_to_255() {
        assert_eq!(raw(0), [0, 0, 0]);
        assert_eq!(raw(0x7fff), [255, 255, 255]);
        assert_eq!(raw(0x001f), [255, 0, 0]);
    }

    #[test]
    fn the_screen_is_duller_than_raw() {
        let [r, g, b] = screen(0x001f);
        assert!(r > 180 && g > 40 && b > 30, "red bleeds: {r} {g} {b}");
        assert!(screen(0x7fff)[0] < 250);
        // Order is kept: brighter in, brighter out.
        for c in 0..31u16 {
            assert!(screen(c + 1)[0] >= screen(c)[0]);
        }
    }

    #[test]
    fn looks_go_light_to_dark() {
        for look in [Look::GameBoy, Look::Pocket, Look::Light, Look::Grey] {
            let s = look.shades();
            let lum = |c: [u8; 3]| c[0] as u32 * 3 + c[1] as u32 * 6 + c[2] as u32;
            assert!(lum(s[0]) > lum(s[1]) && lum(s[1]) > lum(s[2]) && lum(s[2]) > lum(s[3]));
        }
    }
}
