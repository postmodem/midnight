#!/usr/bin/env bash
# The window tests: each runs inside a compositor of its own
# (compositor.sh), never on the desktop this is run from.
#
#   tests/window/test.sh OUT
#
# Needs MIDNIGHT_CC, MIDNIGHT_GPU_FLAGS and MIDNIGHT_WINDOW_FLAGS, and
# sway, swaymsg, grim and mkfifo on PATH, and MIDNIGHT_TEST_INPUT_CLIENT
# naming tests/window/input.c built. Run from the repository root:
# the suite's window tests run as MIDNIGHT_SUITE, the suite's executable,
# or through cargo when that is not set, on the device MIDNIGHT_TEST_GPU
# names (the hardware device when it is not set). RENDERER and
# RENDER_NODE are passed to compositor.sh; VK_DRIVER_FILES, when set,
# chooses the smoke program's device. OUT receives what each test writes.
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

# The suite's window tests, one at a time, in a compositor of their own.
if [ -n "${MIDNIGHT_SUITE:-}" ]; then
  suite=("$MIDNIGHT_SUITE")
else
  suite=(cargo test --release --test suite --)
fi
echo "== the suite's window tests"
MIDNIGHT_TEST_WINDOW=1 COMPOSITOR_LOG=$out/suite-sway.log \
  bash "$here/compositor.sh" "${suite[@]}" --exact --test-threads=1 --nocapture \
  window_shown window_shown_takes_no_resize window_keeps_time window_closes window_resized \
  window_input input_client_refuses_a_desktop input_client_takes_its_commands
echo "window tests passed"
