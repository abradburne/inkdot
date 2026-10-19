//! The DS+'s controls as the game and the emulator use them.
//!
//! | Control | |
//! |---|---|
//! | A, B, Select, Start, D-pad (or the stick) | the Game Boy's own buttons |
//! | X, Y | turbo A and turbo B (on 4 frames, off 4) |
//! | R2 (held) | fast-forward |
//! | Back | the menu on the bottom screen (the game pauses); Back again to play |
//! | Menu (tapped) | save where the game is and leave, back to DSlot's shelf |
//! | Menu + R, Menu + L | quick save, quick load (the last slot used) |
//! | Menu + Select | fast-forward on or off |
//!
//! As every program on the DS+ does it: Back opens the program's own menu, Menu goes home
//! (DSlot's docs/emulators.md: Menu saves the resume state and quits). Menu + volume is nibOS's
//! brightness shortcut and never arrives.
//!
//! On nibOS, Menu arrives only when it is let go of unless another button is pressed with it
//! (nibos-input holds it back in case it is Menu + volume), so a tap is a press and a release
//! together, and Menu + a button is Menu's press, then the button's.

use inkdot_core::hw::button;

/// A control, by what is printed on it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum Pad {
    A,
    B,
    X,
    Y,
    Select,
    Start,
    Up,
    Down,
    Left,
    Right,
    L,
    R,
    R2,
    Menu,
}

/// What a control asks of the emulator, rather than of the game.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    /// Open the menu on the bottom screen, or close it.
    PauseMenu,
    /// Save the resume state and quit.
    Leave,
    QuickSave,
    QuickLoad,
    ToggleFast,
}

/// What a key asks for, by SDL's name for it: Back (and Escape, on a computer) the menu.
pub fn key(name: &str) -> Option<Command> {
    (nibkit::input::BACK_KEYCODES.contains(&name) || name == "Escape").then_some(Command::PauseMenu)
}

/// The controls' state.
#[derive(Clone, Debug, Default)]
pub struct Controls {
    held: Vec<Pad>,
    menu_held: bool,
    /// Menu was held with another button: its release isn't a tap.
    menu_combined: bool,
    stick: (i16, i16),
    frame: u32,
}

/// Stick deflection that counts as a direction (of ±32767): half-way.
const STICK_THRESHOLD: i16 = 16_000;

impl Controls {
    /// A control went down; returns what it asks of the emulator, if anything.
    pub fn press(&mut self, pad: Pad) -> Option<Command> {
        if pad == Pad::Menu {
            self.menu_held = true;
            self.menu_combined = false;
            return None;
        }
        if !self.held.contains(&pad) {
            self.held.push(pad);
        }
        if self.menu_held {
            self.menu_combined = true;
            return match pad {
                Pad::R => Some(Command::QuickSave),
                Pad::L => Some(Command::QuickLoad),
                Pad::Select => Some(Command::ToggleFast),
                _ => None,
            };
        }
        None
    }

    /// A control came up.
    pub fn release(&mut self, pad: Pad) -> Option<Command> {
        if pad == Pad::Menu {
            let tap = self.menu_held && !self.menu_combined;
            self.menu_held = false;
            return tap.then_some(Command::Leave);
        }
        self.held.retain(|&p| p != pad);
        None
    }

    /// The stick's horizontal position alone (SDL reports each axis by itself).
    pub fn stick_x(&mut self, x: i16) {
        self.stick.0 = x;
    }

    /// The stick's vertical position alone.
    pub fn stick_y(&mut self, y: i16) {
        self.stick.1 = y;
    }

    /// Lets go of everything (when the menu opens, so nothing stays held in the game).
    pub fn clear(&mut self) {
        self.held.clear();
        self.stick = (0, 0);
    }

    /// Whether fast-forward is held.
    pub fn fast_held(&self) -> bool {
        self.held.contains(&Pad::R2)
    }

    /// The Game Boy buttons held now, as [`button`] bits; call once a frame (turbo counts
    /// frames).
    pub fn game_buttons(&mut self) -> u8 {
        self.frame = self.frame.wrapping_add(1);
        let turbo = self.frame & 4 != 0;
        // Menu combinations aren't the game's.
        if self.menu_held {
            return 0;
        }
        let mut b = 0;
        for &p in &self.held {
            b |= match p {
                Pad::A => button::A,
                Pad::B => button::B,
                Pad::X if turbo => button::A,
                Pad::Y if turbo => button::B,
                Pad::Select => button::SELECT,
                Pad::Start => button::START,
                Pad::Up => button::UP,
                Pad::Down => button::DOWN,
                Pad::Left => button::LEFT,
                Pad::Right => button::RIGHT,
                _ => 0,
            };
        }
        let (x, y) = self.stick;
        if x < -STICK_THRESHOLD {
            b |= button::LEFT;
        } else if x > STICK_THRESHOLD {
            b |= button::RIGHT;
        }
        if y < -STICK_THRESHOLD {
            b |= button::UP;
        } else if y > STICK_THRESHOLD {
            b |= button::DOWN;
        }
        // Opposite directions together aren't possible on a real D-pad; some games misbehave.
        if b & (button::LEFT | button::RIGHT) == button::LEFT | button::RIGHT {
            b &= !(button::LEFT | button::RIGHT);
        }
        if b & (button::UP | button::DOWN) == button::UP | button::DOWN {
            b &= !(button::UP | button::DOWN);
        }
        b
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Back opens the menu and a Menu tap leaves, as on the rest of the DS+ (the two were the
    /// other way round once).
    #[test]
    fn back_pauses_and_menu_leaves() {
        for back in nibkit::input::BACK_KEYCODES {
            assert_eq!(key(back), Some(Command::PauseMenu));
        }
        assert_eq!(key("Escape"), Some(Command::PauseMenu));
        assert_eq!(key("Return"), None);
        let mut c = Controls::default();
        assert_eq!(c.press(Pad::Menu), None, "Menu acts on release");
        assert_eq!(c.release(Pad::Menu), Some(Command::Leave));
    }

    #[test]
    fn a_menu_tap_leaves_and_a_combination_does_not() {
        let mut c = Controls::default();
        c.press(Pad::Menu);
        assert_eq!(c.release(Pad::Menu), Some(Command::Leave));
        c.press(Pad::Menu);
        assert_eq!(c.press(Pad::R), Some(Command::QuickSave));
        c.release(Pad::R);
        assert_eq!(c.release(Pad::Menu), None);
    }

    #[test]
    fn buttons_and_the_stick() {
        let mut c = Controls::default();
        c.press(Pad::A);
        c.press(Pad::Start);
        c.stick_x(-30_000);
        c.stick_y(2_000);
        assert_eq!(c.game_buttons(), button::A | button::START | button::LEFT);
        c.release(Pad::A);
        c.stick_x(0);
        c.stick_y(0);
        assert_eq!(c.game_buttons(), button::START);
    }

    #[test]
    fn turbo_alternates_every_four_frames() {
        let mut c = Controls::default();
        c.press(Pad::X);
        let frames: Vec<bool> = (0..16).map(|_| c.game_buttons() & button::A != 0).collect();
        assert_eq!(frames.iter().filter(|&&on| on).count(), 8);
        assert!(frames.windows(4).any(|w| w.iter().all(|&on| on)));
    }

    #[test]
    fn opposite_directions_cancel() {
        let mut c = Controls::default();
        c.press(Pad::Left);
        c.press(Pad::Right);
        assert_eq!(c.game_buttons(), 0);
    }
}
