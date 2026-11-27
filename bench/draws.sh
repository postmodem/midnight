# Measures what a frame of shapes costs the device. Runs inside the
# compositor of the window tests:
#
#   nix develop -c bash tests/window/compositor.sh bash bench/draws.sh
#
# bench/draw_frame.12am runs with 0, 10,000, 100,000 and 1,000,000 shapes,
# drawn by its GPU system and, as a second program, by the same system
# made ordinary, whose list is sent to the device every tick. Each runs
# RUNS times (default 3) with its window shown and headless, and the
# fastest run of each counts. A headless window makes no picture, so
# "frame" is shown minus headless: the time a frame of those shapes adds
# to a tick. The device is the one the Vulkan loader finds, which a
# VK_DRIVER_FILES the caller sets chooses; for Mesa's llvmpipe, set
# MIDNIGHT_GPU=software and RENDERER=pixman too, so that the programs may
# use it and the compositor draws on the CPU. MIDNIGHT
# is the compiler (default: cargo run from the repository).

set -euo pipefail
runs=${RUNS:-3}
here=$(cd "$(dirname "$0")/.." && pwd)
midnight=${MIDNIGHT:-"cargo run -q --release --manifest-path $here/Cargo.toml --"}
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

# The fastest ms_per_tick of RUNS runs of program $1, with MIDNIGHT_WINDOW
# set to $2 (empty: a window as the desktop gives).
best() {
  local fastest="" ms
  for _ in $(seq "$runs"); do
    ms=$(env ${2:+MIDNIGHT_WINDOW=$2} "$1" | awk -F': ' '$1 == "ms_per_tick" { print $2 }')
    if [ -z "$fastest" ] || awk -v a="$ms" -v b="$fastest" 'BEGIN { exit !(a < b) }'; then
      fastest=$ms
    fi
  done
  echo "$fastest"
}

device=""
printf '%-10s %-11s %12s %12s %12s\n' shapes "drawn by" shown headless frame
for shapes in 0 10000 100000 1000000; do
  for side in gpu cpu; do
    name="draw_${side}_$shapes"
    if [ "$side" = gpu ]; then
      sed "s/^SHAPES = .*/SHAPES = $shapes/" "$here/bench/draw_frame.12am" > "$work/$name.12am"
    else
      sed -e "s/^SHAPES = .*/SHAPES = $shapes/" -e 's/^gpu system show/system show/' \
        "$here/bench/draw_frame.12am" > "$work/$name.12am"
    fi
    $midnight build "$work/$name.12am" -o "$work/$name"
    if [ -z "$device" ]; then
      device=$(MIDNIGHT_GPU=${MIDNIGHT_GPU:+$MIDNIGHT_GPU,}info MIDNIGHT_WINDOW=headless "$work/$name" 2>&1 >/dev/null |
        sed -n '1s/^midnight: gpu: //p')
    fi
    shown=$(best "$work/$name" "")
    headless=$(best "$work/$name" headless)
    frame=$(awk -v a="$shown" -v b="$headless" 'BEGIN { printf "%.3f", a - b }')
    printf '%-10s %-11s %9s ms %9s ms %9s ms\n' "$shapes" "$side" "$shown" "$headless" "$frame"
  done
done
echo "device: $device"
