# Architecture

## The core (`crates/inkdot-core`)

```
GameBoy ── Cpu::step(&mut Hardware)       one instruction: each memory access is a Bus call
              │
              └─ Hardware (implements cpu::Bus): per M-cycle
                   ├─ tick_devices(): OAM DMA, Ppu::tick(4 dots, 2 at double speed),
                   │                 Apu::tick, Cartridge::tick (the MBC3 clock)
                   ├─ the access itself (the memory map, I/O registers)
                   └─ tick_timer(): the system counter, TIMA, serial, the DIV-APU edge
```

- **Order within an M-cycle.** The PPU, APU and DMA advance before the CPU's access, the system
  counter after it. Mooneye's `rapid_toggle` showed the second: a write to TAC acts on the counter
  as it was before the cycle's increment.
- **The CPU** fetches each opcode as the first M-cycle of its instruction and checks for interrupts
  just before. It is tested against the SM83 vectors cycle by cycle.
- **The PPU** runs a dot at a time. On each visible line the mode 2 interrupt and the OAM read lock
  come at dot 0; LY and the reported mode 2 at dot 4; the VRAM read lock at 80; mode 3 at 84. Pixel
  transfer then runs a background fetcher into a FIFO, with the first fetch done twice, SCX % 8
  pixels discarded, the window restarting the fetcher, and objects stalling it by Pan Docs' penalty
  algorithm. The mode 0 interrupt comes with the last pixel, the reported mode and the unlock a dot
  later. Writes have their own locks. LY=LYC is compared 4 dots after LY changes, and turning the
  LCD on starts line 0 at dot 8. Every one of these numbers is pinned by a Mooneye test
  (`intr_2_*_timing`, `hblank_ly_scx_timing`, `lcdon_timing`, `lcdon_write_timing`,
  `stat_lyc_onoff`, `vblank_stat_intr`); `tests/lcd_on.rs` turns the LCD-on tables into a
  bus-level regression test.
- **The APU** runs on machine time. Level changes go to `synth::Synth`, which places a band-limited
  step for each at its exact fractional sample position. The output rate is a fixed-point ratio, so
  resampling costs nothing; then the output capacitor's high-pass filter.
- **Save states**: each part has one `visit` function that both saves and loads (`state.rs`).

## The DS+ program (`crates/inkdot`)

```
main loop:  wait for the top panel's vblank (nibkit::vblank)
            sleep until vblank + period − (p99 frame cost + 1.5 ms)          pacing.rs
            read input (SDL, the touch panel)                                input.rs
            run one game frame (or 2-8 fast-forwarding, or as due at exact speed)
            sound: synthesizer rate = 48 kHz × 59.7275 × panel period, ± a slow
                   correction holding SDL's queue at 1200 frames            pacing.rs
            picture: frame → RGBA by palette or colour table, upload, shader video.rs
            the bottom screen, if anything on it changed                     ui/
            battery RAM a second after the game writes it (a thread)         saves.rs
```

The options are declared once, in `packaging/nibos/inkdot.toml`: DSlot reads it to show them,
and inkdot builds it in and parses it with the same code (nibkit's `dslot` module) to read its
command line.
