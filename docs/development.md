# Working on Midnight

```sh
nix develop
cargo test
cargo run -- run examples/hello.12am
nix flake check
```

`nix develop` provides the Rust toolchain, clang, `nixfmt`, sway and `grim`,
and sets `MIDNIGHT_GPU_FLAGS` and `MIDNIGHT_WINDOW_FLAGS`. `cargo test` runs
the unit tests and every program under `tests/` and `examples/`, each in
both build modes, with GPU systems on the CPU. `nix flake check` builds the
package (which runs the same tests in the sandbox) and checks clippy,
rustfmt and nixfmt, and the evals' harness. The suite tests the compiler
cargo built, or the one `MIDNIGHT_BIN` names.

Specs come first: [`principles.md`](principles.md) says what Midnight holds
to, [`language.md`](language.md) defines the language, and each change
starts from an approved spec in [`specs/`](../specs/).

## Layout

```
src/            the compiler, one module per stage:
                source, lexer, parser, check, plan, loops, emit, driver
runtime/        midnight_rt.h, midnight_rt.c; midnight_gpu.h, midnight_gpu.c
                for programs with a GPU system or a window;
                midnight_window.h, midnight_window.c for programs with a
                window; midnight_pool.h, midnight_pool.c for programs with
                a loop that can be split
docs/           the reference, the principles, and these guides
examples/       programs with their expected output
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
tests/device/   strict_div_sqrt.c, strict `/` and `sqrt` on the device
tests/render/   the reference renderer that captures are held to
tests/window/   compositor.sh, test.sh; smoke.c, a window from SDL 3 and
                wgpu-native on its own; input.c, a keyboard and a pointer
                for a headless compositor
bench/          the benchmarks and their programs; see bench/README.md
evals/          the evals: eval.rb, the harness; results/, committed runs
                and reports; see evals/README.md
specs/          one spec per change
```

`tests/COVERAGE.md` maps each section of the reference to the tests that
exercise it. After a deliberate change to a diagnostic,
`UPDATE_EXPECT=1 cargo test` rewrites the expected files; review the diff
before committing it. With `MIDNIGHT_TEST_GPU` set it rewrites the `.stats`
and `.frames` files, the only expected files that come from a device.

## Threads

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

## GPU

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
no device, or runs a GPU system on the CPU, fails. A program whose GPU
systems are all strict must print exactly its expected output and may not
state a tolerance. A program with a strict GPU system must report that the
device passed the strict check, and runs again with
`MIDNIGHT_GPU=strict-cpu`, with and without the threads.
`gpu_strict_div_sqrt` runs the strict `/` and `Math.sqrt` of shaders on the
device against the CPU's: on hardware `sqrt` of every 32-bit pattern and
`/` of 2^28 pairs, on software Vulkan 2^24 of each. A program of
`tests/run` with a `NAME.stats` file must also print exactly those `stats`
lines, with `MIDNIGHT_THREADS=4,small` too, and with a `NAME.rows.stats`
file, those lines with `rows` as well.

In hardware mode a `VK_DRIVER_FILES` set by the caller chooses the device.
The software mode points programs at the Vulkan driver file named in
`MIDNIGHT_TEST_SOFTWARE_ICD`, which the dev shell sets to Mesa's
`llvmpipe`. On Linux, `nix flake check` includes `checks.gpu`, which runs
them on Mesa's software Vulkan inside the build sandbox.

## Windows

Every program with a window runs headless in the tests above, with the
commands of its `NAME.drive` file, `capture` left out, or with none, and
with the display variables set to addresses that cannot be connected to.
In GPU mode a program of `tests/run` with a `NAME.frames` file also runs
with its captures, in a directory of its own: the answers to its commands
must be exactly that file, and each file it writes must be a PNG of the
size and pixels its answer gives. A capture that follows a `draws` must
agree with what the reference renderer in `tests/render/` makes of that
answer.

The tests of a window that is shown never use the desktop they are run
from. `tests/window/compositor.sh` starts sway with no output on any screen
and no input devices, and a session bus, in a runtime directory of their
own, and runs a command with that compositor as the only display it can
reach. Inside it, `tests/window/test.sh` runs a hand-written SDL program,
then the suite's window tests: the frame programs with a window and their
commands from a pipe, against a picture of the compositor; 120 ticks timed
at 60 Hz, at 144 Hz and on a workspace that is not shown; a close and a
resize by the compositor; and keys, buttons, moves, the wheel and focus
from the compositor, which must reach a program as its commands do
headless.

```sh
bash tests/window/test.sh OUT            # in the dev shell, on the machine's GPU
nix run .#window-test                    # the packaged compiler, on the machine's GPU
```

On Linux, `nix flake check` includes `checks.window`, which runs them with
sway drawing in software and the programs on Mesa's software Vulkan inside
the build sandbox.

The input comes from `tests/window/input.c`, a Wayland client that gives
the compositor a virtual keyboard and pointer. It connects only to the
socket whose absolute path it is given, and sends nothing unless every
output of the compositor is a headless one, so it cannot type into a
desktop; a test starts a second sway with a Wayland output inside the
compositor and checks that the client refuses it. The flake builds it,
and the dev shell names it in `MIDNIGHT_TEST_INPUT_CLIENT`.
