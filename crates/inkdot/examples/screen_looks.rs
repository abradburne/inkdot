//! Renders the screen textures on the CPU (the shader's sum, `lcd::render`) to PNG files, to look
//! at them without the handheld: `cargo run --release --example screen_looks -- OUT_DIR`.
//!
//! The pictures are the acid2 suites' reference images (fetched by `tools/fetch-test-roms.sh`)
//! and a test card of our own. For each, `before-*.png` is the old texture (a grid mixed 35%
//! toward a gap colour), then one file per texture at the handheld's 5× (800×720), at 5⅓×
//! (853×768, a non-whole size), and a 4× enlarged crop of the 5× picture's middle.

// The example uses part of the program's modules.
#![allow(dead_code)]

#[path = "../src/lcd.rs"]
mod lcd;
#[path = "../src/looks.rs"]
mod looks;

use lcd::{Panel, Pattern, Shading, Style};
use looks::Look;
use std::path::{Path, PathBuf};

fn load(path: &Path) -> Option<Vec<u8>> {
    let file = std::fs::File::open(path).ok()?;
    let mut decoder = png::Decoder::new(std::io::BufReader::new(file));
    // Palettes and small bit depths as plain 8-bit samples.
    decoder.set_transformations(png::Transformations::EXPAND);
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;
    if (info.width, info.height) != (160, 144) {
        return None;
    }
    let px = info.color_type.samples();
    Some(
        buf[..info.buffer_size()]
            .chunks(px)
            .flat_map(|c| match px {
                1 | 2 => [c[0], c[0], c[0], 255],
                _ => [c[0], c[1], c[2], 255],
            })
            .collect(),
    )
}

fn save(path: PathBuf, w: usize, h: usize, rgba: &[u8]) {
    let file = std::fs::File::create(&path).expect("the output folder is writable");
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w as u32, h as u32);
    enc.set_color(png::ColorType::Rgba);
    enc.write_header()
        .and_then(|mut wr| wr.write_image_data(rgba))
        .expect("PNG written");
    println!("{}", path.display());
}

/// A grey picture (four levels) in a DMG look's shades.
fn shade(grey: &[u8], look: Look) -> Vec<u8> {
    let s = look.shades();
    grey.chunks(4)
        .flat_map(|p| {
            let i = 3 - (p[0] as usize * 4 / 256).min(3);
            [s[i][0], s[i][1], s[i][2], 255]
        })
        .collect()
}

/// A test card: colour bars, a grey ramp, fine checks and a dark shape on light.
fn card() -> Vec<u8> {
    let mut f = vec![255u8; 160 * 144 * 4];
    let bars: [[u8; 3]; 8] = [
        [255, 255, 255],
        [255, 255, 0],
        [0, 255, 255],
        [0, 255, 0],
        [255, 0, 255],
        [255, 0, 0],
        [0, 0, 255],
        [0, 0, 0],
    ];
    for y in 0..144 {
        for x in 0..160 {
            let c = if y < 48 {
                bars[x / 20]
            } else if y < 72 {
                let v = (x * 255 / 159) as u8;
                [v; 3]
            } else if y < 100 {
                if (x + y) % 2 == 0 {
                    [0x20, 0x30, 0x80]
                } else {
                    [0xf0, 0xe0, 0x90]
                }
            } else {
                let (dx, dy) = (x as i32 - 80, y as i32 - 122);
                if dx * dx + dy * dy * 4 < 400 {
                    [0x30, 0x30, 0x30]
                } else {
                    [0xd0, 0xd8, 0xc0]
                }
            };
            f[(y * 160 + x) * 4..][..3].copy_from_slice(&c);
        }
    }
    f
}

/// The old texture: the last column and bottom row of each 5×5 square mixed 35% toward `gap`.
fn before(frame: &[u8], gap: [u8; 3]) -> Vec<u8> {
    let mut out = Vec::with_capacity(800 * 720 * 4);
    for y in 0..720 {
        for x in 0..800 {
            let p = &frame[((y / 5) * 160 + x / 5) * 4..][..3];
            let edge = x % 5 == 4 || y % 5 == 4;
            for c in 0..3 {
                let v = p[c] as f32;
                let mixed = if edge {
                    v + (gap[c] as f32 - v) * 0.35
                } else {
                    v
                };
                out.push(mixed.round() as u8);
            }
            out.push(255);
        }
    }
    out
}

/// A `w`×`h` crop at (`x`, `y`), enlarged `k` times.
fn crop(src: &[u8], sw: usize, (x, y, w, h): (usize, usize, usize, usize), k: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(w * h * k * k * 4);
    for oy in 0..h * k {
        for ox in 0..w * k {
            let i = ((y + oy / k) * sw + x + ox / k) * 4;
            out.extend_from_slice(&src[i..i + 4]);
        }
    }
    out
}

fn main() {
    let out = PathBuf::from(
        std::env::args()
            .nth(1)
            .expect("usage: screen_looks OUT_DIR"),
    );
    std::fs::create_dir_all(&out).expect("the output folder can be made");
    let roms = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/roms");
    let mut pictures: Vec<(String, Vec<u8>, Look)> = vec![("card".into(), card(), Look::Color)];
    if let Some(grey) = load(&roms.join("dmg-acid2-ref/reference-dmg.png")) {
        for look in [Look::GameBoy, Look::Pocket, Look::Light] {
            pictures.push((format!("dmg-{look:?}"), shade(&grey, look), look));
        }
    }
    if let Some(cgb) = load(&roms.join("cgb-acid2-ref/reference.png")) {
        pictures.push(("cgb".into(), cgb, Look::Color));
    }
    let styles = Style::CHOICES
        .iter()
        .filter_map(|(v, _)| Style::parse(v).map(|s| (*v, s)));
    let styles: Vec<_> = styles.collect();
    let middle = (300, 260, 200, 150);
    for (name, frame, look) in &pictures {
        let panel = match look {
            Look::GameBoy => Panel::Dmg,
            Look::Pocket | Look::Grey => Panel::Pocket,
            Look::Light => Panel::Light,
            Look::Color => Panel::Gbc,
        };
        let paper = look.shades()[0];
        let gap = if *look == Look::Color {
            [0xe8; 3]
        } else {
            paper
        };
        let old = before(frame, gap);
        save(out.join(format!("{name}-before.png")), 800, 720, &old);
        save(
            out.join(format!("{name}-before-zoom.png")),
            800,
            600,
            &crop(&old, 800, middle, 4),
        );
        for (value, style) in &styles {
            let pattern = Pattern::new(*style, panel);
            let shading = Shading {
                persistence: 0.0,
                paper,
                shadow: pattern.shadow,
            };
            let t = std::time::Instant::now();
            let mask = pattern.build(800, 720, 5.0);
            let built = t.elapsed();
            let px = lcd::render(frame, frame, &mask, 5.0, shading);
            save(out.join(format!("{name}-{value}.png")), 800, 720, &px);
            save(
                out.join(format!("{name}-{value}-zoom.png")),
                800,
                600,
                &crop(&px, 800, middle, 4),
            );
            let cell = 853.0 / 160.0;
            let wide = pattern.build(853, 768, cell);
            let px = lcd::render(frame, frame, &wide, cell, shading);
            save(out.join(format!("{name}-{value}-853.png")), 853, 768, &px);
            println!("  mask built in {built:?}, gain {:.3}", mask.gain);
        }
    }
}
