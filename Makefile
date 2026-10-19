# inkdot: `make check` is what every change must pass (CONTRIBUTING.md).

.PHONY: check fmt clippy test shell accuracy

check: fmt clippy test shell

fmt:
	cargo fmt --all -- --check

# Each system inkdot builds for, with and without DSlot's part.
clippy:
	cargo clippy --all-targets -- -D warnings
	cargo clippy -p inkdot --all-targets --features dslot -- -D warnings
	cargo clippy -p inkdot --all-targets --no-default-features --features stock -- -D warnings

test:
	tools/fetch-test-roms.sh
	cargo test --release
	cargo test --release -p inkdot --features dslot
	cargo test --release -p inkdot --no-default-features --features stock

shell:
	shellcheck -x tools/*.sh packaging/stock/*.sh packaging/stock/Roms/APPS/*.sh

# Regenerates docs/accuracy.md from the test ROMs.
accuracy:
	cargo run --release -q -p inkdot-test -- --accuracy docs/accuracy.md
