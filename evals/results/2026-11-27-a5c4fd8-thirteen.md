# Eval results 2026-11-27, a5c4fd8 (thirteen)

- Commit: `a5c4fd8b040d8e911fd3d3603ddb5ebc741e70b0`, Midnight 0.8.0
- Claude Code 2.1.327, Ruby 3.4.12
- Kit: `evals/kit` (27 files)
- Agent's tools: `/nix/store/p7ip75rls2g87d1yy4568djpa244c7ww-coreutils-9.11/bin:/nix/store/9g53s4a9kv7k5xbfjhyaxjymgw62di6r-bash-interactive-5.3p9/bin:/nix/store/8kxpzdb09cbn252h8wl14zds9jgbw2q0-findutils-4.10.0/bin:/nix/store/hi1zxw2mg94cwrpbq1s8bbjm1k5y3vci-gnugrep-3.12/bin:/nix/store/r3fwyjb1k9v7y6qd5l2jq1sx3b46dx2y-gnused-4.10/bin:/nix/store/q7p8cvfxi5vplngni9a38n17ykhgcx1d-ripgrep-15.1.0/bin`; compiler's PATH: `/nix/store/p7ip75rls2g87d1yy4568djpa244c7ww-coreutils-9.11/bin`
- Models: sonnet, haiku (as reported: claude-haiku-5-5, claude-sonnet-5-5); effort: default; trials: 5

## Pass rates

| Model | pass@1 (± s.e. across tasks) | Tasks passed every time |
| --- | --- | --- |
| `sonnet` | 100.0% ± 0.0 | 13 of 13 |
| `haiku` | 92.3% ± 7.7 | 12 of 13 |

## Tasks

| Task | Model | Passed | Every time | Calls | Failed calls | Turns | Seconds | Output tokens | Input tokens | Cost (est. $) | First try | Verified |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `countdown` | `sonnet` | 5/5 | yes | 1.0 | 0.0 | 7.6 | 12 | 1533 | 92975 | 0.114 | 5 | 5 |
| `countdown` | `haiku` | 5/5 | yes | 2.0 | 0.0 | 8.8 | 26 | 5375 | 137745 | 0.010 | 5 | 5 |
| `crowd` | `sonnet` | 5/5 | yes | 2.0 | 0.0 | 11.4 | 15 | 1934 | 139829 | 0.125 | 5 | 5 |
| `crowd` | `haiku` | 5/5 | yes | 3.0 | 0.0 | 12.0 | 30 | 5712 | 270645 | 0.013 | 5 | 5 |
| `fixup` | `sonnet` | 5/5 | yes | 2.4 | 1.2 | 13.4 | 28 | 3791 | 192545 | 0.153 | 5 | 5 |
| `fixup` | `haiku` | 5/5 | yes | 2.8 | 0.8 | 12.6 | 53 | 11619 | 298938 | 0.017 | 5 | 5 |
| `league` | `sonnet` | 5/5 | yes | 1.0 | 0.0 | 9.4 | 19 | 2494 | 146496 | 0.151 | 5 | 5 |
| `league` | `haiku` | 5/5 | yes | 2.2 | 0.2 | 12.0 | 46 | 9293 | 340726 | 0.017 | 4 | 5 |
| `ledger` | `sonnet` | 5/5 | yes | 1.8 | 0.8 | 12.6 | 20 | 2399 | 196471 | 0.160 | 1 | 5 |
| `ledger` | `haiku` | 5/5 | yes | 2.6 | 0.2 | 14.8 | 43 | 8368 | 413225 | 0.018 | 4 | 5 |
| `mend` | `sonnet` | 5/5 | yes | 1.8 | 0.8 | 10.6 | 19 | 2439 | 219241 | 0.209 | 5 | 5 |
| `mend` | `haiku` | 5/5 | yes | 2.4 | 0.4 | 12.4 | 43 | 8737 | 322755 | 0.015 | 5 | 5 |
| `orbits` | `sonnet` | 5/5 | yes | 1.0 | 0.0 | 11.8 | 16 | 2025 | 161847 | 0.145 | 5 | 5 |
| `orbits` | `haiku` | 5/5 | yes | 1.6 | 0.0 | 11.2 | 26 | 5130 | 234024 | 0.012 | 5 | 5 |
| `paddle` | `sonnet` | 5/5 | yes | 1.0 | 0.0 | 10.8 | 19 | 2429 | 211265 | 0.203 | 5 | 5 |
| `paddle` | `haiku` | 0/5 | no | 3.2 | 1.0 | 15.0 | 47 | 9092 | 447312 | 0.018 | 4 | 5 |
| `pantry` | `sonnet` | 5/5 | yes | 1.0 | 0.0 | 9.8 | 24 | 3488 | 147894 | 0.182 | 5 | 5 |
| `pantry` | `haiku` | 5/5 | yes | 2.4 | 0.4 | 11.2 | 54 | 11926 | 354670 | 0.018 | 3 | 5 |
| `sandpile` | `sonnet` | 5/5 | yes | 1.0 | 0.0 | 6.4 | 9 | 1153 | 67754 | 0.065 | 5 | 5 |
| `sandpile` | `haiku` | 5/5 | yes | 1.8 | 0.0 | 8.8 | 24 | 4629 | 203035 | 0.011 | 5 | 5 |
| `swarm` | `sonnet` | 5/5 | yes | 1.0 | 0.0 | 10.6 | 16 | 2084 | 149296 | 0.147 | 5 | 5 |
| `swarm` | `haiku` | 5/5 | yes | 2.2 | 0.2 | 13.4 | 37 | 7156 | 297453 | 0.015 | 4 | 5 |
| `thermostat` | `sonnet` | 5/5 | yes | 1.8 | 0.8 | 8.8 | 11 | 1293 | 99531 | 0.079 | 1 | 5 |
| `thermostat` | `haiku` | 5/5 | yes | 2.0 | 0.2 | 10.4 | 22 | 4283 | 151372 | 0.009 | 4 | 5 |
| `warren` | `sonnet` | 5/5 | yes | 1.0 | 0.0 | 10.6 | 32 | 4219 | 212639 | 0.199 | 5 | 5 |
| `warren` | `haiku` | 5/5 | yes | 2.4 | 0.4 | 11.0 | 89 | 19331 | 300388 | 0.022 | 3 | 5 |

Means over graded runs. Cost is Claude Code's estimate at API prices.

## Diagnostics and run-time errors

| Kind | Template | Count | Runs | Passed | Repeated | Ruby-shaped | Spans |
| --- | --- | --- | --- | --- | --- | --- | --- |
| error | `` `…` is a system and can't be called `` | 40 | 7 | 7 | 5 |  | arrive ×8, board ×8, report ×8, sail ×8, wait ×8 |
| midnight | `drive: key down ArrowLeft` | 33 | 10 | 5 | 3 |  |  |
| midnight | `drive: key up ArrowLeft` | 33 | 10 | 5 | 3 |  |  |
| error | `` unexpected `…` `` | 13 | 13 | 13 | 0 |  | { ×6, ) ×3, ] ×3, , ×1 |
| midnight | `drive: blur` | 11 | 10 | 5 | 1 |  |  |
| midnight | `drive: close` | 11 | 10 | 5 | 1 |  |  |
| midnight | `drive: focus` | 11 | 10 | 5 | 1 |  |  |
| midnight | `drive: key ArrowRight` | 11 | 10 | 5 | 1 |  |  |
| midnight | `drive: key down ArrowRight` | 11 | 10 | 5 | 1 |  |  |
| midnight | `drive: tick 10` | 11 | 10 | 5 | 1 |  |  |
| midnight | `drive: tick 20` | 11 | 10 | 5 | 1 |  |  |
| midnight | `drive: tick 3` | 11 | 10 | 5 | 1 |  |  |
| midnight | `drive: tick 30` | 11 | 10 | 5 | 1 |  |  |
| midnight | `drive: tick 4` | 11 | 10 | 5 | 1 |  |  |
| midnight | `drive: tick 6` | 11 | 10 | 5 | 1 |  |  |
| midnight | `drive: tick 60` | 11 | 10 | 5 | 1 |  |  |
| midnight | `drive: tick 9` | 11 | 10 | 5 | 1 |  |  |
| error | `` can't assign to `…` `` | 8 | 7 | 7 | 1 |  | p.patience ×8 |
| error | `this query has 1 term, but its block takes 2 parameters` | 8 | 7 | 7 | 1 |  |  |
| midnight | `` can't read `…`: No such file or directory (os error 2) `` | 6 | 6 | 2 | 0 |  | solution.12am ×6 |
| error | `` `…` is a keyword and can't be used as a name `` | 5 | 5 | 5 | 0 |  | on ×5 |
| error | `` `…` can't be a term of an `…` in a GPU system `` | 3 | 1 | 1 | 0 |  | Bounce / each ×1, Position / each ×1, Velocity / each ×1 |
| error | `` unknown variable or function `…` `` | 2 | 1 | 0 | 0 |  | text ×2 |
| error | `` `…` with arguments needs parentheses here `` | 1 | 1 | 1 | 0 |  | spawn ×1 |
| error | `this query has 2 terms, but its block takes 1 parameter` | 1 | 1 | 1 | 0 |  |  |
| error | `` unknown field or method `…` for `…` `` | 1 | 1 | 1 | 0 |  | despawn / Fox ×1 |

## The reference as read

| File | Section | Runs |
| --- | --- | --- |
| `language.md` | (before the first section) | 103 |
| `language.md` | A first program | 101 |
| `language.md` | The model | 96 |
| `language.md` | Lexical structure | 113 |
| `language.md` | Types | 115 |
| `language.md` | Compact field types | 112 |
| `language.md` | Variables and values | 112 |
| `language.md` | What can be modified | 111 |
| `language.md` | Expressions | 114 |
| `language.md` | Control flow | 117 |
| `language.md` | Functions | 108 |
| `language.md` | Declaring data | 80 |
| `language.md` | Structs | 97 |
| `language.md` | Enums | 86 |
| `language.md` | Components and tags | 89 |
| `language.md` | Resources | 89 |
| `language.md` | Events | 90 |
| `language.md` | Constants | 89 |
| `language.md` | Entities | 93 |
| `language.md` | Queries | 97 |
| `language.md` | Structural changes inside `each` | 98 |
| `language.md` | Systems and ticks | 99 |
| `language.md` | The window | 98 |
| `language.md` | Without a display | 74 |
| `language.md` | Drawing | 72 |
| `language.md` | Which pixels a shape covers | 61 |
| `language.md` | Colour | 54 |
| `language.md` | Draws in a GPU system | 64 |
| `language.md` | Draws on several cores | 63 |
| `language.md` | Determinism of a frame | 63 |
| `language.md` | The command `draws` | 28 |
| `language.md` | Keyboard and mouse | 44 |
| `language.md` | Commands for the keyboard and the mouse | 20 |
| `language.md` | GPU systems | 33 |
| `language.md` | What a GPU block may contain | 49 |
| `language.md` | What a GPU system may touch | 33 |
| `language.md` | Snapshot semantics | 29 |
| `language.md` | Arithmetic at 32 bits | 42 |
| `language.md` | Runtime errors in a GPU system | 26 |
| `language.md` | Random numbers in a GPU system | 35 |
| `language.md` | Determinism | 30 |
| `language.md` | Strict GPU systems | 35 |
| `language.md` | Without a GPU | 17 |
| `language.md` | The transfer plan | 38 |
| `language.md` | Several cores | 44 |
| `language.md` | Events | 45 |
| `language.md` | Built-ins | 46 |
| `language.md` | Output | 67 |
| `language.md` | Numbers | 68 |
| `language.md` | Strings | 68 |
| `language.md` | Lists | 66 |
| `language.md` | Random numbers, time and exit | 59 |
| `language.md` | Programs and files | 49 |
| `language.md` | What things cost | 37 |
| `language.md` | What a GPU system costs | 19 |
| `language.md` | What several cores cost | 29 |
| `language.md` | What a draw costs | 5 |
| `language.md` | Errors | 20 |
| `language.md` | Grammar | 22 |
| `language.md` | Not in 0.8 | 40 |
| `README.md` | (before the first section) | 117 |
| `README.md` | Try it | 94 |
| `README.md` | The compiler | 93 |
| `README.md` | GPU systems | 93 |
| `README.md` | Several cores | 88 |
| `README.md` | A window | 88 |
| `README.md` | Drawing | 88 |
| `README.md` | Keyboard and mouse | 88 |
| `README.md` | Benchmarks | 88 |
| `README.md` | Working on it | 88 |
| `README.md` | Status | 88 |
| `README.md` | License | 88 |
| `examples/battle.12am` | (whole file) | 22 |
| `examples/battle.out` | (whole file) | 0 |
| `examples/bench.12am` | (whole file) | 8 |
| `examples/bounce.12am` | (whole file) | 23 |
| `examples/bounce.out` | (whole file) | 3 |
| `examples/hello.12am` | (whole file) | 8 |
| `examples/hello.out` | (whole file) | 0 |
| `examples/input.12am` | (whole file) | 13 |
| `examples/input.drive` | (whole file) | 4 |
| `examples/input.out` | (whole file) | 3 |
| `examples/life.12am` | (whole file) | 8 |
| `examples/life.out` | (whole file) | 1 |
| `examples/life_rules.12am` | (whole file) | 0 |
| `examples/life_rules.out` | (whole file) | 0 |
| `examples/move.12am` | (whole file) | 17 |
| `examples/move.out` | (whole file) | 7 |
| `examples/nbody.12am` | (whole file) | 18 |
| `examples/nbody.out` | (whole file) | 3 |
| `examples/shapes.12am` | (whole file) | 0 |
| `examples/shapes.draws` | (whole file) | 0 |
| `examples/shapes.drive` | (whole file) | 0 |
| `examples/shapes.out` | (whole file) | 0 |
| `examples/window.12am` | (whole file) | 3 |
| `examples/window.drive` | (whole file) | 1 |
| `examples/window.out` | (whole file) | 1 |

Runs of 130 graded that a tool result showed a line of the section to, below its heading.

## Exceptions

- Denied tool calls: 9 in 9 runs: Bash `cd docs/examples; cat hello.12am battle.12am; grep -n "^#" .`, Bash `cat > /tmp/x 2>/dev/null; python3 - 2>/dev/null; echo`, Bash `cat > /tmp/x 2>/dev/null; python3 --version 2>&1 | head -1`, Bash `python3 - <<'E' 2>&1 | head -1`, Bash `grep -n -i 'string.*compar\|compar.*string\|lexicograph\|`<``, Bash `midnight check solution.12am; echo "check exit $?"`, Bash `cd /tmp/eval.skc295el && MIDNIGHT_WINDOW=headless MIDNIGHT_D`, Bash `sed -i 's/\bon\b/cnt/g; s/heaters cnt/heaters on/' solution.`, Bash `sed -i 's/\bon\b = 0/n = 0/; s/on = 2/n = 2/; s/on += 1/n +=`

130 counted runs, 130 graded.
