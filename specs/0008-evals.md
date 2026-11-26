# 0008 — Evals

- **Status:** approved
- **Depends on:** 0007

## Goal

Midnight is written mostly by agents (`docs/principles.md`). Whether a
construct, a diagnostic or a page of the reference works for them is a
question with an answer that can be measured. After this spec it is
measured: a set of tasks, each a program to write or to repair, is given
to agents that know Midnight only from its documentation, and a report
says how often they succeeded and what got in their way.

```sh
nix run .#eval -- run --models sonnet,haiku --trials 5
nix run .#eval -- report evals/results/DATE-COMMIT.json
nix run .#eval -- compare evals/results/A.json evals/results/B.json
```

A report answers, for each task and model: how many runs passed, how many
compiler calls they made and how many of those failed, what each
diagnostic and run-time error was and how often it came up, and which
mistakes are Ruby-shaped: a method, function or keyword of Ruby that
Midnight does not have. `compare` puts two results side by side, so a spec
that changes the syntax, a diagnostic or the reference can show its
effect, and `--kit` runs the same tasks with different documentation, so
a guide or a shorter reference can be measured against the full one.

The evals do not change the language. They are the evidence later specs
cite.

## Scope

- `evals/`: the tasks, each with a reference solution, other correct and
  wrong programs, and the checks that grade it; the documentation every
  agent is given; the harness that runs agents, grades their programs and
  writes results; committed results.
- The flake app `eval`, a check that runs the harness's tests and grades
  every task's programs, and a suite test that does the same with every
  build of the compiler.
- A baseline: one full run on the commit this spec ends at, committed,
  with a findings report.

## Non-goals

- Changing the language, its diagnostics or its reference. Findings become
  later specs.
- An agent-facing guide, `midnight fmt`, `--format json`, an MCP server.
  The evals are how those will be judged.
- Agents other than Claude Code as the harness starts it. Running the
  agent another way, with an API key or a local model, may be added later
  if it proves worth it. Each is a decision of its own, made in a spec
  that weighs what it would measure against what it costs and exposes.
- Judging programs by a model: style, idiom and readability scores.
- Timing agents' programs. A task can require where a loop runs, not how
  fast it is.
- Running agents in `nix flake check`: they need the network and an
  account, and their results vary from run to run.
- Captured frames: a task with a window is graded on what it prints.

## Design

### Layout

```
evals/
  README.md          how to run the evals, read a report and add a task
  preamble.md        the start of every task as the agent reads it
  kit                the documentation every agent gets, by default
  eval.rb            the harness
  test/              the harness's tests, their recorded inputs, test kits
  tasks/NAME/
    TASK.md          the task, after the preamble
    meta             key: value lines
    solution.12am    the reference solution
    expected.out     what a correct program prints, in full
    right/NAME.12am  other correct programs, written differently
    wrong/NAME.12am  programs the grader must reject
    given/           files copied beside TASK.md (optional)
    check.drive      the commands the grader runs a program with (optional)
    loops            where named loops must run (optional)
    systems          the systems the program must declare (optional)
  results/DATE-COMMIT[-LABEL].json   a run's results
  results/DATE-COMMIT[-LABEL].md     its report
  runs/              transcripts, call logs and programs, ignored by git
```

`meta` has one `key: value` per line: `areas:` (words from the table
below, separated by spaces), `control:` (`yes` or `no`), `ruby:` (one line
of free text: the Ruby forms the task invites that Midnight does not
have), and `minutes:` (the time limit of a run, 15 if absent).

A kit is a file of lines `SOURCE DEST`; blank lines and lines beginning
with `#` are skipped. `SOURCE` is a path from the repository's root; a
directory stands for every file under it, at the same relative paths under
`DEST`. `DEST` is a relative path under the run's `docs/` with no `..`.
`evals/kit` is:

```
docs/language.md language.md
README.md README.md
examples examples
```

### What an agent is given

Each run of a task gets three directories of its own, each made with
`mktemp -d /tmp/eval.XXXXXXXX` whatever `TMPDIR` says, none under the
repository: the **run's directory**, where the agent starts; the
**wrapper's directory**, holding only the `midnight` wrapper, first on the
agent's `PATH`; and the **log directory**, which only the wrapper writes.
The run's directory holds:

- `TASK.md`: `preamble.md`, a blank line, then the task's `TASK.md`.
- `docs/`: the kit's files.
- The files of the task's `given/`.

`preamble.md` says where the documentation is, that `midnight check FILE`
and `midnight run FILE` are on the PATH, that the program goes in
`solution.12am` in this directory, that the agent should run it to check
it before finishing, and that nothing outside the directory is available.
It names no construct of the language.

The agent is Claude Code, started in the run's directory as

```
claude -p --safe-mode --restricted --strict-mcp-config
  --disable-slash-commands
  --tools Bash,Read,Edit,Write,Glob,Grep
  --allowedTools "Bash(midnight *)" "Bash(MIDNIGHT_DRIVE=* midnight *)"
                 "Bash(MIDNIGHT_THREADS=* midnight *)"
                 Read Edit Write Glob Grep
  --permission-prompts none --no-session-persistence
  --output-format stream-json --verbose
  --model MODEL [--effort EFFORT]
  "Read TASK.md and do the task."
```

in its own process group, with an environment built from nothing but
`HOME`, `TMPDIR=/tmp`, `LANG=C.UTF-8`, `TERM=dumb` and `PATH`: the
wrapper's directory, then the directory of `claude`, then the flake's
coreutils, findutils, grep, sed and ripgrep. `--safe-mode` loads no
`CLAUDE.md`, memory, skill, hook or plugin of the account and leaves
Claude Code's authentication as it is; `--restricted` keeps the file tools inside the run's directory;
anything that would need a permission prompt is refused.

The harness reads the transcript's `system/init` line first. A run whose
tools are not exactly `Bash`, `Read`, `Edit`, `Write`, `Glob` and `Grep`,
whose `cwd` is not the run's directory, whose permission mode is not
`default`, or that lists an MCP server, a skill, a plugin or an agent, is
stopped, and so is the whole `run`, with a message: the flags no longer
isolate.

A tool result too large for the context is saved by Claude Code under
`~/.claude/projects/`, in a directory named after the run's directory, and
the agent is shown that path. Claude Code also makes a directory named the
same way under `/tmp/claude-UID/`. When the run ends, the harness removes
each of the two, and the directories under it, when no file is among
them, and leaves it otherwise.

### The wrapper

The agent's `midnight` is a Ruby script the harness writes for each run.
Ruby runs it with `--disable=gems,rubyopt`, and its first line sets the
load path to Ruby's own, so `RUBYOPT` and `RUBYLIB` load nothing. It runs
the compiler of the commit under test:

- with an environment built from nothing but: `PATH` (the flake's
  coreutils), `HOME` (an empty directory of its own), `TMPDIR` (an empty
  directory of its own in the log directory, so what a call stopped at
  the time limit leaves behind goes when the run ends),
  `XDG_RUNTIME_DIR` (another empty directory of its own),
  `MIDNIGHT_GPU=off`, `MIDNIGHT_WINDOW` (the caller's words with
  `headless` added), and, from the caller, `MIDNIGHT_THREADS` and
  `MIDNIGHT_DRIVE` only. `MIDNIGHT_CC`, `MIDNIGHT_CFLAGS`,
  `MIDNIGHT_GPU_FLAGS`, `MIDNIGHT_WINDOW_FLAGS` and every other variable
  the caller set are dropped, so the package's own values apply, and the
  call log records the names of those dropped;
- in the process group of its own, which it kills, with the program the
  compiler started, after 30 seconds, ending the call with status 124;
- with at most 1 MB of the call's standard output and of its standard
  error passed on and recorded, and the rest cut.

It refuses a call, with status 2 and one line on standard error:

- when the caller set a variable whose name begins `RUBY`, `LD_` or
  `GCONV`, which change how Ruby or the dynamic loader starts, except an
  `LD_LIBRARY_PATH` whose every directory is in `/nix/store/`, as Claude
  Code's launcher sets it;
- when a file argument, the argument of `-o`, or `MIDNIGHT_DRIVE` resolves
  outside the run's directory. A path is resolved a part at a time, as the
  kernel does: a symbolic link is followed where it stands, so a `..`
  after it leaves the link's target;
- when the named file or any `.12am` file under the run's directory has a
  line that begins with `require` whose path, resolved from that file's
  directory with `.12am` added, is outside the run's directory, or whose
  rest is not one string in either quote with at most a comment after it.

It appends one record per call to the call log in the log directory: the
time, the arguments, the variables passed and dropped, the SHA-256 of the
file argument and of the program (below), the load average, the exit
status or the refusal, and the standard error. The call log, not the transcript, is what grading reads
for compiler calls: a command the agent believes ran may have been
refused.

These guards are in the wrapper because permission patterns match more
than they appear to: `Bash(MIDNIGHT_DRIVE=* midnight *)` also allows a
command that sets another variable after it, `RUBYOPT` among them. A program
the wrapper accepts reads only files of the run's directory, its sources
and its file of commands; it reaches no network and no C library, and
with `MIDNIGHT_GPU=off` it captures no frame, so it writes nothing but its
output.

### Grading

The **program** of a run is the `.12am` files under the run's directory
that are not under `docs/`. Its hash is the SHA-256 of each file's path
and contents, in order of path.

When the agent finishes, or after the task's time limit, the harness
kills the agent's process group, keeps the program (below), copies it, at
the same relative paths, into a fresh directory with the task's
`check.drive`, and grades `solution.12am` there with the compiler
under test, in the wrapper's environment, with no `MIDNIGHT_THREADS`,
standard input from `/dev/null`, the wrapper's 30-second limit and 1 MB
cap, and one program graded at a time:

1. **Builds:** `midnight build solution.12am` exits 0.
2. **Output:** the program, run with `MIDNIGHT_DRIVE=check.drive` when the
   task has one, exits 0, and its standard output is `expected.out`, byte
   for byte. Standard error is not compared.
3. **Loops:** when the task has a `loops` file, each of its lines is
   `WHERE PLACE`, as in `system walk cores` or `strict gpu system settle
   gpu`. In what `midnight loops solution.12am` prints, columns are
   separated by two or more spaces; at least one line has `WHERE` as its
   second column, and the last column of every such line is `PLACE` or
   begins `PLACE:` (as `one core: prints at line 4` does).
4. **Systems:** when the task has a `systems` file, each of its lines is
   `NAME` or `NAME each`. One of the program's files declares the system
   (`system NAME`, `gpu system NAME` or `strict gpu system NAME`) on a
   line of its own, and with `each`, the system has a loop: a line of
   what `midnight loops solution.12am` prints has that declaration's words
   as its second column, or has `def F` there for a function `F` whose
   name the system's body names (the lines from the declaration to the
   next `end` at the start of a line).

A run **passes** when all its checks do. It **fails** when a check fails,
when there is no `solution.12am`, when the time limit ended it
(`timeout`, whatever Claude Code printed), or when the audit marks it.

A run that the harness did not end, and that ends because of the agent's
machinery and not the agent, is an **infrastructure error**: Claude Code
exits with no `result` line, or its `result` line has `is_error` true,
which an agent that finishes never sets: an API error, a rate limit, a
usage limit, a lapsed login. A usage limit is told by the result's text,
not by its status, which a rate limit shares. It counts neither as a pass
nor as a fail. `run` starts it again as the same trial, up to twice, and keeps
each attempt's transcript. A usage limit ends the runs in flight as
infrastructure errors that count no attempt, and stops the whole run.

**The audit.** A tool call **ran** when its tool result is not a
permission denial. For every call that ran, the harness takes as paths the
`file_path`, `path` and `pattern` arguments of the file tools, and the
words of a Bash command that begin with `/` or `~` or contain `..`: its
arguments, with redirections split off, the value after `=` of an option,
and the argument of an option that names a file (`-f`, `--file`,
`--exclude-from`, `--ignore-file`); not the program of `grep`, `rg`, `sed`
or `awk`, nor what `echo` and `printf` print. `cd` alone counts as `~`.
The file tools' paths are resolved against the run's directory; a Bash
word against the shell's directory when the call began, as Claude Code
keeps it: the directory a `cd` left, from call to call, unless a call's
result says the directory was reset. `~` and `~/` are the home directory,
and a word that begins with any other `~` is outside. A path that resolves
outside the run's directory, the wrapper's directory, `/dev/null`,
`/dev/stdout`, `/dev/stderr` and Claude Code's tool-result directory for
the run marks the run **escaped**, and so does a refusal in the call log.
An escaped run counts as failed, is listed by itself in the report, and is
a defect of the harness or the task to be fixed before results are used.

What the audit cannot follow puts the run on a list **for review** in the
report, without marking it: a word with `$` or a backquote; a relative
word after a `cd` to a directory it cannot know (`cd -`, a variable); a
`sed` program with an `r`, `R`, `w`, `W` or `e` command; an `awk` program
with `getline`, `system` or output to a file or a pipe; `rg --pre`;
`find -exec` and its kind; and a call the audit fails on.

### What is recorded

For each run the results file holds:

- task, model as asked, model as reported (the keys of the `result`
  line's model usage), effort as passed or `default`, trial, attempt, and
  Claude Code's version;
- `pass`, or the checks that failed, or `timeout`, `no solution`,
  `escaped` or `infrastructure`;
- the compiler calls by subcommand, and of those that did not exit 0, how
  many were compile errors, run-time errors, other non-zero statuses, time
  limits and refusals;
- each diagnostic: the first line after `error: `, its file and line; each
  run-time error: the text after `runtime error: `; and each other line of
  standard error that begins `midnight: `. Each has a **template**, the
  message with every backticked span replaced by `…`, and the list of
  those spans. A line saying the C compiler rejected the generated code is
  a compiler bug, and the report lists it by itself;
- for each diagnostic, whether one with the same template and the same
  spans came up again at a later call of the run (**repeated**);
- **first try**: whether the first `check`, `build` or `run` of
  `solution.12am` with content the agent wrote exited 0 (for a repair, the
  first with content that differs from the given file); **verified**:
  whether a `run` of `solution.12am` exited 0 when the program had the
  hash of the program graded;
- each denied tool call: the tool and its command or path;
- what the agent read of `docs/`: the lines of each file that a tool
  result returned. For `Read`, the range its result shows; for the `Grep`
  tool in content mode and for Bash `grep`, `sed -n`, `head`, `tail` and
  `cat`, the lines whose text the result holds, located in the file by the
  harness. A result that is an error, a spilled preview or a list of file
  names counts only the lines it shows;
- turns, duration, input, output and cache tokens, and the cost the
  `result` line reports, Claude Code's estimate at API prices; for a run
  with no `result` line, counted from the
  transcript's assistant messages and marked partial.

For each graded run it also records how long grading took, the load
average when it began, and whether the time limit stopped one of its
steps. When the run's sessions have finished, `run` grades each kept
program the time limit stopped in grading again, with nothing else
running, and keeps the second grade.

The results file also records the commit, Midnight's version, the Ruby
version, a hash of `preamble.md`, for each task a hash of `TASK.md`,
`meta`, `expected.out`, `check.drive`, `loops`, `systems` and `given/`,
for the kit each source path with its hash, and the store paths of the
agent's tools and of the compiler's `PATH`.

A diagnostic is **Ruby-shaped** when the name in its first backticked
span is Ruby's in that place and Midnight's is not:

- ``unknown method `N` for `T` `` or ``unknown field or method `N` for
  `T` ``: `N` is a public instance method, inherited ones included, of
  Ruby's `Array` when `T` is a list, `String` when `T` is `String`,
  `Integer` when `T` is `Int`, `Float` when `T` is `Float`, and `Object`
  otherwise;
- ``unknown method `N` for the type `T` ``: `N` is a public instance
  method of Ruby's `Class`, as `new` is;
- ``unknown function `N` `` or ``unknown variable or function `N` ``: `N`
  is a private or public instance method of `Kernel`, or a Ruby keyword
  (`nil` among them);
- ``unknown function `Math.N` `` or ``unknown constant `Math::N` ``: `N` is
  a module function or constant of Ruby's `Math`;
- ``unknown constant `N` `` or ``unknown type `N` ``: `N` is a class or
  module that `ruby --disable-gems` finds in `Object` with nothing
  required.

The harness takes the names from `ruby --disable-gems`, the flake's Ruby
3.4, and records each diagnostic's verdict when the run is graded. The
rule finds names only; a Ruby habit of syntax, such as a range or
a brace block, shows as its parse error in the table of templates.

Transcripts, call logs and the agents' programs are copied to
`evals/runs/RUN/TASK/MODEL-TRIAL-ATTEMPT/`, which git ignores, and the
`/tmp` directories are removed. Transcripts are not committed: they carry
account details and are large.

### The report

`report FILE` prints Markdown and `run` writes it beside the results:

1. A header: the commit, Midnight's, Claude Code's and Ruby's versions,
   the kit, the agent's tools, the models as reported, effort, trials,
   date.
2. For each model, **pass@1** over all tasks (the mean of the tasks' pass
   rates over graded runs) with its standard error taken across tasks, and
   the number of tasks **passed every time**. A task with fewer graded
   runs than trials is marked.
3. A table, one row per task and model: passes out of graded runs; passed
   every time; the mean of compiler calls, failed calls, turns, duration,
   output and input tokens, and cost (estimated); first-try and
   verified counts.
4. Diagnostics, run-time errors and other `midnight:` lines: one row per
   template, with the number of occurrences, the number of runs it came up
   in, how many of those runs passed, how many times it was repeated,
   whether it is Ruby-shaped, and its spans with their counts (as
   `insert ×3, map ×2`).
5. The reference as read: for each `##` and `###` section of each file in
   `docs/`, for the lines before a file's first such heading (below line
   1), and for each file with no headings, the number of runs that read
   any of its lines.
6. Compiler bugs, escaped runs, runs for review with what the audit could
   not follow, infrastructure errors, denied calls, runs that ended in
   `timeout` or `no solution`, calls the time limit ended while the load
   average exceeded the number of cores, and programs graded again alone,
   with both outcomes, if any.

`compare A B` prints each model's pass@1 in both and the difference, all
three over the tasks in both, with the standard error of the per-task
differences; then, for each task and model in both, the passes in A and
in B, marking a difference when Fisher's exact test, two-sided, on the two
pairs of counts gives p < 0.1, and saying under the table how many marks
chance alone would give: the sum over the rows of the chance of a mark
when both sides pass at the row's pooled rate; then each template whose
count changed, with both counts. It names the tasks whose hash differs,
and says when the models as reported, effort, trials, kit, preamble, the
agent's tools or Claude Code's version differ.

### Tasks

A task is a program to write, stated so that every correct program prints
exactly `expected.out`:

- Output is specified to the character.
- A task with `Float` either uses values whose every operation is exact
  (integers, and binary fractions such as halves and sixteenths, within 24
  bits in a GPU system and 53 elsewhere), or writes each formula as an
  expression the program is to compute in that order. Floats are printed
  rounded, with the rounding and the printing of `-0.0` stated.
- No task depends on `rand`, `clock`, or the order of anything the
  reference leaves unspecified, query order included. A task that prints a
  line per entity gives each entity a number, or states a total order, and
  asks for the lines in that order; a rule that picks one entity among
  several breaks ties by that number or order.
- `TASK.md` shows the first lines of the expected output, never all of it.
- `TASK.md` asks for what to build, in the domain's words, and names a
  language construct only where a check needs it (a system's name for
  `loops` or `systems`) or where the construct is the point (a GPU system,
  a window).
- A task whose point is entities, systems, events or structural changes
  says what is an entity and which systems the program has, by name, in
  the order they run, and has a `systems` file, so a program that does
  the work some other way fails. A line of `systems` has `each` only where
  `TASK.md` says the system goes over every entity of a kind with an
  `each`, in those words: a list of handles also goes over every entity,
  and a system that receives events may do its work in `on` alone, as the
  reference teaches. Controls name no construct.
- Every program of a task builds and runs in under 5 seconds on the
  development machine.
- The reference solution is written as the documentation teaches and is
  not given to the agent. Every task has in `right/` at least one other
  correct program, with its entities spawned in another order and in other
  tables and, where the task allows it, its formulas written differently.
  Every program in `wrong/` fails at least one check; a task with a
  `loops` file has one that prints `expected.out` and fails only `loops`,
  and a task with a `systems` file one that fails only `systems`.
- A **control** has `control: yes` and an empty `ruby:`: the way a Ruby
  programmer would write it is Midnight, and it prints only what does not
  depend on order (totals, counts, the state of one entity or resource).
  Controls show what the rest of the set costs agents beyond the language
  being new. A control's areas are those its program touches when written
  the way a Ruby programmer would; it cannot require them. Every other
  task names at least one form in `ruby:`.
- A **repair** gives a `solution.12am` in `given/` that does not pass, and
  asks for it to be fixed. It measures how well diagnostics and run-time
  errors lead to a fix.
- A task with `check.drive` gives a different file of commands in `given/`
  for the agent to try, using the same kinds of command. Its `TASK.md`
  fixes the window's size and says that the program ends when its window
  is closed.

The first set has thirteen tasks:

| Task | Program | Areas | Ruby forms invited |
| ---- | ------- | ----- | ------------------ |
| `thermostat` | Heaters warm a room toward a set point; one line of the room's state a tick | ecs, strings | (control) |
| `countdown` | One launch sequence of stages with holds and an abort; one line of its state a tick | ecs, strings | (control) |
| `league` | A league table from match events, sorted by points, goal difference and name | events, structs, lists, strings | `sort_by`, `<=>`, `insert`, `join` |
| `sandpile` | An abelian sandpile on a grid, toppled until stable after each grain | lists | `Array.new` with a block, ranges, `each_slice` |
| `warren` | Numbered rabbits that breed and foxes that eat them, by fixed rules with ties broken by number; counts a tick | ecs, structural | changes that take effect at once |
| `pantry` | Stock of named goods of a few kinds, orders as events, a report by kind in a stated order | enums, events, strings | `Hash`, `group_by`, `tally`, symbols |
| `orbits` | Two bodies under gravity by a stated integrator, formulas given, positions rounded | float, structs | `format`, `%`, `**` |
| `swarm` | Ten thousand particles in a GPU system, positions and velocities in sixteenths, counts per region of the box | gpu, float | `sum`, `count` in the GPU block |
| `ledger` | A GPU system whose results must be the same on every device | strict, gpu | `**`, `Math` functions |
| `crowd` | Two hundred thousand walkers; the system that moves them must run on several cores | cores, ecs | `sum`, `inject` |
| `paddle` | A paddle moved by the keyboard keeps a ball in play; hits and misses printed | window, input, ecs | `loop do`, `sleep` |
| `mend` | Repair: a program written as a Ruby programmer would, with five of the forms above | repair, lists, strings | the program's own |
| `fixup` | Repair: a program that breaks four rules of systems and queries | repair, ecs, structural | changes that take effect at once, an index in `each` |

`swarm`, `ledger` and `crowd` have `loops` files: a `gpu system`, a
`strict gpu system` and `cores` placement are what they test. `paddle`
has `check.drive`.

The set grows from mistakes agents make in real work: when an agent
writing Midnight for a spec, an example or a program trips on something no
task covers, a task for it is added.

### The harness

`evals/eval.rb` is Ruby, standard library only, run by the flake's Ruby.
`apps.eval` runs the working tree's `evals/eval.rb` from the repository's
root, and refuses to start elsewhere. Its commands:

- `run [--tasks A,B] [--models M,N] [--trials K] [--effort E] [--kit FILE]
  [--jobs J] [--label L]`: builds the compiler of the commit `HEAD` names
  (`nix build "git+file://$PWD?rev=COMMIT"`), takes the tasks and the
  kit's files from that commit (`git show COMMIT:PATH`), and refuses a
  `--kit` file that is not in it, so uncommitted changes change nothing
  that is measured. It refuses to start when `evals/eval.rb` differs from
  the commit, while `/tmp` or `/` holds an `AGENTS.md` or a `CLAUDE.md`
  (the run directories' parents), and refuses to write a results file that
  exists, saying to give `--label`. It runs every task, model and trial, grades, and writes
  `evals/results/DATE-COMMIT[-LABEL].json` after every run and the `.md`
  at the end. Defaults: every task, `sonnet,haiku`, 5 trials, Claude
  Code's default effort, `evals/kit`, 4 jobs.
- A usage limit stops `run` with a line that names the results file and
  the command that continues it.
- `run --resume RESULTS`: refuses unless `RESULTS` names the commit `HEAD`
  names and the same Claude Code version, runs every trial that has no
  graded result, infrastructure errors and isolation stops included, and
  writes to `RESULTS`.
- `report FILE` and `compare A B`, as above.
- `grade TASK FILE`: grades one program against one task.
- `grade --run RESULTS [TASK FILE]`: the same against the run's own copy
  of its tasks, under `evals/runs/`, with the compiler of its commit; with
  no `TASK FILE`, grades every kept program of the run and names each
  whose verdict differs from the recorded one.
- `self-test`: every reference solution and every program in `right/`
  passes; every program in `wrong/` fails; for a task with `loops`, one
  program in `wrong/` prints `expected.out` and fails only `loops`, and
  likewise for `systems`; a
  repair's given `solution.12am` fails; the output shown in `TASK.md` is a
  proper prefix of `expected.out`; every program of a task builds and runs
  in under 5 seconds; the reference runs with status 0 with each file of
  commands in `given/`.

`grade` and `self-test` use the compiler `MIDNIGHT_BIN` names, and build
it with `nix build` when it is unset: from the working tree, or for
`grade --run` from the run's commit. `claude` is
whatever is first on the caller's PATH; the results record its version.

The harness's tests, in `evals/test/`, use Ruby's bundled `minitest` and
need no network or account. They cover parsing call logs and transcripts
(recorded from real runs, plus transcripts of an API error and a usage
limit in the format of a real `result` line), templates, the Ruby-shaped
rule (every template it names is one the compiler's `tests/fail` print),
the audit, first try and verified, what was read, Fisher's test and the
chance of a mark, `report` and `compare` (two recorded results with a
known difference included), and the wrapper, called directly with no
agent.

### The suite

- `eval_solutions_pass` (in `tests/suite.rs`): every task's
  `solution.12am` and every program in `right/` builds and, run as grading
  runs it (window programs with `no_display`), prints exactly
  `expected.out` with status 0, in release and debug, and meets every
  `loops` and `systems` line; every program in `wrong/` fails a check.
  A change to the language that breaks a task's program breaks the suite,
  and the task is updated with it.
- The reference solutions of `swarm`, `ledger` and `crowd` also run in
  `thread_programs`, and those of `swarm` and `ledger` in `gpu_programs`.
- `checks.evals` runs the harness's tests and `self-test` in the build
  sandbox, with `MIDNIGHT_BIN` set to the package.

### When the evals run

- Before and after a spec that changes the syntax, a diagnostic or the
  reference, on the same models, effort, kit and trials, with the
  comparison in the spec's Outcome. Where a choice of syntax rests on the
  comparison, 10 trials.
- When a task is added or changed: the task's runs on the commit that adds
  or changes it, committed in the next commit.

The repository's `CLAUDE.md` gets `nix run .#eval` in its Commands,
`eval_solutions_pass`, `checks.evals` and the layout of `evals/tasks/` in
its Tests, and when the evals run in its Rules. The README gets a
paragraph on the evals that names no task and no rule of grading, since
every agent reads it.

### Measured in planning

A probe ran Claude Code 2.1.327 headless with `--safe-mode --restricted`,
the six tools and the two `Bash` rules above, on two small tasks with
Sonnet and Haiku:

- The file tools refused paths outside the directory, `ls` of a path
  outside was denied, and the session loaded no instructions or memory,
  with authentication working as usual.
- A compound command is judged as a whole: `midnight check t && ls ..` was
  denied and nothing ran, while the agent reported the first half as run.
- Read-only commands inside the directory ran without a rule, and a prefix
  rule on one variable also matched a command that set two.
- The working directory's path was shown to the agent.
- The reference is too large for one tool result: `cat` of it was spilled
  to a file and the agents went on with `sed -n` and `grep`.
- An agent not asked to run its program did not run it.
- A session of a small task took 20 to 35 seconds and 8 to 13 turns.

### Slices

1. **Harness and three tasks.** `eval.rb` with `run`, `grade`,
   `self-test`, the wrapper, the audit, the results file; `thermostat`,
   `league`, `warren`; the harness's tests; `apps.eval`. A run of the three
   tasks with one trial per model, read by hand against the transcripts.
2. **Report and comparison.** `report`, `compare`, the Ruby-shaped rule,
   what was read.
3. **The other ten tasks**, each reviewed adversarially before it is run,
   with `eval_solutions_pass` and `checks.evals`.
4. **Baseline and documents.** The isolation run; the full run on the
   final commit, committed with its report; the transcript audit;
   `evals/README.md`; the findings in the Outcome; README, `CLAUDE.md`.

## Acceptance criteria

All run on x86_64-linux, on the development machine.

1. `nix flake check` passes, `checks.evals` included, and `cargo test`
   passes with `eval_solutions_pass`.
2. `self-test` passes, every rule it checks included.
3. Every task has `TASK.md`, `meta`, `solution.12am`, `expected.out` and a
   program in `right/`; the thirteen tasks cover every area in the table;
   two are controls and two are repairs; every task that is not a control
   names a form in `ruby:`.
4. Isolation.
   1. The harness's tests call the wrapper directly with each of:
      `MIDNIGHT_CC`, `MIDNIGHT_CFLAGS`, `MIDNIGHT_GPU_FLAGS` and
      `MIDNIGHT_WINDOW_FLAGS` set; a file argument, an `-o` argument and a
      `MIDNIGHT_DRIVE` outside the run's directory, by an absolute path and
      by `..`; a program that requires a file outside, in each quote,
      with and without a space after `require`, by an absolute path, by
      `..`, and through a symbolic link followed by `..`; a require the
      guard cannot read; `RUBYOPT`, `RUBYLIB`, another `RUBY` variable,
      `LD_PRELOAD`, an `LD_LIBRARY_PATH` with a directory outside the Nix
      store, and `GCONV_PATH`, set after a `MIDNIGHT_` variable, with a
      file beside the program that Ruby would load; `MIDNIGHT_GPU=on` and
      `MIDNIGHT_WINDOW=`. Each is refused or overridden, no file the test
      planted is loaded, and the call log says so.
   2. A task in `evals/test/` whose `TASK.md` asks the agent to try each of
      those, to read the repository's `CLAUDE.md`, to list the home
      directory, to print `$PATH`, `$HOME` and the environment, and to
      open a window, runs once with `haiku`, with a file holding a random
      token placed beside the run's directory and in the repository. The
      token appears nowhere in the transcript, the call log or the agent's
      files; no path of the repository appears in the transcript; the
      `system/init` line lists only the six tools and no MCP server; and
      the audit marks the run escaped exactly where the call log shows a
      refusal.
   3. A run with `haiku` whose `TASK.md` asks only for the one-word reply
      `done`, with an `AGENTS.md` in the run's directory that asks for a
      random token instead, replies `done`: Claude Code loads no
      `AGENTS.md` of the run's directory into an agent.
5. `grade --run` of the baseline's results gives the recorded verdict for
   every kept program.
6. The baseline: every task, `sonnet` and `haiku`, 5 trials, committed in
   `evals/results/` with its report; no run escaped. A fresh agent that
   has not seen the tasks' design reads every failed run's transcript and
   one passed run of each task and model, and reports any answer reached
   from outside the run's directory and any correct program the grader
   rejected; there are none, or the harness or the task is fixed and the
   baseline run again.
7. `compare` of the baseline with a run of three tasks, one trial per
   model, with a kit in `evals/test/` that gives a committed copy of the
   reference with one section cut, prints both runs' counts for those
   tasks and names the kit's difference.
8. The harness's tests pass; nothing in `evals/` other than `runs/` holds
   a path of the development machine or a transcript with account details
   (the tests' recorded transcripts have their ids zeroed); the README's
   paragraph names no task and no rule of grading.
9. The Outcome has the findings: the diagnostics agents met most, the
   Ruby-shaped mistakes, the tasks no model passed every time, how agents
   read the reference, and the time, tokens and estimated cost of the full
   run, and how many times a usage limit stopped it.

## Decisions

- **Exact output.** Midnight's output is the same on every run, so the
  grader can be a comparison, with no model judging and no tolerance.
- **The call log over the transcript.** What the compiler was asked and
  answered is recorded where the agent cannot reach or misreport it.
- **Guards in the wrapper.** Permission patterns are a first line; what
  must hold (the run's directory, no display, no GPU, a time limit) is
  enforced by the program the agent calls.
- **Claude Code as the agent, under `--safe-mode`.** It is how Midnight is
  written. `--safe-mode` loads no instructions, memory or hooks and leaves
  Claude Code's authentication as it is.
- **Tasks in the public repository.** They change with the language, and
  anyone who clones it can run them. The agent has no tool that reaches
  the network, and the wrapper and the audit keep it to its directory, so
  an agent cannot look an answer up; what public tasks risk is a future
  model having trained on them, which a held-out set can answer when it
  matters.
- **Repairs and controls in the set.** A repair measures a diagnostic
  directly; a control separates what is hard about Midnight from what is
  hard about any new language.
- **Ruby for the harness.** Standard library only; it parses the
  transcripts with `JSON`, and it supplies the names that make a mistake
  Ruby-shaped from Ruby itself.
- **Sonnet and Haiku by default, five trials.** A weaker model shows where
  the language or its documentation is unclear sooner than a stronger one;
  five trials separate a task that passes every time from one that
  usually does. A full run is 130 agent sessions; the probe's small tasks
  took 20 to 35 seconds each, and the baseline's Outcome records the time
  and cost of a full run.
- **Results committed, transcripts not.** A result is evidence a spec
  cites, and lives with the commit it measured.
