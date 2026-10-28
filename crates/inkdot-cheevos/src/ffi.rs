//! The C shim's API (`csrc/shim.h`), declared for Rust. Only opaque pointers, C strings and
//! plain integers cross: no rcheevos struct is mirrored here.

use std::ffi::{c_char, c_int, c_void};

/// A client (`struct ink_rc`): opaque.
#[repr(C)]
pub struct InkRc {
    _private: [u8; 0],
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
pub type EventFn = extern "C" fn(
    kind: u32,
    id: u32,
    title: *const c_char,
    description: *const c_char,
    points: u32,
    message: *const c_char,
    ud: *mut c_void,
);
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

/// `struct ink_rc_summary`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Summary {
    /// Achievements in the game's core set.
    pub total: u32,
    /// Of them, the ones the player has.
    pub unlocked: u32,
    /// Of them, the ones that read memory this build does not back.
    pub unsupported: u32,
    /// The set's points in all.
    pub points: u32,
    /// The points the player has.
    pub points_earned: u32,
}

/// rcheevos' results (rc_error.h) the client acts on.
pub const RC_OK: c_int = 0;
pub const RC_NO_GAME_LOADED: c_int = -29;

/// rcheevos' events (rc_client.h) the client acts on.
pub const EVENT_ACHIEVEMENT_TRIGGERED: u32 = 1;
pub const EVENT_RESET: u32 = 14;
pub const EVENT_GAME_COMPLETED: u32 = 15;
pub const EVENT_SERVER_ERROR: u32 = 16;
pub const EVENT_DISCONNECTED: u32 = 17;
pub const EVENT_RECONNECTED: u32 = 18;
pub const EVENT_SUBSET_COMPLETED: u32 = 19;

extern "C" {
    pub fn ink_rc_create(callbacks: *const Callbacks, ud: *mut c_void) -> *mut InkRc;
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
    pub fn ink_rc_display_name(rc: *const InkRc) -> *const c_char;
    pub fn ink_rc_game_title(rc: *const InkRc) -> *const c_char;
    pub fn ink_rc_game_id(rc: *const InkRc) -> u32;
    pub fn ink_rc_get_summary(rc: *const InkRc, out: *mut Summary);
    pub fn ink_rc_hash_gameboy(
        out: *mut c_char,
        rom: *const u8,
        rom_size: usize,
        color: c_int,
    ) -> c_int;
    pub fn ink_rc_version() -> *const c_char;
}
