# 0012 — Sessions

- **Status:** implemented
- **Depends on:** 0011

## Goal

A driven program takes its commands from a file. An agent that wants to
look, decide and look again adds commands to the file and runs it again
from the first tick: exact, since a driven program is deterministic, but
every look costs a build and a replay, the agent carries the whole
history, and one wrong command ends the program. An agent that guesses a
name (`query Ball` in a program that has `Star`) loses the world and
starts over, and nothing tells it what the names are. When the commands
run out the program runs freely, so a client that dies leaves a program
running, and a windowless `World.run` runs until something kills it.

After this spec a program can be held in a **session**: it stays where
it is between commands, a wrong command is refused and changes nothing,
and the program ends when its session does. On the command line:

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

The agent reads `now.png` as it reads any file. Underneath, a program in
a session reads its commands from standard input and writes a `ready`
line each time it waits for the next one, so any program that can write
to a pipe and read lines can hold a session too.

A short guide for agents, an Agent Skill in the repository, says when to
replay a file and when to hold a session, and how to look at a program.

### How this serves the principles

- **Every tool works from the command line with output an agent can
  use.** The session is `midnight`'s and the runtime's; anything built
  on it later (an MCP server, an editor) adds convenience, never a
  capability the command line lacks.
- **Observable and repeatable.** A session is the same program with the
  same commands. Its commands without the refused ones, up to its first
  `tick` after the program's end, make a `.drive` file that gives the
  same standard output and answers, `line` members aside.
- **Errors teach.** A refused command says what is wrong, and a name
  that does not exist is answered with the names that do.
- **One obvious way.** A session speaks the drive commands, nothing
  else: one vocabulary for files and sessions, the answers already
  documented. `query` and `draws` page the same way.
- **The documentation's size is a cost.** The reference gains a short
  section; the how-to lives in the skill, which an agent reads only when
  it works with Midnight.

## Scope

- `MIDNIGHT_DRIVE=-`: commands from standard input, as a session, with
  four rules of its own: refusals, `ready`, what happens at the
  program's end, and the end of input.
- `quit`, a command for every driven program.
- Errors of `query` and `resource` that name what the program has.
- `draws from N`, and `draws` listing at most 100 draws.
- `midnight session start`, `stop` and `list`, and `midnight drive`.
- `docs/json.md` at `format` 2; the reference at 0.11; `docs/running.md`.
- An Agent Skill in `skills/midnight/`.
- A spike: agents on drive tasks with files of commands, with the
  session and the skill, and with a throwaway MCP server over the
  session, read by hand.

## Non-goals

- An MCP server in the repository, and every other integration; the
  spike's server is throwaway.
- A shown window held by `midnight session`, which runs every program
  headless, and servicing a shown window while a program waits for a
  command.
- Interrupting a `tick N` that is running; commands are read between
  ticks only.
- `record`, captures sent over the pipe instead of written to a file,
  and commands that wait for a condition.
- Changing the world from commands; looking at the world after a
  run-time error.
- Editing help (diagnostics while typing, symbols, rename).
- A change to what a program prints on standard output for the same
  ticks and the same input, or to any answer of a file of commands but
  those of `draws` for a frame of more than 100 draws, the errors of
  `query` and `resource` that now name what the program has, and the
  error of `draws` with an argument; and a file of commands named `-`,
  which is now standard input.

## Design

### A session

`MIDNIGHT_DRIVE=-` reads the program's commands from standard input, and
the program is then **in a session**. Everything [Driving](../docs/language.md#driving)
says holds: the same commands, the same answers, the same tick rule (no
tick runs that a command did not let run). Four rules differ.

**1. A wrong command is refused.** Every command error that stops a
driven program with status 1 instead **refuses** the command in a
session: the program writes one line and goes on to the next command.
After `scale 4`, `resize 4000 10` is refused:

```
midnight: drive: refused: a frame of 16000x40 pixels is larger than 8192 by 8192
```

The line is `midnight: drive: refused: ` and the error's message as a
file reports it, without the file and line. A refused command changes
nothing in the program: no tick runs, no input is given, no window
changes size or scale, and the commands before and after it are carried
out as if it had not been sent. A `capture` refused because its file
cannot be written may have created or emptied that file, opened a
headless window's device (and printed its `info` line), and counted a
submission and a wait in the next `stats` line. This covers every error
of the commands of Driving, of the window and of the keyboard and the
mouse, `capture` with no device among them. A run-time error, an error
of the device or the window, and running out of memory still end the
program at once, as in a file.

**2. A `ready` line each time the program waits for a command.** Before
it reads the first line, and after each line it reads, the program
writes a `ready` line, unless it ends while carrying the line out: after
`quit`, a run-time error, an error of the device or the window, or
running out of memory no `ready` follows, and the end of standard error
is the end of the answer.

```
midnight: drive: ready: 10 ticks, 117 bytes out
midnight: drive: ready: 64 ticks, 2048 bytes out, the program has ended
```

It gives `Time.tick`, the number of bytes the program has written to
standard output since it started, and whether it has ended; `1 tick`
and `1 byte out` for one. Standard output is written out before it. A
line that is empty or a comment gets its `ready` too, so a client that
writes a line and reads up to the next `ready`, or to the end of
standard error, never waits for nothing. What the program writes while
it carries out a line (the program's output, the answers, `info` lines)
comes before that line's `ready`. A `stats` line is written where a
file has it: the line of a tick comes with the command that lets the
next tick run, or with `quit` or the end of input. A line written by
something other than Midnight comes with whichever line it arrives
during. The count of bytes lets a client that reads standard output on
a pipe of its own split it exactly between lines, and nothing a program
prints can be taken for a `ready` line, since the program prints only to
standard output.

**3. At its end, a program in a session waits.** When the program ends
by its last statement or by `exit`, a pending `tick` is answered as in a
file, and the program then carries out commands until `quit` or the end
of input: `query`, `resource`, `draws` and `capture` see the world as a
file's end commands do. A `tick` is answered `tick: the program ended
after 0 ticks` (`tick 5: …` for `tick 5`) and changes nothing. Its
`ready` lines say `the program has ended`.

**4. The end of input ends the program** as `quit` does, without
writing a line: at once when the program is waiting for a command, and
otherwise when the `tick` it is carrying out has run its ticks. A last
line without a line break is carried out first. A program whose
standard output or error has no reader left may end earlier, by
`SIGPIPE`. So a session does not outlive the process that holds its
input by more than the `tick` it was given.

A program given `MIDNIGHT_DRIVE=-` may show its window, as a named pipe
allows today, but the window is not serviced while the program waits
for a command: a resize or close at the desktop is seen when the next
tick begins. `midnight session` always runs headless. As for any driven
program, `MIDNIGHT_THREADS` with `awake` keeps every thread polling
while the program waits.

### `quit`

| Command | Meaning |
| ------- | ------- |
| `quit` | End the program where it stands. |

`quit` takes nothing; `quit` followed by anything is ``\`quit\` takes
nothing``. It is answered `midnight: drive: quit`, and the program ends
without carrying out another command. Before the program's end it ends
as `exit` would between ticks: standard output written out, an open
window closed without a tick, the last `stats` line written, status 0.
After the program has ended, `quit` lets the end go on as a file's end
commands do when they are done: an open window is closed, the last
`stats` line is written, a debug build checks its buffers, and the
status is the one the program ended with.

It is a command for every driven program: a file that ends with `quit`
stops a program that would otherwise run freely, such as a windowless
`World.run` that only `World.stop` ends.

### Errors that name what the program has

Two errors of `query` and `resource` gain the names a program has, in
every driven program, file or session:

```
query: `Bal` is not a component or tag of this program; it has Ball, Paddle, Frozen (tag)
resource: `Average` is not a resource of this program; it has Score, Stats
```

The names are in the order the program declares them, a required file's
at its `require`, components and tags together, a tag followed by
` (tag)`, all of them on the one line; `; it has none` when there are
none. The built-in resources are not listed. The other errors keep
their words.

### `draws` in pages

`draws` lists at most 100 draws, and `draws from N` lists from draw `N`
(`N` a whole number from 0 up), as `query` lists rows. Draws are
numbered from 0 among those `draws` lists, so the count in the first
line, `N` and the range are all over listed draws. The first line gives
the number of draws in the frame and, when the draws that follow are
not all of them, the first and last listed, or `none from N`:

```
midnight: drive: draws: 3 ticks, 320x200, 960 draws, 0 to 99
midnight: drive:   Window.background = Color(16, 16, 32, 255)
midnight: drive:   Draw.circle(…)
…
midnight: drive: draws from 900: 3 ticks, 320x200, 960 draws, 900 to 959
midnight: drive: draws from 2000: 3 ticks, 320x200, 960 draws, none from 2000
```

The background line is printed on every page. A frame of 100 draws or
fewer is printed as before. Each page brings the frame's draws on the
device back as `draws` does: a download for each span and one wait,
counted in `stats`. A page checks its words before it brings anything
back. Two new errors: ``draws: `from` takes a draw number from 0 up``,
and `draws` followed by anything else is ``\`draws\` takes nothing, or
`from` and a draw number``.

### `midnight session` and `midnight drive`

A session needs a process that holds the program's input between the
agent's commands. `midnight` provides one:

```
midnight session start FILE [--name NAME] [--debug] [--format json] [--idle MINUTES]
midnight session stop [NAME] [--format json]
midnight session list [--format json]
midnight drive [--session NAME] [--format json] COMMAND...
midnight drive [--session NAME] [--format json] -
```

**`session start`** builds `FILE` as `build` does, release unless
`--debug`, and on an error prints the diagnostics and exits with status
1. Otherwise it starts a holder in the background, in a process group
of its own, with standard input, output and error on `/dev/null`, so it
outlives the call that started it and a kill of that call's group does
not reach it. The holder starts the program with its three standard
streams as pipes to the holder, in the caller's working directory, with
`MIDNIGHT_DRIVE=-`, `MIDNIGHT_WINDOW=headless` (with `info` kept if the
caller's variable has it), `MIDNIGHT_FORMAT=json` with `--format json`
and no `MIDNIGHT_FORMAT` otherwise, `LD_LIBRARY_PATH` removed, and every
other variable as the caller has it. `session start` waits for the
program's first `ready`, prints what the program wrote before it
(standard output to standard output, the rest to standard error) and
`midnight: session: started \`catch\``, and exits with status 0.

- If the program ends before its first `ready`, `session start` prints
  what it wrote, exits with status 1 and leaves no session. If it has
  written no `ready` within 60 seconds, `session start` stops it, prints
  what it wrote and ``midnight: the program wrote no `ready` within 60
  seconds; a program reads its first command at its first tick, or at
  its end``, exits with status 1 and leaves no session.
- The session's name is `NAME`, or the file's name without `.12am`. A
  name is 1 to 32 ASCII letters, digits, `-` and `_`.
- `MIDNIGHT_THREADS` with `awake` is refused.
- `--idle MINUTES` (default 60, from 1 to 1440): a session with no
  command for that long is stopped. Idle time runs from the last
  `ready`; a command being carried out is not idle.

**`drive`** sends commands to a session and prints what they bring:
each argument is one command, or, with `-`, each line of standard input
is. For each command it writes the program's standard output for it
first, then the program's lines on standard error, without the `ready`
line, in either format. `--session` may be left out when exactly one
session runs.

- Status 0 when every command was carried out, `quit` included; 1 when
  one was refused (the commands after it are still sent) or when the
  program stopped by anything but `quit`.
- When the program stops during the commands, the commands after it are
  not sent, and `drive` writes the program's last lines and
  `midnight: session: \`catch\` stopped with status N`, `N` the
  program's status or 128 plus the signal that ended it. The session is
  then gone.
- An argument that holds a line break is a usage error: ``a command is
  one line; give each as its own argument``.
- The holder sends a command only after the `ready` of the one before,
  and sends none of a client's commands after that client has gone; it
  reads to the next `ready` and discards what it read, so the next
  `drive` starts clean. Two `drive` commands to one session take turns.

**`session stop`** closes the program's input (rule 4), without waiting
for a `drive` in progress, waits up to five seconds for the program to
end and kills it after that, prints what the program wrote last and
`midnight: session: \`catch\` stopped with status N`. `NAME` may be left
out as for `drive`.

**`session list`** prints one line per session, in order of name,
columns separated by two spaces and padded to the longest entry, `file`
as `session start` was given it, and nothing, with status 0, when none
runs; `1 tick` for one:

```
catch  examples/catch.12am  10 ticks  running
flock  examples/flock.12am  3 ticks   ended
```

**Where sessions live.** The holder keeps its socket, the built program
and nothing else in `midnight/sessions/NAME` under `$XDG_RUNTIME_DIR`,
or, when that is not set, in `midnight-UID/sessions/NAME` under the
temporary directory, `UID` the user's number. Each of these directories
is made with mode 0700; one that exists and is not the user's own with
mode 0700 is refused. A socket path longer than 100 bytes is refused at
`session start` (macOS holds 104). A session whose socket does not
answer is stale: `session list` does not list it, `drive` and `stop`
treat it as not running, and any of the four commands removes its
directory. When the program ends, the holder removes the directory and
ends. `capture` paths are the program's, relative to the working
directory `session start` was run in.

**Messages.** The messages of `session` and `drive` begin `midnight: `,
as the compiler's own do, and end with status 1; a usage error (an
unknown option, `--idle` out of range, `drive` with no command, a
command with a line break) has status 2, as for every command:

```
midnight: `my.game` can't name a session: a name is 1 to 32 ASCII letters, digits, `-` and `_`; give one with `--name`
midnight: session `catch` is already running; stop it, or give another `--name`
midnight: MIDNIGHT_THREADS has `awake`, which keeps every thread busy while a session waits
midnight: no session is running; start one with `midnight session start FILE`
midnight: 2 sessions are running (catch, flock); name one with `--session`
midnight: can't keep sessions in `DIR`: it must be the user's own, with mode 0700
midnight: the socket `PATH` would be longer than 100 bytes; set XDG_RUNTIME_DIR to a shorter directory
```

**Formats.** `session stop`, `session list` and `drive` take `--format
text|json` for their own lines; `session start`'s `--format` also sets
the program's. With `json` each writes `{"kind":"midnight",…}` first,
as the compiler does, and `session start` then passes on the program's
lines with the program's own first line, as `run --format json` does.
`drive` passes the program's lines through in the session's format.
Their messages are `session_error` lines.

The holder is `midnight` started again with a word of its own, which is
not in the usage text; the holder, its socket and what passes over it
are `midnight`'s own and not a contract. A `drive` and a holder of
different versions refuse each other. The contract is the commands
above and the program's standard input and output in a session.

### JSON

`format` becomes 2: a `draws` answer may now list some of the frame's
draws, which changes what its `draws` member means.

`ready` and `refused` join the table of "Programs":

| Kind | When | Members |
| ---- | ---- | ------- |
| `ready` | a program in a session waits for a command | `tick`, `output_bytes`, `ended` |
| `refused` | a command refused in a session | `command`, `line`, `message` |

A new section, "Sessions", has the lines of `midnight session` and
`midnight drive`:

| Kind | When | Members |
| ---- | ---- | ------- |
| `session` | a session started or stopped | `name`, `event` (`started` or `stopped`), `status` (optional: for `stopped`, the program's status, or 128 plus the signal) |
| `session_listed` | `midnight session list`, one per session | `name`, `file`, `tick`, `ended` |
| `session_error` | a message of `session` or `drive` | `message`: the text's line after `midnight: ` |

The `draws` answer gains `count` (the draws in the frame) and `from`
(the first listed) before `draws`, and `quit` is an `answer` with no
members of its own. `line` in `refused` and in every answer counts the
lines of standard input, empty lines and comments included.

### The skill

`skills/midnight/SKILL.md` is an Agent Skill: frontmatter with `name:
midnight` and a `description` that says when to load it (writing,
running, testing or debugging a Midnight program), and a body of at
most 150 lines. It is a guide, not a second reference: it says how to
work, and points into `docs/language.md` for what things mean. It
covers:

- the loop: write, `midnight check`, run, compare output;
- looking at a running program: a file of commands when the run is a
  test or will be replayed, a session when looking and deciding; `query`,
  `resource` and `draws` first, `capture` and reading the PNG to check
  pixels; small `tick N` steps; several commands to one `drive`; `quit`
  or `session stop` at the end; turning what a session found into a
  `.drive` file;
- choosing `strict` and the GPU, by pointing at the reference's guidance;
- the mistakes agents make most with Midnight, from the spike.

It names no eval task and no rule of grading. It lives in the repository
and is versioned with the reference: a change to the reference that
makes the skill wrong changes it in the same commit. The README says how
to use it: copy or link the folder into a project's `.claude/skills/`,
or `~/.claude/skills/` for every project. The repository's `CLAUDE.md`
names it in "What defines what".

### The spike

Before the Outcome, a spike, per the repository's rules: a few agent
sessions on rough tasks, read by hand. It informs and decides nothing.

- Tasks that need looking at a running program: a shape drawn in the
  wrong place, seen only in a frame; a run-time error at tick 300 whose
  cause is in the world a few ticks before; a constant to tune until a
  behavior holds.
- Three arms, at the same commit, run separately: the documentation and
  files of commands, with `midnight session` and `midnight drive` not
  allowed; the documentation, the session and the skill; the
  documentation, the skill and a throwaway MCP server over the session,
  kept with the planning notes and not in the repository, with no shell.
  Two models, one or two sessions a task and arm.
- Each agent has a directory and an `XDG_RUNTIME_DIR` of its own, the
  device allowed, and every session left running is stopped when its
  run ends. Slice 3 says whether this uses the evals' wrapper, and what
  in it changes (today it forces `MIDNIGHT_GPU=off`, kills each call's
  process group, and checks only arguments that name files).
- Read per session: turns, cost, refused commands, builds, replays,
  captures, and whether the task was done.
- What would inform: the skill's wording, from the mistakes seen; and
  the MCP question: a server is worth a spec if the MCP arm does the
  tasks it shares with the session arm in clearly fewer turns or at
  clearly less cost, or if in the session arm more than one session in
  four loses more than a third of its turns to holding the session
  (starting, restarting, finding it, stopping it). Every task done in the
  MCP arm must be doable in the session arm with the same commands.
- Transcripts stay out of the repository; the Outcome summarizes.

### The runtime

- `drive` gains `session`, set when `MIDNIGHT_DRIVE` is `-`, which reads
  the C `stdin` stream.
- Refusals: a command error in a session returns to the loop that reads
  commands, with everything the command allocated freed. `resize` and
  `scale` check the frame's size before they change the window.
- `ready`: written by the reader before each `getline` in a session,
  after standard output is written out; the count of bytes is kept by
  `mn_out_*`, through which all of a program's standard output goes.
- The end: `drive_finish` in a session reads commands until `quit` or
  the end of input, answering `tick` as rule 3 says, and then returns to
  `mn_finish`, so a debug build checks its buffers; `quit` and the end
  of input before the program's end take the path of `exit`.
- `quit`, the names in the errors (from the program's tables of
  components, tags and resources), and `draws from N`.
- Nothing new is linked: the runtime stays C11 with libc and libm, and
  `getline`.

### The compiler

- `src/session.rs`: `session start`, `stop`, `list`, the holder, and
  `drive`, with `std::os::unix::net` and
  `std::os::unix::process::CommandExt::process_group`, no crates and no
  `unsafe`. The holder reads the program's standard output and standard
  error on two threads and uses `output_bytes` to split the output
  between commands. The user's number comes from the metadata of a file
  it has just made.
- The usage text gains the four commands.

### Tests

- `tests/session/NAME.12am` with `NAME.drive`, run with the file as
  standard input and `MIDNIGHT_DRIVE=-`, in release and debug, with
  `NAME.out` its standard output and `NAME.session` its standard error
  followed by a line `status N`. Each program begins with an `# expect:`
  comment. `UPDATE_EXPECT=1` rewrites `.session` files from the release
  run. Among them: refusals of every kind, a program that ends and is
  looked at after its end, `quit` before and after the end, a program
  that never ticks, a run-time error, comments and empty lines. The
  programs with refusals end by their last statement, so that their
  debug builds check their buffers.
- `session_refuses_every_drive_error`: for each case of `drive_errors`,
  its commands are sent in a session, followed by `tick`, `draws` (with
  a window open) and `tick`, to a program that prints the fields of
  `Window`, `Keyboard` and `Mouse` each tick. The refused command's
  answer is the refusal with the message the file reports, and the
  standard output and every other answer are those of a file of the same
  commands without the refused one, `line` members aside.
- `session_live`: commands written one at a time, each after the `ready`
  of the one before, to a program that prints between ticks; each
  command's output is the bytes `output_bytes` gives.
- `session_end_of_input`: a windowless `World.run` program, input closed
  after `tick 3`, ends with status 0 within a second and prints nothing
  of a fourth tick; the same with a headless window.
- `run_both_forms` runs the session programs too. `tests/json/NAME.12am`
  with `# json: session` gives the standard error of the program with
  `NAME.drive` as standard input; cases for `ready`, `refused`, a page of
  `draws` and `quit`. `tests/json_lines/mod.rs` reads and writes the new
  kinds.
- In GPU mode, `session_capture` runs `tests/session/capture.12am` and
  requires its `capture` answers to be those of the same commands in a
  file.
- `quit` in `tests/run`: a windowless `World.run` program whose `.drive`
  file ends with `quit`.
- `draws`: `draws_from` covers the first, middle, last and empty pages.
  `window_drawn`, for each `draws` it finds before a `capture`, runs the
  program once to learn the frame's count and then writes `draws from
  100`, `draws from 200`, … after that `draws` in the commands it runs,
  and renders the pages put together. No `.drive` file changes for it;
  `draw_many.draws` holds first pages. `tests/render/mod.rs` and
  `tests/json_lines/mod.rs` read and write the header of a page.
- `drive_errors` has ``\`quit\` takes nothing``, ``draws: `from` takes a
  draw number from 0 up`` and ``\`draws\` takes nothing, or `from` and a
  draw number``, and `query_errors` the new words.
- `session_commands`: `session start`, `drive` (one command, several,
  `-`, a refusal, `quit`, the program stopping with an error, an
  argument with a line break), `list` with two sessions, `stop`, a name
  in use, a name that is not one, a stale session, no session, two
  sessions and no `--session`, `awake` refused, a program that writes no
  `ready` (with the 60 seconds shortened as below), a caller with
  `MIDNIGHT_FORMAT=json` and no `--format` getting a text session, a
  sessions directory of mode 0755 refused, a socket path of 101 bytes
  refused, JSON forms; `--idle 1` stopping an idle session, with
  `MIDNIGHT_SESSION_IDLE_SECONDS`, which only the holder reads, for
  tests, set to 2 (and the first `ready`'s wait shortened the same way
  by `MIDNIGHT_SESSION_START_SECONDS`); `session stop` of a program in
  `tick 1000000000` ends it within six seconds; a window program started
  with `info` prints a `headless` line though the caller's
  `MIDNIGHT_WINDOW` is empty, and one started with `LD_LIBRARY_PATH` set
  runs; the program of a holder killed with `SIGKILL` is found by the
  executable path in its session directory and has ended within a
  second (Linux).
- Every session test has an `XDG_RUNTIME_DIR` of its own, with the
  display variables of `no_display`; window programs run with
  `MIDNIGHT_WINDOW=headless`.
- `skill_is_short_and_works` requires the skill's frontmatter to have
  `name: midnight` and a `description`, its body to be at most 150
  lines, and each `midnight` command in its code blocks, written against
  files of `examples/`, to exit with status 0 in a session test
  directory.
- `tests/COVERAGE.md` maps the new commands, rules, errors and lines.

### Slices

1. **The runtime.** `MIDNIGHT_DRIVE=-`, the four rules, `ready`,
   `quit`, the names in errors, `draws from N`, JSON `format` 2, and
   their tests.
2. **`midnight session` and `midnight drive`.** The holder and the four
   commands, and `session_commands`. Gate, before anything else in the
   slice: a holder started from Claude Code's Bash tool, in an ordinary
   session, in a `claude -p` session confined as the spike will be, and
   under the evals' wrapper, which kills each call's process group, is
   still running when the tool call has returned, and its socket answers
   from the next call. If any fails, stop and report.
3. **Documents, skill, spike.** The reference (below), `docs/json.md`,
   `docs/running.md` (its table names `MIDNIGHT_DRIVE` a file name), the
   README, the repository's `CLAUDE.md`, `skills/midnight/SKILL.md`,
   `tests/COVERAGE.md`; the spike; the Outcome, with how long
   `window_drawn` takes for `flock` in pages.

The reference changes in "Driving" (the session and its four rules,
`quit`, the errors' new words, `MIDNIGHT_DRIVE=-`), "The command
`draws`" (pages), "Without a display" and "Commands for the keyboard and
the mouse" (refusals in a session), "Errors", "What a draw costs" (a
page's cost), the version, and "Not in 0.11", which drops a session
that stays open and gains interrupting a `tick N` and servicing a shown
window while a session waits.

## Acceptance criteria

1. Every program of `tests/session` gives exactly its `.out` and
   `.session`, in release and debug, and in JSON with the same content.
2. `session_refuses_every_drive_error` passes: every command error of
   the reference is refused in a session with the message a file
   reports, and changes nothing the commands after it can see.
3. `session_live` passes: each command's output is split exactly by
   `output_bytes`, and every line a command brings, but `stats` lines,
   comes before its `ready`.
4. `session_end_of_input` passes, and a holder killed with `SIGKILL`
   leaves no program running.
5. `quit` ends a program in a file and in a session with the status this
   spec gives.
6. `draws` lists pages of at most 100, `window_drawn` passes with
   `flock` and `draw_many` in pages, and every other `.draws` file is
   unchanged.
7. `session_commands` passes, and the slice 2 gate passed.
8. A debug build of every program of `tests/session` that ends by its
   last statement, after refusals of every kind and then `quit` or the
   end of input, ends with no sanitizer report and no live buffer.
9. Every `.out`, `.err`, `.plan`, `.loops`, `.split`, `.stats`,
   `.frames`, `.draws`, `.queries` and `.json` file that existed in
   0.10.0 is unchanged, but for `draw_many`'s `.draws`, the errors of
   `query` and `resource` that now name what a program has, the error of
   `draws` with an argument, and the version and `format` in `.json`
   files; for every program that existed in 0.10.0, `midnight emit-c`
   and the generated shaders are byte for byte 0.10.0's but for the
   version string the generated C carries.
10. Every suite, the GPU and window modes and `nix flake check` pass.
11. The reference says 0.11 and `midnight --version` prints `midnight
    0.11.0`; `docs/json.md` has `format` 2 and the new kinds; the README's
    commands work as written; `skill_is_short_and_works` passes;
    `tests/COVERAGE.md` has every construct, error and line of this spec.
12. The spike ran, and the Outcome says what it showed.
13. No test, check or app of this spec opens a window on the desktop it
    is run from.

## Decisions

- **Standard input is a session; a file is a script.** A file of
  commands is a test or a replay, where a wrong command should stop
  everything; standard input is a conversation, where it should not.
  One variable chooses both, with one new value.
- **A `ready` line frames everything.** A client writes a line and reads
  to the next `ready`, as a person at a prompt does. It needs no ids and
  no counting, covers output before the first command, a tick's prints
  and the program's end, and the count of bytes makes the split of
  standard output exact.
- **The end of input ends the program.** The process that holds the
  input is the session; nothing it leaves behind keeps running past the
  `tick` it was carrying out.
- **A holder in `midnight`, over a Unix socket.** An agent at a shell
  holds a session across separate commands without managing a process,
  pipes or cleanup, and the runtime stays libc only.
- **Errors name what the program has.** An agent that guessed a name
  learns the right one from the refusal, with no command to learn.
- **`draws` pages as `query` does.** One rule for both listing commands,
  and a frame of a hundred thousand draws no longer floods a client.
- **The skill is a guide in the repository.** Short, versioned with the
  reference, and read only by an agent working with Midnight.
- **The spike has three arms.** Files, the session at the shell, and a
  throwaway MCP server over the session: it shows whether the session and
  the skill help, and whether a server is worth a spec of its own.

## Measured in planning

From the planning probe and audit, on the development machine (RTX
5090, `llvmpipe`), Midnight 0.10.0 driven over a pipe:

- A `tick` or a `query` of a small program takes about 0.01 ms; a tick
  of `flock` (100,000 points) 0.04 ms and a query 0.45 ms.
- The first headless `capture` opens the device: 140 to 230 ms on the
  5090, 26 ms on `llvmpipe`; later ones 1 to 6 ms (`flock` 30 ms on
  `llvmpipe`). `catch`'s PNG is 3.6 KB, `flock`'s 232 KB.
- A release build takes 0.6 s for a small program, 1.3 s with a window,
  1.5 s for `flock` and `catch`; a debug build of `flock` 0.6 s; `check`
  2 to 3 ms.
- `flock`'s `draws` after one tick takes 0.35 s on either device and is
  one JSON line of 9,468,329 bytes, or 100,004 lines of text.
- A program waiting for a command uses no processor; with
  `MIDNIGHT_THREADS=4,awake` it uses three cores.
- Agents driving programs through a throwaway server over the pipe
  (eight sessions, Haiku and Sonnet): images read correctly; most turns
  lost to restarts after a wrong command, one session restarting seven
  times while guessing names; a program left running after its client
  was killed kept a core busy.

## Outcome

Implemented, with the differences listed under "Differences from the
design". Every criterion passes on x86_64-linux: a Ryzen 9 9950X3D beside
an RTX 5090 and `llvmpipe` of Mesa 26.1.8, `wgpu-native` 27.0.4.0, SDL
3.4.10, sway 1.12, clang 21.1.8, Claude Code 2.1.327 for the gate and the
spike.

### Acceptance criteria

| # | Result |
| - | ------ |
| 1 | `session_programs` gives the `.out` and `.session` of all eight programs of `tests/session` in release and debug, and the same through `run_both_forms` in JSON; `json/session` pins the JSON of a session. |
| 2 | `session_refuses_every_drive_error` passes for all 67 cases of `DRIVE_ERRORS`: each refused with the file's message, and the output and every other answer those of a file without the wrong command. |
| 3 | `session_live` passes: each command's output is the bytes its `ready` counts, and every answer comes before its `ready`. |
| 4 | `session_end_of_input` passes, windowless and with a headless window (status 0 within a second, nothing of a fourth tick); in `session_commands` the program of a holder killed with `SIGKILL` has ended within a second. |
| 5 | `quit_world_run` (a file), `session/quit_before_end` (status 0) and `session/quit_after_end` (the status of `exit(3)`). |
| 6 | `draws_from` and `draws_pages` (also on the device, through `gpu_programs`) pass; `window_drawn` and `window_shown` list `flock` and `draw_many` in pages and pass on the RTX 5090 and on `llvmpipe`, in the build sandbox too; only `draw_many.draws` changed among the `.draws` files. |
| 7 | `session_commands` passes; the gate passed before any of `src/session.rs` was written (below). |
| 8 | `session_programs` runs every program of `tests/session` in debug, `session/refusals` with refusals of every kind and then the end of input after its last statement: no sanitizer report, no live buffer. |
| 9 | Of the expected files of 0.10.0, only `draw_many.draws`, the `query` and `resource` errors of `query_errors`, the `draws` error of `drive_errors` and the ten `.json` files changed, the last in their first line and, in `json/drive_window` and `json/run_text`, by the `count` and `from` that a `draws` answer gains in `format` 2. `emit-c` and `emit-wgsl` of all 901 programs of 0.10.0 give 0.10.0's output but for the version string. |
| 10 | The suite, the software and hardware GPU modes, the window tests and `nix flake check` pass. In one run of `nix flake check`, `window_keeps_time` (3.4 s for 120 ticks drawing 10,000 shapes on `llvmpipe`) and once the harness's timed self-test failed while every check ran at once; each passed run alone, and every other check passed. |
| 11 | The reference says 0.11, `midnight --version` prints `midnight 0.11.0`, `docs/json.md` has `format` 2 and the new kinds, the README's commands work as written, `skill_is_short_and_works` passes (113 lines of body), and `tests/COVERAGE.md` has "A session" and "`midnight session` and `midnight drive`". |
| 12 | The spike ran: below. |
| 13 | Every session runs headless; the tests run with the display variables of `no_display`; window programs ran headless or inside `tests/window/compositor.sh`. |

### The gate

Run with a holder of the same design written for the purpose (std only,
`process_group(0)`, standard streams on `/dev/null`, a child under it, a
Unix socket) before `src/session.rs`:

- An ordinary Bash tool call: the holder was re-parented to init in its
  own group and answered the next call.
- `claude -p` confined as the evals run agents (`--restricted`, only
  `Bash(midnight *)`, `env -i` with a runtime directory of its own),
  Haiku: started in one Bash call, answered the next two. It outlived
  `claude` itself, so the spike stopped every session after each run.
- The evals' wrapper, as `proc::capture` runs a call (a group of its own,
  both streams read to their end, the group killed after): the call
  returned in 18 ms, the kill reached nothing, and the holder answered
  and stopped from later wrapped calls.

### Measured

On the RTX 5090, `flock` driven by its `.drive` file without the capture:
0.23 s in release with one page of `draws`, 1.3 s with all 1,000 pages of
its 100,000 draws; 1.5 s and 4.3 s in debug. `window_drawn` with `flock`
and `draw_many` in pages takes 35 s on the RTX 5090 and 16 s on
`llvmpipe`, in both build modes and at four scales.

### The spike

Three tasks: a door placed by the house's width, seen in the frame; a
run-time error at tick 374 from a particle that reaches the wall exactly;
a constant to tune until a thrown ball lands in a range. Three arms at the
same build, run separately: files of commands with `midnight session` and
`drive` denied; the session and the skill; the skill and a throwaway MCP
server whose tools are the CLI's commands, a capture also returned as an
image, with no shell. Sonnet and Haiku, one session each: 18 sessions.
Each agent had a directory and `XDG_RUNTIME_DIR` of its own, no display
and the device; after each run its sessions were listed and none was left
running. The skill was given as a plugin of the session, and no
`CLAUDE.md` of the account reached the agents.

| Arm | Turns | Cost at API prices | Builds | Replays | Done |
| --- | --- | --- | --- | --- | --- |
| Files | 73 | $0.17 | 23 | 12 | 6 of 6 |
| Session + skill | 61 | $0.16 | 13 | 0 | 6 of 6 |
| MCP + skill | 62 | $0.21 | 13 | 0 | 6 of 6 |

No command was refused in any session. Sonnet found both bugs by reading
the source in every arm and ran the program to confirm; Haiku looked. In
the files arm it wrote copies of the program with `puts` lines and built
11 times for the crash; with a session it stepped and queried in 5. No
session lost a third of its turns to holding a session: agents chained
`session start`, `drive` and `session stop` in one call. The tuning task
was solved by arithmetic in every arm.

What it informed:

- **The skill.** Two mistakes seen went into it: driving a session after
  editing the source, which holds the program as built, and copies of the
  program with `puts` where `query` shows the state.
- **The MCP question.** The MCP arm did its tasks in about the turns of
  the session arm (62 against 61) and at more cost, and no session lost a
  third of its turns to holding one: by the measure stated before the
  runs, a server is not worth a spec now. Every task of the MCP arm was
  done in the session arm with the same commands.

The tasks are small and one session a cell is rough: it says the session
and the skill cost nothing in turns or money and save builds where an
agent looks, not that they make agents succeed more. Transcripts and the
pilot stay with the planning notes.

### Differences from the design

- `midnight drive` and `session stop` print ``no session `NAME` is
  running; `midnight session list` lists those that are`` for a name that
  does not run; the spec gave no text.
- The message of a program that writes no `ready` gives the seconds the
  holder waited, 60 unless the test variable shortens them. What the
  program wrote is passed on as far as it was written out: the holder
  ends it with `SIGKILL`.
- The holder sends a command's standard output on as it arrives rather
  than at the `ready`, so a long printing `tick` does not grow its memory;
  the lines on standard error still come after it, as the spec orders.
- The holder removes the session's directory before it tells its client
  the program stopped, so a `session start` of the same name right after
  `drive quit` finds the name free.
- `session stop` and `session list` work with a holder of another version;
  `drive` refuses it, as the design says.
- `drive -` followed by commands as arguments is a usage error.
- `window_shown` lists frames in pages as `window_drawn` does, its count
  taken from a run with no device.
- Criterion 9 left out the `count` and `from` of `draws` answers in the
  two `.json` files that hold one; they come with `format` 2.
- The README's Status said 0.9 and "no text"; it now says 0.11.
