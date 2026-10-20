//! inkdot: a Game Boy and Game Boy Color emulator for the Anbernic RG DS+, on nibOS or the stock
//! OS, started by DSlot, from the stock OS's Apps or by hand. The game is on the top screen; the
//! bottom touch screen has the emulator's controls. `inkdot --help` lists the options;
//! docs/using.md says how to play.

mod args;
mod input;
mod looks;
mod options;
mod pacing;
mod perf;
mod picker;
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
use ui::{Action, Nav, OptionRow, SlotInfo, Ui};
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

/// The audio device's buffer, in sample frames: 512 plays without gaps on nibOS's PipeWire
/// (measured); the stock OS's ALSA shim needs nibkit's bigger one.
const AUDIO_BUFFER: u32 = if cfg!(feature = "stock") {
    nibkit::audio::ADVICE.buffer_frames
} else {
    512
};
/// The audio queue's target, in sample frames: the device's buffer and about a game frame more.
const AUDIO_TARGET: u32 = AUDIO_BUFFER + 700;
/// The bottom screen dims (or goes off) after this long untouched in play.
const IDLE: Duration = Duration::from_secs(10);

/// INKDOT_DEBUG: `stats` logs pacing and sound every five seconds; `shot=DIR` saves both
/// screens as PNG files there ten seconds in; `latency` logs, for each change of the buttons,
/// when the frame that read it started and the vblank at which it was shown (CLOCK_MONOTONIC
/// ns, comparable with nibOS's latency tools); `frames=FILE` writes a line per frame on leaving:
/// the vblank it followed, when it started, finished the game, finished drawing and swapping,
/// and the CPU's clock then (CLOCK_MONOTONIC µs, kHz). For checking a build on the handheld.
#[derive(Clone, Debug, Default)]
struct Debug {
    stats: bool,
    shot: Option<PathBuf>,
    /// `shots=DIR`: both screens every two seconds, numbered.
    shots: Option<PathBuf>,
    /// `cycle-looks`: the Look page opened and each look chosen in turn, then the restarts,
    /// through the same actions as taps (to check live changes on the handheld).
    cycle_looks: bool,
    latency: bool,
    frames: Option<PathBuf>,
    /// The utilization clamp to ask for in play (of 1024), instead of perf::PLAY.
    clamp: Option<u32>,
}

impl Debug {
    fn from_env() -> Debug {
        let mut d = Debug::default();
        for part in std::env::var("INKDOT_DEBUG").unwrap_or_default().split(',') {
            match part.split_once('=') {
                Some(("shot", dir)) => d.shot = Some(PathBuf::from(dir)),
                Some(("shots", dir)) => d.shots = Some(PathBuf::from(dir)),
                Some(("clamp", n)) => d.clamp = n.parse().ok(),
                Some(("frames", file)) => d.frames = Some(PathBuf::from(file)),
                _ if part == "stats" => d.stats = true,
                _ if part == "latency" => d.latency = true,
                _ if part == "cycle-looks" => d.cycle_looks = true,
                _ => {}
            }
        }
        d
    }
}

/// Counts for INKDOT_DEBUG=stats.
#[derive(Clone, Debug, Default)]
struct Stats {
    frames: u64,
    missed: u64,
    since: Option<Instant>,
    /// The times for running the game, drawing the top screen, its swap, and the bottom screen.
    emulate: Vec<Duration>,
    draw: Vec<Duration>,
    swap: Vec<Duration>,
    bottom: Vec<Duration>,
}

/// "median/worst" in milliseconds.
fn spread(v: &mut [Duration]) -> String {
    v.sort();
    let ms = |d: Option<&Duration>| d.map_or(0.0, |d| d.as_secs_f64() * 1000.0);
    format!("{:.2}/{:.2}", ms(v.get(v.len() / 2)), ms(v.last()))
}

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
    /// The game, to start it again on the other console.
    rom: Vec<u8>,
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
    /// The audio device's sample rate.
    out_rate: f64,
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
    debug: Debug,
    stats: Stats,
    frame_count: u64,
    /// INKDOT_DEBUG=latency: the buttons last given to the game, and a change waiting for the
    /// vblank that shows it (when its frame started).
    last_buttons: u8,
    latency_pending: Option<(u8, Duration)>,
    /// The latest vblank seen, and (INKDOT_DEBUG=frames) the one this frame is planned after,
    /// with a record per frame.
    vblank_at: Duration,
    frame_vblank: Duration,
    frame_log: Vec<[u64; 6]>,
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
    let sdl = sdl2::init()?;
    let video_sys = sdl.video()?;
    let mut video = Video::open(&video_sys)?;
    let mut events = sdl.event_pump()?;
    let pads = sdl.game_controller()?;
    for file in nibkit::input::CONTROLLER_MAPPINGS {
        let _ = pads.load_mappings(file);
    }
    let _pads: Vec<GameController> = (0..pads.num_joysticks().unwrap_or(0))
        .filter_map(|i| pads.open(i).ok())
        .collect();
    let mut touch = nibkit::touch::Panel::open();
    let path = match &args.rom {
        Some(p) => p.clone(),
        None => match pick(&mut video, &mut events, &mut touch) {
            Some(p) => p,
            None => return Ok(()),
        },
    };

    let (rom, _) = rom::load(&path)?;
    let stem = rom::stem(&path);
    let header = inkdot_core::Header::parse(rom.get(..0x150).ok_or("not a Game Boy ROM")?);
    let console = if header.supports_cgb() { "GBC" } else { "GB" };
    let options = Options::new(
        console,
        &stem,
        args.options.clone(),
        Settings::load(options::settings_file()),
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
    let (look_value, look_from) = options.get("look");
    eprintln!(
        "inkdot: {stem} ({console}) on a {model:?}, look {look_value} ({}), settings {}",
        look_from.label(console),
        options.settings.file().display()
    );
    let mut gb = GameBoy::new(rom.clone(), model).map_err(|e| e.to_string())?;
    if let Some(boot) = &args.boot_rom {
        let data = std::fs::read(boot).map_err(|e| format!("{}: {e}", boot.display()))?;
        gb.hw.set_boot_rom(data);
    }
    let dir = args
        .saves
        .clone()
        .unwrap_or_else(|| saves::default_dir(console, &stem));
    let files = SaveFiles::new(dir, &stem);
    if let Some(ram) = files.read_battery() {
        gb.hw.cart.load_ram(&ram);
    }
    if let Ok(clock) = std::fs::read(files.clock()) {
        gb.hw.cart.rtc_load(&clock, saves::unix_now());
    }

    let audio = sdl.audio().ok().and_then(|a| {
        let spec = AudioSpecDesired {
            freq: Some(nibkit::audio::ADVICE.rate as i32),
            channels: Some(2),
            samples: Some(AUDIO_BUFFER as u16),
        };
        let q: Result<AudioQueue<i16>, _> = a.open_queue(None, &spec);
        q.inspect_err(|e| eprintln!("inkdot: no sound: {e}")).ok()
    });
    if let Some(q) = &audio {
        q.resume();
    }
    // What the device took, which the sound is made at.
    let out_rate = audio
        .as_ref()
        .map_or(nibkit::audio::ADVICE.rate, |q| q.spec().freq as u32) as f64;
    let mut app = App {
        gb,
        rom,
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
        out_rate,
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
        debug: Debug::from_env(),
        stats: Stats::default(),
        frame_count: 0,
        last_buttons: 0,
        latency_pending: None,
        vblank_at: Duration::ZERO,
        frame_vblank: Duration::ZERO,
        frame_log: Vec::new(),
    };
    app.apply_look();
    // Resume where the game was left, unless told not to.
    if app.options.bool("resume") {
        if let Some(slot) = app.files.resume(model) {
            match app.gb.load_state(&slot.machine) {
                Ok(()) => app.played_before = slot.played,
                Err(e) => eprintln!("inkdot: not resuming: {e}"),
            }
        }
    }
    app.refresh_model();
    let vblanks = watch_vblanks();
    let play_clamp = Debug::from_env().clamp.unwrap_or(perf::PLAY);
    if !perf::clamp_min(play_clamp) {
        eprintln!(
            "inkdot: no utilization clamp (kernel without uclamp): schedutil decides the clock"
        );
    }
    let mut clamped_for_menu = false;
    let mut pointers = Vec::new();
    while !app.quit {
        // Plan the frame for the next refresh it can make, from the top panel's vblanks, and
        // sleep until the latest start that still makes it: the buttons are read as late as
        // they can be (pacing.rs). Without vblanks (the stock OS) the swap's vsync paces.
        if let Some(latest) = &vblanks {
            let at = Duration::from_nanos(latest.load(Ordering::Acquire));
            if at > Duration::ZERO && at != app.vblank_at {
                app.vblank_at = at;
                app.pacer.vblank(at);
            }
            if let Some(plan) = app.pacer.plan(monotonic_now()) {
                if let Some((buttons, read)) = app.latency_pending.take() {
                    eprintln!(
                        "inkdot: buttons {buttons:02x} read {} shown {}",
                        read.as_nanos(),
                        plan.deadline.as_nanos()
                    );
                }
                app.frame_stats(plan.repeated);
                app.frame_vblank = plan.deadline.saturating_sub(pacing::DEADLINE);
                sleep_until(plan.start);
            }
        }
        let work = Instant::now();
        for event in events.poll_iter() {
            app.event(event);
        }
        // Paused in the menu, the clock may fall as it likes (perf.rs).
        if app.ui.menu != clamped_for_menu {
            clamped_for_menu = app.ui.menu;
            perf::clamp_min(if clamped_for_menu {
                perf::MENU
            } else {
                play_clamp
            });
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
        // Up to the swap, not any wait inside it for the last frame's flip.
        let blocked = app
            .video
            .last_swap()
            .saturating_sub(Duration::from_millis(1));
        app.pacer.frame_took(work.elapsed().saturating_sub(blocked));
    }
    app.leave();
    Ok(())
}

/// The top panel's vblanks, as they come, on a thread of their own: the latest one's time
/// (CLOCK_MONOTONIC ns, 0 until the first). `None` where there are none to wait for.
fn watch_vblanks() -> Option<std::sync::Arc<std::sync::atomic::AtomicU64>> {
    let vblank = nibkit::vblank::Vblank::open().ok()?;
    vblank.wait(Screen::Top).ok()?;
    let latest = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
    let out = latest.clone();
    std::thread::Builder::new()
        .name("vblank".into())
        .spawn(move || loop {
            match vblank.wait(Screen::Top) {
                Ok(at) => out.store(at.as_nanos() as u64, Ordering::Release),
                Err(_) => std::thread::sleep(Duration::from_millis(16)),
            }
        })
        .ok()?;
    Some(latest)
}

/// CLOCK_MONOTONIC now, as the vblank timestamps and kernel input events are stamped.
fn monotonic_now() -> Duration {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: clock_gettime fills the struct given.
    unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) };
    Duration::new(ts.tv_sec as u64, ts.tv_nsec as u32)
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
        Pad::X => Nav::X,
        Pad::Y => Nav::Y,
        Pad::L => Nav::L,
        Pad::R => Nav::R,
        _ => return None,
    })
}

impl App {
    fn event(&mut self, event: Event) {
        match event {
            Event::Quit { .. } => self.quit = true,
            Event::KeyDown {
                keycode: Some(k),
                repeat,
                ..
            } => {
                let name = k.name();
                if let Some(cmd) = input::key(&name) {
                    self.command(Some(cmd));
                } else if let (Some(pad), false) = (input::keyboard_pad(&name), repeat) {
                    self.pad_down(pad);
                }
            }
            Event::KeyUp {
                keycode: Some(k), ..
            } => {
                if let Some(pad) = input::keyboard_pad(&k.name()) {
                    self.pad_up(pad);
                }
            }
            Event::ControllerButtonDown { button, .. } => {
                if let Some(pad) = pad_of(button) {
                    self.pad_down(pad);
                }
            }
            Event::ControllerButtonUp { button, .. } => {
                if let Some(pad) = pad_of(button) {
                    self.pad_up(pad);
                }
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

    /// A control went down, on the gamepad or the keyboard: the menu's when it is open.
    fn pad_down(&mut self, pad: Pad) {
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

    fn pad_up(&mut self, pad: Pad) {
        let cmd = self.controls.release(pad);
        self.command(cmd);
    }

    fn command(&mut self, cmd: Option<Command>) {
        match cmd {
            Some(Command::Leave) => self.quit = true,
            Some(Command::PauseMenu) => {
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
            Backlight::Off => nibkit::backlight::set_off(Screen::Bottom, false),
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
                self.options.set(&key, value, for_game);
                self.apply_look();
            }
            Action::Unset { key, for_game } => {
                self.options.unset(&key, for_game);
                self.apply_look();
            }
            Action::Restart => self.restart(),
            Action::OpenMenu => self.command(Some(Command::PauseMenu)),
            Action::Clock(hour12) => {
                if let Err(e) = nibkit::clock::set_hour12(hour12) {
                    eprintln!("inkdot: cannot keep the clock setting: {e}");
                }
                self.model.clock = clock_now();
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
        if let Some(file) = &self.debug.frames {
            let text: String =
                std::iter::once("vblank start emulated drawn swapped khz\n".to_owned())
                    .chain(
                        self.frame_log.iter().map(|r| {
                            r.iter().map(u64::to_string).collect::<Vec<_>>().join(" ") + "\n"
                        }),
                    )
                    .collect();
            let _ = std::fs::write(file, text);
        }
        let model = if self.gb.hw.cgb {
            Console::Cgb
        } else {
            Console::Dmg
        };
        self.files.make_way(model);
        self.save_slot(0);
        self.save_battery();
        self.wake_bottom();
        self.writer.finish();
    }

    /// The look the options ask for: a GB game's chosen look, a GBC game's own colours.
    fn wanted_look(&self) -> Look {
        if self.console == "GB" {
            Look::parse(&self.options.str("look")).unwrap_or(Look::Color)
        } else {
            Look::Color
        }
    }

    /// The console a look runs on.
    fn model_for(&self, look: Look) -> Console {
        if self.console == "GBC" || look == Look::Color {
            Console::Cgb
        } else {
            Console::Dmg
        }
    }

    fn running(&self) -> Console {
        if self.gb.hw.cgb {
            Console::Cgb
        } else {
            Console::Dmg
        }
    }

    /// Shows the look the options ask for, at once: every look's colours can be shown live
    /// except the Color look on a DMG, which only a CGB can make (the restart offer is for
    /// that, and for running a DMG look on a real DMG).
    fn apply_look(&mut self) {
        let wanted = self.wanted_look();
        if !(wanted == Look::Color && self.running() == Console::Dmg) {
            self.look = wanted;
        }
        self.colors = if self.console == "GBC" {
            looks::color_table(self.options.str("colors") != "raw")
        } else if self.gb.hw.ppu.is_color() && self.look != Look::Color {
            // A DMG look on a CGB: its shades in all three compatibility palettes.
            let shades = self.look.shades().map(looks::rgb555);
            self.gb.hw.ppu.set_palette(false, 0, shades);
            self.gb.hw.ppu.set_palette(true, 0, shades);
            self.gb.hw.ppu.set_palette(true, 1, shades);
            // The picture on screen too, though the game may be paused.
            self.gb.hw.ppu.recolor();
            looks::color_table(false)
        } else if self.gb.hw.ppu.is_color() {
            // A DMG game on a CGB: colours come from the compatibility palettes, chosen by title
            // or by the player.
            let preset = self.options.str("palette");
            let names = [
                "up", "a-up", "b-up", "left", "a-left", "b-left", "down", "a-down", "b-down",
                "right", "a-right", "b-right",
            ];
            let [bg, o0, o1] = match names.iter().position(|n| *n == preset) {
                Some(i) => inkdot_core::compat::PRESETS[i].1,
                None => inkdot_core::compat::for_header(self.gb.hw.cart.header()),
            };
            self.gb.hw.ppu.set_palette(false, 0, bg);
            self.gb.hw.ppu.set_palette(true, 0, o0);
            self.gb.hw.ppu.set_palette(true, 1, o1);
            self.gb.hw.ppu.recolor();
            looks::color_table(true)
        } else {
            let s = self.look.shades();
            (0..4).map(|i| [s[i][0], s[i][1], s[i][2], 255]).collect()
        };
        // The look runs on the other console: Play offers to start again on it.
        let other = self.model_for(wanted);
        self.model.restart = (other != self.running()).then(|| {
            match other {
                Console::Cgb => "Restart as a Game Boy Color",
                Console::Dmg => "Restart on a Game Boy",
            }
            .to_owned()
        });
        self.ui.touch_up();
    }

    /// Starts the game again on the console its look runs on: where it was is saved first (and
    /// kept for when the look goes back), the battery RAM and clock carry over, and it resumes
    /// where it was last left on the new console, if it ever was.
    fn restart(&mut self) {
        let model = self.model_for(self.wanted_look());
        self.files.make_way(self.running());
        self.save_slot(0);
        let rom = self.rom.clone();
        let Ok(mut gb) = GameBoy::new(rom, model) else {
            return;
        };
        gb.hw.cart.load_ram(self.gb.hw.cart.ram());
        if let Some(clock) = self.gb.hw.cart.rtc_save(saves::unix_now()) {
            gb.hw.cart.rtc_load(&clock, saves::unix_now());
        }
        self.gb = gb;
        self.played_before = 0;
        if let Some(slot) = self.files.resume(model) {
            if self.gb.load_state(&slot.machine).is_ok() {
                self.played_before = slot.played;
            }
        }
        self.started = Instant::now();
        self.apply_look();
        self.ui.close_menu();
        self.refresh_model();
    }

    fn lcd_look(&self) -> LcdLook {
        let lcd = self.options.bool("lcd");
        let shades = self.look.shades();
        // A DMG look's screen, on a DMG or in a CGB's palettes alike.
        let dmg = self.look != Look::Color;
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
        let logging = self.debug.frames.is_some();
        let m0 = if logging {
            monotonic_now()
        } else {
            Duration::ZERO
        };
        let paused = self.ui.menu;
        let fast = self.fast || self.controls.fast_held();
        let t0 = Instant::now();
        if !paused {
            let buttons = self.controls.game_buttons();
            if self.debug.latency && buttons != self.last_buttons {
                self.latency_pending = Some((buttons, monotonic_now()));
            }
            self.last_buttons = buttons;
            self.gb.set_buttons(buttons);
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
                self.out_rate
            } else {
                self.pacer.locked_rate(self.out_rate)
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
        let t1 = Instant::now();
        let m1 = if logging {
            monotonic_now()
        } else {
            Duration::ZERO
        };
        self.convert_frame();
        self.video.upload_frame(&self.rgba);
        let look = self.lcd_look();
        self.frame_count += 1;
        if self.debug.cycle_looks {
            self.cycle_looks();
        }
        if let Some(overlay) = self.ui.render_top(&self.model) {
            let overlay = overlay.map(<[u8]>::to_vec);
            self.video.set_overlay(overlay.as_deref());
        }
        let shoot = self.debug.shot.is_some() && self.frame_count == 600;
        let series = self.debug.shots.is_some() && self.frame_count.is_multiple_of(120);
        if let Some(px) = self.video.present_top(look, shoot || series) {
            if series {
                self.save_series(&px);
            } else {
                self.save_shots(&px);
            }
        }
        let t2 = Instant::now();
        if logging {
            let us = |d: Duration| d.as_micros() as u64;
            let swap_start = monotonic_now().saturating_sub(self.video.last_swap());
            let khz =
                std::fs::read_to_string("/sys/devices/system/cpu/cpufreq/policy0/scaling_cur_freq")
                    .ok()
                    .and_then(|s| s.trim().parse().ok())
                    .unwrap_or(0);
            self.frame_log.push([
                us(self.frame_vblank),
                us(m0),
                us(m1),
                us(swap_start),
                us(monotonic_now()),
                khz,
            ]);
        }

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
        // The header's clock and battery, when someone may be looking: not while the bottom
        // screen is dimmed or off in play, where redrawing it would only cost frame time.
        let clock = clock_now();
        // (The clock is only on the menu: the bottom screen is blank in play.)
        let seen = self.ui.menu;
        if clock != self.model.clock && seen {
            self.model.clock = clock;
            self.model.battery = battery();
            self.ui.touch_up();
        }
        if let Some(px) = self.ui.render(&self.model) {
            let px = px.to_vec();
            self.video.upload_ui(&px);
            self.video.present_bottom();
        }
        if self.debug.stats {
            let swap = self.video.last_swap();
            let s = &mut self.stats;
            s.emulate.push(t1 - t0);
            s.draw.push((t2 - t1).saturating_sub(swap));
            s.swap.push(swap);
            s.bottom.push(t2.elapsed());
        }
    }

    /// INKDOT_DEBUG=stats: frames, missed vblanks, the panel's period, the frame budget, the
    /// sound queue and rate, every five seconds.
    fn frame_stats(&mut self, repeated: u64) {
        if !self.debug.stats {
            return;
        }
        let s = &mut self.stats;
        // Refreshes planned without a frame, and swaps that waited for a flip that came a
        // refresh late (the frame before missed its deadline).
        s.missed += repeated;
        if self.video.last_swap() > self.pacer.period() / 2 {
            s.missed += 1;
        }
        s.frames += 1;
        let since = *s.since.get_or_insert_with(Instant::now);
        if since.elapsed() >= Duration::from_secs(5) {
            let queued = self.audio.as_ref().map_or(0, |q| q.size() / 4);
            eprintln!(
                "inkdot: {} frames, {} vblanks missed; median/worst ms: emulate {} draw {} swap {} bottom {}; period {:.4} ms, budget {:.2} ms, sound queue {} frames at {:.1} Hz",
                s.frames,
                s.missed,
                spread(&mut s.emulate),
                spread(&mut s.draw),
                spread(&mut s.swap),
                spread(&mut s.bottom),
                self.pacer.period().as_secs_f64() * 1000.0,
                self.pacer.budget().as_secs_f64() * 1000.0,
                queued,
                self.gb.hw.apu.synth().rate(),
            );
            *s = Stats { ..Stats::default() };
        }
    }

    fn save_shots(&mut self, top: &[u8]) {
        let Some(dir) = &self.debug.shot else { return };
        let _ = std::fs::create_dir_all(dir);
        let (w, h) = self.video.top_size();
        write_png(&dir.join("top.png"), top, w, h);
        self.ui.touch_up();
        if let Some(px) = self.ui.render(&self.model) {
            write_png(&dir.join("bottom.png"), px, ui::W as u32, ui::H as u32);
        }
        self.ui.touch_up();
        eprintln!("inkdot: screens saved in {}", dir.display());
    }

    /// INKDOT_DEBUG=cycle-looks: every four seconds, the next step.
    fn cycle_looks(&mut self) {
        let n = self.frame_count;
        // Frame 120's capture is of play (the bottom screen blank); the menu opens after it.
        if n == 150 {
            self.ui.open_menu();
            self.ui.go(ui::Page::Game);
        }
        if n < 180 || !(n - 180).is_multiple_of(240) {
            return;
        }
        let set = |v: &str| Action::Set {
            key: "look".into(),
            value: v.into(),
            for_game: true,
        };
        let step = match (n - 180) / 240 {
            0 => set("color"),
            1 => set("gameboy"),
            2 => set("pocket"),
            3 => set("light"),
            4 => set("grey"),
            5 => Action::Restart,
            6 => set("color"),
            7 => Action::Restart,
            8 | 9 => {
                // Then the Play and States pages, for their captures.
                self.ui.go(if (n - 180) / 240 == 8 {
                    ui::Page::Play
                } else {
                    ui::Page::States
                });
                return;
            }
            _ => return,
        };
        eprintln!("inkdot: cycle-looks frame {n}: {step:?}");
        self.act(Some(step));
        if !self.ui.menu {
            self.ui.open_menu();
            self.ui.go(ui::Page::Game);
        }
    }

    /// INKDOT_DEBUG=shots: the top screen and the bottom one's picture, numbered by frame.
    fn save_series(&mut self, top: &[u8]) {
        let Some(dir) = self.debug.shots.clone() else {
            return;
        };
        let _ = std::fs::create_dir_all(&dir);
        let (w, h) = self.video.top_size();
        let n = self.frame_count;
        write_png(&dir.join(format!("{n:06}-top.png")), top, w, h);
        self.ui.touch_up();
        if let Some(px) = self.ui.render(&self.model) {
            write_png(
                &dir.join(format!("{n:06}-bottom.png")),
                px,
                ui::W as u32,
                ui::H as u32,
            );
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
                nibkit::backlight::set_off(Screen::Bottom, true);
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
        // Today's play, once there is a minute of it to say.
        let secs = self.started.elapsed().as_secs();
        m.played = if secs >= 60 {
            played(secs).replace(" played", " today")
        } else {
            String::new()
        };
        m.hour12 = hour12();
        m.game_options = option_rows(&self.options, true);
        m.console_options = option_rows(&self.options, false);
        self.ui.touch_up();
    }
}

/// The options as their rows show them at one level: the game's (`for_game`) or its console's.
fn option_rows(o: &Options, for_game: bool) -> Vec<OptionRow> {
    o.keys()
        .into_iter()
        .map(|key| {
            let here = o.here(key, for_game);
            let (value, from) = o.inherited(key, for_game);
            let values: Vec<_> = std::iter::once(None)
                .chain(o.values(key).into_iter().map(|(v, _)| Some(v)))
                .collect();
            let at = here
                .as_ref()
                .and_then(|h| values.iter().position(|v| v.as_ref() == Some(h)))
                .unwrap_or(0);
            OptionRow {
                key: key.to_owned(),
                label: o.label(key),
                here: here.as_ref().map(|h| o.show(key, h)),
                inherit: format!("Use {from} ({})", o.show(key, &value)),
                values,
                at,
                wraps: o.wraps(key),
                use_hint: format!("use {from}"),
                shows: matches!(key, "look" | "palette" | "colors" | "lcd"),
            }
        })
        .collect()
}

/// Without a ROM on the command line: the list of games on the cards, until one is chosen
/// (`None`: the player left, or SIGTERM).
fn pick(
    video: &mut Video,
    events: &mut sdl2::EventPump,
    touch: &mut Option<nibkit::touch::Panel>,
) -> Option<PathBuf> {
    let mut picker = picker::Picker::new(picker::find(&nibkit::paths::rom_roots()));
    let blank = vec![0u8; 160 * 144 * 4];
    let look = LcdLook {
        persistence: 0.0,
        grid: false,
        gap: [0; 3],
        surround: [0; 3],
    };
    let mut pointers = Vec::new();
    loop {
        if TERM.load(Ordering::Relaxed) {
            return None;
        }
        let mut start = None;
        for event in events.poll_iter() {
            match event {
                Event::Quit { .. } => return None,
                Event::KeyDown {
                    keycode: Some(k), ..
                } => {
                    let name = k.name();
                    if nibkit::input::BACK_KEYCODES.contains(&name.as_str()) || name == "Escape" {
                        return None;
                    }
                }
                Event::ControllerButtonDown { button, .. } => {
                    use nibkit::menu::Key;
                    let key = match pad_of(button) {
                        Some(Pad::Up) => Key::Up,
                        Some(Pad::Down) => Key::Down,
                        Some(Pad::L | Pad::Left) => Key::L,
                        Some(Pad::R | Pad::Right) => Key::R,
                        Some(Pad::A | Pad::Start) => Key::A,
                        Some(Pad::B) => Key::B,
                        _ => continue,
                    };
                    match picker.key(key) {
                        Ok(chosen) => start = chosen.or(start),
                        Err(()) => return None,
                    }
                }
                Event::MouseButtonDown { x, y, .. } => {
                    start = video
                        .bottom_point(x, y)
                        .and_then(|(fx, fy)| picker.touch(fx, fy));
                }
                _ => {}
            }
        }
        if let Some(panel) = touch {
            pointers.clear();
            panel.poll(&mut pointers);
            for p in pointers.drain(..) {
                if let nibkit::touch::Pointer::Down(x, y) = p {
                    start = picker.touch(x, y).or(start);
                }
            }
        }
        if let Some(game) = start.and_then(|i| picker.games.get(i)) {
            return Some(game.path.clone());
        }
        if let Some(px) = picker.render(&clock_now()) {
            video.upload_ui(px);
        }
        video.upload_frame(&blank);
        video.present_top(look, false);
        video.present_bottom();
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

fn write_png(path: &Path, rgba: &[u8], w: u32, h: u32) {
    let Ok(file) = std::fs::File::create(path) else {
        return;
    };
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w, h);
    enc.set_color(png::ColorType::Rgba);
    if let Ok(mut out) = enc.write_header() {
        let _ = out.write_image_data(rgba);
    }
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
        "{} {} {}",
        MONTHS[tm.tm_mon.clamp(0, 11) as usize],
        tm.tm_mday,
        nibkit::clock::format(tm.tm_hour as u32, tm.tm_min as u32, hour12())
    )
}

/// Whether times show as 12-hour: the player's choice, shared with DSlot and the rest of nibOS
/// (`nibkit::clock`), read again each second so a change in DSlot shows at once.
fn hour12() -> bool {
    use std::sync::Mutex;
    static CACHE: Mutex<Option<(Instant, bool)>> = Mutex::new(None);
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    match *cache {
        Some((at, value)) if at.elapsed() < Duration::from_secs(1) => value,
        _ => {
            let value = nibkit::clock::hour12();
            *cache = Some((Instant::now(), value));
            value
        }
    }
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
    // The date is the first two words; the time ("17:05" or "5:05 PM") the rest.
    let t = when(saves::unix_now());
    t.splitn(3, ' ').nth(2).unwrap_or("").to_owned()
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

    /// Dates and the header's clock follow the shared 12/24-hour choice.
    #[test]
    fn times_follow_the_clock_choice() {
        let date = when(1_760_000_000);
        let clock = date.splitn(3, ' ').nth(2).unwrap();
        if hour12() {
            assert!(clock.ends_with(" AM") || clock.ends_with(" PM"), "{date}");
        } else {
            assert_eq!(clock.len(), 5, "{date}");
        }
        assert!(!clock_now().is_empty());
    }

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
