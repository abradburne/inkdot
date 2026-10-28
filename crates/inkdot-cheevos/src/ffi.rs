//! The C shim's API (`csrc/shim.h`), declared for Rust: the shim's own structs and numbers,
//! never rcheevos'. [`tests`] checks the structs' sizes against the shim's.

use std::ffi::{c_char, c_int, c_void};

/// A client (`struct ink_rc`): opaque.
#[repr(C)]
pub struct InkRc {
    _private: [u8; 0],
}

/// `INK_RC_EVENT_*`.
pub const EVENT_UNLOCKED: u32 = 1;
pub const EVENT_LEADERBOARD_STARTED: u32 = 2;
pub const EVENT_LEADERBOARD_FAILED: u32 = 3;
pub const EVENT_LEADERBOARD_SUBMITTED: u32 = 4;
pub const EVENT_CHALLENGE_SHOW: u32 = 5;
pub const EVENT_CHALLENGE_HIDE: u32 = 6;
pub const EVENT_PROGRESS_SHOW: u32 = 7;
pub const EVENT_PROGRESS_HIDE: u32 = 8;
pub const EVENT_PROGRESS_UPDATE: u32 = 9;
pub const EVENT_TRACKER_SHOW: u32 = 10;
pub const EVENT_TRACKER_HIDE: u32 = 11;
pub const EVENT_TRACKER_UPDATE: u32 = 12;
pub const EVENT_RESET: u32 = 13;
pub const EVENT_GAME_COMPLETED: u32 = 14;
pub const EVENT_SUBSET_COMPLETED: u32 = 15;
pub const EVENT_SERVER_ERROR: u32 = 16;
pub const EVENT_DISCONNECTED: u32 = 17;
pub const EVENT_RECONNECTED: u32 = 18;

/// `INK_RC_*` results.
pub const OK: c_int = 0;
pub const UNKNOWN_GAME: c_int = 2;
pub const BAD_TOKEN: c_int = 3;
pub const NO_SIGN_IN: c_int = 4;

/// `INK_RC_NOT_REACHED`, rcheevos' `RC_API_SERVER_RESPONSE_RETRYABLE_CLIENT_ERROR`.
pub const NOT_REACHED: c_int = -2;

/// `struct ink_rc_achievement`.
#[repr(C)]
pub struct Achievement {
    pub id: u32,
    pub points: u32,
    pub title: *const c_char,
    pub description: *const c_char,
    pub badge_name: *const c_char,
    pub badge_url: *const c_char,
    pub measured_progress: *const c_char,
    pub measured_percent: f32,
}

/// `struct ink_rc_leaderboard`.
#[repr(C)]
pub struct Leaderboard {
    pub id: u32,
    pub title: *const c_char,
    pub description: *const c_char,
    pub tracker_value: *const c_char,
}

/// `struct ink_rc_tracker`.
#[repr(C)]
pub struct Tracker {
    pub id: u32,
    pub display: *const c_char,
}

/// `struct ink_rc_event`.
#[repr(C)]
pub struct Event {
    pub kind: u32,
    pub achievement: *const Achievement,
    pub leaderboard: *const Leaderboard,
    pub tracker: *const Tracker,
    pub subset_title: *const c_char,
    pub error_api: *const c_char,
    pub error_message: *const c_char,
}

pub type ReadFn =
    extern "C" fn(address: u32, buffer: *mut u8, num_bytes: u32, ud: *mut c_void) -> u32;
pub type RequestFn = extern "C" fn(
    url: *const c_char,
    post_data: *const c_char,
    content_type: *const c_char,
    ticket: *mut c_void,
    ud: *mut c_void,
);
pub type EventFn = extern "C" fn(event: *const Event, ud: *mut c_void);
pub type DoneFn = extern "C" fn(result: c_int, error_message: *const c_char, ud: *mut c_void);
pub type LogFn = extern "C" fn(message: *const c_char, ud: *mut c_void);

/// `struct ink_rc_callbacks`.
#[repr(C)]
pub struct Callbacks {
    pub read: ReadFn,
    pub request: RequestFn,
    pub event: EventFn,
    pub login_done: DoneFn,
    pub load_done: DoneFn,
    pub log: Option<LogFn>,
}

/// `struct ink_rc_settings`.
#[repr(C)]
pub struct Settings {
    pub hardcore: c_int,
    pub encore: c_int,
    pub unofficial: c_int,
}

/// `struct ink_rc_summary`.
#[repr(C)]
#[derive(Default)]
pub struct Summary {
    pub total: u32,
    pub unlocked: u32,
    pub unsupported: u32,
    pub points: u32,
    pub points_earned: u32,
    pub unofficial: u32,
}

extern "C" {
    pub fn ink_rc_create(
        callbacks: *const Callbacks,
        settings: *const Settings,
        ud: *mut c_void,
    ) -> *mut InkRc;
    pub fn ink_rc_destroy(rc: *mut InkRc);
    pub fn ink_rc_respond(
        ticket: *mut c_void,
        http_status: c_int,
        body: *const c_char,
        body_length: usize,
    );
    pub fn ink_rc_discard(ticket: *mut c_void);
    pub fn ink_rc_login_with_token(rc: *mut InkRc, username: *const c_char, token: *const c_char);
    pub fn ink_rc_load_game(rc: *mut InkRc, hash: *const c_char);
    pub fn ink_rc_unload_game(rc: *mut InkRc);
    pub fn ink_rc_do_frame(rc: *mut InkRc);
    pub fn ink_rc_idle(rc: *mut InkRc);
    pub fn ink_rc_reset(rc: *mut InkRc);
    pub fn ink_rc_progress_size(rc: *mut InkRc) -> usize;
    pub fn ink_rc_save_progress(rc: *mut InkRc, buffer: *mut u8, size: usize) -> c_int;
    pub fn ink_rc_load_progress(rc: *mut InkRc, buffer: *const u8, size: usize) -> c_int;
    pub fn ink_rc_user_agent_clause(rc: *mut InkRc, out: *mut c_char, size: usize) -> usize;
    pub fn ink_rc_display_name(rc: *const InkRc) -> *const c_char;
    pub fn ink_rc_game_title(rc: *const InkRc) -> *const c_char;
    pub fn ink_rc_game_id(rc: *const InkRc) -> u32;
    pub fn ink_rc_get_summary(rc: *const InkRc, out: *mut Summary);
    pub fn ink_rc_hardcore(rc: *const InkRc) -> c_int;
    pub fn ink_rc_hash_gameboy(
        out: *mut c_char,
        rom: *const u8,
        rom_size: usize,
        color: c_int,
    ) -> c_int;
    pub fn ink_rc_version() -> *const c_char;
    #[cfg(test)]
    pub fn ink_rc_sizes(out: *mut usize);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::size_of;

    #[test]
    fn the_structs_are_the_shims() {
        let mut sizes = [0usize; 7];
        // SAFETY: the shim writes seven sizes.
        unsafe { ink_rc_sizes(sizes.as_mut_ptr()) };
        assert_eq!(
            sizes,
            [
                size_of::<Event>(),
                size_of::<Achievement>(),
                size_of::<Leaderboard>(),
                size_of::<Tracker>(),
                size_of::<Callbacks>(),
                size_of::<Settings>(),
                size_of::<Summary>(),
            ]
        );
    }
}
