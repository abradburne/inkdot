# Vendored rcheevos

Upstream: <https://github.com/RetroAchievements/rcheevos>, branch `master`, commit
`2ad0b8672f68a48148620164510b963039e49eb1` (2026-07-23), version 12.4.0, MIT (`LICENSE`).
Taken verbatim: `include/`, `src/`, `LICENSE`; the two `.natvis` files (Visual Studio debugger
views) are left out. Nothing here is modified: the achievement logic has to agree with the
service's, and a local change to the condition evaluator or the hash would be a bug that never
shows locally. What inkdot needs done differently is done in `../csrc/shim.c`, through the
callbacks rcheevos provides.

## What is compiled

`../build.rs` builds, with `RC_HASH_NO_DISC`, `RC_HASH_NO_ENCRYPTED`, `RC_HASH_NO_ZIP` and
`RC_DISABLE_LUA`:

| files | why |
|---|---|
| `src/rc_client.c` | the session: sign-in, the game's set, unlocks |
| `src/rapi/*.c` | the service's requests and answers |
| `src/rcheevos/*.c` | the achievement runtime (the whole directory: its units reference each other) |
| `src/rhash/hash.c`, `hash_rom.c`, `md5.c` | the ROM's identity (for GB and GBC, the MD5 of the file) |
| `src/rc_util.c`, `rc_compat.c`, `rc_version.c` | what those need |

Not built: the disc, encrypted-image and zip hashing (consoles inkdot never asks about; a zipped
game is unpacked by inkdot before it is hashed), `rc_client_raintegration.c` (Windows authoring
toolkit), `rc_client_external.c` and `rc_libretro.c`.
