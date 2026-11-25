# Midnight

Midnight is a compiled, statically typed language built around the
entity-component-system pattern, with syntax borrowed from Ruby. Entities,
components, systems, queries, resources and events are language constructs,
not a library. Source files end in `.12am`.

```ruby
component Position
  x: Float
  y: Float
end

component Velocity
  dx: Float
  dy: Float
end

tag Frozen

system move
  each Position, Velocity, without: Frozen do |pos, vel|
    pos.x += vel.dx * Time.dt
    pos.y += vel.dy * Time.dt
  end
end

system report
  each Entity, Position do |e, pos|
    puts "#{e} is at (#{pos.x}, #{pos.y})"
  end
end

spawn Position(0, 0), Velocity(1, 2)
spawn Position(5, 5), Velocity(-1, 0), Frozen

Time.dt = 0.5
3.times do
  World.tick
end
```

A program's data is laid out the way a data-oriented C program would lay it
out by hand: components in contiguous arrays, no per-object headers, fields
as small as you declare them. The `move` system above compiles to a counted
loop over two arrays that the C compiler vectorizes.

A system marked `gpu` runs its queries as compute shaders over those same
arrays, and on the CPU, with the same meaning, where there is no GPU:

```ruby
component Body
  x: Float32
  y: Float32
  vx: Float32
  vy: Float32
end

gpu system drift
  each Body do |body|
    body.x += body.vx * Time.dt
    body.y += body.vy * Time.dt
  end
end
```

[`examples/nbody.12am`](examples/nbody.12am) is a fuller one: gravity, every
body against every other.

- [`docs/language.md`](docs/language.md) is the language reference.
- [`specs/`](specs/) holds one spec per change, with its acceptance criteria
  and outcome.
- [`examples/`](examples/) has seven programs, from `hello.12am` to a
  million-entity benchmark.

## Try it

Everything runs through [Nix](https://nixos.org) with flakes enabled; nothing
else needs to be installed, not even a C compiler.

```sh
nix run . -- run examples/hello.12am
nix run . -- run examples/battle.12am
nix run . -- run --debug examples/life.12am
nix run . -- check examples/bounce.12am
nix run . -- emit-c examples/move.12am
nix run . -- build examples/move.12am -o move && ./move
nix run . -- run examples/nbody.12am
nix run . -- emit-wgsl examples/nbody.12am
nix run . -- plan tests/plan/reader.12am
```

## The compiler

```
midnight build FILE [-o OUT] [--debug]   compile to an executable
midnight run FILE [--debug]              compile to a temporary directory and run
midnight check FILE                      report errors without generating code
midnight plan FILE                       print the CPU-GPU transfers of one tick
midnight emit-c FILE                     print the generated C
midnight emit-wgsl FILE                  print the shader of every GPU block
midnight --version | --help
```

`build` names the executable after the file, without its extension, unless
`-o` says otherwise. `run` exits with the program's status. The compiler
exits 0 on success, 1 when the program has errors and 2 on a usage error.

The compiler is written in Rust with no dependencies beyond the standard
library. It emits one C11 file, which is compiled together with a small
runtime (`runtime/`) by the C compiler named in `MIDNIGHT_CC`, or `cc`. The
packaged compiler defaults to the clang from the pinned nixpkgs.
`MIDNIGHT_CFLAGS` adds flags to that invocation.

| Mode    | Flag      | C flags                                                           | Extra checks          |
| ------- | --------- | ----------------------------------------------------------------- | --------------------- |
| release | (default) | `-O2 -fwrapv -ffp-contract=off`                                   | none                  |
| debug   | `--debug` | `-O0 -g -fwrapv -ffp-contract=off -fsanitize=address,undefined`   | allocation accounting |

On x86-64 both modes add `-march=x86-64-v3`, so a built program needs a
CPU with AVX2 and FMA: Intel since 2013 (not every low-end Pentium and
Celeron) and AMD since 2015. `MIDNIGHT_CFLAGS=-march=x86-64` builds for
any x86-64 CPU. `-ffp-contract=off` keeps a `Float` result the same
whichever level a program is built for. Both modes also add
`-fno-builtin-` for `pow`, `sin`, `cos`, `tan`, `atan2`, `exp` and `log`
and their `float` forms, so that the C compiler does not compute them
itself and a release build gives the results of a debug build.

The checks the language defines (list bounds, missing components, division
by zero) are on in both modes. A debug build also counts live string and list
buffers and exits with status 70 if any is still alive when the program ends.

Errors come with the source line and, where possible, a suggestion:

```
error: unknown component `Postion`
  --> move.12am:14:8
   |
14 |   each Postion, Velocity do |pos, vel|
   |        ^^^^^^^ did you mean `Position`?
```

## GPU systems

For each `each` of a `gpu system` the compiler emits a WGSL compute shader,
embedded in the executable as text, and a C function with the same meaning.
The shaders run through [`wgpu-native`](https://github.com/gfx-rs/wgpu-native),
on Vulkan on Linux and Metal on macOS, without a window. `midnight emit-wgsl`
prints them, and `midnight plan` prints the transfers between CPU and GPU
memory that one tick performs; the reference explains how to read a plan.

Only a program that declares a GPU system is linked against `wgpu-native`,
with the flags in `MIDNIGHT_GPU_FLAGS`, which the packaged compiler and the
dev shell set. Without that variable `midnight` says so in one line and
builds the program with the C functions only. A program with no GPU system
depends on libc and libm alone.

When it starts, a program with a GPU system picks the fastest hardware
adapter. `MIDNIGHT_GPU` changes that. It is a comma-separated list of:

| Word       | Meaning                                                         |
| ---------- | --------------------------------------------------------------- |
| `off`      | Use no device: every GPU system runs its C functions.           |
| `software` | Accept an adapter that computes on the CPU (Mesa's `llvmpipe`) when there is no hardware one. |
| `info`     | Print one line to standard error naming the device in use, or `no device`, and one line for each `each` of a GPU system that runs on the CPU, with the reason. |
| `stats`    | Print one line to standard error for each tick: the uploads and downloads of its systems and of the top-level code after it. |

```sh
MIDNIGHT_GPU=info,stats nix run . -- run tests/plan/reader.12am
```

A word that is not in the table is reported once, when the program starts.
In a `stats` line one upload or download is one component array of one
table. The first line is `tick 0`. A line is printed when the next tick
starts, or when the program ends, so it counts what top-level code
transfers after the tick, as `midnight plan` does; the last line also counts
the code after the loop that ticks.

With no device, because none was found or `off` is set, every GPU system runs
on the CPU. A single `each` also does when it needs more than the device
offers, such as an array larger than the largest buffer the device binds.
An error from the device after start-up prints `midnight: GPU error: …` and
ends the program with status 70.

## Benchmarks

```sh
nix run .#bench
```

builds `examples/bench.12am` (one million entities with two `Float32`
components, moved for 100 ticks) and `bench/baseline.c` (the same work written
by hand) with the same C flags, and prints the time per tick of each, their
ratio and their peak memory. It then runs `bench/churn.12am`, which measures
`spawn`, `despawn`, `add`, `remove` and `e[C]`.

```sh
nix run .#bench-gpu
```

runs gravity among 16,384 bodies on the GPU and on the CPU and prints the
time per tick of each, and a checksum of the bodies after four ticks from
each; the two must agree within a relative 0.00001. It runs gravity among
65,536 bodies on the GPU only. It then moves one million entities three ways and
prints the time per tick of each: on the CPU; on the GPU with the data left
there; and on the GPU with a CPU system reading the result in every tick.
It then prints, on the GPU and on the CPU, the time per tick of six costs:
one block, eight blocks, a block that can fail, a reader over 64 tables, one
`e[C]` and one `spawn` a tick. "On the CPU" is the same program with
`MIDNIGHT_GPU=off`. Last, it prints
the transfer plan of the two GPU cases beside the transfers
`MIDNIGHT_GPU=stats` counted, and fails if they differ. On a machine with no
hardware GPU it prints the CPU times only; `MIDNIGHT_GPU=software` in the
environment lets the GPU runs use software Vulkan. The programs are in
`bench/`.

## Working on it

```sh
nix develop
cargo test
cargo run -- run examples/hello.12am
nix flake check
```

`nix develop` provides the Rust toolchain, clang and `nixfmt`, and sets
`MIDNIGHT_GPU_FLAGS`. `cargo test` runs the unit tests and every program
under `tests/` and `examples/`, each in both build modes, with GPU systems
on the CPU. `nix flake check` builds the package (which runs the same tests
in the sandbox) and checks clippy, rustfmt and nixfmt.

The programs with GPU systems are also run on a device, where a number may
differ from the expected output by the tolerance the program states in its
`# tolerance:` comment:

```sh
MIDNIGHT_TEST_GPU=hardware cargo test    # in the dev shell, on the machine's GPU
MIDNIGHT_TEST_GPU=software cargo test    # in the dev shell, on software Vulkan
nix run .#gpu-test                       # the packaged compiler, on the machine's GPU
```

Every program runs as a release and as a debug build. A run that finds no
device, or runs a GPU system on the CPU, fails. The
software mode points programs at the Vulkan driver file named in
`MIDNIGHT_TEST_SOFTWARE_ICD`, which the dev shell sets to Mesa's `llvmpipe`.
The suite tests the compiler cargo built, or the one `MIDNIGHT_BIN` names.

On Linux, `nix flake check` includes `checks.gpu`, which runs them on Mesa's
software Vulkan inside the build sandbox.

```
src/            the compiler, one module per stage:
                source, lexer, parser, check, plan, emit, driver
runtime/        midnight_rt.h, midnight_rt.c; midnight_gpu.h, midnight_gpu.c
                for programs with a GPU system
docs/           language.md
examples/       programs with their expected output
tests/run/      programs that must print exactly NAME.out
tests/fail/     programs that must be rejected with exactly NAME.err
tests/panic/    programs that must stop with the run-time error in NAME.err
tests/plan/     programs whose transfer plan must be exactly NAME.plan
bench/          baseline.c, churn.12am, bench.sh; gpu_*.12am, bench-gpu.sh
specs/          one spec per change
```

`tests/COVERAGE.md` maps each section of the reference to the tests that
exercise it. After a deliberate change to a diagnostic,
`UPDATE_EXPECT=1 cargo test` rewrites the expected files; review the diff
before committing it.

## Status

Midnight 0.2 is the language core and GPU systems. It is tested on
`x86_64-linux`, GPU systems on an NVIDIA device and on Mesa's software
Vulkan; the flake also offers `aarch64-linux`, `x86_64-darwin` and
`aarch64-darwin`, which evaluate but have not been run. What 0.2 leaves out
is listed at the end of the reference.

## License

MIT. See [`LICENSE`](LICENSE).
