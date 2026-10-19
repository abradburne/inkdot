//! Drawing the bottom screen in software: rectangles with round corners, text in Roboto
//! Condensed (Apache-2.0, as DSlot uses), pictures scaled by whole pixels, and a few icons made
//! of shapes. The screen is redrawn only when something on it changes, so none of this runs
//! while a game plays untouched.

use fontdue::{Font, FontSettings, Metrics};
use std::collections::HashMap;

pub type Rgb = [u8; 3];

/// The palette of the interface.
pub mod colour {
    use super::Rgb;
    pub const BACKGROUND: Rgb = [0x12, 0x14, 0x18];
    pub const PANEL: Rgb = [0x1e, 0x21, 0x28];
    pub const PANEL_LIT: Rgb = [0x2a, 0x2e, 0x38];
    pub const LINE: Rgb = [0x34, 0x39, 0x45];
    pub const TEXT: Rgb = [0xf2, 0xef, 0xe8];
    pub const DIM: Rgb = [0x98, 0x9e, 0xa9];
    pub const INK: Rgb = [0xf0, 0xb8, 0x4a];
    pub const INK_DARK: Rgb = [0x3a, 0x2e, 0x16];
}

/// A rectangle in screen pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> Rect {
        Rect { x, y, w, h }
    }

    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.w && y < self.y + self.h
    }

    pub fn inset(&self, d: i32) -> Rect {
        Rect::new(self.x + d, self.y + d, self.w - 2 * d, self.h - 2 * d)
    }

    pub fn centre(&self) -> (i32, i32) {
        (self.x + self.w / 2, self.y + self.h / 2)
    }
}

/// A font size and weight.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Type {
    pub size: u32,
    pub bold: bool,
}

pub const TITLE: Type = Type {
    size: 38,
    bold: true,
};
pub const LABEL: Type = Type {
    size: 32,
    bold: true,
};
pub const BODY: Type = Type {
    size: 28,
    bold: false,
};
pub const SMALL: Type = Type {
    size: 24,
    bold: false,
};

/// How text is placed horizontally in its box.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Left,
    Centre,
    Right,
}

/// An RGBA picture the size of the screen, and the fonts to write on it.
pub struct Canvas {
    pub w: i32,
    pub h: i32,
    pub px: Vec<u8>,
    regular: Font,
    bold: Font,
    glyphs: HashMap<(char, Type), (Metrics, Vec<u8>)>,
}

impl Canvas {
    pub fn new(w: i32, h: i32) -> Canvas {
        let load = |bytes: &[u8]| Font::from_bytes(bytes, FontSettings::default()).expect("font");
        Canvas {
            w,
            h,
            px: vec![0; (w * h * 4) as usize],
            regular: load(include_bytes!(
                "../../../../assets/RobotoCondensed-Regular.ttf"
            )),
            bold: load(include_bytes!(
                "../../../../assets/RobotoCondensed-Bold.ttf"
            )),
            glyphs: HashMap::new(),
        }
    }

    fn blend(&mut self, x: i32, y: i32, c: Rgb, a: f32) {
        if x < 0 || y < 0 || x >= self.w || y >= self.h || a <= 0.0 {
            return;
        }
        let i = ((y * self.w + x) * 4) as usize;
        for (dst, &src) in self.px[i..i + 3].iter_mut().zip(&c) {
            let d = *dst as f32;
            *dst = (d + (src as f32 - d) * a.min(1.0)) as u8;
        }
        self.px[i + 3] = 255;
    }

    pub fn clear(&mut self, c: Rgb) {
        for p in self.px.chunks_exact_mut(4) {
            p.copy_from_slice(&[c[0], c[1], c[2], 255]);
        }
    }

    pub fn fill(&mut self, r: Rect, c: Rgb) {
        for y in r.y.max(0)..(r.y + r.h).min(self.h) {
            for x in r.x.max(0)..(r.x + r.w).min(self.w) {
                let i = ((y * self.w + x) * 4) as usize;
                self.px[i..i + 4].copy_from_slice(&[c[0], c[1], c[2], 255]);
            }
        }
    }

    /// A rectangle with corners of `radius`, anti-aliased.
    pub fn round(&mut self, r: Rect, radius: i32, c: Rgb) {
        let rad = radius.min(r.w / 2).min(r.h / 2) as f32;
        for y in r.y..r.y + r.h {
            for x in r.x..r.x + r.w {
                let px = x as f32 + 0.5;
                let py = y as f32 + 0.5;
                let cx = px.clamp(r.x as f32 + rad, (r.x + r.w) as f32 - rad);
                let cy = py.clamp(r.y as f32 + rad, (r.y + r.h) as f32 - rad);
                let d = ((px - cx).powi(2) + (py - cy).powi(2)).sqrt();
                self.blend(x, y, c, rad - d + 0.5);
            }
        }
    }

    /// An outline of a rounded rectangle, `width` thick.
    pub fn ring(&mut self, r: Rect, radius: i32, width: i32, c: Rgb) {
        let rad = radius as f32;
        let inner = rad - width as f32;
        for y in r.y..r.y + r.h {
            for x in r.x..r.x + r.w {
                let px = x as f32 + 0.5;
                let py = y as f32 + 0.5;
                let cx = px.clamp(r.x as f32 + rad, (r.x + r.w) as f32 - rad);
                let cy = py.clamp(r.y as f32 + rad, (r.y + r.h) as f32 - rad);
                let d = ((px - cx).powi(2) + (py - cy).powi(2)).sqrt();
                let outside = (rad - d + 0.5).clamp(0.0, 1.0);
                let inside = (d - inner + 0.5).clamp(0.0, 1.0);
                self.blend(x, y, c, outside.min(inside));
            }
        }
    }

    /// A filled circle.
    pub fn disc(&mut self, cx: f32, cy: f32, radius: f32, c: Rgb) {
        for y in (cy - radius - 1.0) as i32..=(cy + radius + 1.0) as i32 {
            for x in (cx - radius - 1.0) as i32..=(cx + radius + 1.0) as i32 {
                let d = ((x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt();
                self.blend(x, y, c, radius - d + 0.5);
            }
        }
    }

    /// A filled triangle.
    pub fn triangle(&mut self, p: [(f32, f32); 3], c: Rgb) {
        let (x0, x1) = (
            p.iter().map(|q| q.0).fold(f32::MAX, f32::min),
            p.iter().map(|q| q.0).fold(f32::MIN, f32::max),
        );
        let (y0, y1) = (
            p.iter().map(|q| q.1).fold(f32::MAX, f32::min),
            p.iter().map(|q| q.1).fold(f32::MIN, f32::max),
        );
        let edge = |a: (f32, f32), b: (f32, f32), x: f32, y: f32| {
            ((b.0 - a.0) * (y - a.1) - (b.1 - a.1) * (x - a.0)) / ((b.0 - a.0).hypot(b.1 - a.1))
        };
        let sign = if edge(p[0], p[1], p[2].0, p[2].1) > 0.0 {
            1.0
        } else {
            -1.0
        };
        for y in y0 as i32..=y1 as i32 + 1 {
            for x in x0 as i32..=x1 as i32 + 1 {
                let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
                let d = (0..3)
                    .map(|i| sign * edge(p[i], p[(i + 1) % 3], fx, fy))
                    .fold(f32::MAX, f32::min);
                self.blend(x, y, c, d + 0.5);
            }
        }
    }

    fn glyph(&mut self, ch: char, t: Type) -> (Metrics, Vec<u8>) {
        if let Some(g) = self.glyphs.get(&(ch, t)) {
            return g.clone();
        }
        let font = if t.bold { &self.bold } else { &self.regular };
        let g = font.rasterize(ch, t.size as f32);
        self.glyphs.insert((ch, t), g.clone());
        g
    }

    /// The width of `text` in type `t`.
    pub fn measure(&mut self, text: &str, t: Type) -> i32 {
        text.chars()
            .map(|c| self.glyph(c, t).0.advance_width)
            .sum::<f32>()
            .round() as i32
    }

    /// Writes `text` in `r`, vertically centred, aligned as asked; cut with an ellipsis if it
    /// doesn't fit.
    pub fn text(&mut self, r: Rect, text: &str, t: Type, c: Rgb, align: Align) {
        let mut s: String = text.to_owned();
        while self.measure(&s, t) > r.w && s.chars().count() > 1 {
            s.pop();
            while s.ends_with(' ') {
                s.pop();
            }
            s.pop();
            s.push('…');
        }
        let w = self.measure(&s, t);
        let mut x = match align {
            Align::Left => r.x as f32,
            Align::Centre => (r.x + (r.w - w) / 2) as f32,
            Align::Right => (r.x + r.w - w) as f32,
        };
        let ascent = t.size as f32 * 0.74;
        let baseline = r.y as f32 + (r.h as f32 + ascent) / 2.0;
        for ch in s.chars() {
            let (m, bitmap) = self.glyph(ch, t);
            let gx = (x + m.xmin as f32).round() as i32;
            let gy = (baseline - m.height as f32 - m.ymin as f32).round() as i32;
            for row in 0..m.height {
                for col in 0..m.width {
                    let a = bitmap[row * m.width + col] as f32 / 255.0;
                    self.blend(gx + col as i32, gy + row as i32, c, a);
                }
            }
            x += m.advance_width;
        }
    }

    /// A picture scaled to fit `at` by averaging (for thumbnails smaller than 1:1).
    pub fn picture_fit(&mut self, at: Rect, rgba: &[u8], sw: i32, sh: i32) {
        for y in 0..at.h {
            for x in 0..at.w {
                let sx = (x * sw / at.w) as usize;
                let sy = (y * sh / at.h) as usize;
                let s = (sy * sw as usize + sx) * 4;
                self.blend(at.x + x, at.y + y, [rgba[s], rgba[s + 1], rgba[s + 2]], 1.0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_fits_its_box() {
        let mut c = Canvas::new(400, 100);
        let long = "A rather long title for a game that will not fit";
        c.text(
            Rect::new(0, 0, 200, 50),
            long,
            BODY,
            colour::TEXT,
            Align::Left,
        );
        assert!(c.measure("Save", LABEL) < 100);
        // Something was drawn, and nothing right of the box.
        let lit = |x: i32, y: i32| c.px[((y * 400 + x) * 4) as usize] > 100;
        assert!((0..200).any(|x| (0..50).any(|y| lit(x, y))));
        assert!(!(205..400).any(|x| (0..50).any(|y| lit(x, y))));
    }

    #[test]
    fn rounded_corners_are_rounded() {
        let mut c = Canvas::new(100, 100);
        c.clear([0, 0, 0]);
        c.round(Rect::new(0, 0, 100, 100), 30, [255, 255, 255]);
        assert_eq!(c.px[0], 0, "the corner stays clear");
        assert_eq!(c.px[((50 * 100 + 50) * 4) as usize], 255);
    }
}
