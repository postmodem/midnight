# JSON output

Midnight's tools write text for people and agents at a shell. Each can
also write JSON lines, for programs that read what Midnight says: the
compiler with `--format json`, a built program with `MIDNIGHT_FORMAT=json`.
Both forms are made from the same data and say the same thing; a JSON
line may say more (a byte offset, an end position, the filters of a
loop), never less. A program written in Midnight never needs this page.

## Stability

A reader skips a kind it does not know and members it does not know.
Within one `format`, a later version may add kinds, optional members, and
values of a string member that lists alternatives (such as `runs`), and
changes nothing else: a member is never removed, renamed or given another
type, and its meaning does not change. Each kind has one shape: what a
line says is told by its kind, never by which members it has. Anything
else raises `format`.

## Lines

A JSON form is a sequence of lines, each one JSON object (RFC 8259) in
UTF-8, ended by `\n`, with no newline inside it. Each object has a string
member `kind` that says what it is, and comes first; the other members
follow in the order this page lists them, with no space between tokens.

- **Keys** are lower case with underscores, in the words of the reference.
  A key that names a component or a field is its name as declared.
- **Strings** escape `"` and `\` with a backslash, write `\n`, `\r` and
  `\t` as such, every other byte below 0x20 and 0x7f as `\u00xx` (lower
  case), and every other character as itself. A byte of a command line or
  a file name that is not part of valid UTF-8 is written as U+FFFD.
- **Optional members** are absent when they have no value, never `null`.
  Every other member is always present. An array may be empty.

The first line of every JSON form, from the compiler and from a program,
is

```json
{"kind":"midnight","format":2,"version":"0.11.0"}
```

`format` is the version of the forms on this page, an integer; `version`
is the compiler's, the one that built the program. `format` 2 came with
Midnight 0.11, when a `draws` answer began to list a page of a frame's
draws rather than all of them.

### Numbers

- Counts, sizes, line numbers, ticks and bytes are integers.
- An `Int` is its exact decimal digits. A reader that keeps numbers in a
  64-bit float (JavaScript's `Number`) reads values beyond ±2^53
  inexactly.
- A `Float` is the text `to_s` gives it, a JSON number for every finite
  value. `NaN`, `Infinity` and `-Infinity` are the strings `"NaN"`,
  `"Infinity"` and `"-Infinity"`.
- A `Float32` is the shortest text that reads back as the same 32-bit
  value, whether read as a 32-bit number or as a 64-bit number rounded to
  32 bits; non-finite values as for `Float`. It differs from the shortest
  32-bit text for two values only, `7.038531e-26` and its negative, which
  a 64-bit reader would round to the next 32-bit value; they are written
  `7.0385307e-26` and `-7.0385307e-26`. The text forms of `draws`, `query`
  and `resource` use the same rule.

A `Color` is an array of its four bytes, `[r, g, b, a]`. An entity is the
string `to_s` gives it: `"Entity(3v1)"`, `"Entity(none)"`.

## The compiler

`--format json` is an option of `check`, `build`, `run`, `plan` and
`loops`. The JSON goes where the text goes: diagnostics and the
compiler's own messages on standard error, `plan` and `loops` on standard
output. The first line comes before anything else, on standard error for
`check`, `build` and `run`, on standard output for `plan` and `loops`; a
`plan` or `loops` that finds errors writes it to both. Exit statuses are
those of the text form.

### `error`

One line per diagnostic, in the order of the text, at most 20:

```json
{"kind":"error","message":"unknown component `Postion`","label":"did you mean `Position`?","file":"move.12am","start":{"line":14,"column":8,"byte":312},"end":{"line":14,"column":15,"byte":319},"notes":[]}
```

| Member    | Value |
| --------- | ----- |
| `message` | the diagnostic's one line |
| `label`   | optional: the text after the marker |
| `file`    | the file's name as the text prints it, `<built-in>` included |
| `start`, `end` | the offending text: `line` from 1, `column` from 1 in Unicode scalar values (as the text counts them), `byte` from 0 into the file; `end` is just past the text, and equal to `start` for an empty span. The text marks only the first line of a span; these give all of it. |
| `notes`   | an array of `{"message", "file", "start", "end"}`, in order; a note may be in another file |

### `errors`

Follows the diagnostics whenever there is one:
`{"kind":"errors","count":23,"shown":20}`. `count` is the number of
diagnostics, `shown` the number of `error` lines; the text says `note: N
more errors not shown` when they differ. A `check` that finds no error
writes the first line and nothing else.

### The compiler's own messages

| Kind | Members | Text |
| ---- | ------- | ---- |
| `cant_read` | `file`, `message` (the system's reason) | ``midnight: can't read `FILE`: …`` |
| `no_flags` | `variables` (an array of the variables not set), `affects` (`gpu_systems` or `window`), `message` (the text's line after `midnight: `) | the line that says what the program is built without |
| `c_compiler` | `compiler`, `status` (optional: absent when it did not start), `output` (what it wrote on standard error, one string), `kept` (optional: the generated C's path, when it ran and failed), `message` (optional: the system's reason, when it did not start) | the C compiler's failure |
| `cant_write` | `file`, `message` | ``midnight: can't write `FILE`: …`` |
| `build_directory` | `directory`, `message` (optional: absent when every name tried was taken) | ``midnight: can't create a build directory in `DIR` …`` |
| `cant_run` | `message` | `midnight: can't run the program: …` |

### `transfer`

`midnight plan`: one line per transfer, in the order of the text.

```json
{"kind":"transfer","direction":"download","component":"Position","from":{"system":"move","strict":false},"to":{"top_level":true},"rows":false}
```

| Member      | Value |
| ----------- | ----- |
| `direction` | `upload` or `download` |
| `component` | the component's name |
| `from`, `to` | `{"system": NAME, "strict": BOOL}`, or `{"top_level": true}` for the top-level code |
| `rows`      | true when only the rows concerned move |
| `on`        | optional: the structural change the transfer happens because of: `spawn`, `despawn`, `add` or `remove` |

### `loop`

`midnight loops`: one line per loop, in the order of the text.

```json
{"kind":"loop","file":"bounce.12am","line":90,"in":{"def":"render"},"terms":["Position","Glyph"],"with":[],"without":[],"runs":"one_core","reason":"uses a `String` at line 93","rule":1}
```

| Member    | Value |
| --------- | ----- |
| `file`, `line` | where the `each` is |
| `in`      | `{"system": NAME, "gpu": BOOL, "strict": BOOL}`, `{"def": NAME}` (`Vec2.length` for a method), or `{"top_level": true}` |
| `terms`   | the terms' names in order; the text prints them after `each `, joined by `, ` |
| `with`, `without` | the filters' names, which the text does not print |
| `runs`    | `gpu`, `cores` or `one_core` |
| `reason`  | optional: for `one_core`, the text after `one core: ` |
| `rule`    | optional: for `one_core`, the number of the rule of an independent `each` it breaks, as "Several cores" in the reference numbers them |

## Programs

A built program reads `MIDNIGHT_FORMAT` before any other variable. With
`json` it writes the first line before anything else, and from then on
every line it writes to standard error is a JSON object; `text` (the
default) keeps the text, and any other value is reported once and the
program writes text. Standard output is the program's, byte for byte the
same in both forms. `midnight run --format json` sets the variable for
the program it starts, and `--format text` takes it away; the program's
lines then follow the compiler's, beginning with the program's own first
line.

Standard output is written out before each line on standard error, so
when both go to one pipe the lines arrive in the order they happened. A
line on standard error that does not begin with `{` was written by
something other than Midnight, such as a sanitizer or a driver.

A JSON line has no prefix: the `midnight: TOPIC:` of the text is its
kind.

| Kind | When | Members |
| ---- | ---- | ------- |
| `runtime_error` | a run-time error | `file`, `line`, `message`: the text's three parts |
| `gpu_error` | `midnight: GPU error: …` | `message` |
| `window_error` | `midnight: window error: …` | `message` |
| `out_of_memory` | `midnight: out of memory` | none |
| `buffers_alive` | a debug build's buffers at exit | `count` |
| `unknown_word` | a word a variable does not have | `variable`, `word` |
| `gpu_device` | `MIDNIGHT_GPU=info`, the device | `device` (optional: absent for no device) |
| `gpu_strict` | `MIDNIGHT_GPU=info`, the strict check | `passed`, `difference` (optional: the device and the first difference, when it failed) |
| `gpu_cpu_block` | `MIDNIGHT_GPU=info`, a block on the CPU | `system`, `each`, `reason` |
| `threads` | `MIDNIGHT_THREADS=info`, at start | `threads` |
| `threads_split` | `MIDNIGHT_THREADS=info`, a loop's first split | `file`, `line`, `rows`, `pieces`, `threads` |
| `window_opened` | `MIDNIGHT_WINDOW=info` | `width`, `height`, `pixel_width`, `pixel_height`, `shown` (`headless`, `wayland`, `x11` or `cocoa`), `present` (optional: `mailbox`, `immediate` or `fifo`; absent headless), `driven` |
| `window_event` | `MIDNIGHT_WINDOW=info`, a shown window | `event`: `focused` or `pointer_inside` |
| `stats` | `MIDNIGHT_GPU=stats` | `tick` (the tick the line counts), `uploads`, `downloads`, `rows_up`, `rows_down`, `submissions`, `waits` |
| `answer` | each drive command carried out | below |
| `drive_error` | a command that stops the program | `file`, `line`, `message` |
| `drive_unopened` | a file of commands that can't be opened | `file`, `message` |
| `ready` | a program in a session waits for a command | `tick`, `output_bytes`, `ended` |
| `refused` | a command refused in a session | `command`, `line`, `message` |

`ready` and `refused` are the lines of a [session](language.md#a-session):
`output_bytes` counts every byte the program has written to standard
output, and `ended` is true once the program has reached its end.
`refused` has the command as written and the message of the error a
file would report, without the file and the line.

```json
{"kind":"ready","tick":10,"output_bytes":117,"ended":false}
{"kind":"refused","command":"query Bal","line":3,"message":"query: `Bal` is not a component or tag of this program; it has Ball"}
```

### `answer`

Every answer has `name` (the command's first word), `command` (the line
as written, the spaces around it removed), `line` (its line in the file,
or on standard input in a session, empty lines and comments counted)
and `tick` (the value of `Time.tick` when it is answered: one more than
the tick that last ended), then members by command:

| Command | Members |
| ------- | ------- |
| `tick`, `tick N` | `ran` (the ticks that ran), `ended` (true when the program ended first) |
| `resize`, `scale`, `close`, `quit`, the keyboard and mouse commands | none |
| `capture FILE` | `file`, `frame_tick`, `pixel_width`, `pixel_height`, `crc` (eight hex digits, as the text) |
| `draws`, `draws from N` | `frame_tick`, `width`, `height`, `background` (a color, with the `a` the program gave it), `count` (the draws in the frame, all of them), `from` (the first draw listed), `draws`: an array of at most 100 shapes |
| `query …` | `count` (the entities that match, all of them), `from` (the first row listed), `rows`: an array of at most 100 rows |
| `resource NAME` | `resource` (the name), `fields`: the resource as an object |

A shape is an object with `shape` (`rect`, `circle`, `line`, `triangle`
or `text`) and its arguments in order under the names of the reference's
table (`x`, `y`, `width`, `height`, `radius`, `x1` … `y3`, `text`,
`size`), each number a `Float32` and `text` a string, then `color`. A
string of any length is written whole.

```json
{"kind":"answer","name":"draws","command":"draws","line":2,"tick":1,"frame_tick":1,"width":64,"height":48,"background":[10,20,30,255],"count":2,"from":0,"draws":[{"shape":"circle","x":10.0,"y":10.0,"radius":2.25,"color":[4,5,6,7]},{"shape":"text","x":4.0,"y":4.5,"text":"SCORE 00040\nLIVES 3","size":24.0,"color":[255,255,255,255]}]}
```

A row of `query` is an object: `entity`, then a member for each
component term, in the order written, named by the component. A
component, struct or resource value is an object of its fields in
declared order (`{}` for none); a list is an array; an enum is its
variant's name as a string; a `String` is a string; `Bool` is `true` or
`false`; numbers, colors and entities as in [Numbers](#numbers).

```json
{"kind":"answer","name":"query","command":"query Body","line":3,"tick":4,"count":100000,"from":0,"rows":[{"entity":"Entity(0v1)","Body":{"x":12.5,"y":40.25,"vx":1.0,"vy":0.5}}]}
{"kind":"answer","name":"resource","command":"resource Score","line":4,"tick":4,"resource":"Score","fields":{"points":30,"best":30}}
```

In an answer, `tick` is `Time.tick` when it is answered: one more than
the tick that last ended, or, after `exit` in a system, the tick in
progress. In a `stats` line, `tick` is the tick the line counts.

## Sessions

`midnight session start`, `stop` and `list` and `midnight drive` take
`--format text|json` for their own lines, as the compiler does: with
`json` each writes the first line first, on standard error, or on
standard output for `session list`, whose lines go there. `session start
--format json` also makes the session's program write JSON, and passes
its lines on after the compiler's, beginning with the program's own first
line, as `run --format json` does. `drive` passes the program's lines on
in the session's form, whatever its own.

| Kind | When | Members |
| ---- | ---- | ------- |
| `session` | a session started or stopped | `name`, `event` (`started` or `stopped`), `status` (optional: for `stopped`, the program's status, or 128 plus the signal that ended it) |
| `session_listed` | `midnight session list`, one per session | `name`, `file` (as `session start` was given it), `tick`, `ended` |
| `session_error` | a message of `session` or `drive` | `message`: the text's line after `midnight: ` |

```json
{"kind":"session","name":"catch","event":"started"}
{"kind":"session_listed","name":"catch","file":"examples/catch.12am","tick":10,"ended":false}
{"kind":"session","name":"catch","event":"stopped","status":0}
{"kind":"session_error","message":"no session is running; start one with `midnight session start FILE`"}
```
