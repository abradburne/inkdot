//! What the CPU sees in the first lines after it turns the LCD on: LY, STAT (with LYC 0 and 1),
//! and whether OAM and VRAM can be read and written, at each M-cycle offset that Mooneye's
//! `lcdon_timing-GS` and `lcdon_write_timing-GS` check. The offsets and expected values are those
//! tests' tables; driving the bus directly says which entry is wrong when one is.

use inkdot_core::cpu::Bus;
use inkdot_core::{GameBoy, Model};

/// A blank ROM-only cartridge.
fn machine() -> GameBoy {
    GameBoy::new(vec![0; 0x8000], Model::Dmg).unwrap()
}

/// Turns the LCD off and on, waits `nops` M-cycles as the tests do (`ldh (LCDC),a`, then `nops`
/// NOPs, then the access's opcode fetch), and makes `access` in the next cycle.
fn after_lcd_on<T>(lyc: u8, nops: u32, access: impl FnOnce(&mut GameBoy) -> T) -> T {
    let mut gb = machine();
    gb.hw.write(0xff40, 0x00);
    gb.hw.write(0xff45, lyc);
    gb.hw.write(0xff40, 0x91);
    for _ in 0..nops + 1 {
        gb.hw.idle(0);
    }
    access(&mut gb)
}

const READ_AT: [u32; 24] = [
    0, 17, 60, 110, 130, 174, 224, 244, 1, 18, 61, 111, 131, 175, 225, 245, 2, 19, 62, 112, 132,
    176, 226, 246,
];

fn check_reads(addr: u16, lyc: u8, want: [u8; 24]) {
    let got: Vec<u8> = READ_AT
        .iter()
        .map(|&n| after_lcd_on(lyc, n, |gb| gb.hw.read(addr)))
        .collect();
    let wrong: Vec<String> = (0..24)
        .filter(|&i| got[i] != want[i])
        .map(|i| {
            format!(
                "after {} nops: {:02x}, want {:02x}",
                READ_AT[i], got[i], want[i]
            )
        })
        .collect();
    assert!(
        wrong.is_empty(),
        "${addr:04x} with LYC {lyc}: {}",
        wrong.join("; ")
    );
}

#[test]
fn ly() {
    #[rustfmt::skip]
    check_reads(0xff44, 0, [0, 0, 0, 0, 1, 1, 1, 2, 0, 0, 0, 1, 1, 1, 2, 2, 0, 0, 0, 1, 1, 1, 2, 2]);
}

#[test]
fn stat() {
    #[rustfmt::skip]
    check_reads(0xff41, 0, [
        0x84, 0x84, 0x87, 0x84, 0x82, 0x83, 0x80, 0x82, 0x84, 0x87, 0x84, 0x80, 0x82, 0x80, 0x80,
        0x82, 0x84, 0x87, 0x84, 0x82, 0x83, 0x80, 0x82, 0x83,
    ]);
    #[rustfmt::skip]
    check_reads(0xff41, 1, [
        0x80, 0x80, 0x83, 0x80, 0x86, 0x87, 0x84, 0x82, 0x80, 0x83, 0x80, 0x80, 0x86, 0x84, 0x80,
        0x82, 0x80, 0x83, 0x80, 0x86, 0x87, 0x84, 0x82, 0x83,
    ]);
}

#[test]
fn oam_and_vram_reads() {
    #[rustfmt::skip]
    check_reads(0xfe00, 0, [
        0, 0, 0xff, 0, 0xff, 0xff, 0, 0xff, 0, 0xff, 0, 0xff, 0xff, 0, 0xff, 0xff, 0, 0xff, 0, 0xff,
        0xff, 0, 0xff, 0xff,
    ]);
    #[rustfmt::skip]
    check_reads(0x8000, 0, [
        0, 0, 0xff, 0, 0, 0xff, 0, 0, 0, 0xff, 0, 0, 0xff, 0, 0, 0xff, 0, 0xff, 0, 0, 0xff, 0, 0,
        0xff,
    ]);
}

#[test]
fn oam_and_vram_writes() {
    const WRITE_AT: [u32; 19] = [
        0, 17, 18, 60, 61, 110, 111, 112, 130, 131, 132, 174, 175, 224, 225, 226, 244, 245, 246,
    ];
    // Whether a write at each offset goes through.
    const OAM: [bool; 19] = [
        true, true, false, false, true, true, true, false, false, true, false, false, true, true,
        true, false, false, true, false,
    ];
    const VRAM: [bool; 19] = [
        true, true, false, false, true, true, true, true, true, true, false, false, true, true,
        true, true, true, true, false,
    ];
    for (addr, want) in [(0xfe00u16, OAM), (0x8000, VRAM)] {
        for (i, &n) in WRITE_AT.iter().enumerate() {
            let passed = after_lcd_on(0, n, |gb| {
                gb.hw.write(addr, 0x81);
                gb.hw.write(0xff40, 0);
                gb.hw.peek(addr) == 0x81
            });
            assert_eq!(passed, want[i], "write to ${addr:04x} after {n} nops");
        }
    }
}

/// LYC = 153 matches on line 153, though LY reads 153 there only for a moment before reading 0
/// (Pan Docs, "LY"): with the LY=LYC interrupt on, the STAT interrupt is requested once a frame,
/// in VBlank's last line. Prehistorik Man waits for it after its title, and hung without it.
#[test]
fn lyc_153_raises_its_interrupt() {
    for model in [Model::Dmg, Model::Cgb] {
        let mut gb = GameBoy::new(vec![0; 0x8000], model).unwrap();
        gb.hw.write(0xff45, 153);
        gb.hw.write(0xff41, 0x40);
        gb.hw.write(0xff0f, 0);
        let mut seen = Vec::new();
        // Two frames, an M-cycle at a time (peeks cost no cycles).
        for _ in 0..2 * 70224 / 4 {
            gb.hw.idle(0);
            if gb.hw.peek(0xff0f) & 0x02 != 0 {
                // Line 153, in VBlank, while LY still reads 153.
                seen.push((gb.hw.peek(0xff41) & 3, gb.hw.peek(0xff44)));
                gb.hw.if_ = 0;
            }
        }
        assert_eq!(seen, [(1, 153), (1, 153)], "{model:?}");
    }
}
