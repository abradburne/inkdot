#!/bin/bash
# Build inkdot for the RG DS+'s stock OS and zip what goes on the games card: Roms/APPS/inkdot.sh
# and the folder inkdot/ beside it (the program and its README). Cross-built on a Mac or Linux
# PC with cargo-zigbuild, for the stock OS's glibc 2.31, against the handheld's own libSDL2 in
# sysroot/ (docs/building.md).
#
#   packaging/stock/package.sh        -> dist/inkdot-<version>-rgdsplus-stock.zip
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
cd "$here/../.."

[ -f sysroot/libSDL2.so ] || { echo "no sysroot/libSDL2.so: see docs/building.md" >&2; exit 1; }
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
cargo zigbuild --release -p inkdot --no-default-features --features stock \
	--target aarch64-unknown-linux-gnu.2.31

stage=dist/stage
zip="dist/inkdot-$version-rgdsplus-stock.zip"
rm -rf "$stage" "$zip" && mkdir -p "$stage"
cp -R "$here/Roms" "$stage/"
app="$stage/Roms/APPS/inkdot"
mkdir -p "$app"
cp target/aarch64-unknown-linux-gnu/release/inkdot "$app/inkdot"
cp README.md docs/using.md LICENSE-MIT LICENSE-APACHE "$app/"
# The Roboto Condensed fonts built in (through nibkit) are under the SIL OFL 1.1.
cp LICENSE-OFL "$app/OFL.txt"
find "$stage" -name .DS_Store -delete
(cd "$stage" && zip -qr "../$(basename "$zip")" Roms)
echo "$zip"
