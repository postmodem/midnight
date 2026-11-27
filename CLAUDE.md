# Midnight

A compiled language for games and simulations, built around entities,
components and systems. The compiler is Rust with no crates; it emits C11
that is built together with a C runtime. For each `each` of a `gpu
system` it also emits a WGSL shader, which the runtime runs through
`wgpu-native`. A GPU block that
runs on the CPU, and an ordinary `each` the compiler finds independent, run
in pieces on several threads, with the output of one. A program with a
window shows it through SDL 3, takes the keyboard and the mouse from it,
and draws its frames on the same device, or runs headless. Any program
can be driven by commands, which let it run a tick at a time and print
its entities and resources between ticks, and the compiler and programs
write JSON lines when asked. `evals/` measures how well agents work with it
from its documentation and tools alone; its harness is `src/bin/midnight-eval/`, a
second binary of the crate. `README.md` has the overview.

## What defines what

- `docs/principles.md` comes before any spec: a spec says how it serves
  the principles, and a change that works against one says so and why.
- `docs/language.md` is the definition of the language. Where the compiler
  and the reference disagree, one of them has a bug; decide which before
  changing either.
- `docs/json.md` defines the JSON forms: of the compiler's output with
  `--format json`, and of every line a program writes to standard error
  with `MIDNIGHT_FORMAT=json`. Each is made from the same data as its
  text form and says no less.
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
  from each other, and what `midnight_window.c` needs of the device.
  Generated code never includes it.
- `runtime/midnight_window.h` is what `midnight_rt.c` and
  `midnight_window.c` need from each other. Generated code never includes
  it.
- `runtime/midnight_pool.h` is what `midnight_rt.c` and `midnight_pool.c`
  need from each other. Generated code never includes it, and
  `midnight_pool.c` uses nothing of `midnight_rt.c`.

## Commands

Nothing is installed outside the flake. From the repository root:

```sh
nix develop -c cargo test                  # unit tests and every test program, both build modes; the evals' harness
nix develop -c cargo test --test harness   # the evals' harness alone
nix develop -c cargo clippy --all-targets -- -D warnings
nix develop -c cargo fmt
nix develop -c cargo run -- run FILE.12am  # also: build, check, emit-c; add --debug
nix develop -c cargo run -- loops FILE.12am  # where each loop runs, and what keeps one on one core
nix develop -c cargo run -- check --format json FILE.12am  # JSON lines; also build, run, plan, loops
nix flake check                            # package, clippy, rustfmt, nixfmt, evals' harness and tasks, GPU programs on llvmpipe, ThreadSanitizer, windows
nix run .#bench                            # timings against bench/baseline.c
nix run .#bench-cores                      # loops on one thread and on all cores
nix run .#gpu-test                         # GPU programs on this machine's GPU, both build modes
MIDNIGHT_TEST_GPU=software nix develop -c cargo test   # the suite, plus GPU programs on llvmpipe
MIDNIGHT_TEST_GPU=hardware nix develop -c cargo test   # the suite, plus GPU programs on this machine's GPU
MIDNIGHT_TEST_TSAN=1 nix develop -c cargo test         # the suite, plus the pool and the programs under ThreadSanitizer
MIDNIGHT_TEST_FLOAT32=1 nix develop -c cargo test float32  # the text of every finite Float32
nix develop -c bash tests/window/test.sh OUT           # windows inside a compositor of their own, on this machine's GPU
nix run .#window-test                                  # the same with the packaged compiler
nix run .#eval -- run [--tasks A,B] [--models M,N] [--trials K] [--label L]  # agents on the eval tasks, at HEAD
nix run .#eval -- report RESULTS           # also: compare A B; grade TASK FILE; grade --run RESULTS
nix run .#eval -- self-test [TASK...]      # every task's own programs grade as they should
nix run .#eval -- isolation                # haiku tries to leave its directory; the token must reach nothing
```

`nix run .#eval -- run` starts Claude Code sessions: it needs the network
and the account, and costs usage. `evals/README.md` has the details.

Nothing opens a window on the desktop it is run from: a program with a
window runs headless, or inside `tests/window/compositor.sh`, which starts
a sway of its own that shows nothing and has no input devices, and a
session bus of its own. Nothing sends input to the desktop either: that
sway gets a keyboard and a pointer from `tests/window/input.c` while
`window_input` and `input_client_takes_its_commands` run, and from
nothing else, and `input.c` refuses any compositor whose outputs are not
all headless.

Flakes see only files known to git: `git add` a new file before a `nix`
command that needs it.

## Rules

- No crates. The lexer, parser, diagnostics and argument parsing are written
  by hand, and `Cargo.lock` lists one package.
- No `unsafe` in the compiler or the evals' harness.
- `midnight_rt.c` is C11 with libc and libm only, and defines
  `_POSIX_C_SOURCE` for `getline`, with which every program reads the
  commands of `MIDNIGHT_DRIVE`; where it is built with
  `MN_THREADS` it also uses a POSIX mutex, thread-local storage and
  `sigsetjmp`, and nothing of those otherwise; where it is built with
  `MN_WINDOW` it also uses `clock_nanosleep` for the pace of a window.
  `midnight_gpu.c` is C11 with libc, libm and `wgpu-native`, and is
  compiled only into programs with a GPU system or a window; a program
  with neither links neither it nor `wgpu-native`.
- `midnight_window.c` is C11 with libc, libm, SDL 3 and `wgpu-native`, and
  is compiled, with `-DMN_FRAMES` and `midnight_gpu.c`, only into programs
  with a window, when `MIDNIGHT_GPU_FLAGS` and `MIDNIGHT_WINDOW_FLAGS` are
  both set. It is called from the thread that runs the ticks and no other.
  A headless window never starts SDL.
- `midnight_pool.c` is C11 with libc and POSIX threads, and is compiled,
  with `-DMN_THREADS -pthread`, only into programs with a loop that can be
  split: a GPU system or an independent `each`. Any other program is built
  without it, has no threads and reads no `MIDNIGHT_THREADS`.
- All of them must stay free of anything that makes a program's output
  depend on addresses, hash order of pointers or wall-clock time (apart
  from `clock`, `sleep` and `srand` with no argument). With
  `midnight_gpu.c`, output may depend on the device only as "Determinism"
  in the reference allows. A window's pace waits for the clock and changes
  nothing a program computes; what the desktop does enters only when a
  tick begins, a headless run takes it from its commands alone, and a
  driven one takes its keyboard, mouse and focus from its commands.
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
  flags in the mode table of `docs/building.md` and no others. A program
  with a GPU system adds `MIDNIGHT_GPU_FLAGS`, whose runpaths are how it finds
  `wgpu-native` and the Vulkan loader; a program with a window adds
  `MIDNIGHT_WINDOW_FLAGS` too, for SDL 3. `midnight` removes
  `LD_LIBRARY_PATH` for the C compiler and for the program `run` starts.
  The suite runs programs directly: a shell whose `LD_LIBRARY_PATH` was set
  by something else (some launchers set it to their own ALSA) breaks every
  program linked with SDL 3, so unset it before `cargo test`.
- The evals run only when a spec asks a question an eval can answer; no
  spec runs them by default. An idea for an eval starts as a spike, a
  few sessions on a rough task read by hand, and becomes tasks, trials
  and a ledger entry only when the spike shows it can tell something.
  Runs that compare are made with the same models, effort, kit and
  trials, and the spec's Outcome has the `compare`. A task added or
  changed is run once per model on the commit that adds or changes it,
  and the results are committed in the next commit. Results are
  committed; transcripts never are.
- Every eval (a set of tasks and the measure read from them) is
  experimental until `evals/ledger.md` records it useful, and an
  experimental eval decides nothing: it may inform. A spec that runs one
  states before the first run which result would change or inform its
  decision (with none, the runs are a regression check only), which
  measure is read, and what would show the eval useful. Before an eval
  is relied on, it shows it can move: no ceiling, and a worse result
  from a reference or compiler made worse on purpose. Its Outcome says
  what the eval added, not counting fixes to the harness or tasks, and
  adds an entry to the ledger. An experimental eval not shown useful
  after two specs is iterated on, replaced or removed. A worse result is
  not a veto by itself: the evals measure writing, not review or
  correctness beyond the task's checks, and the Outcome says what was
  weighed against it.
- The README's paragraph on the evals names no task and no rule of
  grading: every agent the evals start reads the README.

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
- Drawing: `draws_match_expected` runs every program of `tests/run` and
  `examples` that has a `NAME.draws` file headless with `MIDNIGHT_GPU=off`
  and its `NAME.drive`, in both build modes, and requires the answers to
  its `draws` commands, with `midnight: drive: ` removed, to be exactly
  that file. `UPDATE_EXPECT=1` rewrites `.draws` files from its release
  run and no other. A `NAME.frames` file holds the other answers.
  `thread_programs` requires the same `draws` answers for every setting.
- Every program runs with `MIDNIGHT_DRIVE` set to its `NAME.drive` file,
  `capture` lines blanked, or to an empty file, and a program with a
  window with `MIDNIGHT_WINDOW=headless`; the display variables name
  nothing that can be connected to and `XDG_RUNTIME_DIR` is an empty
  directory (`no_display`). A new test that runs a window program outside
  window mode does the same. A driven program runs no tick its commands
  did not let run, so a program that ticks before `Window.open` or after
  its window closes has `tick` lines for those ticks too.
- A run's answers are split by their first line: those of `draws`
  (`NAME.draws`), of `query` and `resource` (`NAME.queries`), and the
  others (`NAME.frames`). `queries_match_expected` runs every program of
  `tests/run` and `examples` with a `NAME.queries` file as
  `draws_match_expected` does, and `UPDATE_EXPECT=1` rewrites them from
  the release run; `thread_programs` requires the same answers for every
  setting, and `gpu_programs` holds them to the program's tolerance.
  `query_errors` has every error of `query` and `resource`, and
  `a_program_without_a_window_is_driven` a windowless program driven.
- JSON:
  - `tests/json_lines/mod.rs` reads JSON lines strictly (Midnight's own
    escapes, no spaces, no member twice) and writes a program's JSON
    lines back as their text, `query` and `resource` values as their
    leaves.
  - `run_both_forms` runs a program as it is and with
    `MIDNIGHT_FORMAT=json` and requires the same status and standard
    output, and the JSON lines, as text, to be its standard error. It
    runs every program of `tests/run`, `examples` and `tests/panic`
    (`programs_in_json`), every case of `drive` (so `drive_errors`,
    `drive_answers` and `query_errors`), the `.split` programs, and in
    GPU mode the `.stats` and `.frames` programs.
  - `fail_programs_in_json` and `plan_and_loops_in_json` hold
    `check`, `plan` and `loops --format json` to every `.err`, `.plan`
    and `.loops` file.
  - `tests/json/NAME.12am` with `NAME.json`: exactly what `check`,
    `plan` or `loops --format json` prints, as its first line `# json:
    COMMAND` says, or, for `# json: run`, the standard error of a
    headless run with `NAME.drive` and `MIDNIGHT_FORMAT=json`; the first
    line's version is written `VERSION`. `UPDATE_EXPECT=1` rewrites them.
  - `tests/runtime/lines.c` is the runtime with a main of its own:
    `float32_text_round_trips` checks the `Float32` rule on a sample, or
    on every finite pattern with `MIDNIGHT_TEST_FLOAT32=1`, and
    `lines_no_program_writes_on_demand` the GPU error, window error,
    out-of-memory and buffers lines in both forms, with
    `tests/runtime/pool_line.c` for the pool's own.
  - `compiler_messages_in_json`, `format_takes_one_word_on_five_commands`,
    `the_format_variable_takes_one_word` and `run_passes_the_format`
    cover the compiler's messages and the two switches.
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
  - `pieces_and_threads_follow_the_work` holds the two loops of
    `tests/run/cores_emit.12am` to the pieces and threads their weight and
    rows give: change it together with a weight or with one of the
    numbers in "What several cores cost".
  - `only_programs_that_split_read_the_threads_variable` requires an
    unknown word of `MIDNIGHT_THREADS` to be reported by a program with a
    loop that can be split, and by no other.
  - `pool_stress` builds `tests/pool/stress.c` with
    `runtime/midnight_pool.c` and runs each of its modes. Run it under
    load when the pool changes: its races need many threads to show.
  - With `MIDNIGHT_TEST_TSAN=1`, `tsan_pool_stress` does the same with
    ThreadSanitizer, and `tsan_programs` builds every program
    `thread_programs` runs in release mode with `-fsanitize=thread` and
    runs it with `4,small` and `7,small,reverse`; any report fails. Without
    the variable they do nothing. `checks.tsan` runs both in the build
    sandbox.
- GPU mode: with `MIDNIGHT_TEST_GPU=software` (llvmpipe) or
  `MIDNIGHT_TEST_GPU=hardware` (the machine's GPU), the tests
  `gpu_programs`, `gpu_stats_match_plan`, `gpu_stats_match_expected`,
  `gpu_driver_finds_its_parts`, `gpu_strict_div_sqrt`, `window_frames` and
  `window_drawn` also run, on release and debug builds. Without it they do
  nothing. `checks.gpu` is the software mode in the build sandbox; `apps.gpu-test` is the hardware mode,
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
    The `draws` answers of a program with a `NAME.draws` file must be
    those: exactly, or, with the line, each number of a shape within the
    bound. A program with a line `# on the CPU: REASON` runs its blocks
    that say so on the CPU for exactly that reason and no other
    (`draw_unfit`, whose block needs more bindings than any device has);
    `window_drawn` and `window_shown` hold it to the line too.
  - `gpu_stats_match_plan` runs `tests/run/gpu_transfers.12am` with
    `MIDNIGHT_GPU=stats` and requires every tick but the first and the last
    to make the uploads and downloads `midnight plan` lists, and no row
    up or down: its plan has no `rows` line. The last line also counts what
    the code after the loop transfers. It runs again with
    `MIDNIGHT_THREADS=4,small`.
  - `gpu_stats_match_expected` runs every program of `tests/run` and
    `examples` that has a `NAME.stats` file with `MIDNIGHT_GPU=stats` and requires exactly those
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
  - `window_frames` runs every program of `tests/run` that has a
    `NAME.frames` file headless with all its commands, `capture` included,
    in a directory of its own, and requires `NAME.out`, exactly the answers
    in `NAME.frames`, and each captured file to be a PNG of the size and
    pixels (by CRC-32) its answer gives. The answers are the same on every
    device. `frames_are_the_colors_painted`, which needs no device, holds
    the CRCs in those files to the colors the programs paint: change it
    with them.
  - `window_drawn` runs every program of `tests/run` and `examples` whose
    commands have a `draws` on the line before a `capture`, headless, in
    both build modes, as given and again after a `scale` of 1.25, 1.5 and
    2, and requires each such capture to agree with what
    `tests/render/mod.rs`, the reference renderer, makes of that `draws`
    answer: every byte equal, but for a pixel under translucent draws,
    which may differ by one for each of them. The renderer states
    "Which pixels a shape covers" and "Color" apart from the shaders and
    has unit tests of its own.
  - The first three and `window_frames` print the device they ran on. In
    GPU mode `UPDATE_EXPECT=1` rewrites `.stats` and `.frames` files and no
    others.
  - The dev shell sets `MIDNIGHT_TEST_SOFTWARE_ICD`, the Vulkan driver file
    the software mode passes to programs as `VK_DRIVER_FILES`.
- Window mode: with `MIDNIGHT_TEST_WINDOW=1`, inside
  `tests/window/compositor.sh` (the tests fail outside it), `window_shown`,
  `window_shown_takes_no_resize`, `window_keeps_time`, `window_closes`,
  `window_resized`, `window_input`,
  `input_client_refuses_a_desktop` and `input_client_takes_its_commands`
  run, one at a time, in release and debug builds where they build a
  program, on the device `MIDNIGHT_TEST_GPU` names or the hardware one.
  `tests/window/test.sh` runs the smoke program and then these;
  `checks.window` runs it in the build sandbox (sway with pixman,
  llvmpipe) and `apps.window-test` on this machine's GPU. The compositor
  floats every window at the top left at the size it asks for, and sends
  one titled `midnight hidden` to a workspace that is not shown. The tests
  act on windows only through `swaymsg` and take pictures with `grim`;
  only `tests/window/input.c` types or clicks, as below.
  - `tests/window/input.c` is a Wayland client that gives the compositor a
    virtual keyboard and pointer and carries out key, button and wheel
    commands from its standard input. It connects only to the socket whose
    absolute path is its one argument, reads no display variable, and
    stops with status 1, having sent nothing, unless every output of the
    compositor is named `HEADLESS-` and a number. The flake builds it, and
    the dev shell, `checks.window` and `apps.window-test` name it in
    `MIDNIGHT_TEST_INPUT_CLIENT`. While it is connected every window in
    the compositor gets the focus and the pointer, so it runs only for the
    two tests that need it and is stopped after each.
  - `window_shown` runs the `.frames` programs whose commands have no
    `resize` or `scale` with a window, the commands from a named pipe: the
    answers and output must be the headless ones, `info` must name
    `wayland`, and a picture of the compositor must hold the last capture's
    pixels. It runs the programs of `window_drawn` with no `resize` or
    `scale` too, each capture held to the reference renderer.
  - `window_keeps_time`: 120 ticks in 1.9 to 2.3 seconds with the output at
    60 Hz, at 144 Hz, and with the window on a workspace that is not shown.
    A second program, which draws ten thousand shapes a tick, is held to
    the same.
  - `window_closes` and `window_resized`: a close and a resize by the
    compositor reach the program as the events.
  - `window_input` runs `tests/run/input_events.12am` with a window, not
    driven, at a scale of 1.5, 20 times in each build mode. Once `info`
    says the window has the focus and the pointer, it sends the input of
    the program's `.drive` file one group (up to a `tick`) at a time, each
    once the program has printed what the group before gave: keys and
    buttons through `input.c`, `mouse move` through sway's cursor
    commands, `blur` and `focus` as moves to an empty workspace and back,
    `mouse leave` not at all. The output must be that of the headless run
    of the `.drive` file. The table from `Key` names to evdev codes is in
    the suite, beside the test.
  - `input_client_refuses_a_desktop` starts a second sway with the Wayland
    backend inside the compositor, whose output is `WL-1`: `input.c`
    pointed at it must stop with status 1 and its line, and that sway must
    list no virtual device.
  - `input_client_takes_its_commands`: `input.c` carries out a command for
    every `Key` and `MouseButton` and turns of the wheel, and stops with
    status 1 at a command it does not know or a number out of range.
- `input_client_needs_an_absolute_socket` runs whenever
  `MIDNIGHT_TEST_INPUT_CLIENT` is set, outside window mode too: with no
  argument, two, a relative path, or a path with no socket, `input.c` must
  stop with status 1 and say why.
- A program that reads the keyboard or the mouse takes its input from its
  `.drive` file in every mode but `window_input`; `drive_errors` holds
  every command error, in the order they are reported.
- Evals: there are no tasks under `evals/tasks/` now; "Adding a task" in
  `evals/README.md` says what one is, and `self-test NAME` checks its
  programs. The harness's tests are `tests/harness.rs` and
  `tests/harness/`, their recorded inputs `evals/test/`; `cargo test`
  runs them, and `checks.evals` runs them in the build sandbox with the
  packaged compiler. The package's own check leaves them out.
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
