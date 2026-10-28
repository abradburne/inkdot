/* SPDX-License-Identifier: MIT OR Apache-2.0
 *
 * The crate's face of rcheevos: a small C API over rc_client in structs of the shim's own, so
 * the Rust side never mirrors an rcheevos struct layout (which may change between rcheevos
 * versions) and a new kind of event adds a field here rather than changing a callback. Numbers
 * are the shim's own too (INK_RC_*), mapped from rcheevos' in shim.c. Every function here is
 * called on one thread, the emulation thread, except ink_rc_discard, which only frees.
 *
 * Every struct below is appended to, never reordered; ink_rc_sizes says how big this build's
 * are, so the Rust side can check its declarations against them.
 */
#ifndef INK_RC_SHIM_H
#define INK_RC_SHIM_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct ink_rc ink_rc;

/* What happened (rc_client's RC_CLIENT_EVENT_*, by the shim's own numbers). */
enum {
  INK_RC_EVENT_UNLOCKED = 1,            /* achievement */
  INK_RC_EVENT_LEADERBOARD_STARTED = 2, /* leaderboard */
  INK_RC_EVENT_LEADERBOARD_FAILED = 3,  /* leaderboard */
  INK_RC_EVENT_LEADERBOARD_SUBMITTED = 4, /* leaderboard, its value in tracker_value */
  INK_RC_EVENT_CHALLENGE_SHOW = 5,      /* achievement */
  INK_RC_EVENT_CHALLENGE_HIDE = 6,      /* achievement */
  INK_RC_EVENT_PROGRESS_SHOW = 7,       /* achievement, with its measured progress */
  INK_RC_EVENT_PROGRESS_HIDE = 8,       /* nothing */
  INK_RC_EVENT_PROGRESS_UPDATE = 9,     /* achievement, with its measured progress */
  INK_RC_EVENT_TRACKER_SHOW = 10,       /* tracker */
  INK_RC_EVENT_TRACKER_HIDE = 11,       /* tracker */
  INK_RC_EVENT_TRACKER_UPDATE = 12,     /* tracker */
  INK_RC_EVENT_RESET = 13,              /* the machine should be reset (hardcore was turned on) */
  INK_RC_EVENT_GAME_COMPLETED = 14,     /* every achievement of the game is earned */
  INK_RC_EVENT_SUBSET_COMPLETED = 15,   /* subset */
  INK_RC_EVENT_SERVER_ERROR = 16,       /* error_api, error_message; not retried */
  INK_RC_EVENT_DISCONNECTED = 17,       /* an unlock could not be sent and is pending */
  INK_RC_EVENT_RECONNECTED = 18         /* every pending unlock is sent */
};

/* The outcome of a sign-in or a game load. */
enum {
  INK_RC_OK = 0,
  INK_RC_FAILED = 1,    /* anything else; the message says what */
  INK_RC_UNKNOWN_GAME = 2, /* the service does not know the hash */
  INK_RC_BAD_TOKEN = 3, /* the token was refused or has expired: sign in again */
  INK_RC_NO_SIGN_IN = 4 /* a game load that needs the sign-in that failed */
};

typedef struct ink_rc_achievement {
  uint32_t id;
  uint32_t points;
  const char* title;
  const char* description;
  /* The service's name for its badge picture ("12345"), and where it is; "" when none. */
  const char* badge_name;
  const char* badge_url;
  /* How far a measured achievement is ("3/10", "45%"), "" when it isn't measured, and the same
   * as a fraction of 1. */
  const char* measured_progress;
  float measured_percent;
} ink_rc_achievement;

typedef struct ink_rc_leaderboard {
  uint32_t id;
  const char* title;
  const char* description;
  /* The value an attempt ended on, formatted (SUBMITTED). */
  const char* tracker_value;
} ink_rc_leaderboard;

typedef struct ink_rc_tracker {
  uint32_t id;
  /* The value to show, formatted. */
  const char* display;
} ink_rc_tracker;

/* One event. The pointers are NULL when the event is not about such a thing; the strings are
 * never NULL in a struct that is there ("" for none). Everything is valid for the call only. */
typedef struct ink_rc_event {
  uint32_t type;
  const ink_rc_achievement* achievement;
  const ink_rc_leaderboard* leaderboard;
  const ink_rc_tracker* tracker;
  const char* subset_title;  /* SUBSET_COMPLETED */
  const char* error_api;     /* SERVER_ERROR: the request that failed */
  const char* error_message; /* SERVER_ERROR */
} ink_rc_event;

/* Reads num_bytes of the console's memory at an address in RetroAchievements' address space
 * for the console; returns how many were backed (unbacked bytes read as zero). */
typedef uint32_t (*ink_rc_read_fn)(uint32_t address, uint8_t* buffer, uint32_t num_bytes, void* ud);
/* A request for the service: post_data and content_type are NULL for a GET. `ticket` must be
 * given back to ink_rc_respond (with the answer) or ink_rc_discard, once. */
typedef void (*ink_rc_request_fn)(const char* url, const char* post_data, const char* content_type,
                                  void* ticket, void* ud);
typedef void (*ink_rc_event_fn)(const ink_rc_event* event, void* ud);
/* The outcome of a sign-in or a game load: an INK_RC_* result and, when it failed, why. */
typedef void (*ink_rc_done_fn)(int result, const char* error_message, void* ud);
typedef void (*ink_rc_log_fn)(const char* message, void* ud);

typedef struct ink_rc_callbacks {
  ink_rc_read_fn read;
  ink_rc_request_fn request;
  ink_rc_event_fn event;
  ink_rc_done_fn login_done;
  ink_rc_done_fn load_done;
  ink_rc_log_fn log; /* NULL: rcheevos' log is dropped */
} ink_rc_callbacks;

/* How the game is played, set before a game loads. */
typedef struct ink_rc_settings {
  int hardcore;   /* no save states, rewind, slow motion or cheats; the service ranks it */
  int encore;     /* earned achievements can be earned again (not sent again) */
  int unofficial; /* the set's unpublished achievements are evaluated too */
} ink_rc_settings;

/* A client, memory read only inside ink_rc_do_frame / ink_rc_idle. NULL when rc_client could
 * not be made. */
ink_rc* ink_rc_create(const ink_rc_callbacks* callbacks, const ink_rc_settings* settings, void* ud);
void ink_rc_destroy(ink_rc* rc);

/* The answer to a request: calls rcheevos back with it and frees the ticket. A negative
 * http_status is rcheevos' RC_API_SERVER_RESPONSE_*: -2 the service was never reached and
 * rcheevos should retry (the body says why), -1 the request could not be made at all. */
void ink_rc_respond(void* ticket, int http_status, const char* body, size_t body_length);
/* Frees a ticket without answering it (after the client is gone). */
void ink_rc_discard(void* ticket);
enum {
  INK_RC_NOT_REACHED = -2, /* RC_API_SERVER_RESPONSE_RETRYABLE_CLIENT_ERROR */
  INK_RC_CANNOT_ASK = -1   /* RC_API_SERVER_RESPONSE_CLIENT_ERROR */
};

void ink_rc_login_with_token(ink_rc* rc, const char* username, const char* token);
void ink_rc_load_game(ink_rc* rc, const char* hash);
void ink_rc_unload_game(ink_rc* rc);
void ink_rc_do_frame(ink_rc* rc);
void ink_rc_idle(ink_rc* rc);
/* The game was reset: the runtime's progress (hit counts) starts over. */
void ink_rc_reset(ink_rc* rc);

/* The runtime's progress, for a save state: how many bytes it takes (0 when no game is
 * loaded), and writing it into `buffer` (INK_RC_OK, or not when it does not fit or no game is
 * loaded). */
size_t ink_rc_progress_size(ink_rc* rc);
int ink_rc_save_progress(ink_rc* rc, uint8_t* buffer, size_t size);
/* Restores progress from a save state; NULL (a state without any) starts every achievement
 * over, as a reset does. INK_RC_OK, or not when no game is loaded or the bytes are not
 * progress (then everything starts over). */
int ink_rc_load_progress(ink_rc* rc, const uint8_t* buffer, size_t size);

/* rcheevos' part of the User-Agent ("rcheevos/12.4.0") into `out`, NUL-terminated; its length. */
size_t ink_rc_user_agent_clause(ink_rc* rc, char* out, size_t size);

/* The signed-in player's name as the service shows it; NULL when not signed in. Valid until
 * the next call into the client. */
const char* ink_rc_display_name(const ink_rc* rc);
/* The loaded game's title and id; NULL / 0 when none is loaded. */
const char* ink_rc_game_title(const ink_rc* rc);
uint32_t ink_rc_game_id(const ink_rc* rc);
/* The loaded game's achievements (core set): how many, how many the player has, how many read
 * memory this build does not back, how many are unofficial, and the points in all and
 * earned. */
typedef struct ink_rc_summary {
  uint32_t total;
  uint32_t unlocked;
  uint32_t unsupported;
  uint32_t points;
  uint32_t points_earned;
  uint32_t unofficial;
} ink_rc_summary;
void ink_rc_get_summary(const ink_rc* rc, ink_rc_summary* out);
/* Whether hardcore is on (rcheevos may turn it off itself, e.g. for an unknown game). */
int ink_rc_hardcore(const ink_rc* rc);

/* The RetroAchievements identity of a Game Boy or Game Boy Color ROM (the file's MD5, as
 * rc_hash defines it for both): 32 lowercase hex characters and a NUL in `out`. Returns 0 on
 * failure. */
int ink_rc_hash_gameboy(char out[33], const uint8_t* rom, size_t rom_size, int color);

/* rcheevos' version, "12.4.0". */
const char* ink_rc_version(void);

/* The sizes of this build's structs, in this order: event, achievement, leaderboard, tracker,
 * callbacks, settings, summary. */
void ink_rc_sizes(size_t out[7]);

#ifdef __cplusplus
}
#endif

#endif
