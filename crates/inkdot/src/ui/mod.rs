//! The bottom screen: the emulator's controls, made for a thumb.
//!
//! | Page | |
//! |---|---|
//! | Play | while the game runs: save, load, the look, fast-forward, options, quit |
//! | States | six slots with their pictures: load one, or save into one |
//! | Look | how the game looks (DMG games: as a GBC, a Game Boy, a Pocket, a Light, grey) |
//! | Options | timing, the bottom screen in play, resume, fast-forward speed; brightness, volume |
//!
//! Every page is a list of widgets with their places; the same list draws the page, finds what a
//! finger touched and moves the focus for the D-pad (when the menu is opened with Back, which
//! pauses the game). A widget does one [`Action`], which the main loop carries out and answers
//! by updating the [`Model`].

pub mod draw;

use draw::{colour, Align, Canvas, Rect, Rgb, BODY, LABEL, SMALL, TITLE};

/// The screen's size.
pub const W: i32 = 1024;
pub const H: i32 = 768;

/// What the interface asks the emulator to do.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    /// Save the game to slot n (1-6), or the quick slot (0).
    Save(usize),
    Load(usize),
    /// Set an option (its key in the description) to a value, for this game or for its console.
    Set {
        key: String,
        value: nibkit::settings::Value,
        for_game: bool,
    },
    /// Clear the options set for this game, or for its console.
    Reset {
        for_game: bool,
    },
    ToggleFast,
    Brightness {
        top: bool,
        percent: u8,
    },
    Volume(u8),
    /// Close the menu and play.
    Resume,
    Quit,
}

/// A save state slot as the States page shows it.
#[derive(Clone, Debug, Default)]
pub struct SlotInfo {
    /// 160×144 RGBA.
    pub picture: Option<Vec<u8>>,
    pub made: String,
    pub played: String,
}

/// An option row: its key, label, value shown, where it comes from, whether set for the game.
#[derive(Clone, Debug)]
pub struct OptionRow {
    pub key: String,
    pub label: String,
    pub shown: String,
    pub source: String,
    /// The values in order, for the row's left and right arrows.
    pub values: Vec<(nibkit::settings::Value, String)>,
    pub current: nibkit::settings::Value,
}

/// A look as the Look page shows it.
#[derive(Clone, Debug)]
pub struct LookTile {
    pub value: String,
    pub label: String,
    pub swatch: [Rgb; 4],
}

/// What the interface shows: the main loop keeps it up to date.
#[derive(Clone, Debug, Default)]
pub struct Model {
    pub title: String,
    pub clock: String,
    pub battery: Option<u8>,
    pub slots: Vec<SlotInfo>,
    pub quick_slot: usize,
    pub looks: Vec<LookTile>,
    pub look: String,
    pub options: Vec<OptionRow>,
    pub fast: bool,
    pub top_brightness: Option<u8>,
    pub bottom_brightness: Option<u8>,
    pub volume: Option<u8>,
    /// Options are changed for the game (true) or for its console.
    pub for_game: bool,
    pub console: String,
    pub toast: Option<String>,
}

/// The pages.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Play,
    States,
    Look,
    Options,
}

#[derive(Clone, Debug)]
enum Kind {
    Tile {
        label: &'static str,
        sub: String,
        icon: Icon,
    },
    Back,
    Tab {
        label: String,
        on: bool,
    },
    Slot(usize),
    Look(usize),
    Row(usize),
    Slider {
        label: String,
        value: u8,
    },
}

#[derive(Clone, Copy, Debug)]
enum Icon {
    Save,
    Load,
    Look,
    Fast,
    Options,
    Quit,
}

#[derive(Clone, Debug)]
struct Widget {
    rect: Rect,
    kind: Kind,
    action: Option<Action>,
}

/// The interface's state.
pub struct Ui {
    pub page: Page,
    /// Opened with Back: the game is paused and the D-pad moves the focus.
    pub menu: bool,
    states_saving: bool,
    /// Saving into a slot that holds a state: tapped once, waiting for a second tap to replace it.
    confirm: Option<usize>,
    focus: usize,
    canvas: Canvas,
    dirty: bool,
}

const HEADER: i32 = 88;

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
            states_saving: false,
            confirm: None,
            focus: 0,
            canvas: Canvas::new(W, H),
            dirty: true,
        }
    }

    /// Something shown changed: draw again.
    pub fn touch_up(&mut self) {
        self.dirty = true;
    }

    pub fn go(&mut self, page: Page) {
        self.page = page;
        self.confirm = None;
        self.focus = 0;
        self.dirty = true;
    }

    /// Opens the menu (Back pressed): paused, the focus shown.
    pub fn open_menu(&mut self) {
        self.menu = true;
        self.go(Page::Play);
    }

    pub fn close_menu(&mut self) {
        self.menu = false;
        self.go(Page::Play);
    }

    fn widgets(&self, m: &Model) -> Vec<Widget> {
        let mut w = Vec::new();
        if self.page != Page::Play {
            w.push(Widget {
                rect: Rect::new(16, 12, 150, 64),
                kind: Kind::Back,
                action: None,
            });
        }
        match self.page {
            Page::Play => {
                let (x0, y0, tw, th, gap) = (32, HEADER + 24, 304, 290, 24);
                let tiles: [(&'static str, String, Icon, Option<Action>); 6] = [
                    ("Save", "Pick a slot".into(), Icon::Save, None),
                    ("Load", "Pick a slot".into(), Icon::Load, None),
                    (
                        "Look",
                        m.looks
                            .iter()
                            .find(|l| l.value == m.look)
                            .map(|l| l.label.clone())
                            .unwrap_or_default(),
                        Icon::Look,
                        None,
                    ),
                    (
                        "Fast",
                        if m.fast { "On".into() } else { "Off".into() },
                        Icon::Fast,
                        Some(Action::ToggleFast),
                    ),
                    ("Options", String::new(), Icon::Options, None),
                    ("Quit", "Saves first".into(), Icon::Quit, Some(Action::Quit)),
                ];
                for (i, (label, sub, icon, action)) in tiles.into_iter().enumerate() {
                    let (col, row) = (i as i32 % 3, i as i32 / 3);
                    let rect = Rect::new(x0 + col * (tw + gap), y0 + row * (th + gap), tw, th);
                    w.push(Widget {
                        rect,
                        kind: Kind::Tile { label, sub, icon },
                        action,
                    });
                }
            }
            Page::States => {
                for (i, (label, on)) in
                    [("Load", !self.states_saving), ("Save", self.states_saving)]
                        .into_iter()
                        .enumerate()
                {
                    let rect = Rect::new(W - 32 - 2 * 180 + i as i32 * 180, 12, 172, 64);
                    w.push(Widget {
                        rect,
                        kind: Kind::Tab {
                            label: label.into(),
                            on,
                        },
                        action: None,
                    });
                }
                let (tw, th, gap_x) = (280, 252, 40);
                let x0 = (W - 3 * tw - 2 * gap_x) / 2;
                for i in 0..6 {
                    let (col, row) = (i as i32 % 3, i as i32 / 3);
                    let rect = Rect::new(
                        x0 + col * (tw + gap_x),
                        HEADER + 20 + row * (th + 88),
                        tw,
                        th + 64,
                    );
                    let slot = i + 1;
                    let empty = m.slots.get(slot).is_none_or(|s| s.picture.is_none());
                    let action = if self.states_saving {
                        Some(Action::Save(slot))
                    } else if empty {
                        None
                    } else {
                        Some(Action::Load(slot))
                    };
                    w.push(Widget {
                        rect,
                        kind: Kind::Slot(slot),
                        action,
                    });
                }
            }
            Page::Look => {
                for (i, (label, on)) in
                    [("This game", m.for_game), (m.console.as_str(), !m.for_game)]
                        .into_iter()
                        .enumerate()
                {
                    let rect = Rect::new(W - 32 - 2 * 200 + i as i32 * 200, 12, 192, 64);
                    let label = if i == 1 {
                        format!("All {label}")
                    } else {
                        label.to_owned()
                    };
                    w.push(Widget {
                        rect,
                        kind: Kind::Tab { label, on },
                        action: None,
                    });
                }
                w.push(Widget {
                    rect: Rect::new(W - 32 - 2 * 200 - 160, 12, 148, 64),
                    kind: Kind::Tab {
                        label: "Reset".into(),
                        on: false,
                    },
                    action: Some(Action::Reset {
                        for_game: m.for_game,
                    }),
                });
                let n = m.looks.len().max(1) as i32;
                let gap = 16;
                let tw = (W - 64 - (n - 1) * gap) / n;
                for i in 0..m.looks.len() {
                    let rect = Rect::new(32 + i as i32 * (tw + gap), HEADER + 24, tw, 260);
                    let action = Some(Action::Set {
                        key: "look".into(),
                        value: m.looks[i].value.as_str().into(),
                        for_game: m.for_game,
                    });
                    w.push(Widget {
                        rect,
                        kind: Kind::Look(i),
                        action,
                    });
                }
                let rows: Vec<usize> = (0..m.options.len())
                    .filter(|&i| matches!(m.options[i].key.as_str(), "lcd" | "palette" | "colors"))
                    .collect();
                for (k, i) in rows.into_iter().enumerate() {
                    let rect = Rect::new(32, HEADER + 312 + k as i32 * 96, W - 64, 84);
                    w.push(Widget {
                        rect,
                        kind: Kind::Row(i),
                        action: None,
                    });
                }
            }
            Page::Options => {
                for (i, (label, on)) in
                    [("This game", m.for_game), (m.console.as_str(), !m.for_game)]
                        .into_iter()
                        .enumerate()
                {
                    let rect = Rect::new(W - 32 - 2 * 200 + i as i32 * 200, 12, 192, 64);
                    let label = if i == 1 {
                        format!("All {label}")
                    } else {
                        label.to_owned()
                    };
                    w.push(Widget {
                        rect,
                        kind: Kind::Tab { label, on },
                        action: None,
                    });
                }
                w.push(Widget {
                    rect: Rect::new(W - 32 - 2 * 200 - 160, 12, 148, 64),
                    kind: Kind::Tab {
                        label: "Reset".into(),
                        on: false,
                    },
                    action: Some(Action::Reset {
                        for_game: m.for_game,
                    }),
                });
                let rows: Vec<usize> = (0..m.options.len())
                    .filter(|&i| {
                        !matches!(
                            m.options[i].key.as_str(),
                            "lcd" | "palette" | "colors" | "look"
                        )
                    })
                    .collect();
                let mut y = HEADER + 16;
                for i in rows {
                    w.push(Widget {
                        rect: Rect::new(32, y, W - 64, 76),
                        kind: Kind::Row(i),
                        action: None,
                    });
                    y += 84;
                }
                for (label, value) in [
                    ("Top screen", m.top_brightness),
                    ("Bottom screen", m.bottom_brightness),
                    ("Volume", m.volume),
                ] {
                    if let Some(v) = value {
                        w.push(Widget {
                            rect: Rect::new(32, y, W - 64, 76),
                            kind: Kind::Slider {
                                label: label.into(),
                                value: v,
                            },
                            action: None,
                        });
                        y += 84;
                    }
                }
            }
        }
        w
    }

    /// A finger came down at `(x, y)` (screen pixels). Returns the action it asks for.
    pub fn tap(&mut self, m: &Model, x: i32, y: i32) -> Option<Action> {
        let widgets = self.widgets(m);
        let (i, w) = widgets
            .iter()
            .enumerate()
            .find(|(_, w)| w.rect.contains(x, y))?;
        self.focus = i;
        self.dirty = true;
        self.activate(m, w, Some((x, y)))
    }

    /// A finger moved to `(x, y)`: a slider under it follows; nothing else reacts.
    pub fn drag(&mut self, m: &Model, x: i32, y: i32) -> Option<Action> {
        let widgets = self.widgets(m);
        let w = widgets
            .iter()
            .find(|w| w.rect.contains(x, y) && matches!(w.kind, Kind::Slider { .. }))?;
        self.dirty = true;
        self.activate(m, &w.clone(), Some((x, y)))
    }

    fn activate(&mut self, m: &Model, w: &Widget, at: Option<(i32, i32)>) -> Option<Action> {
        match &w.kind {
            Kind::Back => {
                self.go(Page::Play);
                None
            }
            Kind::Tile { label: "Load", .. } => {
                self.states_saving = false;
                self.go(Page::States);
                None
            }
            Kind::Tile { label: "Save", .. } => {
                self.states_saving = true;
                self.go(Page::States);
                None
            }
            // Saving over a state takes a second tap on the same slot.
            Kind::Slot(n) if self.states_saving => {
                let used = m.slots.get(*n).is_some_and(|s| s.picture.is_some());
                if used && self.confirm != Some(*n) {
                    self.confirm = Some(*n);
                    return None;
                }
                self.confirm = None;
                w.action.clone()
            }
            Kind::Tile { label: "Look", .. } => {
                self.go(Page::Look);
                None
            }
            Kind::Tile {
                label: "Options", ..
            } => {
                self.go(Page::Options);
                None
            }
            Kind::Tab { label, .. } if label == "Reset" => w.action.clone(),
            Kind::Tab { label, .. } => {
                if label == "Load" || label == "Save" {
                    self.states_saving = label == "Save";
                    self.confirm = None;
                    self.dirty = true;
                    None
                } else {
                    // The game / console switch: the main loop keeps it in the model.
                    Some(Action::Set {
                        key: "@scope".into(),
                        value: (label == "This game").into(),
                        for_game: true,
                    })
                }
            }
            Kind::Row(i) => {
                let row = &m.options[*i];
                let dir = match at {
                    Some((x, _)) if x < w.rect.x + w.rect.w / 2 => -1,
                    _ => 1,
                };
                let at = row
                    .values
                    .iter()
                    .position(|(v, _)| *v == row.current)
                    .unwrap_or(0) as i32;
                let next = (at + dir).rem_euclid(row.values.len().max(1) as i32) as usize;
                let value = row.values.get(next)?.0.clone();
                Some(Action::Set {
                    key: row.key.clone(),
                    value,
                    for_game: m.for_game,
                })
            }
            Kind::Slider { label, value } => {
                let percent = match at {
                    Some((x, _)) => {
                        let track = Self::slider_track(w.rect);
                        (((x - track.x) * 100) / track.w).clamp(0, 100) as u8
                    }
                    None => value.saturating_add(10).min(100),
                };
                Some(match label.as_str() {
                    "Top screen" => Action::Brightness { top: true, percent },
                    "Bottom screen" => Action::Brightness {
                        top: false,
                        percent,
                    },
                    _ => Action::Volume(percent),
                })
            }
            _ => w.action.clone(),
        }
    }

    /// The D-pad and A/B while the menu is open.
    pub fn nav(&mut self, m: &Model, key: Nav) -> Option<Action> {
        let widgets = self.widgets(m);
        if widgets.is_empty() {
            return None;
        }
        self.focus = self.focus.min(widgets.len() - 1);
        self.dirty = true;
        let here = widgets[self.focus].rect.centre();
        let pick = |dx: i32, dy: i32| {
            widgets
                .iter()
                .enumerate()
                .filter(|(i, w)| {
                    let (x, y) = w.rect.centre();
                    *i != self.focus && (x - here.0) * dx + (y - here.1) * dy > 0
                })
                .min_by_key(|(_, w)| {
                    let (x, y) = w.rect.centre();
                    let along = (x - here.0) * dx + (y - here.1) * dy;
                    let across = ((x - here.0) * dy).abs() + ((y - here.1) * dx).abs();
                    along + 3 * across
                })
                .map(|(i, _)| i)
        };
        match key {
            Nav::Up | Nav::Down | Nav::Left | Nav::Right => {
                // On a row or slider, left and right change it.
                let w = &widgets[self.focus];
                if matches!(key, Nav::Left | Nav::Right)
                    && matches!(w.kind, Kind::Row(_) | Kind::Slider { .. })
                {
                    let x = if key == Nav::Left {
                        w.rect.x + 1
                    } else {
                        w.rect.x + w.rect.w - 1
                    };
                    if let Kind::Slider { value, .. } = w.kind {
                        let step = if key == Nav::Left {
                            value.saturating_sub(5)
                        } else {
                            value.saturating_add(5).min(100)
                        };
                        let track = Self::slider_track(w.rect);
                        let x = track.x + track.w * step as i32 / 100;
                        return self.activate(m, &w.clone(), Some((x, 0)));
                    }
                    return self.activate(m, &w.clone(), Some((x, 0)));
                }
                let (dx, dy) = match key {
                    Nav::Up => (0, -1),
                    Nav::Down => (0, 1),
                    Nav::Left => (-1, 0),
                    _ => (1, 0),
                };
                if let Some(i) = pick(dx, dy) {
                    self.focus = i;
                }
                None
            }
            Nav::A => {
                let w = widgets[self.focus].clone();
                self.activate(m, &w, None)
            }
            Nav::B => {
                if self.page == Page::Play {
                    Some(Action::Resume)
                } else {
                    self.go(Page::Play);
                    None
                }
            }
        }
    }

    fn slider_track(r: Rect) -> Rect {
        Rect::new(r.x + 300, r.y + r.h / 2 - 6, r.w - 300 - 120, 12)
    }

    /// The picture, when it has changed since last asked.
    pub fn render(&mut self, m: &Model) -> Option<&[u8]> {
        if !self.dirty {
            return None;
        }
        self.dirty = false;
        let widgets = self.widgets(m);
        let c = &mut self.canvas;
        c.clear(colour::BACKGROUND);
        // The header: the page's title or the game's, the time and the battery.
        let title = match self.page {
            Page::Play => m.title.clone(),
            Page::States if self.states_saving => "Save to a slot".into(),
            Page::States => "Load a slot".into(),
            Page::Look => "Look".into(),
            Page::Options => "Options".into(),
        };
        let title_x = if self.page == Page::Play { 32 } else { 190 };
        let title_w = if self.page == Page::Play {
            W - 32 - 240
        } else {
            380
        };
        c.text(
            Rect::new(title_x, 12, title_w, 64),
            &title,
            TITLE,
            colour::TEXT,
            Align::Left,
        );
        if self.page == Page::Play {
            let battery = m.battery.map(|b| format!("  {b}%")).unwrap_or_default();
            c.text(
                Rect::new(W - 32 - 220, 12, 220, 64),
                &format!("{}{battery}", m.clock),
                BODY,
                colour::DIM,
                Align::Right,
            );
        }
        c.fill(Rect::new(0, HEADER, W, 2), colour::LINE);
        for (i, w) in widgets.iter().enumerate() {
            let focused = self.menu && i == self.focus;
            draw_widget(c, m, w, focused);
            if matches!(w.kind, Kind::Slot(n) if self.confirm == Some(n)) {
                let r = Rect::new(w.rect.x, w.rect.y + w.rect.h - 64 - 72, w.rect.w, 72);
                c.round(r, 12, colour::INK);
                c.text(
                    r,
                    "Tap again to replace",
                    BODY,
                    colour::INK_DARK,
                    Align::Centre,
                );
            }
        }
        if let Some(t) = &m.toast {
            let r = Rect::new(W / 2 - 260, H - 104, 520, 76);
            c.round(r, 38, colour::INK);
            c.text(r, t, LABEL, colour::INK_DARK, Align::Centre);
        }
        if self.menu && self.page == Page::Play {
            c.text(
                Rect::new(32, H - 40, W - 64, 32),
                "Paused · B or Back to play",
                SMALL,
                colour::DIM,
                Align::Centre,
            );
        }
        Some(&self.canvas.px)
    }
}

/// The D-pad and the A and B buttons, as the menu uses them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Nav {
    Up,
    Down,
    Left,
    Right,
    A,
    B,
}

fn draw_widget(c: &mut Canvas, m: &Model, w: &Widget, focused: bool) {
    let r = w.rect;
    match &w.kind {
        Kind::Back => {
            c.round(r, 32, colour::PANEL_LIT);
            c.triangle(
                [
                    (r.x as f32 + 34.0, r.y as f32 + 32.0),
                    (r.x as f32 + 52.0, r.y as f32 + 18.0),
                    (r.x as f32 + 52.0, r.y as f32 + 46.0),
                ],
                colour::TEXT,
            );
            c.text(
                Rect::new(r.x + 60, r.y, r.w - 70, r.h),
                "Back",
                LABEL,
                colour::TEXT,
                Align::Left,
            );
        }
        Kind::Tab { label, on } => {
            c.round(r, 32, if *on { colour::INK } else { colour::PANEL });
            c.text(
                r,
                label,
                BODY,
                if *on { colour::INK_DARK } else { colour::TEXT },
                Align::Centre,
            );
        }
        Kind::Tile { label, sub, icon } => {
            c.round(r, 28, colour::PANEL);
            draw_icon(c, *icon, (r.x + r.w / 2) as f32, (r.y + 112) as f32, m.fast);
            c.text(
                Rect::new(r.x, r.y + r.h - 108, r.w, 48),
                label,
                LABEL,
                colour::TEXT,
                Align::Centre,
            );
            c.text(
                Rect::new(r.x + 12, r.y + r.h - 62, r.w - 24, 40),
                sub,
                SMALL,
                colour::DIM,
                Align::Centre,
            );
        }
        Kind::Slot(n) => {
            let info = m.slots.get(*n).cloned().unwrap_or_default();
            let pic = Rect::new(r.x, r.y, r.w, r.h - 64);
            c.round(
                pic.inset(-4),
                12,
                if *n == m.quick_slot {
                    colour::INK
                } else {
                    colour::PANEL
                },
            );
            match &info.picture {
                Some(p) => c.picture_fit(pic, p, 160, 144),
                None => {
                    c.fill(pic, colour::PANEL);
                    c.text(pic, "Empty", BODY, colour::DIM, Align::Centre);
                }
            }
            let label = if info.made.is_empty() {
                format!("Slot {n}")
            } else {
                format!("{n} · {}", info.made)
            };
            c.text(
                Rect::new(r.x, r.y + r.h - 60, r.w, 32),
                &label,
                BODY,
                colour::TEXT,
                Align::Left,
            );
            c.text(
                Rect::new(r.x, r.y + r.h - 30, r.w, 28),
                &info.played,
                SMALL,
                colour::DIM,
                Align::Left,
            );
        }
        Kind::Look(i) => {
            let look = &m.looks[*i];
            let on = look.value == m.look;
            c.round(r, 24, if on { colour::INK } else { colour::PANEL });
            let inner = Rect::new(r.x + 16, r.y + 16, r.w - 32, 150);
            for (k, s) in look.swatch.iter().enumerate() {
                let sw = inner.w / 4;
                c.fill(Rect::new(inner.x + k as i32 * sw, inner.y, sw, inner.h), *s);
            }
            c.text(
                Rect::new(r.x + 8, r.y + 176, r.w - 16, 72),
                &look.label,
                BODY,
                if on { colour::INK_DARK } else { colour::TEXT },
                Align::Centre,
            );
        }
        Kind::Row(i) => {
            let row = &m.options[*i];
            c.round(r, 20, colour::PANEL);
            c.text(
                Rect::new(r.x + 24, r.y, 420, r.h),
                &row.label,
                BODY,
                colour::TEXT,
                Align::Left,
            );
            c.text(
                Rect::new(r.x + 450, r.y, 340, r.h),
                &format!("‹  {}  ›", row.shown),
                LABEL,
                colour::INK,
                Align::Centre,
            );
            c.text(
                Rect::new(r.x + r.w - 190, r.y, 170, r.h),
                &row.source,
                SMALL,
                colour::DIM,
                Align::Right,
            );
        }
        Kind::Slider { label, value } => {
            c.round(r, 20, colour::PANEL);
            c.text(
                Rect::new(r.x + 24, r.y, 260, r.h),
                label,
                BODY,
                colour::TEXT,
                Align::Left,
            );
            let t = Ui::slider_track(r);
            c.round(t, 6, colour::LINE);
            c.round(
                Rect::new(t.x, t.y, t.w * *value as i32 / 100, t.h),
                6,
                colour::INK,
            );
            c.disc(
                (t.x + t.w * *value as i32 / 100) as f32,
                (t.y + t.h / 2) as f32,
                18.0,
                colour::TEXT,
            );
            c.text(
                Rect::new(r.x + r.w - 110, r.y, 90, r.h),
                &format!("{value}%"),
                BODY,
                colour::DIM,
                Align::Right,
            );
        }
    }
    if focused {
        c.ring(r.inset(-6), 30, 4, colour::INK);
    }
}

fn draw_icon(c: &mut Canvas, icon: Icon, x: f32, y: f32, fast: bool) {
    let t = colour::TEXT;
    let ink = colour::INK;
    match icon {
        Icon::Save | Icon::Load => {
            // A tray, with an arrow into it (save) or out of it (load).
            c.round(Rect::new(x as i32 - 56, y as i32 + 22, 112, 14), 7, t);
            c.round(Rect::new(x as i32 - 56, y as i32 - 4, 14, 40), 7, t);
            c.round(Rect::new(x as i32 + 42, y as i32 - 4, 14, 40), 7, t);
            if matches!(icon, Icon::Save) {
                c.fill(Rect::new(x as i32 - 7, y as i32 - 58, 14, 36), ink);
                c.triangle(
                    [(x - 28.0, y - 24.0), (x + 28.0, y - 24.0), (x, y + 8.0)],
                    ink,
                );
            } else {
                c.fill(Rect::new(x as i32 - 7, y as i32 - 30, 14, 40), ink);
                c.triangle(
                    [(x - 28.0, y - 28.0), (x + 28.0, y - 28.0), (x, y - 60.0)],
                    ink,
                );
            }
        }
        Icon::Look => {
            c.disc(x, y, 50.0, t);
            c.disc(x, y, 42.0, colour::PANEL);
            for (k, col) in [
                [0x9b, 0xbc, 0x0f],
                [0x30, 0x62, 0x30],
                [0xe0, 0x5a, 0x3a],
                [0x4a, 0x7a, 0xe0],
            ]
            .iter()
            .enumerate()
            {
                let a = k as f32 * std::f32::consts::FRAC_PI_2 + 0.6;
                c.disc(x + a.cos() * 22.0, y + a.sin() * 22.0, 13.0, *col);
            }
        }
        Icon::Fast => {
            let col = if fast { ink } else { t };
            c.triangle(
                [(x - 54.0, y - 36.0), (x - 54.0, y + 36.0), (x - 4.0, y)],
                col,
            );
            c.triangle(
                [(x + 2.0, y - 36.0), (x + 2.0, y + 36.0), (x + 52.0, y)],
                col,
            );
        }
        Icon::Options => {
            for k in 0..3 {
                let yy = y - 34.0 + k as f32 * 34.0;
                c.round(Rect::new(x as i32 - 56, yy as i32 - 4, 112, 8), 4, t);
                c.disc(x - 30.0 + k as f32 * 28.0, yy, 13.0, ink);
            }
        }
        Icon::Quit => {
            c.disc(x, y, 46.0, t);
            c.disc(x, y, 37.0, colour::PANEL);
            c.fill(
                Rect::new(x as i32 - 14, y as i32 - 60, 28, 40),
                colour::PANEL,
            );
            c.round(Rect::new(x as i32 - 6, y as i32 - 56, 12, 50), 6, ink);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model() -> Model {
        Model {
            title: "TEST".into(),
            slots: vec![SlotInfo::default(); 7],
            quick_slot: 1,
            console: "GB".into(),
            for_game: true,
            options: vec![OptionRow {
                key: "speed".into(),
                label: "Timing".into(),
                shown: "Smooth".into(),
                source: "default".into(),
                values: vec![
                    ("panel".into(), "Smooth".into()),
                    ("exact".into(), "Exact".into()),
                ],
                current: "panel".into(),
            }],
            ..Model::default()
        }
    }

    /// Save opens the six slots to choose from; an empty one saves at once, a used one only on
    /// a second tap.
    #[test]
    fn save_picks_a_slot_and_asks_before_replacing() {
        let mut ui = Ui::new();
        let mut m = model();
        m.slots[2].picture = Some(vec![0; 160 * 144 * 4]);
        // The first tile, top left, is Save.
        assert_eq!(ui.tap(&m, 100, 200), None);
        assert_eq!((ui.page, ui.states_saving), (Page::States, true));
        // Slot 1 (top left) is empty: saved at once.
        assert_eq!(ui.tap(&m, 150, 200), Some(Action::Save(1)));
        // Slot 2 holds a state: the first tap asks, the second saves.
        assert_eq!(ui.tap(&m, 500, 200), None);
        assert_eq!(ui.confirm, Some(2));
        assert!(ui.render(&m).is_some());
        assert_eq!(ui.tap(&m, 500, 200), Some(Action::Save(2)));
        // A tap elsewhere in between starts the asking again.
        ui.tap(&m, 500, 200);
        ui.tap(&m, 150, 200);
        assert_eq!(ui.tap(&m, 500, 200), None);
    }

    #[test]
    fn the_play_tiles_do_what_they_say() {
        let mut ui = Ui::new();
        let m = model();
        // Quit is bottom right.
        assert_eq!(ui.tap(&m, 900, 600), Some(Action::Quit));
        // Options opens its page.
        assert_eq!(ui.tap(&m, 512, 600), None);
        assert_eq!(ui.page, Page::Options);
    }

    #[test]
    fn a_row_steps_its_value_both_ways() {
        let mut ui = Ui::new();
        ui.go(Page::Options);
        let m = model();
        let right = ui.tap(&m, 900, HEADER + 40);
        assert_eq!(
            right,
            Some(Action::Set {
                key: "speed".into(),
                value: "exact".into(),
                for_game: true
            })
        );
        let left = ui.tap(&m, 100, HEADER + 40);
        assert_eq!(
            left,
            Some(Action::Set {
                key: "speed".into(),
                value: "exact".into(),
                for_game: true
            })
        );
    }

    #[test]
    fn the_dpad_moves_between_tiles_and_b_resumes() {
        let mut ui = Ui::new();
        ui.open_menu();
        let m = model();
        ui.nav(&m, Nav::Right);
        ui.nav(&m, Nav::Down);
        // Middle bottom: Options.
        assert_eq!(ui.nav(&m, Nav::A), None);
        assert_eq!(ui.page, Page::Options);
        ui.nav(&m, Nav::B);
        assert_eq!(ui.nav(&m, Nav::B), Some(Action::Resume));
    }

    #[test]
    fn it_draws_only_when_something_changed() {
        let mut ui = Ui::new();
        let m = model();
        assert!(ui.render(&m).is_some());
        assert!(ui.render(&m).is_none());
        ui.touch_up();
        assert!(ui.render(&m).is_some());
    }
}
