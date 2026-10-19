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
