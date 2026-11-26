# Midnight

A compiled, Ruby-flavored language built around entities, components and
systems. The compiler is Rust with no crates; it emits C11 that is built
together with a C runtime. For each `each` of a `gpu system` it also emits a
WGSL shader, which the runtime runs through `wgpu-native`. A GPU block that
runs on the CPU, and an ordinary `each` the compiler finds independent, run
in pieces on several threads, with the output of one. `README.md` has the
overview.

## What defines what

- `docs/language.md` is the definition of the language. Where the compiler
  and the reference disagree, one of them has a bug; decide which before
  changing either.
- `specs/NNNN-name.md` is one spec per change, with a status line, acceptance
  criteria and, once implemented, an Outcome. Work starts from an approved
  spec. A draft spec is not to be implemented.
- The language and the compiler share one version, `0.MINOR.PATCH`, in
  `Cargo.toml` and in the first line of the reference. A spec that changes
  the reference raises the minor number. A fix to the compiler or the
  runtime that leaves the reference alone raises the patch number.
- Three files are contracts between stages. Change them deliberately and
  update both sides in the same commit:
  - `src/ast.rs`: parser to checker.
  - `src/tir.rs`: checker to emitter. The typed program is the only thing a
    backend, `midnight plan` or `midnight loops` reads. It carries where
    every `each` runs (`Run`), which `src/check/effects.rs` works out: the
    emitter and `midnight loops` read that verdict and decide nothing
    themselves.
  - `runtime/midnight_rt.h`: generated C to runtime. Its comments are
    requirements on `midnight_rt.c`. Its "GPU systems" section is also the
    contract for generated shaders, and a requirement on `midnight_gpu.c`.
    Its "Several threads" section is the contract for the function of an
    independent `each`: what a piece is, and what code that runs in one may
    call.
- `runtime/midnight_gpu.h` is what `midnight_rt.c` and `midnight_gpu.c` need
  from each other. Generated code never includes it.
- `runtime/midnight_pool.h` is what `midnight_rt.c` and `midnight_pool.c`
  need from each other. Generated code never includes it, and
  `midnight_pool.c` uses nothing of `midnight_rt.c`.

## Commands

Nothing is installed outside the flake. From the repository root:

```sh
nix develop -c cargo test                  # unit tests and every test program, both build modes
nix develop -c cargo clippy --all-targets -- -D warnings
nix develop -c cargo fmt
nix develop -c cargo run -- run FILE.12am  # also: build, check, emit-c; add --debug
nix develop -c cargo run -- loops FILE.12am  # where each loop runs, and what keeps one on one core
nix flake check                            # package, clippy, rustfmt, nixfmt, GPU programs on llvmpipe, ThreadSanitizer
nix run .#bench                            # timings against bench/baseline.c
nix run .#bench-cores                      # loops on one thread and on all cores
nix run .#gpu-test                         # GPU programs on this machine's GPU, both build modes
MIDNIGHT_TEST_GPU=software nix develop -c cargo test   # the suite, plus GPU programs on llvmpipe
MIDNIGHT_TEST_GPU=hardware nix develop -c cargo test   # the suite, plus GPU programs on this machine's GPU
MIDNIGHT_TEST_TSAN=1 nix develop -c cargo test         # the suite, plus the pool and the programs under ThreadSanitizer
```

Flakes see only files known to git: `git add` a new file before a `nix`
command that needs it.

## Rules

- No crates. The lexer, parser, diagnostics and argument parsing are written
  by hand, and `Cargo.lock` lists one package.
- No `unsafe` in the compiler.
- `midnight_rt.c` is C11 with libc and libm only; where it is built with
  `MN_THREADS` it also uses a POSIX mutex, thread-local storage and
  `sigsetjmp`, and nothing of those otherwise. `midnight_gpu.c` is C11
  with libc, libm and `wgpu-native`, and is compiled only into programs with
  a GPU system; a program without one links neither it nor `wgpu-native`.
- `midnight_pool.c` is C11 with libc and POSIX threads, and is compiled,
  with `-DMN_THREADS -pthread`, only into programs with a loop that can be
  split: a GPU system or an independent `each`. Any other program is built
  without it, has no threads and reads no `MIDNIGHT_THREADS`.
- All of them must stay free of anything that makes a program's output
  depend on addresses, hash order of pointers or wall-clock time (apart
  from `clock`, `sleep` and `srand` with no argument). With
  `midnight_gpu.c`, output may depend on the device only as "Determinism"
  in the reference allows.
- Nothing may make a program's output, exit status or run-time error depend
  on the number of threads, on which thread runs a piece, or on the order
  pieces finish in. A loop is split only when the result is exactly the
  one-thread result; the size of a piece and the choice to split depend on
  the loop and its rows, never on the thread count.
- Code that runs in a piece calls only what "Several threads" in
  `midnight_rt.h` lists. The rules of an independent `each` in the
  reference and that list change together.
- All-zero bytes are a valid value of every type. Generated code and the
  runtime both rely on it.
- Generated C and the runtime are compiled with `-Wall -Wextra -Werror` under
  test, in both build modes. A debug build must finish with no sanitizer
  report and no live string or list buffer.
- Never set `LD_LIBRARY_PATH`. The flake clears `NIX_HARDENING_ENABLE`
  wherever `midnight` invokes the C compiler, so programs are built with the
  flags in the README's mode table and no others. A program with a GPU
  system adds `MIDNIGHT_GPU_FLAGS`, whose runpaths are how it finds
  `wgpu-native` and the Vulkan loader.

## Tests

- `tests/run/NAME.12am` with `NAME.out`: must print exactly that, with status
  0, in release and debug mode.
- `tests/fail/NAME.12am` with `NAME.err`: `midnight check` must print exactly
  those diagnostics. Each program begins with `# expect:` and `# at:`
  comments that state the intent the `.err` is reviewed against.
- `tests/panic/NAME.12am` with `NAME.err`: must compile, then stop with
  status 1 and exactly that run-time error, within ten seconds.
- `tests/plan/NAME.12am` with `NAME.plan`: `midnight plan` must print exactly
  that, with status 0. Each program begins with an `# expect:` comment that
  states the intent the `.plan` is reviewed against.
- `tests/loops/NAME.12am` with `NAME.loops`: `midnight loops` must print
  exactly that, with status 0. Each program begins with an `# expect:`
  comment that states the intent the `.loops` is reviewed against. Every
  reason `midnight loops` can print has a program here.
- `examples/NAME.12am` with `NAME.out`: as `tests/run`.
- Every program above runs with `MIDNIGHT_GPU=off`: GPU systems run their CPU
  implementation, so the suite needs no device and is exact.
- Several threads:
  - `thread_programs` runs every program of `tests/run`, `tests/panic` and
    `examples` for which `midnight loops` prints a `gpu` or a `cores` line,
    in release and debug, with `MIDNIGHT_THREADS` set to `1`, `4`,
    `3,small`, `7,small,reverse`, `64,small,jitter` and `2,small,awake`.
    Each run must give `NAME.out`, or stop with `NAME.err` having printed
    what the first setting printed. With `small` every piece is one row,
    so the small programs of the suite are split.
  - `splits_match_expected` runs every program of `tests/run` that has a
    `NAME.split` file with `MIDNIGHT_THREADS=4,small,info` and requires
    exactly those lines after `midnight: threads: `, one for each loop the
    first time it is split: a loop that is meant to run in pieces and does
    not fails here. `UPDATE_EXPECT=1` rewrites the files.
  - A new test program for split loops puts its rows in several tables,
    with enough work to be split at `MIDNIGHT_THREADS=4` where pieces of
    several rows matter, and shows the order of what it queues or emits in
    its output. Its `.out` is what it prints on one thread.
  - `pool_stress` builds `tests/pool/stress.c` with
    `runtime/midnight_pool.c` and runs each of its modes.
  - With `MIDNIGHT_TEST_TSAN=1`, `tsan_pool_stress` does the same with
    ThreadSanitizer, and `tsan_programs` builds every program
    `thread_programs` runs in release mode with `-fsanitize=thread` and
    runs it with `4,small` and `7,small,reverse`; any report fails. Without
    the variable they do nothing. `checks.tsan` runs both in the build
    sandbox.
- GPU mode: with `MIDNIGHT_TEST_GPU=software` (llvmpipe) or
  `MIDNIGHT_TEST_GPU=hardware` (the machine's GPU), the tests
  `gpu_programs`, `gpu_stats_match_plan`, `gpu_stats_match_expected`,
  `gpu_driver_finds_its_parts` and `gpu_strict_div_sqrt` also run, on
  release and debug builds. Without it they do nothing. `checks.gpu` is the
  software mode in the build sandbox; `apps.gpu-test` is the hardware mode,
  on the device the Vulkan loader finds, which a `VK_DRIVER_FILES` the
  caller sets chooses.
  - `gpu_programs` runs every program of `tests/run`, `tests/panic` and
    `examples` that declares a GPU system on the device, as it is, again
    with `MIDNIGHT_GPU=rows`, which makes every row travel alone, and again
    with `MIDNIGHT_THREADS=4,small`.
    Output must match `NAME.out`: exactly, or, when the program has a
    `# tolerance: T` line, each `Float` within `T * max(1, |expected|)` and
    everything else, whole numbers included, exactly. A program whose GPU
    systems are all strict may not have the line. A panic program must
    stop with status 1 and exactly `NAME.err`. A run that finds no device,
    or runs a GPU block on the CPU, fails. A program with a strict GPU
    system must say the device passed the strict check, and runs with
    `MIDNIGHT_GPU=strict-cpu` too, with and without the threads, which must
    give the same output.
  - `gpu_stats_match_plan` runs `tests/run/gpu_transfers.12am` with
    `MIDNIGHT_GPU=stats` and requires every tick but the first and the last
    to make the uploads and downloads `midnight plan` lists, and no row
    up or down: its plan has no `rows` line. The last line also counts what
    the code after the loop transfers. It runs again with
    `MIDNIGHT_THREADS=4,small`.
  - `gpu_stats_match_expected` runs every program of `tests/run` that has
    a `NAME.stats` file with `MIDNIGHT_GPU=stats` and requires exactly those
    lines, also with `MIDNIGHT_THREADS=4,small`; with a `NAME.rows.stats`
    file, also with `stats,rows`. The counts are the same on every device
    and for every number of threads. Its programs use tables of 200,000 rows
    of 8 bytes where rows are to travel alone: smaller arrays always move
    whole.
  - `gpu_driver_finds_its_parts` builds a stand-in Vulkan driver that opens
    another library by name, and requires that library to be found through
    the driver's runpath. A debug build has a `dlopen` of its own in
    `midnight_gpu.c` for this: AddressSanitizer's would make the dynamic
    loader search the executable's runpath, and a driver such as NVIDIA's
    would not start.
  - `gpu_strict_div_sqrt` runs the strict `/` and `sqrt` of shaders on the
    device through `tests/device/strict_div_sqrt.c`, and requires the bits
    of the CPU's `/` and `sqrtf` with subnormals flushed: `sqrt` of every
    bit pattern and `/` of 2^28 pairs on hardware, 2^24 of each on
    llvmpipe.
  - The first three print the device they ran on. In GPU mode
    `UPDATE_EXPECT=1` rewrites `.stats` files and no others.
  - The dev shell sets `MIDNIGHT_TEST_SOFTWARE_ICD`, the Vulkan driver file
    the software mode passes to programs as `VK_DRIVER_FILES`.
- `tests/COVERAGE.md` maps the reference to the tests. Every construct,
  diagnostic and run-time error has a test; keep it that way when the
  language changes.
- `UPDATE_EXPECT=1 nix develop -c cargo test` rewrites expected files from
  actual output. Read the diff before committing: an expected file is a
  claim about the language, not a snapshot.

## Style

- Comments say what the code does. Files describe the software, not how it
  came to be written.
- Diagnostics: one line, lower case, backticks around source names, no
  trailing period; the hint goes in the label after the marker.
- Commit messages: a short subject and at most a line or two.
