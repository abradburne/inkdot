//! RetroAchievements for an emulator: a session with the service through rcheevos, the official
//! client library (vendored under `rcheevos/`, MIT), driven once per emulated frame.
//!
//! The player signs in once, in DSlot, which keeps the service's token in a file of its own and
//! starts a game with `RA_TOKEN_FILE` in its environment ([`read_token_file`]); the emulator
//! signs in with that token, never a password, and never writes the token anywhere. The game is
//! identified by the hash of its ROM ([`hash`]). The emulator reads its machine's memory for
//! rcheevos through [`Memory`], and is told what happened in [`Event`]s, which it words and
//! shows in its own way.
//!
//! Threads: rcheevos and everything that calls it run on the emulation thread, inside
//! [`Client::frame`] and [`Client::idle`], where the machine's memory is read. The network is a
//! thread of its own ([`transport`]): a request is handed over and the answer is given to
//! rcheevos on a later frame, so a slow network never holds a frame. Nothing here reads the
//! machine from the network thread.

mod ffi;
mod transport;

use std::cell::{Cell, RefCell};
use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::path::{Path, PathBuf};
use std::ptr::NonNull;
use std::sync::mpsc::{channel, Receiver, Sender};

/// The console's memory as RetroAchievements lays it out for the console (rcheevos'
/// `src/rcheevos/consoleinfo.c` has each console's map).
pub trait Memory {
    /// Reads `buf.len()` bytes at `address`; how many of them are backed by something. Bytes
    /// nothing backs read as zero, and an achievement that needs them is marked unsupported.
    fn read(&self, address: u32, buf: &mut [u8]) -> u32;
}

/// The sign-in: who, and the token that stands for them. The password is never seen here, and
/// the token is never printed, `{:?}` included.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Credentials {
    /// The account; may be empty when the token file had only the token.
    pub username: String,
    /// The service's token for it.
    pub token: String,
}

impl std::fmt::Debug for Credentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Credentials")
            .field("username", &self.username)
            .field("token", &"<redacted>")
            .finish()
    }
}

/// Reads a token file: DSlot's (the username, then the token, one a line), or a token alone
/// (the username then comes from elsewhere, `RA_USERNAME`). Whitespace around either is
/// ignored.
pub fn read_token_file(path: &Path) -> Result<Credentials, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let words: Vec<&str> = text.split_whitespace().take(3).collect();
    match words[..] {
        [] => Err(format!("{}: no token in the file", path.display())),
        [token] => Ok(Credentials {
            username: String::new(),
            token: token.to_owned(),
        }),
        [username, token, ..] => Ok(Credentials {
            username: username.to_owned(),
            token: token.to_owned(),
        }),
    }
}

/// The RetroAchievements identity of a Game Boy or Game Boy Color ROM: 32 lowercase hex digits
/// (the MD5 of the file, for both consoles, as rcheevos' `rc_hash` computes it). `color` says
/// which console's rule to ask for; the digest is the same.
pub fn hash(rom: &[u8], color: bool) -> Option<String> {
    let mut out = [0 as c_char; 33];
    // SAFETY: `out` has the 33 bytes the shim writes; `rom` is read for its length only.
    let ok = unsafe {
        ffi::ink_rc_hash_gameboy(out.as_mut_ptr(), rom.as_ptr(), rom.len(), color.into())
    };
    if ok == 0 {
        return None;
    }
    // SAFETY: the shim NUL-terminates `out` within its 33 bytes.
    let s = unsafe { CStr::from_ptr(out.as_ptr()) };
    Some(s.to_string_lossy().into_owned())
}

/// rcheevos' version.
pub fn rcheevos_version() -> &'static str {
    // SAFETY: the shim returns a static NUL-terminated string.
    unsafe { CStr::from_ptr(ffi::ink_rc_version()) }
        .to_str()
        .unwrap_or("0")
}

/// How the game is played, fixed for the session.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Settings {
    /// Hardcore: the service ranks the unlocks, and the emulator must offer no save states,
    /// rewind, slow motion or cheats while it is on. Off is Casual (softcore).
    pub hardcore: bool,
    /// Achievements already earned can be earned again (they are not sent again).
    pub encore: bool,
    /// The set's unpublished (unofficial) achievements are evaluated too.
    pub unofficial: bool,
}

/// Where the session is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    /// Not signed in (yet, or the sign-in failed).
    SignedOut,
    /// The sign-in is on its way.
    SigningIn,
    /// Signed in, no game loaded.
    SignedIn,
    /// The game's set is being fetched.
    LoadingGame,
    /// A set is loaded and being evaluated.
    Playing,
    /// Signed in, but the service does not know this dump.
    NoSet,
    /// The dump is known, but the game has no published achievements yet.
    EmptySet,
}

/// The loaded set in numbers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Summary {
    /// Achievements in the game's core set.
    pub total: u32,
    /// Of them, the ones the player has.
    pub unlocked: u32,
    /// Of them, the ones that read memory the emulator does not back: they can't be earned.
    pub unsupported: u32,
    /// Unofficial achievements (evaluated only with [`Settings::unofficial`]).
    pub unofficial: u32,
    /// The set's points in all.
    pub points: u32,
    /// The points the player has.
    pub points_earned: u32,
}

/// An achievement, as an event has it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Achievement {
    /// The service's id.
    pub id: u32,
    /// Its name.
    pub title: String,
    /// What earns it.
    pub description: String,
    /// What it is worth.
    pub points: u32,
    /// The service's name for its badge ("12345"); empty when none.
    pub badge_name: String,
    /// Where its badge picture is; empty when rcheevos has none.
    pub badge_url: String,
    /// How far a measured achievement is ("3/10", "45%"); empty when it isn't measured.
    pub measured_progress: String,
    /// The same, as a fraction of 1.
    pub measured_percent: f32,
}

/// A leaderboard, as an event has it (only in hardcore).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Leaderboard {
    /// The service's id.
    pub id: u32,
    /// Its name.
    pub title: String,
    /// What it measures.
    pub description: String,
    /// The value an attempt ended on, formatted (on a submission).
    pub value: String,
}

/// Something the player should be told about, or that changes what is shown over the game.
/// The emulator words it; nothing here is English meant for the player but the service's own
/// titles and messages.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// Signed in, as this name.
    SignedIn {
        /// The player's name as the service shows it.
        display_name: String,
    },
    /// The sign-in failed.
    SignInFailed {
        /// The service refused the token or it has expired: sign in again in the launcher.
        token_refused: bool,
        /// Why, as rcheevos says it; may be empty.
        message: String,
    },
    /// The game's set is loaded and being evaluated: what to say as play begins.
    GameLoaded {
        /// The game's title at the service.
        title: String,
        /// Its set in numbers.
        summary: Summary,
        /// Whether it is played in hardcore.
        hardcore: bool,
    },
    /// The game is known but has no published achievements.
    NoAchievements {
        /// The game's title at the service.
        title: String,
    },
    /// The service doesn't know this dump (a legitimate ROM can still fail to match).
    UnknownGame {
        /// The hash it was asked about.
        hash: String,
    },
    /// The set could not be loaded.
    LoadFailed {
        /// Why, as rcheevos says it; may be empty.
        message: String,
    },
    /// An achievement was earned.
    Unlocked(Achievement),
    /// An achievement can be earned right now: show its badge while the challenge lasts.
    ChallengeShown(Achievement),
    /// The challenge for the achievement with this id is over.
    ChallengeHidden {
        /// The achievement's id.
        id: u32,
    },
    /// A measured achievement moved on: show its progress for a moment.
    Progress(Achievement),
    /// The progress shown can go.
    ProgressHidden,
    /// A leaderboard attempt started.
    LeaderboardStarted(Leaderboard),
    /// A leaderboard attempt failed.
    LeaderboardFailed(Leaderboard),
    /// A leaderboard attempt ended and was sent.
    LeaderboardSubmitted(Leaderboard),
    /// A leaderboard's running value, to show (or update) while it runs.
    Tracker {
        /// Which tracker.
        id: u32,
        /// Its value, formatted.
        display: String,
    },
    /// The tracker with this id can go.
    TrackerHidden {
        /// Which tracker.
        id: u32,
    },
    /// Every achievement of the game is earned.
    GameCompleted {
        /// The game's title at the service.
        title: String,
        /// Its set in numbers, as of now.
        summary: Summary,
        /// Whether it was played in hardcore.
        hardcore: bool,
    },
    /// Every achievement of a subset is earned.
    SubsetCompleted {
        /// The subset's title.
        title: String,
    },
    /// The service answered a request with an error, and it will not be retried.
    ServerError {
        /// The request.
        api: String,
        /// The service's message.
        message: String,
    },
    /// An unlock could not be sent and is waiting.
    Disconnected,
    /// Every waiting unlock is sent.
    Reconnected,
    /// The machine should be reset (rcheevos asks when hardcore is turned on with a game
    /// loaded).
    ResetAsked,
}

/// "Warning" achievements, which the service excludes from a game's set; rcheevos still reports
/// them.
const WARNING_ACHIEVEMENT_ID: u32 = 101_000_001;

/// What the callbacks reach: the machine's memory (only during a frame), the channels, the
/// events.
struct Inner {
    raw: Cell<*mut ffi::InkRc>,
    /// The memory, while [`Client::frame`] or [`Client::idle`] runs: a pointer to the caller's
    /// `&dyn Memory` on its stack; null otherwise, when a read answers zero and unbacked.
    memory: Cell<*const ()>,
    jobs: Sender<transport::Job>,
    /// An answer that never went to the network (no network thread) goes here too.
    answers: Sender<transport::Done>,
    done: Receiver<transport::Done>,
    state: Cell<State>,
    hash: RefCell<String>,
    events: RefCell<Vec<Event>>,
    /// Progress to restore once the game has loaded (a state loaded while it was loading).
    pending_progress: RefCell<Option<Vec<u8>>>,
    verbose: bool,
}

impl Inner {
    fn post(&self, event: Event) {
        self.events.borrow_mut().push(event);
    }

    fn log(&self, line: &str) {
        if self.verbose {
            eprintln!("cheevos: {line}");
        }
    }

    fn summary(&self) -> Summary {
        let mut s = ffi::Summary::default();
        // SAFETY: the client is alive while `raw` is set; `s` is a valid out-pointer.
        unsafe { ffi::ink_rc_get_summary(self.raw.get(), &mut s) };
        Summary {
            total: s.total,
            unlocked: s.unlocked,
            unsupported: s.unsupported,
            unofficial: s.unofficial,
            points: s.points,
            points_earned: s.points_earned,
        }
    }

    fn hardcore(&self) -> bool {
        // SAFETY: the client is alive while `raw` is set.
        unsafe { ffi::ink_rc_hardcore(self.raw.get()) != 0 }
    }

    fn game_title(&self) -> String {
        // SAFETY: the client is alive while `raw` is set; the string is valid until the next
        // call into the client, and copied now.
        unsafe { string(ffi::ink_rc_game_title(self.raw.get())) }
    }

    /// Restores progress (or starts it over, for `None`) in the loaded game.
    fn load_progress(&self, progress: Option<&[u8]>) {
        let (ptr, len) = progress.map_or((std::ptr::null(), 0), |p| (p.as_ptr(), p.len()));
        // SAFETY: the client is alive; `ptr` is null or valid for `len` bytes for the call.
        let r = unsafe { ffi::ink_rc_load_progress(self.raw.get(), ptr, len) };
        if r != ffi::OK {
            self.log("the state's achievement progress was not rcheevos': started over");
        }
    }
}

/// A C string from the shim, copied; empty for null.
///
/// # Safety
///
/// `p` is null or a NUL-terminated string valid for the call.
unsafe fn string(p: *const c_char) -> String {
    if p.is_null() {
        return String::new();
    }
    // SAFETY: as the caller promises.
    unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
}

/// The client's [`Inner`] from the shim's `ud`.
///
/// # Safety
///
/// `ud` is the pointer [`Client::start`] gave the shim, and the client is alive (rcheevos only
/// calls back from inside a call the client makes).
unsafe fn inner<'a>(ud: *mut c_void) -> &'a Inner {
    // SAFETY: as the caller promises: a live Box<Inner>, only ever shared.
    unsafe { &*(ud as *const Inner) }
}

/// An achievement from the shim, copied.
///
/// # Safety
///
/// `a` points to a struct the shim filled, valid for the call.
unsafe fn achievement(a: &ffi::Achievement) -> Achievement {
    // SAFETY: the shim's strings are valid for the call.
    unsafe {
        Achievement {
            id: a.id,
            title: string(a.title),
            description: string(a.description),
            points: a.points,
            badge_name: string(a.badge_name),
            badge_url: string(a.badge_url),
            measured_progress: string(a.measured_progress),
            measured_percent: a.measured_percent,
        }
    }
}

extern "C" fn read(address: u32, buffer: *mut u8, num_bytes: u32, ud: *mut c_void) -> u32 {
    if buffer.is_null() || num_bytes == 0 {
        return 0;
    }
    // SAFETY: rcheevos gives `num_bytes` (not 0) of writable, non-null `buffer`; `ud` is the
    // client's Inner.
    let (inner, buf) = unsafe {
        (
            inner(ud),
            std::slice::from_raw_parts_mut(buffer, num_bytes as usize),
        )
    };
    let at = inner.memory.get();
    if at.is_null() {
        buf.fill(0);
        return 0;
    }
    // SAFETY: `memory` is set only for the duration of frame()/idle(), to a `&dyn Memory` on
    // the caller's stack, and rcheevos only reads memory inside those calls.
    let memory = unsafe { *at.cast::<&dyn Memory>() };
    memory.read(address, buf)
}

extern "C" fn request(
    url: *const c_char,
    post_data: *const c_char,
    content_type: *const c_char,
    ticket: *mut c_void,
    ud: *mut c_void,
) {
    // SAFETY: `ud` is the client's Inner; the strings are valid for the call.
    let (inner, url, post) = unsafe {
        (
            inner(ud),
            string(url),
            (!post_data.is_null()).then(|| (string(content_type), string(post_data))),
        )
    };
    inner.log(&format!(
        "-> {url}{}",
        if post.is_some() { " (POST)" } else { "" }
    ));
    let job = transport::Job {
        url,
        post,
        ticket: ticket as usize,
    };
    if let Err(e) = inner.jobs.send(job) {
        // No network thread: answered on the next frame as a network failure, which rcheevos
        // retries.
        let _ = inner.answers.send(transport::Done::not_reached(
            e.0.ticket,
            "no network thread",
        ));
    }
}

extern "C" fn event(e: *const ffi::Event, ud: *mut c_void) {
    if e.is_null() {
        return;
    }
    // SAFETY: `ud` is the client's Inner; the event and everything it points to are valid for
    // the call.
    let (inner, e) = unsafe { (inner(ud), &*e) };
    // SAFETY: the shim's pointers are null or valid for the call, and so are its strings.
    let (a, board, tracker, subset, api, message) = unsafe {
        (
            e.achievement.as_ref().map(|a| achievement(a)),
            e.leaderboard.as_ref().map(|l| Leaderboard {
                id: l.id,
                title: string(l.title),
                description: string(l.description),
                value: string(l.tracker_value),
            }),
            e.tracker.as_ref().map(|t| (t.id, string(t.display))),
            string(e.subset_title),
            string(e.error_api),
            string(e.error_message),
        )
    };
    let event = match (e.kind, a, board, tracker) {
        (ffi::EVENT_UNLOCKED, Some(a), ..) if a.id >= WARNING_ACHIEVEMENT_ID => {
            inner.log(&format!(
                "ignoring warning achievement {}: {}",
                a.id, a.title
            ));
            return;
        }
        (ffi::EVENT_UNLOCKED, Some(a), ..) => Event::Unlocked(a),
        (ffi::EVENT_CHALLENGE_SHOW, Some(a), ..) => Event::ChallengeShown(a),
        (ffi::EVENT_CHALLENGE_HIDE, Some(a), ..) => Event::ChallengeHidden { id: a.id },
        (ffi::EVENT_PROGRESS_SHOW | ffi::EVENT_PROGRESS_UPDATE, Some(a), ..) => Event::Progress(a),
        (ffi::EVENT_PROGRESS_HIDE, ..) => Event::ProgressHidden,
        (ffi::EVENT_LEADERBOARD_STARTED, _, Some(l), _) => Event::LeaderboardStarted(l),
        (ffi::EVENT_LEADERBOARD_FAILED, _, Some(l), _) => Event::LeaderboardFailed(l),
        (ffi::EVENT_LEADERBOARD_SUBMITTED, _, Some(l), _) => Event::LeaderboardSubmitted(l),
        (ffi::EVENT_TRACKER_SHOW | ffi::EVENT_TRACKER_UPDATE, _, _, Some((id, display))) => {
            Event::Tracker { id, display }
        }
        (ffi::EVENT_TRACKER_HIDE, _, _, Some((id, _))) => Event::TrackerHidden { id },
        (ffi::EVENT_RESET, ..) => Event::ResetAsked,
        (ffi::EVENT_GAME_COMPLETED, ..) => Event::GameCompleted {
            title: inner.game_title(),
            summary: inner.summary(),
            hardcore: inner.hardcore(),
        },
        (ffi::EVENT_SUBSET_COMPLETED, ..) => Event::SubsetCompleted { title: subset },
        (ffi::EVENT_SERVER_ERROR, ..) => Event::ServerError { api, message },
        (ffi::EVENT_DISCONNECTED, ..) => Event::Disconnected,
        (ffi::EVENT_RECONNECTED, ..) => Event::Reconnected,
        (kind, ..) => {
            inner.log(&format!("ignoring event {kind}"));
            return;
        }
    };
    inner.post(event);
}

extern "C" fn login_done(result: c_int, error_message: *const c_char, ud: *mut c_void) {
    // SAFETY: `ud` is the client's Inner; the string is valid for the call.
    let (inner, message) = unsafe { (inner(ud), string(error_message)) };
    if result == ffi::OK {
        inner.state.set(State::SignedIn);
        // SAFETY: the client is alive; the name is copied before the next call into it.
        let display_name = unsafe { string(ffi::ink_rc_display_name(inner.raw.get())) };
        inner.post(Event::SignedIn { display_name });
    } else {
        inner.state.set(State::SignedOut);
        inner.post(Event::SignInFailed {
            token_refused: result == ffi::BAD_TOKEN,
            message,
        });
    }
}

extern "C" fn load_done(result: c_int, error_message: *const c_char, ud: *mut c_void) {
    // SAFETY: `ud` is the client's Inner; the string is valid for the call.
    let (inner, message) = unsafe { (inner(ud), string(error_message)) };
    let pending = inner.pending_progress.borrow_mut().take();
    if result == ffi::OK {
        let title = inner.game_title();
        let summary = inner.summary();
        inner.log(&format!(
            "{} achievements, {} unlocked, {} unsupported, {} unofficial, {} of {} points",
            summary.total,
            summary.unlocked,
            summary.unsupported,
            summary.unofficial,
            summary.points_earned,
            summary.points
        ));
        if summary.total == 0 {
            inner.state.set(State::EmptySet);
            inner.post(Event::NoAchievements { title });
            return;
        }
        inner.state.set(State::Playing);
        if let Some(p) = pending {
            inner.load_progress(Some(&p));
        }
        inner.post(Event::GameLoaded {
            title,
            summary,
            hardcore: inner.hardcore(),
        });
    } else if result == ffi::UNKNOWN_GAME {
        inner.state.set(State::NoSet);
        let hash = inner.hash.borrow().clone();
        inner.post(Event::UnknownGame { hash });
    } else if result == ffi::NO_SIGN_IN || inner.state.get() == State::SignedOut {
        // The sign-in failed and has said so: one notice is enough.
        inner.state.set(State::SignedOut);
        inner.log(&format!("no achievements without the sign-in: {message}"));
    } else {
        inner.state.set(State::SignedIn);
        inner.post(Event::LoadFailed { message });
    }
}

extern "C" fn log(message: *const c_char, _ud: *mut c_void) {
    // SAFETY: the string is valid for the call.
    eprintln!("cheevos: rc: {}", unsafe { string(message) });
}

/// A session with the service. One per run, on the emulation thread.
pub struct Client {
    /// The callbacks' state, boxed and leaked to the shim as its `ud`, freed in Drop.
    inner: NonNull<Inner>,
}

impl Client {
    /// Starts a session: the network thread, and rcheevos as `settings` say. `app` names the
    /// emulator and the system it runs on for the service's User-Agent, `name/version
    /// (system)`; rcheevos' own part follows it. `verbose` logs every request and rcheevos'
    /// own messages. Fails when `curl`, which carries the requests, is not there.
    pub fn start(app: &str, settings: Settings, verbose: bool) -> Result<Client, String> {
        let curl = curl_path().ok_or("no curl to reach the service with")?;
        let (jobs, incoming) = channel();
        let (answers, done) = channel();
        let inner = NonNull::from(Box::leak(Box::new(Inner {
            raw: Cell::new(std::ptr::null_mut()),
            memory: Cell::new(std::ptr::null()),
            jobs,
            answers: answers.clone(),
            done,
            state: Cell::new(State::SignedOut),
            hash: RefCell::new(String::new()),
            events: RefCell::new(Vec::new()),
            pending_progress: RefCell::new(None),
            verbose,
        })));
        // From here the Box is the client's: Drop frees it.
        let client = Client { inner };
        let callbacks = ffi::Callbacks {
            read,
            request,
            event,
            login_done,
            load_done,
            log: verbose.then_some(log as ffi::LogFn),
        };
        let c_settings = ffi::Settings {
            hardcore: settings.hardcore.into(),
            encore: settings.encore.into(),
            unofficial: settings.unofficial.into(),
        };
        // SAFETY: `callbacks` and `c_settings` outlive the call (the shim copies what it
        // keeps); `ud` is the boxed Inner, which stays where it is until Drop destroys the
        // shim's client first.
        let raw = unsafe { ffi::ink_rc_create(&callbacks, &c_settings, inner.as_ptr().cast()) };
        if raw.is_null() {
            return Err("rcheevos could not make a client".into());
        }
        client.inner().raw.set(raw);
        let mut clause = [0 as c_char; 64];
        // SAFETY: the client is alive; the shim writes at most `clause.len()` bytes, NUL
        // included.
        unsafe { ffi::ink_rc_user_agent_clause(raw, clause.as_mut_ptr(), clause.len()) };
        // SAFETY: NUL-terminated within the buffer (rcheevos makes sure of it).
        let clause = unsafe { string(clause.as_ptr()) };
        let user_agent = format!("{app} {clause}");
        transport::start(curl, user_agent.clone(), incoming, answers);
        client
            .inner()
            .log(&format!("session up, user agent {user_agent}"));
        Ok(client)
    }

    fn inner(&self) -> &Inner {
        // SAFETY: the Box this points to lives until Drop, and is only ever shared.
        unsafe { self.inner.as_ref() }
    }

    /// Where the session is.
    pub fn state(&self) -> State {
        self.inner().state.get()
    }

    /// Whether hardcore is on (rcheevos can turn it off itself).
    pub fn hardcore(&self) -> bool {
        self.inner().hardcore()
    }

    /// Signs in with a token. The outcome arrives as an event, on a later frame.
    pub fn sign_in_with_token(&mut self, credentials: &Credentials) {
        let (Ok(user), Ok(token)) = (
            CString::new(credentials.username.as_str()),
            CString::new(credentials.token.as_str()),
        ) else {
            self.inner().post(Event::SignInFailed {
                token_refused: false,
                message: "a NUL in the credentials".into(),
            });
            return;
        };
        self.inner().state.set(State::SigningIn);
        // SAFETY: the client is alive; the strings are valid for the call (rcheevos copies).
        unsafe {
            ffi::ink_rc_login_with_token(self.inner().raw.get(), user.as_ptr(), token.as_ptr())
        };
    }

    /// Loads the set for the ROM with this [`hash`]. rcheevos waits for a pending sign-in
    /// itself. The outcome arrives as an event.
    pub fn load_game(&mut self, hash: &str) {
        let Ok(c) = CString::new(hash) else { return };
        *self.inner().hash.borrow_mut() = hash.to_owned();
        self.inner().state.set(State::LoadingGame);
        // SAFETY: the client is alive; the string is valid for the call (rcheevos copies).
        unsafe { ffi::ink_rc_load_game(self.inner().raw.get(), c.as_ptr()) };
    }

    /// Hands the network's answers to rcheevos, then evaluates the set against the machine:
    /// once per emulated frame, right after it ran.
    pub fn frame(&mut self, memory: &dyn Memory) {
        self.with_memory(memory, |raw| {
            // SAFETY: the client is alive, and the memory is reachable for this call.
            unsafe { ffi::ink_rc_do_frame(raw) }
        });
    }

    /// Instead of [`Client::frame`] while the game is paused: keeps the session alive.
    pub fn idle(&mut self, memory: &dyn Memory) {
        self.with_memory(memory, |raw| {
            // SAFETY: as frame().
            unsafe { ffi::ink_rc_idle(raw) }
        });
    }

    fn with_memory(&mut self, memory: &dyn Memory, call: impl FnOnce(*mut ffi::InkRc)) {
        let at: *const &dyn Memory = &memory;
        self.inner().memory.set(at.cast());
        self.drain();
        call(self.inner().raw.get());
        self.inner().memory.set(std::ptr::null());
    }

    /// Gives rcheevos every answer that has come back.
    fn drain(&mut self) {
        while let Ok(mut d) = self.inner().done.try_recv() {
            self.inner().log(&format!(
                "<- status {}, {} bytes{}",
                d.status,
                d.body.len(),
                if d.status < 0 {
                    format!(", {}", String::from_utf8_lossy(&d.body))
                } else {
                    String::new()
                }
            ));
            // rcheevos reads the body as a C string as well as by its length: a NUL after it,
            // not counted.
            d.body.push(0);
            // SAFETY: the ticket came from the shim through `request` and is answered once;
            // the body is valid for the call and NUL-terminated past its stated length.
            unsafe {
                ffi::ink_rc_respond(
                    d.ticket as *mut c_void,
                    d.status,
                    d.body.as_ptr() as *const c_char,
                    d.body.len() - 1,
                )
            };
        }
    }

    /// The game started over: the set's progress (hit counts, primed conditions) starts over
    /// too, so the jump in the machine's memory can't unlock anything.
    pub fn reset(&mut self) {
        self.inner().pending_progress.borrow_mut().take();
        // SAFETY: the client is alive.
        unsafe { ffi::ink_rc_reset(self.inner().raw.get()) };
    }

    /// The set's progress (hit counts, measured values) as bytes, to keep in a save state;
    /// `None` when no set is loaded. rcheevos' format, opaque here.
    pub fn progress(&mut self) -> Option<Vec<u8>> {
        let raw = self.inner().raw.get();
        // SAFETY: the client is alive.
        let size = unsafe { ffi::ink_rc_progress_size(raw) };
        if size == 0 {
            return None;
        }
        let mut buf = vec![0u8; size];
        // SAFETY: the client is alive; `buf` is `size` writable bytes.
        let r = unsafe { ffi::ink_rc_save_progress(raw, buf.as_mut_ptr(), buf.len()) };
        (r == ffi::OK).then_some(buf)
    }

    /// A save state was loaded: the set's progress becomes the state's ([`Client::progress`]
    /// when it was made), or starts over when it has none. While the set is still loading,
    /// it is restored once it has.
    pub fn restore_progress(&mut self, progress: Option<&[u8]>) {
        match self.state() {
            State::Playing => self.inner().load_progress(progress),
            State::SigningIn | State::LoadingGame => {
                *self.inner().pending_progress.borrow_mut() = progress.map(<[u8]>::to_vec);
            }
            _ => {}
        }
    }

    /// The events since the last call.
    pub fn take_events(&mut self) -> Vec<Event> {
        std::mem::take(&mut *self.inner().events.borrow_mut())
    }

    /// The loaded set in numbers (zeros when none is).
    pub fn summary(&self) -> Summary {
        self.inner().summary()
    }

    /// The loaded game's title, as the service has it.
    pub fn game_title(&self) -> String {
        self.inner().game_title()
    }

    /// The loaded game's id at the service; 0 when none.
    pub fn game_id(&self) -> u32 {
        // SAFETY: the client is alive.
        unsafe { ffi::ink_rc_game_id(self.inner().raw.get()) }
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        let raw = self.inner().raw.replace(std::ptr::null_mut());
        if !raw.is_null() {
            // SAFETY: created by ink_rc_create and destroyed once, here; no call follows. The
            // network thread's later answers go to a dropped channel; a ticket still in flight
            // is never answered, and its few bytes go with the process.
            unsafe {
                ffi::ink_rc_unload_game(raw);
                ffi::ink_rc_destroy(raw);
            }
        }
        while let Ok(d) = self.inner().done.try_recv() {
            // SAFETY: an unanswered ticket is ours to free, once.
            unsafe { ffi::ink_rc_discard(d.ticket as *mut c_void) };
        }
        // SAFETY: the Box leaked in start(); the shim, the only other holder, is gone.
        drop(unsafe { Box::from_raw(self.inner.as_ptr()) });
    }
}

/// The `curl` to talk to the service with: the first on `PATH`, else `/usr/bin/curl`.
pub fn curl_path() -> Option<PathBuf> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path)
        .chain([PathBuf::from("/usr/bin")])
        .map(|dir| dir.join("curl"))
        .find(|p| p.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rom_hashes_to_its_md5() {
        // RFC 1321's test vector for "abc".
        assert_eq!(
            hash(b"abc", false).as_deref(),
            Some("900150983cd24fb0d6963f7d28e17f72")
        );
        assert_eq!(hash(b"abc", true), hash(b"abc", false));
    }

    #[test]
    fn token_files_in_both_formats() {
        let dir = std::env::temp_dir().join(format!("inkdot-cheevos-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let two = dir.join("two");
        std::fs::write(&two, "Alan\nt0k3n\n").unwrap();
        assert_eq!(
            read_token_file(&two).unwrap(),
            Credentials {
                username: "Alan".into(),
                token: "t0k3n".into()
            }
        );
        let one = dir.join("one");
        std::fs::write(&one, "t0k3n").unwrap();
        assert_eq!(
            read_token_file(&one).unwrap(),
            Credentials {
                username: String::new(),
                token: "t0k3n".into()
            }
        );
        let none = dir.join("none");
        std::fs::write(&none, " \n").unwrap();
        assert!(read_token_file(&none).is_err());
        assert!(read_token_file(&dir.join("missing")).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_token_is_never_printed() {
        let c = Credentials {
            username: "Alan".into(),
            token: "t0k3n".into(),
        };
        let shown = format!("{c:?}");
        assert!(
            shown.contains("Alan") && !shown.contains("t0k3n"),
            "{shown}"
        );
    }

    struct Zeros;
    impl Memory for Zeros {
        fn read(&self, _: u32, buf: &mut [u8]) -> u32 {
            buf.fill(0);
            buf.len() as u32
        }
    }

    #[test]
    fn a_client_starts_and_stops() {
        if curl_path().is_none() {
            return;
        }
        let mut c = Client::start("inkdot-test/0 (test)", Settings::default(), false).unwrap();
        assert_eq!(c.state(), State::SignedOut);
        assert!(!c.hardcore(), "Casual unless asked");
        assert!(c.take_events().is_empty());
        assert_eq!(c.summary(), Summary::default());
        assert_eq!(c.game_id(), 0);
        assert_eq!(c.progress(), None, "no progress without a set");
        c.restore_progress(None);
        c.frame(&Zeros);
        c.idle(&Zeros);
        c.reset();
    }

    #[test]
    fn hardcore_when_asked() {
        if curl_path().is_none() {
            return;
        }
        let settings = Settings {
            hardcore: true,
            ..Settings::default()
        };
        let c = Client::start("inkdot-test/0 (test)", settings, false).unwrap();
        assert!(c.hardcore());
    }
}
