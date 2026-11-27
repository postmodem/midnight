# Benchmarks

```sh
nix run .#bench
```

builds `examples/bench.12am` (one million entities with two `Float32`
components, moved for 100 ticks) and `baseline.c` (the same work written by
hand) with the same C flags, and prints the time per tick of each, their
ratio and their peak memory. It then runs `churn.12am`, which measures
`spawn`, `despawn`, `add`, `remove` and `e[C]`.

```sh
nix run .#bench-gpu
```

runs gravity among 16,384 bodies on the GPU and on the CPU and prints the
time per tick of each, and a checksum of the bodies after four ticks from
each; the two must agree within a relative 0.00001. It does the same for
gravity as a strict GPU system, whose two checksums must be equal. It runs
gravity among 65,536 bodies on the GPU only. It then moves one million
entities three ways and prints the time per tick of each: on the CPU; on
the GPU with the data left there; and on the GPU with a CPU system reading
the result in every tick.

It then prints, on the GPU and on the CPU, the time per tick of ten costs:
one block, eight blocks, a block that can fail, a reader over 64 tables, one
`e[C]` read and write, one `spawn`, one `despawn` and one `add` of a tag a
tick, a loop that assigns only under a condition, and a component both
sides assign whole. "On the CPU" is the same program with
`MIDNIGHT_GPU=off`, on as many threads as the program chooses;
`MIDNIGHT_THREADS=1` in the environment gives the times of one thread.

Last, it prints the transfer plan of the two GPU cases beside the transfers
`MIDNIGHT_GPU=stats` counted, and checks what `MIDNIGHT_GPU=stats` counts in
a tick for the move programs and the ten costs; it fails if any of that
differs. On a machine with no hardware GPU it prints the CPU times only;
`MIDNIGHT_GPU=software` in the environment lets the GPU runs use software
Vulkan. The programs are the `gpu_*.12am` files here.

```sh
nix run .#bench-cores
```

times loops on one thread and on the number of threads a program chooses,
with `MIDNIGHT_GPU=off`, and prints the time per tick of each: gravity among
16,384 bodies, also strict, and a move of one million entities, as GPU
systems on the CPU; the ordinary `move` of `examples/bench.12am`; a GPU
block over 64 tables of 100 entities (`gpu_read_tables`); `cores_age`, a
loop over a million entities that counts and despawns the few that expire;
`cores_flee`, a loop with a nested `each`; and `cores_small`, a loop over 64
tables of 100 entities, which stays on one thread. The two runs of each
program must print the same checksum.

```sh
nix develop -c bash tests/window/compositor.sh bash bench/draws.sh
```

times `draw_frame.12am`, which draws 0 to 1,000,000 shapes a tick in a
window of 1920 by 1080, from a GPU system and from an ordinary one, with the
window shown in the compositor of the window tests and headless, and prints
the time per tick of each and their difference: what a frame of those
shapes adds to a tick. `VK_DRIVER_FILES` chooses the device.

The reference's "What things cost" has the numbers these measure, and what
follows from them.
