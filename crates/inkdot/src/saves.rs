//! A game's files in its saves folder: the one DSlot gives (`--saves`), else the system's
//! (`<saves root>/GB/<ROM name>/`, as DSlot and the stock OS lay them out), else, where there is
//! no saves root (a computer), inkdot's own data folder.
//!
//! | File | |
//! |---|---|
//! | `<ROM name>.sav` | Battery RAM, raw: the format other tools read. |
//! | `<ROM name>.rtc` | The MBC3 clock (`Cartridge::rtc_save`). |
//! | `<ROM name>.state1` .. `.state6`, `.auto.state` | Save states, each with its picture. |
//! | `<ROM name>.auto-dmg.state`, `.auto-cgb.state` | The resume state of the other console, kept aside. |
//!
//! A DMG game runs on a DMG or a CGB by its look, and a state made on one won't load on the other.
//! When the look changes the console, the resume state of the old one is kept aside, so the game
//! starts again on the new one without losing the old; changing back resumes it.
//!
//! Files are written by a thread of their own, so a slow card never holds up a frame, and
//! atomically (a temporary file, synced, then renamed over the old one), so a crash or a flat
//! battery leaves the previous save rather than half of a new one.

use inkdot_core::{GameBoy, Model};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Sender};
use std::thread::JoinHandle;

/// The number of save state slots.
pub const SLOTS: usize = 6;

/// Where a game's files are.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SaveFiles {
    pub dir: PathBuf,
    pub stem: String,
}

impl SaveFiles {
    pub fn new(dir: PathBuf, stem: &str) -> SaveFiles {
        SaveFiles {
            dir,
            stem: stem.to_owned(),
        }
    }

    fn file(&self, ext: &str) -> PathBuf {
        self.dir.join(format!("{}.{ext}", self.stem))
    }

    pub fn battery(&self) -> PathBuf {
        self.file("sav")
    }

    /// The battery RAM: `<ROM name>.sav`, or the `<ROM name>.gbc.sav` / `.gb.sav` that some
    /// tools write, so a save brought from elsewhere is found.
    pub fn read_battery(&self) -> Option<Vec<u8>> {
        [self.battery(), self.file("gbc.sav"), self.file("gb.sav")]
            .iter()
            .find_map(|p| std::fs::read(p).ok())
    }

    /// A game played from a slot CFW `Games/<system>/` folder, with no battery save here yet:
    /// the `.sav` slot left beside the ROM (`<ROM name>.sav`, or `<ROM file name>.sav`) is
    /// copied in, so the game goes on from where it was in slot. The original is left as it is.
    /// The file copied, if one was.
    pub fn adopt_slot_save(&self, rom: &Path) -> Option<PathBuf> {
        let dir = rom.parent()?;
        if dir.parent()?.file_name()? != "Games" || self.read_battery().is_some() {
            return None;
        }
        let file = rom.file_name()?.to_string_lossy();
        let from = [format!("{}.sav", self.stem), format!("{file}.sav")]
            .into_iter()
            .map(|name| dir.join(name))
            .find(|p| p.is_file())?;
        let data = std::fs::read(&from).ok()?;
        std::fs::create_dir_all(&self.dir).ok()?;
        write_atomically(&self.battery(), &data).ok()?;
        Some(from)
    }

    pub fn clock(&self) -> PathBuf {
        self.file("rtc")
    }

    /// The resume state made on `model` kept aside, when the other model's is the latest.
    pub fn aside(&self, model: Model) -> PathBuf {
        self.file(match model {
            Model::Dmg => "auto-dmg.state",
            Model::Cgb => "auto-cgb.state",
        })
    }

    /// Where to resume on `model`: the latest resume state if made on it, else the one kept
    /// aside for it.
    pub fn resume(&self, model: Model) -> Option<Slot> {
        [self.state(0), self.aside(model)]
            .iter()
            .filter_map(|p| Slot::read(p))
            .find(|s| GameBoy::state_model(&s.machine) == Some(model))
    }

    /// Before a new resume state made on `model` is written: the latest, if made on the other
    /// model, is kept aside for it; the one aside for `model` is replaced by the new one.
    pub fn make_way(&self, model: Model) {
        let latest = self.state(0);
        if let Some(other) = Slot::read(&latest)
            .and_then(|s| GameBoy::state_model(&s.machine))
            .filter(|&m| m != model)
        {
            let _ = std::fs::rename(&latest, self.aside(other));
        }
        let _ = std::fs::remove_file(self.aside(model));
    }

    /// Slot 0 is the automatic one, written on leaving the game.
    pub fn state(&self, slot: usize) -> PathBuf {
        if slot == 0 {
            self.file("auto.state")
        } else {
            self.file(&format!("state{slot}"))
        }
    }
}

/// The saves folder for a game, when none is given.
pub fn default_dir(console: &str, stem: &str) -> PathBuf {
    let root = nibkit::paths::saves_root();
    let root = if root.is_dir() {
        root
    } else {
        nibkit::paths::AppDirs::new("inkdot").data.join("saves")
    };
    nibkit::paths::saves_dir(&root, console, stem)
}

/// A save state as kept in a slot: the machine, its picture and when it was made.
///
/// ```text
/// "IKSL"  u8 version (1)  u64 made (Unix seconds)  u64 played (seconds)
/// u32 picture length, PNG (160x144)   the rest: the machine (inkdot_core::state)
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Slot {
    pub made: u64,
    pub played: u64,
    pub png: Vec<u8>,
    pub machine: Vec<u8>,
}

impl Slot {
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(25 + self.png.len() + self.machine.len());
        out.extend_from_slice(b"IKSL\x01");
        out.extend_from_slice(&self.made.to_le_bytes());
        out.extend_from_slice(&self.played.to_le_bytes());
        out.extend_from_slice(&(self.png.len() as u32).to_le_bytes());
        out.extend_from_slice(&self.png);
        out.extend_from_slice(&self.machine);
        out
    }

    pub fn decode(data: &[u8]) -> Option<Slot> {
        if data.len() < 25 || &data[..5] != b"IKSL\x01" {
            return None;
        }
        let u64_at = |i: usize| u64::from_le_bytes(data[i..i + 8].try_into().unwrap_or([0; 8]));
        let png_len = u32::from_le_bytes(data[21..25].try_into().ok()?) as usize;
        let png = data.get(25..25 + png_len)?.to_vec();
        let machine = data.get(25 + png_len..)?.to_vec();
        Some(Slot {
            made: u64_at(5),
            played: u64_at(13),
            png,
            machine,
        })
    }

    /// Reads a slot's file; `None` when there is none or it isn't one.
    pub fn read(path: &Path) -> Option<Slot> {
        Slot::decode(&std::fs::read(path).ok()?)
    }
}

/// Writes `data` to `path` so that the file is always either the old one or all of the new.
pub fn write_atomically(path: &Path, data: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp");
    let mut f = std::fs::File::create(&tmp)?;
    f.write_all(data)?;
    f.sync_all()?;
    drop(f);
    std::fs::rename(&tmp, path)
}

/// The thread that writes files.
pub struct Writer {
    jobs: Option<Sender<(PathBuf, Vec<u8>)>>,
    thread: Option<JoinHandle<()>>,
}

impl Writer {
    pub fn start() -> Writer {
        let (tx, rx) = channel::<(PathBuf, Vec<u8>)>();
        let thread = std::thread::Builder::new()
            .name("inkdot-saves".into())
            .spawn(move || {
                for (path, data) in rx {
                    if let Err(e) = write_atomically(&path, &data) {
                        eprintln!("inkdot: cannot save {}: {e}", path.display());
                    }
                }
            })
            .expect("start the saves thread");
        Writer {
            jobs: Some(tx),
            thread: Some(thread),
        }
    }

    /// Queues a file to be written.
    pub fn write(&self, path: PathBuf, data: Vec<u8>) {
        if let Some(jobs) = &self.jobs {
            let _ = jobs.send((path, data));
        }
    }

    /// Waits for every queued file to be written.
    pub fn finish(&mut self) {
        self.jobs = None;
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for Writer {
    fn drop(&mut self) {
        self.finish();
    }
}

/// The time now, in Unix seconds.
pub fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("inkdot-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    /// The look changed a DMG game from a CGB to a DMG and back (Prehistorik Man, from DSlot):
    /// neither console's resume state is lost, and each resumes on its own console.
    #[test]
    fn a_resume_state_per_console() {
        let dir = temp("per-model");
        std::fs::create_dir_all(&dir).unwrap();
        let f = SaveFiles::new(dir.clone(), "Prehistorik Man");
        let slot = |cgb: u8, played: u64| Slot {
            made: 0,
            played,
            png: Vec::new(),
            machine: [b"INKDOT\0\0\x01\x00".as_slice(), &[cgb, 0]].concat(),
        };
        let write = |model, s: Slot| {
            f.make_way(model);
            std::fs::write(f.state(0), s.encode()).unwrap();
        };
        write(Model::Cgb, slot(1, 10));
        assert_eq!(f.resume(Model::Cgb).map(|s| s.played), Some(10));
        assert!(f.resume(Model::Dmg).is_none(), "a fresh start on the DMG");
        write(Model::Dmg, slot(0, 20));
        assert_eq!(f.resume(Model::Dmg).map(|s| s.played), Some(20));
        assert_eq!(
            f.resume(Model::Cgb).map(|s| s.played),
            Some(10),
            "kept aside"
        );
        write(Model::Cgb, slot(1, 30));
        assert_eq!(f.resume(Model::Cgb).map(|s| s.played), Some(30));
        assert_eq!(f.resume(Model::Dmg).map(|s| s.played), Some(20));
        assert!(
            !f.aside(Model::Cgb).exists(),
            "the old CGB state is replaced"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_battery_save_from_elsewhere_is_found() {
        let dir = temp("other-sav");
        std::fs::create_dir_all(&dir).unwrap();
        let f = SaveFiles::new(dir.clone(), "Zelda");
        assert_eq!(f.read_battery(), None);
        std::fs::write(dir.join("Zelda.gbc.sav"), [1, 2]).unwrap();
        assert_eq!(f.read_battery(), Some(vec![1, 2]));
        std::fs::write(dir.join("Zelda.sav"), [3]).unwrap();
        assert_eq!(f.read_battery(), Some(vec![3]));
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// A Games/ game slot CFW played: its `.sav` beside the ROM is copied in on the first start,
    /// left where it was, and never copied over a save of ours.
    #[test]
    fn a_slot_save_is_adopted_once() {
        let card = temp("slot-sav");
        let games = card.join("Games/GBC");
        std::fs::create_dir_all(&games).unwrap();
        let rom = games.join("Zelda.gbc");
        std::fs::write(&rom, [0]).unwrap();
        std::fs::write(games.join("Zelda.sav"), [7, 7]).unwrap();
        let f = SaveFiles::new(card.join("Saves/GBC/Zelda"), "Zelda");
        assert_eq!(f.adopt_slot_save(&rom), Some(games.join("Zelda.sav")));
        assert_eq!(f.read_battery(), Some(vec![7, 7]));
        assert_eq!(std::fs::read(games.join("Zelda.sav")).unwrap(), [7, 7]);
        std::fs::write(f.battery(), [1]).unwrap();
        assert_eq!(f.adopt_slot_save(&rom), None, "ours is kept");
        assert_eq!(f.read_battery(), Some(vec![1]));

        // `<ROM file name>.sav` too; a ROM outside Games/ adopts nothing.
        let other = SaveFiles::new(card.join("Saves/GBC/Mole"), "Mole");
        let mole = games.join("Mole.gbc");
        std::fs::write(games.join("Mole.gbc.sav"), [2]).unwrap();
        assert_eq!(other.adopt_slot_save(&mole), Some(games.join("Mole.gbc.sav")));
        let roms = card.join("Roms/GBC");
        std::fs::create_dir_all(&roms).unwrap();
        std::fs::write(roms.join("Kirby.sav"), [3]).unwrap();
        let kirby = SaveFiles::new(card.join("Saves/GBC/Kirby"), "Kirby");
        assert_eq!(kirby.adopt_slot_save(&roms.join("Kirby.gbc")), None);
        std::fs::remove_dir_all(card).unwrap();
    }

    #[test]
    fn names() {
        let f = SaveFiles::new(PathBuf::from("/s/GB/Tetris"), "Tetris (World)");
        assert_eq!(
            f.battery(),
            PathBuf::from("/s/GB/Tetris/Tetris (World).sav")
        );
        assert_eq!(
            f.state(0),
            PathBuf::from("/s/GB/Tetris/Tetris (World).auto.state")
        );
        assert_eq!(
            f.state(3),
            PathBuf::from("/s/GB/Tetris/Tetris (World).state3")
        );
    }

    #[test]
    fn slots_round_trip() {
        let s = Slot {
            made: 1_760_000_000,
            played: 3600,
            png: vec![1, 2, 3],
            machine: vec![9; 50],
        };
        assert_eq!(Slot::decode(&s.encode()), Some(s));
        assert_eq!(Slot::decode(b"IKSL\x01short"), None);
    }

    #[test]
    fn the_writer_writes_everything_before_finishing() {
        let dir = temp("writer");
        let mut w = Writer::start();
        for i in 0..20u8 {
            w.write(dir.join("a.sav"), vec![i; 1000]);
        }
        w.finish();
        assert_eq!(std::fs::read(dir.join("a.sav")).unwrap(), vec![19; 1000]);
        assert!(!dir.join("a.tmp").exists());
        let _ = std::fs::remove_dir_all(dir);
    }
}
