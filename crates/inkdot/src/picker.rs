//! Choosing a game, when inkdot is started without one (from the stock OS's Apps, or by hand):
//! the `.gb`, `.gbc` and `.zip` files in the `GB` and `GBC` folders of each card's `Roms`, as a
//! list on the bottom screen in DSlot's menu (nibkit's `menu`). The D-pad or a finger moves
//! through it, L and R a screenful at a time; A or a tap on the highlighted game starts it, B or
//! Back leaves.

use nibkit::menu::raster::{Canvas, Fonts};
use nibkit::menu::{colour, layout, Effect, Key, Menu, Row, Status, Value};
use std::path::{Path, PathBuf};

/// A game found on a card.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Game {
    pub path: PathBuf,
    /// The file's name without its extensions, as the list shows it.
    pub name: String,
    /// `GB` or `GBC`: the folder it was found in.
    pub console: &'static str,
}

/// Every game in `<root>/GB` and `<root>/GBC` for each root, by name.
pub fn find(roots: &[PathBuf]) -> Vec<Game> {
    let mut games = Vec::new();
    for root in roots {
        for console in ["GB", "GBC"] {
            let Ok(dir) = std::fs::read_dir(root.join(console)) else {
                continue;
            };
            for entry in dir.flatten() {
                let path = entry.path();
                if is_game(&path) && path.is_file() && !nibkit::paths::slot_copy(&path, roots) {
                    games.push(Game {
                        name: crate::rom::stem(&path),
                        path,
                        console,
                    });
                }
            }
        }
    }
    games.sort_by_key(|g| g.name.to_lowercase());
    games
}

/// The list's state.
pub struct Picker {
    pub games: Vec<Game>,
    menu: Menu,
    fonts: Fonts,
    canvas: Canvas,
    dirty: bool,
}

impl Picker {
    pub fn new(games: Vec<Game>) -> Picker {
        let sep = "   \u{b7}   ";
        let menu = Menu {
            tabs: Vec::new(),
            title: if games.is_empty() {
                "NO GAMES IN ROMS/GB, ROMS/GBC, GAMES/GB OR GAMES/GBC".into()
            } else {
                "CHOOSE A GAME".into()
            },
            rows: games
                .iter()
                .map(|g| Row::new(format!("{}  ({})", g.name, g.console), Value::None))
                .collect(),
            row: 0,
            scroll: 0,
            hint: format!("A  play{sep}D-pad  move{sep}L / R  page{sep}B  leave"),
        };
        Picker {
            games,
            menu,
            fonts: Fonts::new(),
            canvas: Canvas::new(1.0),
            dirty: true,
        }
    }

    /// The highlighted game.
    #[cfg(test)]
    pub fn chosen(&self) -> usize {
        self.menu.row
    }

    /// A button: the game to start, if one was chosen; `Err(())` to leave.
    pub fn key(&mut self, key: Key) -> Result<Option<usize>, ()> {
        self.dirty = true;
        let screenful = |k| std::iter::repeat_n(k, layout::VISIBLE);
        match key {
            Key::L => screenful(Key::Up).for_each(|k| {
                let _ = self.menu.key(k);
            }),
            Key::R => screenful(Key::Down).for_each(|k| {
                let _ = self.menu.key(k);
            }),
            k => match self.menu.key(k) {
                Some(Effect::Choose(i)) => return Ok(Some(i)),
                Some(Effect::Back) => return Err(()),
                _ => {}
            },
        }
        Ok(None)
    }

    /// A finger down at a point on the bottom screen (fractions of its size): the game to
    /// start, when it was the highlighted one.
    pub fn touch(&mut self, fx: f32, fy: f32) -> Option<usize> {
        self.dirty = true;
        match self.menu.touch(fx * nibkit::menu::W, fy * nibkit::menu::H) {
            Some(Effect::Choose(i)) => Some(i),
            _ => None,
        }
    }

    /// The picture, when it has changed.
    pub fn render(&mut self, clock: &str) -> Option<&[u8]> {
        if !std::mem::take(&mut self.dirty) {
            return None;
        }
        self.canvas.clear(colour::BLACK);
        let status = Status {
            clock: clock.to_owned(),
            ..Status::default()
        };
        self.canvas
            .draw(&self.fonts, &layout::menu(&self.menu, &status), |_| None);
        Some(&self.canvas.px)
    }
}

/// Whether a file name is one the picker would list.
pub fn is_game(path: &Path) -> bool {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);
    matches!(ext.as_deref(), Some("gb" | "gbc" | "zip"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_games_in_both_folders_by_name() {
        let root = std::env::temp_dir().join(format!("inkdot-picker-{}", std::process::id()));
        for (dir, file) in [
            ("GB", "Tetris.gb"),
            ("GBC", "alpha.gbc"),
            ("GBC", "Beta.zip"),
            ("GB", "notes.txt"),
        ] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
            std::fs::write(root.join(dir).join(file), b"x").unwrap();
        }
        let games = find(&[root.clone(), root.join("missing")]);
        let names: Vec<_> = games.iter().map(|g| (g.name.as_str(), g.console)).collect();
        assert_eq!(names, [("alpha", "GBC"), ("Beta", "GBC"), ("Tetris", "GB")]);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn the_list_moves_and_chooses() {
        let games = (0..30)
            .map(|i| Game {
                path: PathBuf::from(format!("{i}.gb")),
                name: format!("{i:02}"),
                console: "GB",
            })
            .collect();
        let mut p = Picker::new(games);
        assert_eq!(p.key(Key::Up), Ok(None));
        assert_eq!(p.chosen(), 0);
        p.key(Key::R).unwrap();
        assert_eq!(p.chosen(), layout::VISIBLE);
        assert_eq!(p.key(Key::A), Ok(Some(layout::VISIBLE)));
        assert_eq!(p.key(Key::B), Err(()));
        assert!(p.render("5:05 PM").is_some() && p.render("5:05 PM").is_none());
    }
}
