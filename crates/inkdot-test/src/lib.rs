//! inkdot-test: running inkdot-core headless, for the test suites and benchmarks.
//!
//! Each test ROM suite reports its result its own way; [`run`] knows them all and turns them
//! into an [`Outcome`]:
//!
//! - **Blargg**: text sent over the serial port, or (the newer ROMs) a status byte and text at
//!   $A000 in cartridge RAM, behind the signature `DE B0 61`.
//! - **Mooneye**: the ROM executes `LD B,B` when done, with B C D E H L = 3 5 8 13 21 34 for a
//!   pass (the Fibonacci numbers) or all $42 for a failure.
//! - **acid2**: the picture after a few frames, compared with the suite's reference image.

use inkdot_core::{GameBoy, Model};
use std::path::{Path, PathBuf};

/// The repository's `tests/` folder, where `tools/fetch-test-roms.sh` puts the test ROMs.
pub fn tests_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests")
}

/// A fetched test file, or `None` (and a note on stderr) when the ROMs haven't been fetched, so
/// that `cargo test` still passes on a fresh checkout; `make check` fetches them first.
pub fn fetched(path: &str) -> Option<PathBuf> {
    let p = tests_dir().join("roms").join(path);
    if p.exists() {
        Some(p)
    } else {
        eprintln!(
            "skipped: {} not fetched (tools/fetch-test-roms.sh)",
            p.display()
        );
        None
    }
}

/// How a test ROM finished.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// It reported a pass.
    Pass,
    /// It reported a failure, with what it said.
    Fail(String),
    /// It said nothing conclusive in the time allowed; what it had said so far.
    Timeout(String),
}

/// How a suite reports its results.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Protocol {
    /// Blargg's: serial text or the $A000 text, "Passed" or "Failed".
    Blargg,
    /// Mooneye's: Fibonacci registers at `LD B,B`.
    Mooneye,
}

/// Loads a ROM into a machine.
pub fn boot(path: &Path, model: Model) -> GameBoy {
    let rom = std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    GameBoy::new(rom, model).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// Runs a test ROM for at most `seconds` of emulated time.
pub fn run(path: &Path, model: Model, protocol: Protocol, seconds: u32) -> Outcome {
    let mut gb = boot(path, model);
    match protocol {
        Protocol::Blargg => run_blargg(&mut gb, seconds),
        Protocol::Mooneye => run_mooneye(&mut gb, seconds),
    }
}

/// 2^22 dots a second.
const SECOND: u64 = 1 << 22;

fn blargg_ram_text(gb: &GameBoy) -> Option<(u8, String)> {
    let hw = &gb.hw;
    if [hw.peek(0xa001), hw.peek(0xa002), hw.peek(0xa003)] != [0xde, 0xb0, 0x61] {
        return None;
    }
    let mut text = String::new();
    for addr in 0xa004..0xbfff {
        match hw.peek(addr) {
            0 => break,
            c => text.push(c as char),
        }
    }
    Some((hw.peek(0xa000), text))
}

/// The text on screen, read from the background tile map at $9800: Blargg's ROMs number their
/// font's tiles by ASCII code, so a tile number is the character it shows.
pub fn screen_text(gb: &GameBoy) -> String {
    let vram = &gb.hw.ppu.vram;
    let mut text = String::new();
    for row in 0..18 {
        let line: String = (0..20)
            .map(|col| vram[0x1800 + row * 32 + col])
            .map(|t| {
                if (0x20..0x7f).contains(&t) {
                    t as char
                } else {
                    ' '
                }
            })
            .collect();
        text.push_str(line.trim_end());
        text.push('\n');
    }
    text
}

fn run_blargg(gb: &mut GameBoy, seconds: u32) -> Outcome {
    let end = gb.hw.dots + seconds as u64 * SECOND;
    while gb.hw.dots < end {
        gb.run_dots(SECOND / 16);
        let serial = String::from_utf8_lossy(&gb.hw.serial.sent).into_owned();
        if serial.contains("Passed") {
            return Outcome::Pass;
        }
        if serial.contains("Failed") {
            return Outcome::Fail(serial);
        }
        let screen = screen_text(gb);
        if screen.contains("Passed") {
            return Outcome::Pass;
        }
        if screen.contains("Failed") {
            return Outcome::Fail(screen);
        }
        if let Some((status, text)) = blargg_ram_text(gb) {
            if status != 0x80 {
                return if status == 0 {
                    Outcome::Pass
                } else {
                    Outcome::Fail(text)
                };
            }
        }
    }
    let said = blargg_ram_text(gb).map(|(_, t)| t);
    Outcome::Timeout(said.unwrap_or_else(|| String::from_utf8_lossy(&gb.hw.serial.sent).into()))
}

fn run_mooneye(gb: &mut GameBoy, seconds: u32) -> Outcome {
    let end = gb.hw.dots + seconds as u64 * SECOND;
    while gb.hw.dots < end {
        let at_ld_b_b = gb.hw.peek(gb.cpu.pc) == 0x40;
        gb.step();
        if at_ld_b_b {
            let c = &gb.cpu;
            let regs = [c.b, c.c, c.d, c.e, c.h, c.l];
            return if regs == [3, 5, 8, 13, 21, 34] {
                Outcome::Pass
            } else {
                Outcome::Fail(format!("registers {regs:02x?}"))
            };
        }
    }
    Outcome::Timeout(String::new())
}

/// The picture as 8-bit RGB, converted the way the acid2 suites' reference images were made:
/// DMG shades as $FF $AA $55 $00, CGB colours with each 5-bit component as `(c << 3) | (c >> 2)`.
pub fn frame_rgb(gb: &GameBoy) -> Vec<[u8; 3]> {
    let color = gb.hw.ppu.is_color();
    gb.frame()
        .iter()
        .map(|&p| {
            if color {
                let c = |shift: u16| {
                    let v = ((p >> shift) & 0x1f) as u8;
                    (v << 3) | (v >> 2)
                };
                [c(0), c(5), c(10)]
            } else {
                let g = [0xff, 0xaa, 0x55, 0x00][p as usize & 3];
                [g, g, g]
            }
        })
        .collect()
}

/// Reads a PNG as 8-bit RGB pixels.
pub fn read_png(path: &Path) -> (u32, u32, Vec<[u8; 3]>) {
    let mut decoder = png::Decoder::new(std::fs::File::open(path).expect("open PNG"));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().expect("PNG header");
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).expect("PNG data");
    let channels = info.color_type.samples();
    let pixels = buf[..info.buffer_size()]
        .chunks(channels)
        .map(|c| {
            if channels < 3 {
                [c[0], c[0], c[0]]
            } else {
                [c[0], c[1], c[2]]
            }
        })
        .collect();
    (info.width, info.height, pixels)
}

/// Runs a ROM until it executes `LD B,B` (how the acid2 suites say they are done) or for
/// `seconds`, then two more frames so the last picture is complete, and compares the picture
/// with a reference image. Returns the pixels that differ, as (x, y).
pub fn compare_with_reference(rom: &Path, model: Model, reference: &Path) -> Vec<(usize, usize)> {
    let mut gb = boot(rom, model);
    let end = gb.hw.dots + 10 * SECOND;
    while gb.hw.dots < end {
        let at_ld_b_b = gb.hw.peek(gb.cpu.pc) == 0x40;
        gb.step();
        if at_ld_b_b {
            break;
        }
    }
    gb.run_frame();
    gb.run_frame();
    let (w, h, want) = read_png(reference);
    assert_eq!(
        (w, h),
        (160, 144),
        "{}: not a 160x144 picture",
        reference.display()
    );
    let got = frame_rgb(&gb);
    (0..want.len())
        .filter(|&i| got[i] != want[i])
        .map(|i| (i % 160, i / 160))
        .collect()
}

/// Writes the picture to a PNG, for looking at a failure.
pub fn write_png(gb_rgb: &[[u8; 3]], path: &Path) {
    let file = std::fs::File::create(path).expect("create PNG");
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), 160, 144);
    enc.set_color(png::ColorType::Rgb);
    let mut w = enc.write_header().expect("PNG header");
    w.write_image_data(&gb_rgb.concat()).expect("PNG data");
}
