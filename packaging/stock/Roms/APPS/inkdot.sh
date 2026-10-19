#!/bin/bash
# inkdot: a Game Boy and Game Boy Color emulator, run from Apps on the RG DS+'s stock menu.
# Without a game it lists those in Roms/GB and Roms/GBC on both cards. Its settings are kept
# beside it, in inkdot/settings.toml; saves go in Saves/GB and Saves/GBC on the card the OS
# starts from.

progdir="$(cd "$(dirname "$0")" && pwd)/inkdot"

# shellcheck source=/dev/null
. /etc/profile.d/myenv.sh 2>/dev/null
export XDG_RUNTIME_DIR=/var/run
export WAYLAND_DISPLAY=wayland-0
export SDL_VIDEODRIVER=wayland
export WESTON_OUTPUT_FLOW=horizontal

# The stock helper that routes sound to the speaker or headphones and answers the volume,
# brightness and power keys; it stays up only while the "an app is running" flag is set.
echo 1 > /sys/class/anbernic_misc/runapp
# shellcheck disable=SC2009 # as the stock scripts do it
if ! ps -A | grep -q "portsCtrl.dge"; then
	if [ -f /mnt/mmc/portsCtrl.dge ]; then
		/mnt/mmc/portsCtrl.dge > /dev/null 2>&1 &
	else
		/mnt/vendor/bin/portsCtrl.dge > /dev/null 2>&1 &
	fi
fi
# 0 turns the touch panel on.
echo 0 > /sys/class/anbernic_misc/tpctrl

cd "$progdir" || exit 1
# exec: a signal (SIGTERM at shutdown) reaches inkdot, which saves where the game was.
exec ./inkdot "$@" > "$progdir/log.txt" 2>&1
