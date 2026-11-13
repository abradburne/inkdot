//! The screen textures: what is laid over the game's picture to suggest the screen it came from.
//!
//! Each texture is a pattern over one Game Boy pixel (a *cell*), built into a mask once per
//! picture size and multiplied into the picture by the shader ([`crate::video`]); [`render`] is
//! the same sum on the CPU, for tests and for looking at the textures without the handheld.
//!
//! The pattern is separable: a profile across each cell's width times one down its height.
//! Each profile is a pixel *body* and a *line* at its far edge (the right column, the bottom
//! row), and inside the body optional R, G and B stripes (each a third of it, as on a colour
//! TFT) and an optional softening toward the edges (as d⁴). Separable patterns can be averaged
//! exactly over any screen pixel, whatever the number of screen pixels per cell: each mask texel
//! is its pixel's area coverage of the pattern (the profiles averaged over 64 points along each
//! axis), so lines and stripes thinner than a pixel stay sharp and keep their overall darkness.
//! Below three screen pixels a cell, stripes can't be told apart, and each is replaced by its
//! average (the same darkness, no colour cast).
//!
//! The sums are in linear light, with a gamma of 2 (a square to decode, a square root to
//! encode: close to the panel's, and cheap in a shader), so a thin dark line takes away the
//! light it covers and no more.
//!
//! What the lines are depends on the screen:
//!
//! - On a colour TFT (the Game Boy Color, and the stripes, scanlines and soft textures on any
//!   console) the lines are the dark matrix between pixels: the picture is darkened, and a
//!   *gain* worked out from the mask's own average brightens it again so mid-tones keep their
//!   brightness. The gain is the same on all three channels and stops where a pixel's brightest
//!   channel reaches full, so colours keep their hue.
//! - On the reflective STN screens of the DMG, the Pocket and the Light, the gaps between pixels
//!   show the screen's backing ("paper", the lightest shade): dark pixels are framed by light
//!   lines, light areas look smooth. A dark pixel also casts a shadow onto the backing, a little
//!   down and to the right, as the original's do: the shader darkens a pixel by the shadow of the
//!   one up and to the left of it, relative to its own (so flat areas keep their shade, and only
//!   edges show it).

/// What a screen texture tries to look like, as the option names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Style {
    /// The console's own screen ([`Panel`]).
    Real,
    /// A grid between pixels.
    Grid,
    /// R, G and B stripes in each pixel.
    Stripes,
    /// Stronger stripes and grid.
    BoldStripes,
    /// A dark band along the bottom of each row.
    Scanlines,
    /// Each pixel a soft, rounded dot.
    Soft,
    /// Nothing: sharp square pixels.
    Off,
}

impl Style {
    /// The option's values and their labels, in the menus' order.
    pub const CHOICES: &'static [(&'static str, &'static str)] = &[
        ("real", "Like its screen"),
        ("grid", "Pixel grid"),
        ("stripes", "Colour stripes"),
        ("bold", "Bold colour stripes"),
        ("scanlines", "Scanlines"),
        ("soft", "Soft pixels"),
        ("off", "None (sharp)"),
    ];

    pub fn parse(s: &str) -> Option<Style> {
        Some(match s {
            "real" => Style::Real,
            "grid" => Style::Grid,
            "stripes" => Style::Stripes,
            "bold" => Style::BoldStripes,
            "scanlines" => Style::Scanlines,
            "soft" => Style::Soft,
            "off" => Style::Off,
            _ => return None,
        })
    }
}

/// The screen a look suggests.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Panel {
    /// The original Game Boy: a reflective STN with large gaps and a deep pixel shadow.
    Dmg,
    /// The Pocket: a faster, sharper STN with finer gaps and a lighter shadow.
    Pocket,
    /// The Light: the Pocket's screen, backlit, so its shadow hardly shows.
    Light,
    /// The Game Boy Color: a reflective colour TFT with R, G, B stripes.
    Gbc,
}

impl Panel {
    /// Whether the gaps between pixels show the screen's backing (an STN) or are dark (a TFT).
    fn reflective(self) -> bool {
        self != Panel::Gbc
    }
}

/// One profile of a cell, across it or down it; positions are shares of the cell.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Axis {
    /// The line at the cell's far edge, as a share of the cell.
    line: f32,
    /// What a dark line lets through (linear light); unused where the line shows the backing.
    line_level: f32,
    /// R, G, B stripes across the body: what each lets through of the other two channels.
    stripes: Option<f32>,
    /// How much darker the body is at its edges than at its middle, falling off as d⁴.
    soft: f32,
}

impl Axis {
    const PLAIN: Axis = Axis {
        line: 0.0,
        line_level: 1.0,
        stripes: None,
        soft: 0.0,
    };

    const fn line(line: f32, line_level: f32) -> Axis {
        Axis {
            line,
            line_level,
            ..Axis::PLAIN
        }
    }

    /// The profile at `u` (0..1 across the cell): what it lets through of each channel, and
    /// whether this is the pixel's body (1) or the line (0) where the line shows the backing.
    /// With `grey`, stripes are replaced by their average.
    fn at(&self, u: f32, backing: bool, grey: bool) -> ([f32; 3], f32) {
        let body_end = 1.0 - self.line;
        if u >= body_end {
            return if backing {
                ([0.0; 3], 0.0)
            } else {
                ([self.line_level; 3], 1.0)
            };
        }
        let x = u / body_end;
        let mut rgb = [1.0; 3];
        if let Some(other) = self.stripes {
            if grey {
                rgb = [(1.0 + 2.0 * other) / 3.0; 3];
            } else {
                let own = ((x * 3.0) as usize).min(2);
                for (i, c) in rgb.iter_mut().enumerate() {
                    if i != own {
                        *c = other;
                    }
                }
            }
        }
        let d = (2.0 * x - 1.0).abs();
        let soft = 1.0 - self.soft * d.powi(4);
        (rgb.map(|c| c * soft), 1.0)
    }
}

/// A screen texture, ready to build into a [`Mask`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pattern {
    across: Axis,
    down: Axis,
    /// The gaps show the screen's backing at this brightness (a share of the lightest shade),
    /// or, `None`, are dark.
    backing: Option<f32>,
    /// How dark a black pixel's shadow is (0: no shadow).
    pub shadow: f32,
    /// Whether the picture keeps the last frame's ghost (the panel's slow pixels).
    pub slow: bool,
}

/// How far a pixel's shadow falls, down and to the right, as a share of a cell.
pub const SHADOW_OFFSET: f32 = 0.22;

/// The most a mask's gain may brighten (linear light): 150% in sRGB terms.
const MAX_GAIN: f32 = 2.25;

/// Points averaged along each axis of a screen pixel.
const SAMPLES: usize = 64;

/// Below this many screen pixels a cell, stripes are averaged to grey.
const MIN_STRIPE_CELL: f32 = 3.0;

impl Pattern {
    /// Nothing over the picture.
    pub const OFF: Pattern = Pattern {
        across: Axis::PLAIN,
        down: Axis::PLAIN,
        backing: None,
        shadow: 0.0,
        slow: false,
    };

    /// The texture for a style on a screen.
    pub fn new(style: Style, panel: Panel) -> Pattern {
        let dark = |across: Axis, down: Axis| Pattern {
            across,
            down,
            backing: None,
            shadow: 0.0,
            slow: true,
        };
        let stn = |line: f32, backing: f32, shadow: f32| Pattern {
            across: Axis::line(line, 1.0),
            down: Axis::line(line, 1.0),
            backing: Some(backing),
            shadow,
            slow: true,
        };
        match (style, panel) {
            (Style::Off, _) => Pattern::OFF,
            (Style::Real, Panel::Dmg) => stn(0.14, 0.94, 0.30),
            (Style::Real, Panel::Pocket) => stn(0.10, 0.97, 0.20),
            (Style::Real, Panel::Light) => stn(0.10, 1.0, 0.10),
            (Style::Real, Panel::Gbc) => dark(
                Axis {
                    stripes: Some(0.6),
                    ..Axis::line(0.12, 0.30)
                },
                Axis::line(0.10, 0.45),
            ),
            (Style::Grid, p) if p.reflective() => stn(0.16, 0.94, 0.0),
            (Style::Grid, _) => dark(Axis::line(0.16, 0.35), Axis::line(0.16, 0.35)),
            (Style::Stripes, _) => dark(
                Axis {
                    stripes: Some(0.4),
                    ..Axis::line(0.14, 0.30)
                },
                Axis::line(0.12, 0.45),
            ),
            (Style::BoldStripes, _) => dark(
                Axis {
                    stripes: Some(0.27),
                    ..Axis::line(0.12, 0.30)
                },
                Axis::line(0.12, 0.40),
            ),
            (Style::Scanlines, _) => dark(
                Axis::PLAIN,
                Axis {
                    soft: 0.3,
                    ..Axis::line(0.30, 0.30)
                },
            ),
            (Style::Soft, _) => dark(
                Axis {
                    soft: 0.6,
                    ..Axis::PLAIN
                },
                Axis {
                    soft: 0.6,
                    ..Axis::PLAIN
                },
            ),
        }
    }

    /// Builds the mask for a picture of `width` × `height` screen pixels, `cell` screen pixels
    /// to a Game Boy pixel (any size, whole or not).
    pub fn build(&self, width: usize, height: usize, cell: f32) -> Mask {
        let backing = self.backing.is_some();
        let grey = cell < MIN_STRIPE_CELL;
        let across = profile(&self.across, width, cell, backing, grey);
        let down = profile(&self.down, height, cell, backing, grey);
        let level = self.backing.unwrap_or(0.0);
        let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        let mut texels = Vec::with_capacity(width * height * 4);
        for (rgb_d, body_d) in &down {
            for (rgb_a, body_a) in &across {
                texels.extend([
                    byte(rgb_a[0] * rgb_d[0]),
                    byte(rgb_a[1] * rgb_d[1]),
                    byte(rgb_a[2] * rgb_d[2]),
                    byte(level * (1.0 - body_a * body_d)),
                ]);
            }
        }
        Mask {
            width,
            height,
            texels,
            gain: self.gain(grey),
        }
    }

    /// The brightening that gives a mid-tone back the light the mask takes from it.
    fn gain(&self, grey: bool) -> f32 {
        if self.backing.is_some() {
            // The backing fills the gaps with light: nothing to give back.
            return 1.0;
        }
        let mean = |axis: &Axis| {
            let n = 4096;
            let mut sum = [0.0f32; 3];
            for i in 0..n {
                let (rgb, body) = axis.at((i as f32 + 0.5) / n as f32, false, grey);
                for (s, c) in sum.iter_mut().zip(rgb) {
                    *s += c * body / n as f32;
                }
            }
            sum
        };
        let (a, d) = (mean(&self.across), mean(&self.down));
        let through = luma([a[0] * d[0], a[1] * d[1], a[2] * d[2]]);
        (1.0 / through.max(1e-3)).min(MAX_GAIN)
    }
}

/// A profile's average over each screen pixel along an axis `len` pixels long: the channels it
/// lets through, and the share of the pixel that is body.
fn profile(axis: &Axis, len: usize, cell: f32, backing: bool, grey: bool) -> Vec<([f32; 3], f32)> {
    (0..len)
        .map(|i| {
            let mut rgb = [0.0f32; 3];
            let mut body = 0.0;
            for s in 0..SAMPLES {
                let x = (i as f32 + (s as f32 + 0.5) / SAMPLES as f32) / cell;
                let (c, b) = axis.at(x.fract(), backing, grey);
                for (sum, v) in rgb.iter_mut().zip(c) {
                    *sum += v * b;
                }
                body += b;
            }
            (rgb.map(|v| v / SAMPLES as f32), body / SAMPLES as f32)
        })
        .collect()
}

/// A built texture: one RGBA8 texel per screen pixel of the picture, top row first. RGB is
/// what passes of each channel of the picture, A how much of the backing shows; both in
/// linear light.
#[derive(Clone, Debug, PartialEq)]
pub struct Mask {
    pub width: usize,
    pub height: usize,
    pub texels: Vec<u8>,
    /// The picture is brightened by this much (linear light) before the mask.
    pub gain: f32,
}

/// Relative luminance (Rec. 709 weights) of a linear colour.
pub fn luma(c: [f32; 3]) -> f32 {
    0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
}

/// What the shader is told besides the mask.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shading {
    /// The share of the last frame left in this one.
    pub persistence: f32,
    /// The screen's backing (the lightest shade), as 8-bit sRGB.
    pub paper: [u8; 3],
    pub shadow: f32,
}

/// The shader's sum on the CPU: a 160×144 RGBA frame (and the one before) through a mask whose
/// cells are `cell` screen pixels, as RGBA rows top first. The shader in [`crate::video`]
/// does exactly this.
// Used by the tests and the screen_looks example, not the program (`allow`, not `expect`: the
// example includes this file too, and uses it).
#[cfg_attr(not(test), allow(dead_code))]
pub fn render(now: &[u8], before: &[u8], mask: &Mask, cell: f32, s: Shading) -> Vec<u8> {
    let lin = |v: u8| {
        let f = v as f32 / 255.0;
        f * f
    };
    let (w, h) = (mask.width as i32, mask.height as i32);
    let at = |x: i32, y: i32| -> [f32; 3] {
        let (x, y) = (x.clamp(0, w - 1), y.clamp(0, h - 1));
        let gx = ((x as f32 / cell) as usize).min(159);
        let gy = ((y as f32 / cell) as usize).min(143);
        let i = (gy * 160 + gx) * 4;
        std::array::from_fn(|c| {
            let (a, b) = (lin(now[i + c]), lin(before[i + c]));
            a + (b - a) * s.persistence
        })
    };
    let paper = s.paper.map(lin);
    let lp = luma(paper).max(1e-4);
    let darkness = |c: [f32; 3]| (1.0 - luma(c) / lp).clamp(0.0, 1.0);
    let step = shadow_step(cell);
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let c = at(x, y);
            let top = c[0].max(c[1]).max(c[2]).max(1e-6);
            let k = mask.gain.min(1.0 / top);
            let i = ((y * w + x) * 4) as usize;
            let m: [f32; 4] = std::array::from_fn(|c| mask.texels[i + c] as f32 / 255.0);
            let mut o: [f32; 3] = std::array::from_fn(|i| c[i] * k * m[i] + paper[i] * m[3]);
            if s.shadow > 0.0 {
                let cast = darkness(at(x - step, y - step));
                let ratio = (1.0 - s.shadow * cast) / (1.0 - s.shadow * darkness(c));
                o = o.map(|v| v * ratio);
            }
            out.extend(o.map(|v| (v.clamp(0.0, 1.0).sqrt() * 255.0).round() as u8));
            out.push(255);
        }
    }
    out
}

/// How far a shadow falls, in screen pixels (at least one).
pub fn shadow_step(cell: f32) -> i32 {
    ((SHADOW_OFFSET * cell).round() as i32).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    const STYLES: [Style; 7] = [
        Style::Real,
        Style::Grid,
        Style::Stripes,
        Style::BoldStripes,
        Style::Scanlines,
        Style::Soft,
        Style::Off,
    ];
    const PANELS: [Panel; 4] = [Panel::Dmg, Panel::Pocket, Panel::Light, Panel::Gbc];

    /// The mask's average over `cells` × `cells` Game Boy pixels: per channel, and the backing.
    fn average(mask: &Mask) -> ([f32; 3], f32) {
        let n = (mask.width * mask.height) as f32;
        let mut sum = [0.0f32; 4];
        for t in mask.texels.chunks(4) {
            for (s, v) in sum.iter_mut().zip(t) {
                *s += *v as f32 / 255.0 / n;
            }
        }
        ([sum[0], sum[1], sum[2]], sum[3])
    }

    #[test]
    fn every_option_value_parses() {
        for (value, _) in Style::CHOICES {
            assert!(Style::parse(value).is_some(), "{value}");
        }
        assert_eq!(Style::CHOICES.len(), STYLES.len());
    }

    /// With its gain, a dark-gapped texture keeps a mid-tone's brightness; the textures that
    /// show the backing need no gain.
    #[test]
    fn mid_tones_keep_their_brightness() {
        for style in STYLES {
            for panel in PANELS {
                let p = Pattern::new(style, panel);
                let mask = p.build(800, 720, 5.0);
                let (rgb, backing) = average(&mask);
                if p.backing.is_some() {
                    assert_eq!(mask.gain, 1.0);
                    assert!(
                        backing > 0.1 && backing < 0.4,
                        "{style:?} {panel:?} {backing}"
                    );
                    continue;
                }
                assert_eq!(backing, 0.0);
                let kept = luma(rgb) * mask.gain;
                assert!((kept - 1.0).abs() < 0.02, "{style:?} {panel:?}: {kept}");
                // And none darkens so much that the gain runs out.
                assert!(mask.gain < MAX_GAIN, "{style:?} {panel:?}: {}", mask.gain);
            }
        }
    }

    #[test]
    fn off_changes_nothing() {
        let mask = Pattern::OFF.build(800, 720, 5.0);
        assert!(mask.texels.chunks(4).all(|t| t == [255, 255, 255, 0]));
        assert_eq!(mask.gain, 1.0);
        let frame: Vec<u8> = (0..160 * 144 * 4).map(|i| (i * 7 % 256) as u8).collect();
        let shading = Shading {
            persistence: 0.0,
            paper: [255; 3],
            shadow: 0.0,
        };
        let out = render(&frame, &frame, &mask, 5.0, shading);
        for y in [0, 3, 719] {
            for x in [0, 4, 799] {
                let o = &out[(y * 800 + x) * 4..][..4];
                let f = &frame[((y / 5) * 160 + x / 5) * 4..][..3];
                for c in 0..3 {
                    assert!(o[c].abs_diff(f[c]) <= 1, "{x},{y}: {o:?} {f:?}");
                }
            }
        }
    }

    /// In the stripes texture, the left third of a pixel is red, the middle green, the right
    /// blue, and the line at the right edge is dark.
    #[test]
    fn stripes_are_red_green_blue() {
        let mask = Pattern::new(Style::Stripes, Panel::Gbc).build(160, 10, 10.0);
        let texel = |x: usize| &mask.texels[x * 4..][..3];
        let (r, g, b) = (texel(1), texel(4), texel(7));
        assert!(r[0] > 200 && r[1] < 120 && r[2] < 120, "{r:?}");
        assert!(g[1] > 200 && g[0] < 120 && g[2] < 120, "{g:?}");
        assert!(b[2] > 200 && b[0] < 120 && b[1] < 120, "{b:?}");
        let line = texel(9);
        assert!(line.iter().all(|&v| v < 100), "{line:?}");
        // The next cell starts red again.
        assert_eq!(texel(11), r);
    }

    /// At 5⅓ screen pixels a cell (160 pixels across 853), each screen pixel takes the share of
    /// the pattern it covers: the darkness stays what it is at a whole size, and no pixel is
    /// as dark as a line (a line thinner than a pixel is blended, not dropped or doubled).
    #[test]
    fn a_fractional_size_keeps_the_pattern() {
        for style in [Style::Grid, Style::Stripes, Style::Scanlines] {
            let p = Pattern::new(style, Panel::Gbc);
            let whole = average(&p.build(800, 720, 5.0)).0;
            let part = p.build(853, 768, 853.0 / 160.0);
            let frac = average(&part).0;
            for c in 0..3 {
                assert!(
                    (whole[c] - frac[c]).abs() < 0.01,
                    "{style:?}: {whole:?} {frac:?}"
                );
            }
            assert!((p.build(800, 720, 5.0).gain - part.gain).abs() < 1e-3);
        }
        // The grid: a 0.16-cell line is 0.85 of a screen pixel; where it falls across two,
        // both are partly dark.
        let mask = Pattern::new(Style::Grid, Panel::Gbc).build(853, 1, 853.0 / 160.0);
        let darkest = mask.texels.chunks(4).map(|t| t[0]).min().unwrap_or(0);
        let line = (0.35f32 * 255.0) as u8;
        assert!(darkest > line, "{darkest} vs {line}");
        let lines = mask.texels.chunks(4).filter(|t| t[0] < 250).count();
        assert!((160..=320).contains(&lines), "{lines} darkened columns");
    }

    /// Below three screen pixels a cell the stripes are grey, and as dark as the coloured ones.
    #[test]
    fn small_cells_have_grey_stripes() {
        let p = Pattern::new(Style::Stripes, Panel::Gbc);
        let small = p.build(400, 360, 2.5);
        for t in small.texels.chunks(4) {
            assert!(
                t[0].abs_diff(t[1]) <= 1 && t[1].abs_diff(t[2]) <= 1,
                "{t:?}"
            );
        }
        let big = p.build(800, 720, 5.0);
        assert!((luma(average(&small).0) - luma(average(&big).0)).abs() < 0.01);
        assert!((small.gain - big.gain).abs() < 1e-3);
    }

    /// The gain keeps a colour's hue, and stops where its brightest channel is full.
    #[test]
    fn the_gain_keeps_hue() {
        let mask = Pattern::new(Style::Soft, Panel::Gbc).build(800, 720, 5.0);
        let shading = Shading {
            persistence: 0.0,
            paper: [255; 3],
            shadow: 0.0,
        };
        let frame: Vec<u8> = [240u8, 120, 60, 255].repeat(160 * 144);
        let out = render(&frame, &frame, &mask, 5.0, shading);
        // The middle of a pixel, where the soft texture lets nearly everything through: red is
        // brightened to full, and no further.
        let o = &out[(2 * 800 + 2) * 4..][..3];
        assert_eq!(o[0], 255, "{o:?}");
        let ratio = |a: u8, b: u8| a as f32 / b as f32;
        assert!((ratio(o[1], o[0]) - 0.5).abs() < 0.02, "{o:?}");
        assert!((ratio(o[2], o[0]) - 0.25).abs() < 0.02, "{o:?}");
    }

    /// On the DMG's screen the gaps show the backing, a dark pixel's shadow falls on its light
    /// neighbour down and to the right, and a flat area is its own shade.
    #[test]
    fn the_dmg_screen() {
        let p = Pattern::new(Style::Real, Panel::Dmg);
        let mask = p.build(800, 720, 5.0);
        let paper = [0xc4, 0xcc, 0x92];
        let dark = [0x22, 0x30, 0x29];
        let mut frame: Vec<u8> = [paper[0], paper[1], paper[2], 255].repeat(160 * 144);
        // One dark pixel at (10, 10).
        frame[(10 * 160 + 10) * 4..][..3].copy_from_slice(&dark);
        let shading = Shading {
            persistence: 0.0,
            paper,
            shadow: p.shadow,
        };
        let out = render(&frame, &frame, &mask, 5.0, shading);
        let px = |x: usize, y: usize| &out[(y * 800 + x) * 4..][..3];
        // Flat paper is the paper, nearly.
        assert!(px(300, 300)
            .iter()
            .zip(paper)
            .all(|(a, b)| a.abs_diff(b) <= 3));
        // The dark pixel's body is dark; its right edge is the lighter gap.
        assert!(px(51, 51)[1] < 0x40, "{:?}", px(51, 51));
        assert!(px(54, 51)[1] > px(51, 51)[1] + 40, "{:?}", px(54, 51));
        // Its shadow falls to the right and below it, not above or to the left.
        assert!(px(55, 53)[1] + 10 < paper[1], "{:?}", px(55, 53));
        assert!(px(53, 55)[1] + 10 < paper[1], "{:?}", px(53, 55));
        assert!(px(46, 51)[1].abs_diff(px(301, 301)[1]) <= 3);
        assert!(px(51, 46)[1].abs_diff(px(301, 301)[1]) <= 3);
    }

    /// White can't be brightened, so it keeps what the mask lets through: none of the textures
    /// takes more than 60% of its light (about a fifth in sRGB terms).
    #[test]
    fn white_stays_bright() {
        for style in STYLES {
            let mask = Pattern::new(style, Panel::Gbc).build(800, 720, 5.0);
            let white = luma(average(&mask).0);
            assert!(white > 0.4, "{style:?}: {white}");
            if style == Style::Real {
                assert!(
                    white > 0.6,
                    "the Color's own screen is the gentlest: {white}"
                );
            }
        }
    }
}
