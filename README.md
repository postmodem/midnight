# Midnight

**A compiled language for games and simulations.**

You write entities, components and systems. Midnight lays the data out as
a careful C programmer would, runs your loops on one core, on every core
or on the GPU, and tells you what it decided and why.

It is written mostly by AI agents and read by agents and people, so it
favors code that says exactly what it does, output that is the same on
every run, and errors that say how to fix them where they can.
[`docs/principles.md`](docs/principles.md) has the whole idea.

Source files end in `.12am`.

## A first look

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

```
Entity(0v1) is at (0.5, 1.0)
Entity(1v1) is at (5.0, 5.0)
Entity(0v1) is at (1.0, 2.0)
Entity(1v1) is at (5.0, 5.0)
Entity(0v1) is at (1.5, 3.0)
Entity(1v1) is at (5.0, 5.0)
```

Entities, components, systems, queries, resources and events are part of
the language, not a library, so the compiler sees all of them. That is
what lets it do the rest of this page.

## What it does

### Data laid out for you

Components live in contiguous arrays, with no per-object headers and
fields as small as you declare them.

The `move` system above compiles to a counted loop over two arrays, which
the C compiler vectorizes.

### Every core

A loop whose runs cannot affect one another, like `move`, runs on every
core of the machine. The compiler finds those loops itself; there is
nothing to annotate.

The program prints exactly what it prints on one thread, every time.
`midnight loops` says where each loop runs, and what keeps one on a
single core.

### The GPU

Mark a system `gpu` and its queries run as compute shaders over those
same arrays. Where there is no GPU, they run on the CPU with the same
meaning.

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

Data stays on the device between ticks unless the CPU needs it, and
`midnight plan` prints exactly what moves.

A GPU system agrees closely with its CPU run and with other GPUs, but not
to the last bit. Declare it `strict gpu system` and it gives the same bits
on every device and on the CPU, at a cost in speed.

### A window, shapes, text, keyboard and mouse

`Window.open` shows a window, and every tick ends with a frame of what it
drew. Four statements draw shapes: `Draw.rect`, `Draw.circle`, `Draw.line`
and `Draw.triangle`. They work in GPU systems too, where nothing travels
back to the CPU. `Draw.text` draws text in a built-in pixel font,
[monogram](https://datagoblin.itch.io/monogram), whose characters all
have one width, so a score is centered with arithmetic.

```ruby
system show
  Draw.rect(0, 190, 320, 10, Color(40, 40, 60))
  each Ball do |ball|
    Draw.circle(ball.x, ball.y, 4, Color(255, 200, 0))
  end
  Draw.text(8, 8, "SCORE #{Score.points.to_s.rjust(5, "0")}", 24, Color(255, 255, 255))
  World.stop if Keyboard.pressed?(Key::Escape)
end
```

The keyboard and the mouse are the built-in resources `Keyboard` and
`Mouse`, as they were when the tick began, with an event for each press
and release. Windows come from [SDL 3](https://libsdl.org) and are drawn
on the same device as GPU systems.

### Headless, and driven by commands

Any program with a window also runs with no display at all. A file of
commands steps any program a tick at a time and prints its entities and
resources between ticks; with a window it also gives it keys and clicks,
prints its frames as text and captures them as PNG files.

A driven program runs no tick a command did not let run, so the same
commands give the same output on every machine, and an agent can check a
game the way it checks any other program. The compiler and programs
also write JSON lines, for tools that read what they say.

## Try it

Everything runs through [Nix](https://nixos.org) with flakes enabled.
Nothing else needs to be installed, not even a C compiler.

```sh
# Programs that print
nix run . -- run examples/hello.12am
nix run . -- run examples/battle.12am
nix run . -- run --debug examples/life.12am
nix run . -- build examples/move.12am -o move && ./move

# What the compiler sees
nix run . -- check examples/bounce.12am
nix run . -- loops examples/bounce.12am
nix run . -- plan tests/plan/reader.12am

# The GPU: gravity among bodies
nix run . -- run examples/nbody.12am

# Windows: 100,000 points moved and drawn on the GPU, a window whose
# color follows the pointer, and a game: catch falling stars with the
# arrow keys (Escape ends each)
nix run . -- run examples/flock.12am
nix run . -- run examples/input.12am
nix run . -- run examples/catch.12am

# The same kind of program headless: each frame printed as the
# statements that drew it, and saved as shapes.png and shapes-cloud.png
MIDNIGHT_WINDOW=headless MIDNIGHT_DRIVE=examples/shapes.drive nix run . -- run examples/shapes.12am

# Any program, a tick at a time: its entities after two ticks
printf 'tick 2\nquery Position, Velocity\n' > look.drive
MIDNIGHT_DRIVE=look.drive nix run . -- run examples/move.12am

# The same in JSON, and the compiler's errors in JSON
MIDNIGHT_DRIVE=look.drive nix run . -- run examples/move.12am --format json
nix run . -- check tests/fail/unknown_component_query_suggest.12am --format json
```

[`examples/`](examples/) has them all, each with its expected output.

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

`build`, `run`, `check`, `plan` and `loops` take `--format json`, which
writes their diagnostics and output as JSON lines;
[`docs/json.md`](docs/json.md) describes them.

`build` names the executable after the file, without its extension,
unless `-o` says otherwise. `run` exits with the program's status. The
compiler exits 0 on success, 1 when the program has errors and 2 on a
usage error.

Errors come with the source line and, where possible, a suggestion:

```
error: unknown component `Postion`
  --> move.12am:14:8
   |
14 |   each Postion, Velocity do |pos, vel|
   |        ^^^^^^^ did you mean `Position`?
```

The compiler is Rust with no dependencies beyond the standard library. It
emits C11, which a C compiler builds together with a small runtime, and a
WGSL shader for each loop of a GPU system, which runs through
[`wgpu-native`](https://github.com/gfx-rs/wgpu-native).

## Documentation

| Read this                                      | For                                                   |
| ---------------------------------------------- | ----------------------------------------------------- |
| [`docs/language.md`](docs/language.md)         | The language reference, with what each construct costs |
| [`docs/principles.md`](docs/principles.md)     | What Midnight is for, and how it is built             |
| [`docs/running.md`](docs/running.md)           | Choosing the device and threads; running headless and driven |
| [`docs/json.md`](docs/json.md)                 | The JSON lines of the compiler and of programs        |
| [`docs/building.md`](docs/building.md)         | Build modes, C flags, and what a program links against |
| [`docs/development.md`](docs/development.md)   | Working on Midnight: the repository and its tests     |
| [`bench/README.md`](bench/README.md)           | The benchmarks                                        |
| [`specs/`](specs/)                             | One spec per change, with its acceptance criteria and outcome |

The evals measure how well agents work with Midnight from its
documentation and tools alone. They run only when a spec asks them a question, and its Outcome
says what they showed. [`evals/README.md`](evals/README.md) says how to
run them.

## Status

Midnight is at 0.9 and young. Until 1.0 a better syntax is worth a
breaking change, and every spec that changes the language raises the
minor version.

There is no text, sound, sprites or 3D yet, and no files or command-line
arguments. [Not in 0.9](docs/language.md#not-in-09) lists everything the
language leaves out.

It is tested on `x86_64-linux`: GPU systems on an NVIDIA device, on an AMD
iGPU through Mesa's RADV and on Mesa's software Vulkan, and the window on
Wayland. The flake also offers `aarch64-linux`, `x86_64-darwin` and
`aarch64-darwin`, which evaluate but have not been run.

## License

MIT. See [`LICENSE`](LICENSE).
