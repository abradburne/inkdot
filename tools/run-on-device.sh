#!/bin/bash
# Builds inkdot in the build machine, copies it and a ROM to the handheld, and runs it on the
# screens in place of DSlot, its output here. DSlot is started again when it exits (Back,
# Quit, Ctrl-C here).
#
#   tools/run-on-device.sh ROM [-- INKDOT_ARGS...]      ROM: a path on the Mac, or device:/path
#
# Refuses while a game is being played on the device. Nothing is installed: the binary goes to
# /tmp, the saves next to the ROM's copy in /tmp/inkdot-run (or by the device's ROM).
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source-path=SCRIPTDIR source=lib.sh
. "$here/lib.sh"

[ $# -ge 1 ] || usage 2
[ "$1" = -h ] && usage
rom="$1"
shift
[ "${1:-}" = -- ] && shift

in_vm bash -lc "cd '$root' && CARGO_TARGET_DIR=$TARGET_DIR cargo build --release --quiet -p inkdot && cp $TARGET_DIR/release/inkdot /tmp/inkdot-for-device"
in_vm cat /tmp/inkdot-for-device | on_device "cat > /tmp/inkdot && chmod 755 /tmp/inkdot"
if [[ "$rom" == device:* ]]; then
    remote_rom="${rom#device:}"
    saves="$(dirname "$remote_rom")"
else
    on_device "mkdir -p /tmp/inkdot-run"
    remote_rom="/tmp/inkdot-run/$(basename "$rom")"
    on_device "cat > '$remote_rom'" <"$rom"
    saves=/tmp/inkdot-run
fi

on_device 'pgrep -x dsperate >/dev/null || pgrep -x inkdot >/dev/null' && die "a game is running on the device: try again when it isn't"
dslot=$(on_device 'systemctl is-active dslot.service || true')
restore() {
    [ "$dslot" = active ] && on_device 'sudo systemctl start dslot.service' >/dev/null 2>&1
}
trap restore EXIT
[ "$dslot" = active ] && on_device 'sudo systemctl stop dslot.service'
args=$(printf ' %q' "$@" --saves "$saves" "$remote_rom")
ssh "${ssh_opts[@]}" -t "$DEVICE" \
    "SDL_VIDEODRIVER=kmsdrm SDL_KMSDRM_ATOMIC=0 XDG_RUNTIME_DIR=/run/user/\$(id -u) /tmp/inkdot$args"
