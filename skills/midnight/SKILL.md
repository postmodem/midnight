---
name: midnight
description: How to work with Midnight, the compiled language for games and simulations whose source files end in .12am and whose compiler is `midnight`. Load it when writing, running, testing or debugging a Midnight program, and before looking at a running one with a file of commands or a session.
---

# Working with Midnight

`docs/language.md` is the definition of the language: read the sections a
task touches, and trust it over memory of other languages. Midnight
borrows from Ruby where Ruby reads well, but it is its own language:
entities, components and systems, static types, and no `nil`. This guide
says how to work; the reference says what things mean.

## The loop

1. Write the program in a `.12am` file.
2. `midnight check FILE` until it is clean. Each error names the line and
   column and often the fix (``did you mean `Position`?``).
3. `midnight run FILE` and compare what it prints with what you expect.
   `--debug` adds the sanitizers and a check that nothing leaked.
4. `midnight loops FILE` says where each loop runs and what keeps one on
   one core; `midnight plan FILE` lists what moves between CPU and GPU.

```sh
midnight check examples/move.12am
midnight run examples/move.12am
midnight loops examples/bounce.12am
```

Every command takes `--format json` for JSON lines (`docs/json.md`).

## Looking at a running program

A program can be driven: it runs no tick that a command did not let run,
and between ticks it answers commands about its world. The commands are
in the reference's "Driving" (`tick`, `query`, `resource`, `quit`) and,
for a window, "Without a display" (`draws`, `capture`, `resize`, …) and
"Commands for the keyboard and the mouse". There are two ways to send
them, with the same commands and the same answers.

**A file of commands** when the run is a test, will be replayed, or you
already know what to look at. It is exact and repeatable, but a wrong
command ends the program, and looking again means running it again from
the start.

```sh
printf 'tick 2\nquery Position, Velocity\nquit\n' > look.drive
MIDNIGHT_DRIVE=look.drive midnight run examples/move.12am
```

**A session** when you are looking and deciding as you go. The program
stays where it is between your commands, a wrong command is refused and
changes nothing, and it ends when you stop it.

```sh
midnight session start examples/catch.12am
midnight drive 'tick 10' 'resource Score' 'query Star'
midnight drive 'key down ArrowLeft' 'tick 5' 'key up ArrowLeft' 'query Star'
midnight drive 'draws' 'capture catch.png'
midnight session stop
```

How to look well:

- **Send several commands to one `drive`.** Each call is a turn; the
  answers come back in order, each command's printed output first.
- **Read text before pixels.** `query`, `resource` and `draws` give exact
  numbers. `capture FILE` writes a PNG to read when the question is about
  the picture: what is on top, what a color looks like, whether a frame
  looks right. A capture is of the last frame only.
- **Step in small `tick N`.** A running `tick N` can't be interrupted, and
  a bounce or a crash between two looks is easy to step past.
- **Let refusals teach.** A refused command names what is wrong, and a
  name that does not exist is answered with the names the program has:
  `query`'s components and tags, `resource`'s resources.
- **A program waits at its end.** `query`, `resource`, `draws` and
  `capture` still answer; a `tick` says the program ended.
- **Stop what you start.** `midnight drive quit` or `midnight session
  stop` ends the program; `midnight session list` shows what is running.
  An idle session stops itself after an hour (`--idle MINUTES`).
- **Keep what you found.** The commands of a session, without the
  refused ones, make a `.drive` file that replays the same answers: turn
  a finding into a test that way.

Sessions run windows headless. `capture` needs a device: don't set
`MIDNIGHT_GPU=off` when you will want a frame. `midnight session start
--debug` builds the program with the debug checks.

## Choosing `strict` and the GPU

The reference decides these; read it before choosing:

- "GPU systems" says what a `gpu system` may contain and touch;
  "What a GPU system costs" when the GPU pays for itself.
- "Strict GPU systems", under **When to use it**, says when results must
  be the same on every device and when the default is right.
- `midnight loops` and `midnight plan` show where loops run and what
  moves, once the program compiles.

## Mistakes to avoid

- Guessing names. Read the program, or let a refused `query` list them.
  `query` takes the terms of an `each`, separated by commas:
  `query Ball, Velocity`, never `query Ball Velocity`.
- Resources are not components: `resource Score`, not `query Score`.
- A `.drive` file of a program that ticks forever needs `quit` at its
  end, or the program runs on once its commands run out.
- Restarting to recover from a wrong command in a session: it was
  refused, and nothing changed. Send the corrected command.
- Editing the source and driving the old session: a session holds the
  program as it was built. After an edit, `midnight session stop` and
  start it again; `midnight session start FILE && midnight drive …`
  in one call does both.
- Adding `puts` to a copy of the program to see its state: `query` and
  `resource` at the tick you care about show it without a rebuild.
- Capturing many ticks apart and guessing what happened between: step
  and `query` instead.
