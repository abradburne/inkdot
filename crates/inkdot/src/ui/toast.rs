//! A notice over the game on the top screen, in DSlot's style: a small rounded panel with a
//! heading, a line and a line under it, drawn with DSlot's face into a picture of its own size
//! with alpha, laid at the bottom right of the screen for a few seconds. The same panel as
//! DSperate's (nibkit-menu-c's toast, for C apps); drawn once when the notice appears, not every
//! frame. Until nibkit has it, it is here.

use nibkit::menu::colour::{self, Rgb};
use nibkit::menu::raster::{Canvas, Fonts};

/// What a notice is about: the heading's colour says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Something to know.
    Info,
    /// An achievement earned: gold.
    Unlock,
    /// Something went wrong: red.
    Problem,
}

/// In screen units (the screen is 1024 wide).
const PAD: f32 = 16.0;
const RADIUS: f32 = 14.0;
const EDGE: f32 = 2.0;
const GAP: f32 = 14.0;
const HEAD: (f32, f32) = (18.0, 26.0);
const TITLE: (f32, f32) = (24.0, 32.0);
const DETAIL: (f32, f32) = (20.0, 28.0);
const POINTS: f32 = 20.0;
/// The panel's opacity: the game shows through a little.
const OPACITY: f32 = 0.94;
/// The narrowest a notice is, so a short one doesn't look like a label.
const LEAST: f32 = 260.0;
/// The widest, in units: two thirds of the screen.
const MOST: f32 = nibkit::menu::W * 2.0 / 3.0;
/// The panel's edge.
const EDGE_COLOUR: Rgb = [52, 60, 80];

/// What a notice says. A heading or a detail may be empty and is then left out.
pub struct Said<'a> {
    pub kind: Kind,
    pub header: &'a str,
    pub title: &'a str,
    pub detail: &'a str,
    /// An unlock's value; 0 for none.
    pub points: u32,
}

/// `s` cut to `width` at `size`, ending in an ellipsis when it had to be cut.
fn fit(f: &Fonts, s: &str, (size, bold): (f32, bool), width: f32) -> String {
    if f.width(s, size, bold) <= width {
        return s.to_owned();
    }
    let mut out: String = s.to_owned();
    while !out.is_empty() && f.width(&format!("{out}\u{2026}"), size, bold) > width {
        out.pop();
    }
    format!("{}\u{2026}", out.trim_end())
}

/// Which of the lines is the title: the second when there is a heading.
fn title_at<T>(lines: &[(&str, (f32, f32), T)]) -> usize {
    usize::from(lines.len() > 1 && lines[1].1 == TITLE)
}

/// Draws the notice at `scale` pixels to a unit: RGBA with straight alpha, rows top first, its
/// own size.
pub fn draw(f: &Fonts, said: &Said, scale: f32) -> Canvas {
    let points = (said.points > 0).then(|| format!("{} points", said.points));
    let lines = [
        (!said.header.is_empty()).then_some((said.header, HEAD, true)),
        Some((said.title, TITLE, true)),
        (!said.detail.is_empty()).then_some((said.detail, DETAIL, false)),
    ];
    let lines: Vec<_> = lines.into_iter().flatten().collect();
    let text_h: f32 = lines.iter().map(|(_, (_, step), _)| step).sum::<f32>() - 4.0;
    let left = PAD;
    let points_w = points
        .as_ref()
        .map_or(0.0, |p| f.width(p, POINTS, true) + GAP);
    // As wide as the longest line wants, within MOST.
    let want = lines
        .iter()
        .enumerate()
        .map(|(i, (s, (size, _), bold))| {
            f.width(s, *size, *bold) + if i == title_at(&lines) { points_w } else { 0.0 }
        })
        .fold(0.0f32, f32::max);
    let w = (left + want + PAD + 2.0).clamp(LEAST, MOST).ceil();
    let h = (PAD * 2.0 + text_h).ceil();
    let room = w - left - PAD;

    let (pw, ph) = ((w * scale).ceil() as usize, (h * scale).ceil() as usize);
    let mut c = Canvas {
        w: pw,
        h: ph,
        px: vec![0; pw * ph * 4],
        scale,
    };
    panel(&mut c, w, h);
    let accent = match said.kind {
        Kind::Unlock => colour::GOLD,
        Kind::Problem => colour::RED,
        Kind::Info => colour::DIM,
    };
    let mut y = PAD - 3.0;
    for (i, (s, (size, step), bold)) in lines.iter().enumerate() {
        let is_title = i == title_at(&lines);
        let (colour, width) = match (i, is_title) {
            (_, true) => (colour::WHITE, room - points_w),
            (0, false) => (accent, room),
            _ => (colour::DIM, room),
        };
        let s = fit(f, s, (*size, *bold), width.max(0.0));
        c.text(f, &s, (*size, *bold), (left, y), 0.0, colour);
        if is_title {
            if let Some(p) = &points {
                c.text(
                    f,
                    p,
                    (POINTS, true),
                    (w - PAD, y + (size - POINTS) / 2.0 + 1.0),
                    1.0,
                    colour::GOLD,
                );
            }
        }
        y += step;
    }
    c
}

/// The panel, `w` x `h` units: its colour inside a thin edge, the corners rounded, and its shape
/// in the picture's alpha (a little see-through). Pixels near a corner are sampled 4 x 4 for a
/// smooth curve; the rest are wholly in, so drawing it costs a pass over the pixels and no more.
fn panel(c: &mut Canvas, w: f32, h: f32) {
    let s = c.scale;
    let inner = (EDGE, EDGE, w - 2.0 * EDGE, h - 2.0 * EDGE);
    let near = |v: f32, len: f32| v < RADIUS + 1.0 || v > len - RADIUS - 1.0;
    for py in 0..c.h {
        for px in 0..c.w {
            let (ux, uy) = ((px as f32 + 0.5) / s, (py as f32 + 0.5) / s);
            let (outside, edge) = if near(ux, w) && near(uy, h) {
                let (mut o, mut e) = (0u32, 0u32);
                for k in 0..16 {
                    let sx = (px as f32 + ((k % 4) as f32 + 0.5) / 4.0) / s;
                    let sy = (py as f32 + ((k / 4) as f32 + 0.5) / 4.0) / s;
                    if rounded(sx, sy, (0.0, 0.0, w, h), RADIUS) {
                        o += 1;
                        e += u32::from(!rounded(sx, sy, inner, RADIUS - EDGE));
                    }
                }
                (
                    o as f32 / 16.0,
                    if o > 0 { e as f32 / o as f32 } else { 0.0 },
                )
            } else {
                let in_inner = ux >= EDGE && ux <= w - EDGE && uy >= EDGE && uy <= h - EDGE;
                (1.0, if in_inner { 0.0 } else { 1.0 })
            };
            let i = (py * c.w + px) * 4;
            for ((d, p), e) in c.px[i..i + 3]
                .iter_mut()
                .zip(colour::PANEL)
                .zip(EDGE_COLOUR)
            {
                *d = (p as f32 + (e as f32 - p as f32) * edge).round() as u8;
            }
            c.px[i + 3] = (outside * OPACITY * 255.0).round() as u8;
        }
    }
}

/// Whether a point is inside a rectangle with rounded corners.
fn rounded(x: f32, y: f32, (rx, ry, rw, rh): (f32, f32, f32, f32), r: f32) -> bool {
    let dx = (x - rx - rw / 2.0).abs() - rw / 2.0 + r;
    let dy = (y - ry - rh / 2.0).abs() - rh / 2.0 + r;
    dx.max(0.0).hypot(dy.max(0.0)) + dx.max(dy).min(0.0) - r < 0.0
}

/// Lays `notice` over `screen` (a screen-sized canvas, premultiplied RGBA) at its bottom right,
/// `margin` units in.
pub fn lay(screen: &mut Canvas, notice: &Canvas, margin: f32) {
    let m = (margin * screen.scale).round() as usize;
    let x0 = screen.w.saturating_sub(notice.w + m);
    let y0 = screen.h.saturating_sub(notice.h + m);
    for y in 0..notice.h.min(screen.h - y0) {
        for x in 0..notice.w.min(screen.w - x0) {
            let s = (y * notice.w + x) * 4;
            let d = ((y0 + y) * screen.w + x0 + x) * 4;
            let a = notice.px[s + 3] as u32;
            for k in 0..3 {
                screen.px[d + k] = ((notice.px[s + k] as u32 * a + 127) / 255) as u8;
            }
            screen.px[d + 3] = a as u8;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alpha(c: &Canvas, x: usize, y: usize) -> u8 {
        c.px[(y * c.w + x) * 4 + 3]
    }

    #[test]
    fn an_unlock_with_points() {
        let f = Fonts::new();
        let said = Said {
            kind: Kind::Unlock,
            header: "Achievement unlocked",
            title: "First Blood",
            detail: "Win a battle",
            points: 25,
        };
        let c = draw(&f, &said, 1.0);
        assert_eq!(c.h, (PAD * 2.0 + 26.0 + 32.0 + 28.0 - 4.0).ceil() as usize);
        assert!(c.w >= LEAST as usize && c.w as f32 <= MOST);
        // Corners clear, the middle all but opaque.
        assert_eq!(alpha(&c, 0, 0), 0);
        assert_eq!(alpha(&c, c.w / 2, c.h / 2), (OPACITY * 255.0).round() as u8);
        // Laid at the bottom right of a clear screen, premultiplied.
        let mut screen = Canvas::new(1.0);
        screen.px.fill(0);
        lay(&mut screen, &c, 16.0);
        assert_eq!(alpha(&screen, 0, 0), 0);
        let (x, y) = (screen.w - 16 - c.w / 2, screen.h - 16 - c.h / 2);
        assert_eq!(alpha(&screen, x, y), (OPACITY * 255.0).round() as u8);
        let i = (y * screen.w + x) * 4;
        assert!(screen.px[i] <= colour::PANEL[0]);
    }

    #[test]
    fn long_lines_are_cut_to_the_width() {
        let f = Fonts::new();
        let long = "A very long achievement description that goes on and on past any notice";
        let cut = fit(&f, long, (20.0, false), 200.0);
        assert!(cut.ends_with('\u{2026}'));
        assert!(f.width(&cut, 20.0, false) <= 200.0);
        assert_eq!(fit(&f, "Short", (20.0, false), 200.0), "Short");
        let said = Said {
            kind: Kind::Info,
            header: "",
            title: long,
            detail: "",
            points: 0,
        };
        let c = draw(&f, &said, 1.0);
        assert_eq!(c.w, MOST.ceil() as usize);
        assert_eq!(c.h, (PAD * 2.0 + 32.0 - 4.0).ceil() as usize);
    }
}
