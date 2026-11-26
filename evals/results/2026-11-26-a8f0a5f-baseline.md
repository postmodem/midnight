# Eval results 2026-11-26, a8f0a5f (baseline)

- Commit: `a8f0a5fa5ff7b3cdf031337dc6866392ea997f69`, Midnight 0.7.0
- Claude Code 2.1.327, Ruby 3.4.12
- Kit: `evals/kit` (23 files)
- Agent's tools: `/nix/store/p7ip75rls2g87d1yy4568djpa244c7ww-coreutils-9.11/bin:/nix/store/9g53s4a9kv7k5xbfjhyaxjymgw62di6r-bash-interactive-5.3p9/bin:/nix/store/8kxpzdb09cbn252h8wl14zds9jgbw2q0-findutils-4.10.0/bin:/nix/store/hi1zxw2mg94cwrpbq1s8bbjm1k5y3vci-gnugrep-3.12/bin:/nix/store/r3fwyjb1k9v7y6qd5l2jq1sx3b46dx2y-gnused-4.10/bin:/nix/store/q7p8cvfxi5vplngni9a38n17ykhgcx1d-ripgrep-15.1.0/bin`; compiler's PATH: `/nix/store/p7ip75rls2g87d1yy4568djpa244c7ww-coreutils-9.11/bin`
- Models: sonnet, haiku (as reported: claude-haiku-5-5, claude-sonnet-5-5); effort: default; trials: 5

## Pass rates

| Model | pass@1 (± s.e. across tasks) | Tasks passed every time |
| --- | --- | --- |
| `sonnet` | 100.0% ± 0.0 | 13 of 13 |
| `haiku` | 90.8% ± 7.7 | 11 of 13 |

## Tasks

| Task | Model | Passed | Every time | Calls | Failed calls | Turns | Seconds | Output tokens | Input tokens | Cost (est. $) | First try | Verified |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `countdown` | `sonnet` | 5/5 | yes | 1.2 | 0.2 | 7.0 | 12 | 1534 | 73735 | 0.068 | 4 | 5 |
| `countdown` | `haiku` | 5/5 | yes | 1.6 | 0.0 | 10.0 | 27 | 5648 | 148507 | 0.011 | 5 | 5 |
| `crowd` | `sonnet` | 5/5 | yes | 2.0 | 0.0 | 10.2 | 14 | 1793 | 129797 | 0.119 | 5 | 5 |
| `crowd` | `haiku` | 5/5 | yes | 3.2 | 0.2 | 14.8 | 30 | 5587 | 382327 | 0.015 | 5 | 5 |
| `fixup` | `sonnet` | 5/5 | yes | 2.0 | 1.0 | 10.8 | 27 | 3651 | 181290 | 0.160 | 5 | 5 |
| `fixup` | `haiku` | 5/5 | yes | 2.8 | 0.8 | 12.6 | 51 | 11167 | 259784 | 0.014 | 4 | 5 |
| `league` | `sonnet` | 5/5 | yes | 1.0 | 0.0 | 10.2 | 20 | 2621 | 155276 | 0.164 | 5 | 5 |
| `league` | `haiku` | 5/5 | yes | 2.6 | 0.2 | 12.4 | 52 | 10825 | 378581 | 0.018 | 4 | 5 |
| `ledger` | `sonnet` | 5/5 | yes | 2.4 | 1.4 | 13.0 | 19 | 2383 | 187172 | 0.145 | 0 | 5 |
| `ledger` | `haiku` | 5/5 | yes | 2.2 | 0.2 | 13.0 | 40 | 7465 | 353532 | 0.015 | 4 | 5 |
| `mend` | `sonnet` | 5/5 | yes | 1.8 | 0.8 | 9.8 | 17 | 2243 | 188816 | 0.187 | 5 | 5 |
| `mend` | `haiku` | 5/5 | yes | 3.0 | 1.0 | 13.0 | 38 | 7631 | 241698 | 0.012 | 4 | 5 |
| `orbits` | `sonnet` | 5/5 | yes | 1.0 | 0.0 | 10.8 | 16 | 1941 | 149010 | 0.133 | 5 | 5 |
| `orbits` | `haiku` | 5/5 | yes | 1.6 | 0.0 | 13.0 | 28 | 5163 | 345552 | 0.015 | 5 | 5 |
| `paddle` | `sonnet` | 5/5 | yes | 1.2 | 0.2 | 11.0 | 19 | 2498 | 174398 | 0.196 | 4 | 4 |
| `paddle` | `haiku` | 0/5 | no | 2.6 | 0.6 | 10.6 | 38 | 7165 | 317373 | 0.017 | 3 | 5 |
| `pantry` | `sonnet` | 5/5 | yes | 1.0 | 0.0 | 10.0 | 25 | 3439 | 203695 | 0.199 | 5 | 5 |
| `pantry` | `haiku` | 5/5 | yes | 2.2 | 0.2 | 11.2 | 56 | 12261 | 318018 | 0.018 | 4 | 5 |
| `sandpile` | `sonnet` | 5/5 | yes | 1.0 | 0.0 | 6.0 | 9 | 1083 | 60807 | 0.062 | 5 | 5 |
| `sandpile` | `haiku` | 5/5 | yes | 1.8 | 0.0 | 9.0 | 22 | 4340 | 186576 | 0.012 | 5 | 5 |
| `swarm` | `sonnet` | 5/5 | yes | 1.0 | 0.0 | 10.4 | 16 | 2103 | 139400 | 0.142 | 5 | 5 |
| `swarm` | `haiku` | 4/5 | no | 2.6 | 0.6 | 11.8 | 36 | 7043 | 300411 | 0.016 | 2 | 5 |
| `thermostat` | `sonnet` | 5/5 | yes | 1.6 | 0.6 | 8.6 | 11 | 1225 | 105037 | 0.082 | 2 | 5 |
| `thermostat` | `haiku` | 5/5 | yes | 1.8 | 0.2 | 9.6 | 20 | 3681 | 185348 | 0.011 | 4 | 5 |
| `warren` | `sonnet` | 5/5 | yes | 1.2 | 0.2 | 10.4 | 31 | 4032 | 215737 | 0.203 | 4 | 5 |
| `warren` | `haiku` | 5/5 | yes | 2.4 | 0.4 | 9.6 | 91 | 20276 | 271342 | 0.023 | 4 | 5 |

Means over graded runs. Cost is Claude Code's estimate at API prices.

## Diagnostics and run-time errors

| Kind | Template | Count | Runs | Passed | Repeated | Ruby-shaped | Spans |
| --- | --- | --- | --- | --- | --- | --- | --- |
| error | `` `…` is a system and can't be called `` | 40 | 8 | 8 | 0 |  | arrive ×8, board ×8, report ×8, sail ×8, wait ×8 |
| midnight | `drive: key down ArrowLeft` | 27 | 9 | 4 | 0 |  |  |
| midnight | `drive: key up ArrowLeft` | 27 | 9 | 4 | 0 |  |  |
| error | `` unexpected `…` `` | 16 | 14 | 14 | 0 |  | { ×7, ) ×5, : ×2, ] ×1, or ×1 |
| midnight | `drive: blur` | 9 | 9 | 4 | 0 |  |  |
| midnight | `drive: close` | 9 | 9 | 4 | 0 |  |  |
| midnight | `drive: focus` | 9 | 9 | 4 | 0 |  |  |
| midnight | `drive: key ArrowRight` | 9 | 9 | 4 | 0 |  |  |
| midnight | `drive: key down ArrowRight` | 9 | 9 | 4 | 0 |  |  |
| midnight | `drive: tick 10` | 9 | 9 | 4 | 0 |  |  |
| midnight | `drive: tick 20` | 9 | 9 | 4 | 0 |  |  |
| midnight | `drive: tick 3` | 9 | 9 | 4 | 0 |  |  |
| midnight | `drive: tick 30` | 9 | 9 | 4 | 0 |  |  |
| midnight | `drive: tick 4` | 9 | 9 | 4 | 0 |  |  |
| midnight | `drive: tick 6` | 9 | 9 | 4 | 0 |  |  |
| midnight | `drive: tick 60` | 9 | 9 | 4 | 0 |  |  |
| midnight | `drive: tick 9` | 9 | 9 | 4 | 0 |  |  |
| error | `` can't assign to `…` `` | 7 | 7 | 7 | 0 |  | p.patience ×7 |
| error | `this query has 1 term, but its block takes 2 parameters` | 7 | 7 | 7 | 0 |  |  |
| error | `` `…` can't be a term of an `…` in a GPU system `` | 5 | 3 | 3 | 0 |  | Particle / each ×2, Bounces / each ×1, Position / each ×1, Velocity / each ×1 |
| error | `` `…` is a keyword and can't be used as a name `` | 4 | 4 | 4 | 0 |  | on ×4 |
| error | `` unknown method `…` for `…` `` | 4 | 2 | 2 | 0 | yes | to_f / Float ×2, max / [Int] ×1, min / [Int] ×1 |
| midnight | `` can't read `…`: No such file or directory (os error 2) `` | 4 | 4 | 4 | 0 |  | solution.12am ×4 |
| error | `` can't infer the element type of `…` `` | 2 | 2 | 2 | 0 |  | [] ×2 |
| error | `` unknown variable or function `…` `` | 2 | 1 | 0 | 0 |  | msg ×2 |
| error | `` `…` appears twice in this query `` | 1 | 1 | 1 | 0 |  | Fox ×1 |
| error | `` `…` is listed twice `` | 1 | 1 | 0 | 0 |  | Ball ×1 |
| error | `` unknown field or method `…` for `…` `` | 1 | 1 | 1 | 0 |  | despawn / Fox ×1 |
| midnight | `stopped after 30 seconds` | 1 | 1 | 0 | 0 |  |  |

## The reference as read

| File | Section | Runs |
| --- | --- | --- |
| `language.md` | (before the first section) | 114 |
| `language.md` | A first program | 109 |
| `language.md` | The model | 102 |
| `language.md` | Lexical structure | 115 |
| `language.md` | Types | 113 |
| `language.md` | Compact field types | 115 |
| `language.md` | Variables and values | 116 |
| `language.md` | What can be modified | 111 |
| `language.md` | Expressions | 113 |
| `language.md` | Control flow | 116 |
| `language.md` | Functions | 102 |
| `language.md` | Declaring data | 80 |
| `language.md` | Structs | 97 |
| `language.md` | Enums | 85 |
| `language.md` | Components and tags | 88 |
| `language.md` | Resources | 87 |
| `language.md` | Events | 91 |
| `language.md` | Constants | 85 |
| `language.md` | Entities | 96 |
| `language.md` | Queries | 99 |
| `language.md` | Structural changes inside `each` | 97 |
| `language.md` | Systems and ticks | 98 |
| `language.md` | The window | 99 |
| `language.md` | Without a display | 82 |
| `language.md` | Keyboard and mouse | 83 |
| `language.md` | Commands for the keyboard and the mouse | 70 |
| `language.md` | GPU systems | 74 |
| `language.md` | What a GPU block may contain | 38 |
| `language.md` | What a GPU system may touch | 27 |
| `language.md` | Snapshot semantics | 25 |
| `language.md` | Arithmetic at 32 bits | 41 |
| `language.md` | Runtime errors in a GPU system | 21 |
| `language.md` | Random numbers in a GPU system | 33 |
| `language.md` | Determinism | 33 |
| `language.md` | Strict GPU systems | 39 |
| `language.md` | Without a GPU | 23 |
| `language.md` | The transfer plan | 43 |
| `language.md` | Several cores | 52 |
| `language.md` | Events | 50 |
| `language.md` | Built-ins | 44 |
| `language.md` | Output | 62 |
| `language.md` | Numbers | 67 |
| `language.md` | Strings | 66 |
| `language.md` | Lists | 60 |
| `language.md` | Random numbers, time and exit | 55 |
| `language.md` | Programs and files | 47 |
| `language.md` | What things cost | 43 |
| `language.md` | What a GPU system costs | 33 |
| `language.md` | What several cores cost | 37 |
| `language.md` | Errors | 22 |
| `language.md` | Grammar | 29 |
| `language.md` | Not in 0.7 | 35 |
| `README.md` | (before the first section) | 113 |
| `README.md` | Try it | 97 |
| `README.md` | The compiler | 94 |
| `README.md` | GPU systems | 94 |
| `README.md` | Several cores | 94 |
| `README.md` | A window | 94 |
| `README.md` | Keyboard and mouse | 95 |
| `README.md` | Benchmarks | 94 |
| `README.md` | Working on it | 94 |
| `README.md` | Status | 94 |
| `README.md` | License | 94 |
| `examples/battle.12am` | (whole file) | 25 |
| `examples/battle.out` | (whole file) | 2 |
| `examples/bench.12am` | (whole file) | 8 |
| `examples/bounce.12am` | (whole file) | 24 |
| `examples/bounce.out` | (whole file) | 2 |
| `examples/hello.12am` | (whole file) | 10 |
| `examples/hello.out` | (whole file) | 2 |
| `examples/input.12am` | (whole file) | 14 |
| `examples/input.drive` | (whole file) | 5 |
| `examples/input.out` | (whole file) | 5 |
| `examples/life.12am` | (whole file) | 11 |
| `examples/life.out` | (whole file) | 0 |
| `examples/life_rules.12am` | (whole file) | 0 |
| `examples/life_rules.out` | (whole file) | 0 |
| `examples/move.12am` | (whole file) | 17 |
| `examples/move.out` | (whole file) | 3 |
| `examples/nbody.12am` | (whole file) | 18 |
| `examples/nbody.out` | (whole file) | 5 |
| `examples/window.12am` | (whole file) | 7 |
| `examples/window.drive` | (whole file) | 1 |
| `examples/window.out` | (whole file) | 0 |

Runs of 130 graded that a tool result showed a line of the section to, below its heading.

## Exceptions

- Denied tool calls: 12 in 12 runs: Bash `sed -i 's/^def \(.*\)): \(.*\)$/def \1) -> \2/' solution.12a`, Bash `cd docs/examples; cat hello.12am battle.12am; grep -n "^#" .`, Bash `sed -n 2091,2200p language.md; sed -n 1,20p /dev/null`, Bash `python3 -c 1`, Bash `cd /tmp/eval.2oh9gT1V; python3 - <<'E' 2>&1 | head -3`, Bash `mkdir -p /tmp/dbg && sed 's/^  rows.length.times do |i|\n   `, Bash `MIDNIGHT_WINDOW=headless MIDNIGHT_DRIVE=try.drive midnight r` ×2, Bash `cd /tmp/eval.E09Knj86 && MIDNIGHT_WINDOW=headless MIDNIGHT_D`, Bash `sed -i 's/\bon\b = 0/n_on = 0/; s/on += /n_on += /; s/heater`, Bash `sed -i 's/\bon\b = 0/n_on = 0/; s/    on += /    n_on += /; `, Bash `sed -i 's/\bon\b/n_on/g; s/heaters n_on/heaters on/' solutio`

130 counted runs, 130 graded.
