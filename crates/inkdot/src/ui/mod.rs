//! The menu on the bottom screen, and what the top screen shows while it is open: DSlot's own
//! menu (nibkit's `menu`), so pausing a game feels like opening DSlot on it.
//!
//! | Page | |
//! |---|---|
//! | Play | resume, save and load a state, fast-forward, starting again on the other console, save and quit |
//! | States | the six slots and the resume point as a shelf of cards, the highlighted one's picture on the top screen |
//! | Game, GB / GBC | the options for this game, and for every game of its console (the same rows as DSlot's pages) |
//! | System | both screens' brightness, the volume, the clock |
//!
//! While the game plays the bottom screen is blank; Back or a tap opens the menu (the game
//! pauses). The pages are built from a [`Model`] the main loop keeps up to date; a button or a
//! touch comes back as an [`Action`] for it to carry out.

use nibkit::menu::raster::{Canvas, Fonts};
use nibkit::menu::{
    self, colour, layout, Card, Draw, Effect, Key, Menu, Row, Shelf, Slot, Status, Value,
};
use nibkit::settings::Value as Setting;

/// The screen's size.
pub const W: i32 = 1024;
pub const H: i32 = 768;

/// What the interface asks the emulator to do.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    /// Pause and show the menu (a tap on the blank bottom screen).
    OpenMenu,
    /// Save the game to slot n (1-6), or the quick slot (0).
    Save(usize),
    Load(usize),
    /// Set an option (its key) to a value, for this game or for its console.
    Set {
        key: String,
        value: Setting,
        for_game: bool,
    },
    /// Clear an option at this game's level or its console's, so the level above applies.
    Unset {
        key: String,
        for_game: bool,
    },
    /// Start the game again on the console its look runs on.
    Restart,
    ToggleFast,
    Brightness {
        top: bool,
        percent: u8,
    },
    Volume(u8),
    /// 12-hour (true) or 24-hour clock, for every app.
    Clock(bool),
    /// Close the menu and play.
    Resume,
    Quit,
}

/// A save state slot as the shelf shows it.
#[derive(Clone, Debug, Default)]
pub struct SlotInfo {
    /// 160×144 RGBA.
    pub picture: Option<Vec<u8>>,
    pub made: String,
    pub played: String,
}

/// An option as its rows show it, at one level.
#[derive(Clone, Debug)]
pub struct OptionRow {
    pub key: String,
    pub label: String,
    /// The value set at this level, shown; `None` when it comes from the level above.
    pub here: Option<String>,
    /// "Use GB (Light)", "Use default (On)".
    pub inherit: String,
    /// The choices in order, for left and right: first "Use …" (`None`), then the values.
    pub values: Vec<Option<Setting>>,
    /// Which of them is the row's now.
    pub at: usize,
    /// Choices go round; a number stops at its ends.
    pub wraps: bool,
    /// What Y does on it set here: "use GB".
    pub use_hint: String,
    /// What it changes shows on the top screen (the look's rows): don't dim the game there.
    pub shows: bool,
}

/// What the interface shows: the main loop keeps it up to date.
#[derive(Clone, Debug, Default)]
pub struct Model {
    pub title: String,
    /// "GB" or "GBC": the console's page.
    pub console: String,
    pub clock: String,
    pub battery: Option<u8>,
    /// Today's play time, as the paused screen says it.
    pub played: String,
    /// Slots 0 (the resume point) to 6.
    pub slots: Vec<SlotInfo>,
    pub quick_slot: usize,
    pub fast: bool,
    /// The options for this game, and for every game of its console.
    pub game_options: Vec<OptionRow>,
    pub console_options: Vec<OptionRow>,
    pub top_brightness: Option<u8>,
    pub bottom_brightness: Option<u8>,
    pub volume: Option<u8>,
    pub hour12: bool,
    /// The look runs on the other console: what starting again on it is called.
    pub restart: Option<String>,
    pub toast: Option<String>,
}

/// The pages, in their tabs' order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Play,
    States,
    Game,
    Console,
    System,
}

const PAGES: [Page; 5] = [
    Page::Play,
    Page::States,
    Page::Game,
    Page::Console,
    Page::System,
];

/// What a row does.
#[derive(Clone, Debug, PartialEq)]
enum Does {
    Resume,
    SaveState,
    LoadState,
    Fast,
    Restart,
    Quit,
    Option { key: String, for_game: bool },
    Brightness { top: bool },
    Volume,
    Clock,
    Nothing,
}

/// A button the menu answers (the D-pad, A, B, X, Y, L, R).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Nav {
    Up,
    Down,
    Left,
    Right,
    A,
    B,
    X,
    Y,
    L,
    R,
}

/// The interface's state.
pub struct Ui {
    pub page: Page,
    /// Open (the game paused) or not (the bottom screen blank).
    pub menu: bool,
    /// Each page's highlighted row, and its first row on screen.
    rows: [(usize, usize); 5],
    /// The highlighted card on States: its place on the shelf (slots 1-6, then the resume
    /// point).
    card: usize,
    /// An action waiting for its second press: which one.
    confirm: Option<Confirm>,
    fonts: Fonts,
    bottom: Canvas,
    top: Canvas,
    dirty: bool,
    /// The top screen's overlay needs drawing again.
    top_dirty: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Confirm {
    Restart,
    SaveOver(usize),
}

/// Whether there is a resume point (slot 0) to show: it is written when the game is left.
fn has_resume(m: &Model) -> bool {
    m.slots.first().is_some_and(|s| s.picture.is_some())
}

/// The slot at the shelf's place `at`: the resume point (0) first when there is one, then slots
/// 1-6, as every app's shelf has them.
fn slot_at(m: &Model, at: usize) -> usize {
    if has_resume(m) {
        at
    } else {
        at + 1
    }
}

/// The shelf's place of a numbered slot (1-6).
fn place_of(m: &Model, slot: usize) -> usize {
    if has_resume(m) {
        slot
    } else {
        slot - 1
    }
}

impl Default for Ui {
    fn default() -> Ui {
        Ui::new()
    }
}

impl Ui {
    pub fn new() -> Ui {
        Ui {
            page: Page::Play,
            menu: false,
            rows: [(0, 0); 5],
            card: 0,
            confirm: None,
            fonts: Fonts::new(),
            bottom: Canvas::new(1.0),
            top: Canvas::new(1.0),
            dirty: true,
            top_dirty: true,
        }
    }

    /// Something shown changed: draw again.
    pub fn touch_up(&mut self) {
        self.dirty = true;
        self.top_dirty = true;
    }

    pub fn go(&mut self, page: Page) {
        self.page = page;
        self.confirm = None;
        self.touch_up();
    }

    /// Opens the menu on Play (the game pauses).
    pub fn open_menu(&mut self) {
        self.menu = true;
        self.go(Page::Play);
    }

    pub fn close_menu(&mut self) {
        self.menu = false;
        self.confirm = None;
        self.touch_up();
    }

    fn index(page: Page) -> usize {
        PAGES.iter().position(|p| *p == page).unwrap_or(0)
    }

    fn tabs(&self, m: &Model) -> Vec<(String, bool)> {
        PAGES
            .iter()
            .map(|p| {
                let name = match p {
                    Page::Play => "Play".to_owned(),
                    Page::States => "States".to_owned(),
                    Page::Game => "Game".to_owned(),
                    Page::Console => m.console.clone(),
                    Page::System => "System".to_owned(),
                };
                (name, *p == self.page)
            })
            .collect()
    }

    /// The page's rows, what each does, and the line under the tabs.
    fn rows(&self, m: &Model) -> (Vec<(Row, Does)>, String) {
        let row = |label: &str, value: Value, does: Does| (Row::new(label, value), does);
        let quick = format!("Slot {}", m.quick_slot.max(1));
        match self.page {
            Page::Play => {
                let mut rows = vec![
                    row("Resume", Value::None, Does::Resume),
                    row("Save state", Value::More(quick.clone()), Does::SaveState),
                    row("Load state", Value::More(quick), Does::LoadState),
                    row(
                        "Fast-forward",
                        Value::Text(if m.fast { "On" } else { "Off" }.into()),
                        Does::Fast,
                    ),
                ];
                if let Some(restart) = &m.restart {
                    let value = if self.confirm == Some(Confirm::Restart) {
                        Value::Prompt {
                            text: "A again (saved first)".into(),
                            warning: false,
                        }
                    } else {
                        Value::None
                    };
                    rows.push(row(restart, value, Does::Restart));
                }
                rows.push(row("Save and quit", Value::None, Does::Quit));
                (rows, m.title.to_uppercase())
            }
            Page::Game | Page::Console => {
                let for_game = self.page == Page::Game;
                let options = if for_game {
                    &m.game_options
                } else {
                    &m.console_options
                };
                // The look's rows under LOOK, the rest under PLAY.
                let group = |k: &str| match k {
                    "look" | "palette" | "colors" | "lcd" => "LOOK",
                    _ => "PLAY",
                };
                let mut rows = Vec::new();
                let mut last = "";
                for o in options {
                    if group(&o.key) != last {
                        last = group(&o.key);
                        rows.push((Row::heading(last), Does::Nothing));
                    }
                    let value = match &o.here {
                        Some(v) => Value::Text(v.clone()),
                        None => Value::Inherited(o.inherit.clone()),
                    };
                    rows.push(row(
                        &o.label,
                        value,
                        Does::Option {
                            key: o.key.clone(),
                            for_game,
                        },
                    ));
                }
                let title = if for_game {
                    m.title.to_uppercase()
                } else {
                    format!("EVERY {} GAME", m.console)
                };
                (rows, title)
            }
            Page::System => {
                let mut rows = Vec::new();
                if let Some(p) = m.top_brightness {
                    rows.push(row(
                        "Top screen",
                        Value::Slider(p),
                        Does::Brightness { top: true },
                    ));
                }
                if let Some(p) = m.bottom_brightness {
                    rows.push(row(
                        "Bottom screen",
                        Value::Slider(p),
                        Does::Brightness { top: false },
                    ));
                }
                if let Some(v) = m.volume {
                    rows.push(row("Volume", Value::Slider(v), Does::Volume));
                }
                let clock = if m.hour12 { "12-hour" } else { "24-hour" };
                rows.push(row("Clock", Value::Text(clock.into()), Does::Clock));
                (rows, "NIBOS".into())
            }
            Page::States => (Vec::new(), String::new()),
        }
    }

    /// The page as nibkit's menu, with what each row does.
    fn page(&self, m: &Model) -> (Menu, Vec<Does>) {
        let (rows, title) = self.rows(m);
        let (row, scroll) = self.rows[Self::index(self.page)];
        let (rows, does): (Vec<Row>, Vec<Does>) = rows.into_iter().unzip();
        let mut menu = Menu {
            tabs: self.tabs(m),
            title,
            row: row.min(rows.len().saturating_sub(1)),
            scroll,
            rows,
            hint: String::new(),
        };
        menu.settle();
        menu.hint = self.hint(m, &menu, &does);
        (menu, does)
    }

    fn shelf(&self, m: &Model) -> Shelf {
        // The resume point first, set apart (its picture is slot 0's), then the six slots.
        let resume = has_resume(m).then(|| Card::resume(Some(0), m.slots[0].made.clone()));
        let slots = (1..=6).map(|n| {
            let info = m.slots.get(n).filter(|i| i.picture.is_some());
            let label = match info {
                Some(i) => format!("{n} \u{b7} {}", i.made),
                None => format!("{n}"),
            };
            Card::slot(info.map(|_| n), label)
        });
        let cards: Vec<Card> = resume.into_iter().chain(slots).collect();
        let sep = "   \u{b7}   ";
        let on_resume = cards.get(self.card).is_some_and(|c| c.resume);
        let hint = match self.confirm {
            Some(Confirm::SaveOver(n)) => format!("X  again to save over slot {n}{sep}B  resume"),
            // The resume point is written on leaving: it loads, nothing saves onto it.
            _ if on_resume => format!("A  load{sep}D-pad  browse{sep}L / R  page{sep}B  resume"),
            _ => {
                format!("A  load{sep}X  save here{sep}D-pad  browse{sep}L / R  page{sep}B  resume")
            }
        };
        Shelf {
            tabs: self.tabs(m),
            cards,
            at: self.card,
            hint,
        }
    }

    fn hint(&self, _m: &Model, menu: &Menu, does: &[Does]) -> String {
        let sep = "   \u{b7}   ";
        match self.page {
            Page::Play => format!("A  choose{sep}D-pad  move{sep}L / R  page{sep}B  resume"),
            Page::System => format!("D-pad  change{sep}L / R  page{sep}B  resume"),
            _ => {
                let set_here = matches!(
                    menu.rows.get(menu.row).map(|r| &r.value),
                    Some(Value::Text(_))
                );
                let opts = if self.page == Page::Game {
                    &_m.game_options
                } else {
                    &_m.console_options
                };
                let use_hint = match does.get(menu.row) {
                    Some(Does::Option { key, .. }) if set_here => opts
                        .iter()
                        .find(|o| &o.key == key)
                        .map(|o| format!("Y  {}{sep}", o.use_hint)),
                    _ => None,
                };
                format!(
                    "D-pad  change{sep}{}L / R  page{sep}B  resume",
                    use_hint.unwrap_or_default()
                )
            }
        }
    }

    fn status(m: &Model) -> Status {
        Status {
            clock: m.clock.clone(),
            battery: m.battery.map(|b| (b, false)),
            slot: Slot::Two(420.0),
        }
    }

    /// The bottom screen's picture, when it has changed since last asked: blank in play.
    pub fn render(&mut self, m: &Model) -> Option<&[u8]> {
        if !std::mem::take(&mut self.dirty) {
            return None;
        }
        self.bottom.clear(colour::BLACK);
        if self.menu {
            let mut draws = match self.page {
                Page::States => layout::shelf(&self.shelf(m), &Self::status(m)),
                _ => layout::menu(&self.page(m).0, &Self::status(m)),
            };
            if let Some(t) = &m.toast {
                draws.push(Draw::Text {
                    text: t.clone(),
                    size: 24.0,
                    bold: true,
                    x: menu::W / 2.0,
                    y: 596.0,
                    anchor: 0.5,
                    colour: colour::WHITE,
                });
            }
            let pictures: Vec<Option<&[u8]>> =
                m.slots.iter().map(|s| s.picture.as_deref()).collect();
            self.bottom.draw(&self.fonts, &draws, |i| {
                pictures.get(i).copied().flatten().map(|p| (p, 160, 144))
            });
        }
        Some(&self.bottom.px)
    }

    /// What goes over the game on the top screen, when it has changed since last asked:
    /// `Some(Some(px))`, premultiplied RGBA, the game dimmed with "Paused" or a save state's
    /// picture and details; `Some(None)` to show the game as it is (in play, and on the look's
    /// rows, so a change shows); `None` when nothing changed.
    pub fn render_top(&mut self, m: &Model) -> Option<Option<&[u8]>> {
        if !std::mem::take(&mut self.top_dirty) {
            return None;
        }
        Some(self.overlay(m))
    }

    fn overlay(&mut self, m: &Model) -> Option<&[u8]> {
        if !self.menu {
            return None;
        }
        // On the look's rows the game shows as it is, so a change shows.
        let look_row = match (self.page, self.focused(m)) {
            (Page::Game | Page::Console, Does::Option { key, for_game }) => {
                let opts = if for_game {
                    &m.game_options
                } else {
                    &m.console_options
                };
                opts.iter().any(|o| o.key == key && o.shows)
            }
            _ => false,
        };
        let f = &self.fonts;
        let c = &mut self.top;
        c.clear(colour::BLACK);
        let text =
            |c: &mut Canvas, s: &str, size: (f32, bool), at: (f32, f32), anchor: f32, ink| {
                c.text(f, s, size, at, anchor, ink)
            };
        if self.page == Page::States {
            let slot = slot_at(m, self.card);
            let info = m.slots.get(slot).cloned().unwrap_or_default();
            let name = if slot == 0 {
                "Resume point".to_owned()
            } else {
                format!("Slot {slot}")
            };
            match &info.picture {
                Some(p) => c.draw(
                    f,
                    &[Draw::Picture {
                        index: 0,
                        x: 62.0,
                        y: 104.0,
                        w: 560.0,
                        h: 504.0,
                        faded: false,
                    }],
                    |_| Some((&p[..], 160, 144)),
                ),
                None => c.draw(
                    f,
                    &[Draw::Rect {
                        x: 62.0,
                        y: 104.0,
                        w: 560.0,
                        h: 504.0,
                        colour: [21, 24, 30],
                    }],
                    |_| None,
                ),
            }
            text(c, &name, (60.0, true), (660.0, 170.0), 0.0, colour::WHITE);
            if slot != 0 && slot == m.quick_slot {
                text(
                    c,
                    "Quick slot",
                    (30.0, false),
                    (660.0, 246.0),
                    0.0,
                    colour::DIM,
                );
            }
            if info.picture.is_some() {
                let played = info.played.trim_end_matches(" played");
                for (k, (label, value)) in [("Saved", info.made.as_str()), ("Played", played)]
                    .iter()
                    .enumerate()
                {
                    let y = 340.0 + k as f32 * 40.0;
                    text(c, label, (26.0, false), (660.0, y), 0.0, colour::DIM);
                    text(c, value, (26.0, true), (780.0, y), 0.0, colour::WHITE);
                }
            } else {
                text(c, "Empty", (26.0, false), (660.0, 340.0), 0.0, colour::DIM);
            }
            return Some(&c.px);
        }
        if look_row {
            return None;
        }
        // The game dimmed, its name and today's play time over it.
        text(
            c,
            "Paused",
            (64.0, true),
            (512.0, 300.0),
            0.5,
            colour::WHITE,
        );
        let sub = if m.played.is_empty() {
            m.title.clone()
        } else {
            format!("{} \u{b7} {}", m.title, m.played)
        };
        text(c, &sub, (30.0, false), (512.0, 384.0), 0.5, colour::DIM);
        // Black behind is a dim (alpha 55%); text keeps its own colour.
        for p in c.px.chunks_exact_mut(4) {
            p[3] = p[0].max(p[1]).max(p[2]).max(140);
        }
        Some(&c.px)
    }

    /// What the highlighted row does.
    fn focused(&self, m: &Model) -> Does {
        let (menu, does) = self.page(m);
        does.get(menu.row).cloned().unwrap_or(Does::Nothing)
    }

    fn keep(&mut self, menu: &Menu) {
        self.rows[Self::index(self.page)] = (menu.row, menu.scroll);
    }

    /// A button, with the menu open.
    pub fn nav(&mut self, m: &Model, nav: Nav) -> Option<Action> {
        self.touch_up();
        let key = match nav {
            Nav::Up => Key::Up,
            Nav::Down => Key::Down,
            Nav::Left => Key::Left,
            Nav::Right => Key::Right,
            Nav::A => Key::A,
            Nav::B => Key::B,
            Nav::X => Key::X,
            Nav::Y => Key::Y,
            Nav::L => Key::L,
            Nav::R => Key::R,
        };
        if self.page == Page::States {
            let mut shelf = self.shelf(m);
            let effect = shelf.key(key);
            if shelf.at != self.card {
                self.card = shelf.at;
                self.confirm = None;
            }
            return self.effect(m, effect, &[]);
        }
        let (mut menu, does) = self.page(m);
        let before = menu.row;
        let effect = menu.key(key);
        if menu.row != before {
            self.confirm = None;
        }
        self.keep(&menu);
        self.effect(m, effect, &does)
    }

    /// A finger down on the bottom screen (screen pixels).
    pub fn tap(&mut self, m: &Model, x: i32, y: i32) -> Option<Action> {
        self.touch_up();
        if !self.menu {
            return Some(Action::OpenMenu);
        }
        let (x, y) = (x as f32, y as f32);
        if self.page == Page::States {
            let mut shelf = self.shelf(m);
            let effect = shelf.touch(x, y);
            if shelf.at != self.card {
                self.card = shelf.at;
                self.confirm = None;
            }
            return self.effect(m, effect, &[]);
        }
        let (mut menu, does) = self.page(m);
        // A slider follows the finger along its bar.
        if let Some(a) = self.slide(&menu, &does, x, y) {
            return Some(a);
        }
        let effect = menu.touch(x, y);
        self.keep(&menu);
        self.effect(m, effect, &does)
    }

    /// A finger moving: a slider under it follows.
    pub fn drag(&mut self, m: &Model, x: i32, y: i32) -> Option<Action> {
        if !self.menu || self.page != Page::System {
            return None;
        }
        let (menu, does) = self.page(m);
        self.dirty = true;
        self.slide(&menu, &does, x as f32, y as f32)
    }

    fn slide(&self, menu: &Menu, does: &[Does], x: f32, y: f32) -> Option<Action> {
        let i = menu.scroll + layout::row_hit(y)?;
        let Value::Slider(_) = menu.rows.get(i)?.value else {
            return None;
        };
        let (bar, gap) = (180.0, 76.0);
        let x0 = layout::RIGHT - gap - bar;
        if !(x0 - 20.0..x0 + bar + 20.0).contains(&x) {
            return None;
        }
        let percent = (((x - x0) / bar) * 100.0).clamp(0.0, 100.0) as u8;
        match does.get(i)? {
            Does::Brightness { top } => Some(Action::Brightness { top: *top, percent }),
            Does::Volume => Some(Action::Volume(percent)),
            _ => None,
        }
    }

    fn option<'m>(m: &'m Model, key: &str, for_game: bool) -> Option<&'m OptionRow> {
        let opts = if for_game {
            &m.game_options
        } else {
            &m.console_options
        };
        opts.iter().find(|o| o.key == key)
    }

    /// Carries out what a button or touch said.
    fn effect(&mut self, m: &Model, effect: Option<Effect>, does: &[Does]) -> Option<Action> {
        match effect? {
            Effect::Page(i) => {
                if PAGES[i] == Page::States {
                    // Opened on the first numbered slot, past the resume point.
                    self.card = place_of(m, 1);
                }
                self.go(PAGES[i]);
                None
            }
            Effect::Back => Some(Action::Resume),
            Effect::Card { at, key } => {
                let slot = slot_at(m, at);
                let used = m.slots.get(slot).is_some_and(|s| s.picture.is_some());
                match key {
                    Key::A if used => Some(Action::Load(slot)),
                    Key::X if slot != 0 => {
                        if used && self.confirm != Some(Confirm::SaveOver(slot)) {
                            self.confirm = Some(Confirm::SaveOver(slot));
                            return None;
                        }
                        self.confirm = None;
                        Some(Action::Save(slot))
                    }
                    _ => None,
                }
            }
            Effect::Use(row) => match does.get(row)? {
                Does::Option { key, for_game } => Some(Action::Unset {
                    key: key.clone(),
                    for_game: *for_game,
                }),
                _ => None,
            },
            Effect::Change { row, step } => self.change(m, does.get(row)?, step),
            Effect::Choose(row) => match does.get(row)? {
                Does::Resume => Some(Action::Resume),
                Does::SaveState | Does::LoadState => {
                    self.card = place_of(m, m.quick_slot.clamp(1, 6));
                    self.go(Page::States);
                    None
                }
                Does::Restart => {
                    if self.confirm == Some(Confirm::Restart) {
                        self.confirm = None;
                        Some(Action::Restart)
                    } else {
                        self.confirm = Some(Confirm::Restart);
                        None
                    }
                }
                Does::Quit => Some(Action::Quit),
                other => {
                    let other = other.clone();
                    self.change(m, &other, 1)
                }
            },
        }
    }

    fn change(&mut self, m: &Model, does: &Does, step: i32) -> Option<Action> {
        match does {
            Does::Fast => Some(Action::ToggleFast),
            Does::Clock => Some(Action::Clock(!m.hour12)),
            Does::Brightness { top } => {
                let now = if *top {
                    m.top_brightness
                } else {
                    m.bottom_brightness
                }?;
                let percent = (now as i32 + 5 * step).clamp(0, 100) as u8;
                Some(Action::Brightness { top: *top, percent })
            }
            Does::Volume => {
                let percent = (m.volume? as i32 + 5 * step).clamp(0, 100) as u8;
                Some(Action::Volume(percent))
            }
            Does::Option { key, for_game } => {
                let o = Self::option(m, key, *for_game)?;
                let n = o.values.len().max(1) as i32;
                let next = if o.wraps {
                    (o.at as i32 + step).rem_euclid(n)
                } else {
                    (o.at as i32 + step).clamp(0, n - 1)
                } as usize;
                if next == o.at {
                    return None;
                }
                Some(match o.values.get(next)?.clone() {
                    Some(value) => Action::Set {
                        key: key.clone(),
                        value,
                        for_game: *for_game,
                    },
                    None => Action::Unset {
                        key: key.clone(),
                        for_game: *for_game,
                    },
                })
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn option(key: &str, here: Option<&str>) -> OptionRow {
        OptionRow {
            key: key.into(),
            label: key.into(),
            here: here.map(str::to_owned),
            inherit: "Use default (Smooth)".into(),
            values: vec![None, Some("panel".into()), Some("exact".into())],
            at: if here.is_some() { 1 } else { 0 },
            wraps: true,
            use_hint: "use GB".into(),
            shows: key == "look",
        }
    }

    fn model() -> Model {
        let mut slots = vec![SlotInfo::default(); 7];
        slots[2].picture = Some(vec![0; 160 * 144 * 4]);
        slots[2].made = "Today 4:41 PM".into();
        Model {
            title: "Tetris".into(),
            console: "GB".into(),
            slots,
            quick_slot: 2,
            game_options: vec![option("look", None), option("speed", Some("Smooth"))],
            console_options: vec![option("look", None), option("speed", None)],
            hour12: true,
            ..Model::default()
        }
    }

    #[test]
    fn the_bottom_screen_is_blank_in_play_and_a_tap_opens_the_menu() {
        let mut ui = Ui::new();
        let m = model();
        let px = ui.render(&m).unwrap();
        assert!(px.chunks(4).all(|p| p[..3] == [0, 0, 0]));
        assert_eq!(ui.tap(&m, 500, 400), Some(Action::OpenMenu));
        assert_eq!(
            ui.render_top(&m),
            Some(None),
            "nothing over the game in play"
        );
        ui.open_menu();
        assert_eq!(ui.page, Page::Play);
        assert!(ui
            .render(&m)
            .unwrap()
            .chunks(4)
            .any(|p| p[..3] != [0, 0, 0]));
        assert!(
            matches!(ui.render_top(&m), Some(Some(_))),
            "Paused over the game"
        );
        assert_eq!(ui.render_top(&m), None, "drawn once");
    }

    #[test]
    fn play_resumes_saves_and_quits() {
        let mut ui = Ui::new();
        let m = model();
        ui.open_menu();
        assert_eq!(ui.nav(&m, Nav::A), Some(Action::Resume));
        ui.nav(&m, Nav::Down);
        assert_eq!(ui.nav(&m, Nav::A), None, "Save state opens States");
        assert_eq!(ui.page, Page::States);
        assert_eq!(slot_at(&m, ui.card), 2, "on the quick slot");
        // Back on Play, the highlight is where it was (Save state).
        ui.go(Page::Play);
        for _ in 0..2 {
            ui.nav(&m, Nav::Down);
        }
        assert_eq!(ui.nav(&m, Nav::Right), Some(Action::ToggleFast));
        ui.nav(&m, Nav::Down);
        assert_eq!(ui.nav(&m, Nav::A), Some(Action::Quit));
        assert_eq!(ui.nav(&m, Nav::B), Some(Action::Resume));
    }

    #[test]
    fn states_load_and_save_asking_before_replacing() {
        let mut ui = Ui::new();
        let m = model();
        ui.open_menu();
        ui.go(Page::States);
        // Slot 1, empty: nothing to load; saved at once.
        assert_eq!(ui.nav(&m, Nav::A), None);
        assert_eq!(ui.nav(&m, Nav::X), Some(Action::Save(1)));
        ui.nav(&m, Nav::Right);
        assert_eq!(ui.nav(&m, Nav::A), Some(Action::Load(2)));
        assert_eq!(ui.nav(&m, Nav::X), None, "slot 2 holds a state: X again");
        assert_eq!(ui.nav(&m, Nav::X), Some(Action::Save(2)));
        assert!(
            matches!(ui.render_top(&m), Some(Some(_))),
            "its picture on the top screen"
        );
        // With a resume point it comes first, left of slot 1: it loads but takes no save.
        let mut m = m;
        m.slots[0].picture = Some(vec![0; 160 * 144 * 4]);
        m.slots[0].made = "Today 5:02 PM".into();
        ui.go(Page::Play);
        ui.nav(&m, Nav::R);
        assert_eq!(
            (ui.page, slot_at(&m, ui.card)),
            (Page::States, 1),
            "opens on slot 1"
        );
        ui.nav(&m, Nav::Left);
        assert_eq!(slot_at(&m, ui.card), 0);
        assert_eq!(ui.nav(&m, Nav::X), None);
        assert_eq!(ui.nav(&m, Nav::A), Some(Action::Load(0)));
        ui.render(&m);
        ui.nav(&m, Nav::Right);
        assert_eq!(ui.nav(&m, Nav::X), Some(Action::Save(1)));
    }

    #[test]
    fn option_pages_step_inherit_and_use() {
        let mut ui = Ui::new();
        let m = model();
        ui.open_menu();
        ui.nav(&m, Nav::R);
        ui.nav(&m, Nav::R);
        assert_eq!(ui.page, Page::Game);
        // On the look's row the game shows as it is (to see the change).
        assert_eq!(ui.render_top(&m), Some(None));
        assert_eq!(
            ui.nav(&m, Nav::Right),
            Some(Action::Set {
                key: "look".into(),
                value: "panel".into(),
                for_game: true
            })
        );
        assert_eq!(ui.nav(&m, Nav::Y), None, "inherited: nothing to clear");
        ui.nav(&m, Nav::Down);
        assert_eq!(
            ui.nav(&m, Nav::Y),
            Some(Action::Unset {
                key: "speed".into(),
                for_game: true
            })
        );
        assert_eq!(
            ui.nav(&m, Nav::Left),
            Some(Action::Unset {
                key: "speed".into(),
                for_game: true
            })
        );
        ui.nav(&m, Nav::R);
        assert_eq!(ui.page, Page::Console);
        assert_eq!(
            ui.nav(&m, Nav::Right),
            Some(Action::Set {
                key: "look".into(),
                value: "panel".into(),
                for_game: false
            })
        );
    }

    #[test]
    fn restarting_on_the_other_console_asks_first() {
        let mut ui = Ui::new();
        let mut m = model();
        m.restart = Some("Restart as a Game Boy Color".into());
        ui.open_menu();
        for _ in 0..4 {
            ui.nav(&m, Nav::Down);
        }
        assert_eq!(ui.nav(&m, Nav::A), None);
        assert_eq!(ui.nav(&m, Nav::A), Some(Action::Restart));
    }

    #[test]
    fn system_sliders_and_the_clock() {
        let mut ui = Ui::new();
        let mut m = model();
        m.top_brightness = Some(60);
        m.volume = Some(40);
        ui.open_menu();
        ui.go(Page::System);
        assert_eq!(
            ui.nav(&m, Nav::Right),
            Some(Action::Brightness {
                top: true,
                percent: 65
            })
        );
        ui.nav(&m, Nav::Down);
        assert_eq!(ui.nav(&m, Nav::Left), Some(Action::Volume(35)));
        ui.nav(&m, Nav::Down);
        assert_eq!(ui.nav(&m, Nav::A), Some(Action::Clock(false)));
    }
}
