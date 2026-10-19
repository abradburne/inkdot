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

/// Writes interleaved stereo 16-bit samples as a WAV file.
pub fn write_wav(path: &Path, samples: &[i16], rate: u32) {
    let data_len = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&2u16.to_le_bytes()); // stereo
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * 4).to_le_bytes());
    out.extend_from_slice(&4u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        out.extend_from_slice(&s.to_le_bytes());
    }
    std::fs::write(path, out).expect("write WAV");
}

/// A group of test ROMs run the same way.
pub struct Suite {
    /// As docs/accuracy.md names it.
    pub name: &'static str,
    /// The console its ROMs run on.
    pub model: Model,
    /// How its ROMs report.
    pub protocol: Protocol,
    /// Emulated time allowed per ROM.
    pub seconds: u32,
    /// Expected to pass in full (a target); otherwise tracked (a stretch goal).
    pub target: bool,
    /// Its ROMs, as found in tests/roms.
    pub roms: Vec<PathBuf>,
}

/// Every `.gb`/`.gbc` under a fetched folder whose path `filter` accepts, sorted.
pub fn roms(dir: &str, filter: impl Fn(&str) -> bool) -> Vec<PathBuf> {
    let root = tests_dir().join("roms").join(dir);
    let mut found = Vec::new();
    let mut stack = vec![root];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(d) else {
            continue;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "gb" || x == "gbc")
                && filter(&p.to_string_lossy())
            {
                found.push(p);
            }
        }
    }
    found.sort();
    found
}

/// The model suffix of a Mooneye test's name (`-GS`, `-dmgABC`, `-C`...), if any.
fn mooneye_suffix(name: &str) -> Option<String> {
    let stem = name.trim_end_matches(".gb");
    let file = stem.rsplit('/').next().unwrap_or("");
    file.rsplit_once('-').map(|(_, s)| s.to_owned())
}

/// The suites, in the order docs/accuracy.md lists them.
pub fn suites() -> Vec<Suite> {
    let blargg = |n: &str| {
        n.ends_with("cpu_instrs/cpu_instrs.gb")
            || n.ends_with("instr_timing.gb")
            || n.ends_with("mem_timing/mem_timing.gb")
            || n.ends_with("mem_timing-2/mem_timing.gb")
            || n.ends_with("halt_bug.gb")
    };
    let dmg_only = ["dmg0", "mgb", "S", "sgb", "sgb2"];
    let not_cgb = [
        "dmg0",
        "dmgABC",
        "dmgABCmgb",
        "mgb",
        "S",
        "sgb",
        "sgb2",
        "GS",
    ];
    let suite = |name, model, protocol, seconds, target, roms| Suite {
        name,
        model,
        protocol,
        seconds,
        target,
        roms,
    };
    vec![
        suite(
            "Blargg CPU and timing (DMG)",
            Model::Dmg,
            Protocol::Blargg,
            60,
            true,
            roms("blargg", blargg),
        ),
        suite(
            "Blargg CPU and timing (CGB)",
            Model::Cgb,
            Protocol::Blargg,
            60,
            true,
            roms("blargg", blargg),
        ),
        suite(
            "Blargg dmg_sound (DMG)",
            Model::Dmg,
            Protocol::Blargg,
            60,
            true,
            roms("blargg/dmg_sound/rom_singles", |_| true),
        ),
        suite(
            "Blargg cgb_sound (CGB)",
            Model::Cgb,
            Protocol::Blargg,
            60,
            true,
            roms("blargg/cgb_sound/rom_singles", |_| true),
        ),
        suite(
            "Blargg interrupt_time (CGB)",
            Model::Cgb,
            Protocol::Blargg,
            60,
            true,
            roms("blargg/interrupt_time", |_| true),
        ),
        suite(
            "Mooneye acceptance (DMG-ABC)",
            Model::Dmg,
            Protocol::Mooneye,
            20,
            true,
            roms("mooneye/acceptance", |n| {
                !mooneye_suffix(n).is_some_and(|s| dmg_only.contains(&s.as_str()))
            }),
        ),
        suite(
            "Mooneye acceptance, model-independent (CGB)",
            Model::Cgb,
            Protocol::Mooneye,
            20,
            true,
            roms("mooneye/acceptance", |n| {
                !mooneye_suffix(n).is_some_and(|s| not_cgb.contains(&s.as_str()))
            }),
        ),
        suite(
            "Mooneye misc (CGB-C/E)",
            Model::Cgb,
            Protocol::Mooneye,
            20,
            true,
            roms("mooneye/misc", |n| {
                mooneye_suffix(n).is_some_and(|s| matches!(s.as_str(), "C" | "cgb" | "cgbABCDE"))
            }),
        ),
        suite(
            "Mooneye emulator-only (MBC1, MBC2, MBC5)",
            Model::Dmg,
            Protocol::Mooneye,
            20,
            true,
            roms("mooneye/emulator-only", |_| true),
        ),
        suite(
            "Blargg oam_bug (DMG)",
            Model::Dmg,
            Protocol::Blargg,
            60,
            false,
            roms("blargg/oam_bug/rom_singles", |_| true),
        ),
    ]
}

/// Runs a suite's ROMs in parallel; each ROM with its outcome, in order.
pub fn run_suite(suite: &Suite) -> Vec<(PathBuf, Outcome)> {
    std::thread::scope(|s| {
        let handles: Vec<_> = suite
            .roms
            .iter()
            .map(|rom| {
                s.spawn(move || {
                    (
                        rom.clone(),
                        run(rom, suite.model, suite.protocol, suite.seconds),
                    )
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    })
}

/// docs/accuracy.md: every suite's results, and the acid2 pictures.
pub fn accuracy_report() -> String {
    let mut out = String::from(
        "# Accuracy\n\nGenerated by `inkdot-test --accuracy docs/accuracy.md` from the test ROMs that\n\
         `tools/fetch-test-roms.sh` fetches; `cargo test` runs the same lists, and every suite marked\n\
         *target* must pass in full. Stretch suites are tracked, not required.\n\n\
         | Suite | Kind | Passed |\n|---|---|---|\n",
    );
    let mut open = Vec::new();
    for suite in suites() {
        let results = run_suite(&suite);
        let passed = results.iter().filter(|(_, o)| *o == Outcome::Pass).count();
        let kind = if suite.target { "target" } else { "stretch" };
        out += &format!(
            "| {} | {kind} | {passed} / {} |\n",
            suite.name,
            results.len()
        );
        for (rom, o) in results.into_iter().filter(|(_, o)| *o != Outcome::Pass) {
            let name = rom
                .strip_prefix(tests_dir().join("roms"))
                .unwrap_or(&rom)
                .display()
                .to_string();
            open.push(format!("- {}: `{name}`", suite.name));
            let _ = o;
        }
    }
    out += "\n| Picture | Reference | Result |\n|---|---|---|\n";
    for (rom, reference, model, name) in [
        (
            "dmg-acid2/dmg-acid2.gb",
            "dmg-acid2-ref/reference-dmg.png",
            Model::Dmg,
            "dmg-acid2 on a DMG",
        ),
        (
            "dmg-acid2/dmg-acid2.gb",
            "dmg-acid2-cgb-ref/reference-cgb.png",
            Model::Cgb,
            "dmg-acid2 on a CGB",
        ),
        (
            "cgb-acid2/cgb-acid2.gbc",
            "cgb-acid2-ref/reference.png",
            Model::Cgb,
            "cgb-acid2",
        ),
    ] {
        let (r, f) = (
            tests_dir().join("roms").join(rom),
            tests_dir().join("roms").join(reference),
        );
        let result = if r.exists() && f.exists() {
            let wrong = compare_with_reference(&r, model, &f).len();
            if wrong == 0 {
                "identical".to_owned()
            } else {
                format!("{wrong} pixels differ")
            }
        } else {
            "not fetched".to_owned()
        };
        out += &format!("| {name} | `{reference}` | {result} |\n");
    }
    if !open.is_empty() {
        out += "\n## Not passing yet\n\n";
        out += &open.join("\n");
        out += "\n";
    }
    out
}
