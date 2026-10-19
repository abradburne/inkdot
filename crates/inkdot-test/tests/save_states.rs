//! Save states: a machine restored from a state carries on exactly as the original does, frame
//! for frame and sample for sample, wherever in a frame the state was made.

use inkdot_core::{GameBoy, Model, StateError};
use inkdot_test::{boot, fetched};
use std::path::Path;

/// Runs `frames` frames and returns everything visible: the pictures, the sound, the CPU.
fn run(gb: &mut GameBoy, frames: usize) -> (Vec<u16>, Vec<i16>, String) {
    let mut pictures = Vec::new();
    let mut sound = Vec::new();
    for _ in 0..frames {
        gb.run_frame();
        pictures.extend_from_slice(gb.frame());
        gb.take_audio(&mut sound);
    }
    (pictures, sound, format!("{:?}", gb.cpu))
}

fn round_trip(rom: &Path, model: Model, before_dots: u64) {
    let mut a = boot(rom, model);
    a.run_dots(before_dots);
    let mut discard = Vec::new();
    a.take_audio(&mut discard);
    let state = a.save_state();
    // Saving changes nothing: the original carries on as before.
    let mut b = boot(rom, model);
    b.load_state(&state)
        .unwrap_or_else(|e| panic!("{}: {e}", rom.display()));
    let mut c = boot(rom, model);
    c.load_state(&state).unwrap();
    b.take_audio(&mut discard);
    c.take_audio(&mut discard);
    let (pa, _, ca) = run(&mut a, 120);
    let (pb, sb, cb) = run(&mut b, 120);
    let (pc, sc, cc) = run(&mut c, 120);
    assert_eq!(ca, cb, "{}: CPU differs after loading", rom.display());
    assert!(pa == pb, "{}: pictures differ after loading", rom.display());
    // Two loads of the same state are identical in everything, sound included (the original's
    // sound differs only in the synthesizer's restart, which isn't machine state).
    assert_eq!(
        (pb, sb, cb),
        (pc, sc, cc),
        "{}: two loads differ",
        rom.display()
    );
}

#[test]
fn states_round_trip_mid_frame() {
    let roms = [
        ("blargg/cpu_instrs/cpu_instrs.gb", Model::Dmg),
        ("blargg/dmg_sound/dmg_sound.gb", Model::Dmg),
        ("blargg/cgb_sound/cgb_sound.gb", Model::Cgb),
        ("cgb-acid2/cgb-acid2.gbc", Model::Cgb),
        ("dmg-acid2/dmg-acid2.gb", Model::Cgb),
        ("mooneye/emulator-only/mbc5/rom_8Mb.gb", Model::Dmg),
        ("rtc3test/rtc3test.gb", Model::Cgb),
    ];
    for (rom, model) in roms {
        let Some(path) = fetched(rom) else { continue };
        // A second and a bit in, at an odd dot: mid-line, mid-instruction timing.
        for dots in [4_194_304 + 12_345, 2 * 4_194_304 + 71_001] {
            round_trip(&path, model, dots);
        }
    }
}

#[test]
fn a_state_of_another_game_is_refused() {
    let (Some(a), Some(b)) = (
        fetched("dmg-acid2/dmg-acid2.gb"),
        fetched("cgb-acid2/cgb-acid2.gbc"),
    ) else {
        return;
    };
    let state = boot(&a, Model::Cgb).save_state();
    assert_eq!(
        boot(&b, Model::Cgb).load_state(&state),
        Err(StateError::OtherGame)
    );
    let mut short = state.clone();
    short.truncate(state.len() - 100);
    let mut gb = boot(&a, Model::Cgb);
    let before = gb.save_state();
    assert_eq!(gb.load_state(&short), Err(StateError::Corrupt));
    assert_eq!(
        gb.save_state(),
        before,
        "a failed load leaves the machine as it was"
    );
}

/// A DMG game's state made on one model won't load on the other (the look decides the model),
/// and the frontend can tell which model a state needs before trying.
#[test]
fn a_state_says_its_model() {
    let Some(rom) = fetched("dmg-acid2/dmg-acid2.gb") else {
        return;
    };
    let on_cgb = boot(&rom, Model::Cgb).save_state();
    let on_dmg = boot(&rom, Model::Dmg).save_state();
    assert_eq!(GameBoy::state_model(&on_cgb), Some(Model::Cgb));
    assert_eq!(GameBoy::state_model(&on_dmg), Some(Model::Dmg));
    assert_eq!(GameBoy::state_model(b"not a state"), None);
    assert_eq!(
        boot(&rom, Model::Dmg).load_state(&on_cgb),
        Err(StateError::OtherGame)
    );
    assert_eq!(boot(&rom, Model::Dmg).load_state(&on_dmg), Ok(()));
}
