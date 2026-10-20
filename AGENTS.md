# AGENTS.md: working on inkdot

inkdot is a Game Boy and Game Boy Color emulator written from scratch, for the Anbernic RG DS+ on
nibOS. Read this first; [CONTRIBUTING.md](CONTRIBUTING.md) has the rest.

## Clean room: the rule above all others

inkdot must stay entirely its owner's, so it can be licensed freely. **Never read, copy or
paraphrase the source of another emulator or emulator component** (SameBoy, mGBA, Gambatte,
Gearboy, binjgb, DSperate including its `gb_psg`, any port or snippet of them, forum posts that
paste their code). Work from:

- hardware documentation: Pan Docs, the gbdev.io resources, *Game Boy: Complete Technical
  Reference*, the Cycle-Accurate GB Docs;
- test ROMs, their documentation and their assembly sources (they say what hardware does, not
  how an emulator does it), and published per-instruction test vectors;
- running a locally built emulator as a black box (its frames and sound compared, its source
  never opened).

Every source used is listed in [docs/provenance.md](docs/provenance.md); add a row before
using a new one. A commit that implements hardware behaviour names its source in its body.

## Layout

| | |
|---|---|
| `crates/inkdot-core` | The machine: CPU, PPU, APU, timer, cartridges, save states. `no_std`, no dependencies, no `unsafe`. |
| `crates/inkdot-test` | Headless runner: the test ROM suites, benchmarks, `--wav`, `--io`, `--break`. |
| `crates/inkdot` | The DS+ program: SDL2 + GLES, nibkit (`../nibos/sdk/nibkit`). |
| `packaging/nibos` | The DSlot description (`inkdot.toml`, checked against `options::SPECS` by a test with `--features dslot`) and the PKGBUILD. |
| `packaging/stock` | The stock OS's Apps script and `package.sh` (cargo-zigbuild, `--features stock`). |
| `tools/` | `fetch-test-roms.sh`, `run-on-device.sh`, `lib.sh` (settings from `config.local.sh`). |
| `docs/` | Design, architecture, accuracy, provenance, using. |

## Checks

```bash
tools/fetch-test-roms.sh     # once: the test ROMs, from their official releases, hash-checked
make check                   # fmt, clippy -D warnings, all tests (ROM suites included), shellcheck
```

The ROM suites (Blargg, Mooneye, acid2, sound) run in `cargo test` in a few seconds; a change
to the core must keep every one passing. Timing in the PPU and timer was fitted to the Mooneye
tests' published tables: don't move a dot without a test that says so.

## On the device

Builds run in the aarch64 build machine (`BUILD_MACHINE`). Take the shared device lock first
(nibOS's `tools/device-lock.sh take "WHO: WHAT"`, `drop` after, `game` to see if someone is playing); never stop DSlot while a game is being played (`pgrep dsperate`,
`pgrep inkdot`); start DSlot again after. `tools/run-on-device.sh ROM` does the stopping and
starting. Follow nibOS's AGENTS.md for screens, input, audio and device settings.

inkdot must work without DSlot (started by hand, its own settings and saves) and on the stock OS.
Trying it on the stock OS means swapping the card: ask the user first, every time.

## Commits

One logical change per commit, `core:`, `frontend:`, `test:`, `docs:`, `packaging:` prefixes;
the body says why, what was measured, what was tested. Agents add `Co-Authored-By:`, never
`Signed-off-by:` (that is the human's certification, the DCO).
