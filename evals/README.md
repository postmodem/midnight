# Evals

The evals measure how well agents write Midnight when all they know of it
is its documentation. Each task is a program to write or to repair. The
harness gives it to Claude Code in a directory of its own, grades the
program the agent leaves, and writes a results file and a report: how
often each model passed, which compiler calls failed and why, which
mistakes are Ruby-shaped, and which parts of the reference the agents
read. `specs/0008-evals.md` is the design; this file is how to use it.

## Running

Everything runs from the repository's root, with `claude` logged in and
on the PATH:

```sh
nix run .#eval -- run                                   # every task, sonnet and haiku, 5 trials
nix run .#eval -- run --tasks league,warren --models haiku --trials 1 --label try
nix run .#eval -- run --resume evals/results/DATE-COMMIT.json
nix run .#eval -- report evals/results/DATE-COMMIT.json
nix run .#eval -- compare evals/results/A.json evals/results/B.json
```

`run` measures the commit `HEAD` names, never the working tree: it builds
that commit's compiler and takes the tasks, the preamble and the kit from
it. It refuses to start when `evals/eval.rb` has uncommitted changes, and
refuses to overwrite a results file; `--label` names a second run of the
same commit. Its options:

| Option | Default | Meaning |
| --- | --- | --- |
| `--tasks A,B` | every task | the tasks to run |
| `--models M,N` | `sonnet,haiku` | the models, as `claude --model` takes them |
| `--trials K` | 5 | runs of each task by each model |
| `--effort E` | Claude Code's | passed as `claude --effort` |
| `--kit FILE` | `evals/kit` | the documentation the agents get |
| `--jobs J` | 4 | sessions at a time |
| `--label L` | none | added to the results file's name |

A full run is 130 sessions. It writes `evals/results/DATE-COMMIT[-LABEL].json`
after every session and the report, the `.md` beside it, at the end. A
usage limit stops it with the command that continues it (`--resume`),
which runs every trial that has no grade yet. An API error or a lapsed
login is an infrastructure error: the trial runs again, up to twice, and
counts as neither a pass nor a fail.

Transcripts, call logs and every program the agents left are kept under
`evals/runs/`, which git ignores: with each run's call log, `programs/`
holds the program as it was at every `check`, `build` and `run`, one
directory for each hash the call log records, and `graded.out` and
`graded.draws` what the graded program printed. Transcripts carry account details: they
are not committed.

### A kit

A kit is a file of `SOURCE DEST` lines, `SOURCE` a path from the
repository's root (a directory stands for every file under it), `DEST` a
path under the agent's `docs/`. `evals/kit` gives the reference, the README
and the examples. To measure other documentation, commit a kit and its
files and pass `--kit`; `evals/test/kit-no-structural/kit` is one, the
reference with a section cut.

## What an agent gets, and what keeps it there

Each session runs in a new directory under `/tmp` holding `TASK.md` (the
preamble and the task), `docs/` and the task's given files. Claude Code
runs with `--safe-mode --restricted`, six tools, and permission rules that
allow `midnight` commands and nothing that needs a prompt. Its `midnight`
is a wrapper that runs the compiler under test with an environment of its
own, no GPU, no display, a 30-second limit, and only files inside the
directory. Every call is logged where the agent cannot write, and grading
reads that log, not the transcript.

After each session the audit checks every tool call that ran for a path
outside the directory. A run that reached outside is **escaped**: it
counts as failed and is a defect of the harness or the task, to be fixed
before the results are used. What the audit cannot follow (a `$` in a
command, a `cd` to an unknown place) puts the run on a list for review,
and so does a call the wrapper refused: nothing ran, and the run is
graded on its program.

```sh
nix run .#eval -- isolation
```

runs the two tasks in `evals/test/tasks/` once each with `haiku`: one asks
the agent to try every way out the wrapper guards against, to find the
repository, to list the home directory and to print the environment; the
other gives it an `AGENTS.md` that contradicts its task. A random token
sits in a file beside the run's directory and in the repository. It
passes when the token reaches nothing the agent wrote or read, no path of
the repository appears in the transcript, every path the audit finds is
in a call the wrapper refused, and the agent ignores the `AGENTS.md`. Run it after
upgrading Claude Code, and whenever `run` stops with "the flags no longer
isolate".

## Reading a report

1. **Header**: the commit, the versions, the kit, the agent's tools, the
   models as Claude Code reported them.
2. **Pass rates**: pass@1 is the mean of the tasks' pass rates, with its
   standard error across tasks. Five trials make it coarse: a change
   worth ten points will not show in it.
3. **Tasks**: per task and model, the passes, the means of compiler calls,
   failed calls, turns, time, tokens and estimated cost, and two counts:
   **first try** (the first compile of the agent's own program succeeded)
   and **verified** (the agent ran exactly the program that was graded).
4. **Diagnostics and run-time errors**: one row per template (the message
   with each backticked name replaced by `…`), how often it came up, in how
   many runs, how many of those passed, how often an agent got the same
   error again, and whether it is Ruby-shaped: the name is one Ruby has
   in that place and Midnight does not.
5. **The reference as read**: for each section of each documentation
   file, how many runs read any of it.
6. **Draws**, when a task has the check: per task and model, the runs
   that built and failed it, by how their answers first differ (an
   answer missing, the frame, a shape missing or too many, the order, or
   a shape that differs); then the runs whose first differing shape is
   the expected one with two numbers exchanged, and those whose first
   differing shape is the expected rectangle moved by half its size.
7. Compiler bugs, escaped runs, runs for review, infrastructure errors,
   denied tool calls, timeouts, and programs graded again alone.

The finer measures (failed calls per run, first try, how often a
diagnostic comes up and repeats) show smaller effects than the pass rate.

`compare A B` puts two runs side by side: pass@1 for each model in both,
the passes per task, with a mark where Fisher's exact test gives p < 0.1
and how many marks chance alone would give, and the passes and graded
runs of each over all the rows with Fisher's test on them, and every
template whose count changed. It names tasks whose text differs and any difference in models,
effort, trials, kit, preamble, tools or Claude Code's version.

## Grading by hand

```sh
nix run .#eval -- grade TASK FILE            # one program against the working tree's task
nix run .#eval -- grade --run RESULTS        # every kept program of a run, against its own snapshot
nix run .#eval -- self-test [TASK...]        # every task's own programs
```

A program passes when it builds, prints `expected.out` byte for byte with
status 0, gives the `draws` answers of `expected.draws` where the task has
one, and meets the task's `loops` and `systems` files. The check **draws**
reads every answer of `draws` in the standard error of the run that
printed the output, and compares what each draws, not its text: the
frame's tick, size and background, then the shapes, each as its kind,
its numbers as 32-bit floats and its colour. With `draws: in order` in
`meta` the shapes are compared in order; with `draws: any order`, as a
set with counts. It fails when standard error was cut at 1 MB. `grade` and
`self-test` use the compiler `MIDNIGHT_BIN` names, or build the working
tree's.

## Adding a task

A task is a directory under `evals/tasks/`:

```
TASK.md          the task, after the preamble
meta             areas:, control:, ruby:, minutes:, draws:
solution.12am    the reference solution, never shown to agents
expected.out     what a correct program prints, in full
right/*.12am     other correct programs, written differently
wrong/*.12am     programs the grader must reject
given/           files placed beside TASK.md (optional)
check.drive      the commands grading runs a window program with (optional)
expected.draws   every `draws` answer of check.drive, as the suite's .draws files (optional)
loops            `WHERE PLACE` lines the program must meet (optional)
systems          `NAME` or `NAME each` lines, in the order they run, and `others after` (optional)
```

The rules are in the spec's "Tasks" section. In short: output is fixed to
the character and depends on nothing the reference leaves open; `TASK.md`
shows only the first lines of it; it asks for what to build in the
domain's words, and names a construct only where a check needs it or the
construct is the point; a task about entities, systems or events names
the entities and the systems, in order, and has a `systems` file, whose
last line `others after` says, where `TASK.md` does, that any other
system runs after them; a `wrong/` program fails only `loops`, one only
`systems`, and one only **draws**, where the task has those files. A task that draws has the
area `draw` and `expected.draws`, and `meta` says `draws: in order` or
`draws: any order`.

Before it is run, a new task gets an adversarial review: a fresh agent
looks for correct programs it would reject, wrong ones it would pass,
and wording two agents would read differently. Then:

```sh
nix run .#eval -- self-test NAME
nix develop -c cargo test eval_solutions_pass
```

and, on the commit that adds the task, one run per model
(`run --tasks NAME --trials 1 --label NAME`), whose results are committed
in the next commit. A change to the language that breaks a task's program
breaks `eval_solutions_pass`, and the task changes with it.

## When the evals run

- Before and after a spec that changes the syntax, a diagnostic or the
  reference, with the same models, effort, kit and trials, and the
  comparison in the spec's Outcome; 10 trials where a choice of syntax
  rests on it.
- When a task is added or changed, as above.

## What a result decides

- A spec that runs the evals says, before the first run, which result
  would change its decision. If none would, the runs are a regression
  check only, and the Outcome says so.
- A worse result is not a veto by itself. The evals measure how agents
  write a program, not how easily a person reviews it or whether it is
  right beyond the task's checks, and a change is weighed on those as
  well. The Outcome says what was weighed against the result.

## The harness's tests

```sh
nix develop -c bash -c 'for t in evals/test/test_*.rb; do ruby "$t"; done'
```

They need no network or account: they read recorded transcripts and call
logs in `evals/test/fixtures/` (ids zeroed) and call the wrapper directly.
`nix flake check` runs them and `self-test` as `checks.evals`.
