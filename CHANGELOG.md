# Changelog

## 0.1.0 (unreleased)

The first version.

- **Core**: the SM83 CPU stepped per M-cycle; a per-dot PPU with a pixel FIFO, for the DMG and
  the CGB; the APU with band-limited 48 kHz output; the timer, interrupts, OAM DMA, the CGB's VRAM
  DMA and double speed; ROM-only, MBC1 (and MBC1M multicarts), MBC2, MBC3 with its clock, MBC5;
  the CGB's compatibility palettes for DMG games; versioned save states. No boot ROM needed.
- **Accuracy**: the SM83 vectors, Blargg's CPU, timing and sound suites, Mooneye's DMG acceptance,
  CGB and MBC tests, dmg-acid2 and cgb-acid2 all pass (docs/accuracy.md).
- **DS+ program**: the game at 5× on the top screen with an optional LCD texture; four looks for
  DMG games; frames locked to the panel's refresh with the sound resampled to match, or exact
  speed; the bottom screen as a touch menu (save states with pictures, look, options, brightness,
  volume), dimmed or off while untouched; resume where the game was left; battery saves and the
  clock in DSlot's saves folder.
- **nibOS**: a PKGBUILD, and a DSlot description whose options DSlot shows per console and game.
- **SDL on the DS+**: SDL 3.4's KMSDRM driver is told to use its legacy page-flip path
  (`SDL_KMSDRM_ATOMIC=0`): its atomic one has no plane for the second panel and crashed the
  program at the bottom screen's first picture and at exit.
- **Without DSlot**: `inkdot game.gbc` or `inkdot` alone (a list of the games on the cards), its
  own settings file and saves folders.
- **Stock OS**: an Apps script and a zip for the games card (`--features stock`): one window
  across both panels, 44.1 kHz sound, the stock paths.
- **Slot CFW cards**: games in `Games/GB` and `Games/GBC` too, listed once when also in `Roms/`;
  a `.sav` slot left beside the ROM is copied into the saves folder on the first start, the
  original untouched.
- **Menu**: DSlot's design, shared through nibkit: save-state cards with a resume tile, options
  that say where their value comes from, a Connections page on nibOS, and long game names that
  wrap and shrink on the top screen.
- **Desktop**: `--features desktop`, both screens in one window, the keyboard as the controls.
- **RetroAchievements** (`--features cheevos`, on in the nibOS package): signed in through
  DSlot's Achievements tab (the token file DSlot passes as `RA_TOKEN_FILE`; never a password),
  the game identified by its ROM's MD5, its set evaluated once a frame by rcheevos over the
  service's Game Boy address space (the bus, then the Color's work-RAM and cartridge-RAM
  banks). Casual (softcore) only: save states and fast-forward stay usable. The set's progress
  goes with each save state and the resume point and comes back with it, as rcheevos asks (a
  state without any starts the set over, and so does a restart). Unlocks and the session's
  state go to the log (`cheevos:` lines) and show over the game on the top screen, in DSlot's
  style (nibkit's `menu::toast`, the one DSperate shows): a placard as play begins (the game,
  Casual, how many of its achievements you have and how many can't be earned here), each
  unlock for four seconds with its badge, the game completed, one after another (nibkit's
  toast queue; an unlock waits a moment for a badge still being fetched). A measured achievement's
  progress and each achievement that can be earned right now show at the bottom left while they
  last, nibkit's indicators (the same as DSperate's): *Progress* a panel at the top, each
  challenge an icon in a row under it, its badge alone, 60% opaque, in a slowly pulsing gold
  ring; a challenge on the achievement in progress is shown in its panel. Badges come from the
  cache DSlot fills (`~/.cache/retroachievements/badges`), fetched when missing. Built on
  nibkit 0.1.4
  (`menu::toast` and its queue, `menu::indicators`).
- **inkdot-cheevos** knows neither the Game Boy nor English, for DSlot and nibcase to use too:
  the emulator reads its memory for it through a `Memory` trait, is told what happened in
  typed `Event`s, and passes `Settings` (hardcore, encore, unofficial); the C shim hands Rust
  structs of its own, checked against the C side by a test. Network failures reach rcheevos as
  the retryable error it expects, with the reason; the User-Agent carries rcheevos' own
  clause.
