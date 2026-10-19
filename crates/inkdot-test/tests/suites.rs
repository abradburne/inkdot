//! The test ROM suites, as `cargo test` runs them once `tools/fetch-test-roms.sh` has fetched
//! them. Each suite is one test that runs all its ROMs (in parallel) and lists every failure.
//! docs/accuracy.md says which ROMs are expected to pass and which are still open.

use inkdot_core::Model;
use inkdot_test::{fetched, run, Outcome, Protocol};
use std::path::PathBuf;

/// Runs each ROM and returns the ones that didn't pass, with their outcome.
fn failures(roms: &[PathBuf], model: Model, protocol: Protocol, seconds: u32) -> Vec<String> {
    let mut out: Vec<String> = std::thread::scope(|s| {
        let handles: Vec<_> = roms
            .iter()
            .map(|rom| s.spawn(move || (rom, run(rom, model, protocol, seconds))))
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().unwrap())
            .filter(|(_, o)| *o != Outcome::Pass)
            .map(|(rom, o)| format!("{}: {o:?}", rom.display()))
            .collect()
    });
    out.sort();
    out
}

fn roms(dir: &str, filter: impl Fn(&str) -> bool) -> Vec<PathBuf> {
    let Some(root) = fetched(dir) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    let mut stack = vec![root];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(d).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "gb" || x == "gbc") {
                let name = p.to_string_lossy().into_owned();
                if filter(&name) {
                    found.push(p);
                }
            }
        }
    }
    found.sort();
    found
}

fn assert_all_pass(roms: &[PathBuf], model: Model, protocol: Protocol, seconds: u32) {
    let failed = failures(roms, model, protocol, seconds);
    assert!(
        failed.is_empty(),
        "{} of {} failed:\n{}",
        failed.len(),
        roms.len(),
        failed.join("\n")
    );
}

/// A Mooneye test runs on the DMG unless its name ends in a model list that leaves the DMG
/// out (-dmg0, -mgb, -S, -sgb, -sgb2 for the others; G in -GS is the DMG).
fn mooneye_on_dmg(name: &str) -> bool {
    let stem = name.trim_end_matches(".gb");
    !["-dmg0", "-mgb", "-S", "-sgb", "-sgb2"]
        .iter()
        .any(|s| stem.ends_with(s))
}

#[test]
fn mooneye_acceptance_dmg() {
    let roms = roms("mooneye/acceptance", mooneye_on_dmg);
    assert_all_pass(&roms, Model::Dmg, Protocol::Mooneye, 20);
}

#[test]
fn mooneye_emulator_only() {
    let roms = roms("mooneye/emulator-only", |_| true);
    assert_all_pass(&roms, Model::Dmg, Protocol::Mooneye, 20);
}

#[test]
fn blargg_cpu_and_timing() {
    let picked = |n: &str| {
        n.ends_with("cpu_instrs/cpu_instrs.gb")
            || n.ends_with("instr_timing.gb")
            || n.ends_with("mem_timing/mem_timing.gb")
            || n.ends_with("mem_timing-2/mem_timing.gb")
            || n.ends_with("halt_bug.gb")
    };
    let roms = roms("blargg", picked);
    assert_all_pass(&roms, Model::Dmg, Protocol::Blargg, 60);
    assert_all_pass(&roms, Model::Cgb, Protocol::Blargg, 60);
}
