# Midnight

A compiled, Ruby-flavored language built around entities, components and
systems. The compiler is Rust with no crates; it emits C11 that is built
together with a C runtime. `README.md` has the overview.

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
    backend reads.
  - `runtime/midnight_rt.h`: generated C to runtime. Its comments are
    requirements on `midnight_rt.c`.

## Commands

Nothing is installed outside the flake. From the repository root:

```sh
nix develop -c cargo test                  # unit tests and every test program, both build modes
nix develop -c cargo clippy --all-targets -- -D warnings
nix develop -c cargo fmt
nix develop -c cargo run -- run FILE.12am  # also: build, check, emit-c; add --debug
nix flake check                            # package, clippy, rustfmt, nixfmt
nix run .#bench                            # timings against bench/baseline.c
```

Flakes see only files known to git: `git add` a new file before a `nix`
command that needs it.

## Rules

- No crates. The lexer, parser, diagnostics and argument parsing are written
  by hand, and `Cargo.lock` lists one package.
- No `unsafe` in the compiler.
- The runtime is C11 with libc and libm only, and must stay free of anything
  that depends on addresses, hash order of pointers or wall-clock time
  (apart from `clock`, `sleep` and `srand` with no argument).
- All-zero bytes are a valid value of every type. Generated code and the
  runtime both rely on it.
- Generated C and the runtime are compiled with `-Wall -Wextra -Werror` under
  test, in both build modes. A debug build must finish with no sanitizer
  report and no live string or list buffer.
- Never set `LD_LIBRARY_PATH`. The flake clears `NIX_HARDENING_ENABLE`
  wherever `midnight` invokes the C compiler, so programs are built with the
  flags in the README's mode table and no others.

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
