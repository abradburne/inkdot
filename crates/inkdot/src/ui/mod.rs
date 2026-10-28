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
    self, colour, layout, top, Card, Draw, Effect, Key, Menu, Row, Shelf, Slot, Status, Value,
};
use nibkit::settings::Value as Setting;

// The toast over the game is nibkit's, the same as DSperate's.
use nibkit::menu::indicators::Indicators;
pub use nibkit::menu::toast;

/// A picture to lay over the game: RGBA with straight alpha, its width and height.
pub type Picture = (Vec<u8>, usize, usize);

/// A rectangle on the top screen, in pixels: left, top, width, height.
pub type Rect = (usize, usize, usize, usize);

/// What the top screen shows over the game, when it has changed ([`Ui::render_top`]).
#[derive(Debug, PartialEq, Eq)]
pub enum Top<'a> {
    /// The game as it is.
    Game,
    /// This over it: premultiplied RGBA, the whole screen.
    Whole(&'a [u8]),
    /// The same screen as before but for this rectangle (a challenge's ring moving on): only it
    /// need go to the GPU.
    Part(&'a [u8], Rect),
}

/// Whether two rectangles share a pixel.
fn overlap(a: Rect, b: Rect) -> bool {
    a.0 < b.0 + b.2 && b.0 < a.0 + a.2 && a.1 < b.1 + b.3 && b.1 < a.1 + a.3
}

// The Connections page (Wi-Fi, Bluetooth, the paired devices) is nibkit's, on nibOS; elsewhere
// there are no radios for it, and no page.
#[cfg(feature = "nibos")]
use nibkit::menu::connections::{self, Connections};
#[cfg(not(feature = "nibos"))]
use no_connections::{self as connections, Connections};

#[cfg(not(feature = "nibos"))]
mod no_connections {
    use nibkit::menu::Row;
    pub const HINT: &str = "";
    /// Never there: a build without nibOS's radios.
    pub struct Connections;
    impl Connections {
        pub fn start() -> Option<Connections> {
            None
        }
        pub fn refresh(&mut self) {}
        pub fn title(&self) -> String {
            String::new()
        }
        pub fn rows(&self) -> Vec<Row> {
            Vec::new()
        }
        pub fn act(&mut self, _row: usize) -> bool {
            false
        }
    }
}

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
    /// Wi-Fi, Bluetooth and the paired devices: after System, where there are radios.
    Connections,
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
    rows: [(usize, usize); 6],
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
    /// The notices over the game in play, one after another (nibkit's toast queue), and the
    /// one last drawn (its serial).
    notices: toast::Queue,
    notice_drawn: Option<u64>,
    /// RetroAchievements' progress and challenge indicators (nibkit's, the same as DSperate's),
    /// bottom left while they last; what their last tick gave, and where they and the notice
    /// were last laid on the top screen.
    indicators: Indicators,
    indicators_shown: u64,
    stack_at: Option<Rect>,
    notice_at: Option<Rect>,
    /// The Connections page's state while the menu is open (its worker stops when dropped),
    /// and what it last showed, to draw it again when that changes.
    conn: Option<Connections>,
    conn_seen: Option<(String, Vec<Row>)>,
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
            rows: [(0, 0); 6],
            card: 0,
            confirm: None,
            fonts: Fonts::new(),
            bottom: Canvas::new(1.0),
            top: Canvas::new(1.0),
            dirty: true,
            top_dirty: true,
            notices: toast::Queue::new(),
            notice_drawn: None,
            indicators: Indicators::new(),
            indicators_shown: 0,
            stack_at: None,
            notice_at: None,
            conn: None,
            conn_seen: None,
        }
    }

    /// Something shown changed: draw again.
    pub fn touch_up(&mut self) {
        self.dirty = true;
        self.top_dirty = true;
    }

    /// Queues a notice over the game: each shows for its time (DSperate's four seconds for an
    /// unlock) once the ones before it have, while the menu is closed.
    pub fn notice(&mut self, notice: toast::Notice) {
        self.notices.push(notice);
    }

    /// A badge a notice or an indicator awaits ([`toast::Notice::awaits_badge`]) has arrived.
    #[cfg_attr(not(feature = "cheevos"), allow(dead_code))]
    pub fn badge(&mut self, name: &str, (rgba, w, h): Picture) {
        self.indicators.badge(name, rgba.clone(), w, h);
        self.notices.badge(name, rgba, w, h);
    }

    /// Achievement `id` can be earned right now ([`toast::Notice::challenge`], with its badge or
    /// awaiting it): its icon stays up until [`Ui::challenge_over`].
    pub fn challenge(&mut self, id: u32, notice: toast::Notice) {
        self.indicators.challenge(id, notice);
    }

    /// The challenge is over.
    #[cfg_attr(not(feature = "cheevos"), allow(dead_code))]
    pub fn challenge_over(&mut self, id: u32) {
        self.indicators.end_challenge(id);
    }

    /// Achievement `id`'s measured progress ([`toast::Notice::progress`]), up or updated until
    /// [`Ui::hide_progress`].
    pub fn progress(&mut self, id: u32, notice: toast::Notice) {
        self.indicators.progress(id, notice);
    }

    /// The progress comes down.
    #[cfg_attr(not(feature = "cheevos"), allow(dead_code))]
    pub fn hide_progress(&mut self) {
        self.indicators.hide_progress();
    }

    pub fn go(&mut self, page: Page) {
        self.page = page;
        self.confirm = None;
        self.touch_up();
    }

    /// Shows a page by its name (play, states, game, console, system, connections), if it is
    /// there: for INKDOT_DEBUG=menu.
    pub fn go_named(&mut self, name: &str) {
        let page = self
            .pages()
            .into_iter()
            .find(|p| format!("{p:?}").eq_ignore_ascii_case(name));
        if let Some(page) = page {
            self.go(page);
        }
    }

    /// Opens the menu on Play (the game pauses).
    pub fn open_menu(&mut self) {
        self.menu = true;
        self.conn = Connections::start();
        self.go(Page::Play);
    }

    pub fn close_menu(&mut self) {
        self.menu = false;
        self.confirm = None;
        self.conn = None;
        self.conn_seen = None;
        if self.page == Page::Connections {
            self.page = Page::Play;
        }
        self.touch_up();
    }

    /// The pages there are: Connections only where there are radios.
    fn pages(&self) -> Vec<Page> {
        let mut pages = PAGES.to_vec();
        if self.conn.is_some() {
            pages.push(Page::Connections);
        }
        pages
    }

    fn index(page: Page) -> usize {
        PAGES.iter().position(|p| *p == page).unwrap_or(PAGES.len())
    }

    fn tabs(&self, m: &Model) -> Vec<(String, bool)> {
        self.pages()
            .iter()
            .map(|p| {
                let name = match p {
                    Page::Play => "Play".to_owned(),
                    Page::States => "States".to_owned(),
                    Page::Game => "Game".to_owned(),
                    Page::Console => m.console.clone(),
                    Page::System => "System".to_owned(),
                    Page::Connections => "Connections".to_owned(),
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
            Page::Connections => match &self.conn {
                Some(c) => (
                    c.rows().into_iter().map(|r| (r, Does::Nothing)).collect(),
                    c.title(),
                ),
                None => (Vec::new(), String::new()),
            },
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
            Page::Connections => connections::HINT.into(),
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
        // The Connections page follows the radios: drawn again when what it shows changes.
        if let (true, Page::Connections, Some(c)) = (self.menu, self.page, self.conn.as_mut()) {
            c.refresh();
            let now = (c.title(), c.rows());
            if self.conn_seen.as_ref() != Some(&now) {
                self.conn_seen = Some(now);
                self.dirty = true;
            }
        }
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

    /// What goes over the game on the top screen, when it has changed since last asked: the game
    /// dimmed with "Paused" or a save state's picture and details; in play, the notice and the
    /// achievement indicators; [`Top::Game`] to show the game as it is (in play with nothing over
    /// it, and on the look's rows, so a change shows); `None` when nothing changed. While a
    /// challenge's ring pulses, the indicators change 16 times a second, and only their
    /// rectangle is laid again ([`Top::Part`]).
    pub fn render_top(&mut self, m: &Model) -> Option<Top<'_>> {
        let now = std::time::Instant::now();
        let shown = self.notices.tick(now).map(toast::Shown::serial);
        if shown != self.notice_drawn {
            self.notice_drawn = shown;
            self.top_dirty = true;
        }
        let up = self.indicators.tick(now);
        // Over the menu's pictures they aren't shown: drawn again when it closes.
        let moved = up != self.indicators_shown && !self.menu;
        self.indicators_shown = up;
        if moved && !self.top_dirty {
            if let Some(r) = self.relay_indicators() {
                return Some(Top::Part(&self.top.px, r));
            }
        }
        self.top_dirty |= moved;
        if !std::mem::take(&mut self.top_dirty) {
            return None;
        }
        Some(match self.overlay(m) {
            Some(px) => Top::Whole(px),
            None => Top::Game,
        })
    }

    /// The indicators laid again where they were, when they still cover the same rectangle
    /// and the notice isn't in it: that rectangle.
    fn relay_indicators(&mut self) -> Option<Rect> {
        let was = self.stack_at?;
        let s = self
            .indicators
            .draw(&self.fonts, (self.top.w, self.top.h))?;
        let (x, y) = s.place();
        let at = (x, y, s.width(), s.height());
        if at != was || self.notice_at.is_some_and(|n| overlap(n, at)) {
            return None;
        }
        let c = &mut self.top;
        for row in y..y + at.3 {
            c.px[(row * c.w + x) * 4..(row * c.w + x + at.2) * 4].fill(0);
        }
        s.lay(c);
        Some(at)
    }

    fn overlay(&mut self, m: &Model) -> Option<&[u8]> {
        self.stack_at = None;
        self.notice_at = None;
        if !self.menu {
            // In play: the notice, when one is up, at the bottom right; the achievement
            // indicators at the bottom left.
            if self.notice_drawn.is_none() && self.indicators_shown == 0 {
                return None;
            }
            let c = &mut self.top;
            c.px.fill(0);
            if let Some(t) = self.notices.draw(&self.fonts, c.scale) {
                toast::lay(c, t);
                let (x, y) = t.place((c.w, c.h));
                self.notice_at = Some((x, y, t.width(), t.height()));
            }
            if let Some(s) = self.indicators.draw(&self.fonts, (c.w, c.h)) {
                s.lay(c);
                let (x, y) = s.place();
                self.stack_at = Some((x, y, s.width(), s.height()));
            }
            return Some(&c.px);
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
        if self.page == Page::States {
            let slot = slot_at(m, self.card);
            let info = m.slots.get(slot).cloned().unwrap_or_default();
            let name = if slot == 0 {
                "Resume point".to_owned()
            } else {
                format!("Slot {slot}")
            };
            let under = if slot != 0 && slot == m.quick_slot {
                "Quick slot"
            } else {
                ""
            };
            let played = info.played.trim_end_matches(" played");
            let rows: &[(&str, &str)] = if info.picture.is_some() {
                &[("Saved", info.made.as_str()), ("Played", played)]
            } else {
                &[("Empty", "")]
            };
            let width = |s: &str, z, b| f.width(s, z, b);
            let draws =
                top::state_measured(&name, under, rows, info.picture.as_ref().map(|_| 0), width);
            let picture = info.picture.as_deref();
            c.draw(f, &draws, |_| picture.map(|p| (p, 160, 144)));
            return Some(&c.px);
        }
        if look_row {
            return None;
        }
        // The game dimmed, its name and today's play time over it.
        let sub = if m.played.is_empty() {
            m.title.clone()
        } else {
            format!("{} \u{b7} {}", m.title, m.played)
        };
        let draws = top::paused_measured("Paused", &sub, |s, z, b| f.width(s, z, b));
        c.draw(f, &draws, |_| None);
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
        let effect = effect?;
        // On Connections a press turns a radio on or off, or connects a device: nibkit's.
        if let (Page::Connections, Effect::Choose(row) | Effect::Change { row, .. }) =
            (self.page, effect)
        {
            if let Some(c) = self.conn.as_mut() {
                c.act(row);
            }
            self.touch_up();
            return None;
        }
        match effect {
            Effect::Page(i) => {
                let page = *self.pages().get(i)?;
                if page == Page::States {
                    // Opened on the first numbered slot, past the resume point.
                    self.card = place_of(m, 1);
                }
                self.go(page);
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
            Some(Top::Game),
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
            matches!(ui.render_top(&m), Some(Top::Whole(_))),
            "Paused over the game"
        );
        assert_eq!(ui.render_top(&m), None, "drawn once");
    }

    #[test]
    fn the_indicators_sit_bottom_left_and_only_the_ring_goes_again() {
        let mut ui = Ui::new();
        let m = model();
        let badge = [200u8, 90, 40, 255].repeat(16 * 16);
        ui.progress(1, toast::Notice::progress("Collector", "3/10"));
        ui.challenge(
            2,
            toast::Notice::challenge("No Damage").badge(badge, 16, 16),
        );
        let Some(Top::Whole(px)) = ui.render_top(&m) else {
            panic!("drawn whole first");
        };
        // Bottom left, the margin in: the panel there, the bottom right clear.
        let at = |x: usize, y: usize| px[(y * 1024 + x) * 4 + 3];
        assert_ne!(at(16 + 30, 768 - 16 - 30), 0);
        assert_eq!(at(1024 - 16 - 30, 768 - 16 - 30), 0);
        let (x, y, w, h) = ui.stack_at.unwrap();
        assert_eq!((x, y + h), (16, 768 - 16));
        // A step of the ring later: only the indicators' rectangle, the rest kept.
        std::thread::sleep(std::time::Duration::from_millis(80));
        let Some(Top::Part(_, part)) = ui.render_top(&m) else {
            panic!("the ring alone");
        };
        assert_eq!(part, (x, y, w, h));
        // Over: the game as it is.
        ui.hide_progress();
        ui.challenge_over(2);
        assert_eq!(ui.render_top(&m), Some(Top::Game));
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
            matches!(ui.render_top(&m), Some(Top::Whole(_))),
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
        assert_eq!(ui.render_top(&m), Some(Top::Game));
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

    /// Connections is a tab only where there are radios to show (nibOS on the handheld); on a
    /// computer, or the stock OS, the tabs stop at System.
    #[test]
    fn connections_only_where_there_are_radios() {
        let mut ui = Ui::new();
        let m = model();
        ui.open_menu();
        let tabs: Vec<String> = ui.tabs(&m).into_iter().map(|t| t.0).collect();
        assert_eq!(tabs.contains(&"Connections".to_owned()), ui.conn.is_some());
        if ui.conn.is_none() {
            assert_eq!(tabs.len(), 5);
            ui.nav(&m, Nav::L);
            assert_eq!(
                ui.page,
                Page::System,
                "round from Play to the last tab, System"
            );
        }
        ui.close_menu();
        assert!(ui.conn.is_none(), "its worker stops when the menu closes");
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
