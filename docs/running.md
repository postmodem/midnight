# Running programs

A built program reads a few environment variables when it starts. Each is
a comma-separated list of words, but `MIDNIGHT_FORMAT`, one word, and
`MIDNIGHT_DRIVE`, a file name or `-`; a word that is not in its table is
reported once, when the program starts. [Building programs](building.md)
says how programs are built and linked.

| Variable           | Read by                                         | Section |
| ------------------ | ----------------------------------------------- | ------- |
| `MIDNIGHT_GPU`     | programs with a GPU system or a window          | [The device](#the-device) |
| `MIDNIGHT_THREADS` | programs with a loop that can be split          | [Threads](#threads) |
| `MIDNIGHT_WINDOW`  | programs with a window                          | [A window, headless and driven](#a-window-headless-and-driven) |
| `MIDNIGHT_DRIVE`   | every program                                   | [Driving](#driving) |
| `MIDNIGHT_FORMAT`  | every program, before the others                | [JSON](#json) |

## The device

When it starts, a program with a GPU system picks the fastest hardware
adapter. `MIDNIGHT_GPU` changes that:

| Word       | Meaning                                                         |
| ---------- | --------------------------------------------------------------- |
| `off`      | Use no device: every GPU system runs its C functions, and a window shows nothing and captures nothing. |
| `software` | Accept an adapter that computes on the CPU (Mesa's `llvmpipe`) when there is no hardware one. |
| `info`     | Print one line to standard error naming the device in use, or `no device`; in a program with a strict GPU system, one line saying whether the device passed the strict check; and one line for each `each` of a GPU system that runs on the CPU, with the reason. |
| `stats`    | Print one line to standard error for each tick: what its systems and the top-level code after it move between CPU and GPU memory, and how often they hand work to the device and wait for it. |
| `rows`     | Move every row on its own, whatever it costs, where a program would otherwise move a whole array because the rows cost more. For tests. |
| `strict-cpu` | Run the blocks of strict GPU systems on the CPU, as when the device fails the strict check, and the others on the device. For tests. |

```sh
MIDNIGHT_GPU=info,stats nix run . -- run tests/plan/reader.12am
```

In a `stats` line one upload or download is one component array of one
table. Rows up and rows down are rows of component arrays moved on their
own. Handles, uniform words, error words and the copies a nested `each`
reads are not counted in these four. Submissions and waits count every
submission of work to the device and every wait for it. The first line is
`tick 0`. A line is printed when the next tick starts, or when the program
ends, so it counts what top-level code transfers after the tick, as
`midnight plan` does; the last line also counts the code after the loop
that ticks. A frame that is rendered counts one upload for each run of draws
made on the CPU that has something for the device: a run of shapes, with
or without text among them, or a run of text alone that shows a
character. Text adds no upload to a run that holds a shape.

```sh
MIDNIGHT_GPU=stats MIDNIGHT_WINDOW=headless MIDNIGHT_DRIVE=examples/flock.drive nix run . -- run examples/flock.12am
```

runs `examples/flock.12am`, where a GPU system moves 100,000 points and
draws them: after the first tick, which sends the points, nothing crosses
between CPU and GPU memory until `draws` brings the shapes back to print
them.

With no device, because none was found or `off` is set, every GPU system
runs on the CPU, its rows divided among the cores. A single `each` also
does when it needs more than the device offers, such as an array larger
than the largest buffer the device binds.

A program with a strict GPU system checks the device when it opens it: it
runs a fixed piece of strict code on the device and on the CPU and compares
the results bit for bit. When they differ, `info` prints `strict blocks run
on the CPU:` with the device and the first difference, and every strict
block runs on the CPU; the other GPU systems still use the device. When
they agree it prints `strict check passed`.

A window that is shown uses the device of the program's GPU systems, or
asks for one that can draw to it; a headless window opens the device at
its first capture. `software` lets a window draw with an adapter that
computes on the CPU, and `info` names the device when it is opened.

An error from the device after start-up prints `midnight: GPU error: …` and
ends the program with status 70. It is reported when the work is handed to
the device, which can be later in the tick than the block that caused it.
A failure of a window after it has opened prints `midnight: window error:
…` and ends the program with status 70 too.

## Threads

Two kinds of loop run on several threads: an `each` of a GPU system when it
runs on the CPU, and an ordinary `each` that the compiler finds
independent, as the reference defines it. Each time such a loop runs, the
runtime divides its rows into pieces and hands them to threads when the
loop has enough work, and runs it where it stands when it has little. The
program prints what it prints on one thread either way. `midnight loops`
prints one line for each loop that is not inside another: where it is,
what it stands in, its terms, and `gpu`, `cores` or `one core:` with the
first thing in the loop that keeps it on one core.

A program with such a loop uses one thread for each processor core it may
run on, and creates the threads the first time a loop is split.
`MIDNIGHT_THREADS` changes that:

| Word       | Meaning                                                         |
| ---------- | --------------------------------------------------------------- |
| `1` to `256` | The number of threads, the one that runs the rest of the program among them. With `1` no thread is created and no loop is split. |
| `small`    | Make every piece one row, split every loop of two rows or more that can be split, and let a thread take part for each piece, up to the number of threads. For tests. |
| `reverse`  | Have each thread run its share of the pieces from the last to the first. For tests. |
| `jitter`   | Have each thread yield at random, from a fixed seed, before each piece. For tests. |
| `awake`    | Never let a thread block: one with nothing to do keeps polling, and uses a core to do it. For a program whose split loops come more than a few milliseconds apart, where a thread that blocked returns on a slowed core. |
| `info`     | Print `midnight: threads: N`, the number of threads, to standard error when the program starts, and one line for each loop the first time it is split: `midnight: threads: FILE:LINE: R rows in P pieces on T threads`. |

```sh
MIDNIGHT_GPU=off MIDNIGHT_THREADS=4,info nix run . -- run tests/run/cores_gpu_tables.12am
```

No word changes what a program prints to standard output. A thread with
nothing to do polls for 50 microseconds and then blocks; with `awake` it
keeps polling while a driven program waits for its next command too. A program with no loop that can be split has no threads and does not read
the variable.

## Driving

`MIDNIGHT_DRIVE` names a file or a named pipe of commands that step any
program a tick at a time and print its entities and resources between
ticks; a program with a window takes more commands, which give it
keyboard and mouse input, capture its frames as PNG files and print them
as text. Each command is answered on standard error with a line that
begins `midnight: drive: `. A driven program runs no tick a command did
not let run, so a file of commands gives the same answers on every
machine.

```sh
printf 'tick 2
query Position, Velocity
tick
' > look.drive
MIDNIGHT_DRIVE=look.drive nix run . -- run examples/move.12am
```

`MIDNIGHT_DRIVE=-` reads the commands from standard input instead, and
puts the program in a **session**: a wrong command is refused and changes
nothing, a `ready` line says each time the program waits for a command,
the program waits at its end to be looked at, and the end of input ends
it. A file ending with `quit` ends a program that would run freely once
its commands ran out. [A session](language.md#a-session) has the rules.

## Sessions

A shell runs one command at a time, so something has to hold a program's
input between them. `midnight session start` builds a program and leaves
it waiting for commands in a session of its own, and `midnight drive`
sends it commands, one call at a time:

```sh
midnight session start examples/catch.12am
midnight drive 'key down ArrowLeft' 'tick 10' 'key up ArrowLeft'
midnight drive 'resource Score' 'capture now.png'
midnight drive 'query Ball' 'query Star'
midnight session stop
```

```
midnight: session: started `catch`
midnight: drive: key down ArrowLeft
midnight: drive: tick 10
midnight: drive: key up ArrowLeft
midnight: drive: resource Score: 10 ticks, Score(points: 0, missed: 0)
midnight: drive: capture now.png: 10 ticks, 320x200, crc 2edd6de1
midnight: drive: refused: query: `Ball` is not a component or tag of this program; it has Star
midnight: drive: query Star: 10 ticks, 1 row
midnight: drive:   Entity(0v1) Star(x: 131.0, y: 27.0)
midnight: session: `catch` stopped with status 0
```

```
midnight session start FILE [--name NAME] [--debug] [--format json] [--idle MINUTES]
midnight session stop [NAME] [--format json]
midnight session list [--format json]
midnight drive [--session NAME] [--format json] COMMAND...
midnight drive [--session NAME] [--format json] -
```

- **`session start`** builds `FILE` as `build` does, release unless
  `--debug`, and prints the diagnostics and exits with status 1 if it has
  errors. Otherwise the program starts headless, with `MIDNIGHT_DRIVE=-`,
  in the directory `session start` was run in, and with the caller's
  variables but `LD_LIBRARY_PATH`; `info` in the caller's
  `MIDNIGHT_WINDOW` is kept. `session start` prints what the program
  writes before it waits for its first command and returns. A program
  that ends first, or writes no `ready` within 60 seconds, leaves no
  session, and `session start` exits with status 1.
- A session is named `NAME`, or after the file without `.12am`: 1 to 32
  ASCII letters, digits, `-` and `_`. `--idle MINUTES` (60 unless given,
  from 1 to 1440) stops a session that has had no command for that long.
  `MIDNIGHT_THREADS` with `awake` is refused, since every thread would
  stay busy while the session waits.
- **`drive`** sends each argument as one command, or with `-` each line
  of standard input, and prints what each brings: the program's standard
  output first, then its lines on standard error. It exits with status 0
  when every command was carried out, `quit` included, and 1 when one was
  refused (the commands after it are still sent) or when the program
  stopped by anything but `quit`. When the program stops, the commands
  after it are not sent, and `drive` prints ``midnight: session: `NAME`
  stopped with status N``. Two `drive` commands to one session take turns.
- **`session stop`** closes the program's input, which ends it as `quit`
  does once a running `tick` has run its ticks; after five seconds it is
  killed. **`session list`** prints a line per session: its name, the
  file, its ticks, and whether the program is `running` or has `ended`.
- `--session` and `NAME` may be left out when exactly one session runs.
- Sessions live in `midnight/sessions` under `$XDG_RUNTIME_DIR`, or in
  `midnight-UID/sessions` under the temporary directory, each directory
  the user's own with mode 0700; a session's socket path may be at most
  100 bytes. A session whose holder is gone is cleared by the next
  `session` or `drive` command. `capture` writes files relative to the
  directory `session start` was run in.
- A usage error (an unknown option, `--idle` out of range, `drive` with no
  command, a command with a line break) exits with status 2, and every
  other message with status 1.

## A window, headless and driven

`MIDNIGHT_WINDOW=headless` runs a program with a window without a display:
the window exists in the program only, and ticks run as fast as they can.
`info` prints a line when a window opens.

```sh
MIDNIGHT_WINDOW=headless MIDNIGHT_DRIVE=examples/shapes.drive nix run . -- run examples/shapes.12am
```

The reference defines the variables and every command: see
[Driving](language.md#driving),
[Without a display](language.md#without-a-display) and
[Commands for the keyboard and the mouse](language.md#commands-for-the-keyboard-and-the-mouse).

## JSON

`MIDNIGHT_FORMAT=json` makes every line a program writes to standard
error a JSON object: answers, `draws`, `stats`, `info` lines and errors.
Standard output does not change. `midnight run --format json` sets it for
the program it runs, and `check`, `build`, `plan` and `loops` take
`--format json` for their own output. [JSON output](json.md) describes
every line.

```sh
MIDNIGHT_FORMAT=json MIDNIGHT_DRIVE=look.drive nix run . -- run examples/move.12am
```
