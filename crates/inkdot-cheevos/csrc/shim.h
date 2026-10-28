/* SPDX-License-Identifier: MIT OR Apache-2.0
 *
 * inkdot's face of rcheevos: a flat C API over rc_client, so the Rust side deals in opaque
 * pointers, C strings and plain integers and never has to mirror an rcheevos struct layout
 * (which may change between rcheevos versions). Every function here is called on one thread,
 * the emulation thread, except ink_rc_discard, which only frees.
 */
#ifndef INKDOT_RC_SHIM_H
#define INKDOT_RC_SHIM_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct ink_rc ink_rc;

/* Reads num_bytes of the console's memory at an address in RetroAchievements' address space
 * for the console; returns how many were backed (unbacked bytes read as zero). */
typedef uint32_t (*ink_rc_read_fn)(uint32_t address, uint8_t* buffer, uint32_t num_bytes, void* ud);
/* A request for the service: post_data and content_type are NULL for a GET. `ticket` must be
 * given back to ink_rc_respond (with the answer) or ink_rc_discard, once. */
typedef void (*ink_rc_request_fn)(const char* url, const char* post_data, const char* content_type,
                                  void* ticket, void* ud);
/* An rc_client event (RC_CLIENT_EVENT_*): the achievement it is about (id 0 when none) and,
 * for a server error, its message. Strings are valid for the call only. */
typedef void (*ink_rc_event_fn)(uint32_t type, uint32_t id, const char* title, const char* description,
                                uint32_t points, const char* message, void* ud);
/* The outcome of a sign-in or a game load: an RC_* result (RC_OK is 0) and, when it failed,
 * why. */
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

/* A client, hardcore off, memory read only inside ink_rc_do_frame / ink_rc_idle. NULL when
 * rc_client could not be made. */
ink_rc* ink_rc_create(const ink_rc_callbacks* callbacks, void* ud);
void ink_rc_destroy(ink_rc* rc);

/* The answer to a request: calls rcheevos back with it and frees the ticket. http_status 0
 * means the service was never reached (rcheevos retries). */
void ink_rc_respond(void* ticket, int http_status, const char* body, size_t body_length);
/* Frees a ticket without answering it (after the client is gone). */
void ink_rc_discard(void* ticket);

void ink_rc_login_with_token(ink_rc* rc, const char* username, const char* token);
void ink_rc_load_game(ink_rc* rc, const char* hash);
void ink_rc_unload_game(ink_rc* rc);
void ink_rc_do_frame(ink_rc* rc);
void ink_rc_idle(ink_rc* rc);
/* The game was reset or a state loaded: the runtime's progress (hit counts) starts over. */
void ink_rc_reset(ink_rc* rc);

/* The signed-in player's name as the service shows it; NULL when not signed in. Valid until
 * the next call into the client. */
const char* ink_rc_display_name(const ink_rc* rc);
/* The loaded game's title and id; NULL / 0 when none is loaded. */
const char* ink_rc_game_title(const ink_rc* rc);
uint32_t ink_rc_game_id(const ink_rc* rc);
/* The loaded game's achievements (core set): how many, how many the player has, how many read
 * memory this build does not back, and the points in all and earned. */
typedef struct ink_rc_summary {
  uint32_t total;
  uint32_t unlocked;
  uint32_t unsupported;
  uint32_t points;
  uint32_t points_earned;
} ink_rc_summary;
void ink_rc_get_summary(const ink_rc* rc, ink_rc_summary* out);

/* The RetroAchievements identity of a Game Boy or Game Boy Color ROM (the file's MD5, as
 * rc_hash defines it for both): 32 lowercase hex characters and a NUL in `out`. Returns 0 on
 * failure. */
int ink_rc_hash_gameboy(char out[33], const uint8_t* rom, size_t rom_size, int color);

/* rcheevos' version, "12.4.0". */
const char* ink_rc_version(void);

#ifdef __cplusplus
}
#endif

#endif
