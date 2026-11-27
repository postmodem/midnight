# Running programs

A built program reads a few environment variables when it starts. Each is
a comma-separated list of words, and a word that is not in its table is
reported once, when the program starts. [Building programs](building.md)
says how programs are built and linked.

| Variable           | Read by                                         | Section |
| ------------------ | ----------------------------------------------- | ------- |
| `MIDNIGHT_GPU`     | programs with a GPU system or a window          | [The device](#the-device) |
| `MIDNIGHT_THREADS` | programs with a loop that can be split          | [Threads](#threads) |
| `MIDNIGHT_WINDOW`  | programs with a window                          | [A window, headless and driven](#a-window-headless-and-driven) |
| `MIDNIGHT_DRIVE`   | programs with a window                          | [A window, headless and driven](#a-window-headless-and-driven) |

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
that ticks.

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
nothing to do polls for 50 microseconds and then blocks. A program with no loop that can be split has no threads and does not read
the variable.

## A window, headless and driven

`MIDNIGHT_WINDOW=headless` runs a program with a window without a display:
the window exists in the program only, and ticks run as fast as they can.
`info` prints a line when a window opens. `MIDNIGHT_DRIVE` names a file or
a named pipe of commands that step the program, give it keyboard and mouse
input, capture its frames as PNG files and print them as text; each
command is answered on standard error with a line that begins
`midnight: drive: `.

```sh
MIDNIGHT_WINDOW=headless MIDNIGHT_DRIVE=examples/shapes.drive nix run . -- run examples/shapes.12am
```

The reference defines both variables and every command: see
[Without a display](language.md#without-a-display) and
[Commands for the keyboard and the mouse](language.md#commands-for-the-keyboard-and-the-mouse).
