//! RetroAchievements for inkdot: a session with the service through rcheevos, the official
//! client library (vendored under `rcheevos/`, MIT), driven once per emulated frame.
//!
//! The player signs in once, in DSlot, which keeps the service's token in a file of its own and
//! starts a game with `RA_TOKEN_FILE` in its environment ([`read_token_file`]); inkdot signs in
//! with that token, never a password, and never writes the token anywhere. The game is
//! identified by the MD5 of its ROM, as the service defines it for the Game Boy and Game Boy
//! Color ([`hash`]). Softcore ("Casual") only: save states and fast-forward stay usable, and the
//! service is never told otherwise.
//!
//! Threads: rcheevos and everything that calls it run on the emulation thread, inside
//! [`Client::frame`] and [`Client::idle`], where the machine's memory is read ([`memory`]). The
//! network is a thread of its own ([`transport`]): a request is handed over and the answer is
//! given to rcheevos on a later frame, so a slow network never holds a frame. Nothing here reads
//! the machine from the network thread.

#![allow(clippy::missing_safety_doc)]

mod ffi;
pub mod memory;
mod transport;

use inkdot_core::GameBoy;
use std::cell::{Cell, RefCell};
use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::path::Path;
use std::sync::mpsc::{Receiver, Sender};

/// The sign-in: who, and the token that stands for them. The password is never seen here.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Credentials {
    /// The account; may be empty when the token file had only the token.
    pub username: String,
    /// The service's token for it.
    pub token: String,
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

/// The RetroAchievements identity of a ROM: 32 lowercase hex digits (the MD5 of the file, for
/// the Game Boy and the Game Boy Color alike, as rcheevos' `rc_hash` computes it). `color`
/// says which console's rule to ask for; the digest is the same.
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

/// rcheevos' version, as in the User-Agent.
pub fn rcheevos_version() -> &'static str {
    // SAFETY: the shim returns a static NUL-terminated string.
    unsafe { CStr::from_ptr(ffi::ink_rc_version()) }
        .to_str()
        .unwrap_or("0")
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

/// Something the player should be told about (logged, and shown when there is a way to).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Message {
    /// What it is.
    pub kind: Kind,
    /// The line: an achievement's title, or what happened.
    pub text: String,
    /// The line under it: the achievement's description, or why.
    pub detail: String,
    /// An unlock's value; 0 otherwise.
    pub points: u32,
}

/// What a message is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// An achievement was earned.
    Unlock,
    /// Something to know.
    Info,
    /// Something went wrong (not fatal: play goes on).
    Problem,
}

/// The loaded set in numbers.
pub type Summary = ffi::Summary;

/// "Warning" achievements, which the service excludes from a game's set; rcheevos still reports
/// them.
const WARNING_ACHIEVEMENT_ID: u32 = 101_000_001;

/// What the callbacks reach: the machine (only during a frame), the channels, the messages.
struct Inner {
    raw: Cell<*mut ffi::InkRc>,
    /// The machine, while [`Client::frame`] or [`Client::idle`] runs; null otherwise, when a
    /// read answers zero and unbacked.
    gb: Cell<*const GameBoy>,
    jobs: Sender<transport::Job>,
    done: Receiver<transport::Done>,
    state: Cell<State>,
    hash: RefCell<String>,
    messages: RefCell<Vec<Message>>,
    verbose: bool,
}

impl Inner {
    fn post(&self, kind: Kind, text: impl Into<String>, detail: impl Into<String>, points: u32) {
        self.messages.borrow_mut().push(Message {
            kind,
            text: text.into(),
            detail: detail.into(),
            points,
        });
    }

    fn log(&self, line: &str) {
        if self.verbose {
            eprintln!("cheevos: {line}");
        }
    }

    fn c_str<'a>(&self, p: *const c_char) -> &'a str {
        if p.is_null() {
            return "";
        }
        // SAFETY: the shim passes NUL-terminated strings valid for the call (the caller's
        // lifetime is the call's).
        unsafe { CStr::from_ptr(p) }.to_str().unwrap_or("")
    }

    fn summary(&self) -> Summary {
        let mut s = Summary::default();
        // SAFETY: the client is alive while `raw` is set; `s` is a valid out-pointer.
        unsafe { ffi::ink_rc_get_summary(self.raw.get(), &mut s) };
        s
    }

    fn game_title(&self) -> String {
        // SAFETY: the client is alive while `raw` is set; the string is valid until the next
        // call into the client, and copied before one.
        unsafe { self.c_str(ffi::ink_rc_game_title(self.raw.get())) }.to_owned()
    }
}

/// `ud` is the `Inner` the client owns, alive for as long as rcheevos can call back.
unsafe fn inner<'a>(ud: *mut c_void) -> &'a Inner {
    &*(ud as *const Inner)
}

extern "C" fn read(address: u32, buffer: *mut u8, num_bytes: u32, ud: *mut c_void) -> u32 {
    // SAFETY: rcheevos gives `num_bytes` of writable `buffer`; `ud` is the client's Inner.
    let (inner, buf) = unsafe {
        (
            inner(ud),
            std::slice::from_raw_parts_mut(buffer, num_bytes as usize),
        )
    };
    let gb = inner.gb.get();
    if gb.is_null() {
        buf.fill(0);
        return 0;
    }
    // SAFETY: `gb` is set only for the duration of frame()/idle(), by the caller that holds
    // the &GameBoy, and rcheevos only reads memory inside those calls.
    let gb = unsafe { &*gb };
    memory::read(gb, address, buf)
}

extern "C" fn request(
    url: *const c_char,
    post_data: *const c_char,
    content_type: *const c_char,
    ticket: *mut c_void,
    ud: *mut c_void,
) {
    // SAFETY: `ud` is the client's Inner; the strings are valid for the call.
    let inner = unsafe { inner(ud) };
    let url = inner.c_str(url).to_owned();
    let post = (!post_data.is_null()).then(|| {
        (
            inner.c_str(content_type).to_owned(),
            inner.c_str(post_data).to_owned(),
        )
    });
    inner.log(&format!(
        "-> {url}{}",
        if post.is_some() { " (POST)" } else { "" }
    ));
    let job = transport::Job {
        url,
        post,
        ticket: ticket as usize,
    };
    if inner.jobs.send(job).is_err() {
        // No network thread: the request is dropped, and rcheevos' own timeout says so.
        // SAFETY: the ticket is ours to free, once.
        unsafe { ffi::ink_rc_discard(ticket) };
    }
}

extern "C" fn event(
    kind: u32,
    id: u32,
    title: *const c_char,
    description: *const c_char,
    points: u32,
    message: *const c_char,
    ud: *mut c_void,
) {
    // SAFETY: `ud` is the client's Inner; the strings are valid for the call.
    let inner = unsafe { inner(ud) };
    let title = inner.c_str(title);
    let description = inner.c_str(description);
    let message = inner.c_str(message);
    match kind {
        ffi::EVENT_ACHIEVEMENT_TRIGGERED if id >= WARNING_ACHIEVEMENT_ID => {
            inner.log(&format!("ignoring warning achievement {id}: {title}"));
        }
        ffi::EVENT_ACHIEVEMENT_TRIGGERED => {
            let text = if title.is_empty() {
                "Achievement unlocked".to_owned()
            } else {
                title.to_owned()
            };
            inner.post(Kind::Unlock, text, description, points);
        }
        ffi::EVENT_GAME_COMPLETED => {
            inner.post(Kind::Info, "All achievements earned", inner.game_title(), 0);
        }
        ffi::EVENT_SUBSET_COMPLETED => inner.post(Kind::Info, "Subset completed", "", 0),
        ffi::EVENT_SERVER_ERROR => {
            inner.post(Kind::Problem, "RetroAchievements error", message, 0);
        }
        ffi::EVENT_DISCONNECTED => inner.post(
            Kind::Problem,
            "RetroAchievements offline",
            "unlocks will be sent when the connection returns",
            0,
        ),
        ffi::EVENT_RECONNECTED => inner.post(
            Kind::Info,
            "RetroAchievements reconnected",
            "pending unlocks sent",
            0,
        ),
        ffi::EVENT_RESET => inner.log("ignoring a reset event; hardcore is not played"),
        _ => {}
    }
}

extern "C" fn login_done(result: c_int, error_message: *const c_char, ud: *mut c_void) {
    // SAFETY: `ud` is the client's Inner; the string is valid for the call.
    let inner = unsafe { inner(ud) };
    if result == ffi::RC_OK {
        inner.state.set(State::SignedIn);
        // SAFETY: the client is alive; the name is copied before the next call into it.
        let name = unsafe { inner.c_str(ffi::ink_rc_display_name(inner.raw.get())) }.to_owned();
        inner.post(Kind::Info, "Signed in to RetroAchievements", name, 0);
    } else {
        inner.state.set(State::SignedOut);
        let why = inner.c_str(error_message);
        inner.post(
            Kind::Problem,
            "RetroAchievements sign-in failed",
            if why.is_empty() {
                "sign-in failed"
            } else {
                why
            },
            0,
        );
    }
}

extern "C" fn load_done(result: c_int, error_message: *const c_char, ud: *mut c_void) {
    // SAFETY: `ud` is the client's Inner; the string is valid for the call.
    let inner = unsafe { inner(ud) };
    if result == ffi::RC_OK {
        let title = inner.game_title();
        let s = inner.summary();
        if s.total == 0 {
            // Nothing has gone wrong and there is nothing to do: the log only.
            inner.state.set(State::EmptySet);
            inner.log(&format!(
                "{title} is known but has no published achievements"
            ));
        } else {
            inner.state.set(State::Playing);
            let text = if title.is_empty() {
                "Achievements loaded".to_owned()
            } else {
                title
            };
            inner.post(Kind::Info, text, "achievements active", 0);
            inner.log(&format!(
                "{} achievements, {} unlocked, {} unsupported, {} of {} points",
                s.total, s.unlocked, s.unsupported, s.points_earned, s.points
            ));
        }
    } else if result == ffi::RC_NO_GAME_LOADED {
        inner.state.set(State::NoSet);
        // The hash names a dump, not a game: a legitimate ROM can still fail to match.
        let hash = inner.hash.borrow();
        inner.post(
            Kind::Problem,
            "No achievements for this ROM",
            if hash.is_empty() {
                "RetroAchievements does not recognise this dump".to_owned()
            } else {
                format!("RetroAchievements does not recognise this dump (hash {hash})")
            },
            0,
        );
    } else {
        inner.state.set(State::SignedIn);
        let why = inner.c_str(error_message);
        inner.post(
            Kind::Problem,
            "Could not load achievements",
            if why.is_empty() {
                "could not load achievements"
            } else {
                why
            },
            0,
        );
    }
}

extern "C" fn log(message: *const c_char, ud: *mut c_void) {
    // SAFETY: `ud` is the client's Inner; the string is valid for the call.
    let inner = unsafe { inner(ud) };
    eprintln!("cheevos: rc: {}", inner.c_str(message));
}

/// A session with the service. One per run, on the emulation thread.
pub struct Client {
    inner: Box<Inner>,
}

impl Client {
    /// Starts a session: the network thread, and rcheevos with hardcore off. `verbose` logs
    /// every request and rcheevos' own messages. Fails when `curl`, which carries the
    /// requests, is not there.
    pub fn start(verbose: bool) -> Result<Client, String> {
        if !curl_available() {
            return Err("no curl to reach the service with".into());
        }
        let user_agent = format!(
            "inkdot/{} rcheevos/{}",
            env!("CARGO_PKG_VERSION"),
            rcheevos_version()
        );
        let (jobs, done) = transport::start(user_agent.clone());
        let inner = Box::new(Inner {
            raw: Cell::new(std::ptr::null_mut()),
            gb: Cell::new(std::ptr::null()),
            jobs,
            done,
            state: Cell::new(State::SignedOut),
            hash: RefCell::new(String::new()),
            messages: RefCell::new(Vec::new()),
            verbose,
        });
        let callbacks = ffi::Callbacks {
            read,
            request,
            event,
            login_done,
            load_done,
            log: verbose.then_some(log as ffi::LogFn),
        };
        let ud = &*inner as *const Inner as *mut c_void;
        // SAFETY: `callbacks` outlives the call (the shim copies it); `ud` is the boxed Inner,
        // which stays where it is for the client's life.
        let raw = unsafe { ffi::ink_rc_create(&callbacks, ud) };
        if raw.is_null() {
            return Err("rcheevos could not make a client".into());
        }
        inner.raw.set(raw);
        inner.log(&format!("session up, user agent {user_agent}"));
        Ok(Client { inner })
    }

    /// Where the session is.
    pub fn state(&self) -> State {
        self.inner.state.get()
    }

    /// Signs in with a token. The outcome arrives as a message, on a later frame.
    pub fn sign_in_with_token(&mut self, credentials: &Credentials) {
        let (Ok(user), Ok(token)) = (
            CString::new(credentials.username.as_str()),
            CString::new(credentials.token.as_str()),
        ) else {
            self.inner.post(
                Kind::Problem,
                "RetroAchievements sign-in failed",
                "a NUL in the credentials",
                0,
            );
            return;
        };
        self.inner.state.set(State::SigningIn);
        // SAFETY: the client is alive; the strings are valid for the call (rcheevos copies).
        unsafe {
            ffi::ink_rc_login_with_token(self.inner.raw.get(), user.as_ptr(), token.as_ptr())
        };
    }

    /// Loads the set for the ROM with this [`hash`]. rcheevos waits for a pending sign-in
    /// itself. The outcome arrives as a message.
    pub fn load_game(&mut self, hash: &str) {
        let Ok(c) = CString::new(hash) else { return };
        *self.inner.hash.borrow_mut() = hash.to_owned();
        self.inner.state.set(State::LoadingGame);
        // SAFETY: the client is alive; the string is valid for the call (rcheevos copies).
        unsafe { ffi::ink_rc_load_game(self.inner.raw.get(), c.as_ptr()) };
    }

    /// Hands the network's answers to rcheevos, then evaluates the set against the machine:
    /// once per emulated frame, right after it ran.
    pub fn frame(&mut self, gb: &GameBoy) {
        self.with_machine(gb, |raw| {
            // SAFETY: the client is alive, and the machine is reachable for this call.
            unsafe { ffi::ink_rc_do_frame(raw) }
        });
    }

    /// Instead of [`Client::frame`] while the game is paused: keeps the session alive.
    pub fn idle(&mut self, gb: &GameBoy) {
        self.with_machine(gb, |raw| {
            // SAFETY: as frame().
            unsafe { ffi::ink_rc_idle(raw) }
        });
    }

    fn with_machine(&mut self, gb: &GameBoy, call: impl FnOnce(*mut ffi::InkRc)) {
        self.inner.gb.set(gb as *const GameBoy);
        self.drain();
        call(self.inner.raw.get());
        self.inner.gb.set(std::ptr::null());
    }

    /// Gives rcheevos every answer that has come back.
    fn drain(&mut self) {
        while let Ok(mut d) = self.inner.done.try_recv() {
            self.inner.log(&format!(
                "<- status {}, {} bytes{}",
                d.status,
                d.body.len(),
                d.error
                    .as_deref()
                    .map(|e| format!(", {e}"))
                    .unwrap_or_default()
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

    /// The game started over, or a state was loaded: the set's progress (hit counts, primed
    /// conditions) starts over too, so a jump in the machine's memory can't unlock anything.
    pub fn reset(&mut self) {
        // SAFETY: the client is alive.
        unsafe { ffi::ink_rc_reset(self.inner.raw.get()) };
    }

    /// The messages since the last call.
    pub fn take_messages(&mut self) -> Vec<Message> {
        std::mem::take(&mut *self.inner.messages.borrow_mut())
    }

    /// The loaded set in numbers (zeros when none is).
    pub fn summary(&self) -> Summary {
        self.inner.summary()
    }

    /// The loaded game's title, as the service has it.
    pub fn game_title(&self) -> String {
        self.inner.game_title()
    }

    /// The loaded game's id at the service; 0 when none.
    pub fn game_id(&self) -> u32 {
        // SAFETY: the client is alive.
        unsafe { ffi::ink_rc_game_id(self.inner.raw.get()) }
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        // SAFETY: created by ink_rc_create and destroyed once, here; no call follows. The
        // network thread's later answers go to a dropped channel; a ticket still in flight is
        // never answered, and its few bytes go with the process.
        unsafe {
            ffi::ink_rc_unload_game(self.inner.raw.get());
            ffi::ink_rc_destroy(self.inner.raw.get());
        }
        self.inner.raw.set(std::ptr::null_mut());
        while let Ok(d) = self.inner.done.try_recv() {
            // SAFETY: an answered-nothing ticket is ours to free, once.
            unsafe { ffi::ink_rc_discard(d.ticket as *mut c_void) };
        }
    }
}

/// Whether `curl` is there to talk to the service with.
pub fn curl_available() -> bool {
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path)
        .chain([std::path::PathBuf::from("/usr/bin")])
        .any(|dir| dir.join("curl").is_file())
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
    fn a_client_starts_and_stops() {
        if !curl_available() {
            return;
        }
        let mut c = Client::start(false).unwrap();
        assert_eq!(c.state(), State::SignedOut);
        assert!(c.take_messages().is_empty());
        assert_eq!(c.summary().total, 0);
        assert_eq!(c.game_id(), 0);
        c.reset();
    }
}
