# 0001 — Language core: Midnight 0.1, its compiler and its runtime

- **Status:** implemented
- **Tested on:** x86_64-linux, 2026-11-24

## Goal

`midnight build game.12am` turns a Midnight program into a native executable,
and `midnight run game.12am` builds and runs it. The language is
[Midnight 0.1](../docs/language.md): statically typed, Ruby-flavored, with
entities, components, systems, queries, resources and events as language
constructs rather than a library.

After this spec, a program of a few hundred lines (a simulation with several
systems, events and thousands to millions of entities) compiles with helpful
errors and runs deterministically. Its data is laid out the way a
data-oriented C program would lay it out by hand: components in contiguous
arrays, no per-object headers, fields as small as the programmer declares
them. A query over that data runs at close to the speed of the hand-written
loop.

## Scope

- `docs/language.md`: the language reference. This spec implements all of it.
- The `midnight` compiler, with the commands `build`, `run`, `check` and
  `emit-c`.
- The runtime library that every compiled program links: entity and component
  storage, queries, deferred changes, events, strings, lists and the
  built-ins.
- Compile-time diagnostics with source excerpts and suggestions, and runtime
  errors with source locations.
- `examples/`: six programs that double as end-to-end tests.
- A test suite that runs every test program in both build modes.
- `flake.nix` pinned to `nixos-26.05`, with outputs for `x86_64-linux`,
  `aarch64-linux`, `x86_64-darwin` and `aarch64-darwin`: `packages.default`,
  `apps.default`, `apps.bench`, `devShells.default`, `checks` and
  `formatter`.
- `README.md`, `CLAUDE.md`, `LICENSE` (MIT), `.gitignore`, `.envrc`.

## Non-goals

- Everything listed under "Not in 0.1" in `docs/language.md`.
- Running systems in parallel, or analysing which systems could be.
- Storage layouts other than one array per component per table: no per-field
  arrays, and no sparse storage for components that are added and removed
  often.
- Spatial indexes or any other acceleration of nested queries.
- Running anything on the GPU. That is drafted as 0002, and this spec is
  shaped so that 0002 adds to it rather than reworks it (see "Ready for the
  GPU").
- Graphics, audio, windowing or input.
- Calling C from Midnight, or Midnight from C.
- A formatter, a language server, editor plugins or a REPL.
- Incremental or separate compilation. Every build compiles the whole
  program.
- A native code generator. The compiler emits C.
- Windows, and 32-bit targets.
- Verifying the three systems other than `x86_64-linux` on real hardware.
  The flake offers them and they must evaluate.

## Design

### Compiler

**Written in Rust, with no crates.** The compiler is one Cargo package,
`midnight`, that depends only on the standard library: the lexer, parser,
diagnostics renderer and argument parsing are written by hand. `Cargo.lock`
is committed. The Rust toolchain comes from the pinned nixpkgs, so
`nix build`, `nix flake check` and the dev shell all use the same compiler.

**Pipeline.** Each stage is a module with one job:

| Stage     | Input → output                | Responsibility                          |
| --------- | ----------------------------- | --------------------------------------- |
| `source`  | paths → files                 | Load the root file and resolve `require`, each file once. |
| `lexer`   | text → tokens                 | Tokens with spans, string interpolation, the `?`/`!` suffix rule, statement-ending newlines. |
| `parser`  | tokens → AST                  | Recursive descent, following the grammar in the reference. |
| `check`   | AST → typed program           | Name resolution, type inference and checking, constant expressions, place and read-only rules, query validation. |
| `emit`    | typed program → C             | One C translation unit for the program. |
| `driver`  | C → executable                | Write the C and the runtime to a build directory and invoke the C compiler. |

The typed program is the only thing `emit` reads. It carries a resolved type
on every expression and has no unresolved names, so a second backend could be
written against it later without touching the front end.

**The backend emits C11.** The generated C and the runtime are compiled and
linked in a single C compiler invocation; nothing but libc and libm is linked.
The C compiler is `$MIDNIGHT_CC` if set, otherwise `cc` on `PATH`. The packaged
`midnight` is wrapped so that `MIDNIGHT_CC` defaults to the clang from the
pinned nixpkgs, which makes `nix run` work on a machine with no compiler
installed. Generated code carries `#line` directives, so C-level tools
(debuggers, sanitizers, profilers) report `.12am` locations.

If the C compiler rejects generated code, that is a compiler bug: `midnight`
says so, keeps the C file, and prints its path and the C compiler's output.

**Two build modes.**

| Mode      | Flag      | C flags                                         | Extra checks |
| --------- | --------- | ----------------------------------------------- | ------------ |
| release   | (default) | `-O2 -fwrapv`                                   | none         |
| debug     | `--debug` | `-O0 -g -fwrapv -fsanitize=address,undefined`   | allocation accounting |

The checks the language defines (list bounds, missing components, division by
zero) are on in both modes. In debug mode the runtime also counts live string
and list buffers, tears the world down when the program ends normally, and
fails with a message and status 70 if any buffer is still alive.

### Command line

```
midnight build FILE [-o OUT] [--debug]   compile to an executable
midnight run FILE [--debug]              compile to a temporary directory and run
midnight check FILE                      report errors without generating code
midnight emit-c FILE                     print the generated C
midnight --version | --help
```

`build` writes the executable to the current directory, named after the root
file without its extension, unless `-o` says otherwise. `run` exits with the
program's status. The compiler itself exits 0 on success, 1 when the program
has errors, and 2 on a usage error.

### Diagnostics

Compile errors use the format shown in the reference: a one-line message, the
location, the source line, a marker under the offending span, and a hint when
there is one. Color is used only when standard error is a terminal.

The parser reports the first syntax error in a file. The checker keeps going
and reports every independent error, up to 20.

Each of these has a dedicated message and a test:

- unexpected token; and a block that is never closed, pointing at the keyword
  that opened it,
- unknown variable, function, type, field, method or enum variant, with a
  "did you mean" suggestion when a name within a small edit distance exists,
- type mismatch, stating the expected and found types,
- wrong number of arguments; unknown or repeated keyword argument; missing
  field in a constructor,
- a condition that is not a `Bool`,
- assignment to something read-only, saying why it is read-only,
- assigning a value of a different type to an existing variable,
- query errors: a tag used as a term (with the hint to use `with:`), a
  non-component in a query, a repeated type, a parameter count that does not
  match the terms,
- a bare component in `spawn` or `add` whose fields lack defaults,
- a compact type used for a variable, parameter or return value, with the
  hint that these are `Int` or `Float`; and a constant that does not fit the
  compact field it is stored in,
- `[]` with no type to infer,
- a recursive function without a declared return type,
- `case` over an enum that misses variants, naming them,
- a duplicate definition, pointing at the first one,
- a `require` that can't be found.

### Runtime

**Written in C11, embedded in the compiler.** The runtime is two files,
`runtime/midnight_rt.h` and `runtime/midnight_rt.c`, compiled into the
`midnight` binary with `include_str!` and written next to the generated C at
build time. Everything that depends on the program's types is generated;
everything that does not is in the runtime.

**Archetype storage.** Entities with the same set of components live in one
table, with one contiguous array per component and one for the entity
handles. Adding or removing a component moves the entity's row to another
table; removal within a table swaps the last row into the gap. The compiler
assigns every component a small integer id and emits a table of sizes,
alignments and destructors for the runtime to use. Tags take no storage.
Tables are found by component set through a hash map, and each table caches
the tables reached by adding or removing one component. Arrays grow by
doubling.

**Compact layout.** A component is a C struct with no header. The compiler
orders its fields by decreasing alignment, which minimises padding; source
order is kept for construction and `to_s`. `Bool` is one byte. An enum is the
smallest unsigned integer that holds its variants. Compact field types map to
the C type of that width. A string or a list is one pointer.

**Entity handles.** A handle is 64 bits: a 32-bit index into an entity table
and a 32-bit generation. Destroying an entity bumps the generation of its
slot and puts the index on a free list. A handle is alive when its generation
matches the slot's. The entity table stores 12 bytes per slot (generation,
table, row), and each table stores its entities' handles, so an entity costs
20 bytes beyond its components.

**Random access.** `e[C]` reads the entity's slot, then finds the column
through a per-table array indexed by component id. Both steps are constant
time.

**Queries become loops over columns.** Each `each` in the source gets a
static cache of the tables it matches, extended when new tables appear. The
generated code is a loop over those tables, skipping empty ones, and inside
it a counted loop over rows that indexes typed column pointers directly.
Filters are resolved when a table is matched and cost nothing per entity.

**Query loops are written for the vectorizer.** When a block contains no call
to a user function and no `e[C]` access, nothing in it can reach the columns
except the block parameters. The emitter then declares the column pointers
`restrict`, and loads each resource field the block reads but never writes
into a local before the loop. Reads and writes of compact fields are plain
conversions. Under those conditions a block over numeric components compiles
to a loop that clang vectorizes. Stores to compact integer fields carry a
range check and are not expected to vectorize.

**Deferred changes.** The runtime keeps a nesting depth for `each`. While it
is above zero, `spawn`, `despawn`, `add` and `remove` append to a command
buffer that owns the component values; when the depth returns to zero the
buffer is applied in order. A deferred `spawn` reserves its handle at once.
`break`, `next` and `return` out of an `each` are generated so that the depth
is always restored.

**Places are resolved late.** Columns can move whenever a structural change
is applied. For a read or write of `e[C]…`, generated code first evaluates
every operand (the entity, indexes, arguments, the right-hand side) into
temporaries, then looks up the address and uses it with no user code in
between.

**Events.** Each event type has a buffer and a running sequence number. Each
`on` in the source has a static cursor holding the sequence number it has
read up to. The end of a tick discards events from before the previous
tick's end.

**Strings and lists are reference counted.** A string or list is a pointer to
a buffer with a count, and a list is copied before a write when its buffer is
shared. Because every type is a value type, buffers can't form cycles, so
counting frees everything and no collector is needed. String literals are
static and never counted. For each type that contains a string or list, the
compiler generates a copy function and a destructor. Counts are not atomic.

**All-zero bytes are a valid value of every type:** `0`, `0.0`, `false`, the
empty string, the empty list, `Entity.none`, and the first variant of an
enum. The runtime relies on this to initialise storage.

**Determinism.** Nothing in the runtime depends on addresses, hash order of
pointers or wall-clock time, apart from `clock`, `sleep` and `srand` with no
argument. The random generator is xoshiro256\*\* with a fixed default seed.

### Ready for the GPU

Nothing here runs on a GPU, but three properties of this spec exist so that
GPU systems (0002) can be added without changing it:

- **Columns of plain components are plain bytes.** Their layout is fixed by
  the compiler, with no pointers and no headers, so a column can be copied
  to a GPU buffer as it is. The compact types give fields the 32-bit widths
  GPUs compute in.
- **Access is recorded.** The `check` stage records, for every query term
  and every `e[C]`, whether the code reads or writes the component. In this
  spec the emitter uses that to mark read-only columns `const`. Later it is
  what tells the compiler when data must move between CPU and GPU.
- **Column addresses are never kept.** Generated code asks the runtime for a
  column's address at the start of each table's loop and at each `e[C]`, and
  does not hold it past that. A column can therefore gain a second home
  behind those calls.

### Tests

`cargo test` runs everything; no network or GPU is needed, so it also runs
inside `nix flake check`.

- **Unit tests** for the lexer, parser and checker.
- **`tests/run/NAME.12am` with `NAME.out`:** the program must compile and
  produce exactly that standard output with status 0. Each one is built and
  run twice, in release and in debug mode. Debug mode must produce the same
  output with no sanitizer report and no live buffers at exit.
- **`tests/fail/NAME.12am` with `NAME.err`:** `midnight check` must fail with
  exactly those diagnostics.
- **`tests/panic/NAME.12am` with `NAME.err`:** the program must compile, then
  end with status 1 and exactly that runtime error.
- **Examples:** every `examples/NAME.12am` has a `NAME.out` and is run like a
  `tests/run` program. `bench.12am` and `bench/churn.12am` are compiled in
  both modes but run only by `apps.bench`.

Generated C is compiled with `-Wall -Wextra -Werror` under test, and without
them for users.

Coverage is tied to the reference: every construct, operator and built-in in
`docs/language.md` appears in at least one `tests/run` program; every
diagnostic listed above has a `tests/fail` case; every runtime error the
reference lists has a `tests/panic` case.

### Examples

| File          | Shows                                                           |
| ------------- | --------------------------------------------------------------- |
| `hello.12am`  | Top-level statements and `puts`.                                |
| `move.12am`   | The first program in the reference, verbatim.                   |
| `bounce.12am` | Structs with operator methods, resources, constants, lists and strings: particles under gravity in a box, drawn as text every few ticks. |
| `battle.12am` | Enums, tags, events, functions, `rand`, deferred `despawn`, `World.run` and `World.stop`: two teams fight until one is gone. |
| `life.12am`   | `add` and `remove` driven by a query, a list in a resource, `require`: Conway's Life on a small torus. |
| `bench.12am`  | One million entities with `Position` and `Velocity`, each two `Float32` fields, moved for 100 ticks; prints entity updates per second. |

`bench/churn.12am` is a second benchmark, not an example. It measures the
operations that are not iteration and prints a rate for each: `spawn`,
`despawn`, `add` and `remove` of a tag, and `e[C]` access through handles
visited in a shuffled order.

### Flake

- `packages.default`: the compiler, built with `rustPlatform.buildRustPackage`
  from `Cargo.lock`, with clang available to its test phase, wrapped to
  default `MIDNIGHT_CC` to the pinned clang.
- `apps.default`: `midnight`.
- `apps.bench`: builds `examples/bench.12am` and `bench/baseline.c` with the
  same C flags, runs both, and prints both timings, their ratio, and the peak
  resident memory of each. The baseline is the same work written by hand: two
  arrays of two-`float` structs and one counted loop. The app then runs
  `bench/churn.12am` and prints its rates. It is an app because timing is not
  reproducible enough for a check.
- `devShells.default`: `cargo`, `rustc`, `clippy`, `rustfmt`,
  `rust-analyzer`, `clang` and `nixfmt`, with `MIDNIGHT_CC` set to that clang.
- `checks`: the package (which runs `cargo test`), `clippy` with warnings
  denied, `cargo fmt --check`, and `nixfmt --check`.
- `formatter`: `nixfmt-tree`.

`LD_LIBRARY_PATH` is never set.

### Repository layout

```
flake.nix  flake.lock  Cargo.toml  Cargo.lock
src/            the compiler, one module per pipeline stage
runtime/        midnight_rt.h, midnight_rt.c
docs/           language.md
examples/       *.12am with expected output
tests/          run/, fail/, panic/
bench/          baseline.c, churn.12am
specs/          one spec per change
```

### Open questions for planning

- **Hardening flags.** nixpkgs' clang wrapper adds hardening flags (fortify
  needs optimisation and warns at `-O0`). Planning must settle which to
  disable for debug builds so that `-Werror` test builds are clean.
- **Sanitizers on darwin.** LeakSanitizer is not available on every darwin
  target. The debug-mode buffer accounting does not depend on it; planning
  must confirm the sanitizer flags evaluate and are at least harmless there.
- **Measuring peak memory.** `apps.bench` needs peak resident memory on both
  Linux and darwin. Planning must pick a tool from nixpkgs that reports it
  the same way on both.

## Acceptance criteria

Each one is a command whose result can be checked. All run on x86_64-linux.

1. `nix flake check` passes: the package builds, `cargo test` passes inside
   the sandbox in both build modes, and clippy, rustfmt and nixfmt are clean.
2. `nix flake check --all-systems --no-build` passes, and `nix flake show`
   lists `packages`, `apps` (`default`, `bench`), `devShells` and `checks`
   for all four systems.
3. `nix build`, then
   `env -i ./result/bin/midnight run examples/hello.12am` prints
   `Hello, world!`. With an empty environment there is no `PATH`, so this
   shows the packaged compiler finds its C compiler by itself.
4. In `nix develop -i -k HOME -k TERM -c bash --norc --noprofile`,
   `command -v cargo rustc clang` resolves each to `/nix/store/…`, and
   `cargo test` passes.
5. `midnight run examples/move.12am` prints the positions of both entities for
   three ticks, with the frozen one never moving, and the file matches the
   first program in `docs/language.md` byte for byte.
6. `battle.12am` and `life.12am` each produce their expected output on two
   consecutive runs, and `midnight run --debug` gives the same output for
   both.
7. Every construct, operator and built-in in `docs/language.md` is exercised
   by a `tests/run` program. A checklist mapping each section of the
   reference to test files is recorded in this spec's Outcome.
8. Every diagnostic in the list under Diagnostics has a `tests/fail` case
   whose expected output includes the source excerpt and marker, and the
   misspelled-name cases include a correct "did you mean" suggestion.
9. Every runtime error listed in the reference has a `tests/panic` case that
   reports the right `.12am` file and line and exits with status 1.
10. A `tests/run` program that copies, mutates and drops strings and lists
    through locals, struct fields, component fields, resources, events,
    deferred commands and despawned entities passes in debug mode: no
    sanitizer report, and no live buffers at exit. Removing one destructor
    call from the generated C by hand makes the same run fail with status 70.
11. `nix run .#bench` prints entity updates per second for `bench.12am` and
    for `bench/baseline.c`, and Midnight's time per tick is at most 1.25 times
    the baseline's.
12. In the same run, the peak resident memory of `bench.12am` is at most
    64 MB for its one million entities: 16 bytes of components and 20 bytes
    of overhead each, with the rest as headroom for array growth and the
    process itself.
13. In the same run, `bench/churn.12am` prints a rate for `spawn`, `despawn`,
    `add`, `remove` and `e[C]`. The numbers are recorded in this spec's
    Outcome; no threshold applies.
14. `midnight emit-c examples/move.12am` shows the `move` system as a counted
    loop over rows, with `restrict` column pointers, `Time.dt` loaded before
    the loop, and no function call in the loop body. Disassembling the
    release build of `bench.12am` shows packed floating-point instructions in
    that loop.
15. `midnight emit-c examples/move.12am` declares the `Velocity` column
    pointer `const` in `move` and the `Position` column pointer not `const`.
16. A `tests/run` program declares a component with fields of mixed sizes
    (`Float`, `Float32`, `Int16`, `UInt8`, `Bool`, an enum) and the generated
    C's `sizeof` for it, checked with a static assertion the test adds, equals
    the sum of the field sizes rounded up to the largest alignment.
17. Every command in the README works as written.

## Outcome

Implemented as specified, with the differences listed under "Differences
from the design". All seventeen criteria pass on x86_64-linux.

### Acceptance criteria

| #  | Result |
| -- | ------ |
| 1  | `nix flake check` passes: the package builds, its check phase runs `cargo test` (229 unit tests and the end-to-end suite, in both build modes), and clippy, rustfmt and nixfmt are clean. |
| 2  | `nix flake check --all-systems --no-build` passes; `nix flake show` lists `packages`, `apps` (`default`, `bench`), `devShells`, `checks` and `formatter` for the four systems. |
| 3  | `env -i ./result/bin/midnight run examples/hello.12am` prints `Hello, world!`. |
| 4  | In the isolated dev shell `cargo`, `rustc` and `clang` resolve to `/nix/store/…` and `cargo test` passes. |
| 5  | `examples/move.12am` prints both entities for three ticks, the frozen one unmoved. A suite test compares the file with the first program in the reference byte for byte. |
| 6  | `battle.12am` and `life.12am` print their expected output on two consecutive runs and under `--debug`. |
| 7  | Every construct, operator and built-in has a `tests/run` program. See "Coverage" below. |
| 8  | Each listed diagnostic has a `tests/fail` case with excerpt and marker; the misspelled-name cases carry the right "did you mean". 243 cases. |
| 9  | Each run-time error has a `tests/panic` case with file, line and status 1. 60 cases, each run in both build modes. |
| 10 | `tests/run/refcount.12am` passes in debug mode with no sanitizer report and no live buffer. With one `mn_str_release` call deleted from its generated C, the same run exits with status 70 and `midnight: 18 buffers still alive at exit`. |
| 11 | `nix run .#bench`: `bench.12am` 0.371 ms per tick, `bench/baseline.c` 0.371 ms per tick, ratio 1.00 (limit 1.25), with equal checksums. |
| 12 | Same run: `bench.12am` peaks at 36.7 MB (limit 64 MB); the baseline at 17.1 MB. |
| 13 | Same run, `bench/churn.12am` on one million entities, operations per second: `spawn` 89.1 million, `despawn` 129.8 million, `add` of a tag 23.3 million, `remove` of a tag 56.0 million, `e[C]` through shuffled handles 171.8 million. Peak 58.9 MB. |
| 14 | `midnight emit-c examples/move.12am` shows `move` as a counted loop over rows with `restrict` column pointers, `Time.dt` loaded into a local before the loop, and no call in the body. The release build of `bench.12am` has only packed instructions in that loop (`cvtps2pd`, `mulpd`, `addpd`, `cvtpd2ps`). |
| 15 | In the same output the `Velocity` column pointer is `const` and the `Position` one is not. |
| 16 | A suite test appends `_Static_assert(sizeof(T_Body) == 24, …)` to the generated C of `tests/run/layout.12am`: 17 bytes of fields rounded up to the largest alignment, 8. |
| 17 | Every command in the README was run as written. |

### Coverage

`tests/COVERAGE.md` maps every table row and rule of the reference to the
programs that exercise it, and every diagnostic and run-time error to its
case. By section (names are prefixes of files in `tests/run` unless a
directory is given):

| Reference section | Tests |
| ----------------- | ----- |
| A first program, The model | `system_first_program`, `value_*`, `entity_handles`, `examples/move` |
| Lexical structure | `lex_*` (10), `tests/fail/syntax_*` |
| Types | `types_*` (5), `conv_*` (3) |
| Compact field types | `compact_*` (5), `layout`, `tests/panic/compact_*` (21) |
| Variables and values | `vars_*` (3), `value_*` (3), `place_*` (2) |
| Expressions | `ops_*` (19), `eval_*` (3) |
| Control flow | `if_*`, `unless_*`, `case_*`, `loop_*`, `break_*`, `next_*`, `modifier_*` (38) |
| Functions | `fn_*` (16) |
| Declaring data | `struct_*`, `enum_*`, `component_*`, `tag_*`, `resource_*`, `const_*` (19) |
| Entities | `entity_*` (6), `tests/panic/component_*` (11) |
| Queries | `query_*` (7), `world_*` |
| Structural changes inside `each` | `each_*` (8), `refcount` |
| Systems and ticks | `system_*` (3), `time_*` (2), `tests/panic/world_*` (5) |
| Events | `event_*` (9) |
| Built-ins | `out_*`, `tos_*`, `num_*`, `math_*`, `str_*`, `list_*`, `rand_*`, `clock_*`, `exit_*` (40), `tests/panic/list_*`, `int_*`, `power_*`, `rand_*` (23) |
| Programs and files | `require_*` (3), `examples/life` |
| What things cost | `layout`, `examples/bench`, `bench/churn` |
| Errors | `tests/fail` (243), `tests/panic` (60) |

Not covered by a test: that two runs of one program draw the same random
numbers (criterion 6 shows it for `battle.12am`), and `exit` with a non-zero
status.

### Open questions, settled

- **Hardening flags.** The compiler wrapper's fortify flag needs
  optimisation and can't be undone by a later flag, so the flake clears
  `NIX_HARDENING_ENABLE` in the dev shell, in the package's check phase and
  in the `midnight` wrapper. Programs are built with the flags of the mode
  table and no others.
- **Sanitizers on darwin.** Not run. The buffer accounting does not depend
  on LeakSanitizer; the flags evaluate.
- **Measuring peak memory.** `hyperfine` 1.20, whose JSON export reports peak
  resident memory in bytes on Linux and macOS.

### Differences from the design

- **Extra C flags.** The suite passes `-Wall -Wextra -Werror` through an
  environment variable, `MIDNIGHT_CFLAGS`, which is also there for users.
- **Warnings silenced in generated C.** Three clang warnings are turned off
  by pragma because valid programs trigger them: comparing a value with
  itself, assigning a variable to itself, and comparing a `Float32` field
  with a constant it can't hold exactly.
- **Where the error limit is applied.** The checker returns every error; the
  renderer prints the first 20 and then `note: N more errors not shown`.
- **Syntax-level rules are reported by the parser.** Chained comparisons,
  `def ==`, a query with no terms and a misplaced `require` are syntax
  errors, with dedicated messages.
- **Recursion and return types.** A recursive function must declare its
  return type only when a call's value is needed to infer it. The reference
  now says so; a recursive function that returns nothing has no type to
  declare.
- **Leaving the program.** Run-time errors and `exit` flush both standard
  streams and call `_Exit`, so that LeakSanitizer does not replace the
  status in debug builds.
- **`sleep` flushes standard output first**, so text drawn in a loop shows
  through a pipe.
- **Float text.** Positional notation is used for decimal exponents from -5
  up to 15, exponent form elsewhere. The reference now says so.
- **`restrict` and hoisting** are also withheld when a block contains a
  nested `each` or touches the parameter of an enclosing one, since two
  pointers could then name one column.
- **The reference** gained four clarifications: the recursion rule, the
  exponent thresholds of Float text, that construction and `to_s` keep the
  declared field order, and that the newline after `do |…|` and before `end`
  may be left out on a one-line block.

### Known limits

- Inside parentheses and brackets a newline never ends a statement, as the
  reference says. An `if` written there needs `then` after its condition,
  and each of its branches holds one statement.
- Deeply nested expressions are limited by the compiler's stack; there is no
  depth diagnostic.
- A counting loop whose inclusive bound is the largest `Int` does not end.
- Removing several components in one `remove` looks the destination table up
  by hash each time; a single-component `remove` uses the cached edge.
- The runtime and the generated code have not been compiled on macOS or on
  aarch64.
