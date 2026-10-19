//! inkdot: a Game Boy and Game Boy Color emulator for the Anbernic RG DS+ on nibOS, started by
//! DSlot. The game is on the top screen; the bottom touch screen has the emulator's controls.
//! `inkdot --help` lists the options; docs/using.md says how to play.

mod args;
mod input;
mod looks;
mod options;
mod pacing;
mod rom;
mod saves;
mod ui;
mod video;

use inkdot_core::{GameBoy, Model as Console};
use input::{Command, Controls, Pad};
use looks::Look;
use nibkit::screens::Screen;
use nibkit::settings::Settings;
use options::Options;
use pacing::{Pacer, RateControl, GAME_HZ};
use saves::{SaveFiles, Slot, Writer, SLOTS};
use sdl2::audio::{AudioQueue, AudioSpecDesired};
use sdl2::controller::{Axis, Button, GameController};
use sdl2::event::Event;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use ui::{Action, LookTile, Nav, OptionRow, SlotInfo, Ui};
use video::{LcdLook, Video};

/// Set by SIGTERM: DSlot (or systemd) asks the game to be put away.
static TERM: AtomicBool = AtomicBool::new(false);

extern "C" fn on_term(_: libc::c_int) {
    TERM.store(true, Ordering::SeqCst);
}

fn watch_sigterm() {
    // SAFETY: a handler that only stores to an atomic.
    unsafe {
        let mut action: libc::sigaction = std::mem::zeroed();
        action.sa_sigaction = on_term as extern "C" fn(libc::c_int) as libc::sighandler_t;
        action.sa_flags = libc::SA_RESTART;
        libc::sigemptyset(&mut action.sa_mask);
        libc::sigaction(libc::SIGTERM, &action, std::ptr::null_mut());
    }
}

/// The audio queue's target, in sample frames: about a frame and a half.
const AUDIO_TARGET: u32 = 1200;
/// The bottom screen dims (or goes off) after this long untouched in play.
const IDLE: Duration = Duration::from_secs(10);

/// What the bottom screen's backlight is doing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Backlight {
    Lit,
    /// Dimmed to the floor; the percentage to go back to.
    Dim(u8),
    Off,
}

struct App {
    gb: GameBoy,
    options: Options,
    console: &'static str,
    title: String,
    files: SaveFiles,
    writer: Writer,
    video: Video,
    audio: Option<AudioQueue<i16>>,
    controls: Controls,
    ui: Ui,
    model: ui::Model,
    pacer: Pacer,
    rate: RateControl,
    samples: Vec<i16>,
    rgba: Vec<u8>,
    colors: Vec<[u8; 4]>,
    look: Look,
    fast: bool,
    battery_dirty: Option<Instant>,
    played_before: u64,
    started: Instant,
    quick_slot: usize,
    last_touch: Instant,
    backlight: Backlight,
    toast_until: Option<Instant>,
    quit: bool,
    /// Exact-speed timing: when the run started and how many frames it has made.
    exact_start: Option<(Instant, u64)>,
}

fn main() {
    let words: Vec<String> = std::env::args().skip(1).collect();
    if words.iter().any(|w| w == "--help" || w == "-h") {
        print!("{}", args::usage());
        return;
    }
    let args = match args::parse(&words) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("inkdot: {e}\n\n{}", args::usage());
            std::process::exit(2);
        }
    };
    if let Err(e) = run(args) {
        eprintln!("inkdot: {e}");
        std::process::exit(1);
    }
}

fn run(args: args::Args) -> Result<(), String> {
    watch_sigterm();
    let (rom, _) = rom::load(&args.rom)?;
    let stem = args
        .rom
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("game")
        .to_owned();
    let header = inkdot_core::Header::parse(rom.get(..0x150).ok_or("not a Game Boy ROM")?);
    let console = if header.supports_cgb() { "GBC" } else { "GB" };
    let settings_file = nibkit::paths::AppDirs::new("inkdot")
        .config
        .join("settings.toml");
    let options = Options::new(
        console,
        &stem,
        args.options.clone(),
        Settings::load(settings_file),
    );
    let look = if console == "GB" {
        Look::parse(&options.str("look")).unwrap_or(Look::Color)
    } else {
        Look::Color
    };
    let model = match args.model {
        args::ModelChoice::Dmg => Console::Dmg,
        args::ModelChoice::Cgb => Console::Cgb,
        args::ModelChoice::Auto if console == "GBC" || look == Look::Color => Console::Cgb,
        args::ModelChoice::Auto => Console::Dmg,
    };
    let mut gb = GameBoy::new(rom, model).map_err(|e| e.to_string())?;
    if let Some(boot) = &args.boot_rom {
        let data = std::fs::read(boot).map_err(|e| format!("{}: {e}", boot.display()))?;
        gb.hw.set_boot_rom(data);
    }
    let dir = args
        .saves
        .clone()
        .unwrap_or_else(|| args.rom.parent().map(Path::to_path_buf).unwrap_or_default());
    let files = SaveFiles::new(dir, &stem);
    if let Ok(ram) = std::fs::read(files.battery()) {
        gb.hw.cart.load_ram(&ram);
    }
    if let Ok(clock) = std::fs::read(files.clock()) {
        gb.hw.cart.rtc_load(&clock, saves::unix_now());
    }

    let sdl = sdl2::init()?;
    let video_sys = sdl.video()?;
    let video = Video::open(&video_sys)?;
    let audio = sdl.audio().ok().and_then(|a| {
        let spec = AudioSpecDesired {
            freq: Some(48_000),
            channels: Some(2),
            samples: Some(512),
        };
        let q: Result<AudioQueue<i16>, _> = a.open_queue(None, &spec);
        q.inspect_err(|e| eprintln!("inkdot: no sound: {e}")).ok()
    });
    if let Some(q) = &audio {
        q.resume();
    }
    let pads = sdl.game_controller()?;
    for file in nibkit::input::CONTROLLER_MAPPINGS {
        let _ = pads.load_mappings(file);
    }
    let _pads: Vec<GameController> = (0..pads.num_joysticks().unwrap_or(0))
        .filter_map(|i| pads.open(i).ok())
        .collect();
    let mut touch = nibkit::touch::Panel::open();

    let mut app = App {
        gb,
        options,
        console,
        title: stem.clone(),
        files,
        writer: Writer::start(),
        video,
        audio,
        controls: Controls::default(),
        ui: Ui::new(),
        model: ui::Model::default(),
        pacer: Pacer::new(nibkit::screens::REFRESH_HZ),
        rate: RateControl::new(AUDIO_TARGET),
        samples: Vec::new(),
        rgba: vec![0; 160 * 144 * 4],
        colors: Vec::new(),
        look,
        fast: false,
        battery_dirty: None,
        played_before: 0,
        started: Instant::now(),
        quick_slot: 1,
        last_touch: Instant::now(),
        backlight: Backlight::Lit,
        toast_until: None,
        quit: false,
        exact_start: None,
    };
    app.apply_look();
    // Resume where the game was left, unless told not to.
    if app.options.bool("resume") {
        if let Some(slot) = Slot::read(&app.files.state(0)) {
            match app.gb.load_state(&slot.machine) {
                Ok(()) => app.played_before = slot.played,
                Err(e) => eprintln!("inkdot: not resuming: {e}"),
            }
        }
    }
    app.refresh_model();
    let vblank = nibkit::vblank::Vblank::open().ok();
    let mut events = sdl.event_pump()?;
    let mut pointers = Vec::new();
    while !app.quit {
        // Wait for the top panel's vblank, then until the latest moment that still leaves time
        // for a frame: the buttons are read as late as they can be.
        if let Some(v) = &vblank {
            if let Ok(at) = v.wait(Screen::Top) {
                app.pacer.vblank(at);
                if let Some(start) = app.pacer.next_start() {
                    sleep_until(start);
                }
            }
        }
        let work = Instant::now();
        for event in events.poll_iter() {
            app.event(event);
        }
        if let Some(panel) = &mut touch {
            pointers.clear();
            panel.poll(&mut pointers);
            for p in pointers.drain(..) {
                match p {
                    nibkit::touch::Pointer::Down(x, y) => app.touch(x, y),
                    nibkit::touch::Pointer::Move(x, y) => app.drag(x, y),
                    nibkit::touch::Pointer::Up => {}
                }
            }
        }
        if TERM.load(Ordering::SeqCst) {
            app.quit = true;
            break;
        }
        app.frame();
        app.pacer.frame_took(work.elapsed());
    }
    app.leave();
    Ok(())
}

/// Sleeps until a CLOCK_MONOTONIC time (as the vblank timestamps are).
fn sleep_until(t: Duration) {
    let ts = libc::timespec {
        tv_sec: t.as_secs() as libc::time_t,
        tv_nsec: t.subsec_nanos() as libc::c_long,
    };
    // SAFETY: clock_nanosleep with an absolute time; an early return on a signal is harmless.
    #[cfg(target_os = "linux")]
    unsafe {
        libc::clock_nanosleep(
            libc::CLOCK_MONOTONIC,
            libc::TIMER_ABSTIME,
            &ts,
            std::ptr::null_mut(),
        );
    }
    #[cfg(not(target_os = "linux"))]
    let _ = ts;
}

fn pad_of(button: Button) -> Option<Pad> {
    use nibkit::input::{printed, Face};
    let face = |f| {
        Some(match printed(f) {
            Face::A => Pad::A,
            Face::B => Pad::B,
            Face::X => Pad::X,
            Face::Y => Pad::Y,
        })
    };
    match button {
        Button::A => face(Face::A),
        Button::B => face(Face::B),
        Button::X => face(Face::X),
        Button::Y => face(Face::Y),
        Button::Back => Some(Pad::Select),
        Button::Start => Some(Pad::Start),
        Button::DPadUp => Some(Pad::Up),
        Button::DPadDown => Some(Pad::Down),
        Button::DPadLeft => Some(Pad::Left),
        Button::DPadRight => Some(Pad::Right),
        Button::LeftShoulder => Some(Pad::L),
        Button::RightShoulder => Some(Pad::R),
        Button::Guide => Some(Pad::Menu),
        _ => None,
    }
}

fn nav_of(pad: Pad) -> Option<Nav> {
    Some(match pad {
        Pad::Up => Nav::Up,
        Pad::Down => Nav::Down,
        Pad::Left => Nav::Left,
        Pad::Right => Nav::Right,
        Pad::A => Nav::A,
        Pad::B => Nav::B,
        _ => return None,
    })
}

impl App {
    fn event(&mut self, event: Event) {
        match event {
            Event::Quit { .. } => self.quit = true,
            Event::KeyDown {
                keycode: Some(k), ..
            } => {
                let name = k.name();
                if nibkit::input::BACK_KEYCODES.contains(&name.as_str()) || name == "Escape" {
                    self.quit = true;
                }
            }
            Event::ControllerButtonDown { button, .. } => {
                let Some(pad) = pad_of(button) else { return };
                if self.ui.menu {
                    if let Some(nav) = nav_of(pad) {
                        let action = self.ui.nav(&self.model, nav);
                        self.act(action);
                    }
                    if pad == Pad::Menu {
                        self.controls.press(pad);
                    }
                    return;
                }
                let cmd = self.controls.press(pad);
                self.command(cmd);
            }
            Event::ControllerButtonUp { button, .. } => {
                let Some(pad) = pad_of(button) else { return };
                let cmd = self.controls.release(pad);
                self.command(cmd);
            }
            Event::ControllerAxisMotion { axis, value, .. } => match axis {
                Axis::LeftX => self.controls.stick_x(value),
                Axis::LeftY => self.controls.stick_y(value),
                Axis::TriggerRight => {
                    if value > 16_000 {
                        self.controls.press(Pad::R2);
                    } else {
                        self.controls.release(Pad::R2);
                    }
                }
                _ => {}
            },
            Event::MouseButtonDown { x, y, .. } => {
                if let Some((fx, fy)) = self.video.bottom_point(x, y) {
                    self.touch(fx, fy);
                }
            }
            _ => {}
        }
    }

    fn command(&mut self, cmd: Option<Command>) {
        match cmd {
            Some(Command::Menu) => {
                if self.ui.menu {
                    self.ui.close_menu();
                } else {
                    self.controls.clear();
                    self.ui.open_menu();
                    self.wake_bottom();
                }
            }
            Some(Command::QuickSave) => self.act(Some(Action::Save(0))),
            Some(Command::QuickLoad) => self.act(Some(Action::Load(self.quick_slot))),
            Some(Command::ToggleFast) => self.act(Some(Action::ToggleFast)),
            None => {}
        }
    }

    /// A finger came down on the bottom screen (0..1 each way).
    fn touch(&mut self, x: f32, y: f32) {
        self.last_touch = Instant::now();
        if self.backlight != Backlight::Lit {
            // The first touch only wakes the screen.
            self.wake_bottom();
            return;
        }
        let action = self.ui.tap(
            &self.model,
            (x * ui::W as f32) as i32,
            (y * ui::H as f32) as i32,
        );
        self.act(action);
    }

    /// A finger moved on the bottom screen: sliders follow it.
    fn drag(&mut self, x: f32, y: f32) {
        self.last_touch = Instant::now();
        if self.backlight == Backlight::Lit {
            let action = self.ui.drag(
                &self.model,
                (x * ui::W as f32) as i32,
                (y * ui::H as f32) as i32,
            );
            self.act(action);
        }
    }

    fn wake_bottom(&mut self) {
        self.last_touch = Instant::now();
        match self.backlight {
            Backlight::Dim(p) => nibkit::backlight::set(Screen::Bottom, p),
            Backlight::Off => set_screen_off(Screen::Bottom, false),
            Backlight::Lit => {}
        }
        self.backlight = Backlight::Lit;
    }

    fn act(&mut self, action: Option<Action>) {
        let Some(action) = action else { return };
        match action {
            Action::Save(slot) => {
                let slot = if slot == 0 { self.quick_slot } else { slot };
                self.save_slot(slot);
                self.quick_slot = slot;
                self.toast(&format!("Saved to slot {slot}"));
            }
            Action::Load(slot) => {
                match Slot::read(&self.files.state(slot)).map(|s| self.gb.load_state(&s.machine)) {
                    Some(Ok(())) => {
                        self.quick_slot = slot;
                        self.toast(&format!("Slot {slot} loaded"));
                        self.ui.close_menu();
                    }
                    Some(Err(e)) => self.toast(&format!("Can't load: {e}")),
                    None => self.toast(&format!("Slot {slot} is empty")),
                }
            }
            Action::Set {
                key,
                value,
                for_game,
            } => {
                if key == "@scope" {
                    self.model.for_game = value.as_bool().unwrap_or(true);
                } else {
                    let look_before = self.options.str("look");
                    self.options.set(&key, value, for_game);
                    if key == "look" && self.options.str("look") != look_before {
                        self.toast("Restart the game for the new console");
                    }
                    self.apply_look();
                }
            }
            Action::ToggleFast => self.fast = !self.fast,
            Action::Brightness { top, percent } => {
                let screen = if top { Screen::Top } else { Screen::Bottom };
                nibkit::backlight::set(screen, percent);
                nibkit::backlight::save();
            }
            Action::Volume(p) => nibkit::device::set_volume(p),
            Action::Resume => self.ui.close_menu(),
            Action::Quit => self.quit = true,
        }
        self.refresh_model();
    }

    fn toast(&mut self, text: &str) {
        self.model.toast = Some(text.to_owned());
        self.toast_until = Some(Instant::now() + Duration::from_secs(2));
        self.ui.touch_up();
    }

    fn played(&self) -> u64 {
        self.played_before + self.started.elapsed().as_secs()
    }

    fn save_slot(&mut self, slot: usize) {
        let machine = self.gb.save_state();
        let png = encode_png(&self.rgba);
        let s = Slot {
            made: saves::unix_now(),
            played: self.played(),
            png,
            machine,
        };
        self.writer.write(self.files.state(slot), s.encode());
        self.model.slots.resize(SLOTS + 1, SlotInfo::default());
        self.model.slots[slot] = SlotInfo {
            picture: Some(self.rgba.clone()),
            made: when(s.made),
            played: played(s.played),
        };
    }

    fn save_battery(&mut self) {
        if self.gb.hw.cart.has_battery() && !self.gb.hw.cart.ram().is_empty() {
            self.writer
                .write(self.files.battery(), self.gb.hw.cart.ram().to_vec());
        }
        if let Some(clock) = self.gb.hw.cart.rtc_save(saves::unix_now()) {
            self.writer.write(self.files.clock(), clock);
        }
        self.gb.hw.cart.dirty = false;
        self.battery_dirty = None;
    }

    /// Leaving: the resume state, battery RAM, the screens lit again.
    fn leave(&mut self) {
        self.save_slot(0);
        self.save_battery();
        self.wake_bottom();
        self.writer.finish();
    }

    fn apply_look(&mut self) {
        self.colors = if self.console == "GBC" {
            looks::color_table(self.options.str("colors") != "raw")
        } else if self.gb.hw.ppu.is_color() {
            // A DMG game on a CGB: colours come from the compatibility palettes, chosen by title
            // or by the player.
            let preset = self.options.str("palette");
            let names = [
                "up", "a-up", "b-up", "left", "a-left", "b-left", "down", "a-down", "b-down",
                "right", "a-right", "b-right",
            ];
            if let Some(i) = names.iter().position(|n| *n == preset) {
                let [bg, o0, o1] = inkdot_core::compat::PRESETS[i].1;
                self.gb.hw.ppu.set_palette(false, 0, bg);
                self.gb.hw.ppu.set_palette(true, 0, o0);
                self.gb.hw.ppu.set_palette(true, 1, o1);
            }
            looks::color_table(true)
        } else {
            let s = self.look.shades();
            (0..4).map(|i| [s[i][0], s[i][1], s[i][2], 255]).collect()
        };
    }

    fn lcd_look(&self) -> LcdLook {
        let lcd = self.options.bool("lcd");
        let shades = self.look.shades();
        let dmg = !self.gb.hw.ppu.is_color();
        LcdLook {
            persistence: if lcd {
                if dmg {
                    self.look.persistence()
                } else {
                    0.15
                }
            } else {
                0.0
            },
            grid: lcd,
            gap: if dmg { shades[0] } else { [0xe8, 0xe8, 0xe8] },
            surround: if dmg {
                [0x1b, 0x1d, 0x1a]
            } else {
                [0x14, 0x15, 0x18]
            },
        }
    }

    fn convert_frame(&mut self) {
        let frame = self.gb.frame();
        for (i, &p) in frame.iter().enumerate() {
            let c = self.colors[(p as usize) & (self.colors.len() - 1)];
            self.rgba[i * 4..i * 4 + 4].copy_from_slice(&c);
        }
    }

    fn frame(&mut self) {
        let paused = self.ui.menu;
        let fast = self.fast || self.controls.fast_held();
        if !paused {
            self.gb.set_buttons(self.controls.game_buttons());
            let frames = if fast {
                self.options.int("fast").clamp(2, 8) as u64
            } else if self.options.str("speed") == "exact" {
                self.exact_frames_due()
            } else {
                1
            };
            for _ in 0..frames {
                self.gb.run_frame();
            }
            let base = if self.options.str("speed") == "exact" {
                48_000.0
            } else {
                self.pacer.locked_rate(48_000.0)
            };
            let queued = self.audio.as_ref().map_or(AUDIO_TARGET, |q| q.size() / 4);
            let rate = self.rate.rate(base, queued);
            self.gb.hw.apu.synth().set_rate(rate);
            self.samples.clear();
            self.gb.take_audio(&mut self.samples);
            // Fast-forward is silent; otherwise the sound goes out.
            if let (Some(q), false) = (&self.audio, fast) {
                let _ = q.queue_audio(&self.samples);
            }
            if self.gb.hw.cart.dirty && self.battery_dirty.is_none() {
                self.battery_dirty = Some(Instant::now());
            }
        }
        self.convert_frame();
        self.video.upload_frame(&self.rgba);
        let look = self.lcd_look();
        self.video.present_top(look);

        // Battery RAM a second after the game last started writing it.
        if self
            .battery_dirty
            .is_some_and(|t| t.elapsed() > Duration::from_secs(1))
        {
            self.save_battery();
        }
        if self.toast_until.is_some_and(|t| Instant::now() > t) {
            self.toast_until = None;
            self.model.toast = None;
            self.ui.touch_up();
        }
        self.idle_bottom();
        let clock = clock_now();
        if clock != self.model.clock {
            self.model.clock = clock;
            self.model.battery = battery();
            self.ui.touch_up();
        }
        if let Some(px) = self.ui.render(&self.model) {
            let px = px.to_vec();
            self.video.upload_ui(&px);
            self.video.present_bottom();
        }
    }

    fn exact_frames_due(&mut self) -> u64 {
        let (start, done) = *self.exact_start.get_or_insert((Instant::now(), 0));
        let due = (start.elapsed().as_secs_f64() * GAME_HZ) as u64;
        let n = due.saturating_sub(done).min(3);
        if due > done + 10 {
            // A suspend or a stall: start counting again rather than race to catch up.
            self.exact_start = Some((Instant::now(), 0));
            return 1;
        }
        self.exact_start = Some((start, done + n));
        n
    }

    fn idle_bottom(&mut self) {
        if self.ui.menu || self.backlight != Backlight::Lit || self.last_touch.elapsed() < IDLE {
            return;
        }
        match self.options.str("bottom").as_str() {
            "off" => {
                set_screen_off(Screen::Bottom, true);
                self.backlight = Backlight::Off;
            }
            "dim" => {
                if let Some(p) = nibkit::backlight::get(Screen::Bottom) {
                    nibkit::backlight::set(Screen::Bottom, 0);
                    self.backlight = Backlight::Dim(p);
                }
            }
            _ => {}
        }
    }

    fn refresh_model(&mut self) {
        let m = &mut self.model;
        m.title = self.title.clone();
        m.console = self.console.to_owned();
        m.quick_slot = self.quick_slot;
        m.fast = self.fast;
        m.clock = clock_now();
        m.battery = battery();
        m.top_brightness = nibkit::backlight::get(Screen::Top);
        m.bottom_brightness = match self.backlight {
            Backlight::Dim(p) => Some(p),
            _ => nibkit::backlight::get(Screen::Bottom),
        };
        m.volume = nibkit::device::volume();
        if m.slots.len() != SLOTS + 1 {
            m.slots = (0..=SLOTS)
                .map(|i| slot_info(&self.files.state(i)))
                .collect();
        }
        m.look = if self.console == "GB" {
            self.options.str("look")
        } else {
            String::new()
        };
        m.looks = if self.console == "GB" {
            self.options
                .values("look")
                .into_iter()
                .map(|(v, _)| {
                    let value = v.as_str().unwrap_or("").to_owned();
                    // The tiles are narrow: the models' short names.
                    let label = match value.as_str() {
                        "color" => "Color",
                        "gameboy" => "Game Boy",
                        "pocket" => "Pocket",
                        "light" => "Light",
                        _ => "Grey",
                    }
                    .to_owned();
                    let swatch = match Look::parse(&value) {
                        Some(Look::Color) => {
                            let p = inkdot_core::compat::DEFAULT[0];
                            [0, 1, 2, 3].map(|i| looks::screen(p[i]))
                        }
                        Some(l) => l.shades(),
                        None => [[0; 3]; 4],
                    };
                    LookTile {
                        value,
                        label,
                        swatch,
                    }
                })
                .collect()
        } else {
            Vec::new()
        };
        let console = self.console;
        m.options = self
            .options
            .defs()
            .map(|o| o.def.key.trim_start_matches("inkdot.").to_owned())
            .collect::<Vec<_>>()
            .into_iter()
            .map(|key| {
                let (current, source) = self.options.get(&key);
                OptionRow {
                    label: self.options.label(&key),
                    shown: self.options.show(&key, &current),
                    source: source.label(console),
                    values: self.options.values(&key),
                    current,
                    key,
                }
            })
            .collect();
        if !m.for_game && m.options.is_empty() {
            m.for_game = true;
        }
        self.ui.touch_up();
    }
}

/// Writes or clears a screen's "off" (nibOS's bl_power knob, AGENTS.md "Screen off"). Ignored
/// where the knob isn't there.
fn set_screen_off(screen: Screen, off: bool) {
    let name = match screen {
        Screen::Top => "top",
        Screen::Bottom => "bottom",
    };
    let path = PathBuf::from(format!("/run/nibos/backlight/{name}/bl_power"));
    if path.exists() {
        let _ = std::fs::write(path, if off { "4" } else { "0" });
    }
}

fn encode_png(rgba: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, 160, 144);
        enc.set_color(png::ColorType::Rgba);
        if let Ok(mut w) = enc.write_header() {
            let _ = w.write_image_data(rgba);
        }
    }
    out
}

fn decode_png(data: &[u8]) -> Option<Vec<u8>> {
    let mut reader = png::Decoder::new(data).read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;
    (info.width == 160 && info.height == 144 && info.color_type == png::ColorType::Rgba).then(
        || {
            buf.truncate(info.buffer_size());
            buf
        },
    )
}

fn slot_info(path: &Path) -> SlotInfo {
    match Slot::read(path) {
        Some(s) => SlotInfo {
            picture: decode_png(&s.png),
            made: when(s.made),
            played: played(s.played),
        },
        None => SlotInfo::default(),
    }
}

/// A Unix time as "Oct 19 12:40", in local time.
fn when(t: u64) -> String {
    let secs = t as libc::time_t;
    // SAFETY: localtime_r fills the struct given.
    let tm = unsafe {
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&secs, &mut tm);
        tm
    };
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    format!(
        "{} {} {:02}:{:02}",
        MONTHS[tm.tm_mon.clamp(0, 11) as usize],
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min
    )
}

fn played(secs: u64) -> String {
    let (h, m) = (secs / 3600, secs / 60 % 60);
    if h > 0 {
        format!("{h} h {m} min played")
    } else {
        format!("{m} min played")
    }
}

fn clock_now() -> String {
    let t = when(saves::unix_now());
    t.rsplit(' ').next().unwrap_or("").to_owned()
}

/// The battery's charge, from the first power supply that is a battery.
fn battery() -> Option<u8> {
    let dir = std::fs::read_dir("/sys/class/power_supply").ok()?;
    for e in dir.flatten() {
        let p = e.path();
        if std::fs::read_to_string(p.join("type")).is_ok_and(|t| t.trim() == "Battery") {
            return std::fs::read_to_string(p.join("capacity"))
                .ok()?
                .trim()
                .parse()
                .ok();
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn played_time() {
        assert_eq!(played(59), "0 min played");
        assert_eq!(played(3600 + 23 * 60), "1 h 23 min played");
    }

    #[test]
    fn thumbnails_round_trip() {
        let rgba: Vec<u8> = (0..160 * 144 * 4).map(|i| (i % 251) as u8).collect();
        assert_eq!(decode_png(&encode_png(&rgba)), Some(rgba));
    }
}
