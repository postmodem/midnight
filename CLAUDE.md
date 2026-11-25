# Midnight

A compiled, Ruby-flavored language built around entities, components and
systems. The compiler is Rust with no crates; it emits C11 that is built
together with a C runtime. For each `each` of a `gpu system` it also emits a
WGSL shader, which the runtime runs through `wgpu-native`. `README.md` has
the overview.

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
    backend or `midnight plan` reads.
  - `runtime/midnight_rt.h`: generated C to runtime. Its comments are
    requirements on `midnight_rt.c`. Its "GPU systems" section is also the
    contract for generated shaders, and a requirement on `midnight_gpu.c`.
- `runtime/midnight_gpu.h` is what `midnight_rt.c` and `midnight_gpu.c` need
  from each other. Generated code never includes it.

## Commands

Nothing is installed outside the flake. From the repository root:

```sh
nix develop -c cargo test                  # unit tests and every test program, both build modes
nix develop -c cargo clippy --all-targets -- -D warnings
nix develop -c cargo fmt
nix develop -c cargo run -- run FILE.12am  # also: build, check, emit-c; add --debug
nix flake check                            # package, clippy, rustfmt, nixfmt, GPU programs on llvmpipe
nix run .#bench                            # timings against bench/baseline.c
nix run .#gpu-test                         # GPU programs on this machine's GPU, both build modes
MIDNIGHT_TEST_GPU=software nix develop -c cargo test   # the suite, plus GPU programs on llvmpipe
MIDNIGHT_TEST_GPU=hardware nix develop -c cargo test   # the suite, plus GPU programs on this machine's GPU
```

Flakes see only files known to git: `git add` a new file before a `nix`
command that needs it.

## Rules

- No crates. The lexer, parser, diagnostics and argument parsing are written
  by hand, and `Cargo.lock` lists one package.
- No `unsafe` in the compiler.
- `midnight_rt.c` is C11 with libc and libm only. `midnight_gpu.c` is C11
  with libc, libm and `wgpu-native`, and is compiled only into programs with
  a GPU system; a program without one links neither it nor `wgpu-native`.
- Both must stay free of anything that makes a program's output depend on
  addresses, hash order of pointers or wall-clock time (apart from `clock`,
  `sleep` and `srand` with no argument). With `midnight_gpu.c`, output may
  depend on the device only as "Determinism" in the reference allows.
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
  status 1 and exactly that run-time error.
- `tests/plan/NAME.12am` with `NAME.plan`: `midnight plan` must print exactly
  that, with status 0. Each program begins with an `# expect:` comment that
  states the intent the `.plan` is reviewed against.
- `examples/NAME.12am` with `NAME.out`: as `tests/run`.
- Every program above runs with `MIDNIGHT_GPU=off`: GPU systems run their CPU
  implementation, so the suite needs no device and is exact.
- GPU mode: with `MIDNIGHT_TEST_GPU=software` (llvmpipe) or
  `MIDNIGHT_TEST_GPU=hardware` (the machine's GPU), the tests
  `gpu_programs`, `gpu_stats_match_plan`, `gpu_stats_match_expected` and
  `gpu_driver_finds_its_parts` also run, on release and debug builds.
  Without it they do nothing. `checks.gpu` is the software mode in the
  build sandbox; `apps.gpu-test` is the hardware mode.
  - `gpu_programs` runs every program of `tests/run`, `tests/panic` and
    `examples` that declares a GPU system on the device, as it is and
    again with `MIDNIGHT_GPU=rows`, which makes every row travel alone.
    Output must match `NAME.out`: exactly, or, when the program has a
    `# tolerance: T` line, each `Float` within `T * max(1, |expected|)` and
    everything else, whole numbers included, exactly. A panic program must
    stop with status 1 and exactly `NAME.err`. A run that finds no device,
    or runs a GPU block on the CPU, fails.
  - `gpu_stats_match_plan` runs `tests/run/gpu_transfers.12am` with
    `MIDNIGHT_GPU=stats` and requires every tick but the first and the last
    to make the uploads and downloads `midnight plan` lists, and no row
    up or down: its plan has no `rows` line. The last line also counts what
    the code after the loop transfers.
  - `gpu_stats_match_expected` runs every program of `tests/run` that has
    a `NAME.stats` file with `MIDNIGHT_GPU=stats` and requires exactly those
    lines; with a `NAME.rows.stats` file, also with `stats,rows`. The counts
    are the same on every device. Its programs use tables of 200,000 rows
    of 8 bytes where rows are to travel alone: smaller arrays always move
    whole.
  - `gpu_driver_finds_its_parts` builds a stand-in Vulkan driver that opens
    another library by name, and requires that library to be found through
    the driver's runpath. A debug build has a `dlopen` of its own in
    `midnight_gpu.c` for this: AddressSanitizer's would make the dynamic
    loader search the executable's runpath, and a driver such as NVIDIA's
    would not start.
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
