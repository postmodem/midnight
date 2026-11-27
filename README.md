# Midnight

Midnight is a compiled, statically typed language for games and
simulations, written mostly by AI agents and read by agents and people.
Entities, components, systems, queries, resources and events are language
constructs, not a library. A system runs on one core, on every core or on
the GPU. Its output does not depend on the number of threads, and with
`strict`, not on the GPU. The compiler tells you what moves between CPU
and GPU memory and where each loop runs, rather than leaving you to guess.
Source files end in `.12am`.

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

## What it does

**Data laid out by hand, without writing it by hand.** Components live in
contiguous arrays, with no per-object headers and fields as small as you
declare them. The `move` system above compiles to a counted loop over two
arrays that the C compiler vectorizes.

**GPU systems.** A system marked `gpu` runs its queries as compute shaders
over those same arrays, and on the CPU, with the same meaning, where there
is no GPU. Data stays on the device between ticks unless the CPU needs it,
and `midnight plan` prints what moves. A GPU system agrees closely with its
CPU run and with other GPUs, but not to the last bit; declared `strict gpu
system`, it gives the same bits on every device and on the CPU, at a cost
in speed.

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

**Every core.** A loop whose runs cannot affect one another, like `move`
above, runs on every core of the machine, and so does a GPU system when it
runs on the CPU. The compiler finds those loops, and the program prints
exactly what it prints on one thread. `midnight loops` says where each loop
runs, and what keeps one on a single core.

**A window, drawing, keyboard and mouse.** `Window.open` shows a window
(through [SDL 3](https://libsdl.org), on the same device as GPU systems),
and a tick ends with a frame of what it drew. Four statements draw:
`Draw.rect`, `Draw.circle`, `Draw.line` and `Draw.triangle`, in GPU systems
too, where nothing travels back to the CPU. The keyboard and the mouse are
the built-in resources `Keyboard` and `Mouse`, as they were when the tick
began, plus an event for each press and release.

```ruby
system show
  Draw.rect(0, 190, 320, 10, Color(40, 40, 60))
  each Ball do |ball|
    Draw.circle(ball.x, ball.y, 4, Color(255, 200, 0))
  end
  World.stop if Keyboard.pressed?(Key::Escape)
end
```

**Headless and driven.** Any program with a window also runs with no
display, driven by a file of commands that step it, give it input, print
its frames as text and capture them as PNG files. The same commands give
the same output on every machine, so an agent can check a game the way it
checks any other program.

## Try it

Everything runs through [Nix](https://nixos.org) with flakes enabled; nothing
else needs to be installed, not even a C compiler.

```sh
nix run . -- run examples/hello.12am
nix run . -- run examples/battle.12am
nix run . -- run --debug examples/life.12am
nix run . -- check examples/bounce.12am
nix run . -- build examples/move.12am -o move && ./move
nix run . -- run examples/nbody.12am
nix run . -- plan tests/plan/reader.12am
nix run . -- loops examples/bounce.12am
nix run . -- run examples/flock.12am
nix run . -- run examples/input.12am
MIDNIGHT_WINDOW=headless MIDNIGHT_DRIVE=examples/shapes.drive nix run . -- run examples/shapes.12am
```

`nbody` is gravity among bodies on the GPU, `flock` a GPU system that moves
and draws 100,000 points, and `input` a window whose colour follows the
pointer, where a click prints where it was and Escape ends it. The last
line prints each frame of `shapes` as the statements that drew it and
writes `shapes.png` and `shapes-cloud.png`. [`examples/`](examples/) has
them all, each with its expected output.

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
Errors come with the source line and, where possible, a suggestion:

```
error: unknown component `Postion`
  --> move.12am:14:8
   |
14 |   each Postion, Velocity do |pos, vel|
   |        ^^^^^^^ did you mean `Position`?
```

The compiler is Rust with no dependencies beyond the standard library. It
emits C11, built together with a small runtime by a C compiler, and a WGSL
shader for each loop of a GPU system, run through
[`wgpu-native`](https://github.com/gfx-rs/wgpu-native).

## Documentation

- [`docs/principles.md`](docs/principles.md): what Midnight is for and how
  it is built.
- [`docs/language.md`](docs/language.md): the language reference, with what
  each construct costs.
- [`docs/running.md`](docs/running.md): the variables a program reads to
  choose its device and threads, and to run headless and driven.
- [`docs/building.md`](docs/building.md): build modes, C flags, and what a
  program is linked against.
- [`docs/development.md`](docs/development.md): working on Midnight: the
  layout of the repository and every kind of test.
- [`bench/README.md`](bench/README.md): the benchmarks.
- [`specs/`](specs/): one spec per change, with its acceptance criteria and
  outcome.

The evals measure how well agents write Midnight from its documentation
alone. They run only when a spec asks them a question, and its Outcome
says what they showed; [`evals/README.md`](evals/README.md) says how to run
them.

## Status

Midnight is at 0.8 and young. Until 1.0 a better syntax is worth a
breaking change, and every spec that changes the language raises the
minor version. There is no text, sound, sprites or 3D yet, and no files
or command-line arguments; [Not in 0.8](docs/language.md#not-in-08) lists
everything the language leaves out.

It is tested on `x86_64-linux`: GPU systems on an NVIDIA device, on an AMD
iGPU through Mesa's RADV and on Mesa's software Vulkan, and the window on
Wayland. The flake also offers `aarch64-linux`, `x86_64-darwin` and
`aarch64-darwin`, which evaluate but have not been run.

## License

MIT. See [`LICENSE`](LICENSE).
