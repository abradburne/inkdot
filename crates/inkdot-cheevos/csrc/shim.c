/* SPDX-License-Identifier: MIT OR Apache-2.0 */
#include "shim.h"

#include <stdlib.h>
#include <string.h>

#include "rc_api_request.h"
#include "rc_client.h"
#include "rc_consoles.h"
#include "rc_error.h"
#include "rc_hash.h"
#include "rc_version.h"

/* The two answers the transport gives that are not HTTP statuses are rcheevos' own. */
_Static_assert(INK_RC_NOT_REACHED == RC_API_SERVER_RESPONSE_RETRYABLE_CLIENT_ERROR,
               "INK_RC_NOT_REACHED is rcheevos' retryable client error");
_Static_assert(INK_RC_CANNOT_ASK == RC_API_SERVER_RESPONSE_CLIENT_ERROR,
               "INK_RC_CANNOT_ASK is rcheevos' client error");

struct ink_rc {
  rc_client_t* client;
  ink_rc_callbacks cb;
  void* ud;
};

/* A request in flight: what rcheevos wants called with the answer. */
typedef struct ticket {
  rc_client_server_callback_t callback;
  void* callback_data;
} ticket;

static ink_rc* self_of(const rc_client_t* client) {
  return (ink_rc*)rc_client_get_userdata(client);
}

static const char* or_empty(const char* s) {
  return s ? s : "";
}

static uint32_t read_memory(uint32_t address, uint8_t* buffer, uint32_t num_bytes, rc_client_t* client) {
  ink_rc* rc = self_of(client);
  if (!rc) {
    memset(buffer, 0, num_bytes);
    return 0;
  }
  return rc->cb.read(address, buffer, num_bytes, rc->ud);
}

static void server_call(const rc_api_request_t* request, rc_client_server_callback_t callback,
                        void* callback_data, rc_client_t* client) {
  ink_rc* rc = self_of(client);
  if (!rc || !request) return;
  ticket* t = (ticket*)calloc(1, sizeof *t);
  if (!t) return;
  t->callback = callback;
  t->callback_data = callback_data;
  rc->cb.request(request->url, request->post_data, request->content_type, t, rc->ud);
}

/* rcheevos' event type as the shim's; 0 for one the shim does not pass on (the scoreboard,
 * which comes only in hardcore and needs a struct of its own). */
static uint32_t event_type(uint32_t type) {
  switch (type) {
    case RC_CLIENT_EVENT_ACHIEVEMENT_TRIGGERED: return INK_RC_EVENT_UNLOCKED;
    case RC_CLIENT_EVENT_LEADERBOARD_STARTED: return INK_RC_EVENT_LEADERBOARD_STARTED;
    case RC_CLIENT_EVENT_LEADERBOARD_FAILED: return INK_RC_EVENT_LEADERBOARD_FAILED;
    case RC_CLIENT_EVENT_LEADERBOARD_SUBMITTED: return INK_RC_EVENT_LEADERBOARD_SUBMITTED;
    case RC_CLIENT_EVENT_ACHIEVEMENT_CHALLENGE_INDICATOR_SHOW: return INK_RC_EVENT_CHALLENGE_SHOW;
    case RC_CLIENT_EVENT_ACHIEVEMENT_CHALLENGE_INDICATOR_HIDE: return INK_RC_EVENT_CHALLENGE_HIDE;
    case RC_CLIENT_EVENT_ACHIEVEMENT_PROGRESS_INDICATOR_SHOW: return INK_RC_EVENT_PROGRESS_SHOW;
    case RC_CLIENT_EVENT_ACHIEVEMENT_PROGRESS_INDICATOR_HIDE: return INK_RC_EVENT_PROGRESS_HIDE;
    case RC_CLIENT_EVENT_ACHIEVEMENT_PROGRESS_INDICATOR_UPDATE: return INK_RC_EVENT_PROGRESS_UPDATE;
    case RC_CLIENT_EVENT_LEADERBOARD_TRACKER_SHOW: return INK_RC_EVENT_TRACKER_SHOW;
    case RC_CLIENT_EVENT_LEADERBOARD_TRACKER_HIDE: return INK_RC_EVENT_TRACKER_HIDE;
    case RC_CLIENT_EVENT_LEADERBOARD_TRACKER_UPDATE: return INK_RC_EVENT_TRACKER_UPDATE;
    case RC_CLIENT_EVENT_RESET: return INK_RC_EVENT_RESET;
    case RC_CLIENT_EVENT_GAME_COMPLETED: return INK_RC_EVENT_GAME_COMPLETED;
    case RC_CLIENT_EVENT_SUBSET_COMPLETED: return INK_RC_EVENT_SUBSET_COMPLETED;
    case RC_CLIENT_EVENT_SERVER_ERROR: return INK_RC_EVENT_SERVER_ERROR;
    case RC_CLIENT_EVENT_DISCONNECTED: return INK_RC_EVENT_DISCONNECTED;
    case RC_CLIENT_EVENT_RECONNECTED: return INK_RC_EVENT_RECONNECTED;
    default: return 0;
  }
}

static void on_event(const rc_client_event_t* event, rc_client_t* client) {
  ink_rc* rc = self_of(client);
  if (!rc || !event) return;
  ink_rc_event e;
  ink_rc_achievement a;
  ink_rc_leaderboard l;
  ink_rc_tracker t;
  memset(&e, 0, sizeof e);
  e.type = event_type(event->type);
  if (!e.type) return;
  e.subset_title = event->subset ? or_empty(event->subset->title) : "";
  e.error_api = event->server_error ? or_empty(event->server_error->api) : "";
  e.error_message = event->server_error ? or_empty(event->server_error->error_message) : "";
  if (event->achievement) {
    const rc_client_achievement_t* x = event->achievement;
    memset(&a, 0, sizeof a);
    a.id = x->id;
    a.points = x->points;
    a.title = or_empty(x->title);
    a.description = or_empty(x->description);
    a.badge_name = x->badge_name;
    a.badge_url = or_empty(x->badge_url);
    a.measured_progress = x->measured_progress;
    a.measured_percent = x->measured_percent;
    e.achievement = &a;
  }
  if (event->leaderboard) {
    const rc_client_leaderboard_t* x = event->leaderboard;
    memset(&l, 0, sizeof l);
    l.id = x->id;
    l.title = or_empty(x->title);
    l.description = or_empty(x->description);
    l.tracker_value = or_empty(x->tracker_value);
    e.leaderboard = &l;
  }
  if (event->leaderboard_tracker) {
    memset(&t, 0, sizeof t);
    t.id = event->leaderboard_tracker->id;
    t.display = event->leaderboard_tracker->display;
    e.tracker = &t;
  }
  rc->cb.event(&e, rc->ud);
}

static void on_log(const char* message, const rc_client_t* client) {
  ink_rc* rc = self_of(client);
  if (rc && rc->cb.log) rc->cb.log(or_empty(message), rc->ud);
}

/* An rcheevos result as the shim's. */
static int outcome(int result) {
  switch (result) {
    case RC_OK: return INK_RC_OK;
    case RC_NO_GAME_LOADED: return INK_RC_UNKNOWN_GAME;
    case RC_INVALID_CREDENTIALS:
    case RC_EXPIRED_TOKEN:
    case RC_ACCESS_DENIED: return INK_RC_BAD_TOKEN;
    case RC_LOGIN_REQUIRED: return INK_RC_NO_SIGN_IN;
    default: return INK_RC_FAILED;
  }
}

static void login_done(int result, const char* error_message, rc_client_t* client, void* userdata) {
  ink_rc* rc = self_of(client);
  (void)userdata;
  if (rc) rc->cb.login_done(outcome(result), error_message, rc->ud);
}

static void load_done(int result, const char* error_message, rc_client_t* client, void* userdata) {
  ink_rc* rc = self_of(client);
  (void)userdata;
  if (rc) rc->cb.load_done(outcome(result), error_message, rc->ud);
}

ink_rc* ink_rc_create(const ink_rc_callbacks* callbacks, const ink_rc_settings* settings, void* ud) {
  ink_rc* rc = (ink_rc*)calloc(1, sizeof *rc);
  if (!rc) return NULL;
  rc->cb = *callbacks;
  rc->ud = ud;
  rc->client = rc_client_create(read_memory, server_call);
  if (!rc->client) {
    free(rc);
    return NULL;
  }
  rc_client_set_userdata(rc->client, rc);
  /* rc_client defaults hardcore on; the caller says. */
  rc_client_set_hardcore_enabled(rc->client, settings->hardcore != 0);
  rc_client_set_encore_mode_enabled(rc->client, settings->encore != 0);
  rc_client_set_unofficial_enabled(rc->client, settings->unofficial != 0);
  /* The console's memory is only read inside do_frame / idle, on the emulation thread. */
  rc_client_set_allow_background_memory_reads(rc->client, 0);
  rc_client_set_event_handler(rc->client, on_event);
  if (rc->cb.log) rc_client_enable_logging(rc->client, RC_CLIENT_LOG_LEVEL_VERBOSE, on_log);
  return rc;
}

void ink_rc_destroy(ink_rc* rc) {
  if (!rc) return;
  rc_client_destroy(rc->client);
  free(rc);
}

void ink_rc_respond(void* ticket_ptr, int http_status, const char* body, size_t body_length) {
  ticket* t = (ticket*)ticket_ptr;
  if (!t) return;
  rc_api_server_response_t response;
  memset(&response, 0, sizeof response);
  response.body = body ? body : "";
  response.body_length = body ? body_length : 0;
  response.http_status_code = http_status;
  if (t->callback) t->callback(&response, t->callback_data);
  free(t);
}

void ink_rc_discard(void* ticket_ptr) {
  free(ticket_ptr);
}

void ink_rc_login_with_token(ink_rc* rc, const char* username, const char* token) {
  rc_client_begin_login_with_token(rc->client, username, token, login_done, NULL);
}

void ink_rc_load_game(ink_rc* rc, const char* hash) {
  rc_client_begin_load_game(rc->client, hash, load_done, NULL);
}

void ink_rc_unload_game(ink_rc* rc) {
  rc_client_unload_game(rc->client);
}

void ink_rc_do_frame(ink_rc* rc) {
  rc_client_do_frame(rc->client);
}

void ink_rc_idle(ink_rc* rc) {
  rc_client_idle(rc->client);
}

void ink_rc_reset(ink_rc* rc) {
  rc_client_reset(rc->client);
}

size_t ink_rc_progress_size(ink_rc* rc) {
  if (!rc_client_is_game_loaded(rc->client)) return 0;
  return rc_client_progress_size(rc->client);
}

int ink_rc_save_progress(ink_rc* rc, uint8_t* buffer, size_t size) {
  if (!rc_client_is_game_loaded(rc->client) || !buffer) return INK_RC_FAILED;
  return rc_client_serialize_progress_sized(rc->client, buffer, size) == RC_OK ? INK_RC_OK : INK_RC_FAILED;
}

int ink_rc_load_progress(ink_rc* rc, const uint8_t* buffer, size_t size) {
  if (!rc_client_is_game_loaded(rc->client)) return INK_RC_FAILED;
  if (buffer && rc_client_deserialize_progress_sized(rc->client, buffer, size) == RC_OK) return INK_RC_OK;
  /* None, or not progress: everything starts over (rcheevos' own answer to NULL). */
  rc_client_deserialize_progress_sized(rc->client, NULL, 0);
  return buffer ? INK_RC_FAILED : INK_RC_OK;
}

size_t ink_rc_user_agent_clause(ink_rc* rc, char* out, size_t size) {
  if (!out || size == 0) return 0;
  return rc_client_get_user_agent_clause(rc->client, out, size);
}

const char* ink_rc_display_name(const ink_rc* rc) {
  const rc_client_user_t* u = rc_client_get_user_info(rc->client);
  return u ? u->display_name : NULL;
}

const char* ink_rc_game_title(const ink_rc* rc) {
  const rc_client_game_t* g = rc_client_get_game_info(rc->client);
  return g ? g->title : NULL;
}

uint32_t ink_rc_game_id(const ink_rc* rc) {
  const rc_client_game_t* g = rc_client_get_game_info(rc->client);
  return g ? g->id : 0;
}

void ink_rc_get_summary(const ink_rc* rc, ink_rc_summary* out) {
  memset(out, 0, sizeof *out);
  rc_client_user_game_summary_t s;
  memset(&s, 0, sizeof s);
  rc_client_get_user_game_summary(rc->client, &s);
  out->total = s.num_core_achievements;
  out->unlocked = s.num_unlocked_achievements;
  out->unsupported = s.num_unsupported_achievements;
  out->points = s.points_core;
  out->points_earned = s.points_unlocked;
  out->unofficial = s.num_unofficial_achievements;
}

int ink_rc_hardcore(const ink_rc* rc) {
  return rc_client_get_hardcore_enabled(rc->client);
}

int ink_rc_hash_gameboy(char out[33], const uint8_t* rom, size_t rom_size, int color) {
  rc_hash_iterator_t it;
  /* A synthetic name: its extension picks the console's path; the bytes come from the buffer,
   * never a file. */
  rc_hash_initialize_iterator(&it, color ? "game.gbc" : "game.gb", rom, rom_size);
  out[0] = '\0';
  const int ok = rc_hash_generate(out, color ? RC_CONSOLE_GAMEBOY_COLOR : RC_CONSOLE_GAMEBOY, &it);
  rc_hash_destroy_iterator(&it);
  if (!ok || out[0] == '\0') {
    out[0] = '\0';
    return 0;
  }
  return 1;
}

const char* ink_rc_version(void) {
  return RCHEEVOS_VERSION_STRING;
}

void ink_rc_sizes(size_t out[7]) {
  out[0] = sizeof(ink_rc_event);
  out[1] = sizeof(ink_rc_achievement);
  out[2] = sizeof(ink_rc_leaderboard);
  out[3] = sizeof(ink_rc_tracker);
  out[4] = sizeof(ink_rc_callbacks);
  out[5] = sizeof(ink_rc_settings);
  out[6] = sizeof(ink_rc_summary);
}
