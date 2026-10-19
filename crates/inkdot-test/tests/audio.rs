//! The sound output as a signal: a tone comes out at its frequency and the level is right.

use inkdot_core::cpu::Bus;
use inkdot_core::{GameBoy, Model};

fn machine() -> GameBoy {
    GameBoy::new(vec![0; 0x8000], Model::Dmg).unwrap()
}

/// Plays channel 2 at `period` with a 50% duty for a second and returns the left samples.
fn tone(period: u16) -> Vec<i16> {
    let mut gb = machine();
    let hw = &mut gb.hw;
    hw.write(0xff26, 0x80);
    hw.write(0xff24, 0x77);
    hw.write(0xff25, 0x22);
    hw.write(0xff16, 0x80);
    hw.write(0xff17, 0xf0);
    hw.write(0xff18, period as u8);
    hw.write(0xff19, 0x80 | (period >> 8) as u8);
    let mut out = Vec::new();
    gb.take_audio(&mut out);
    out.clear();
    gb.run_dots(1 << 22);
    gb.take_audio(&mut out);
    out.chunks(2).map(|s| s[0]).collect()
}

#[test]
fn a_pulse_tone_has_its_frequency() {
    // 131072 / (2048 - period) Hz: period 1917 gives 1000.5 Hz.
    let left = tone(1917);
    assert!(
        (47_900..48_100).contains(&left.len()),
        "{} samples",
        left.len()
    );
    let rising = left.windows(2).filter(|w| w[0] < 0 && w[1] >= 0).count();
    assert!(
        (995..=1006).contains(&rising),
        "{rising} cycles in a second"
    );
}

#[test]
fn a_full_volume_square_wave_is_loud_but_does_not_clip() {
    let left = tone(1917);
    let peak = left.iter().map(|v| v.unsigned_abs()).max().unwrap();
    assert!((5_000..32_767).contains(&peak), "peak {peak}");
}
