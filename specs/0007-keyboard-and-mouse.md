# 0007 — Keyboard and mouse

- **Status:** implemented
- **Depends on:** 0006

## Goal

A Midnight program with a window can show it, keep time and learn that it
was resized or closed, but nothing a person does with the keyboard or the
mouse reaches it. After this spec it does:

```ruby
Window.open("Steer", 640, 480)

resource Ship
  x: Float = 320
end

system steer
  Ship.x -= 4 if Keyboard.down?(Key::A)
  Ship.x += 4 if Keyboard.down?(Key::D)
  World.stop if Keyboard.pressed?(Key::Escape)
end

system aim
  on MousePressed do |click|
    puts "#{click.button} at #{click.x}, #{click.y}" if click.button == MouseButton::Left
  end
end

World.run
```

Input follows the rule the window set: what happens at the desktop enters
a program only when a tick begins, so a tick sees one fixed state of the
keyboard and the mouse. A program reads that state from two built-in
resources, `Keyboard` and `Mouse`, and receives what happened as events.
Keys are named by where they are on the keyboard, not by what the layout
prints.

The other half of the spec is the same as in 0006: a headless run takes its
input from the commands of `MIDNIGHT_DRIVE` (`key down W`, `mouse move 120
80`, `mouse Left`), so a program that reads the keyboard and the mouse
prints the same output on every machine, and the test suite and an agent
drive it without a desktop.

## Scope

- The built-in enums `Key` and `MouseButton`, the built-in resources
  `Keyboard` and `Mouse`, and the built-in events `KeyPressed`,
  `KeyReleased`, `MousePressed` and `MouseReleased`.
- Keyboard focus: `Window.focused?` and the events `WindowFocused` and
  `WindowUnfocused`.
- The commands `key`, `mouse`, `scroll`, `blur` and `focus`.
- Keyboard, mouse and focus events from SDL 3 in `midnight_window.c`.
- A test client that gives a headless compositor a keyboard and a pointer,
  and a window-mode test of input from a real compositor.
- The reference becomes Midnight 0.7 and the compiler 0.7.0.

## Non-goals

- Text input: characters, the layout's names for keys, dead keys, input
  methods, key repeat, the clipboard.
- Lock states (Caps Lock, Num Lock) other than as keys going down and up.
- Hiding or changing the cursor, locking the pointer, motion that is not a
  change of position, and grabbing the keyboard.
- Double clicks as such. A program counts them in ticks.
- Touch, pens and controllers. Touch and pens do not act as a mouse.
- Keyboard and mouse queries in a GPU system. The fields of `Mouse` are
  read there; a query is answered in an ordinary system and copied into a
  resource field.
- Recording what a person does at the desktop as a file of commands.

## Design

### The language

Ten names become built in and cannot be declared by a program: `Key`,
`MouseButton`, `Keyboard`, `Mouse`, `KeyPressed`, `KeyReleased`,
`MousePressed`, `MouseReleased`, `WindowFocused` and `WindowUnfocused`.
Nothing changes in the grammar. The reference gains a section,
"Keyboard and mouse", after "The window".

A program that names `Window`, `WindowResized`, `WindowClosed`,
`Keyboard`, `Mouse` or one of the six new events **has a window**, with
everything this section and "The window" declare. A program that names
`Key` or `MouseButton` alone gets those enums and nothing else, as `Color`
alone gives the struct. A name counts where it stands for the built-in: an
enum variant spelled the same, declared in an `enum` or written after
`::`, does not count.

**`Key`** is an enum with a variant for each key below. A variant names a
**position** on the keyboard: the key that is there on a US keyboard,
spelled as the W3C's `KeyboardEvent.code` spells it, without the `Key`
before a letter. `Key::W` is the key above `Key::S`, whatever the layout
prints on it: on a French keyboard it is the key that types Z, and WASD
lies under the same fingers on every layout.

| Group | Variants |
| ----- | -------- |
| Letters | `A` to `Z` |
| Digits | `Digit0` to `Digit9` |
| Punctuation | `Backquote`, `Minus`, `Equal`, `BracketLeft`, `BracketRight`, `Backslash`, `Semicolon`, `Quote`, `Comma`, `Period`, `Slash`, `IntlBackslash` |
| Whitespace and editing | `Space`, `Enter`, `Tab`, `Backspace`, `Escape`, `Insert`, `Delete` |
| Modifiers and locks | `ShiftLeft`, `ShiftRight`, `ControlLeft`, `ControlRight`, `AltLeft`, `AltRight`, `MetaLeft`, `MetaRight`, `CapsLock`, `NumLock`, `ScrollLock` |
| Navigation | `ArrowUp`, `ArrowDown`, `ArrowLeft`, `ArrowRight`, `Home`, `End`, `PageUp`, `PageDown` |
| Function | `F1` to `F12`, `PrintScreen`, `Pause`, `ContextMenu` |
| Numeric keypad | `Numpad0` to `Numpad9`, `NumpadAdd`, `NumpadSubtract`, `NumpadMultiply`, `NumpadDivide`, `NumpadDecimal`, `NumpadEnter` |

That is 105 keys, those of a full-size ISO keyboard. The enum declares
them in the order of their USB usage numbers: `A` to `Z`, `Digit1` to
`Digit9`, `Digit0`, `Enter`, `Escape`, and so on to `MetaRight`. A key the
table does not have never reaches a program. Modifiers are keys like any
other: Shift held is `Keyboard.down?(Key::ShiftLeft) ||
Keyboard.down?(Key::ShiftRight)`.

**`MouseButton`** is an enum: `Left`, `Middle`, `Right`, `Back`, `Forward`.

**`Keyboard`** is a resource with one instance and no fields. Its members
are three queries:

| Member | Type | Meaning |
| ------ | ---- | ------- |
| `Keyboard.down?(key)` | `Bool` | The key was held when the tick began. |
| `Keyboard.pressed?(key)` | `Bool` | The key went down at least once between the last tick's beginning and this one's. |
| `Keyboard.released?(key)` | `Bool` | The key came up at least once between the last tick's beginning and this one's. |

**`Mouse`** is a resource with one instance:

| Member | Type | Meaning |
| ------ | ---- | ------- |
| `Mouse.x` | `Float` | Points from the left edge of the window. Read-only. |
| `Mouse.y` | `Float` | Points from the top edge of the window. Read-only. |
| `Mouse.inside?` | `Bool` | The pointer is over the window. Read-only. |
| `Mouse.wheel_x` | `Float` | Notches scrolled to the right between the last tick's beginning and this one's. Read-only. |
| `Mouse.wheel_y` | `Float` | Notches scrolled up between the last tick's beginning and this one's. Read-only. |
| `Mouse.down?(button)` | `Bool` | The button was held when the tick began. |
| `Mouse.pressed?(button)` | `Bool` | The button went down at least once between the last tick's beginning and this one's. |
| `Mouse.released?(button)` | `Bool` | The button came up at least once between the last tick's beginning and this one's. |

- The position is in the units of `Window.width` and `Window.height`, with
  a fraction where the desktop gives one: on a display with a scale of 1.5
  a point is a pixel and a half, and the pointer can be between two
  points.
- `Mouse.inside?` is true when the last thing the pointer did was move,
  not leave, and its position is within the window's size as it is when
  the tick begins: `0 <= x < width` and `0 <= y < height`.
- When the pointer leaves the window, the position keeps the last value
  the desktop gave, at the window's edge. While a button is held the
  desktop goes on reporting the pointer outside the window, so the
  position can be negative or larger than the window, and `inside?` is
  false.
- The wheel's notches are those of a wheel that clicks; a smooth wheel or
  a touchpad gives fractions of one. The sign is the one the desktop gives
  after the person's own setting: `wheel_y` is positive when the person
  asks to see what is above, whichever movement of the hand their setting
  makes that.

`Window` gains a field:

| Member | Type | Meaning |
| ------ | ---- | ------- |
| `Window.focused?` | `Bool` | Keys go to the window. Read-only. |

**Events.** What a person did between the last tick's beginning and this
one's reaches the program when the tick begins, before the first system
runs:

```ruby
event KeyPressed
  key: Key
end

event KeyReleased
  key: Key
end

event MousePressed
  button: MouseButton
  x: Float
  y: Float
end

event MouseReleased
  button: MouseButton
  x: Float
  y: Float
end

event WindowFocused
end

event WindowUnfocused
end
```

- The events of one type are in the order they happened. Events of
  different types are received by different `on` blocks, so their order
  relative to each other is that of the program's `on` blocks.
- A key or a button pressed and released between two ticks gives both
  events, and in that tick `pressed?` and `released?` are true and `down?`
  is false. Nothing a person does is lost, however short.
- The position of `MousePressed` and `MouseReleased` is where the pointer
  was at that moment; `Mouse.x` and `Mouse.y` are where it was last.
- A key held down gives one `KeyPressed`, however long it is held. The
  presses a desktop repeats while a key is held do not reach a program.
- **Focus.** When the window loses keyboard focus every key and button
  that is held is released: their `KeyReleased` and `MouseReleased` are
  among the tick's events, `released?` is true for them, `WindowUnfocused`
  arrives and `focused?` is false. When the focus comes back,
  `WindowFocused` arrives; a key that is still held is not down until it
  is pressed again, except that the desktop may report a held Shift,
  Control, Alt or Meta key as pressed at that moment. The pointer goes on
  moving over a window without focus.
- **Opening.** A window opens without the focus, with the pointer outside
  it at 0, 0 and nothing held: until a tick begins, `Window.focused?` and
  `Mouse.inside?` are false. A headless or driven window is given the
  focus as the first thing that happens to it, so its first tick begins
  with `WindowFocused` and `focused?` true. A window on the desktop that
  is not driven gets the focus when the desktop gives it.
- **Closing.** In the tick that begins with `WindowClosed`, everything
  held is released, as on a loss of focus, and `focused?` and `inside?`
  are false; no `WindowUnfocused` is emitted. `Mouse.x` and `Mouse.y` keep
  their last values until a window opens again.
- At the beginning of every tick, whether a window is open or not, what
  was pressed and released and the wheel are cleared. Before a window is
  opened, and in the ticks after the one that begins with `WindowClosed`,
  nothing is held, pressed or released, and the wheel is 0.
- The fields and the queries change when a tick begins and at no other
  time: during a tick they do not change.
- Emitting one of these events is a compile error.

**Where they can be read.** The queries and the fields are read wherever a
resource field is, in an independent `each` on several cores too. The
fields of `Mouse` and `Window.focused?` are read in a GPU system; a query
there is a compile error.

**Determinism.** A window brings its sizes, its closing, its focus and the
keyboard and the mouse into a program, and all of them enter only when a
tick begins. A headless or driven run takes them from its commands alone,
so it prints the same output on every machine and for every run.
Everything the desktop can tell a program of the keyboard and the mouse is
something a command can say.

### Commands

`MIDNIGHT_DRIVE` gains these commands. A **driven** program takes its
keyboard, mouse and focus from its commands alone, with a window on the
desktop or headless: keys, buttons, motion, the wheel and changes of focus
at the desktop do not reach it. Resizes and closes at the desktop reach a
driven window on the desktop as in 0.6. A headless program without
commands has no input.

| Command | Meaning |
| ------- | ------- |
| `key down K` | The key `K` goes down. |
| `key up K` | The key `K` comes up. |
| `key K` | The key `K` goes down and comes up. |
| `mouse move X Y` | The pointer moves to `X`, `Y` in points. |
| `mouse down B` | The button `B` goes down where the pointer is. |
| `mouse up B` | The button `B` comes up where the pointer is. |
| `mouse B` | The button `B` goes down and comes up where the pointer is. |
| `mouse leave` | The pointer leaves the window and keeps its position. Buttons stay as they are. |
| `scroll DX DY` | The wheel turns `DX` notches to the right and `DY` up. |
| `blur` | The window loses keyboard focus. |
| `focus` | The window gets keyboard focus. |

- `K` is the name of a `Key` variant and `B` that of a `MouseButton`
  variant, as the source writes them after `::`.
- A number is an optional `-`, one or more digits, and optionally a point
  followed by one to eight digits. It is read as its digits, as a whole
  number, divided by the power of ten of its places, which gives the
  `Float` nearest to it on every machine; `-0` and its like are 0.0. `X`
  and `Y` are from -8192 to 8192, `DX` and `DY` from -1000 to 1000.
  `scroll 0 0` is allowed and changes nothing.
- A command is carried out, and answered with `midnight: drive: ` and the
  command as it was written, when it is read, as `resize` is in 0.6; what
  it does reaches the program when the next tick begins, in the order of
  the file.
- A command is judged against the state the commands before it leave, in
  the order of the file: after `key down A`, `key up A` is right before
  the same tick. A headless or driven window begins as "Opening" says,
  with the focus already given; `blur` and `close` leave nothing held for
  the commands after them. The commands carried out after the program
  ends are judged the same way.
- These stop the program with status 1 and a line that names the file and
  the line number, as a wrong command does in 0.6, and in this order when
  more than one applies:
  - a name that is not a key or a button, or a number that is not as
    above or out of range;
  - a `key` command while the window does not have the focus;
  - `key down` of a key that is down, `key up` of a key that is up, and
    the same for buttons;
  - `mouse move` outside the window while no button is held (the desktop
    reports such a pointer as having left: the hint names `mouse leave`);
  - `mouse down` or `mouse B` while the pointer is not over the window and
    no button is held;
  - `mouse leave` while the pointer is not over the window;
  - `blur` without the focus and `focus` with it.

### The compiler

- The built-in declarations are Midnight source, as in 0.6: the enums, the
  events and `Mouse` are ordinary declarations to everything after the
  checker. `Mouse` has a field the source cannot name, `inside`, which
  `Mouse.inside?` reads, and `Window` gains one, `focused`, which
  `Window.focused?` reads. `Keyboard` exists in the checker alone and
  emits nothing.
- The checker resolves `Keyboard.down?(…)`, `pressed?` and `released?`,
  and those of `Mouse`. Each is an expression of its own kind in the typed
  program, with the key or button as its argument, which is any
  expression of type `Key` or `MouseButton`.
- Diagnostics:
  - a query with the wrong number or type of arguments, the label naming
    `Key` or `MouseButton`, and a query without its arguments;
  - `Mouse.inside`, `Window.focused`, and an unknown member of `Keyboard`
    or `Mouse`, with the right spelling in the hint where there is one;
  - `Keyboard` or `Mouse` used as a value;
  - assigning to a field of `Mouse` or to `Window.focused?`;
  - a query in a GPU system, or in a function a GPU system calls, with the
    note that names the system and the call as other GPU diagnostics do,
    and a hint to answer it in an ordinary system and copy it into a
    resource field;
  - `emit` of a built-in event, and a declaration that takes a built-in
    name.
- The effect analysis gives a query no reason of its own to keep a loop on
  one core; its argument is judged as any expression.
- A program without a window is built as in 0.6: the generated C is the
  same text. A program with a window from 0.6 prints what it printed; its
  generated C changes with the new built-in declarations.

### The runtime

**`midnight_rt.h`** declares, in "The window":

- the layouts of `Mouse`, the four input events and `Window` with
  `focused` after `open`, which generated code asserts as it asserts
  those of 0.6;
- the names of the variants of `Key` and `MouseButton` in their order,
  and their numbers, which generated code asserts; a unit test holds the
  built-in declarations in `src/source.rs` to that list;
- `MnWindowProgram` with the address of `Mouse` and the numbers of the six
  new events;
- the three queries of each resource, under "Several threads" too: they
  read state that does not change during a tick, and code in a piece may
  call them.

**In `midnight_rt.c`**, under `MN_WINDOW`, with libc alone:

- **What happened**: an ordered list of happenings since the last tick
  began, each a key or a button going down or up, a move, a leave, a turn
  of the wheel, a loss or a gain of focus. The happening is
  `midnight_rt.c`'s own. Commands append to it, and so does the desktop,
  through a function of `midnight_rt.c` that `midnight_window.h` declares
  and `mn_window_poll` calls for each one. A happening from the desktop is
  one a command can state.
- **The state**: the keys and buttons held, those pressed and those
  released since the last tick began, the position, whether the last
  thing the pointer did was a leave, the wheel's sums and the focus.
- **When a tick begins**, whether a window is open or not, the pressed
  and released sets and the wheel are cleared. Then, with a window open,
  after the sizes: each happening is applied in order and emits its event;
  a loss of focus or a close first releases what is held, keys in the
  order of `Key`, then buttons in the order of `MouseButton`. A press of a
  key that is held, or a release of one that is not, is dropped. The
  `Mouse` fields and `Window.focused` are assigned, then `WindowClosed` is
  emitted as in 0.6.
- **`MIDNIGHT_WINDOW=info`** also prints `midnight: window: focused` each
  time a shown window gains the focus, `midnight: window: pointer inside`
  each time the pointer enters it, and, for a driven window, `midnight:
  window: input from commands` when it opens.

**In `midnight_window.c`**:

- SDL's key events become happenings by their **scancode**, the key's
  position, and never by key code, modifier bits, `raw` or timestamp.
  Repeated presses are dropped. A scancode without a `Key` is dropped. The
  releases SDL makes itself at a loss of focus come in the order of the
  scancodes, which is the order of `Key`.
- Buttons 1 to 5 of SDL are `Left`, `Middle`, `Right`, `Back` and
  `Forward`; others are dropped. Touch and pens are told not to act as a
  mouse. The click that gives the window the focus reaches the program.
- Positions are taken as SDL gives them, in the units of
  `SDL_GetWindowSize`, which are those of `Window.width` and
  `Window.height` on every system. SDL's leave is a leave, and its enter is
  the move that comes with it.
- The wheel's values are summed as SDL gives them, in notches, after the
  person's setting.
- SDL's focus events become happenings.
- Text input is never started. Keyboard and mouse state is never read from
  SDL's own state functions: it changes when SDL takes events from the
  desktop, not when the program takes them from SDL, so a press and
  release inside one tick would not show.
- With `MIDNIGHT_DRIVE` set, SDL's key, button, motion, wheel, enter, leave
  and focus events are taken and dropped.

### The flake

- `tests/window/input.c`, built for the window tests with libwayland's
  client library, xkbcommon, and the code `wayland-scanner` makes from the
  virtual keyboard protocol in wtype's source and the virtual pointer
  protocol in `wlr-protocols`, all from the pinned nixpkgs. It is a
  Wayland client that gives the compositor a keyboard and a pointer and
  acts on commands read from its standard input: a key down or up by its
  evdev code, a button down or up, a turn of the wheel, and a wait for the
  compositor. Moves are made with sway's own cursor commands, which act on
  the client's pointer.
  - It connects only to the socket whose absolute path is its one
    argument, opening the socket itself, and never reads
    `WAYLAND_DISPLAY`, `WAYLAND_SOCKET` or `XDG_RUNTIME_DIR`.
  - It binds every `wl_output` at version 4 and sends nothing until each
    has announced a name, `HEADLESS-` and a number. A compositor that
    offers an older `wl_output`, an output that announces no name, any
    other name, or an output announced later stops it with status 1 and a
    line that says so. A desktop's outputs are real displays, so a run
    pointed at one sends nothing.
  - A command it does not know, or a number out of range, stops it with
    status 1.
  - Its keymap is the US layout of xkb's `evdev` rules, and it sends the
    modifiers that keymap gives, as a compositor does for a real keyboard.
  - It is built with `-Wall -Wextra -Werror`.
  - It runs only while `window_input` runs and is stopped after it: while
    it is connected, every window in the compositor gets the focus and the
    pointer.

### Measured in planning

On a Ryzen 9 9950X3D with an RTX 5090, NixOS 26.05, SDL 3.4.10 and sway
1.12, with a hand-written program that logs SDL's events, polled every
16.7 ms as ticks are. Every window and every source of input was inside a
headless sway on a socket of its own; nothing created a kernel input
device.

- **A sway with no input device delivers nothing.** Its seat has no
  keyboard and no pointer, the window never gets keyboard focus, and
  sway's own cursor commands answer success and reach no one.
- **A client that holds a virtual keyboard and pointer, connected before
  the window opens, is exact.** Sending once the program had the focus and
  the pointer, keys, moves, buttons and the wheel arrived exactly and in
  order in 50 runs of 50, and sway's cursor commands moved its pointer
  exactly in 50 of 50. A device created for each action loses input unless
  it waits for the program to take it. A virtual keyboard's modifiers come
  only from its client: without them Shift and A typed a small a.
- **Input must wait for the program.** The compositor reports the window
  focused before it is where a click goes; sent once the program said it
  had the focus and the pointer, 50 of 50 arrived.
- **Scancodes are positions.** With a French keymap the key in the US A's
  place gave the scancode for A and the key code for Q. Key codes follow
  the layout and are not shifted.
- **Repeats.** On Wayland SDL makes the repeated presses of a held key
  itself, from the compositor's rate, while it takes events; on X11 the
  server makes them. Both are marked as repeats.
- **Nothing between two polls is lost or merged.** A press and release
  inside one poll arrived as two events, in order, while SDL's keyboard
  state after the poll said the key was up. Ten moves in one poll were
  ten events.
- **Focus.** On a loss of focus SDL sent a release it made itself for each
  held key, then the loss; a held button kept the window. When the focus
  came back, held modifiers came back as presses and other held keys were
  never reported, nor their release.
- **Positions are points with fractions.** At scales 1 and 1.5 the
  pointer's position was in points, with fractions, and a window of 640 by
  360 points had 960 by 540 pixels at 1.5. Dragging out of the window
  gave positions outside it; leaving without a button gave a leave and no
  more positions. Every entry came with a move to where the pointer was.
- **The wheel.** One notch of a wheel was 1.0 in SDL's units, positive
  away from the person.
- **X11 through Xwayland** gave the same scancodes, whole-number
  positions, and lost the first turn of the wheel in a window's life; two
  taps of one key in the same millisecond became a press and a repeat.

### Tests

- `tests/run`: programs with `NAME.drive` files, run headless as in 0.6.
  They cover `down?`, `pressed?` and `released?` across ticks; a tap
  between two ticks; two taps of a key between two ticks; the order of
  `KeyPressed` and `KeyReleased` across several keys, and of
  `MousePressed` and `MouseReleased` across several buttons, in one tick;
  the position in the events and in the fields; `inside?` after a move,
  after `mouse leave`, during a drag past the edge, and after a `resize`
  that leaves the pointer outside; the wheel summed over several commands
  and cleared the tick after; the first tick's `WindowFocused`; `blur`
  releasing everything held, `focus`, and a key held across a loss of
  focus; a close releasing what is held, and the state in the ticks after
  it; the state before a window opens; a window opened again;
  `Mouse.x` read in a GPU system; a query in an `each` that runs on
  several cores, with a `.split` file.
- `tests/fail`: every diagnostic above.
- `drive_errors`: every new way a command can be wrong, and the order in
  which they are reported.
- `tests/loops`: a loop that reads the queries is independent.
- An enum variant named `Mouse` leaves the generated C of a program as it
  was without it.
- Window mode, inside the compositor of `tests/window/compositor.sh`:
  - `window_input` starts `tests/window/input.c` on the compositor's
    socket and then, without `MIDNIGHT_DRIVE` and with
    `MIDNIGHT_WINDOW=info`, a program with a window that prints each input
    and focus event it receives, with its fields, and nothing that depends
    on which tick an input falls into. The test waits for the `info`
    lines that say the window has the focus and the pointer, then sends
    the input of the program's `.drive` file one command at a time, each
    once the program has printed the event of the one before: `key` and
    `mouse` commands through `input.c`, `mouse move` through sway's cursor
    commands, `blur` as a move to an empty workspace and `focus` as the
    way back. The `.drive` file also says what the desktop does with them:
    `mouse leave` after `blur` and a `mouse move` after `focus`. `tick`
    lines are skipped. The table from `Key` names to evdev codes is in the
    suite, beside the test. The program's output must be that of the
    headless run of the `.drive` file. It covers letters, digits, a
    modifier, an arrow, a key of the keypad, each button, moves to
    multiples of 1/256 point at a scale of 1.5 at least a point inside
    the window's edges, the wheel, and a loss and a gain of focus. It runs
    its program 20 times in each build mode.
  - `input_client_refuses_a_desktop`: inside the test compositor, a second
    sway with the Wayland backend, whose output is `WL-1`; `input.c`
    pointed at it stops with status 1 and its line, and that sway's
    `swaymsg -t get_inputs` lists no virtual device.
- `tests/COVERAGE.md` maps "Keyboard and mouse" to these.
- `examples/input.12am`, with `examples/input.drive`: the background
  follows the pointer, a click prints where it was, Escape ends the
  program.

### Slices

1. **The language, headless.** The built-in names, the checker, the typed
   program, the emitter, the contract in `midnight_rt.h`, the state and the
   happenings in the runtime, the events, focus, the commands, and their
   tests. No SDL.
2. **The desktop.** Scancodes, buttons, positions, the wheel and focus
   from SDL; dropping repeats, touch and pens; the `info` lines;
   `tests/window/input.c` and its refusal test, then `window_input`.
3. **Documents.**
   - The reference 0.7: "Keyboard and mouse"; "Not in 0.7", where the
     window's list loses the keyboard and the mouse and gains the
     non-goals above; the queries in what an independent `each` may read;
     the window's "Determinism", which now names focus and input.
   - The repository's `CLAUDE.md`: the compositor gets input devices from
     `input.c` during `window_input` and from nothing else; `window_input`
     and the refusal test in the window-mode list.
   - README, `tests/COVERAGE.md`, the example, the Outcome.

## Acceptance criteria

All run on x86_64-linux, on the development machine.

1. `nix flake check` passes, and `nix run .#gpu-test` and
   `nix run .#window-test` pass on the hardware device.
2. Every `.out`, `.err`, `.plan`, `.loops`, `.split`, `.stats` and
   `.frames` file that existed in 0.6.1 is unchanged, and for every program
   of `tests/`, `examples/` and `bench/` that existed in 0.6.1 without a
   window `midnight emit-c` and the generated shaders are byte for byte
   what 0.6.1 gives.
3. Every program of `tests/run` and `examples` that reads input prints its
   expected output headless, in release and debug, with no device and no
   display variable set, and the same output on every run, and on several
   threads as `thread_programs` runs them.
4. Every command, diagnostic and command error of this spec has a test
   that passes.
5. A debug build of every program that reads input ends with no sanitizer
   report.
6. No test, check or app of this spec opens a window on the desktop it is
   run from, and none sends input to it.
7. `window_input` passes in the build sandbox and on the hardware device,
   in release and debug, all 20 runs of each.
8. `examples/input.12am`, started by hand on the development machine's
   desktop in release and in debug: keys and clicks reach it, a key held
   while the focus moves to another window is released, Escape ends it,
   the debug build with no sanitizer report.
9. The reference says 0.7 and `midnight --version` 0.7.0, every command in
   the README works as written, and `tests/COVERAGE.md` maps every
   construct, command, diagnostic and command error of "Keyboard and
   mouse" to a test.

## Decisions

- **State and events.** The state answers "is it held", which movement
  needs; the events keep every press and the order of each kind, which a
  sequence of keys needs. A resource and events are the two shapes the
  language already has for what comes from outside, and the window uses
  both.
- **Pressed and released are sets.** A tick that saw a key go down and up
  says both, however short the tap.
- **Keys are positions.** A position means the same key on every layout,
  so controls work on every keyboard and a file of commands replays the
  same everywhere. The W3C's names are a published, layout-free list, and
  each key of the table is one SDL scancode. What a key types belongs to
  text input.
- **`Key` in the order of USB usage numbers.** The order releases come in
  is then the same from the desktop and from commands.
- **No repeats.** How fast a held key repeats is a setting of each desktop;
  a program that wants a repeat counts ticks, the same on every machine.
- **Points, as `Float`.** The units of the window's size, with the
  precision a scaled display gives.
- **The wheel as sums.** A tick wants how far the wheel turned.
- **The position passes through.** A drag past the edge of the window goes
  on, and `inside?` says where the pointer is.
- **Focus releases.** A key let go while another window had the focus
  would otherwise stay down for ever. The runtime keeps the held state
  itself, so the release is the same whatever SDL does on a system.
- **Commands say only what a desktop can.** A file of commands that passes
  headless then describes input a real compositor can give, and the
  window-mode test can send it.
- **A driven program takes its input from its commands.** A run with a
  window on the desktop then prints exactly what the headless run prints.
- **The focusing click counts.** Nothing a person does is lost, the click
  that brings the window forward included.
- **A test client that can only reach a headless compositor.** Input from
  a real compositor is the one way to check that SDL's scancodes, buttons,
  positions and focus become what the commands say. A client that holds
  its devices and waits for the program is exact, and one that opens the
  socket it is given and refuses any output but a headless one cannot
  send input to a desktop, whatever the environment it is started in.
- **No queries on the GPU.** The fields of `Mouse` are plain resource
  fields and go to a shader as they are; a query would need a set of keys
  in the shader's uniforms, for an answer an ordinary system gives in one
  line.

## Outcome

Implemented, with the differences listed under "Differences from the
design". Eight of the nine criteria pass on x86_64-linux: a Ryzen 9
9950X3D beside an RTX 5090, `llvmpipe` from Mesa 26.1.8, SDL 3.4.10,
`wgpu-native` 27.0.4.0, sway 1.12, clang 21.1.8. Criterion 8 is a run
by hand at the development machine's desktop and has not been made yet.

### Acceptance criteria

| # | Result |
| - | ------ |
| 1 | `nix flake check` passes, with `checks.window` (the smoke program and the seven window tests in the sandbox, `window_input` among them) and `checks.tsan`. `nix run .#gpu-test` and `nix run .#window-test` pass on the RTX 5090. |
| 2 | Against `65bd4ec` (0.6.1) no `.out`, `.err`, `.plan`, `.loops`, `.split`, `.stats` or `.frames` file has changed; the new ones are added. The 447 programs of `tests/`, `examples/` and `bench/` that 0.6.1 has without a window give the same `emit-c` bytes, shaders included. |
| 3 | The seven programs that read input (`input_keys`, `input_mouse`, `input_focus`, `input_gpu`, `input_cores`, `input_events`, `examples/input`) print their expected output headless in both build modes with `MIDNIGHT_GPU=off`, no display variable and an empty runtime directory: in the suite, and 10 runs of each in each mode, all the same. `input_gpu` and `input_cores` run in `thread_programs` under the six thread settings. |
| 4 | Every command, diagnostic and command error has a test: the `tests/run` programs above, seven `tests/fail/input_*` programs, `drive_errors` (37 cases of the keyboard and the mouse), `loops/cores_input`, `a_variant_named_like_a_built_in_gives_no_window`. `tests/COVERAGE.md` lists them under "Keyboard and mouse". |
| 5 | No sanitizer report from any debug run of a program that reads input, in the suite, in the runs of criterion 3 and in `window_input`. |
| 6 | Every window test runs inside `tests/window/compositor.sh`. `tests/window/input.c` opens only the socket it is given and sends nothing to a compositor with an output not named `HEADLESS-`; `input_client_refuses_a_desktop` holds it to that. |
| 7 | `window_input` passes 20 runs in each build mode in the sandbox (sway with pixman, `llvmpipe`) and on the 5090. |
| 8 | Not yet run: `examples/input.12am` at the desktop, release and debug. |
| 9 | The reference says 0.7 and `midnight --version` prints `midnight 0.7.0`. The README's commands were run as written: those that write files in a scratch copy, the two windows inside `tests/window/compositor.sh`, closed through sway. `tests/COVERAGE.md` has "Keyboard and mouse". |

### Differences from the design

- **Choices the design left open.** A query is an expression kind of its
  own in the typed program (`ExprKind::Input`), and generated C calls
  `mn_key_down`, `mn_key_pressed` and the like, which answer false for a
  number out of range. `midnight_rt.h` lists the keys and buttons as
  X-macros (`MN_KEYS`, `MN_MOUSE_BUTTONS`) with their USB usage and SDL
  numbers.
- **A driven window drops the desktop's input in `midnight_rt.c`**, in
  the functions `mn_window_poll` calls for each happening, not in
  `midnight_window.c`. The `info` lines `focused` and `pointer inside` are
  printed there too, for a shown window that is not driven.
- **`close` is a happening.** It takes its place among the happenings of
  the tick, and nothing after it in that tick reaches the program. A close
  from the desktop, and the one when the commands run out, come after all
  the tick's happenings.
- **A button's happening carries its position**, and sets the pointer's
  position when it is applied.
- **SDL hints.** `SDL_HINT_TOUCH_MOUSE_EVENTS` and
  `SDL_HINT_PEN_MOUSE_EVENTS` are 0, `SDL_HINT_MOUSE_FOCUS_CLICKTHROUGH`
  is 1, and events with the touch or pen mouse id are dropped as well.
- **`key down` with no key** is read as a tap of a key named `down`, and
  reported as `` `down` is not a key ``.
- **Two more tests of the input client.** `input_client_takes_its_commands`
  (window mode: a command for every key and button, turns of the wheel,
  and the commands it refuses) and `input_client_needs_an_absolute_socket`
  (whenever `MIDNIGHT_TEST_INPUT_CLIENT` is set).
- **`window_input` sends no `mouse leave`**: the compositor makes the
  leave when the window goes to another workspace. Its program prints
  only events and the wheel, so moves and leaves print nothing and which
  tick an input falls into cannot change its output. A fresh sway's cursor
  is at 100, 100, so `input_events.drive` begins with a move there.
- **`examples/input.12am` also prints keys and the focus**, so a key
  released by a loss of focus can be seen when it is run by hand.
