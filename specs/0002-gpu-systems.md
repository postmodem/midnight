# 0002 — GPU systems: run data-parallel systems on the GPU

- **Status:** draft
- **Depends on:** 0001
- **Tested on:** —

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
  reference becomes Midnight 0.2.

## Non-goals

- Windows, drawing, input and audio. The device, the buffers and the shader
  emitter built here are what a graphics spec will draw with.
- `spawn`, `despawn`, `add`, `remove`, `emit`, `on`, output, strings and
  lists inside a GPU system.
- `e[C]`, `e.has?`, `e.alive?` and `World.count` inside a GPU system.
- Collecting a result across entities into a resource (a sum, a minimum).
- Running ordinary systems, or the CPU implementation of a GPU system, on
  several CPU threads.
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
