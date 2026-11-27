# Ledger of the evals

Each eval, its status, and one entry for each spec that ran it: the
question it was asked, what was expected, what it showed, and whether it
changed or informed the decision. "What a result decides" in `README.md`
has the rules.

## The thirteen

`thermostat`, `countdown`, `league`, `sandpile`, `warren`, `pantry`,
`orbits`, `swarm`, `ledger`, `crowd`, `paddle`, `mend`, `fixup`; five
trials of `sonnet` and `haiku`. Measure: pass rates by task and model,
through `compare`. Used as a regression check of the reference.

**Status:** experimental. Can it move: not shown. Sonnet passes every
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

## Drawing

`poster`, `drift`, `gauge`, `retouch`; ten trials of `sonnet` and
`haiku`, graded by output, systems and the check **draws**.

**Status:** experimental. Can it move: not shown; both forms passed
nearly every run.

- **0009** (`2026-11-27-a5c4fd8-draw`, `2026-11-27-546bbca-draw`): do
  agents pass the drawing tasks significantly more often when drawing is
  built-in components than when it is statements? Measure: passes over
  all rows, Fisher's exact test, p < 0.1. Expected: no. Statements 80 of
  80, components 79 of 80, p = 1.000. Changed nothing: statements stood
  by default. Informed: first-try compiles 80 of 80 against 68 of 80, no
  compile error against 33 in 10 runs, mostly a tag used as a query term
  and query terms that did not match a block's parameters.
