//! The CPU against the SM83 single-step test vectors: 1000 cases for each of the 500 opcodes,
//! each an initial state, a final state and the bus activity of every M-cycle in between, on a
//! flat 64 KiB memory with nothing else attached.

use inkdot_core::cpu::{Bus, Cpu};
use serde_json::Value;

/// One M-cycle as the vectors write it: an address and data for a read or a write, nothing for
/// an internal cycle (where the vectors' address and data are "don't care").
#[derive(Debug, PartialEq)]
enum Cycle {
    Read(u16, u8),
    Write(u16, u8),
    Idle,
}

struct Flat {
    mem: Vec<u8>,
    cycles: Vec<Cycle>,
}

impl Bus for Flat {
    fn read(&mut self, addr: u16) -> u8 {
        let v = self.mem[addr as usize];
        self.cycles.push(Cycle::Read(addr, v));
        v
    }
    fn write(&mut self, addr: u16, value: u8) {
        self.mem[addr as usize] = value;
        self.cycles.push(Cycle::Write(addr, value));
    }
    fn idle(&mut self, _: u16) {
        self.cycles.push(Cycle::Idle);
    }
    fn interrupts(&self) -> u8 {
        0
    }
    fn acknowledge(&mut self, _: u8) {}
}

fn byte(v: &Value, key: &str) -> u8 {
    v[key].as_u64().unwrap_or_else(|| panic!("no {key}")) as u8
}

fn word(v: &Value, key: &str) -> u16 {
    v[key].as_u64().unwrap_or_else(|| panic!("no {key}")) as u16
}

fn state(v: &Value) -> Cpu {
    Cpu {
        a: byte(v, "a"),
        f: byte(v, "f"),
        b: byte(v, "b"),
        c: byte(v, "c"),
        d: byte(v, "d"),
        e: byte(v, "e"),
        h: byte(v, "h"),
        l: byte(v, "l"),
        sp: word(v, "sp"),
        pc: word(v, "pc"),
        ime: byte(v, "ime") != 0,
        ..Cpu::default()
    }
}

/// Runs every case in one file, returning a description of the first failure.
fn run_file(path: &std::path::Path, check_cycles: bool) -> Result<usize, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let cases: Vec<Value> = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    let mut bus = Flat {
        mem: vec![0; 0x10000],
        cycles: Vec::new(),
    };
    for case in &cases {
        let name = case["name"].as_str().unwrap_or("?");
        let (init, fin) = (&case["initial"], &case["final"]);
        let mut cpu = state(init);
        for cell in init["ram"].as_array().unwrap() {
            bus.mem[cell[0].as_u64().unwrap() as usize] = cell[1].as_u64().unwrap() as u8;
        }
        bus.cycles.clear();
        cpu.step(&mut bus);
        let want = state(fin);
        // EI's effect is delayed past the end of the case, so `ei_pending` isn't compared.
        let got = Cpu {
            mode: want.mode,
            ei_pending: false,
            ..cpu.clone()
        };
        if got != want {
            return Err(format!("{name}: got {got:?}, want {want:?}"));
        }
        for cell in fin["ram"].as_array().unwrap() {
            let (addr, v) = (
                cell[0].as_u64().unwrap() as usize,
                cell[1].as_u64().unwrap() as u8,
            );
            if bus.mem[addr] != v {
                return Err(format!(
                    "{name}: [{addr:04x}] = {:02x}, want {v:02x}",
                    bus.mem[addr]
                ));
            }
        }
        if check_cycles {
            let want: Vec<Cycle> = case["cycles"]
                .as_array()
                .unwrap()
                .iter()
                .map(|c| {
                    let kind = c[2].as_str().unwrap_or("---");
                    let addr = c[0].as_u64().unwrap_or(0) as u16;
                    let data = c[1].as_u64().unwrap_or(0) as u8;
                    match kind {
                        "r-m" => Cycle::Read(addr, data),
                        "-wm" => Cycle::Write(addr, data),
                        _ => Cycle::Idle,
                    }
                })
                .collect();
            if bus.cycles != want {
                return Err(format!("{name}: cycles {:?}, want {want:?}", bus.cycles));
            }
        }
        // Leave memory as it was for the next case.
        for cell in init["ram"]
            .as_array()
            .unwrap()
            .iter()
            .chain(fin["ram"].as_array().unwrap())
        {
            bus.mem[cell[0].as_u64().unwrap() as usize] = 0;
        }
    }
    Ok(cases.len())
}

#[test]
fn single_step_vectors() {
    let Some(dir) = inkdot_test::fetched("sm83/v1") else {
        return;
    };
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .collect();
    files.sort();
    let mut failures = Vec::new();
    let mut cases = 0;
    for path in &files {
        let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
        // HALT and STOP: the vectors give them three cycles on a bus with no interrupts and no
        // buttons, which is the vectors' model and not a timing the hardware documents; their
        // state is still checked, and their timing is the Mooneye and Blargg suites' to test.
        let check_cycles = stem != "76" && stem != "10";
        match run_file(path, check_cycles) {
            Ok(n) => cases += n,
            Err(e) => failures.push(format!("{stem}: {e}")),
        }
    }
    assert!(
        failures.is_empty(),
        "{} files failed:\n{}",
        failures.len(),
        failures.join("\n")
    );
    assert_eq!(files.len(), 500);
    eprintln!("{cases} cases passed");
}
