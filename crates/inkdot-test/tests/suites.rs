//! The test ROM suites, as `cargo test` runs them once `tools/fetch-test-roms.sh` has fetched
//! them: every suite that is a target must pass in full. The same lists make docs/accuracy.md
//! (`inkdot-test --accuracy`).

use inkdot_core::Model;
use inkdot_test::{fetched, run_suite, suites, Outcome};

#[test]
fn target_suites_pass() {
    if fetched("mooneye").is_none() {
        return;
    }
    let mut failed = Vec::new();
    for suite in suites().iter().filter(|s| s.target) {
        assert!(!suite.roms.is_empty(), "{}: no ROMs found", suite.name);
        for (rom, outcome) in run_suite(suite) {
            if outcome != Outcome::Pass {
                failed.push(format!("{}: {}: {outcome:?}", suite.name, rom.display()));
            }
        }
    }
    assert!(
        failed.is_empty(),
        "{} failed:\n{}",
        failed.len(),
        failed.join("\n")
    );
}

fn acid2(rom: &str, reference: &str, model: Model) {
    let (Some(rom), Some(reference)) = (fetched(rom), fetched(reference)) else {
        return;
    };
    let wrong = inkdot_test::compare_with_reference(&rom, model, &reference);
    assert!(
        wrong.is_empty(),
        "{} pixels differ from {}, first at {:?}",
        wrong.len(),
        reference.display(),
        &wrong[..wrong.len().min(8)]
    );
}

#[test]
fn dmg_acid2_on_dmg() {
    acid2(
        "dmg-acid2/dmg-acid2.gb",
        "dmg-acid2-ref/reference-dmg.png",
        Model::Dmg,
    );
}

#[test]
fn dmg_acid2_on_cgb() {
    acid2(
        "dmg-acid2/dmg-acid2.gb",
        "dmg-acid2-cgb-ref/reference-cgb.png",
        Model::Cgb,
    );
}

#[test]
fn cgb_acid2() {
    acid2(
        "cgb-acid2/cgb-acid2.gbc",
        "cgb-acid2-ref/reference.png",
        Model::Cgb,
    );
}
