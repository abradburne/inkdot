# Provenance

Every source this project's code and data rest on, what was taken from it, and its licence
where that matters. The rules are in [design.md](design.md#clean-room-rules): public hardware
documentation, test ROMs and their documentation, published test vectors and general
literature; never the source of an emulator, ours included. A commit that implements a
hardware behaviour names its source section in its body. Add a row before using a new source.

## Hardware documentation

| Source | Where | Used for | Licence |
|---|---|---|---|
| Pan Docs | https://gbdev.io/pandocs/ | Memory map, I/O registers, CPU instruction set and timings, PPU modes and pixel FIFO, APU, timer, DMA/HDMA, CGB registers and double speed, MBCs and RTC, power-up state, compatibility palettes, cartridge header | CC0 (gbdev/pandocs repository) |
| Game Boy: Complete Technical Reference (Gekkio) | https://github.com/Gekkio/gb-ctr | SM83 per-M-cycle bus activity, interrupt dispatch, timer edge behaviour | CC BY-SA 4.0 |
| The Cycle-Accurate Game Boy Docs (AntonioND) | https://github.com/AntonioND/giibiiadvance/tree/master/docs | Timer, interrupt, PPU and DMA timing details (the docs only, not the emulator in the same repository) | per the document |
| The Cutting Room Floor, "Notes:Game Boy Color Bootstrap ROM" | https://tcrf.net/Notes:Game_Boy_Color_Bootstrap_ROM, read from the Internet Archive's copy of 2026-09-18 (web.archive.org/web/20260918011510) because the live site's bot check blocks scripts | The CGB's compatibility palettes: the title-checksum table and the twelve palettes chosen by buttons at boot (data, converted from its 24-bit colours) | CC BY 3.0 (TCRF's content licence) |
| Mooneye test sources (`*.s`) | https://github.com/Gekkio/mooneye-test-suite | What each test measures and its expected tables (`lcdon_timing`, `lcdon_write_timing`, `intr_2_mode0_timing_sprites`), which the PPU's timing was fitted to | MIT |
| gbdev.io resources list | https://gbdev.io/resources.html | Finding the documents and test suites above | — |

## Test ROMs and test vectors

| Source | Where | Used for | Licence |
|---|---|---|---|
| Blargg's Game Boy test ROMs | https://github.com/retrio/gb-test-roms (mirror of the original release) | cpu_instrs, instr_timing, mem_timing(-2), halt_bug, interrupt_time, dmg_sound, cgb_sound, oam_bug; pass = serial or screen text | no licence stated: fetched, not redistributed |
| Mooneye Test Suite | https://github.com/Gekkio/mooneye-test-suite | acceptance, emulator-only (MBC), misc; pass = Fibonacci registers | MIT |
| dmg-acid2, cgb-acid2 (Matt Currie) | https://github.com/mattcurrie/dmg-acid2, https://github.com/mattcurrie/cgb-acid2 | PPU, with reference images | MIT |
| Mealybug Tearoom tests | https://github.com/mattcurrie/mealybug-tearoom-tests (commit 70e88fb) | Mid-scanline PPU (stretch): the ROMs, and the expected pictures in `expected/` (DMG-blob, CPU CGB D) | MIT |
| rtc3test | https://github.com/aaaaaa123456789/rtc3test | MBC3 RTC | Unlicense |
| SM83 single-step tests | https://github.com/SingleStepTests/sm83 | Per-instruction state and bus-cycle vectors | MIT |
| SameSuite, gbmicrotest, age test ROMs | their official repositories (test ROMs only) | Stretch targets; their test ROMs and READMEs only, never an emulator's source in the same organisation | per repository |

## Reference emulator (black box)

To be chosen at the start of step 2: built locally, run, its frames and audio compared with
ours. Its source is never opened. Record here which one, the version, and how it was built.

## Other

| Source | Used for |
|---|---|
| nibOS docs (display, audio, input, device interface; `sdk/` app template) | Panel timings, pipes, input names, audio routing, brightness interface (our own project) |
| DSlot `src/emulator.rs`, `src/launch.rs` (our own launcher, not an emulator) | The description format and launch.conf levels |
| crates.io API, AUR RPC, Arch package search, GitHub search (2026-10-19) | Name collision checks |
| Roboto Condensed (Google Fonts; built in through nibkit's `menu-raster`) | The menu's text. SIL OFL 1.1 (LICENSE-OFL) |
| PKWARE APPNOTE.TXT (the zip format's specification) | Reading `.zip` games (`crates/inkdot/src/rom.rs`) |
| nibkit ([github.com/abradburne/nibkit](https://github.com/abradburne/nibkit), our own, MIT) | Screens, vblank, input names, touch, brightness, volume, paths, settings, DSlot's description format |
| rcheevos ([github.com/RetroAchievements/rcheevos](https://github.com/RetroAchievements/rcheevos), the service's client library, MIT; vendored in `crates/inkdot-cheevos/rcheevos`) | The RetroAchievements session, the ROM hash, the service's Game Boy address space (its console table, `src/rcheevos/consoleinfo.c`), and what an integration owes it (`include/rc_client.h`: progress in save states, the indicators, the User-Agent clause, the retryable transport error; the service's answers as `src/rapi` parses them, for a test): a client library, not an emulator |
| DSlot `src/achievements/set.rs`, `service.rs` (our own launcher, not an emulator) | Where badges are cached for every app (`retroachievements/badges/<name>.png` under the XDG cache) and where they are fetched from |
| DSperate `src/cheevos/` (our own DS emulator's RetroAchievements layer, not its emulation) | How the launcher's sign-in is read (`RA_TOKEN_FILE`) and what the log says, so DSlot's bug report reads the same for both |
