# inkdot

A Game Boy and Game Boy Color emulator, written from scratch in Rust for the Anbernic RG DS+ on
nibOS and started from DSlot. The game fills the top screen at an exact 5× scale; the bottom touch
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
| [`crates/inkdot`](crates/inkdot) | The program for the DS+: SDL2, GLES, [nibkit](../nibos/sdk/nibkit). |
| [`crates/inkdot-test`](crates/inkdot-test) | Runs the core headless: the test suites, benchmarks, sound to WAV. |
| [`packaging/nibos`](packaging/nibos) | The PKGBUILD and the description DSlot reads. |

[docs/design.md](docs/design.md) is the design as agreed; [docs/architecture.md](docs/architecture.md)
how it is built; [docs/using.md](docs/using.md) how to play; [docs/provenance.md](docs/provenance.md)
the documents and tests it was written from (a clean room: no other emulator's source).

## Building and testing

```bash
tools/fetch-test-roms.sh       # the test ROMs, from their official releases, checked by SHA-256
make check                     # format, lint, every test including the ROM suites
cargo run --release -p inkdot -- path/to/game.gb      # on a computer: both screens in one window
```

For the DS+: `tools/run-on-device.sh game.gb` builds in the aarch64 build machine and runs it on
the handheld in place of DSlot; `packaging/nibos/PKGBUILD` builds the package.

## Licence

MIT OR Apache-2.0, at your option. See [CONTRIBUTING.md](CONTRIBUTING.md) for how changes are made
(the DCO, and the rules for AI-assisted contributions).
