#!/usr/bin/env bash
# Runs a command with a compositor of its own as the only display it can
# reach:
#
#   tests/window/compositor.sh COMMAND [ARG ...]
#
# The compositor is sway with the headless backend: it has one output of
# 1280x720 at 60 Hz that is shown nowhere, and no input devices. It and
# the command get a runtime directory of their own, so its sockets are
# the only ones in reach. The display, compositor and session-bus
# variables of the desktop this is run from are removed from everything
# started here, and DISPLAY and the session bus are set to addresses that
# cannot be connected to. The command sees WAYLAND_DISPLAY and SWAYSOCK of
# this compositor, and its status is the script's.
#
# sway, swaymsg and the other tools come from PATH. RENDERER chooses how
# sway draws: `pixman` draws on the CPU and needs no GPU; without it sway
# draws with its default renderer on a GPU it chooses, or the one the
# render node in RENDER_NODE names.
set -uo pipefail

dir=$(mktemp -d)
unset LD_LIBRARY_PATH WAYLAND_DISPLAY WAYLAND_SOCKET DISPLAY XAUTHORITY \
  HYPRLAND_INSTANCE_SIGNATURE SWAYSOCK I3SOCK DBUS_SESSION_BUS_ADDRESS \
  XDG_SESSION_TYPE XDG_CURRENT_DESKTOP
export XDG_RUNTIME_DIR=$dir
export DISPLAY=:no-display
export DBUS_SESSION_BUS_ADDRESS=unix:path=$dir/no-bus

cat > "$dir/config" << 'EOF'
output HEADLESS-1 mode 1280x720@60Hz position 0 0 scale 1
default_border none
focus_follows_mouse no
xwayland disable
EOF

export WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1
if [ -n "${RENDERER:-}" ]; then export WLR_RENDERER=$RENDERER; fi
if [ -n "${RENDER_NODE:-}" ]; then export WLR_RENDER_DRM_DEVICE=$RENDER_NODE; fi
sway -c "$dir/config" > "$dir/sway.log" 2>&1 &
sway_pid=$!
cleanup() {
  kill "$sway_pid" 2> /dev/null
  wait "$sway_pid" 2> /dev/null
  rm -r "$dir"
}
trap cleanup EXIT

socket=
ipc=
for _ in $(seq 200); do
  socket=$(find "$dir" -maxdepth 1 -name 'wayland-*' ! -name '*.lock' -printf '%f\n' | head -1)
  ipc=$(find "$dir" -maxdepth 1 -name 'sway-ipc.*' | head -1)
  if [ -n "$socket" ] && [ -n "$ipc" ]; then break; fi
  if ! kill -0 "$sway_pid" 2> /dev/null; then break; fi
  sleep 0.05
done
if [ -z "$socket" ] || [ -z "$ipc" ]; then
  echo "compositor.sh: sway did not start" >&2
  tail -20 "$dir/sway.log" >&2
  exit 1
fi

export WAYLAND_DISPLAY=$socket SWAYSOCK=$ipc SDL_VIDEODRIVER=wayland
"$@"
status=$?
if [ "$status" -ne 0 ] && [ -n "${COMPOSITOR_LOG:-}" ]; then
  cp "$dir/sway.log" "$COMPOSITOR_LOG"
fi
exit "$status"
