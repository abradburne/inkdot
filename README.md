# inkdot

A Game Boy and Game Boy Color emulator, written from scratch in Rust for the Anbernic RG DS+:
on nibOS, from DSlot or by itself, and on the stock OS, from Apps. The game fills the top screen at an exact 5× scale; the bottom touch
screen holds the emulator's controls: save states with their pictures, the look (a Game Boy Color,
a Game Boy, a Pocket, a Light), fast-forward and options.

## Accuracy

The core passes the test suites that matter for a Game Boy emulator in full:
[docs/accuracy.md](docs/accuracy.md) is generated from them.

- the SM83 per-instruction vectors (500 opcodes × 1000 cases, each M-cycle's bus activity);
- Blargg's `cpu_instrs`, `instr_timing`, `mem_timing`, `mem_timing-2`, `halt_bug`, `dmg_sound`,
  `cgb_sound` and `interrupt_time`;
- Mooneye's acceptance tests for the DMG and the model-independent ones on a CGB, its CGB `misc`
  tests and its MBC1/MBC2/MBC5 tests;
- dmg-acid2 (on a DMG and on a CGB) and cgb-acid2, pixel for pixel.

## The pieces

| | |
|---|---|
| [`crates/inkdot-core`](crates/inkdot-core) | The machine: CPU, picture, sound, timer, cartridges (MBC1/1M/2/3+RTC/5), save states. `no_std`, no dependencies. |
| [`crates/inkdot`](crates/inkdot) | The program for the DS+: SDL2, GLES, [nibkit](https://github.com/abradburne/nibkit); RetroAchievements through nibkit's `nib-cheevos` ([rcheevos](https://github.com/RetroAchievements/rcheevos), vendored there, MIT; `curl` as the transport). |
| [`crates/inkdot-test`](crates/inkdot-test) | Runs the core headless: the test suites, benchmarks, sound to WAV. |
| [`packaging/nibos`](packaging/nibos) | The PKGBUILD and the description DSlot reads. |
| [`packaging/stock`](packaging/stock) | The stock OS's Apps script, and the script that builds its zip. |

[docs/performance.md](docs/performance.md) has the measurements on the handheld (about 3 ms a
frame for most games at full clock; press to screen about 23 ms).

[docs/design.md](docs/design.md) is the design; [docs/architecture.md](docs/architecture.md)
how it is built; [docs/using.md](docs/using.md) how to play; [docs/provenance.md](docs/provenance.md)
the documents and tests it was written from (a clean room: no other emulator's source).

## Building and testing

```bash
tools/fetch-test-roms.sh       # the test ROMs, from their official releases, checked by SHA-256
make check                     # format, lint, every test including the ROM suites
cargo run --release -p inkdot -- path/to/game.gb      # on a computer: both screens in one window
```

For the DS+ on nibOS: `tools/run-on-device.sh game.gb` builds in the aarch64 build machine and
runs it on the handheld in place of DSlot; `packaging/nibos/PKGBUILD` builds the package.

The program is built for one system at a time, by Cargo feature: `nibos` (the default),
`stock`, or `desktop` (a computer: a window, the keyboard and the mouse; chosen anyway on any
system but Linux). `dslot` adds DSlot's part (its settings file, and a test that its description of inkdot
matches inkdot's options); without it inkdot keeps its own settings. `cheevos` (in the default
set, with `nibos`) builds RetroAchievements in: the sign-in comes from DSlot through the
environment (docs/using.md, "RetroAchievements"); it needs a C compiler and, at run time, `curl`.

For the stock OS, cross-built with [cargo-zigbuild](https://github.com/rust-cross/cargo-zigbuild)
for its glibc 2.31 against the handheld's own SDL2, copied out of a stock image into
`sysroot/libSDL2.so` (as for DSlot: its docs/building.md shows how; it isn't in the repository):

```bash
packaging/stock/package.sh     # -> dist/inkdot-<version>-rgdsplus-stock.zip: unzip onto the games card
```

## Licence

MIT OR Apache-2.0, at your option. The program has the Roboto Condensed fonts built in (through
nibkit's menu), which are under the SIL Open Font License 1.1: [LICENSE-OFL](LICENSE-OFL), installed
with every package. See [CONTRIBUTING.md](CONTRIBUTING.md) for how changes are made
(the DCO, and the rules for AI-assisted contributions).
