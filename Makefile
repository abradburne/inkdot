# inkdot: `make check` is what every change must pass (CONTRIBUTING.md).

.PHONY: check fmt clippy test shell accuracy

check: fmt clippy test shell

fmt:
	cargo fmt --all -- --check

clippy:
	cargo clippy --all-targets -- -D warnings

test:
	tools/fetch-test-roms.sh
	cargo test --release

shell:
	shellcheck -x tools/*.sh

# Regenerates docs/accuracy.md from the test ROMs.
accuracy:
	cargo run --release -q -p inkdot-test -- --accuracy docs/accuracy.md
