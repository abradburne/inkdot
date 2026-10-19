#!/bin/bash
# Shared by the tools: settings and helpers. Sourced, not run.
#
# Who and where come from config.local.sh at the repository's root (gitignored; copy
# config.local.sh.example): DEVICE (user@host of the handheld), SSH_KEY, BUILD_MACHINE (an
# OrbStack machine with Arch Linux ARM, as nibOS builds in).

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=/dev/null
[ -f "$root/config.local.sh" ] && . "$root/config.local.sh"
: "${BUILD_MACHINE:=rgbuild}"
: "${TARGET_DIR:=\$HOME/inkdot-target}"

die() {
    echo "${0##*/}: $*" >&2
    exit 1
}

usage() {
    sed -n '2,/^set -euo/p' "$0" | sed '$d' | sed 's/^# \{0,1\}//'
    exit "${1:-0}"
}

ssh_opts=(-o IdentitiesOnly=yes -o ConnectTimeout=5)
[ -n "${SSH_KEY:-}" ] && ssh_opts+=(-i "$SSH_KEY")

on_device() {
    [ -n "${DEVICE:-}" ] || die "set DEVICE in config.local.sh"
    # The command is meant to expand here and run there.
    # shellcheck disable=SC2029
    ssh "${ssh_opts[@]}" "$DEVICE" "$@"
}

in_vm() {
    orb -m "$BUILD_MACHINE" "$@"
}
