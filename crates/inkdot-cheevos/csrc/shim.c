/* SPDX-License-Identifier: MIT OR Apache-2.0 */
#include "shim.h"

#include <stdlib.h>
#include <string.h>

#include "rc_client.h"
#include "rc_consoles.h"
#include "rc_hash.h"
#include "rc_version.h"

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

static void on_event(const rc_client_event_t* event, rc_client_t* client) {
  ink_rc* rc = self_of(client);
  if (!rc || !event) return;
  const rc_client_achievement_t* a = event->achievement;
  const char* message = NULL;
  if (event->server_error) message = event->server_error->error_message;
  rc->cb.event(event->type, a ? a->id : 0, a ? a->title : NULL, a ? a->description : NULL,
               a ? a->points : 0, message, rc->ud);
}

static void on_log(const char* message, const rc_client_t* client) {
  ink_rc* rc = self_of(client);
  if (rc && rc->cb.log) rc->cb.log(message ? message : "", rc->ud);
}

static void login_done(int result, const char* error_message, rc_client_t* client, void* userdata) {
  ink_rc* rc = self_of(client);
  (void)userdata;
  if (rc) rc->cb.login_done(result, error_message, rc->ud);
}

static void load_done(int result, const char* error_message, rc_client_t* client, void* userdata) {
  ink_rc* rc = self_of(client);
  (void)userdata;
  if (rc) rc->cb.load_done(result, error_message, rc->ud);
}

ink_rc* ink_rc_create(const ink_rc_callbacks* callbacks, void* ud) {
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
  /* rc_client defaults hardcore on. Off: save states and fast-forward stay usable, and a
   * hardcore unlock claimed with them in reach would be false. */
  rc_client_set_hardcore_enabled(rc->client, 0);
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
