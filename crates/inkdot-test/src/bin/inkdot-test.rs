//! `inkdot-test [--cgb] [--mooneye] [--break ADDR] ROM...`: runs test ROMs and prints each
//! one's outcome. `--break` prints the CPU's registers each time PC reaches ADDR (hex) instead,
//! for the first few seconds, to see what a test measured. `--io` prints $FF00-$FF7F as the
//! machine starts. `--wav FILE` runs the first ROM for 10 seconds and writes its sound.
//! `--accuracy FILE` runs every suite and writes the results as Markdown (docs/accuracy.md).
//! `--bench` runs each ROM for 3600 frames as fast as it can and reports frames per second and
//! the time per frame.
//!
//! `--play SCRIPT --frames N [--every K] [--shots DIR]` plays the first ROM with a button script
//! (see [`inkdot_test::Script`]) and every K frames (60) prints the frame's hash, PC, LY, IE/IF and
//! the CPU's mode, and with `--shots` writes the picture: to see where a game stops.
//!
//! `--check-shot PNG [--frames N]` runs the ROM N frames (600) and checks a capture of inkdot's
//! top screen (INKDOT_DEBUG=shot, taken at frame 600) against it ([`inkdot_test::check_shot`]).

use inkdot_core::Model;
use inkdot_test::{boot, run, Outcome, Protocol};
use std::path::Path;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let model = if args.iter().any(|a| a == "--cgb") {
        Model::Cgb
    } else {
        Model::Dmg
    };
    let protocol = if args.iter().any(|a| a == "--mooneye") {
        Protocol::Mooneye
    } else {
        Protocol::Blargg
    };
    let brk = args
        .iter()
        .position(|a| a == "--break")
        .and_then(|i| args.get(i + 1))
        .map(|a| u16::from_str_radix(a.trim_start_matches("0x"), 16).expect("--break ADDR (hex)"));
    let roms: Vec<&String> = args
        .iter()
        .enumerate()
        .filter(|(i, a)| {
            !a.starts_with("--")
                && (*i == 0
                    || ![
                        "--break", "--wav", "--play", "--frames", "--every", "--shots",
                    ]
                    .contains(&args[i - 1].as_str()))
        })
        .map(|(_, a)| a)
        .collect();
    if let Some(wav) = args
        .iter()
        .position(|a| a == "--wav")
        .and_then(|i| args.get(i + 1))
    {
        let rom = roms.iter().find(|r| **r != wav).expect("a ROM");
        let mut gb = boot(Path::new(rom), model);
        let mut samples = Vec::new();
        for _ in 0..600 {
            gb.run_frame();
            gb.take_audio(&mut samples);
        }
        inkdot_test::write_wav(Path::new(wav), &samples, 48_000);
        return;
    }
    let value = |flag: &str| {
        args.iter()
            .position(|a| a == flag)
            .and_then(|i| args.get(i + 1))
    };
    if let Some(shot) = value("--check-shot") {
        let frames: u64 = value("--frames").map_or(600, |n| n.parse().expect("--frames N"));
        let (w, _, rgb) = inkdot_test::read_png(Path::new(shot));
        let mut gb = boot(Path::new(roms[0]), model);
        for _ in 0..frames {
            gb.run_frame();
        }
        let wrong = inkdot_test::check_shot(&rgb, w as usize, gb.frame());
        for line in &wrong {
            println!("{line}");
        }
        println!(
            "{}",
            if wrong.is_empty() {
                "capture OK"
            } else {
                "capture WRONG"
            }
        );
        std::process::exit(if wrong.is_empty() { 0 } else { 1 });
    }
    if let Some(script) = value("--play") {
        let script = std::fs::read_to_string(script).expect("the script");
        let script = inkdot_test::Script::parse(&script).expect("a valid script");
        let frames: u64 = value("--frames").map_or(3600, |n| n.parse().expect("--frames N"));
        let every: u64 = value("--every").map_or(60, |n| n.parse().expect("--every K"));
        let shots = value("--shots").map(std::path::PathBuf::from);
        let mut gb = boot(Path::new(roms[0]), model);
        inkdot_test::play(&mut gb, &script, frames, |f, gb| {
            if (f + 1) % every != 0 {
                return;
            }
            let hw = &gb.hw;
            println!(
                "frame {:>6} hash {:016x} pc {:04x} ly {:3} lyc {:3} ie {:02x} if {:02x} stat {:02x} lcdc {:02x} {:?} ime {} ei {} buttons {:02x}",
                f + 1,
                inkdot_test::frame_hash(gb),
                gb.cpu.pc,
                hw.peek(0xff44),
                hw.peek(0xff45),
                hw.peek(0xffff),
                hw.peek(0xff0f),
                hw.peek(0xff41),
                hw.peek(0xff40),
                gb.cpu.mode,
                gb.cpu.ime,
                gb.cpu.ei_pending,
                script.at(f),
            );
            if std::env::var_os("INKDOT_DUMP").is_some() {
                let d: Vec<String> = (0xd4fd..0xd520)
                    .map(|a| format!("{:02x}", hw.peek(a)))
                    .collect();
                println!(
                    "  d4fd: {} d6ef {:02x} ffaa {:02x}",
                    d.join(" "),
                    hw.peek(0xd6ef),
                    hw.peek(0xffaa)
                );
            }
            if let Some(dir) = &shots {
                let _ = std::fs::create_dir_all(dir);
                inkdot_test::write_png(
                    &inkdot_test::frame_rgb(gb),
                    &dir.join(format!("{:06}.png", f + 1)),
                );
            }
        });
        return;
    }
    if let Some(out) = args
        .iter()
        .position(|a| a == "--accuracy")
        .and_then(|i| args.get(i + 1))
    {
        std::fs::write(out, inkdot_test::accuracy_report()).expect("write the report");
        return;
    }
    if args.iter().any(|a| a == "--bench") {
        for rom in &roms {
            let mut gb = boot(Path::new(rom), model);
            let mut samples = Vec::new();
            let mut times = Vec::with_capacity(3600);
            let start = std::time::Instant::now();
            for _ in 0..3600 {
                let t = std::time::Instant::now();
                gb.run_frame();
                gb.take_audio(&mut samples);
                samples.clear();
                times.push(t.elapsed().as_secs_f64() * 1000.0);
            }
            let total = start.elapsed().as_secs_f64();
            times.sort_by(f64::total_cmp);
            let pct = |p: f64| times[((times.len() - 1) as f64 * p) as usize];
            println!(
                "{rom}: {:.0} frames/s; ms per frame: median {:.3}, p99 {:.3}, max {:.3}",
                3600.0 / total,
                pct(0.5),
                pct(0.99),
                pct(1.0)
            );
        }
        return;
    }
    if args.iter().any(|a| a == "--io") {
        for rom in &roms {
            let gb = boot(Path::new(rom), model);
            for row in (0xff00u16..0xff80).step_by(8) {
                let bytes: Vec<String> = (row..row + 8)
                    .map(|a| format!("{:02X}", gb.hw.peek(a)))
                    .collect();
                println!("${row:04X}: {}", bytes.join(" "));
            }
        }
        return;
    }
    if let Some(addr) = brk {
        for rom in roms {
            let mut gb = boot(Path::new(rom), model);
            let mut hits = 0;
            while gb.hw.dots < 4 << 22 && hits < 50 {
                gb.step();
                if gb.cpu.pc == addr {
                    hits += 1;
                    let c = &gb.cpu;
                    println!(
                        "{rom}: AF={:04x} BC={:04x} DE={:04x} HL={:04x} SP={:04x} LY={} dots={}",
                        c.af(),
                        c.bc(),
                        c.de(),
                        c.hl(),
                        c.sp,
                        gb.hw.peek(0xff44),
                        gb.hw.dots
                    );
                }
            }
        }
        return;
    }
    let mut failed = 0;
    for rom in roms {
        let outcome = run(Path::new(rom), model, protocol, 60);
        if outcome != Outcome::Pass {
            failed += 1;
        }
        println!("{outcome:?}\t{rom}");
    }
    std::process::exit(if failed == 0 { 0 } else { 1 });
}
