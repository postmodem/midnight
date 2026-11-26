#!/usr/bin/env bash
# The window tests: each runs inside a compositor of its own
# (compositor.sh), never on the desktop this is run from.
#
#   tests/window/test.sh OUT
#
# Needs MIDNIGHT_CC, MIDNIGHT_GPU_FLAGS and MIDNIGHT_WINDOW_FLAGS, and
# sway, swaymsg and grim on PATH. RENDERER and RENDER_NODE are passed to
# compositor.sh; VK_DRIVER_FILES, when set, chooses the device. OUT
# receives what each test writes.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
out=$1
mkdir -p "$out"
unset LD_LIBRARY_PATH

# smoke.c in release and debug builds.
# shellcheck disable=SC2086
"$MIDNIGHT_CC" -std=c11 -O2 -Wall -Wextra -Werror "$here/smoke.c" -o "$out/smoke" \
  $MIDNIGHT_GPU_FLAGS $MIDNIGHT_WINDOW_FLAGS -lm
# shellcheck disable=SC2086
"$MIDNIGHT_CC" -std=c11 -O0 -g -fsanitize=address,undefined -Wall -Wextra -Werror \
  "$here/smoke.c" -o "$out/smoke-debug" $MIDNIGHT_GPU_FLAGS $MIDNIGHT_WINDOW_FLAGS -lm

for build in smoke smoke-debug; do
  echo "== $build"
  mkdir -p "$out/$build-run"
  COMPOSITOR_LOG=$out/$build-run/sway.log \
    bash "$here/compositor.sh" bash "$here/smoke.sh" "$out/$build" "$out/$build-run"
done
echo "window tests passed"
