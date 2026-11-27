# Building programs

How `midnight` turns a `.12am` file into an executable: the C compiler,
the build modes and their flags, and what each kind of program is linked
against. [Running programs](running.md) has the variables a built program
reads.

## The C compiler

The compiler is written in Rust with no dependencies beyond the standard
library. It emits one C11 file, which is compiled together with a small
runtime (`runtime/`) by the C compiler named in `MIDNIGHT_CC`, or `cc`. The
packaged compiler defaults to the clang from the pinned nixpkgs.
`MIDNIGHT_CFLAGS` adds flags to that invocation.

## Build modes

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

## What a program is linked against

A program with no GPU system and no window depends on libc and libm alone.

- A program with a loop that can run on several cores (a GPU system, or an
  `each` the compiler finds independent) is built with
  `-DMN_THREADS -pthread` and `runtime/midnight_pool.c`, and uses POSIX
  threads. Any other program has no threads.
- A program that declares a GPU system or has a window is built with
  `runtime/midnight_gpu.c` and linked against
  [`wgpu-native`](https://github.com/gfx-rs/wgpu-native) (Vulkan on Linux,
  Metal on macOS), with the flags in `MIDNIGHT_GPU_FLAGS`. Without that
  variable `midnight` says so in one line and builds the program with the C
  functions of its GPU systems only.
- A program with a window is also built with `runtime/midnight_window.c`
  and linked against [SDL 3](https://libsdl.org), with the flags in
  `MIDNIGHT_WINDOW_FLAGS`. Without either variable `midnight` says so in one
  line and builds a program that runs only headless and captures nothing.

The packaged compiler and the dev shell set both variables. Their runpaths
are how a program finds its libraries, so `midnight` removes
`LD_LIBRARY_PATH` from the environment of the C compiler and of the
program `midnight run` starts.

For each `each` of a `gpu system` the compiler emits a WGSL compute shader,
embedded in the executable as text, and a C function with the same meaning.
`midnight emit-wgsl` prints the shaders and `midnight emit-c` the C.
