# Builds and runs the benchmarks.
#
# examples/bench.12am and bench/baseline.c do the same work and are built
# with the same C flags. Each runs RUNS times (default 5) and its fastest
# run counts. Then bench/churn.12am runs once. BENCH_SRC is the directory
# that holds examples/ and bench/.

runs=${RUNS:-5}
cc=${MIDNIGHT_CC:-cc}
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

midnight build "$BENCH_SRC/examples/bench.12am" -o "$work/bench"
march=()
if [ "$(uname -m)" = x86_64 ]; then
  march=(-march=x86-64-v3)
fi
"$cc" -std=c11 -O2 -fwrapv -ffp-contract=off "${march[@]}" -o "$work/baseline" "$BENCH_SRC/bench/baseline.c" -lm
midnight build "$BENCH_SRC/bench/churn.12am" -o "$work/churn"

# Prints the value of one `key: value` line of a program's saved output.
value() {
  awk -F': ' -v key="$2" '$1 == key { print $2 }' "$work/$1.out"
}

# Runs a program once. Its output goes to $work/NAME.out and its peak
# resident memory, in bytes, to $work/NAME.rss.
run_once() {
  hyperfine -N --runs 1 --style none \
    --output "$work/$1.out" --export-json "$work/$1.json" \
    "$work/$1" >/dev/null 2>&1
  jq '.results[0].memory_usage_byte[0]' "$work/$1.json" >"$work/$1.rss"
}

# Runs a program RUNS times. Keeps the output of the fastest run in
# $work/NAME.out and the largest peak memory in $work/NAME.rss.
run_best() {
  local best="" peak=0 seconds rss
  for _ in $(seq "$runs"); do
    run_once "$1"
    seconds=$(value "$1" seconds)
    rss=$(cat "$work/$1.rss")
    if [ "$rss" -gt "$peak" ]; then
      peak=$rss
    fi
    if [ -z "$best" ] || awk -v a="$seconds" -v b="$best" 'BEGIN { exit !(a < b) }'; then
      best=$seconds
      cp "$work/$1.out" "$work/$1.best"
    fi
  done
  mv "$work/$1.best" "$work/$1.out"
  echo "$peak" >"$work/$1.rss"
}

megabytes() {
  awk -v bytes="$(cat "$work/$1.rss")" 'BEGIN { printf "%.1f", bytes / 1048576 }'
}

report() {
  printf '%-12s %8s ms/tick %14s updates/s %8s MB peak\n' \
    "$2" "$(value "$1" ms_per_tick)" "$(value "$1" updates_per_second)" "$(megabytes "$1")"
}

run_best bench
run_best baseline

echo "$(value bench entities) entities, $(value bench ticks) ticks, fastest of $runs runs"
report bench bench.12am
report baseline baseline.c
awk -v m="$(value bench seconds)" -v c="$(value baseline seconds)" \
  'BEGIN { printf "%-12s %8.2f (bench.12am time / baseline.c time)\n", "ratio", m / c }'
if [ "$(value bench checksum)" = "$(value baseline checksum)" ]; then
  echo "checksum     $(value bench checksum) (both)"
else
  echo "checksum     bench.12am $(value bench checksum), baseline.c $(value baseline checksum)"
fi

run_once churn
echo
echo "Structural changes on $(value churn entities) entities, operations per second"
for key in spawn lookup add remove despawn; do
  printf '%-12s %14s\n' "$key" "$(value churn "${key}_per_second")"
done
printf '%-12s %14s MB\n' "peak" "$(megabytes churn)"
