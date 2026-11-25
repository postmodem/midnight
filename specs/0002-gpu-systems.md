# 0002 — GPU systems: run data-parallel systems on the GPU

- **Status:** implemented
- **Depends on:** 0001
- **Tested on:** x86_64-linux, 2026-11-25

## Goal

A system marked `gpu` runs its queries as compute shaders, over the same
component arrays the rest of the program uses:

```ruby
component Body
  x: Float32
  y: Float32
  vx: Float32
  vy: Float32
  mass: Float32
end

gpu system gravity
  each Body do |a|
    ax = 0.0
    ay = 0.0
    each Body do |b|
      dx = b.x - a.x
      dy = b.y - a.y
      d2 = dx * dx + dy * dy + SOFTENING
      pull = b.mass / (d2 * Math.sqrt(d2))
      ax += dx * pull
      ay += dy * pull
    end
    a.vx += ax * Time.dt
    a.vy += ay * Time.dt
    a.x += a.vx * Time.dt
    a.y += a.vy * Time.dt
  end
end
```

The programmer writes ordinary Midnight. The compiler checks that the system
is data-parallel, generates the shader, and works out at compile time when
component data has to move between CPU and GPU memory. On a machine with no
GPU the same system runs on the CPU and means the same thing.

**What this is for.** Three kinds of work get faster:

- heavy arithmetic per entity,
- entity-against-entity work (a query nested in a query), where the GPU's
  advantage grows with the entity count,
- data that stays in GPU memory from tick to tick, which a later graphics
  spec can draw without copying it back.

It does not help a light system whose components CPU systems also touch every
tick. Moving the data costs more than the arithmetic saves. The compiler
reports those transfers so the cost is visible before the program runs.

## Scope

- The `gpu system` declaration, the rules its body must follow, and a
  diagnostic for each rule.
- Snapshot semantics, 32-bit arithmetic and run-time errors inside GPU
  systems.
- A WGSL emitter beside the C emitter.
- Runtime support: a GPU device, a GPU copy of each column a GPU system
  uses, transfers, and dispatch.
- A CPU implementation of every GPU system, used when there is no device.
- `midnight plan FILE`: prints the transfers one tick performs.
- Flake: `wgpu-native` and the Vulkan loader from the pinned nixpkgs, linked
  only into programs that declare a GPU system; a check that runs the GPU
  tests on software Vulkan; `apps.gpu-test` and `apps.bench-gpu`.
- `examples/nbody.12am`, two benchmarks, and the reference updated. The
  reference becomes Midnight 0.2 and the compiler 0.2.0.

## Non-goals

- Windows, drawing, input and audio. The device, the buffers and the shader
  emitter built here are what a graphics spec will draw with.
- `spawn`, `despawn`, `add`, `remove`, `emit`, `on`, output, strings and
  lists inside a GPU system.
- `e[C]`, `e.has?`, `e.alive?` and `World.count` inside a GPU system.
- Collecting a result across entities into a resource (a sum, a minimum).
- Running ordinary systems, or the CPU implementation of a GPU system, on
  several CPU threads. A threading spec follows this one.
- Spatial structures. A nested query still visits every pair.
- Bit-identical results between the GPU and the CPU implementation.
- Moving rows between tables on the GPU. A structural change to a table
  brings that table's columns back to CPU memory first.
- More than one GPU, or choosing among several.
- Recovering from a lost device, or stopping a GPU system that does not
  finish.
- Running on real hardware other than x86_64-linux. The other three systems
  must evaluate.

## Design

### The language

**`gpu system`.** The body is one or more `each` blocks and nothing else.
Each of them is a *GPU block*. Inside one, these are allowed:

- local variables, arithmetic, comparison, `&&`, `||` and `!`;
- `if`, `unless`, `case` and the conditional operator;
- `while`, `until`, `loop` and counting loops, with `break` and `next`;
- constructing records, reading and assigning their fields, and calling
  their methods and operators;
- reading constants, resource fields and `Time`;
- the number built-ins except `to_s`, `min`, `max`, the `Math` functions,
  `rand` and `rand(n)`;
- `==` and `!=` on entities, and `Entity.none`;
- calling functions that follow these rules;
- one nested `each`, directly inside the block.

Everything else is a compile error that names the rule and the reason.

`next` directly inside a GPU block ends the run for that entity and keeps
what it has written. `break` directly inside a GPU block and `return`
anywhere in one are errors: runs have no order, so there is nothing to stop.
Inside the nested `each`, `break` and `next` work as usual. The nested
`each` visits entities in the order an ordinary `each` would.

**What a GPU system may touch.**

- The terms of its queries are `Entity` or components that are plain (no
  string or list, at any depth) with every numeric field at most 32 bits
  wide and signed if it is 32: `Float32`, `Int32`, `Int16`, `Int8`,
  `UInt16`, `UInt8`, plus `Bool`, enums, `Entity` and structs of these. A
  `Float`, `Int` or `UInt32` field is an error with the hint to declare it
  `Float32` or `Int32`. Types named in `with:` and `without:` are not
  restricted.
- It writes only to the component parameters of the outer `each`. The
  parameters of the nested `each`, resources and constants are read-only.
- The resource fields and constants it reads are numbers, `Bool`s, enums,
  entities or plain structs.
- A function it calls takes and returns such values, does not recurse,
  directly or through other functions, and contains no query and nothing
  else that a GPU block may not contain. It may read resources and
  constants.

**Snapshot semantics.** The block of a GPU `each` runs once for every
matching entity, as if all at once.

- A run reads and writes its own entity's components through the outer
  parameters, and sees its own writes.
- The parameters of the nested `each` show every entity, the run's own
  included, as it was when the outer `each` began.
- Resource fields and `Time` are read once, when the outer `each` begins.
- The `each` blocks of one system run in order. Each sees what the ones
  before it wrote.

So the result does not depend on the order entities are processed in.

**32-bit arithmetic.** Inside a GPU system, and inside any function while it
is called from one, `Float` is 32 bits wide and `Int` is 32 bits wide and
wraps. The source does not change: the same `def` can be called from ordinary
code, where it computes at 64 bits, and from a GPU system. Resource fields
and constants are converted on the way in: a `Float` is rounded and an `Int`
wraps. A `Float` converted to `Int` gives the nearest 32-bit value when it is
out of range and 0 for NaN. An `Int` literal that does not fit in 32 bits is
a compile error.

**Run-time errors.** The operations that can fail in ordinary code can fail
in a GPU system: `/` and `%` by zero on `Int`, a negative `Int` exponent,
`rand(n)` with `n <= 0`, and storing an `Int` that does not fit in a compact
field. A run that fails keeps going with 0 as the result of the failed
operation (the stored value wraps), every other run finishes, and the
program then stops, when the `each` ends, with the usual message and
status 1. No later statement sees what the `each` wrote. When several
operations fail, the one earliest in the source is reported. The message
names the file and line and not the entity or the value:

```
swarm.12am:41: runtime error: value does not fit in UInt8
```

**`rand` inside a GPU system** is a function of the seed, the tick, the
block, the entity, and how many random numbers that run has drawn so far.
It needs no shared state, and gives the same numbers on the GPU and on the
CPU. The seed is the one last given to `srand`, or the default. Drawing
numbers in a GPU system does not advance the generator the rest of the
program uses.

**Determinism.** The CPU implementation gives the same result on every run,
in both build modes. A GPU gives the same result from run to run on the same
machine and driver. Between the GPU and the CPU implementation, or between
two different GPUs, results agree closely but not to the last bit: GPUs are
allowed small differences in division, square roots and trigonometry, and
may combine a multiplication and an addition into one operation.

### The compiler

**One system, two outputs.** For each GPU block the compiler emits a WGSL
compute shader, embedded in the executable as text, and a C function with
the same meaning. Functions called from GPU systems are compiled once more
at 32 bits, for both outputs. The C implementation runs on one thread and is
compiled with floating-point contraction off, so that it computes the same
thing at every optimization level and on every CPU.

**Buffers are byte-for-byte the columns.** A GPU buffer holds a column
exactly as CPU memory does, so a transfer is a plain copy. The shader sees
the buffer as an array of 32-bit words; the emitter generates the shifts and
casts that read and write each field, including one-byte and two-byte fields.

**Rows own whole words.** A shader can only store whole words, and two runs
that stored into one word would overwrite each other's bytes. So the size of
a component that a GPU block writes is rounded up to a multiple of 4 bytes.
The reference's rule for memory per entity says so.

**Writes go straight into the column.** A run reads and writes only its own
row of the outer columns, so the shader updates them in place.

**The nested `each` reads a snapshot.** For each component the nested query
reads there is one buffer holding the rows of every table the query matches,
one table after another, and the nested loop walks it. When the query
matches a single table and the block does not write that component, the
buffer is the column itself. Otherwise it is a copy, made in GPU memory when
the outer `each` begins.

**Run-time errors.** A shader that contains an operation that can fail has
one more word to write to: the line of the earliest failure. The runtime
reads that word back when the `each` ends. Blocks with no such operation,
which is most arithmetic on `Float32` and `Int32` fields, have no such word
and nothing is read back.

**Transfers are planned from the source.** Systems run in a fixed order and
the compiler knows which components every query and every `e[C]` reads and
writes (0001 records this), and which systems make structural changes. From
that it derives, for one tick in steady state, each point where a column
must be copied to the GPU or back, assuming every statement of every system
runs. `midnight plan FILE` prints them, one line per transfer, naming the
direction, the component and the two systems it travels between:

```
download  Position  move -> report
download  Position  move -> reap     on despawn
upload    Position  reap -> move     on despawn
```

A transfer caused by a structural change is marked with the operation and
happens only in a tick where that operation runs. Top-level code that
touches a component a GPU system uses is listed as `top level`; the plan
does not know how often it runs. A line stands for one transfer per table
that holds the component. A program whose plan is empty keeps its GPU data
on the GPU.

### The runtime

**Where a column lives.** Every column a GPU system uses has a CPU copy and
a GPU copy, and a note of which is current: one or both. A GPU block that
finds only the CPU copy current uploads it. CPU code (a query, `e[C]`, a
structural change, the top level) that finds only the GPU copy current
downloads it first. A write on either side leaves only that side current.
Columns that no GPU system uses have no GPU copy and pay nothing; the
compiler knows which those are. The handles of a table's entities are a
column like any other, uploaded when a GPU block has an `Entity` term or
calls `rand`.

**Dispatch.** One compute dispatch per matching table per GPU block, all of
a block's dispatches in one submission. The row count, the tick, the seed
and the resource fields and constants the block reads go in a small uniform
buffer. A table too large for one dispatch is split across several.

**`World.tick` waits.** A tick returns when the GPU has finished the work
the tick gave it. A program that times its ticks with `clock` measures the
work, and unfinished work cannot pile up.

**The GPU library is `wgpu-native` 27,** used through its C headers
(`webgpu.h` and `wgpu.h`, as shipped with that version), from the pinned
nixpkgs. It runs on Vulkan on Linux and Metal on macOS, covers the four
systems the flake offers, and works without a window. Its shading language
is text, which suits a compiler that already emits C, and the same device
and language serve drawing later. The code that talks to it is
`runtime/midnight_gpu.c`, compiled only into programs that declare a GPU
system. Programs with no GPU system do not link it and stay dependent on
libc alone.

**Linking.** nixpkgs ships `wgpu-native` as a shared library, which opens
the Vulkan loader by name when it starts, and a runpath on the executable is
not consulted for that. So on Linux a GPU program names `libvulkan.so.1` as
a library it needs itself, with runpaths to it and to `wgpu-native`; the
loader is then already in the process when `wgpu-native` asks for it. The
GPU driver comes from the host. `LD_LIBRARY_PATH` is never set. The flags
come from `MIDNIGHT_GPU_FLAGS`, which the packaged compiler and the dev
shell set. Without it, `midnight` says so in one line and builds the program
with its CPU implementation only.

**Which device.** The runtime asks for the fastest hardware adapter and
requests the limits that adapter offers. `MIDNIGHT_GPU` is a comma-separated
list of:

| Word       | Meaning                                                         |
| ---------- | --------------------------------------------------------------- |
| `off`      | Use no device.                                                  |
| `software` | Accept a software adapter (Mesa's `llvmpipe`) when there is no hardware one. |
| `info`     | Print one line to standard error naming the device in use, or saying that there is none, and one line for each GPU block that will run on the CPU. |
| `stats`    | Print one line to standard error at the end of each tick: the uploads and downloads it performed. |

**No device.** If no hardware adapter is found, or `off` is set, every GPU
system runs its C implementation. A single block also runs its C
implementation when it needs more storage buffers than the device binds at
once, or a column larger than the device's largest binding.

**GPU failures.** A validation error or a lost device after start-up prints
`midnight: GPU error: …` and exits with status 70.

### Tests

- Every rule under "The language" has a `tests/fail` case.
- Every run-time error has a `tests/panic` case in a GPU system.
- `tests/run` programs with GPU systems run with `MIDNIGHT_GPU=off` inside
  the package's check phase, in both build modes, and must print exactly
  `NAME.out`.
- `checks.gpu`, on Linux, runs the same programs with
  `MIDNIGHT_GPU=software` on Mesa's software Vulkan inside the build
  sandbox, and compares the output with `NAME.out`: text exactly, numbers
  within the tolerance the program states in a `# tolerance:` comment
  (exact when there is none).
- `apps.gpu-test` does the same on the machine's real device.
- One of those programs prints `MIDNIGHT_GPU=stats` lines, and the test
  checks them against `midnight plan`.

### Measured in planning

On an RTX 5090 (driver 595.71.05) and a Ryzen 9 9950X3D, NixOS 26.05, with
`wgpu-native` 27.0.4.0 and hand-written shaders of the shape the emitter
will produce:

- **It runs here.** Compute without a window works from an account with no
  special groups, linked as described under "Linking". With only a runpath
  on the executable, `wgpu-native` finds no adapter.
- **Software Vulkan works in the sandbox.** The same program finds
  `llvmpipe` and computes correctly inside a Nix build, with
  `VK_DRIVER_FILES` pointing at Mesa's `lvp_icd` file.
- **One-byte and two-byte fields** read and written through shifts on words
  come back exact.
- **Gravity, 16,384 bodies:** 1.29 ms per tick on the GPU, snapshot copy
  included; 356 ms on one CPU thread.
- **GPU against CPU,** 2,048 bodies after 100 ticks: positions differ by at
  most 7e-7, or 9e-5 relative. Contraction on the CPU side does not narrow
  it.
- **Moving entities** (two `Float32` pairs, time per tick in ms):

  | Entities  | CPU    | GPU, data stays | GPU, tick waits | GPU, download each tick |
  | --------- | ------ | --------------- | --------------- | ----------------------- |
  | 1,000     | 0.0002 | 0.010           | 0.038           | 0.031                   |
  | 10,000    | 0.0018 | 0.016           | 0.030           | 0.041                   |
  | 100,000   | 0.018  | 0.017           | 0.030           | 0.081                   |
  | 1,000,000 | 0.19   | 0.020           | 0.033           | 0.40                    |
  | 4,000,000 | 1.23   | 0.045           | 0.057           | 2.26                    |

  A dispatch that is waited for costs about 0.03 ms whatever its size, so a
  light system wins on the GPU from roughly 200,000 entities when its data
  stays there, and at no size when a CPU system reads the result every
  tick. A transfer costs about 0.05 to 0.07 ms per megabyte in either
  direction.
- **Limits.** The 5090 and `llvmpipe` each bind about a million storage
  buffers per shader; the largest binding is 2 GB on the 5090 and 128 MB on
  `llvmpipe`.

### Open questions for planning

- **Many small tables.** A block makes one dispatch per table. Measure a
  query that matches dozens of small tables and record the cost; batching
  them is a later spec's work unless the cost is far above the 0.03 ms of a
  single dispatch.
- **The error word.** Recording the earliest failing line needs an atomic
  minimum in WGSL. The first runtime slice must show it on the 5090 and on
  `llvmpipe`.
- **The numbers above, from the compiler.** The reference's cost section
  gets the crossover and the transfer cost as measured with generated
  shaders, not hand-written ones.

## Acceptance criteria

All run on x86_64-linux. Criteria 2, 6, 7 and 8 need a hardware GPU.

1. `nix flake check` passes, including every GPU-system test on the CPU
   implementation and, in `checks.gpu`, on software Vulkan.
2. `MIDNIGHT_GPU=info midnight run examples/nbody.12am` names a hardware
   device, and every position it prints after 100 ticks differs from the
   `MIDNIGHT_GPU=off` run's by at most 0.001 × max(1, |position|).
3. `MIDNIGHT_GPU=off midnight run examples/nbody.12am` prints the same output
   on two runs, and in debug mode.
4. Each rule under "The language" has a `tests/fail` case: output, a
   structural change, `emit`, `e[C]`, a string field, a 64-bit field, a
   `UInt32` field, a write to a resource, a write to a nested parameter, a
   second level of nested `each`, `break` in the block, `return`, recursion,
   a query in a called function, an `Int` literal too large, and a statement
   outside `each`.
5. Each run-time error in a GPU system has a `tests/panic` case that stops
   with the same message and status 1 with `MIDNIGHT_GPU=off` and in
   `checks.gpu`.
6. `nix run .#bench-gpu` runs the gravity system on 16,384 bodies on the GPU
   and on the CPU and prints both times per tick. On a machine with a
   discrete GPU, the GPU is at least 10 times faster.
7. The same app runs a light system (move one million entities) three ways
   and prints time per tick for each, with no threshold: on the CPU; on the
   GPU with the data left there; and on the GPU with a CPU system reading
   the result every tick.
8. For the third case in criterion 7, `midnight plan` lists exactly one
   download and no upload, and `MIDNIGHT_GPU=stats` reports one download and
   no upload in every tick after the first. For the second case the plan is
   empty and every tick after the first reports none.
9. `ldd` on the release build of `examples/hello.12am` lists libc and libm
   and no GPU library.
10. Every command in the README works as written.

## Outcome

Implemented as specified, with the differences listed under "Differences
from the design". All ten criteria pass on x86_64-linux: an RTX 5090 (driver
595.71.05) beside a Ryzen 9 9950X3D, `wgpu-native` 27.0.4.0, clang 21.1.8,
and Mesa's `llvmpipe` for software Vulkan.

### Acceptance criteria

| #  | Result |
| -- | ------ |
| 1  | `nix flake check` passes. The package's check phase runs `cargo test`: 302 unit tests and the suite, with every GPU system on its CPU implementation, in both build modes. `checks.gpu` ran 90 runs of 45 programs on `llvmpipe` inside the sandbox (32 of `tests/run`, 12 of `tests/panic` and `examples/nbody.12am`, each in release and debug). `nix flake check --no-build --all-systems` passes. |
| 2  | `MIDNIGHT_GPU=info midnight run examples/nbody.12am` prints `midnight: gpu: NVIDIA GeForce RTX 5090 (Vulkan)`. Against the `MIDNIGHT_GPU=off` run, 2 of the 32 printed numbers differ, by 0.00001, which is 6.0e-6 × max(1, \|position\|). Over all 256 bodies, unrounded, the largest difference is 9.5e-6 (7.8e-6 on `llvmpipe`). |
| 3  | Two `MIDNIGHT_GPU=off` runs and one with `--debug` print the same bytes, those of `examples/nbody.out`. |
| 4  | 85 `tests/fail/gpu_*` cases. For the rules the criterion lists: `gpu_output_puts`, `gpu_spawn` (and `gpu_despawn`, `gpu_add`, `gpu_remove`), `gpu_emit`, `gpu_index_component`, `gpu_term_string_field`, `gpu_term_float_field` and `gpu_term_int_field`, `gpu_term_uint32_field`, `gpu_write_resource`, `gpu_write_nested_param`, `gpu_each_two_deep`, `gpu_break`, `gpu_return`, `gpu_recursion_direct` and `gpu_recursion_mutual`, `gpu_function_each`, `gpu_int_literal_too_big`, `gpu_statement_outside_each`. |
| 5  | 12 `tests/panic/gpu_*` cases: `/` and `%` by zero, a negative exponent, `rand(n)` with `n` not positive, a store that does not fit each of the four compact types, the earliest of several failures, a failure in a called function, a failure that stops the tick, and a constant divisor that wraps to zero. Each stops with its message and status 1 with `MIDNIGHT_GPU=off` in both build modes, and in `checks.gpu`. |
| 6  | `nix run .#bench-gpu`, gravity on 16,384 bodies: 1.33 ms per tick on the GPU, 356 ms on the CPU, ratio 267. |
| 7  | Same run, one million entities moved, ms per tick: 0.13 on the CPU; 0.04 on the GPU with the data left there; 0.78 on the GPU with a CPU system reading every tick. The app adds a fourth row, the reading program on the CPU: 0.53. Every run prints the same checksum. |
| 8  | Same run. `gpu_move_read.12am`: the plan is `download  Position  move -> total`, and `stats` counts 0 uploads and 1 download in each of the 1,000 ticks after the first. `gpu_move.12am`: the plan is empty and `stats` counts none. The app exits 1 if either differs. |
| 9  | `ldd` on the release build of `examples/hello.12am` lists `libm.so.6`, `libc.so.6` and the loader. The build of `examples/nbody.12am` adds `libwgpu_native.so`, `libvulkan.so.1`, `libgcc_s.so.1` and `libdl.so.2`. |
| 10 | Every command in the README was run as written. |

`MIDNIGHT_TEST_GPU=hardware cargo test` and `nix run .#gpu-test` ran the 45
programs on the RTX 5090 in release mode. Every output is exact except
`examples/nbody.12am`, which is within its tolerance on both devices.

### Measured with generated shaders

On the RTX 5090, with nothing else running. Times are ms per tick, the
fastest of 5 runs; the slowest was within 10 % of it up to a million
entities.

Moving entities (`bench/gpu_move.12am` at other sizes, with
`Time.dt = 2^-20`). In the download column an ordinary system reads one
position in every tick; in the upload column one writes one velocity.

| Entities  | CPU implementation | GPU, data stays | GPU, download each tick | GPU, upload each tick |
| --------- | ------------------ | --------------- | ----------------------- | --------------------- |
| 1,000     | 0.00012            | 0.039           | 0.043                   | 0.045                 |
| 10,000    | 0.0011             | 0.038           | 0.047                   | 0.050                 |
| 100,000   | 0.012              | 0.039           | 0.103                   | 0.106                 |
| 200,000   | 0.024              | 0.038           | 0.135                   | 0.149                 |
| 300,000   | 0.039              | 0.037           | 0.169                   | 0.190                 |
| 400,000   | 0.053              | 0.039           | 0.207                   | 0.231                 |
| 500,000   | 0.067              | 0.038           | 0.242                   | 0.274                 |
| 1,000,000 | 0.129              | 0.042           | 0.399                   | 0.484                 |
| 4,000,000 | 1.241              | 0.063           | 1.811                   | 1.931                 |

- **A block costs about 0.04 ms** whatever its size up to a million
  entities. Planning measured 0.03 with a hand-written shader.
- **A light system wins on the GPU from about 300,000 entities** when its
  data stays there. Planning estimated 200,000; the generated CPU
  implementation is faster than the hand-written loop was (0.13 against
  0.19 ms at a million).
- **A transfer costs 0.04 to 0.06 ms per megabyte,** an upload a little more
  than a download: a download 0.045 and an upload 0.055 at 8 MB, 0.055 and
  0.058 at 32 MB.
- **With a transfer in every tick the GPU loses at every size measured.**

The reference's "What a GPU system costs" carries these figures.

### Open questions, settled

- **Many small tables.** A query that matches tables of 100 rows: 0.040 ms
  per tick for 1 table, 0.043 for 8, 0.064 for 64, about 0.0004 ms for each
  further table. A block has one uniform buffer, written once, and each
  table is bound to its own range of it. Dispatches are not batched.
- **The error word.** `atomicMin` on an `atomic<u32>` reports the earliest
  line on the 5090, `llvmpipe` and RADV. `tests/panic/gpu_earliest_failure`
  covers it.
- **The numbers from the compiler** are in the section above.

### Differences from the design

- **A `UInt32` field cannot be used in GPU code,** not only in a query
  term: reading one, assigning to one, and constructing a struct that has
  one among its own fields are errors, in a GPU block and in a function it
  calls. A shader's 32-bit `Int` is signed, so the value would have changed
  meaning on the way in. A struct that has such a field is still a value
  there: it can be read from a constant or a resource field, copied, passed,
  returned and compared, and its other fields used. The first version, 0.2.0,
  rejected every such struct whole; 0.2.1 narrowed the rule after review.
- **Constants are worked out by the compiler.** An `Int` constant made from
  literals and other such constants with `+`, `-`, `*`, `/`, `%` and `**`
  is a known number. It takes no uniform word, and `x / H` is not an
  operation that can fail when `H` is known and not zero.
- **Assigning to an `Entity` parameter** in a GPU block is an error.
  Ordinary code allows it.
- **A large table is one dispatch in two dimensions,** not several
  dispatches.
- **A snapshot of rows that are not whole words,** for a nested query that
  matches several tables, is put together in CPU memory and written in one
  piece: a copy between GPU buffers moves whole words.
- **The plan is worked out for two ticks:** one in which no structural
  change runs, and one in which every structural change runs. A single pass
  that takes every statement to run hid a transfer of every tick behind one
  that only a structural change causes.
- **Top-level code in the plan** counts only inside a loop that ticks.
- **Tolerances apply to `Float`s only,** as `T * max(1, |expected|)`. Whole
  numbers and all other text are compared exactly. A GPU-mode run that
  finds no device, or runs a block on the CPU, fails.
- **The suite's GPU mode is `MIDNIGHT_TEST_GPU=software` or `hardware`,**
  which `checks.gpu` and `apps.gpu-test` set. The stats test is
  `gpu_stats_match_plan` on `tests/run/gpu_transfers.12am`.
- **`MIDNIGHT_GPU`** reports a word it does not know, once, when the program
  starts.
- **`midnight emit-wgsl FILE`** prints the shader of every GPU block.
- **`bench-gpu`** also prints the reading program's time on the CPU, and
  compares the two plans with the counted transfers. It prints the gravity
  ratio and does not enforce the factor of 10.
- **The uniform buffer.** Words 0 to 7 are a header (the table's rows, the
  snapshot's rows, the seed, the block, and `Time.tick`, `Time.dt` and
  `Time.elapsed`). From word 8 every block of a program uses one
  layout of the resource fields and constants that any block reads.
  `runtime/midnight_rt.h` states it.
- **Build flags.** Generated C is compiled with `-DMN_GPU_SYSTEMS` when the
  program declares a GPU system, and with `-DMN_GPU` when
  `MIDNIGHT_GPU_FLAGS` is set and the GPU runtime is linked.
- **Leaks a driver leaves are passed over.** In a debug build, LeakSanitizer
  skips a leak whose allocation came through a library that is no longer
  loaded when the program ends. RADV leaves one such allocation. Every
  other leak is still reported.

### Known limits

- A debug build cannot use an NVIDIA device: the driver fails to start
  under AddressSanitizer. Such a build uses another adapter if the machine
  has one, and the CPU otherwise. `apps.gpu-test` runs release builds only.
- `%` on `Float` at 32 bits is not the exact remainder; the reference says
  how far off it can be. The sign of a zero result of `%` and `**` can
  differ from what ordinary code gives.
- A divisor that is a resource field, a field of a struct constant or a
  constant made by a call still makes its block one that can fail, so its
  error word is read back in every tick.
- On a device, `Math` functions outside their domain and comparisons with
  NaN are not tested; the reference leaves their results open.
- `stats` does not count the copy a nested `each` reads, and the plan does
  not list it.
- The plan does not follow ticks written out one after another with code
  between them, or a tick in which only some structural changes run.
- `gpu_stats_match_plan` needs a program whose GPU components sit in one
  table, since a plan line stands for one transfer per table.
- `llvmpipe` binds at most 128 MB per buffer. A block with a larger column
  runs on the CPU there and `info` says so; five million entities with a
  32-byte component did.
- Creating the device adds about 0.15 s to a run on the 5090.
- Nothing has run on macOS or on aarch64; the flake evaluates there.

### Noticed, not changed

- Floor division is written twice in the compiler, in `check/util.rs` and
  in `emit/analysis.rs`.
- `narrow` in `emit/gpu/lower.rs` still names `UInt32` as a source type,
  which the checker no longer lets through.
- A division by a constant known to be zero could be a compile error. It is
  a run-time error, as in ordinary code.
- The check of query terms names one kind of unsuitable field per compile.
- GCC 15.3 reports `-Werror=maybe-uninitialized` in `midnight_rt.c`. The
  flake uses clang.
