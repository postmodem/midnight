# Builds and runs the GPU benchmarks.
#
# bench/gpu_gravity.12am, bench/gpu_move.12am, bench/gpu_move_read.12am and
# the programs of COSTS each run on the GPU and on the CPU. "On the CPU" is
# the same program with MIDNIGHT_GPU=off, which runs the C implementation of
# its GPU system on one thread, as a machine with no GPU does.
# bench/gpu_gravity_64k.12am runs on the GPU only. Each runs RUNS times
# (default 3) and its fastest run counts. The gravity checksums of the GPU
# and the CPU must agree within GRAVITY_TOLERANCE, relative. Then the
# transfers `midnight plan` lists for the two move programs are compared
# with the ones MIDNIGHT_GPU=stats counts.
#
# The words of MIDNIGHT_GPU in the environment apply to the GPU runs, so
# MIDNIGHT_GPU=software lets them use a software adapter. BENCH_SRC is the
# directory that holds bench/.

runs=${RUNS:-3}
# The cost programs, each with what it measures.
COSTS=(
  "gpu_block:one block a tick, 1,000 entities"
  "gpu_blocks8:eight blocks a tick, 1,000 entities"
  "gpu_fallible:one block that can fail"
  "gpu_read_tables:a reader of 2 components, 64 tables"
  "gpu_poke:one e[C] read and write a tick"
  "gpu_spawn:one spawn a tick into 1,000,000"
)
GRAVITY_TOLERANCE=0.00001
words=${MIDNIGHT_GPU:+$MIDNIGHT_GPU,}
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

for name in gpu_gravity gpu_gravity_64k gpu_move gpu_move_read "${COSTS[@]%%:*}"; do
  midnight build "$BENCH_SRC/bench/$name.12am" -o "$work/$name"
done

# Prints the value of one `key: value` line of a saved output.
value() {
  awk -F': ' -v key="$2" '$1 == key { print $2 }' "$work/$1.out"
}

# Runs program $2 once with MIDNIGHT_GPU set to $3. Its output goes to
# $work/$1.run and its standard error to $work/$1.err.
run_once() {
  if ! MIDNIGHT_GPU=$3 "$work/$2" >"$work/$1.run" 2>"$work/$1.err"; then
    cat "$work/$1.err" >&2
    exit 1
  fi
}

# Runs program $2 RUNS times with MIDNIGHT_GPU set to $3 and keeps the
# output of the fastest run in $work/$1.out. Prints the name of any GPU
# block that ran on the CPU although a device was asked for.
run_best() {
  local best="" ms
  for _ in $(seq "$runs"); do
    run_once "$@"
    ms=$(awk -F': ' '$1 == "ms_per_tick" { print $2 }' "$work/$1.run")
    if [ -z "$best" ] || awk -v a="$ms" -v b="$best" 'BEGIN { exit !(a < b) }'; then
      best=$ms
      cp "$work/$1.run" "$work/$1.out"
    fi
  done
  sed -n 's/^midnight: gpu: \(.* runs on the CPU: .*\)$/note: \1/p' "$work/$1.err"
}

row() {
  printf '%-31s %10.3f ms/tick%s\n' "$1" "$(value "$2" ms_per_tick)" "${3:-}"
}

# The device a GPU system gets here, as MIDNIGHT_GPU=info names it.
run_once probe gpu_move "${words}info"
device=$(sed -n '1s/^midnight: gpu: //p' "$work/probe.err")
if [ "$device" = "no device" ]; then
  device=""
  echo "device: none. Every GPU system runs on the CPU, so only CPU times follow."
else
  echo "device: $device"
fi

run_best gravity_cpu gpu_gravity off
run_best move_cpu gpu_move off
run_best read_cpu gpu_move_read off
if [ -n "$device" ]; then
  run_best gravity_gpu gpu_gravity "${words}info"
  run_best move_gpu gpu_move "${words}info"
  run_best read_gpu gpu_move_read "${words}info"
fi
for cost in "${COSTS[@]%%:*}"; do
  run_best "${cost}_cpu" "$cost" off
  if [ -n "$device" ]; then
    run_best "${cost}_gpu" "$cost" "${words}info"
  fi
done
if [ -n "$device" ]; then
  run_best gravity64_gpu gpu_gravity_64k "${words}info"
fi

echo
echo "Gravity: $(value gravity_cpu bodies) bodies, each pulled by every body; fastest of $runs runs"
if [ -n "$device" ]; then
  row "GPU" gravity_gpu "  ($(value gravity_gpu ticks) ticks)"
fi
row "CPU" gravity_cpu "  ($(value gravity_cpu ticks) ticks)"
if [ -n "$device" ]; then
  awk -v cpu="$(value gravity_cpu ms_per_tick)" -v gpu="$(value gravity_gpu ms_per_tick)" \
    'BEGIN { printf "%-31s %10.1f (CPU time / GPU time)\n", "ratio", cpu / gpu }'
fi
echo "checksum $(value gravity_cpu checksum) (CPU)"
if [ -n "$device" ]; then
  echo "checksum $(value gravity_gpu checksum) (GPU)"
  row "GPU, $(value gravity64_gpu bodies) bodies" gravity64_gpu "  ($(value gravity64_gpu ticks) ticks)"
fi

echo
echo "Move: $(value move_cpu entities) entities, $(value move_cpu ticks) ticks; fastest of $runs runs"
row "CPU" move_cpu
if [ -n "$device" ]; then
  row "GPU, data stays on the GPU" move_gpu
  row "GPU, read on the CPU each tick" read_gpu
fi
row "CPU, read on the CPU each tick" read_cpu
moves=(move_cpu read_cpu)
if [ -n "$device" ]; then
  moves+=(move_gpu read_gpu)
fi
checksums=$(for name in "${moves[@]}"; do value "$name" checksum; done | sort -u)
if [ "$(wc -l <<<"$checksums")" -eq 1 ]; then
  echo "checksum $checksums (every run)"
else
  for name in "${moves[@]}"; do
    echo "checksum $(value "$name" checksum) ($name)"
  done
fi

status=0

if [ -n "$device" ] && ! awk -v cpu="$(value gravity_cpu checksum)" -v gpu="$(value gravity_gpu checksum)" -v t="$GRAVITY_TOLERANCE" \
  'BEGIN { d = cpu - gpu; if (d < 0) d = -d; m = cpu < 0 ? -cpu : cpu; exit !(d <= t * m) }'; then
  echo "the gravity checksums differ by more than $GRAVITY_TOLERANCE of the CPU's"
  status=1
fi

echo
echo "Costs: fastest of $runs runs"
printf '%-40s %12s %12s\n' "" "GPU ms/tick" "CPU ms/tick"
for cost in "${COSTS[@]}"; do
  name=${cost%%:*}
  gpu="-"
  if [ -n "$device" ]; then
    gpu=$(value "${name}_gpu" ms_per_tick)
  fi
  printf '%-40s %12s %12s\n' "${cost#*:}" "$gpu" "$(value "${name}_cpu" ms_per_tick)"
done

# Prints the plan of program $1 and what `stats` counted in each tick but
# the first and the last, whose line also counts the code after the loop.
# Both must come to $2 downloads and no upload.
transfers() {
  local name=$1 downloads=$2 expected counted ticks
  expected="0 uploads, $downloads downloads"
  if [ "$downloads" -eq 1 ]; then
    expected="0 uploads, 1 download"
  fi

  echo "$name.12am"
  midnight plan "$BENCH_SRC/bench/$name.12am" >"$work/$name.plan"
  if [ -s "$work/$name.plan" ]; then
    sed 's/^/  plan     /' "$work/$name.plan"
  else
    echo "  plan     no transfer"
  fi
  if [ "$(wc -l <"$work/$name.plan")" -ne "$downloads" ] ||
    [ "$(grep -c '^download ' "$work/$name.plan" || true)" -ne "$downloads" ]; then
    echo "  expected $downloads downloads and no upload in the plan"
    status=1
  fi

  if [ -z "$device" ]; then
    echo "  counted  nothing, with no device"
    return
  fi
  run_once "$name.stats" "$name" "${words}stats"
  sed -n 's/^midnight: gpu: tick [0-9]*: //p' "$work/$name.stats.err" | sed '1d;$d' >"$work/$name.ticks"
  ticks=$(wc -l <"$work/$name.ticks")
  counted=$(sort -u "$work/$name.ticks")
  if [ "$counted" = "$expected" ]; then
    echo "  counted  $counted in each of the $ticks ticks but the first and the last"
  else
    sort "$work/$name.ticks" | uniq -c | sed 's/^ *\([0-9]*\) \(.*\)$/  counted  \2 in \1 ticks/'
    echo "  expected $expected in each of the $ticks ticks but the first and the last"
    status=1
  fi
}

echo
echo "Transfers per tick: planned (midnight plan) and counted (MIDNIGHT_GPU=stats)"
transfers gpu_move 0
transfers gpu_move_read 1
exit "$status"
