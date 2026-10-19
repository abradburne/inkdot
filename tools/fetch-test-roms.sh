#!/bin/bash
# Downloads the test ROMs, reference images and per-instruction test vectors the test suite
# runs, from their official releases, checks each against its pinned SHA-256, and unpacks them
# into tests/roms/ (gitignored: their licences vary, so they are fetched, never committed;
# docs/provenance.md lists each with its licence).
#
#   tools/fetch-test-roms.sh            fetch whatever is missing
#   tools/fetch-test-roms.sh --force    fetch everything again
#
# Needs curl, tar, unzip and shasum or sha256sum. Changes only tests/roms/.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
dest="$here/../tests/roms"
force=0
case "${1:-}" in
    --force) force=1 ;;
    -h | --help) sed -n '2,10p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    "") ;;
    *) echo "unknown option: $1" >&2; exit 2 ;;
esac

sha256() {
    if command -v sha256sum >/dev/null; then sha256sum "$1"; else shasum -a 256 "$1"; fi | cut -d' ' -f1
}

# name, URL, SHA-256, and how to unpack it ("file" keeps it as it is).
fetch() {
    local name="$1" url="$2" sum="$3" unpack="$4"
    [ "$force" = 0 ] && [ -e "$dest/.done-$name" ] && return
    local tmp
    tmp="$(mktemp -d)"
    trap 'rm -rf "$tmp"' RETURN
    echo "fetching $name"
    curl -fsSL -o "$tmp/download" "$url"
    local got
    got="$(sha256 "$tmp/download")"
    if [ "$got" != "$sum" ]; then
        echo "$name: SHA-256 is $got, expected $sum" >&2
        exit 1
    fi
    rm -rf "${dest:?}/$name"
    mkdir -p "$dest/$name"
    case "$unpack" in
        tar) tar -xzf "$tmp/download" -C "$dest/$name" --strip-components 1 ;;
        zip) unzip -q "$tmp/download" -d "$dest/$name" ;;
        file) cp "$tmp/download" "$dest/$name/$(basename "$url")" ;;
    esac
    touch "$dest/.done-$name"
}

mkdir -p "$dest"
fetch blargg https://codeload.github.com/retrio/gb-test-roms/tar.gz/c240dd7d700e5c0b00a7bbba52b53e4ee67b5f15 \
    1e121842bc380cc69ec4d5946335c0e6656926af8a794ccc732d1b293f29e411 tar
fetch mooneye https://gekkio.fi/files/mooneye-test-suite/mts-20260714-0944-31510e1/mts-20260714-0944-31510e1.tar.gz \
    5f0694e4e9e58441db23ff057a5ab681870d4cfaed91267e99885936db8e6f1d tar
fetch sm83 https://codeload.github.com/SingleStepTests/sm83/tar.gz/f9c30210245dd691661db39f5ace022c465ecc2f \
    ac364307f2bc012034ae55b14d237f832cb3f3d9b14bb9c7c80086165d1afeaf tar
fetch dmg-acid2 https://github.com/mattcurrie/dmg-acid2/releases/download/v1.0/dmg-acid2.gb \
    464e14b7d42e7feea0b7ede42be7071dc88913f75b9ffa444299424b63d1dff1 file
fetch dmg-acid2-ref https://raw.githubusercontent.com/mattcurrie/dmg-acid2/master/img/reference-dmg.png \
    ca966d50895c7efef05838590d148c2cbfd7fba57dab986f25b35b4da71abb57 file
fetch cgb-acid2 https://github.com/mattcurrie/cgb-acid2/releases/download/v1.1/cgb-acid2.gbc \
    197fb0bcec544f0400527fc707e0a94f55435974986e6986b424ace5de81720e file
fetch cgb-acid2-ref https://raw.githubusercontent.com/mattcurrie/cgb-acid2/master/img/reference.png \
    9ea9c262c5383353e77d715d021a0f7c5ccbe438f88082cb225756e50c4fdf01 file
fetch rtc3test https://github.com/aaaaaa123456789/rtc3test/releases/download/v004/rtc3test.gb \
    a271013fb37ea1d927b854798401320b220d57abb3ca77f3d318ceb8a9def30d file
fetch mealybug https://github.com/mattcurrie/mealybug-tearoom-tests/raw/master/mealybug-tearoom-tests.zip \
    e2fa6ca96ad48bd64d7297fda042db04c15117a730da3bbd590f139e89b150bd zip
echo "test ROMs in $dest"
