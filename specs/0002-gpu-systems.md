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

The programmer writes ordinary Gremlin. The compiler checks that the system
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
- Snapshot semantics and 32-bit arithmetic inside GPU systems.
- A WGSL emitter beside the C emitter.
- Runtime support: a GPU device, a GPU copy of each column a GPU system
  uses, transfers, and dispatch.
- A CPU implementation of every GPU system, used when there is no device.
- `gremlin plan FILE`: prints the transfers one tick performs.
- Flake: `wgpu-native` from the pinned nixpkgs, linked only into programs
  that declare a GPU system.
- `examples/nbody.grem`, two benchmarks, and the reference updated.

## Non-goals

- Windows, drawing, input and audio. The device, the buffers and the shader
  emitter built here are what a graphics spec will draw with.
- `spawn`, `despawn`, `add`, `remove`, `emit`, `on`, output, strings and
  lists inside a GPU system.
- `e[C]` inside a GPU system.
- Collecting a result across entities into a resource (a sum, a minimum).
- Running ordinary systems on several CPU threads.
- Spatial structures. A nested query still visits every pair.
- Bit-identical results between the GPU and the CPU implementation.
- Moving rows between tables on the GPU. A structural change to a table
  brings that table's columns back to CPU memory first.
- More than one GPU, or choosing among several.

## Design

### The language

**`gpu system`.** The body is one or more `each` blocks and nothing else.
Inside a block, these are allowed: local variables, arithmetic and
comparison, `if`, `unless`, `case`, the conditional operator, `while`,
`until`, counting loops, `break` and `next`, `Math` functions, reading
constants, reading resource fields, calling functions and methods that
themselves follow these rules, `rand`, and one level of nested `each`.

Everything else is a compile error that names the rule and the reason.

**What a GPU system may touch.**

- The components in its queries must be plain (no string or list, at any
  depth) and their numeric fields must be at most 32 bits wide: `Float32`,
  `Int32`, `UInt32` and the smaller compact types, plus `Bool`, enums,
  `Entity` and structs of these. A `Float` or `Int` field is an error with
  the hint to declare it `Float32` or `Int32`.
- It writes only to the parameters of its outer `each`. Parameters of a
  nested `each`, resources and constants are read-only.
- Functions it calls must not recurse.

**Snapshot semantics.** A GPU system's block runs for every matching entity
as if all at once. Each run starts from the entity's components as they were
before the system began, sees its own writes, and sees every *other* entity
as it was before the system began. The writes of all runs land together when
the `each` finishes. This is what makes the result independent of the order
entities are processed in, and so the same on any number of cores.

Resource fields are read once, before the `each` starts.

**32-bit arithmetic.** Inside a GPU system, and inside any function while it
is called from one, `Float` is 32 bits wide and `Int` is 32 bits wide and
wraps. The source does not change: the same `def` can be called from ordinary
code, where it computes at 64 bits, and from a GPU system. Resource fields
and constants are converted on the way in.

**`rand` inside a GPU system** is a function of the seed, the tick, the
entity and the position of the call in the block. It needs no shared state,
and gives the same numbers on the GPU and on the CPU.

**Determinism.** A GPU system gives the same result from run to run on the
same machine and driver. Between the GPU and the CPU implementation, or
between two different GPUs, results agree closely but not to the last bit:
GPUs are allowed small differences in division, square roots and
trigonometry.

### The compiler

**One system, two outputs.** For each `each` in a GPU system the compiler
emits a WGSL compute shader, embedded in the executable as text, and a C
function with the same meaning. Functions called from GPU systems are
compiled once more at 32 bits, for both outputs.

**Buffers are byte-for-byte the columns.** A GPU buffer holds a column
exactly as CPU memory does, so a transfer is a plain copy. The shader sees
the buffer as an array of 32-bit words; the emitter generates the shifts and
casts that read and write each field, including one-byte and two-byte fields.

**In place when it can be.** A block with no nested `each` reads only its own
entity, so snapshot semantics and updating in place give the same result and
the shader writes straight into the column. A block with a nested `each`
writes the columns it changes to a second buffer, and the two are swapped
when the `each` finishes.

**Transfers are planned from the source.** Systems run in a fixed order and
the compiler knows which components every query and every `e[C]` reads and
writes (0001 records this). From that it derives, for one tick in steady
state, each point where a column must be copied to the GPU or back.
`gremlin plan FILE` prints them, one line per transfer, naming the component
and the two systems it travels between. A program whose plan is empty keeps
its GPU data on the GPU.

### The runtime

**Where a column lives.** Every column a GPU system uses has a CPU copy, a
GPU copy, and a note of which is current. A GPU system that finds the CPU
copy current uploads it. CPU code (a query, `e[C]`, a structural change, the
top level) that finds the GPU copy current downloads it first. Columns that
no GPU system uses have no GPU copy and pay nothing; the compiler knows which
those are.

**Dispatch.** One compute dispatch per matching table per `each`. The row
count, the tick, the seed and the resource fields the block reads go in a
small uniform buffer.

**The GPU library is `wgpu-native`,** used through its C header, from the
pinned nixpkgs. It runs on Vulkan on Linux and Metal on macOS, covers the
four systems the flake offers, and works without a window. Its shading
language is text, which suits a compiler that already emits C, and the same
device and language serve drawing later. Programs with no GPU system do not
link it and stay dependent on libc alone.

On Linux the executable gets a runpath to the Vulkan loader from nixpkgs; the
GPU driver itself comes from the host. `LD_LIBRARY_PATH` is never set.

**No device.** If no GPU is found, or `GREMLIN_GPU=off` is set, every GPU
system runs its C implementation. `GREMLIN_GPU=info` prints one line to
standard error naming the device in use, or saying that there is none.

### Tests

- Every kernel rule has a `tests/fail` case.
- `tests/run` programs with GPU systems run with `GREMLIN_GPU=off` inside
  `nix flake check`, in both build modes.
- The same programs run on a real device through `apps.gpu-test`, which
  compares GPU output with CPU output within a tolerance each test states.
- In debug mode the runtime counts uploads and downloads per tick, and a test
  checks the counts against `gremlin plan`.

### Open questions for planning

- **Does it run here?** `wgpu-native` compute without a window on NixOS: the
  runpath to the Vulkan loader and the driver lookup must be shown to work,
  not assumed.
- **A software GPU in the sandbox.** If Mesa's software Vulkan driver works
  inside the Nix build sandbox, the GPU path can be a `check` instead of an
  app. Planning must try it.
- **Where the crossover is.** For a light system, measure the entity count
  above which the GPU wins when the data stays on the GPU, and the cost per
  megabyte of a transfer. The numbers go in the reference's cost section.
- **Matching the CPU more closely.** Whether compiling the C implementation
  without fused multiply-add narrows the difference from the GPU.
- **Many small tables.** One dispatch per table is wasteful when a query
  matches dozens of small ones. Planning must decide whether to batch them.
- **The header's stability.** `webgpu.h` has changed between releases;
  planning must pin what the runtime relies on.

## Acceptance criteria

All run on x86_64-linux. Criteria 2, 5, 6 and 7 need a GPU.

1. `nix flake check` passes, including every GPU-system test on the CPU
   implementation.
2. `GREMLIN_GPU=info gremlin run examples/nbody.grem` names a GPU device, and
   its output agrees with the `GREMLIN_GPU=off` run to within one part in a
   thousand on every printed position after 100 ticks.
3. `GREMLIN_GPU=off gremlin run examples/nbody.grem` prints the same output
   on two runs, and in debug mode.
4. Each rule under "The language" has a `tests/fail` case: output, a
   structural change, `emit`, a string field, a 64-bit field, a write to a
   resource, a write to a nested parameter, `e[C]`, recursion, and a
   statement outside `each`.
5. `nix run .#bench-gpu` runs the gravity system on 16,384 bodies on the GPU
   and on the CPU and prints both times per tick. On a machine with a
   discrete GPU, the GPU is at least 10 times faster.
6. The same app runs a light system (move one million entities) three ways
   and prints time per tick for each, with no threshold: on the CPU; on the
   GPU with the data left there; and on the GPU with a CPU system reading
   the result every tick.
7. For the third case in criterion 6, `gremlin plan` lists exactly one
   download per tick, and the debug-mode transfer counts match it. For the
   second case the plan is empty and the counts are zero.
8. `ldd` on the release build of `examples/hello.grem` lists libc and libm
   and no GPU library.
9. Every command in the README works as written.
