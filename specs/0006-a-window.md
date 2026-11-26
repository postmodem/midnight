# 0006 — A window: a program on the screen, and a way to drive it without one

- **Status:** approved
- **Depends on:** 0005

## Goal

A Midnight program prints text and nothing else. After this spec it can
open a window, keep real time, learn that the window was resized or
closed, and show a frame at the end of every tick:

```ruby
Window.background = Color(16, 16, 32)
Window.open("Drift", 800, 600)

system report
  on WindowResized do |size|
    puts "#{size.width} by #{size.height}"
  end
end

World.run
puts "closed after #{Time.tick} ticks"
```

A frame is the window's background colour and nothing more. Drawing comes
in the specs that follow, keyboard and mouse in the next one; both build on
what this one fixes: where the window comes from, how a tick relates to a
frame and to the clock, how window events enter a program, and how a
program is run and looked at with no window at all.

That last part is half of the spec. The same program runs **headless**: no
window appears, nothing is asked of the desktop, and commands from a file
or a pipe step its ticks, resize and close its window, and write a frame
to a PNG file. A headless run with the same commands prints the same
output and writes the same pictures on every machine. The test suite runs
windowed programs that way, and so does anyone, person or agent, who wants
to see what a program shows without putting it on a screen.

Measurements on the development machine, summarised under "Measured in
planning", fixed the shape of the design.

## Scope

- The built-in resource `Window`: `Window.open`, `Window.open?`, the
  window's size in points and in pixels, and `Window.background`.
- The built-in struct `Color` and the built-in events `WindowResized` and
  `WindowClosed`.
- What closing the window does to a program.
- Ticks in real time while a window is open.
- One frame for each tick, shown without waiting for the display.
- Headless runs, and the commands of `MIDNIGHT_DRIVE`: `tick`, `resize`,
  `scale`, `close` and `capture`.
- A part of the runtime that only programs with a window are built with,
  on SDL 3 for the window and its events and on the `wgpu-native` device
  that GPU systems use for the frames.
- Tests that need no display, tests of frames on a device, and tests of a
  real window inside a compositor that the tests start themselves and that
  shows nothing on the desktop, in the build sandbox too.
- The reference becomes Midnight 0.6 and the compiler 0.6.0.

## Non-goals

- Drawing anything but the background.
- Keyboard, mouse, controllers, text input and the clipboard.
- Sound.
- More than one window at a time.
- Closing, resizing, moving or renaming the window from the program, full
  screen, the cursor, an icon, and refusing a close the user asked for.
- Running faster or slower than real time while a window is open.
- Drawing a state between two ticks. A display whose rate is not a
  multiple of the tick rate shows ticks for uneven lengths of time.
- Reading entities, components or resources over the pipe. A program shows
  its state with `puts`.
- Commands that arrive at a chosen moment of real time.
- Windows, Android and the web. macOS and AArch64 are built for and not
  tested here.

## Design

### The language

Four names become built in and cannot be declared by a program: `Window`,
`Color`, `WindowResized` and `WindowClosed`. Nothing changes in the
grammar. The reference gains a section, "The window", after "Systems and
ticks".

**`Color`** is a struct, as if declared

```ruby
struct Color
  r: UInt8 = 0
  g: UInt8 = 0
  b: UInt8 = 0
  a: UInt8 = 255
end
```

Its components are the bytes of an sRGB colour, the numbers a paint
program or a style sheet shows: `Color(255, 128, 0)` is orange. It is a
plain value with the rules of any struct, and may be a field of a
component. A component out of range is the error it is for any `UInt8`.

**`Window`** is a resource with one instance, like `Time`:

| Member                 | Type    | Meaning                                         |
| ---------------------- | ------- | ----------------------------------------------- |
| `Window.open(title, width, height)` | | Opens the window. `title` is a `String`; `width` and `height` are `Int`, in points. |
| `Window.open?`         | `Bool`  | Whether a window is open. Read-only.            |
| `Window.width`         | `Int`   | Width in points. Read-only.                     |
| `Window.height`        | `Int`   | Height in points. Read-only.                    |
| `Window.pixel_width`   | `Int`   | Width of a frame in pixels. Read-only.          |
| `Window.pixel_height`  | `Int`   | Height of a frame in pixels. Read-only.         |
| `Window.background`    | `Color` | What a frame is filled with. Starts as `Color(0, 0, 0)`. Its `a` is ignored: a window is opaque. |

A **point** is the unit a desktop measures windows in. On most displays a
point is a pixel; on a display with a scale of 2 a window of 800 by 600
points has frames of 1600 by 1200 pixels. A program asks for a size in
points, so a window is the same size to the eye on every display, and
reads the pixel size where it needs the exact grid.

Before a window is opened the four sizes are 0. After one is closed they
keep their last values. The fields are read wherever a resource field is,
in a GPU system too.

**`Window.open`** follows the rule of `World.tick`: it is called from
top-level code, it is a compile error in the body of an `each`, and a
run-time error while a tick is in progress. It is a run-time error when a
window is already open, when `width` or `height` is less than 1 or more
than the largest frame the device draws, and when no window can be shown
(see "Without a display"). When it returns, the window is open, shows a
frame of the background colour, and the four sizes hold what the desktop
gave it, which need not be what was asked for. A window may be opened
again after it was closed.

**Events.** What happens to the window reaches the program when a tick
begins, before the first system runs, as events:

```ruby
event WindowResized
  width: Int
  height: Int
  pixel_width: Int
  pixel_height: Int
end

event WindowClosed
end
```

- `WindowResized` is emitted when any of the four sizes has changed since
  the last tick began, once, with the sizes the window has now. The fields
  of `Window` change at the same moment and at no other: during a tick
  they do not change.
- `WindowClosed` is emitted when the person at the desktop has closed the
  window. The window is gone when the tick begins: `Window.open?` is
  false.

They are received with `on`, by the rules of every event: emitted at the
start of a tick, they are seen by every `on` of that tick. A program
cannot `emit` them.

**Closing the window ends `World.run`.** In the tick that begins with
`WindowClosed`, the runtime also calls `World.stop`. So a program whose
last statement is `World.run` ends when its window is closed, having run
one last tick in which it can react, and a program that ticks in a loop of
its own writes

```ruby
while Window.open?
  World.tick
end
```

When a program ends, by its last statement, `exit` or a run-time error,
its window closes.

**Ticks keep real time while a window is open.** A tick does not begin
until `Time.dt` real seconds after the tick before it began. With the
default `Time.dt` that is sixty ticks a second, on every display. A tick
that takes longer than `Time.dt` is followed at once by the next, and the
time lost is not made up: the program runs slow, never in a burst. The
first tick after `Window.open` begins at once. `Time.dt` is read each time,
so a program that changes it changes its pace.

Nothing about a tick depends on that wait. `Time.tick`, `Time.dt` and
`Time.elapsed` are the simulated values they always were, and a program
with no window open, or running headless, ticks as fast as it can.

**A frame for each tick.** When a tick ends while a window is open, the
window shows a new **frame**: the picture of the world after that tick. In
0.6 the picture is `Window.background`. Showing a frame never waits for
the display, so the display's rate changes neither the pace nor the
output of a program, and a window that is hidden, minimized or on another
workspace costs the program nothing: its ticks go on.

**Determinism.** A window brings two things from outside into a program:
its sizes and the moment it is closed. Both enter only when a tick begins.
A headless run takes them from its commands alone, so it prints the same
output on every machine and for every run. A frame of 0.6 is the same
picture, pixel for pixel, on every device.

### Without a display

`MIDNIGHT_WINDOW` is a comma-separated list of words, read when the
program starts, by programs with a window only:

| Word       | Meaning                                                       |
| ---------- | ------------------------------------------------------------- |
| `headless` | Show no window. `Window.open` gives a window of exactly the size asked for, at a scale of 1, that exists in the program only. Ticks are not held to real time. |
| `info`     | Print one line to standard error when a window opens: its sizes, where it is shown (`wayland`, `x11`, `cocoa` or `headless`), and for a window that is shown, how frames reach it. |

A headless window touches no desktop, and no device until a frame is
captured.

Without `headless`, a program that cannot show a window stops at
`Window.open` with a run-time error that says why and names
`MIDNIGHT_WINDOW=headless`: no display was found, the program was built
without the window flags, `MIDNIGHT_GPU` has `off`, or the device the
program runs on cannot draw to the desktop.

`MIDNIGHT_DRIVE=FILE` gives a program with a window its **commands**: one
to a line, read from a file or a named pipe. A line that is empty or
begins with `#` is skipped. With commands a program is **driven**: its
ticks are not held to real time, headless or not, and before each tick
begins the runtime carries out commands until one lets the tick run.

| Command        | Meaning                                                    |
| -------------- | ---------------------------------------------------------- |
| `tick`, `tick N` | Let the next tick, or the next `N` ticks, run.           |
| `resize W H`   | The window becomes `W` by `H` points, as if resized at the desktop. Headless only. |
| `scale S`      | A point becomes `S` pixels, a decimal number from 0.5 to 4; a pixel size is the size in points times `S`, rounded. Headless only. |
| `close`        | The window is closed, as if at the desktop.                |
| `capture FILE` | Write the last frame to `FILE` as a PNG: 8 bits each of red, green, blue and alpha, `Window.pixel_width` by `Window.pixel_height`. |

- `resize`, `scale` and `close` take effect in the tick that begins next,
  through the events above.
- `capture` before the first tick writes the frame `Window.open` showed.
- When the commands run out the window is closed, as by `close`, and from
  then on ticks run freely.
- When the program ends, the commands up to the next `tick` are carried
  out.
- Each command is answered with one line on standard error, when it has
  been carried out: `midnight: drive: ` and the command as it was written.
  For `tick` the line comes when its last tick has finished, and says how
  many ticks have run if the program ended first. For `capture` it adds
  the number of ticks the frame is the picture of, its size in pixels and
  the CRC-32 of its pixels:

  ```
  midnight: drive: tick 3
  midnight: drive: capture after.png: 3 ticks, 320x200, crc 5f1d02c9
  ```

- A command that is not in the table, or has the wrong arguments, a
  `resize` or `scale` in a program that is not headless, and a `capture`
  that cannot be made (no window is open, no device, the file cannot be
  written) stop the program with status 1 and a line that names the file
  and the line number.

`MIDNIGHT_GPU=off` means what it says for frames as well: the program uses
no device, so it shows no window and captures nothing.

### The compiler

- `Window`, `Color`, `WindowResized` and `WindowClosed` join the built-in
  names. A program that names `Window` or one of the two events **has a
  window**; the typed program says so, and only then holds the three
  declarations: the resource, with its read-only fields marked, the
  struct and the events. They are ordinary declarations to everything
  after the checker: the effect analysis, the transfer plan, the GPU
  lowering and both emitters handle `Window.width` as they handle a field
  of any resource. A program that uses `Color` alone gets the struct and
  nothing else.
- `Window.open(…)` and `Window.open?` are members the checker resolves, as
  it resolves those of `World` and `Time`. The call is a statement of its
  own kind in the typed program, with the placement rule of `World.tick`.
- Diagnostics: assigning to a read-only field of `Window`; `emit` of a
  built-in event; `Window.open` in the body of an `each` or in a GPU
  system; a declaration that takes a built-in name.
- The emitter lays the resource out as `runtime/midnight_rt.h` declares it
  and hands the runtime its address and the numbers of the two events
  with the rest of the program's description.
- A program with a window is built with `-DMN_WINDOW`, with
  `midnight_gpu.c` and `midnight_window.c`, and with the flags of
  `MIDNIGHT_GPU_FLAGS` and `MIDNIGHT_WINDOW_FLAGS`. When either variable
  is unset `midnight` says so in one line and builds the program without
  the two files: it runs headless and captures nothing.
- A program without a window is built as in 0.5: the generated C is the
  same text, and it links nothing new.
- `midnight` removes `LD_LIBRARY_PATH` from the environment of the C
  compiler and of the program `midnight run` starts. A program finds its
  libraries through its runpaths alone, and the variable, set by something
  else for something else, would put another library in front of them.

### The runtime

**In `midnight_rt.c`**, compiled under `MN_WINDOW` and so only into
programs with a window, with libc alone:

- The window as the program sees it: the resource, what is waiting to be
  told at the next tick, and the two events.
- `MIDNIGHT_WINDOW`, and the commands: reading, carrying out, answering.
- The pace: tick `k` of a row of ticks is due `Time.dt` after tick `k - 1`
  was due; a tick that begins late is due when it begins. The wait sleeps
  to the moment and does not spin.
- PNG: 8-bit RGBA, written by the runtime itself as a deflate stream of
  fixed codes in which a run of equal pixels, along a row or down from
  the row above, is a few bytes. The same pixels always give the same
  file.
- A tick, when a window is open, in order: carry out commands until one
  lets the tick run, or wait for the tick's time; take what the desktop
  reports; update the resource and emit the events; run the systems;
  advance `Time`; make the frame.

**`runtime/midnight_window.c`** is C11 with libc, SDL 3 and `wgpu-native`.
`runtime/midnight_window.h` is what it and `midnight_rt.c` need from each
other; generated code never includes it. `midnight_gpu.h` gains what the
window needs of the device.

- **One device.** A program has one device, the one its GPU systems run
  on. A window uses it. When none is open yet, the window opens it, and a
  window that is shown asks for an adapter that can draw to it. An adapter
  that computes on the CPU is taken only with `software` in `MIDNIGHT_GPU`,
  as for GPU systems.
- **The frame is a texture the runtime owns**, `RGBA8UnormSrgb`, of the
  window's pixel size. A frame is drawn into it. A capture copies it to a
  buffer and reads it back. A window that is shown gets it by one more
  pass, a triangle that covers the surface. So the pixels a capture writes
  are the same with a window and without one, and the surface needs
  nothing but to be drawn to.
- **The background** is cleared as the linear value of each byte, computed
  on the CPU, which leaves exactly that byte in the texture on every
  device.
- **The surface** is made from the window's native handles: the Wayland
  display and surface, the X11 display and window, or on macOS the Metal
  layer of a view SDL makes. Everything it is configured with is taken
  from what the surface lists: an 8-bit sRGB format by name, and the first
  of mailbox, immediate and FIFO. A window whose surface lists no such
  format cannot be shown.
- **SDL** is asked for video only, with the Wayland driver before the X11
  one, a window that is resizable and counts in pixels of the display, no
  signal handlers of its own, and its D-Bus connection shut at the end. It
  is called from the thread that runs the ticks and from no other. A
  headless program never initialises it.
- **Events** are taken once a tick. A change of size configures the
  surface again and makes a new frame texture.
- **A frame is not presented** while SDL says the window is hidden,
  minimized or occluded, nor when the surface gives no texture in time;
  the tick is never held. Each frame's surface texture is released before
  the next is asked for.
- **Errors.** A failure of the window or the surface after `Window.open`
  has returned prints `midnight: window error: …` and ends the program
  with status 70, as an error from the device does.

`midnight_window.c` and the window parts of `midnight_rt.c` keep the rules
of the runtime: nothing in them makes a program's output depend on
addresses or on the clock, and a debug build of a program with a window
ends with no sanitizer report.

### The flake

- `MIDNIGHT_WINDOW_FLAGS`: SDL 3 from the pinned nixpkgs, its headers, its
  library and a runpath to it, set wherever `MIDNIGHT_GPU_FLAGS` is.
- `checks.window`, on Linux: the suite's real-window tests in the build
  sandbox, inside a compositor the check starts, on llvmpipe.
- `apps.window-test`: the same on this machine's GPU.
- Both start the compositor through `tests/window/compositor.sh`: sway
  with the headless backend and no input devices, in a runtime directory
  of its own, with the desktop's display, compositor and session-bus
  variables removed from everything it starts. No test opens a window on
  the desktop it is run from.

### Measured in planning

On a Ryzen 9 9950X3D with an RTX 5090 (driver 595) and the Ryzen's own
GPU (RADV), NixOS 26.05, SDL 3.4.10, `wgpu-native` 27.0.4.0, Mesa 26.1.8
and sway 1.12, with a hand-written program that opens a window, presents a
changing clear colour and runs a compute pass on the same device. Every
window was inside a headless sway on a socket of its own.

- **One device serves both.** Compute and frames went through one device
  and one submission in every run, and 1,024 counters matched the frame
  count. Opening the device and the surface took 140 to 150 ms on the
  5090 and 29 ms on llvmpipe; the SDL window 1.4 ms.
- **FIFO stalls a hidden window.** With the window on a workspace that was
  not shown for 3 seconds, one present blocked 3,018 ms on the 5090,
  3,011 ms on llvmpipe and 3,005 ms on RADV, and SDL reported nothing at
  either end. On X11 the same test ran one frame a second. Mailbox on
  Wayland and immediate on X11 ran on at full rate.
- **A timer and a present that does not wait give the same pace on every
  display.** Frames per second on the 5090:

  | Output | FIFO  | FIFO and a 60/s timer | Mailbox and a 60/s timer |
  | ------ | ----- | --------------------- | ------------------------ |
  | 60 Hz  | 62.7  | 60.0                  | 60.0                     |
  | 144 Hz | 163.1 | 60.0                  | 60.0                     |
  | 30 Hz  | 31.3  | 30.8                  | 60.0                     |

  With the timer a present took 0.02 ms and the time between frames was
  16.66 ms at the median, 15.4 to 17.5 ms at the extremes, sleeping to an
  absolute time with no spinning.
- **What surfaces list.** On Wayland all three drivers listed mailbox,
  FIFO and immediate, and four 8-bit formats, sRGB and not, in an order
  that changed with the driver and with the GPU the compositor drew on. On
  X11 the 5090 listed FIFO and immediate only.
- **Bytes are exact, values between bytes are not.** Each of the 256 byte
  values, cleared as its linear value into an sRGB texture and read back,
  was that byte on all three devices. 4,097 values between bytes gave a
  different checksum on each. A capture of the same frame was the same PNG
  file on all three, with a window and without one.
- **No display is needed.** With no SDL call and no display variable set:
  device, texture, clear, read-back, PNG; 5 frames of 640 by 360 in
  4.6 ms on the 5090. It runs in the build sandbox on llvmpipe.
- **A real window runs in the build sandbox.** sway with its software
  renderer and the program on llvmpipe: 60.0 frames a second, an exact
  capture, a matching screenshot of the compositor, in release and
  sanitizer builds. Outside the sandbox, a sanitizer build on llvmpipe
  ended with no report after nine reconfigurations of the surface and a
  close by the compositor.
- **A compositor that draws in software can only be drawn to by
  llvmpipe.** There the request for an adapter that fits the surface
  returned llvmpipe with the 5090 present, and nothing with RADV alone.
- **A surface that cannot be configured ends the process.** RADV as an
  X11 client of a compositor drawing on the other GPU was offered as
  fitting, listed its modes and formats, and `wgpu-native` aborted in
  `wgpuSurfaceConfigure`. The same pair on Wayland worked. With both
  displays there, SDL chose X11 on sway by itself.
- **Window events.** A resize by the compositor, output scales of 2 and
  1.5 (an 800 by 600 window became 1600 by 1200 and 1200 by 900 pixels),
  full screen and a close all arrived as SDL events, and on Wayland
  nothing but those events told the surface its size. A tiling compositor
  resized the window in its first 20 ms.
- **On a compositor with `fifo-v1`**, SDL chose Wayland by itself and the
  surface listed the same modes and 8-bit formats. The window had a size
  the compositor chose when `SDL_CreateWindow` returned, and another
  within its first frames. With the timer and mailbox the pace was the
  timer's, as above. With FIFO the wait was in getting the next texture,
  one refresh long. A capture of a whole window is megabytes as a PNG
  without compression.
- **`LD_LIBRARY_PATH`.** In a shell where another program's launcher had
  set it to a newer ALSA, a program linked with SDL 3 failed to link, and
  one built elsewhere did not start, headless or not.
- **Input sent through a compositor is unreliable.** One-shot tools lost
  the first key of two and every pointer event.

### Tests

- `tests/run`: programs with a window, each with a `NAME.drive` file of
  commands where it needs one. The suite runs every program with
  `MIDNIGHT_WINDOW=headless` and `MIDNIGHT_DRIVE` set to its `.drive`
  file, or to an empty one. They cover the fields before, during and
  after a window; the two events and the tick they arrive in; several
  changes between two ticks giving one event; `World.run` ending on a
  close; a loop on `Window.open?`; a window opened twice in a row; what
  running out of commands does; `Window.width` read in a GPU system.
- `tests/fail` and `tests/panic`: every diagnostic and run-time error
  above.
- `drive_errors`: every way a command can be wrong stops the program with
  status 1 and its line.
- `only_programs_with_a_window_read_the_window_variables`: an unknown word
  of `MIDNIGHT_WINDOW` is reported by a program with a window and by no
  other.
- GPU mode, `window_frames`: every program of `tests/run` with a
  `NAME.frames` file runs headless on the device, in release and debug,
  and must give exactly those lines after `midnight: drive: `. They are
  the same on every device. `UPDATE_EXPECT=1` rewrites them. One program
  captures a frame of each of the 256 values of a byte.
- Window mode, with `MIDNIGHT_TEST_WINDOW=1`, inside the compositor of
  `tests/window/compositor.sh`:
  - `window_shown`: the programs with a `.frames` file run with a window
    and their commands, and must give the same lines; `info` must name
    `wayland`; and a screenshot of the compositor must hold the pixels of
    the last capture.
  - `window_keeps_time`: 120 ticks take between 1.9 and 2.3 seconds, with
    the output at 60 Hz and at 144 Hz, and again with the window on a
    workspace that is not shown.
  - `window_closes`: a close by the compositor ends `World.run`.
  - `window_resized`: a resize by the compositor arrives as one event.
- `tests/COVERAGE.md` maps "The window" to these.
- `examples/window.12am`: a window whose background drifts through
  colours until it is closed.

### Slices

1. **A window from the repository's own flake.** `MIDNIGHT_WINDOW_FLAGS`,
   `tests/window/compositor.sh`, `checks.window` and `apps.window-test`
   around one hand-written C program, and the removal of
   `LD_LIBRARY_PATH`. If a real window cannot be had in the build
   sandbox from the flake, stop and report before going on.
2. **The language, headless.** The built-in names, the checker, the typed
   program, the window state in the runtime, the events, the close, the
   commands without `capture`, and their tests. No device.
3. **Frames.** The frame texture, the background, `capture`, PNG,
   `window_frames`.
4. **The window.** SDL, the surface, the pass that shows a frame, events
   from the desktop, the pace, `info`, the window-mode tests.
5. Reference 0.6, README, COVERAGE, the repository's `CLAUDE.md`, the
   example, Outcome.

## Acceptance criteria

All run on x86_64-linux, on the development machine.

1. `nix flake check` passes, with `checks.window` in it, and
   `nix run .#gpu-test` and `nix run .#window-test` pass on the hardware
   device.
2. Every `.out`, `.err`, `.plan`, `.loops`, `.split` and `.stats` file
   that existed in 0.5.0 is unchanged, and for every program of `tests/`
   and `examples/` that existed in 0.5.0 `midnight emit-c` and the
   generated shaders are byte for byte what 0.5.0 gives. The one
   exception is a program that declared a name this spec makes built in:
   it is renamed in the source, and its expected output is unchanged.
3. Every program of `tests/run` and `examples` with a window prints its
   expected output headless, in release and debug, with no device and no
   display variable set, and the same output on every run.
4. `window_frames` passes on the hardware device and on llvmpipe with the
   same `.frames` files, and the PNG files the two write are the same
   files.
5. `window_shown` passes in the build sandbox and on the hardware device:
   the lines of a run with a window are the lines of the headless run, and
   the screenshot of the compositor holds the captured pixels.
6. `window_keeps_time` passes on the hardware device: 120 ticks in 1.9 to
   2.3 seconds on outputs of 60 Hz and 144 Hz and with the window not
   shown.
7. A debug build of every program with a window ends with no sanitizer
   report: headless, and with a window on llvmpipe in the build sandbox.
8. No test, check or app of this spec opens a window on the desktop it is
   run from: each runs with the desktop's display variables set to a
   value that cannot be connected to, and passes.
9. A program without a window is built without `midnight_window.c`: `ldd`
   on the release builds of `examples/hello.12am` and of a program with a
   GPU system lists what it lists on 0.5.0.
10. With `LD_LIBRARY_PATH` set to a directory holding another build of a
    library SDL needs, `midnight run examples/window.12am` builds and
    runs headless.
11. `examples/window.12am`, started by hand on the development machine's
    desktop in release and in debug, shows its window, keeps sixty ticks a
    second, goes on ticking on another workspace, and ends when the window
    is closed, the debug build with no sanitizer report.
12. The reference says 0.6 and `midnight --version` 0.6.0, every command
    in the README works as written, and `tests/COVERAGE.md` maps every
    construct, diagnostic and run-time error of "The window" to a test.

## Decisions

- **A resource, and events.** The window is one thing every system may
  want to read, which is what a resource is, and what happens to it are
  messages that systems react to, which is what events are. A program
  with a window needs no construct it does not already have.
- **The outside comes in when a tick begins.** A size that changed halfway
  through a tick would be read as two values by two systems. Taken once a
  tick, the window is as fixed during a tick as `Time` is, and a headless
  run can replay it exactly.
- **Closing the window stops `World.run`.** The shortest windowed program
  is `Window.open` and `World.run`, and it must end when its window is
  closed. The last tick, with its event, is where a program saves what it
  wants to keep.
- **The clock paces ticks.** `Time.dt` already says how long a tick is;
  holding ticks to it makes a program run at the speed it describes, on a
  display of any rate and with its window out of sight, the two cases
  where a wait for the display was measured to change the pace or stop
  it.
- **One frame for each tick, shown without waiting.** The picture is the
  state after a tick, so there is nothing to show between two. A present
  that cannot wait leaves the pace to the clock alone.
- **Colours are bytes.** Bytes are what a person types and what a picture
  file holds, and they are the values every device reproduces exactly,
  which is what lets a capture be compared with a file.
- **Points for sizes, pixels beside them.** A program that asks for 800 by
  600 gets a window of the size it meant on a display of any density, and
  prints the same numbers on each.
- **No display is an error.** A program that waits for its window to be
  closed would wait for ever where nobody can close it, so a run with no
  window is one that was asked for.
- **Commands on a pipe of the runtime's own.** Stepping, resizing,
  closing and capturing through the program reach it on every machine,
  need no compositor, cannot touch anything else on a desktop, and happen
  at a known tick.
- **`capture` reads the frame.** The frame is the same
  with a window and without, so the tests of frames need no window, and
  the tests of a window need only show that the frame arrives.
- **A window needs the device.** Frames are drawn on the device that GPU
  systems use, so that what is computed there can later be drawn without
  leaving it. `MIDNIGHT_GPU=off` keeps meaning that a program uses none.
- **Wayland before X11.** Where both are offered the Wayland surface is
  the one measured to work across two GPUs and at fractional scales.
- **The window parts in files of their own**, built only into programs
  with a window, as the GPU and the pool are: every other program is
  built and runs as today.
- **`midnight` drops `LD_LIBRARY_PATH`.** Programs are linked to exact
  libraries by runpath; a variable that overrides those is never what a
  Midnight program was built for.
