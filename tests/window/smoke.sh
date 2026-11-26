#!/usr/bin/env bash
# Inside tests/window/compositor.sh: runs a build of smoke.c with a
# window, takes a picture of the compositor's output once a frame has been
# captured, closes the window from the compositor, and compares the two.
#
#   tests/window/smoke.sh PROGRAM DIR
#
# DIR receives the program's log, its capture and the picture.
set -uo pipefail
program=$1
dir=$2
mkdir -p "$dir"
log=$dir/log

"$program" window "$dir/capture.ppm" > "$log" 2>&1 &
pid=$!
for _ in $(seq 400); do
  if grep -q '^smoke: shown' "$log" || ! kill -0 "$pid" 2> /dev/null; then break; fi
  sleep 0.05
done
if ! grep -q '^smoke: shown' "$log"; then
  kill "$pid" 2> /dev/null
  wait "$pid" 2> /dev/null
  cat "$log" >&2
  echo "smoke.sh: $program showed no frame" >&2
  exit 1
fi

grim -t ppm "$dir/screen.ppm"
swaymsg '[title="midnight smoke"] kill' > /dev/null
wait "$pid"
status=$?
cat "$log"
if [ "$status" -ne 0 ]; then
  echo "smoke.sh: $program ended with status $status" >&2
  exit 1
fi
if grep -qE 'runtime error|Sanitizer' "$log"; then
  echo "smoke.sh: $program has a sanitizer report" >&2
  exit 1
fi
"$program" compare "$dir/capture.ppm" "$dir/screen.ppm"
