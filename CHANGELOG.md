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
  banks). Softcore only: save states and fast-forward stay usable; a state load or a restart
  starts the set's progress over. Unlocks and the session's state go to the log (`cheevos:`
  lines) and show over the game on the top screen for four seconds, in DSlot's style, as
  DSperate shows its own.
