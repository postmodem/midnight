# 0010 — JSON output, and driving every program

- **Status:** approved
- **Depends on:** 0009

## Goal

Midnight's tools explain themselves in text: diagnostics, `midnight
plan`, `midnight loops`, the answers to drive commands, `stats` lines,
run-time errors. An agent at a shell reads that text well, and it stays
the default. A program reading it (a script, a test, the MCP server that
a later spec builds) has to parse column layouts and prefixes that were
written for people. Only a program with a window can be driven, and a
driven program can show its frames but not its world.

After this spec:

```sh
midnight check --format json game.12am
```

prints each diagnostic as one JSON object on a line, with the file, the
start and end of the offending text and its message, and `plan` and
`loops` do the same for transfers and loops. A program run with
`MIDNIGHT_FORMAT=json` writes every line it writes to standard error as a
JSON object: drive answers, `draws`, `stats`, `info` lines, run-time
errors. Its standard output is unchanged.

Any program can be driven, window or not, and two new commands look at
its world between ticks:

```
tick 3
query Position, Velocity, without: Frozen
resource Score
```

```
midnight: drive: tick 3
midnight: drive: query Position, Velocity, without: Frozen: 3 ticks, 2 rows
midnight: drive:   Entity(0v1) Position(x: 1.5, y: 3.0) Velocity(dx: 1.0, dy: 2.0)
midnight: drive:   Entity(2v1) Position(x: -0.5, y: 1.0) Velocity(dx: -1.0, dy: 0.0)
midnight: drive: resource Score: 3 ticks, Score(points: 30, best: 30)
```

A driven program runs no tick that a command did not let run, so the
same program and the same commands give the same answers on every
machine. An agent that wants to look, decide and look again adds
commands to its file and runs it again.

### How this serves the principles

- **Observable and repeatable.** A program's world can be read at any
  tick, exactly, without adding `puts` to it. Answers come between ticks,
  in the program's own visiting order, so they are the same on every run,
  for any number of threads and on any device within what "Determinism"
  allows.
- **Every tool works from the command line with output an agent can
  use.** JSON is for programs that read Midnight's output; the text stays
  what a person or an agent at a shell reads. Both are made from the same
  data, so they say the same thing. The MCP server of a later spec is a
  layer over these, adding no capability the command line lacks.
- **One obvious way.** One switch for the compiler, one variable for a
  program, one framing (a JSON object a line) for both. `query` takes the
  terms of an `each`, so there is no second query language to learn.
- **Errors teach.** A diagnostic in JSON carries the same message and
  label as the text. The errors of the new commands each say what is
  wrong and what to write.
- **The documentation's size is a cost.** The commands are in the
  reference, beside the other commands; the JSON forms are in a document
  of their own, `docs/json.md`, which an agent writing a program does
  not need to read.

## Scope

- `--format json` for `check`, `build`, `run`, `plan` and `loops`.
- `MIDNIGHT_FORMAT=json` for programs: every line the runtime writes to
  standard error as a JSON object. `midnight run --format json` sets it
  for the program it starts.
- `MIDNIGHT_DRIVE` for every program, and one rule for when a driven
  program ticks.
- The commands `query` and `resource`, in text and in JSON.
- `Float32` numbers in answers: one rule that a 64-bit reader also reads
  exactly, which changes the text of two values in `draws`.
- `docs/json.md`; the reference at 0.9; `docs/running.md`.

## Non-goals

- A long-lived session: a channel of its own for commands and answers,
  errors that leave a program running, a client that sends one command to
  a running program, a first message that describes the program. A later
  spec adds them, with the MCP server that needs them.
- `record`, a frame captured at every tick. A file of commands with a
  `capture` after each `tick` does the same.
- Changing the world from commands (spawning, setting a field), and
  commands that wait for a condition.
- Pending events, single fields (`Body.pos.x`), filters on values, and
  the built-in resources in `resource`.
- Error codes, machine-applicable fixes, SARIF, a JSON Schema file,
  UTF-16 columns.
- JSON for `emit-c`, `emit-wgsl`, `--help`, `--version` and usage errors.
- A change to what a program prints on standard output for the same
  ticks and the same input. A `.drive` file of a program that ticks while
  no window is open gives those ticks with `tick` commands.

## Design

### Two forms, one content

Every output this spec touches has a text form, the text the reference
and the README give (unchanged but for the `Float32` rule below), and a
JSON form. Both are rendered from the same data: the compiler builds a
diagnostic, a transfer or a loop as data and has two renderers for it,
and the runtime builds an answer once and writes it in the form asked
for. A JSON object holds what the text says and may hold more (a byte
offset, an end position, the filters of a loop), never less.

### JSON lines

A JSON form is a sequence of lines, each one JSON object (RFC 8259) in
UTF-8, ended by `\n`, with no newline inside it. Each object has a
string member `kind` that says what it is; the other members depend on
the kind.

- **Keys.** The keys `docs/json.md` defines are lower case with
  underscores, in the words of the reference (`message`, `label`,
  `component`, `pixel_width`). Keys that name a component or a field are
  its name as declared; a component's name begins with a capital, so it
  is never a defined key.
- **The text of a line.** No space or newline between tokens. `kind`
  comes first, then the members in the order `docs/json.md` lists them.
  A string escapes `"` and `\` with a backslash, writes `\n`, `\r` and
  `\t` as such and every other byte below 0x20, and 0x7f, as `\u00xx`
  with lower-case hex digits, and writes every other character as its
  UTF-8 bytes. A byte that is not part of valid UTF-8 (in a command line
  or a file name) is written as U+FFFD.
- **Members.** A member marked optional is absent when it has no value,
  never `null`; every other member is always present. An array may be
  empty.

The first line of every JSON form, from the compiler and from a program,
is

```json
{"kind":"midnight","format":1,"version":"0.9.0"}
```

`version` is the compiler's version, the one that built the program:
the generated code gives it to the runtime in `MnProgram`. `format` is
the version of the JSON forms, an integer.

**Stability.** A reader must skip a kind it does not know and members it
does not know. Within a `format`, a later version may add kinds, optional
members, and values of a string member that lists alternatives (as
`runs` below), and changes nothing else: a member is never removed,
renamed or given another type, and its meaning does not change. Each
kind has one shape: what a line says is told by its kind, never by which
members it has. Anything else raises `format`. `docs/json.md` says this
first.

**Numbers.**

- Counts, sizes, line numbers, ticks and bytes are JSON integers.
- An `Int` is its exact decimal digits. A reader that keeps a number in
  a 64-bit float (JavaScript's `Number`) reads values beyond ±2^53
  inexactly; `docs/json.md` says so.
- A `Float` is the text `to_s` gives it, which is a JSON number for every
  finite value. `NaN`, `Infinity` and `-Infinity` are the strings
  `"NaN"`, `"Infinity"` and `"-Infinity"`.
- A `Float32` (a `Float32` field at any depth, an element of a
  `[Float32]` at any depth, and the numbers of a draw) is the shortest
  text that reads back as the same 32-bit value, read either as a 32-bit
  number or as a 64-bit number rounded to 32 bits; non-finite values as
  for `Float`. The text forms of `draws`, `query` and `resource` use the
  same rule. It differs from the shortest 32-bit text for two values
  only, `7.038531e-26` and `-7.038531e-26`, which a 64-bit reader would
  round to the next 32-bit value; they are written `7.0385309e-26` and
  `-7.0385309e-26`.

A `Color` is an array of its four bytes, `[r, g, b, a]`, wherever it
appears. An entity is a string, the text `to_s` gives it:
`"Entity(3v1)"`, `"Entity(none)"`.

### The compiler: `--format`

`--format text` and `--format json` are options of `check`, `build`,
`run`, `plan` and `loops`; `text` is the default. Any other value,
`--format` given twice, `--format=json`, or `--format` on another
command is a usage error (status 2, text, as every usage error).
`USAGE` lists the option.

The JSON goes where the text goes: diagnostics and the compiler's own
messages on standard error, `plan` and `loops` on standard output. The
first line is written before anything else, on standard error for
`check`, `build` and `run` and on standard output for `plan` and
`loops`; a `plan` or `loops` that finds errors writes it to both. Exit
statuses do not change.

**Diagnostics.** One line per diagnostic, in the order of the text, at
most 20 (`MAX_ERRORS`):

```json
{"kind":"error","message":"unknown component `Postion`","label":"did you mean `Position`?","file":"move.12am","start":{"line":14,"column":8,"byte":312},"end":{"line":14,"column":15,"byte":319},"notes":[]}
```

- `file` is the name as the text prints it, `<built-in>` included.
- `start` and `end` give the offending text: `line` from 1, `column` from
  1 counted in Unicode scalar values (as the text counts them), `byte`
  from 0 into the file. `end` is just past the text; an empty span has
  `end` equal to `start`. The text marks only the first line of a span
  that runs over several; the JSON gives the whole span.
- `label` (optional) is the text after the marker.
- `notes` is an array of `{"message", "file", "start", "end"}`, in order;
  a note may be in another file.

Whenever there is at least one diagnostic, one line with the totals
follows them, which the text says as `note: N more errors not shown`
when some are hidden:

```json
{"kind":"errors","count":23,"shown":20}
```

A `check` that finds no error writes the first line and nothing else.

**The compiler's own messages.** Each is one line:

| Kind | Members | Text |
| ---- | ------- | ---- |
| `cant_read` | `file`, `message` (the system's reason) | ``midnight: can't read `FILE`: …`` |
| `no_flags` | `variables` (an array of the variables not set), `affects` (`gpu_systems` or `window`), `message` (the text's line) | the line that says what the program is built without |
| `c_compiler` | `compiler`, `status` (optional: absent when it did not start), `output` (what it printed, one string), `kept` (optional: the generated C's path, when it ran and failed) | the C compiler's failure |
| `cant_write` | `file`, `message` | ``midnight: can't write `FILE`: …`` |
| `build_directory` | `directory`, `message` (optional: absent when every name was taken) | ``midnight: can't create a build directory in `DIR` …`` |
| `cant_run` | `message` | `midnight: can't run the program: …` |

**`plan`.** One line per transfer, in the order of the text:

```json
{"kind":"transfer","direction":"download","component":"Position","from":{"system":"move","strict":false},"to":{"top_level":true},"rows":false}
```

- `direction` is `upload` or `download`.
- `from` and `to` are `{"system": NAME, "strict": BOOL}`, or
  `{"top_level": true}` for the top-level code.
- `rows` is true when only the rows concerned move.
- `on` (optional) is the structural change the transfer happens because
  of: `spawn`, `despawn`, `add` or `remove`.

**`loops`.** One line per loop, in the order of the text:

```json
{"kind":"loop","file":"bounce.12am","line":90,"in":{"def":"render"},"terms":["Position","Glyph"],"with":[],"without":[],"runs":"one_core","reason":"uses a `String` at line 93","rule":6}
```

- `in` is `{"system": NAME, "gpu": BOOL, "strict": BOOL}`,
  `{"def": NAME}` (`Vec2.length` for a method), or `{"top_level": true}`.
- `terms` is an array of the terms' names in order; the text's `each
  Position, Glyph` is `each ` and these joined by `, `. `with` and
  `without` are the filters, which the text does not print.
- `runs` is `gpu`, `cores` or `one_core`. For `one_core`, `reason`
  (optional) is the text after `one core: ` and `rule` (optional) the
  number of the rule of an independent `each` it breaks, as the table in
  "Several cores" numbers them.

### Programs: `MIDNIGHT_FORMAT`

`MIDNIGHT_FORMAT` is `text` (the default) or `json`, one word; any other
value is reported once and the program writes text. Every program reads
it, before any other variable, and with `json` writes the first line
above before anything else. From then on every line the runtime writes
to standard error is a JSON object. Standard output is the program's,
byte for byte as without it.

`midnight run --format json` passes `MIDNIGHT_FORMAT=json` to the
program it starts, and `--format text` removes the variable; without the
option the variable is passed on as the caller set it, and `midnight run`
itself writes text. On standard error, `run --format json` writes its
first line and its own lines, then the program's lines, which begin with
the program's own first line. When the build fails, the program does not
start.

The runtime writes standard output out before each line it writes to
standard error, as it does before each answer of the text form. So when
both go to one pipe (`2>&1`), the program's lines and the runtime's
arrive in the order they happened. A line on standard error that does not
begin with `{` was written by something other than Midnight (a
sanitizer, a driver); `docs/json.md` says so.

Every line the runtime writes, by kind:

| Kind | When | Members |
| ---- | ---- | ------- |
| `runtime_error` | a run-time error | `file`, `line`, `message`: the text's three parts |
| `gpu_error` | `midnight: GPU error: …` | `message` |
| `window_error` | `midnight: window error: …` | `message` |
| `out_of_memory` | `midnight: out of memory` | none |
| `buffers_alive` | a debug build's buffers at exit | `count` |
| `unknown_word` | a word a variable does not have | `variable`, `word` |
| `gpu_device` | `MIDNIGHT_GPU=info`, the device | `device` (optional: absent for no device) |
| `gpu_strict` | `MIDNIGHT_GPU=info`, the strict check | `passed` (Bool), `difference` (optional string: the device and the first difference, when it failed) |
| `gpu_cpu_block` | `MIDNIGHT_GPU=info`, a block on the CPU | `system`, `each`, `reason` |
| `threads` | `MIDNIGHT_THREADS=info`, at start | `threads` |
| `threads_split` | `MIDNIGHT_THREADS=info`, a loop's first split | `file`, `line`, `rows`, `pieces`, `threads` |
| `window_opened` | `MIDNIGHT_WINDOW=info` | `width`, `height`, `pixel_width`, `pixel_height`, `shown` (`headless`, `wayland`, `x11` or `cocoa`), `present` (optional: `mailbox`, `immediate` or `fifo`; absent headless), `driven` (Bool) |
| `window_event` | `MIDNIGHT_WINDOW=info`, a shown window | `event`: `focused` or `pointer_inside` |
| `stats` | `MIDNIGHT_GPU=stats` | `tick`, `uploads`, `downloads`, `rows_up`, `rows_down`, `submissions`, `waits` |
| `answer` | each drive command carried out | below |
| `drive_error` | a command that stops the program | `file`, `line`, `message` |
| `drive_unopened` | a file of commands that cannot be opened | `file`, `message` |

A JSON line has no prefix: the `midnight: TOPIC:` of the text is its
`kind`. In a `stats` line, `tick` is the tick the line counts; in an
answer it is `Time.tick` when answered, one more than the tick that last
ended. `docs/json.md` says so beside each.

The thread pool writes its own out-of-memory line, without allocating:
`mn_pool_start` is told whether the program writes JSON
(`midnight_pool.h`), and writes `{"kind":"out_of_memory"}` then.

**Answers.** Each answer has `name` (the command's first word),
`command` (the line as written, the spaces around it removed), `line`
(its line in the file) and `tick` (the value of `Time.tick` when it is
answered), and members by command:

| Command | Members |
| ------- | ------- |
| `tick`, `tick N` | `ran` (ticks run, an integer), `ended` (Bool: the program ended first) |
| `resize`, `scale`, `close`, the keyboard and mouse commands | none |
| `capture FILE` | `file`, `frame_tick`, `pixel_width`, `pixel_height`, `crc` (a string of eight hex digits, as the text) |
| `draws` | `frame_tick`, `width`, `height`, `background`, `draws`: an array of shapes |
| `query …` | `count`, `from`, `rows`: an array of rows |
| `resource NAME` | `resource` (the name), `fields`: the resource as an object |

A shape is an object with `shape` (`rect`, `circle`, `line`, `triangle`)
and the arguments under the names of the reference's table (`x`, `y`,
`width`, `height`, `radius`, `x1` … `y3`, `color`). The background is a
color, with the `a` the program gave it.

### Driving every program

`MIDNIGHT_DRIVE` is read by every program, window or not. "Without a
display" keeps its commands and its rules but these:

- **A driven program runs no tick that a command did not let run.**
  Before each tick begins, window open or not, the runtime carries out
  commands until one lets the tick run. A program that ticks before
  `Window.open`, or between a close and another `Window.open`, takes
  `tick` commands for those ticks too.
- When the commands run out, an open window is closed as by `close`, and
  so is a window opened later, in its first tick; from then on ticks run
  freely. A program without a window whose `World.run` ends only by
  `World.stop` runs until it calls it.
- When the program ends by its last statement or by `exit`, the commands
  up to the next `tick` are carried out, in every program.
- A run-time error, an error of the device or the window, or a command
  error ends the program at once: a pending `tick` is not answered, no
  further command is carried out and no further `stats` line is written;
  the error's line is the last.
- **The window's commands need an open window.** While `Window.open?` is
  false (before the first `Window.open`, and from the tick a close takes
  effect in until the next `Window.open`), `resize`, `scale`, `close`,
  `capture`, `draws` and the commands of the keyboard and the mouse stop
  the program with ``can't `COMMAND`: no window is open``, `COMMAND` being
  the command as written; `capture` and `draws` keep their messages. This
  is checked before anything else about the command, the need for
  `MIDNIGHT_WINDOW=headless` included, and it is the first rule of the
  list of input-command errors. Commands carried out after the program
  ends are judged the same way. Between a `close` and the tick it takes
  effect in, the window is open.
- `MIDNIGHT_WINDOW` stays a variable of programs with a window only.

### `query`

```
query TERMS
query TERMS from N
```

`TERMS` is written as after `each`: one or more terms, each a component
or `Entity`, then `with:` and `without:` as the reference has them, a tag
or a bracketed list each. It is read as the source reads the same words:
names, `,`, `with:`, `without:`, `[` and `]`, with spaces or tabs allowed
between any two and none needed; `with:` and `without:` have no space
before the colon and each appears at most once; `from` and its number,
if given, come last. `N` is a whole number from 0 up, in digits.

The answer lists the entities that match, in the order an `each` with
these terms visits them at that point, from row `N` (from 0 when `from`
is not given), at most 100; when `N` is the count or more, no row
follows. Each row is the entity, then each component term in the order
written, whole. An `Entity` term adds nothing to a row, since a row
always begins with its entity; tags are not listed. `count` is the number
of entities that match, all of them.

Text:

```
midnight: drive: query Body: 4 ticks, 100000 rows, 0 to 99
midnight: drive:   Entity(0v1) Body(x: 12.5, y: 40.25, vx: 1.0, vy: 0.5)
…
midnight: drive: query Ghost: 4 ticks, 0 rows
```

The first line is the command, `: `, `Time.tick` as `N ticks`, the count
as `N rows` (`1 tick` and `1 row` for one), and, when the rows that
follow are not all of them, `, A to B`, the first and last listed, or
`, none from N` when none is. A row is two spaces, the entity and each
component as `to_s` prints it, separated by one space, but for `Float32`
numbers, which follow the rule above.

JSON:

```json
{"kind":"answer","name":"query","command":"query Body","line":3,"tick":4,"count":100000,"from":0,"rows":[{"entity":"Entity(0v1)","Body":{"x":12.5,"y":40.25,"vx":1.0,"vy":0.5}}]}
```

A row is an object: `entity`, then a member per component term, named
by the component. A component, struct or event value is an object of its
fields in declared order (`{}` for none); a list is an array; an enum is
its variant's name as a string; a `String` is a JSON string; `Bool` is
`true` or `false`; numbers, colors and entities as above.

**When.** Commands are carried out between ticks, so a query sees the
world as the last tick and the top-level code after it left it, with
every queued change applied. The exception is the end of a program that
called `exit`: the commands see the world as it was where `exit` was
called, part way through a tick if it was called in a system, with
`Time.tick` the tick in progress, and, if it was called inside an
`each`, without the changes that loop queued.

**The GPU.** A query of a component whose current copy is on the device
reads it as top-level code would: the column is downloaded, behind one
wait, and `stats` counts it in the line of the last tick that ended,
`tick T-1` for an answer whose `tick` is `T`, as it counts the readback
of `draws`; before the first tick, in the line of tick 0; at the end of
a program, in the last line. After it the copy on the CPU is current
too, so later `stats` lines can differ from those of a run without the
query. What the program prints does not change.

### `resource`

```
resource NAME
```

`NAME` is a resource the program declares. The built-in resources
(`Time`, `Window`, `Keyboard`, `Mouse`) are not read.

Text: ``midnight: drive: resource Score: 3 ticks, Score(points: 30, best:
30)``: the command, `: `, the tick, then the resource as `to_s` would
print a struct of its fields (`Float32` numbers by the rule above); a
resource with no fields is `Empty()`. JSON: `resource` is its name and
`fields` the object of its fields, as a component in a row (`{}` for
none).

### Errors of `query` and `resource`

Each stops the program with status 1, as a wrong command does, with the
file and line of the command. When more than one applies, the first in
the line, left to right, is reported:

```
query: `NAME` is not a component or tag of this program
query: tag `NAME` can't be a term; filter with `with: NAME`
query: `Entity` can't be a filter
query: `NAME` appears twice
query: at least one component or `Entity` comes before `with:` and `without:`
query: `from` takes a row number from 0 up
`query` takes the terms of an `each`
resource: `NAME` is not a resource of this program
resource: `NAME` is built in; `resource` reads the program's own resources
`resource` takes the name of a resource
```

The last of each group is for a line that cannot be read as the command
at all (nothing after the word, a stray character, two names after
`resource`).

### What a program has to describe itself

The runtime knows each component's name, size and alignment, and
nothing of its fields; resources are statics it cannot see. The compiler
now generates, for every component and every resource the program
declares (not only those whose `to_s` is used):

- a function that writes a value as text, as `to_s` writes it but for
  the `Float32` rule, and one that writes it as JSON, with the same for
  each struct, list and enum type they reach;
- for resources, a table of names, addresses (none for a resource with
  no fields, which has no storage) and those two functions.

`MnComponentInfo` gains the two functions, and `MnProgram` a
`MnResourceInfo` table and the version. The runtime finds a component or
tag by name in the component table, builds the query as generated code
builds one, and walks its tables and rows; formatting goes into a
growing buffer, so a row of strings and lists has no limit.

These functions are in every program, driven or not. The Outcome gives
the size of the generated C and the build time, release and debug, of
`tests/run/draw_unfit.12am` (500 components) and `examples/flock.12am`
before and after. If a release build of `draw_unfit` takes more than 1.5
times as long, the formatters become one walker in the runtime over a
generated table of fields, and the Outcome says so.

### The text `draws` and the `Float32` rule

The text form of `draws` takes the `Float32` rule too, so that one
implementation serves both forms: the two values ±7.038531e-26 print as
±7.0385309e-26. No expected file of the suite holds either.

### The compiler

- `src/main.rs`: `--format`, for the five commands, and in `USAGE`; the
  first line; `run` passes or removes `MIDNIGHT_FORMAT`.
- `src/diag.rs`: a JSON renderer beside `render`; positions from
  `src/source.rs`, which already counts lines and columns.
- `src/plan.rs`: `Transfer` keeps `from` and `to` as steps: a system and
  whether it is strict, or the top level. Both renderers read it.
- `src/loops.rs`: `Line` keeps its parts as data (where it stands, the
  terms and filters, the reason's text and its rule) for both renderers.
- `src/tir.rs`: `ReasonKind` gains the number of its rule, the number
  the reference's table gives it; nothing else in the contract changes.
- `src/driver.rs`: each of its messages keeps its parts apart (a C
  compiler failure its status, output and kept file).
- `src/emit`: the formatters, the tables and the version.
- `evals/eval.rb`: the wrapper passes `MIDNIGHT_FORMAT` and knows that
  `--format` takes a value; no eval runs in this spec, and
  `checks.evals` is otherwise unaffected.

### The runtime

- A JSON writer in `midnight_rt.c`: strings by the rules above, numbers
  by the rules above, objects and arrays into a `MnBuilder`, and an
  out-of-memory line that allocates nothing.
- Every line on standard error goes through one place that writes it as
  text or as JSON, so no printer writes text only. The pool's
  out-of-memory line is the one exception, written by `midnight_pool.c`
  itself.
- The drive leaves `#ifdef MN_WINDOW`: the reader of commands, the
  answers and errors, `tick` and the hooks at the start of a tick and at
  the end of the program are in every program. The window keeps its own
  commands and calls into the drive for them.
- The reader uses `getline`, as it does now; `midnight_rt.c` defines
  `_POSIX_C_SOURCE`. The rules of `CLAUDE.md` name it.
- `query` and `resource` use the generated tables; `query` calls
  `mn_gpu_use` on the query it builds, as generated top-level code does.
- `midnight_rt.h` changes: its block on what run-time errors print gains
  the JSON form; the drive becomes a section of every program; the JSON
  helpers the generated formatters call; `MnComponentInfo`,
  `MnResourceInfo` and `MnProgram`. `midnight_pool.h` gains the format
  for `mn_pool_start`.

### Tests

The suite splits a run's answers by their first line (`draws: …`,
`query …:`, `resource …:` or any other) and the two-space lines after
it: `.draws`, `.queries` and `.frames` each hold their own.

- A JSON reader in the suite (hand-written, as everything else), used by
  the tests below.
- **Two forms, one content.**
  - For every program of `tests/fail`, `check --format json` gives the
    diagnostics of its `.err`: the same messages, labels, notes, files,
    lines and columns, and the totals line.
  - For every program of `tests/plan` and `tests/loops`, the JSON gives
    the lines of its `.plan` or `.loops`.
  - For every program with a `.draws`, `.queries` or `.split` file, a run
    with `MIDNIGHT_FORMAT=json` gives the same answers and lines in JSON;
    in GPU mode, those with a `.stats` or `.frames` file too, as
    `gpu_stats_match_expected` and `window_frames` run them.
  - Every case of `drive_errors` and `drive_answers`, and every program
    of `tests/panic`, gives its errors and answers as JSON lines.
  - The `info` lines: the `threads` and `threads_split` lines beside
    `.split`, `gpu_device` where `gpu_programs` reads the device line, the
    window's lines in the window-variable test, and `unknown_word` for
    each variable.
  - Each line of every JSON output parses, has a `kind`, and the first
    is the `midnight` line.
- **Exact JSON.** `tests/json/NAME.12am` with `NAME.json`, which holds
  exactly what `check`, `plan` or `loops --format json` prints (its
  first comment says which), or, with a `NAME.drive`, the standard error
  of a headless run with `MIDNIGHT_FORMAT=json`. They write the first
  line's `version` as `VERSION`, which the test replaces with the
  compiler's before comparing. `UPDATE_EXPECT=1` rewrites them. They
  cover each kind of the compiler and each answer, with strings holding
  control characters and non-ASCII text, and a command line that is not
  UTF-8.
- **The compiler's messages.** A missing file, `MIDNIGHT_CC=false`, a
  directory that cannot be written and unset flags give their kinds.
  `run --format json` passes the variable, and its standard error is
  the compiler's lines, then the program's.
- **Standard output.** For every program of `tests/run` and `examples`,
  a run with `MIDNIGHT_FORMAT=json` prints its `.out` exactly; for every
  program of `tests/panic`, what it prints without the variable. Each
  one's standard error holds only JSON lines.
- **Queries.** `tests/run` programs with `.drive` and `.queries` files:
  rows over several tables, `with:` and `without:`, `from` within and
  past the count, more than 100 rows, strings, lists, nested structs,
  enums, entities, `Color` and `Float32` fields and `[Float32]`, NaN, a
  resource, a resource with no fields; a query after a despawn and after
  queued changes; a query at the end of a program and after `exit`.
  `UPDATE_EXPECT=1` rewrites `.queries` files from the release run.
  `thread_programs` holds them to the same answers for every thread
  setting (a program meant for it has a loop `midnight loops` prints as
  `cores` or `gpu`). In GPU mode `gpu_programs` holds them for every
  device: exactly, or, for a program with a `# tolerance: T` line, each
  `Float` and `Float32` within that bound, as `.draws` are; one has a
  component kept on the device and `stats` lines that count its
  download.
- **Without a window.** A windowless program driven through ticks,
  queries, the end of its commands and of the program; the window's
  commands as errors.
- **The tick rule.** A window program that ticks before `Window.open`
  waits for commands there. These change, their `.out` unchanged:
  `tests/run/input_keys.drive` gains a `tick` at the top for its
  `World.tick` before `Window.open`; `tests/run/input_focus.drive` gains
  two `tick` lines before the second window, for the two `World.tick`s
  after the first closes. The two `close` commands at the end of
  `drive_answers` become a case of `drive_errors`. In
  `only_programs_with_a_window_read_the_window_variables`, a windowless
  program with a missing `MIDNIGHT_DRIVE` now stops with status 1 at its
  end, and the test requires it to read `MIDNIGHT_DRIVE` and not
  `MIDNIGHT_WINDOW`. `on_cpu`, which gives every program of the suite a
  `.drive` file or an empty one, now has windowless programs read it.
- **Command errors** for `query` and `resource`, one for each message,
  and one where two apply.
- **The `Float32` rule.** `float32_text_round_trips` checks, with
  `MIDNIGHT_TEST_FLOAT32=1`, every finite 32-bit pattern in a release
  build, split across threads: each text reads back as the same value
  through `strtof` and through `strtod` rounded to 32 bits, and no text
  with fewer significant digits does both. Without the variable it checks
  every power of two, ±7.0385309e-26 and a fixed sample of a million
  patterns.
- **Untestable on demand.** `gpu_error`, `window_error`,
  `out_of_memory` and `buffers_alive` are held by a unit test of the
  writer that writes each.
- `--format` usage errors.
- `tests/COVERAGE.md` gains the commands, `MIDNIGHT_FORMAT` and the JSON
  forms.

### Slices

1. **The compiler.** `--format`, the JSON reader of the suite,
   diagnostics, messages, `plan`, `loops`, the cross-checks and the
   compiler's `tests/json`; `docs/json.md` begun.
2. **The runtime's JSON.** `MIDNIGHT_FORMAT`, the writer, one place for
   every line, every kind, the pool's line; the `Float32` rule and its
   test; the cross-checks of answers, stats, splits, info lines, drive
   errors and panics.
3. **Driving every program.** The drive out of `MN_WINDOW`, the tick
   rule, the window's commands without a window, windowless tests, the
   `.drive` files and tests listed above.
4. **`query` and `resource`.** The formatters and tables, the commands,
   their errors, the GPU read, `.queries` tests in every mode, the build
   cost measured.
5. **Documents.** The reference at 0.9 (the commands, the tick rule, the
   variable), `docs/json.md` complete, `docs/running.md` (and that
   `awake` threads keep polling while a driven program waits for a
   command), the README, `CLAUDE.md`, COVERAGE, the Outcome.

## Acceptance criteria

1. `midnight check --format json` on every program of `tests/fail` gives
   the diagnostics of its `.err`, as stated in Tests, with the first line
   and the totals line.
2. `plan --format json` and `loops --format json` give the lines of every
   `.plan` and `.loops` file.
3. Every line of every JSON form the suite produces parses as one JSON
   object with a `kind`, and the first line is the `midnight` line. Every
   `tests/json` file matches.
4. With `MIDNIGHT_FORMAT=json`, every program of `tests/run` and
   `examples` prints its `.out` exactly, every program of `tests/panic`
   prints on standard output what it prints without the variable, and
   each one's standard error holds only JSON lines.
5. The answers of every `.draws`, `.queries`, `.frames`, `.stats` and
   `.split` file, every case of `drive_errors` and `drive_answers`, and
   every run-time error of `tests/panic` are given in JSON with the same
   content.
6. A program without a window is driven: ticks, `query`, `resource`, the
   end of its commands and of the program, and the window's commands as
   errors.
7. No driven program runs a tick its commands did not let run, before
   `Window.open` and after a close included.
8. `query` and `resource` give the answers of every `.queries` file, in
   release and debug, for every `MIDNIGHT_THREADS` setting of
   `thread_programs`, and in GPU mode on the 5090, RADV and llvmpipe:
   exactly, or within the program's tolerance.
9. `float32_text_round_trips` passes with `MIDNIGHT_TEST_FLOAT32=1`.
10. Every suite, the GPU and window modes and `nix flake check` pass.
11. The Outcome gives the build cost of the formatters, within the bound
    in "What a program has to describe itself" or with the walker that
    replaces them.
12. The reference says 0.9 and `midnight --version` prints `midnight
    0.9.0`. `docs/json.md` documents every kind. The README's commands
    work as written. `tests/COVERAGE.md` has the commands and the JSON
    forms.

## Decisions

1. The long-lived session and `record` are left to later specs: a
   driven program is deterministic, so running it again with more
   commands shows what a session would.
2. Compiler JSON goes where its text goes; a program's JSON replaces
   every line it writes to standard error, chosen by
   `MIDNIGHT_FORMAT=json`.
3. A driven program runs no tick a command did not let run, window or
   not.
4. `query` takes the terms of an `each`; `resource` reads a declared
   resource.
5. At most 100 rows an answer, `from N` for the rest.
6. `Float32` as its shortest 32-bit text that a 64-bit reader also reads
   exactly, in both forms.
7. An entity in JSON is the text `to_s` gives it.
8. A query of a column on the device is a read by top-level code.
9. The JSON forms are documented in `docs/json.md`, the commands in the
   reference.
10. Every window command needs an open window; sent while none is open,
    it stops the program.
11. Formatters are generated for every component and resource, within a
    measured bound on build time.

## Measured in planning

- Every finite 32-bit value's shortest text, read as a 64-bit number and
  rounded to 32 bits, gives the same value, but for ±7.038531e-26, where
  the 64-bit reading falls between two 32-bit values and rounds the
  other way; ±7.0385309e-26 reads back exactly both ways. The runtime's
  own search of `%.*e` with `strtof`, tried at every power of two and on
  a sample of one pattern in 997, finds the same pair and no other.
- A `draws` answer with named members and colors as objects is about
  1.8 times the bytes of one with arrays of numbers.
