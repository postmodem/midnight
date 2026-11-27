# Midnight

Midnight is a compiled, statically typed language for games and
simulations, written mostly by AI agents and read by agents and people.
Entities, components, systems, queries, resources and events are language
constructs, not a library. A system runs on one core, on every core or on
the GPU. Its output does not depend on the number of threads, and with
`strict`, not on the GPU. The compiler tells you what moves between CPU
and GPU memory and where each loop runs, rather than leaving you to guess.
Source files end in `.12am`. [`docs/principles.md`](docs/principles.md)
says what Midnight holds to.

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
A GPU system agrees closely with its CPU run and with other GPUs, but not
to the last bit. Declared `strict gpu system`, it gives the same bits on
every device and on the CPU, at a cost in speed.

A loop whose runs cannot affect one another, like `move` above, runs on
every core of the machine, and so does a GPU system when it runs on the
CPU. The compiler finds those loops, and the program prints exactly what it
prints on one thread. `midnight loops` says where each loop runs, and what
keeps one on a single core.

- [`docs/principles.md`](docs/principles.md) says what Midnight is for and
  how it is built: mostly by AI agents, read by agents and people.
- [`docs/language.md`](docs/language.md) is the language reference.
- [`specs/`](specs/) holds one spec per change, with its acceptance criteria
  and outcome.
- [`examples/`](examples/) has eleven programs, from `hello.12am` to a
  million-entity benchmark, a window that follows the mouse and a scene
  drawn with shapes.

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
nix run . -- loops examples/bounce.12am
nix run . -- run examples/window.12am
nix run . -- run examples/input.12am
nix run . -- run examples/shapes.12am
nix run . -- run examples/flock.12am
```

## The compiler

```
midnight build FILE [-o OUT] [--debug]   compile to an executable
midnight run FILE [--debug]              compile to a temporary directory and run
midnight check FILE                      report errors without generating code
midnight plan FILE                       print the CPU-GPU transfers of one tick
midnight loops FILE                      print where each loop runs
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

Only a program that declares a GPU system or has a window is linked
against `wgpu-native`, with the flags in `MIDNIGHT_GPU_FLAGS`, which the
packaged compiler and the dev shell set. Without that variable `midnight`
says so in one line and builds the program with the C functions only. A
program with no GPU system and no window depends on libc and libm alone,
and on POSIX threads when it has a loop that can run on several cores (see
[Several cores](#several-cores)).

When it starts, a program with a GPU system picks the fastest hardware
adapter. `MIDNIGHT_GPU` changes that. It is a comma-separated list of:

| Word       | Meaning                                                         |
| ---------- | --------------------------------------------------------------- |
| `off`      | Use no device: every GPU system runs its C functions.           |
| `software` | Accept an adapter that computes on the CPU (Mesa's `llvmpipe`) when there is no hardware one. |
| `info`     | Print one line to standard error naming the device in use, or `no device`; in a program with a strict GPU system, one line saying whether the device passed the strict check; and one line for each `each` of a GPU system that runs on the CPU, with the reason. |
| `stats`    | Print one line to standard error for each tick: what its systems and the top-level code after it move between CPU and GPU memory, and how often they hand work to the device and wait for it. |
| `rows`     | Move every row on its own, whatever it costs, where a program would otherwise move a whole array because the rows cost more. For tests. |
| `strict-cpu` | Run the blocks of strict GPU systems on the CPU, as when the device fails the strict check, and the others on the device. For tests. |

```sh
MIDNIGHT_GPU=info,stats nix run . -- run tests/plan/reader.12am
```

A word that is not in the table is reported once, when the program starts.
In a `stats` line one upload or download is one component array of one
table. Rows up and rows down are rows of component arrays moved on their
own. Handles, uniform words, error words and the copies a nested `each`
reads are not counted in these four. Submissions and waits count every
submission of work to the device and every wait for it. The first line is
`tick 0`. A line is printed when the next tick starts, or when the program
ends, so it counts what top-level code transfers after the tick, as
`midnight plan` does; the last line also counts the code after the loop
that ticks.

With no device, because none was found or `off` is set, every GPU system runs
on the CPU, its rows divided among the cores. A single `each` also does when
it needs more than the device offers, such as an array larger than the
largest buffer the device binds.
A program with a strict GPU system checks the device when it opens it: it
runs a fixed piece of strict code on the device and on the CPU and compares
the results bit for bit. When they differ, `info` prints `strict blocks run
on the CPU:` with the device and the first difference, and every strict
block runs on the CPU; the other GPU systems still use the device. When
they agree it prints `strict check passed`.

An error from the device after start-up prints `midnight: GPU error: …` and
ends the program with status 70. It is reported when the work is handed to
the device, which can be later in the tick than the block that caused it.

## Several cores

Two kinds of loop run on several threads: an `each` of a GPU system when it
runs on the CPU, and an ordinary `each` that the compiler finds
independent, as the reference defines it. Each time such a loop runs, the
runtime divides its rows into pieces and hands them to threads when the
loop has enough work, and runs it where it stands when it has little. The
program prints what it prints on one thread either way.

```sh
nix run . -- loops examples/bounce.12am
```

prints one line for each loop that is not inside another: where it is, what
it stands in, its terms, and `gpu`, `cores` or `one core:` with the first
thing in the loop that keeps it on one core.

Only a program with a GPU system or an independent `each` is built for
threads: `midnight` then adds `-DMN_THREADS -pthread` and compiles
`runtime/midnight_pool.c` with it. Any other program is built as before,
has no threads and reads none of what follows.

Such a program uses one thread for each processor core it may run on, and
creates the threads the first time a loop is split. `MIDNIGHT_THREADS`
changes that. It is a comma-separated list of:

| Word       | Meaning                                                         |
| ---------- | --------------------------------------------------------------- |
| `1` to `256` | The number of threads, the one that runs the rest of the program among them. With `1` no thread is created and no loop is split. |
| `small`    | Make every piece one row, split every loop of two rows or more that can be split, and let a thread take part for each piece, up to the number of threads. For tests. |
| `reverse`  | Have each thread run its share of the pieces from the last to the first. For tests. |
| `jitter`   | Have each thread yield at random, from a fixed seed, before each piece. For tests. |
| `awake`    | Never let a thread block: one with nothing to do keeps polling, and uses a core to do it. For a program whose split loops come more than a few milliseconds apart, where a thread that blocked returns on a slowed core. |
| `info`     | Print `midnight: threads: N`, the number of threads, to standard error when the program starts, and one line for each loop the first time it is split: `midnight: threads: FILE:LINE: R rows in P pieces on T threads`. |

```sh
MIDNIGHT_GPU=off MIDNIGHT_THREADS=4,info nix run . -- run tests/run/cores_gpu_tables.12am
```

A word that is not in the table is reported once, when the program starts.
No word changes what a program prints to standard output. A thread with
nothing to do polls for 50 microseconds and then blocks.

## A window

A program that names `Window` opens a window with `Window.open`, keeps
real time while it is open, and shows a frame at the end of every tick:
`Window.background` with what the tick drew over it. The reference has
the rules.

```sh
nix run . -- run examples/window.12am
```

shows a window whose colour drifts until you close it. The window comes
from [SDL 3](https://libsdl.org), on Wayland when there is one and on X11
otherwise, or on macOS; its frames are drawn on the same `wgpu-native`
device as GPU systems. Only a program with a window is built with
`runtime/midnight_window.c` and `runtime/midnight_gpu.c` and linked against
SDL 3 and `wgpu-native`, with the flags in `MIDNIGHT_GPU_FLAGS` and
`MIDNIGHT_WINDOW_FLAGS`, which the packaged compiler and the dev shell set.
Without either variable `midnight` says so in one line and builds a
program that runs only headless and captures nothing. `midnight` removes
`LD_LIBRARY_PATH` from the environment of the C compiler and of the
program `midnight run` starts: a program finds its libraries through its
runpaths.

The same program runs with no display, and commands step it and take
pictures of it:

```sh
MIDNIGHT_WINDOW=headless MIDNIGHT_DRIVE=examples/window.drive nix run . -- run examples/window.12am
```

`MIDNIGHT_WINDOW` is a comma-separated list of:

| Word       | Meaning                                                         |
| ---------- | --------------------------------------------------------------- |
| `headless` | Show no window: the window exists in the program only, at exactly the size asked for and a scale of 1, and ticks run as fast as they can. |
| `info`     | Print one line to standard error when a window opens: its sizes in points and pixels, and `headless` or where it is shown and how frames reach it, such as `wayland, mailbox`; `input from commands` when it is driven; and, for a shown window that is not driven, a line each time it gains the focus and each time the pointer enters it. |

`MIDNIGHT_DRIVE` names a file or a named pipe of commands, one to a line:
`tick`, `tick N`, `resize W H`, `scale S`, `close`, `capture FILE`, which
writes the last frame as a PNG, and `draws`, which prints it as text.
Each command is answered on standard error with a line that begins
`midnight: drive: `; the reference has the rest.
The run above writes `drift.png` and `drift-2x.png` to the working
directory.

### Drawing

A system draws with four statements, `Draw.rect`, `Draw.circle`,
`Draw.line` and `Draw.triangle`, each of which adds one shape to the
frame the tick ends with:

```ruby
system show
  Draw.rect(0, 190, 320, 10, Color(40, 40, 60))
  each Ball do |ball|
    Draw.circle(ball.x, ball.y, 4, Color(255, 200, 0))
  end
end
```

Positions and sizes are in points from the window's top left, `y` down,
and the colour comes last. What is drawn later covers what was drawn
earlier, in the order the program runs, and nothing of a frame is kept
for the next: a program draws every tick what it wants on screen. The
same statement works in a GPU system, where the shape is recorded on the
device and nothing travels. A frame is a list before it is a picture,
and the command `draws` prints that list, which needs no device, so
drawing can be checked exactly on any machine:

```sh
MIDNIGHT_WINDOW=headless MIDNIGHT_DRIVE=examples/shapes.drive nix run . -- run examples/shapes.12am
```

prints each frame of [`examples/shapes.12am`](examples/shapes.12am) as
the statements that drew it, and writes `shapes.png` and
`shapes-cloud.png`. The scene follows the window: resize it and it is drawn again to
fit. The reference says which pixels each shape covers
and how a translucent colour is laid over what is under it.

```sh
MIDNIGHT_GPU=stats MIDNIGHT_WINDOW=headless MIDNIGHT_DRIVE=examples/flock.drive nix run . -- run examples/flock.12am
```

runs [`examples/flock.12am`](examples/flock.12am), where a GPU system
moves 100,000 points and draws them, and prints what crosses between CPU
and GPU memory in each tick: after the first, which sends the points,
nothing, until `draws` brings the shapes back to print them.

### Keyboard and mouse

A program with a window reads the keyboard and the mouse as they were
when the tick began, through the built-in resources `Keyboard` and
`Mouse`, and receives each press and release as an event:

```ruby
system steer
  Ship.x -= 4 if Keyboard.down?(Key::A)
  World.stop if Keyboard.pressed?(Key::Escape)
  on MousePressed do |click|
    puts "#{click.button} at #{click.x}, #{click.y}"
  end
end
```

Keys are named by their place on the keyboard, as the W3C's
`KeyboardEvent.code` names them: `Key::W` is the key above `Key::S` on
every layout. Positions are in points, with fractions. Losing the focus
releases everything held.

```sh
nix run . -- run examples/input.12am
```

shows a window whose colour follows the pointer; a click prints where it
was, and Escape ends it. A driven program takes its keyboard, mouse and
focus from its commands alone, headless or not, so it prints the same on
every machine:

```sh
MIDNIGHT_WINDOW=headless MIDNIGHT_DRIVE=examples/input.drive nix run . -- run examples/input.12am
```

The commands are `key down K`, `key up K`, `key K`, `mouse move X Y`,
`mouse down B`, `mouse up B`, `mouse B`, `mouse leave`, `scroll DX DY`,
`blur` and `focus`. A command that a desktop could not give, such as a
key while the window does not have the focus, stops the program with the
file and line. The run above writes `input-middle.png` and
`input-green.png`.

A program with a window reads `MIDNIGHT_GPU` too: `off` means it shows no
window and captures nothing, `software` lets it draw with an adapter that
computes on the CPU, and `info` names the device when it is opened. A
window that is shown uses the device of the program's GPU systems, or asks
for one that can draw to it; a headless window opens the device at its
first capture. A failure of the window after it has opened prints
`midnight: window error: …` and ends the program with status 70.

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
each; the two must agree within a relative 0.00001. It does the same for
gravity as a strict GPU system, whose two checksums must be equal. It
runs gravity among 65,536 bodies on the GPU only. It then moves one million
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
Last, it prints the transfer plan of the two GPU cases
beside the transfers `MIDNIGHT_GPU=stats` counted, and checks what
`MIDNIGHT_GPU=stats` counts in a tick for the move programs and the ten
costs; it fails if any of that differs. On a machine with no
hardware GPU it prints the CPU times only; `MIDNIGHT_GPU=software` in the
environment lets the GPU runs use software Vulkan. The programs are in
`bench/`.

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

times `bench/draw_frame.12am`, which draws 0 to 1,000,000 shapes a tick
in a window of 1920 by 1080, from a GPU system and from an ordinary one,
with the window shown in the compositor of the window tests and headless,
and prints the time per tick of each and their difference: what a frame
of those shapes adds to a tick. `VK_DRIVER_FILES` chooses the device.

## Working on it

```sh
nix develop
cargo test
cargo run -- run examples/hello.12am
nix flake check
```

`nix develop` provides the Rust toolchain, clang and `nixfmt`, and sets
`MIDNIGHT_GPU_FLAGS` and `MIDNIGHT_WINDOW_FLAGS`. `cargo test` runs the unit tests and every program
under `tests/` and `examples/`, each in both build modes, with GPU systems
on the CPU. `nix flake check` builds the package (which runs the same tests
in the sandbox) and checks clippy, rustfmt and nixfmt, and the evals'
harness.

Every program with a loop that can be split also runs, in both build modes,
with `MIDNIGHT_THREADS` set to `1`, `4`, `3,small`, `7,small,reverse`,
`64,small,jitter` and `2,small,awake`, and must give its expected output
each time. A program of `tests/run` with a `NAME.split` file must split
exactly the loops the file lists when it runs with
`MIDNIGHT_THREADS=4,small,info`. A program that must stop with an error
fails when it is still running after ten seconds. `tests/pool/stress.c`
exercises the thread pool on its own.

```sh
MIDNIGHT_TEST_TSAN=1 cargo test          # in the dev shell
```

also builds the pool's stress test and every program with a loop that can
be split with ThreadSanitizer, runs the programs with `4,small` and with
`7,small,reverse`, and fails on any report. On Linux, `nix flake check`
includes `checks.tsan`, which does this inside the build sandbox.

The programs with GPU systems are also run on a device, where a number may
differ from the expected output by the tolerance the program states in its
`# tolerance:` comment:

```sh
MIDNIGHT_TEST_GPU=hardware cargo test    # in the dev shell, on the machine's GPU
MIDNIGHT_TEST_GPU=software cargo test    # in the dev shell, on software Vulkan
nix run .#gpu-test                       # the packaged compiler, on the machine's GPU
```

Every program runs as a release and as a debug build, as it is, with
`MIDNIGHT_GPU=rows` and with `MIDNIGHT_THREADS=4,small`. A run that finds
no device, or runs a GPU system on the
CPU, fails. A program whose GPU systems are all strict must print exactly
its expected output and may not state a tolerance. A program with a strict
GPU system must report that the device passed the strict check, and runs
again with `MIDNIGHT_GPU=strict-cpu`, with and without the threads. `gpu_strict_div_sqrt` runs the
strict `/` and `Math.sqrt` of shaders on the device against the CPU's: on
hardware `sqrt` of every 32-bit pattern and `/` of 2^28 pairs, on software
Vulkan 2^24 of each. In hardware mode a `VK_DRIVER_FILES` set by the caller
chooses the device. A program of `tests/run` with a `NAME.stats` file must also
print exactly those `stats` lines, with `MIDNIGHT_THREADS=4,small` too, and
with a `NAME.rows.stats` file, those lines with `rows` as well. The software mode points programs at the Vulkan driver file named in
`MIDNIGHT_TEST_SOFTWARE_ICD`, which the dev shell sets to Mesa's `llvmpipe`.
The suite tests the compiler cargo built, or the one `MIDNIGHT_BIN` names.

On Linux, `nix flake check` includes `checks.gpu`, which runs them on Mesa's
software Vulkan inside the build sandbox.

Every program with a window runs headless in the tests above, with the
commands of its `NAME.drive` file, `capture` left out, or with none. In GPU
mode a program of `tests/run` with a `NAME.frames` file also runs with its
captures, in a directory of its own: the answers to its commands must be
exactly that file, and each file it writes must be a PNG of the size and
pixels its answer gives.

The tests of a window that is shown never use the desktop they are run
from. `tests/window/compositor.sh` starts sway with no output on any screen
and no input devices, and a session bus, in a runtime directory of their
own, and runs a command with that compositor as the only display it can
reach. Inside it,
`tests/window/test.sh` runs a hand-written SDL program, then the suite's
window tests: the frame programs with a window and their commands from a
pipe, against a picture of the compositor; 120 ticks timed at 60 Hz, at
144 Hz and on a workspace that is not shown; a close and a resize by the
compositor; and keys, buttons, moves, the wheel and focus from the
compositor, which must reach a program as its commands do headless.

The input comes from `tests/window/input.c`, a Wayland client that gives
the compositor a virtual keyboard and pointer. It connects only to the
socket whose absolute path it is given, and sends nothing unless every
output of the compositor is a headless one, so it cannot type into a
desktop; a test starts a second sway with a Wayland output inside the
compositor and checks that the client refuses it. The flake builds it,
and the dev shell names it in `MIDNIGHT_TEST_INPUT_CLIENT`.

```sh
bash tests/window/test.sh OUT            # in the dev shell, on the machine's GPU
nix run .#window-test                    # the packaged compiler, on the machine's GPU
```

On Linux, `nix flake check` includes `checks.window`, which runs them with
sway drawing in software and the programs on Mesa's software Vulkan inside
the build sandbox. The dev shell provides sway and `grim`. Every other
test that runs a program with a window sets the display variables to
addresses that cannot be connected to.

The evals measure how well agents write Midnight from its documentation
alone. Claude Code is given a set of small programs to write or repair,
each in a directory of its own with the documentation and the compiler
and nothing else, and a report says how often each model succeeded,
which diagnostics it met and how it got past them, and which parts of
the reference it read. They run only when a spec asks them a question,
and its Outcome says what they showed. They need the network and a Claude account, so `nix flake check`
tests only the harness and the tasks' own programs. `evals/README.md`
says how to run them, read a report and add a task.

```sh
nix run .#eval -- run                    # every task, sonnet and haiku, five trials each
```

```
src/            the compiler, one module per stage:
                source, lexer, parser, check, plan, loops, emit, driver
runtime/        midnight_rt.h, midnight_rt.c; midnight_gpu.h, midnight_gpu.c
                for programs with a GPU system or a window;
                midnight_window.h, midnight_window.c for programs with a
                window; midnight_pool.h, midnight_pool.c for programs with
                a loop that can be split
docs/           language.md; principles.md
examples/       programs with their expected output
evals/          the evals: eval.rb, the harness; tasks/, each a program
                to write or repair; results/, committed runs and reports
tests/run/      programs that must print exactly NAME.out, on a device the
                `stats` lines in NAME.stats when there is one, and with
                MIDNIGHT_THREADS=4,small,info the lines in NAME.split; a
                program with a window gets the commands in NAME.drive,
                must give the `draws` answers in NAME.draws, and on a
                device must answer the others with NAME.frames
tests/fail/     programs that must be rejected with exactly NAME.err
tests/panic/    programs that must stop with the run-time error in NAME.err
tests/plan/     programs whose transfer plan must be exactly NAME.plan
tests/loops/    programs for which `midnight loops` must print exactly
                NAME.loops
tests/pool/     stress.c, which exercises the thread pool on its own
tests/window/   compositor.sh, test.sh; smoke.c, a window from SDL 3 and
                wgpu-native on its own; input.c, a keyboard and a pointer
                for a headless compositor
bench/          baseline.c, churn.12am, bench.sh; gpu_*.12am, bench-gpu.sh;
                cores_*.12am, cores.sh
specs/          one spec per change
```

`tests/COVERAGE.md` maps each section of the reference to the tests that
exercise it. After a deliberate change to a diagnostic,
`UPDATE_EXPECT=1 cargo test` rewrites the expected files; review the diff
before committing it. With `MIDNIGHT_TEST_GPU` set it rewrites the `.stats`
and `.frames` files, the only expected files that come from a device.

## Status

Midnight 0.8 is the language core, GPU systems, the rules that move only
what is needed between CPU and GPU memory, strict GPU systems, loops on
several cores, a window, the keyboard and the mouse, and drawing. It is
tested on `x86_64-linux`, GPU systems on an NVIDIA device, on an AMD iGPU
through Mesa's RADV, and on Mesa's software Vulkan, and the window on
Wayland; the flake also offers
`aarch64-linux`, `x86_64-darwin` and `aarch64-darwin`, which evaluate but
have not been run. What 0.8 leaves out
is listed at the end of the reference.

## License

MIT. See [`LICENSE`](LICENSE).
