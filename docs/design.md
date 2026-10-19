# Design: a Game Boy and Game Boy Color emulator for the RG DS+ on nibOS

Status: **approved** (2026-10-19), with all seven decisions as recommended (name inkdot,
MIT OR Apache-2.0, nibkit in nibOS's `sdk/`, panel-locked timing, DMG games on a CGB by default,
the bottom screen dimmed with an "off" option, resume). Built since; where the build differs from
this document:

- The band-limited synthesis lives in the core (`synth.rs`), not the frontend: it is plain DSP,
  and the headless tests can then check the sound.
- nibkit, the shared crate, was written by the nibOS and DSlot sessions; inkdot uses it. Screen
  off is nibOS's `bl_power` knob (AGENTS.md "Screen off").
- The options are declared in the DSlot description, `packaging/nibos/inkdot.toml`, which DSlot
  shows per console and game and inkdot builds in; changes made in inkdot's own menu are kept in
  its settings file for now (whether to write DSlot's file instead is with the DSlot session).
- The DMG looks are Game Boy Color, Game Boy, Pocket, Light and plain grey (the user asked for
  the original and Pocket styles); the Color look runs the game on a CGB, the others on a DMG.

The rest of this document is the design as reviewed.

## What it is

An emulator of the Game Boy (DMG) and Game Boy Color (CGB), written from scratch in Rust, made
for one handheld: the Anbernic RG DS+ running nibOS, started from DSlot. Two parts:

- **`inkdot-core`**: the machine. Platform-free, no I/O, no dependencies, no `unsafe`; given a
  ROM and button states it produces frames, audio samples, and save data. Anything can host it
  (the DS+ frontend, a headless test runner, a future port to another handheld).
- **`inkdot`**: the DS+ frontend. The game on the top screen, the bottom touch screen as the
  emulator's own controls, frame pacing locked to the panels, low-latency input, quiet audio
  threads, and the nibOS conventions for saves, settings, brightness and volume.

Goals, in order: correct (documented hardware behaviour, measured with test ROMs), responsive
(input to photon as short as the hardware allows), smooth (no repeated or dropped frames in
the default mode), frugal (the CPU asleep most of each frame), and pleasant to use with one
hand on the touch screen. The code is meant to be read: every behaviour that isn't obvious
says which document it comes from.

Not in the first version: Super Game Boy features (borders and palettes from SGB commands),
link-cable play (serial is a stub that behaves like an unplugged cable), the Game Boy Camera,
Printer and infrared port, MBC6/MBC7/MMM01/HuC1/HuC3/TAMA5 cartridges, cheats, rewind. Each is
a self-contained later addition; [Later](#later) lists them with what they need.

## Clean-room rules

The user must own the code outright and be free to license it, including to clients who fork
it. So:

- **Sources allowed**: public hardware documentation (Pan Docs, the gbdev.io resources, the
  *Game Boy: Complete Technical Reference*, the Cycle-Accurate GB Docs), test-ROM documentation
  and the test ROMs with their expected outputs, published per-instruction test vectors, and
  general literature (DSP textbooks for audio resampling, Rust and SDL documentation).
- **Never read**: the source of any emulator or emulator component, ours included (DSperate's
  `gb_psg`, SameBoy, mGBA, Gambatte, Gearboy, binjgb, and the rest), nor ports, forum
  posts or blog posts that paste emulator code. Test ROM sources (assembly) are fine: they are
  documentation of what the hardware does, not of how an emulator does it.
- **Black box comparison is allowed**: a reference emulator built locally may be run and its
  frames and audio compared with ours, never its source opened. Which one is chosen at the
  start of step 2 and recorded in the provenance log.
- **Provenance log**: [provenance.md](provenance.md) lists every source used, what was taken
  from it (facts, test expectations), and its licence where it matters. A commit that implements
  a hardware behaviour names the document section in its body (`Source: Pan Docs, "Timer
  obscure behaviour"`).
- **Our own data**: palettes, borders, fonts, sounds, the colour-correction curve and the UI
  are made for this project (or taken from fonts with a licence that allows it, logged). No
  Nintendo boot ROM, logo or artwork ships; the emulator starts games without a boot ROM
  ([Start-up](#start-up-without-a-boot-rom)).

## Repository

```
inkdot/
  crates/
    inkdot-core/      the machine (no_std + alloc, no dependencies, #![forbid(unsafe_code)])
    inkdot/           the DS+ frontend binary (SDL2, GLES, nibkit)
    inkdot-test/      headless runner: test ROMs, frame/audio hashes, benchmarks
  assets/             palettes, borders, UI font and sounds (ours)
  packaging/nibos/    PKGBUILD, the DSlot description, the default settings
  tests/              ROM manifest (URLs + SHA-256), expected results; ROMs are fetched, not committed
  tools/              fetch-test-roms.sh, run on the device, latency and battery measurements
  docs/               design (this), provenance, accuracy (generated), architecture, using
  AGENTS.md, CONTRIBUTING.md, CHANGELOG.md, LICENSE
```

The same bar as nibOS: `cargo fmt` (100 columns), `cargo clippy --all-targets -- -D warnings`,
unit tests next to the code, `make check` (fmt, clippy, tests, the test-ROM suite when the ROMs
are present, Markdown links), a CI workflow ready for when there is a remote, dependencies
pinned in `Cargo.lock` and built `--locked`. CONTRIBUTING.md and AGENTS.md follow nibOS's:
comments say why, reference docs describe the present, one logical change per commit, no
`Signed-off-by:` added on someone's behalf (the DCO line is the author's own), agent commits
carry `Co-Authored-By:`.

## The core

### Timing model

The core is stepped by the CPU at **M-cycle** granularity (1,048,576 per second in normal
speed, twice that in CGB double speed). Every memory access the CPU makes, and every internal
cycle, first advances the rest of the machine by one M-cycle: timer, serial, DMA, then the PPU
by 4 dots (2 in double speed) and the APU by its own clock. This is how the hardware is
described (Cycle-Accurate GB Docs; Pan Docs "CPU Instruction Set" per-instruction timings), and
it makes mid-instruction effects fall out naturally: a write lands on the M-cycle it happens in,
so STAT and timer races, OAM DMA conflicts and HDMA stalls are where the tests expect them.

```
Cpu::step()                 one instruction, or one HALT/STOP cycle, or an interrupt dispatch
  └─ bus.read/write/idle    each one M-cycle:
        timer.tick()        DIV/TIMA, the falling-edge detector, the 1-cycle TIMA reload delay
        serial.tick()
        dma.tick()          OAM DMA (one byte per M-cycle, bus conflicts), HDMA/GDMA (CGB)
        ppu.tick(dots)      4 dots (2 in double speed)
        apu.tick()          frame sequencer from DIV, channels
        cart.tick()         MBC3 RTC
```

Performance: this is the simplest accurate model and cheap enough. A Game Boy needs ~4.2 M dots
a second; budgeting 15 ns a dot on a Cortex-A55 is ~60 ms of CPU a second, about 6% of one
core. The target is **≤ 2.5 ms per frame (p99) on one A55** at the clocks schedutil chooses for
it, which leaves the CPU idle for ~85% of every 16.7 ms frame. If profiling shows the per-dot
PPU dominating, the PPU can catch up lazily (run when something reads or writes it, or at the
end of the frame), without changing behaviour; the tests decide.

### SM83 CPU

- All 245 base and 256 CB-prefixed opcodes, with documented flag behaviour and per-M-cycle bus
  activity (which cycle reads, which writes, which is internal).
- Interrupts: IME, the one-instruction delay after `EI`, dispatch timing (5 M-cycles) including
  the documented case of IE being overwritten by the PC push, `HALT` and the HALT bug, `STOP`
  (and its use for the CGB speed switch), `DI`/`EI` around `HALT`.
- Undefined opcodes lock the CPU as the hardware does (reported to the frontend, which shows
  it rather than hanging silently).
- Tested with per-instruction state vectors (one JSON case per opcode × 1000 random states,
  with the expected bus cycle trace) before any test ROM runs.

### PPU (DMG and CGB)

- Modes 2/3/0/1 with the documented lengths; mode 3 length varies with SCX, the window and
  objects (Pan Docs "Rendering", mode 3 penalties), so STAT interrupt timing and the STAT
  "blocking" behaviour match the Mooneye PPU tests.
- A **pixel FIFO** renderer (background/window fetcher, object fetcher, the FIFO and mixer as
  documented in Pan Docs "Pixel FIFO"), so mid-scanline register writes (SCX, palettes, LCDC
  bits) take effect at the right pixel. One renderer for both models; the CGB adds tile
  attributes, VRAM bank 1, colour palettes (BCPS/BCPD, OCPS/OCPD with auto-increment and the
  mode 3 lock), the BG-to-OBJ priority rules, and object priority by OAM order (OPRI).
- LY=LYC, the line-153 quirk, LCD on/off (the first frame after turning on is not shown, as on
  hardware), VRAM/OAM access blocking per mode, the DMG STAT write bug.
- Output: one 160×144 frame of 15-bit colour (CGB) or 2-bit shades (DMG) per frame; the
  frontend maps it to RGB. The core never decides how colours look.

### APU

- Four channels (two pulse, one with sweep; wave; noise), the frame sequencer clocked from DIV
  (bit 4, bit 5 in double speed), length counters with the documented extra-clock quirks,
  envelopes, sweep with its negate/overflow behaviour, wave RAM access rules (DMG corruption on
  retrigger, CGB differences), NR52 power-off behaviour, PCM12/PCM34 (CGB).
- Output: each channel's level changes as they happen, in machine time. The frontend turns them
  into 48 kHz samples with **band-limited step synthesis** (a windowed-sinc step table we compute
  ourselves), which removes aliasing, and **the resampling ratio is the time-to-sample factor**
  in that synthesis, so adjusting playback speed by a fraction of a percent costs nothing and
  sounds clean. A one-pole high-pass filter models the output capacitor (Pan Docs "Audio
  details").

### Timers, interrupts, DMA, serial, joypad

- DIV as the 16-bit system counter; TIMA increments on the falling edge of the selected bit
  (so writes to DIV and TAC can tick TIMA, as documented), the overflow → reload → interrupt
  sequence over 4 T-cycles with the write races.
- OAM DMA: 160 M-cycles, OAM inaccessible to the PPU and CPU, the bus conflicts by source.
- CGB general and HBlank DMA (HDMA1–5): 16 bytes per HBlank, CPU paused, the termination and
  `FF55` read-back behaviour, interaction with HALT and double speed.
- CGB double speed (KEY1 + STOP): CPU, timer, serial, DMA at 2×; PPU, APU and HDMA rate in dots
  unchanged; the switch's pause.
- Serial: a stub that behaves like an unplugged cable (internal clock transfers complete and
  shift in `$FF`); its output bytes are also captured, which is how Blargg's tests report.
- Joypad: P1 select lines, the joypad interrupt on a high-to-low edge.

### Cartridges and saves

- Header parsing (title, CGB flag, cartridge type, ROM/RAM sizes, checksums), with the header
  checksum checked and the global checksum used as part of a game's identity.
- **ROM only, MBC1 (and MBC1M multicarts), MBC2 (built-in 512×4-bit RAM), MBC3 (+RTC, and the
  8 MB / RAM-bank-7 variant), MBC5 (+rumble bit)**, each a small type implementing one trait
  (`read`, `write`, `ram`, `rtc`), with Mooneye's emulator-only MBC tests as their spec.
- **RTC**: the five registers, latch, halt, day carry, sub-second counter; it keeps running
  while the game is off by storing the wall-clock time at save and adding the elapsed time at
  load (documented in the RTC save format, below). Tested with `rtc3test`.
- Saves: the frontend owns files; the core exposes the battery RAM and RTC state and a "dirty"
  flag. The rumble bit is reported to the frontend (the DS+ has a motor; [Later](#later)).

### Models

`Dmg` (DMG-ABC), `Cgb` (CGB-C/E), and `Cgb` running a DMG game in compatibility mode (with the
compatibility palettes). The model is chosen per game from the header and the user's settings
(a DMG game can run on either; a CGB-only game only on CGB).

### Start-up without a boot ROM

The core starts at `$0100` with the registers, I/O and memory the boot ROM leaves behind on
each model (Pan Docs "Power Up Sequence"; the Mooneye `boot_regs`, `boot_hwio` and `boot_div`
tests check exactly this). For DMG games on CGB, it applies the compatibility palette the CGB
boot ROM would choose from the title checksum (Pan Docs table), or the user's chosen palette.
No Nintendo code runs and no logo is drawn; a game starts in its first frame. A user may point
the emulator at their own boot ROM dump (setting `boot_rom`), which then runs as on hardware.

### Save states

A save state is the whole machine, serialised by the core into a versioned, tagged format:

```
"INKDOT-STATE\0" u16 version, u8 model, u32 ROM global-checksum + title
sections: [tag: 4 bytes, length: u32, payload] ...  CPU, BUS, PPU, APU, TIM, DMA, CART, RTC, ...
```

Unknown sections are skipped and missing ones are an error, so a newer build reads older
states as long as their sections still mean the same thing; a change in meaning bumps the
version and adds a converter with a test. The frontend wraps it with a thumbnail (160×144
PNG), the time it was made and the play time. Round-trip tests: run N frames, save, run M
frames, load, run M frames, and compare frames and audio bit for bit.

## Accuracy targets and test plan

Each target is a test in `inkdot-test`, run by `cargo test` when the ROMs are present
(`tools/fetch-test-roms.sh` downloads them from their official releases and checks SHA-256s;
they aren't committed, since their licences vary). [docs/accuracy.md](accuracy.md) is generated
from the results, so it can't drift from the code.

| Suite | How a pass is detected | Target |
|---|---|---|
| Per-instruction vectors (SM83 single-step tests) | final state + bus trace per case | 100% |
| Blargg `cpu_instrs` (11) | serial output "Passed" | 100%, DMG and CGB |
| Blargg `instr_timing`, `mem_timing`, `mem_timing-2`, `halt_bug`, `interrupt_time` | serial / screen text | 100% |
| Blargg `dmg_sound` (12) on DMG, `cgb_sound` (12) on CGB | screen text / RAM result | 100% |
| Blargg `oam_bug` (DMG) | screen text | stretch |
| Mooneye acceptance (timer, interrupts, PPU, OAM DMA, boot state, instr, bits, serial) | Fibonacci registers after `LD B,B` | 100% of tests for DMG-ABC and CGB-C/E |
| Mooneye emulator-only (MBC1, MBC2, MBC5) | same | 100% |
| Mooneye `misc` (CGB) | same | 100% |
| dmg-acid2, cgb-acid2 | frame equals the reference image, pixel for pixel | 100% |
| `rtc3test` (MBC3 RTC) | screen text | 100% |
| Mealybug Tearoom (mid-scanline PPU) | frame equals reference image | stretch, tracked |
| SameSuite (APU, DMA), gbmicrotest, age | per their docs | stretch, tracked |

Beyond pass/fail:

- **Regression hashes**: frame and audio hashes after fixed numbers of frames for homebrew
  demos and test ROMs, with scripted button presses, in `tests/expected/`; any change in output
  shows up in review and is explained in the commit.
- **Black-box comparison**: the same runs on the reference emulator, frames compared
  pixel by pixel and audio by spectrum and RMS difference, to find bugs the suites don't cover.
- **Game smoke tests** on the device, from the user's own card (`/mnt/roms`, `/mnt/ds`; never
  copied off it): boot to gameplay, a save made, quit, resume, the save there, for one game per
  mapper and for known-demanding cases (an MBC3 game with RTC, MBC5 games with large RAM, CGB
  games using HDMA and double speed, DMG games in CGB compatibility mode, games known for
  mid-scanline effects). Free homebrew and demos fetched from their official pages cover the
  same ground in CI without commercial ROMs.
- **Benchmarks** (`inkdot-test bench`): frames per second headless on rgbuild and on the DS+,
  per-frame time distribution, for a fixed set of ROMs; results in `docs/` with date, hardware
  and build, as nibOS does.

## The DS+ frontend

### Screens

```
 top panel 1024×768                      bottom panel 1024×768 (touch)
┌────────────────────────────┐          ┌────────────────────────────┐
│ border / backdrop          │          │  Game title         12:41 ▮│
│   ┌────────────────────┐   │          │ ┌──────┐ ┌──────┐ ┌──────┐ │
│   │                    │   │          │ │ Save │ │ Load │ │ Look │ │
│   │  game, 5× = 800×720 │   │          │ └──────┘ └──────┘ └──────┘ │
│   │  at (112, 24)       │   │          │ ┌──────┐ ┌──────┐ ┌──────┐ │
│   │                    │   │          │ │ Fast │ │Options│ │ Quit │ │
│   └────────────────────┘   │          │ └──────┘ └──────┘ └──────┘ │
└────────────────────────────┘          └────────────────────────────┘
```

- **Top screen**: the game at **5×, 800×720**, centred (112 px either side, 24 above and
  below), the largest integer scale that fits 768 lines, with square pixels on the panel's
  square pixels. Around it, a border: a plain colour, one of ours (drawn for this frame size),
  or a user PNG of 1024×768 with a transparent 800×720 window. Drawn with GLES 3.1 through SDL:
  the 160×144 frame is uploaded as a texture (92 KB a frame) and scaled by a fragment shader
  with optional, cheap looks: **DMG LCD** (dot grid, the panel's slow response as a blend with
  the previous frame, which some games rely on for transparency through flicker), **CGB
  colour** (raw 15-bit, or a correction curve of ours that approximates the original panel's
  duller, darker colours), or **sharp** (nearest-neighbour, nothing else).
- **Bottom screen**: the emulator's controls, made for a thumb: large targets (≥ 120 px),
  instant feedback, no small text. Pages: **Play** (the dashboard above), **States** (slots
  with thumbnails), **Look** (palette, border, LCD effect, with a live preview on the top
  screen), **Options** (the settings, with their level, below), **Quit**. Opening a page other
  than Play pauses the game (a setting). The physical Menu key (on its release, since
  `nibos-input` holds Menu back until it knows it isn't a brightness shortcut) opens the same
  menu on the bottom screen and makes it navigable with the D-pad and A/B, for play without
  touching.
- **Save states**: 6 slots a page shown as 2× thumbnails (320×288, three across), each with its
  date and play time, plus the automatic slot written on quit. Tap to load (with a confirmation
  only if it would lose more than a minute of play); Save writes the selected slot with a short
  animation of the thumbnail flying into it. Menu + R and Menu + L are quick save and quick load.
- **Battery**: the bottom screen is redrawn only when something on it changes (never at 60 Hz
  during play), and dims to the backlight floor after a while untouched; a touch brings it
  back. See [Decisions](#decisions) for whether it may also turn off.

### Frame pacing: one emulated frame per panel refresh

A Game Boy frame is 70,224 dots at 4,194,304 Hz: **59.7275 Hz**. The DS+ panels run at
**59.937 Hz** (measured, nibOS [display notes](../../nibos/docs/engineering/display.md)). They
can't be made equal (the panels' pixel clock is fixed by VPLL), so there are two ways to run:

- **Panel-locked (default)**: exactly one emulated frame per top-panel refresh. Motion is
  perfectly smooth and the game runs 0.35% fast (a minute of play is 0.2 s shorter), which no
  one can notice. Audio is generated for that same rate, i.e. resampled by 1.0035
  (+6 cents of pitch, below what ears can tell), plus a small adjustment (±0.5% at most, slewed
  slowly) that keeps the audio queue at its target level, so the audio clock and the panel clock
  can drift without ever under- or over-running.
- **Exact speed**: the emulator runs at 59.7275 Hz against the audio clock, and the top panel
  shows the latest finished frame; one frame is shown twice every **4.8 s**. For those who'd
  rather have exact timing (speedrun timing, music recording).

Each frame follows the top panel's vblank (DRM pipe 1, the one the game is shown on; the bottom
panel's vblank comes 8.2 ms later and the bottom screen is never waited on during play):

```
  vblank N                                                     vblank N+1
  │◄──────────────────── 16.68 ms ────────────────────────────►│
  │  idle: thread asleep (clock_nanosleep)   │ input │ run │ draw │ flip
  │                                          ▲ read  │≈2ms │<1ms ▲
  │                                          └─ start = vblank N+1 − (p99 run + draw + margin)
```

- **Input read just before the frame**: the loop sleeps until the latest start that still
  finishes before the next vblank (learned from the measured p99 of the last few hundred
  frames, plus a 1.5 ms margin), then reads input, emulates one frame, draws and flips.
- **The emulated frame starts at the Game Boy's own vblank** (LY = 144), not at line 0: games
  read the joypad in their vblank handler, so the buttons just read are seen at the very start
  of the frame, and lines 0–143 that the game then draws already show the result. Together
  with the late start, a press reaches the panel in the next refresh after it is read.
- **Latency target**: median button-to-flip ≤ 12 ms plus the game's own frames of lag, measured
  with nibOS's latency tooling (injected press timed in the kernel, to the page flip). A pass
  condition for the frontend.
- If a frame runs late (rare: the core needs ~15% of a frame), it is shown at the next vblank
  and the schedule moves earlier; after a suspend or a stall of more than two frames, the loop
  re-syncs instead of racing to catch up.

### Threads and real-time safety

- **One main thread at normal priority** does everything that takes time: events and input,
  emulation, drawing, the bottom screen, saving.
- **Audio** goes through SDL's audio queue (PipeWire underneath, as the nibOS app template
  does): the main thread pushes each frame's samples; SDL's own thread, which PipeWire may run
  with real-time priority, only copies from the queue. None of our code runs on a real-time
  thread, so PipeWire's 200 ms `RLIMIT_RTTIME` can never kill the process. Queue target: ~1.5
  frames (25 ms); the stream asks PipeWire for a 256-sample quantum (5.3 ms).
- **Saving files** (battery RAM, states, thumbnails, settings) happens on a small worker thread
  at normal priority, so a slow card never delays a frame. Writes are atomic (temporary file,
  `fsync`, rename) so a crash or a pulled battery leaves the previous save.

### Sleeping, not spinning

Between frames the main thread sleeps in `clock_nanosleep` to an absolute time, and in the
GPU's flip; it never polls in a loop. Expected CPU load during play: one core ~15% busy, the
others idle, so schedutil keeps the clocks low. Measured on the device by CPU time per frame,
frequency residency (`cpufreq` stats) and the battery's discharge current
(`/sys/class/power_supply/*/current_now`) over a few minutes of play, next to DSlot's shelf;
the numbers go in the README.

### Input

- The merged gamepad (`nibos-input`) through SDL's game controller API, by **printed** names: A
  (right) → A, B (bottom) → B, Select, Start, the D-pad (or the stick, a setting). SDL's swap of
  the face buttons on this pad is undone in one place (the shared crate, below).
- **Back** quits to DSlot (saving), bound under both its SDL names (AC_BACK, and "Sleep" as
  sdl2-compat passes it), as the nibOS input notes require.
- **Menu** on its own opens the menu (acted on at release); **Menu + R / L** quick save / load;
  **Menu + Select** fast-forward while held (2× to 8×, a setting; audio sped up then muted
  above 2×). X and Y are free (optional turbo A/B).
- **Brightness and volume** are not the emulator's business while playing: Menu + volume
  (brightness) is handled below every program by `nibos-input`, the volume keys by
  `nibos-audio`. The Options page also shows both screens' brightness as perceptual percent
  (`/run/nibos/backlight/<screen>/brightness`, the mapping from the shared crate) and the volume
  (through PipeWire), for adjusting by touch.

### Lifecycle

- Start: from exec to the first frame on screen in **< 250 ms** (no boot ROM, ROM loaded
  straight from the card or a `.zip`, GL set up once).
- Quit (Back, the menu, or `SIGTERM` from DSlot): battery RAM flushed, automatic state written,
  settings saved, screens released; back to DSlot in **< 300 ms**.
- Suspend (lid, power key: logind suspends the system): nothing to do beforehand since battery
  RAM is flushed within a second of the game's last write; on resume, the pacing re-syncs, the
  RTC catches up from the wall clock, and the audio queue restarts at its target level.

## Integration with nibOS and DSlot

### DSlot description

The package installs `/usr/share/dslot/emulators/inkdot.toml` (format: DSlot's
`src/emulator.rs`):

```toml
# Inkdot, for DSlot (packaging/nibos/inkdot.toml in Inkdot).
name = "Inkdot"
priority = 100

[systems]
GB  = ["/usr/bin/inkdot", "{options}", "--saves", "{saves}", "{rom}"]
GBC = ["/usr/bin/inkdot", "{options}", "--saves", "{saves}", "{rom}"]

# Put in place before a game starts when missing; never overwritten.
[[seed]]
from = "/usr/share/inkdot/inkdot.toml"
to = "~/.config/inkdot/inkdot.toml"
```

DSlot already knows GB and GBC as systems (Roms/GB, Roms/GBC, `.gb`/`.gbc`, inside `.zip`
too, header titles and codes); their shelf pages appear once DSlot has a cart shell for them,
which is a request for DSlot, not this project. The emulator opens `.zip` files itself.

### Settings: three levels

| Level | Where | Example |
|---|---|---|
| All games | `~/.config/inkdot/inkdot.toml`, `[all]` | input timing, LCD look, bottom screen dimming |
| A system | the same file, `[GB]` and `[GBC]` | GB games run on CGB with the colourised palette; GB palette |
| One game | `/mnt/roms/Saves/<GB or GBC>/<ROM name>/inkdot.toml` | this game's palette, its border, exact speed |

A game's setting wins over its system's, which wins over all games'. The Options page shows
each setting with the level it currently comes from and lets the user change it "for this
game", "for all GB (or GBC) games" or "for all games", and reset a level. DSlot's
`launch.conf` options (`{options}`) are command-line flags with the same names, applied on top
for that run only, never written back. The settings file lives with the saves so a game's
choices travel with it from one card to another.

### Saves

DSlot makes the game's folder and passes it as `--saves`: `/mnt/roms/Saves/GB/<ROM name>/` or
`.../GBC/...` (on the ROMS partition, whichever card the game is on). In it:

| File | What |
|---|---|
| `<ROM name>.sav` | battery RAM, raw, the size the header says (the common format other tools read) |
| `<ROM name>.rtc` | MBC3 RTC state: our documented little-endian layout (registers, latched registers, wall-clock time at save) |
| `<ROM name>.state1` … `.state12`, `.auto.state` | save states with their thumbnails |
| `inkdot.toml` | this game's settings |

The partition is FAT, so names are kept to characters FAT allows; atomic writes as above.

### nibOS conventions

`SDL_VIDEODRIVER=kmsdrm` and `SDL_KMSDRM_ATOMIC=0` come from DSlot's environment; PipeWire is
found through `XDG_RUNTIME_DIR`; the emulator runs as the user in DSlot's unit, with the
`video` group for the backlights. It follows nibOS's device interface: `/run/nibos/` names,
never kernel paths; percent, never raw levels; nothing below the backlight floor. The nibOS
`AGENTS.md` being written next to the app template (`sdk/`) is the reference; where this design
and it disagree, it wins and this design changes.

### Package

`packaging/nibos/PKGBUILD` builds in the rgbuild machine (`--locked`, `-C target-cpu=cortex-a55`,
LTO, `panic = "abort"`), and installs `/usr/bin/inkdot`, `/usr/share/dslot/emulators/inkdot.toml`,
`/usr/share/inkdot/` (default settings, palettes, borders, font, sounds) and the licence.
Depends on `sdl2` (sdl2-compat) and `mesa-nibos`. Tested on the DS+ with the device lock, DSlot
restarted after each run.

## Shared code with DSlot: `nibkit`

DSlot and this emulator, and every nibOS app after them, need the same handful of things done
the same way. Proposed: a small library crate, **`nibkit`**, with no UI framework and few
dependencies:

| Module | What | Now in |
|---|---|---|
| `screens` | open the top and bottom panels by role (SDL display order under KMSDRM, checked), GLES 3.1 contexts, swap policy | DSlot `platform/`, the app template |
| `vblank` | the time of a panel's vblanks (DRM vblank events on its CRTC), for pacing | new (DSlot `kms.rs` is related) |
| `pad` | face buttons by printed name, Back under both names, Menu-release handling, stick dead zone | DSlot `platform/nibos.rs`, the template |
| `touch` | touch events in bottom-panel pixels | DSlot `touch.rs` |
| `backlight` | perceptual percent ↔ level with the floor, per named screen | nibos-hw `backlight.rs` |
| `settings` | layered TOML settings (all / system / game) with atomic saves | new |
| `emulator` | the DSlot description format (serde types), so DSlot reads what emulators write with one definition, and emulators test their own file | DSlot `emulator.rs` |
| `paths` | ROM roots, the saves layout, XDG folders | DSlot `platform/nibos.rs` |

It is extracted from DSlot by the DSlot session (DSlot then depends on it), not rewritten from
memory here. Two things to settle: **where it lives** (its own small repository, or `sdk/nibkit`
in the nibOS repository next to the app template) and **its licence**. DSlot is GPL-3.0-or-later
and nibOS is MIT; the code moved into nibkit is the user's own, so the user can license it MIT,
which both GPL DSlot and this emulator (under any licence) can use. The emulator's UI drawing
(text, buttons) stays in the emulator for now, in DSlot's visual language; if a third app needs
it, it moves to a `nibkit-ui` crate.

## Licence

The user holds the copyright and can always license the code to a client on other terms too;
the question is what the public licence is.

| Option | For | Against |
|---|---|---|
| **MIT** | Same as nibOS; one short file; anyone, clients included, can fork and ship, closed or open | No explicit patent grant; forks need not share fixes |
| **MIT OR Apache-2.0** | The Rust ecosystem's default; Apache adds an explicit patent grant and a NOTICE convention | Two licence files; slightly more to explain |
| **MPL-2.0** | Changes to our files stay open, while a product may combine them with closed code | Clients' forks of the core must publish their changes to it; less simple for "license freely" |
| GPL-3.0 | Everything built on it stays open (as DSlot) | Contradicts licensing freely to clients without a separate deal |

**Recommendation: MIT OR Apache-2.0**: permissive (matches "license freely, including client
forks"), consistent with Rust crates the core might one day be published as, and the patent
grant is worth having for an emulator. Plain MIT is the close second, for being the same as
nibOS. (Not legal advice; one more point for a lawyer if it matters to a client: the extent to
which code written mostly by an AI agent is protected by copyright differs by country, and the
provenance log and the person's direction and testing of each change are the record of
authorship.)

## Name

Short, pronounceable, not trademark-adjacent (no Nintendo, Game Boy or "GB" as the leading word),
and ideally fitting nibOS: a pen's **nib** makes **dots**, and the Game Boy is a dot-matrix game.
Checked 2026-10-19 (crates.io API, AUR and Arch repositories, GitHub repository search):

| Name | crates.io | AUR / Arch | GitHub | Notes |
|---|---|---|---|---|
| **inkdot** | free (`inkdot`, `inkdot-core`) | free | 8 repos, all tiny; none an emulator | "Inkdot Games", a small studio's website exists; low risk, but a games-adjacent name |
| **dotwell** | free | free | 6 tiny repos | An ink well holds a nib's ink; no games use found |
| **dotlet** | free | free | 11; a bioinformatics dot-plot tool (21 stars) | Different field |
| **stippl** | free | free | `stipple` names are taken by a pixel-art editor and UI toolkits | Too close to "Stipple Effect" (a pixel-art editor) |

Not taken further: `stipple`, `tittle`, `jot`, `quill`, `speck`, `nibble`, `mote`, `blot`
(crates taken), `smudge`, `mottle`, `ferrule`, `jotter` (busy elsewhere).

**Recommendation: inkdot** (`inkdot`, `inkdot-core`; prose "Inkdot"), with **dotwell** if the
games-studio overlap bothers. Before any public release, a trademark search (USPTO, EUIPO, JPO)
for the chosen name in class 9 (software).

## Plan for step 2

Test-first throughout: each milestone starts with its tests failing and ends with them in the
suite.

1. Repository, CI, `make check`, the test-ROM fetcher with pinned hashes, `inkdot-test` harness
   (serial capture, Mooneye signature, image compare, hashes), AGENTS.md, CONTRIBUTING.md.
2. CPU against the per-instruction vectors; then Blargg `cpu_instrs`, `instr_timing`.
3. Timer, interrupts, HALT, OAM DMA; Blargg `mem_timing*`, Mooneye timer/interrupt/DMA tests,
   boot state tests.
4. DMG PPU: dmg-acid2, Mooneye PPU tests.
5. Cartridges, battery saves, RTC: Mooneye MBC tests, `rtc3test`.
6. CGB: palettes, banks, HDMA, double speed, compatibility mode: cgb-acid2, CGB tests.
7. APU: `dmg_sound`, `cgb_sound`; band-limited synthesis tests (spectral, against ideal tones).
8. Save states (round-trip tests over every test ROM and demo).
9. Frontend, top screen: pacing, audio, input, latency and battery measurements on the DS+.
10. Bottom screen: menu, states with thumbnails, looks, options with levels; nibkit adopted.
11. DSlot description, PKGBUILD, device tests with the user's games, docs (README, using,
    architecture, accuracy), benchmarks.

## Later

Super Game Boy borders and palettes (SGB command packets over the joypad lines; borders at 3× on
the top screen), link cable between two DS+ over Wi-Fi (serial with latency hiding), rumble for
MBC5 rumble carts (needs nibOS's rumble interface), rewind (a ring of compressed states; a state
is ~50 KB), MBC6/MBC7 (tilt via the stick)/HuC1/HuC3/MMM01, our own open boot ROM with our own
start-up animation, cheats.

## Decisions

For the user:

1. **Name**: inkdot (recommended), dotwell, or another.
2. **Licence**: MIT OR Apache-2.0 (recommended), MIT, or MPL-2.0.
3. **nibkit**: its own repository or `sdk/nibkit` in nibOS (recommended: in nibOS, next to the
   app template, so the SDK is one place), and MIT for it.
4. **Default speed**: panel-locked (recommended) or exact.
5. **DMG games by default**: on CGB with the compatibility palette (recommended, it's what a
   Game Boy Color shows), or as a DMG with a green/grey palette.
6. **Bottom screen during play**: dim to the floor when untouched (recommended), or also turn
   its backlight off entirely (more battery; nibOS's floor rule would need an exception for it).
7. **Resume**: start each game from its automatic state (recommended, with "Start over" on the
   bottom screen), or always cold-boot the game.
