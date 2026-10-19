//! Real games, played headless with a button script past their intros: a game that hangs or
//! draws something else fails. The games aren't in the repository (they are the player's own);
//! set `INKDOT_GAMES` to the folders they are in (`:`-separated; `GB/` and `GBC/` under each are
//! searched too) and the test plays every one it finds. Without it, or without a game, it checks
//! only that the scripts read. The games stay on the handheld: build the test there (or copy its
//! binary over) and run it with `INKDOT_GAMES` and `INKDOT_GAME_SCRIPTS` (a copy of
//! `tests/games`). A zipped game must be unzipped first.
//!
//! The hashes pin the exact picture, so a change to the core that alters what a game draws
//! changes them: look at the frame (`inkdot-test --play ... --shots`) and record the new one.
//!
//! Each `tests/games/*.play` is a [`Script`] whose comments carry the rest:
//!
//! ```text
//! # rom: Prehistorik Man (USA, Europe).gb
//! # model: dmg
//! # check: 1800 0123456789abcdef     (the frame hash after that many frames; several allowed)
//! ```
//!
//! `inkdot-test --play FILE --frames N --every K --shots DIR ROM` shows what a script does, to
//! write one or to see where a game stops.

use inkdot_core::Model;
use inkdot_test::{boot, frame_hash, play, Script};
use std::path::{Path, PathBuf};

struct Game {
    file: PathBuf,
    rom: String,
    model: Model,
    checks: Vec<(u64, u64)>,
    script: Script,
}

fn games() -> Vec<Game> {
    // INKDOT_GAME_SCRIPTS: where the scripts are, when the test runs on the handheld.
    let dir = std::env::var_os("INKDOT_GAME_SCRIPTS")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/games"));
    let mut out = Vec::new();
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "play"))
        .collect();
    files.sort();
    for file in files {
        let text = std::fs::read_to_string(&file).unwrap();
        let field = |key: &str| {
            text.lines()
                .filter_map(|l| l.trim().strip_prefix('#'))
                .filter_map(|l| l.trim().strip_prefix(key))
                .map(|v| v.trim().to_owned())
                .collect::<Vec<_>>()
        };
        let model = match field("model:").first().map(String::as_str) {
            Some("cgb") => Model::Cgb,
            _ => Model::Dmg,
        };
        let checks = field("check:")
            .iter()
            .map(|c| {
                let mut w = c.split_whitespace();
                let frame = w.next().and_then(|f| f.parse().ok());
                let hash = w.next().and_then(|h| u64::from_str_radix(h, 16).ok());
                frame
                    .zip(hash)
                    .unwrap_or_else(|| panic!("{}: check: FRAME HASH", file.display()))
            })
            .collect::<Vec<(u64, u64)>>();
        let script = Script::parse(&text).unwrap_or_else(|e| panic!("{}: {e}", file.display()));
        let rom = field("rom:").first().cloned().unwrap_or_default();
        assert!(!rom.is_empty(), "{}: no rom:", file.display());
        assert!(!checks.is_empty(), "{}: no check:", file.display());
        out.push(Game {
            file,
            rom,
            model,
            checks,
            script,
        });
    }
    out
}

fn find(rom: &str) -> Option<PathBuf> {
    let dirs = std::env::var("INKDOT_GAMES").ok()?;
    dirs.split(':').find_map(|d| {
        ["", "GB", "GBC"]
            .iter()
            .map(|sub| Path::new(d).join(sub).join(rom))
            .find(|p| p.is_file())
    })
}

#[test]
fn games_get_past_their_intros() {
    let mut failed = Vec::new();
    for game in games() {
        let Some(rom) = find(&game.rom) else { continue };
        let mut gb = boot(&rom, game.model);
        let last = game.checks.iter().map(|c| c.0).max().unwrap_or(0);
        let mut seen = Vec::new();
        play(&mut gb, &game.script, last, |f, gb| {
            if let Some(&(frame, want)) = game.checks.iter().find(|c| c.0 == f + 1) {
                seen.push((frame, want, frame_hash(gb), gb.cpu.pc));
            }
        });
        for (frame, want, got, pc) in seen {
            if got != want {
                failed.push(format!(
                    "{}: frame {frame}: hash {got:016x}, not {want:016x} (pc {pc:04x})",
                    game.file.display()
                ));
            }
        }
        eprintln!("played {}", game.rom);
    }
    assert!(failed.is_empty(), "\n{}", failed.join("\n"));
}
