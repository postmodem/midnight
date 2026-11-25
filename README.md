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

- [`docs/language.md`](docs/language.md) is the language reference.
- [`specs/`](specs/) holds one spec per change, with its acceptance criteria
  and outcome.
- [`examples/`](examples/) has six programs, from `hello.12am` to a
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
```

## The compiler

```
midnight build FILE [-o OUT] [--debug]   compile to an executable
midnight run FILE [--debug]              compile to a temporary directory and run
midnight check FILE                      report errors without generating code
midnight emit-c FILE                     print the generated C
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

| Mode    | Flag      | C flags                                       | Extra checks          |
| ------- | --------- | --------------------------------------------- | --------------------- |
| release | (default) | `-O2 -fwrapv`                                 | none                  |
| debug   | `--debug` | `-O0 -g -fwrapv -fsanitize=address,undefined` | allocation accounting |

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

## Benchmarks

```sh
nix run .#bench
```

builds `examples/bench.12am` (one million entities with two `Float32`
components, moved for 100 ticks) and `bench/baseline.c` (the same work written
by hand) with the same C flags, and prints the time per tick of each, their
ratio and their peak memory. It then runs `bench/churn.12am`, which measures
`spawn`, `despawn`, `add`, `remove` and `e[C]`.

## Working on it

```sh
nix develop
cargo test
cargo run -- run examples/hello.12am
nix flake check
```

`nix develop` provides the Rust toolchain, clang and `nixfmt`. `cargo test`
runs the unit tests and every program under `tests/` and `examples/`, each in
both build modes. `nix flake check` builds the package (which runs the same
tests in the sandbox) and checks clippy, rustfmt and nixfmt.

```
src/            the compiler, one module per stage:
                source, lexer, parser, check, emit, driver
runtime/        midnight_rt.h, midnight_rt.c
docs/           language.md
examples/       programs with their expected output
tests/run/      programs that must print exactly NAME.out
tests/fail/     programs that must be rejected with exactly NAME.err
tests/panic/    programs that must stop with the run-time error in NAME.err
bench/          baseline.c, churn.12am, bench.sh
specs/          one spec per change
```

`tests/COVERAGE.md` maps each section of the reference to the tests that
exercise it. After a deliberate change to a diagnostic,
`UPDATE_EXPECT=1 cargo test` rewrites the expected files; review the diff
before committing it.

## Status

Midnight 0.1 is the language core. It is tested on `x86_64-linux`; the flake
also offers `aarch64-linux`, `x86_64-darwin` and `aarch64-darwin`, which
evaluate but have not been run. What 0.1 leaves out is listed at the end of
the reference.

## License

MIT. See [`LICENSE`](LICENSE).
