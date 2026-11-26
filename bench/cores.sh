# Builds and runs the benchmarks of loops that run on several cores.
#
# Each program runs with MIDNIGHT_GPU=off, so that a GPU system runs its C
# implementation, twice: with MIDNIGHT_THREADS=1, on one thread, and with
# MIDNIGHT_THREADS unset, on one thread for each core the process may run
# on. Each way it runs RUNS times (default 5) and its fastest run counts.
# The two checksums of a program must be the same text: the number of
# threads changes no result.
#
# BENCH_SRC is the directory that holds bench/ and examples/.

runs=${RUNS:-5}
# The programs, each with what its loop is.
PROGRAMS=(
  "bench/gpu_gravity:GPU block, gravity among 16,384 bodies"
  "bench/gpu_gravity_strict:the same, strict"
  "bench/gpu_move:GPU block, move of 1,000,000"
  "examples/bench:each, move of 1,000,000"
  "bench/cores_age:each, 1,000,000 age, 1,000 despawn"
  "bench/cores_flee:each, 100,000 look at 64"
  "bench/gpu_read_tables:GPU block, 64 tables of 100"
  "bench/cores_small:each, 64 tables of 100"
)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

for program in "${PROGRAMS[@]%%:*}"; do
  midnight build "$BENCH_SRC/$program.12am" -o "$work/$(basename "$program")"
done

# Prints the value of one `key: value` line of a saved output.
value() {
  awk -F': ' -v key="$2" '$1 == key { print $2 }' "$work/$1.out"
}

# Runs program $1 RUNS times and keeps the output of the fastest run in
# $work/$1.$2.out: on one thread when $2 is `one`, on the default number
# when it is `all`.
run_best() {
  local best="" ms
  for _ in $(seq "$runs"); do
    if [ "$2" = one ]; then
      MIDNIGHT_GPU=off MIDNIGHT_THREADS=1 "$work/$1" >"$work/$1.run"
    else
      (
        unset MIDNIGHT_THREADS
        MIDNIGHT_GPU=off "$work/$1" >"$work/$1.run"
      )
    fi
    ms=$(awk -F': ' '$1 == "ms_per_tick" { print $2 }' "$work/$1.run")
    if [ -z "$best" ] || awk -v a="$ms" -v b="$best" 'BEGIN { exit !(a < b) }'; then
      best=$ms
      cp "$work/$1.run" "$work/$1.$2.out"
    fi
  done
}

# The number of threads a program uses when MIDNIGHT_THREADS is unset.
threads=$(MIDNIGHT_GPU=off MIDNIGHT_THREADS=info "$work/cores_small" 2>&1 >/dev/null |
  sed -n 's/^midnight: threads: \([0-9]*\)$/\1/p')

echo "Loops on several cores: ms per tick on one thread and on $threads; fastest of $runs runs"
printf '%-40s %12s %12s %8s\n' "" "1 thread" "$threads threads" "times"
differ=0
for entry in "${PROGRAMS[@]}"; do
  name=$(basename "${entry%%:*}")
  run_best "$name" one
  run_best "$name" all
  one=$(value "$name.one" ms_per_tick)
  all=$(value "$name.all" ms_per_tick)
  printf '%-40s %12.4f %12.4f %8.2f\n' "${entry#*:}" "$one" "$all" \
    "$(awk -v a="$one" -v b="$all" 'BEGIN { print a / b }')"
  if [ "$(value "$name.one" checksum)" != "$(value "$name.all" checksum)" ]; then
    echo "  $name: the checksum is $(value "$name.one" checksum) on one thread and $(value "$name.all" checksum) on $threads"
    differ=1
  fi
done
if [ "$differ" = 1 ]; then
  echo "A checksum depends on the number of threads."
  exit 1
fi
echo "Every checksum is the same on one thread and on $threads."
