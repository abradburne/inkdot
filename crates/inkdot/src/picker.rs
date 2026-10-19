//! Choosing a game, when inkdot is started without one (from the stock OS's Apps, or by hand):
//! the `.gb`, `.gbc` and `.zip` files in the `GB` and `GBC` folders of each card's `Roms`, as a
//! list on the bottom screen. The D-pad or a finger moves through it; A or a tap starts the game,
//! B or Back leaves.

use crate::ui::draw::{colour, Align, Canvas, Rect, BODY, SMALL, TITLE};
use crate::ui::{H, W};
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
                if is_game(&path) && path.is_file() {
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

const ROW: i32 = 72;
const TOP: i32 = 96;
const ROWS: usize = ((H - TOP - 48) / ROW) as usize;

/// The list's state: what is chosen and what is shown.
pub struct Picker {
    pub games: Vec<Game>,
    pub chosen: usize,
    first: usize,
    canvas: Canvas,
    dirty: bool,
}

impl Picker {
    pub fn new(games: Vec<Game>) -> Picker {
        Picker {
            games,
            chosen: 0,
            first: 0,
            canvas: Canvas::new(W, H),
            dirty: true,
        }
    }

    /// Moves the choice by `d` rows (a page: ±ROWS).
    pub fn step(&mut self, d: isize) {
        if self.games.is_empty() {
            return;
        }
        let last = self.games.len() - 1;
        self.chosen = (self.chosen as isize + d).clamp(0, last as isize) as usize;
        if self.chosen < self.first {
            self.first = self.chosen;
        } else if self.chosen >= self.first + ROWS {
            self.first = self.chosen + 1 - ROWS;
        }
        self.dirty = true;
    }

    pub fn page() -> isize {
        ROWS as isize
    }

    /// The game at a point on the bottom screen (fractions of its size), if any.
    pub fn at(&self, fx: f32, fy: f32) -> Option<usize> {
        let (x, y) = ((fx * W as f32) as i32, (fy * H as f32) as i32);
        if !(0..W).contains(&x) || y < TOP {
            return None;
        }
        let i = self.first + ((y - TOP) / ROW) as usize;
        (i < self.games.len() && i < self.first + ROWS).then_some(i)
    }

    /// The picture, when it has changed.
    pub fn render(&mut self) -> Option<&[u8]> {
        if !std::mem::take(&mut self.dirty) {
            return None;
        }
        let c = &mut self.canvas;
        c.clear(colour::BACKGROUND);
        c.text(
            Rect::new(32, 12, W - 64, 64),
            "Choose a game",
            TITLE,
            colour::TEXT,
            Align::Left,
        );
        c.fill(Rect::new(0, TOP - 8, W, 2), colour::LINE);
        if self.games.is_empty() {
            c.text(
                Rect::new(32, TOP + 40, W - 64, 48),
                "No games in Roms/GB or Roms/GBC on either card.",
                BODY,
                colour::DIM,
                Align::Centre,
            );
        }
        for (row, g) in self.games.iter().enumerate().skip(self.first).take(ROWS) {
            let r = Rect::new(16, TOP + (row - self.first) as i32 * ROW, W - 32, ROW - 8);
            if row == self.chosen {
                c.round(r, 20, colour::PANEL_LIT);
            }
            c.text(
                Rect::new(r.x + 24, r.y, r.w - 140, r.h),
                &g.name,
                BODY,
                colour::TEXT,
                Align::Left,
            );
            c.text(
                Rect::new(r.x + r.w - 120, r.y, 100, r.h),
                g.console,
                SMALL,
                colour::DIM,
                Align::Right,
            );
        }
        c.text(
            Rect::new(32, H - 44, W - 64, 36),
            "A to play · B to leave",
            SMALL,
            colour::DIM,
            Align::Centre,
        );
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
    fn the_list_scrolls_with_the_choice() {
        let games = (0..30)
            .map(|i| Game {
                path: PathBuf::from(format!("{i}.gb")),
                name: format!("{i:02}"),
                console: "GB",
            })
            .collect();
        let mut p = Picker::new(games);
        p.step(-1);
        assert_eq!(p.chosen, 0);
        p.step(ROWS as isize);
        assert_eq!((p.chosen, p.first), (ROWS, 1));
        p.step(100);
        assert_eq!(p.chosen, 29);
        assert_eq!(p.at(0.5, (TOP as f32 + 1.0) / H as f32), Some(p.first));
        assert!(p.render().is_some() && p.render().is_none());
    }
}
