# Contributing

inkdot is meant to be read as much as run: someone new should be able to follow any change,
what it does, why, and what it was measured or tested to do.

## Before you send a change

```bash
tools/fetch-test-roms.sh   # once
make check
```

`make check` runs `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, every test
(the core's unit tests, the SM83 vectors, the Blargg, Mooneye, acid2 and sound suites, save-state
round trips, the frontend's tests) and shellcheck. Anything that changes what the player sees or
hears is also tried on the handheld, and the commit says how.

## nibkit

The frontend uses [nibkit](https://github.com/abradburne/nibkit), nibOS's SDK library, as a git
dependency at a tag. To work on both at once, build against a local checkout: copy
`.cargo/config.example.toml` to `.cargo/config.toml` (gitignored, so your paths stay yours) and
uncomment its `[patch]`, with the path to your nibkit relative to this repository. The example
also has the settings for building on an Apple silicon Mac.

Cargo then records the local nibkit in `Cargo.lock`; don't commit that change. The PKGBUILD does
the same with `NIBKIT=path/to/nibkit makepkg ...`, and gives the build its own Cargo config.

## The clean room

The first rule (see [AGENTS.md](AGENTS.md)): no other emulator's source, read or copied, ever.
Hardware documentation, test ROMs and their sources, published test vectors, and black-box
comparison with a locally built emulator are the sources; [docs/provenance.md](docs/provenance.md)
lists each one used.

## Style

- **Comments say why**, in plain sentences: the hardware behaviour, the document it comes from,
  the test that pins it down. What the code does should read from the code. History goes in
  commit messages.
- **Rust**: `cargo fmt` (100 columns), clippy clean. The core is `no_std`, has no dependencies
  and forbids `unsafe`; the frontend's few `unsafe` blocks (GL, libc) each carry a `SAFETY:`
  comment. Logic that can be tested without the hardware lives apart from SDL and GL, with
  unit tests.
- **Shell**: shellcheck clean; scripts start with what they do and how to call them.
- **Documentation** describes the present; measurements say what was measured and how.

## AI-assisted contributions

AI contributions are accepted, under the rules nibOS uses (and DSlot):

- **A human submits and answers for every change**: they have read it, run it, and stand behind
  it. Disclose AI help (a `Co-Authored-By:` trailer on the commits). Never claim a test or a
  device run that didn't happen; say plainly what wasn't checked.
- **Small and precise**: one logical change per pull request, a few hundred changed lines at
  most unless agreed; no unrelated refactors or reformatting. Open an issue first for new
  features and new dependencies.
- **Every pull request says** what, why (with evidence), how, the alternatives, the risk, and
  how it was tested (`make check`, plus the device, the build and the results).
- **Tests**: new behaviour comes with tests; a bug fix with a regression test where possible.
- **Clean room**: a pull request that brings in code derived from another emulator is closed.

## Commits and the DCO

Every commit in a pull request carries a `Signed-off-by:` line from its human author
(`git commit -s`): the [Developer Certificate of Origin](https://developercertificate.org/).
Agents never add it on someone's behalf; they add `Co-Authored-By:`.

## Licence

inkdot is MIT OR Apache-2.0, at the user's option ([LICENSE-MIT](LICENSE-MIT),
[LICENSE-APACHE](LICENSE-APACHE)). Contributions are made under the same terms.
