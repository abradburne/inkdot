//! `inkdot-test [--cgb] [--mooneye] [--break ADDR] ROM...`: runs test ROMs and prints each
//! one's outcome. `--break` prints the CPU's registers each time PC reaches ADDR (hex) instead,
//! for the first few seconds, to see what a test measured.

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
        .filter(|(i, a)| !a.starts_with("--") && (*i == 0 || args[i - 1] != "--break"))
        .map(|(_, a)| a)
        .collect();
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
