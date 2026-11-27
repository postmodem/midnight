# Ledger of the evals

Each eval, its status, and one entry for each spec that ran it: the
question it was asked, what was expected, what it showed, and whether it
changed or informed the decision. "When the evals run" and "What a
result decides" in `README.md` have the rules.

After spec 0009 no eval runs by default: both sets below were at their
ceiling, and what told anything apart came from the finer measures, the
audits and the reviews of the tasks. A new eval starts as a spike read by
hand, and both sets were removed. The Ruby-shaped measure of
diagnostics went at the same time: over 390 sessions of the thirteen it flagged one kind of error 4
times, all in runs that passed.

## The thirteen

`thermostat`, `countdown`, `league`, `sandpile`, `warren`, `pantry`,
`orbits`, `swarm`, `ledger`, `crowd`, `paddle`, `mend`, `fixup`; five
trials of `sonnet` and `haiku`. Measure: pass rates by task and model,
through `compare`. Used as a regression check of the reference.

**Status:** removed after 0009; the files are in the history
(`git show 0483b58:evals/tasks`). Can it move: not shown. Sonnet passes every
task every time; Haiku fails `paddle` every time; a run without
"Structural changes inside `each`" in the reference passed all six of
its runs (one trial of `warren`, `fixup` and `thermostat` per model).

- **0008** (`2026-11-26-a8f0a5f-baseline`): the baseline, 124 of 130
  runs passed. Its transcript audit found a correct `warren` rejected,
  which the task's wording caused; fixed before the baseline.
- **0009** (`2026-11-27-a5c4fd8-thirteen`): would "Drawing" in the
  reference make agents pass the thirteen less often? Expected: no.
  125 of 130, no row marked, chance about 0.0. Informed: the section did
  no harm that this eval can see. Changed nothing.
- **0009** (`2026-11-27-f95fdb7-thirteen`), at the spec's last commit:
  the same question with `examples/flock.12am` and the README's commands
  in the kit. 125 of 130 (Haiku fails `paddle` five times, as before), no
  row marked, chance about 0.0. Changed nothing; the eval had shown no
  movement in four runs, and it was removed.

## Drawing

`poster`, `drift`, `gauge`, `retouch`; ten trials of `sonnet` and
`haiku`, graded by output, systems and the check **draws**.

**Status:** removed after 0009, with the thirteen.
Can it move: not shown; both forms passed nearly every run.

- **0009** (`2026-11-27-a5c4fd8-draw`, `2026-11-27-546bbca-draw`): do
  agents pass the drawing tasks significantly more often when drawing is
  built-in components than when it is statements? Measure: passes over
  all rows, Fisher's exact test, p < 0.1. Expected: no. Statements 80 of
  80, components 79 of 80, p = 1.000. Changed nothing: statements stood
  by default. Informed: first-try compiles 80 of 80 against 68 of 80, no
  compile error against 33 in 10 runs, mostly a tag used as a query term
  and query terms that did not match a block's parameters.
- **0009** (`2026-11-27-f95fdb7-draw`), at the spec's last commit: did
  the statement form hold up with the renderer and the costs in the
  reference? 80 of 80, as at `a5c4fd8`; every run that made a first
  compile of its own made it without an error. Changed nothing.
