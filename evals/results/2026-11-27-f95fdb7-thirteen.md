# Eval results 2026-11-27, f95fdb7 (thirteen)

- Commit: `f95fdb748621fc45e1f98366d906f8f8b70270ad`, Midnight 0.8.0
- Claude Code 2.1.327, Ruby 3.4.12
- Kit: `evals/kit` (31 files)
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
| `countdown` | `sonnet` | 5/5 | yes | 1.0 | 0.0 | 7.6 | 12 | 1489 | 91940 | 0.092 | 5 | 5 |
| `countdown` | `haiku` | 5/5 | yes | 1.4 | 0.0 | 9.4 | 27 | 5577 | 118868 | 0.010 | 5 | 5 |
| `crowd` | `sonnet` | 5/5 | yes | 2.2 | 0.0 | 11.8 | 16 | 2010 | 152305 | 0.151 | 5 | 5 |
| `crowd` | `haiku` | 5/5 | yes | 3.0 | 0.0 | 12.4 | 32 | 5985 | 325475 | 0.014 | 5 | 5 |
| `fixup` | `sonnet` | 5/5 | yes | 2.6 | 1.6 | 13.6 | 29 | 3828 | 200041 | 0.146 | 5 | 5 |
| `fixup` | `haiku` | 5/5 | yes | 3.2 | 1.2 | 12.6 | 59 | 12904 | 337961 | 0.018 | 4 | 5 |
| `league` | `sonnet` | 5/5 | yes | 1.0 | 0.0 | 10.2 | 20 | 2585 | 151672 | 0.164 | 5 | 5 |
| `league` | `haiku` | 5/5 | yes | 2.2 | 0.2 | 12.4 | 47 | 9457 | 360637 | 0.017 | 5 | 5 |
| `ledger` | `sonnet` | 5/5 | yes | 1.8 | 0.8 | 11.2 | 18 | 2199 | 177935 | 0.165 | 1 | 5 |
| `ledger` | `haiku` | 5/5 | yes | 2.4 | 0.4 | 13.6 | 40 | 7889 | 420767 | 0.017 | 3 | 5 |
| `mend` | `sonnet` | 5/5 | yes | 1.8 | 0.8 | 10.8 | 19 | 2465 | 214187 | 0.210 | 5 | 5 |
| `mend` | `haiku` | 5/5 | yes | 3.2 | 1.2 | 14.4 | 40 | 7953 | 283187 | 0.013 | 3 | 5 |
| `orbits` | `sonnet` | 5/5 | yes | 1.4 | 0.4 | 12.6 | 18 | 2195 | 177750 | 0.137 | 3 | 5 |
| `orbits` | `haiku` | 5/5 | yes | 1.6 | 0.0 | 12.0 | 24 | 4486 | 303696 | 0.013 | 5 | 5 |
| `paddle` | `sonnet` | 5/5 | yes | 1.0 | 0.0 | 11.6 | 20 | 2536 | 224844 | 0.220 | 5 | 5 |
| `paddle` | `haiku` | 0/5 | no | 2.2 | 0.2 | 14.0 | 50 | 8680 | 328638 | 0.015 | 5 | 5 |
| `pantry` | `sonnet` | 5/5 | yes | 1.0 | 0.0 | 10.4 | 26 | 3637 | 173653 | 0.190 | 5 | 5 |
| `pantry` | `haiku` | 5/5 | yes | 2.6 | 0.4 | 12.8 | 52 | 11394 | 347640 | 0.018 | 4 | 5 |
| `sandpile` | `sonnet` | 5/5 | yes | 1.0 | 0.0 | 6.4 | 10 | 1194 | 69448 | 0.068 | 5 | 5 |
| `sandpile` | `haiku` | 5/5 | yes | 1.6 | 0.0 | 9.0 | 22 | 4472 | 180268 | 0.012 | 5 | 5 |
| `swarm` | `sonnet` | 5/5 | yes | 1.0 | 0.0 | 9.8 | 16 | 2004 | 147618 | 0.142 | 5 | 5 |
| `swarm` | `haiku` | 5/5 | yes | 2.4 | 0.4 | 14.2 | 38 | 7454 | 356883 | 0.015 | 4 | 5 |
| `thermostat` | `sonnet` | 5/5 | yes | 1.8 | 0.8 | 8.6 | 11 | 1250 | 98123 | 0.085 | 1 | 5 |
| `thermostat` | `haiku` | 5/5 | yes | 2.2 | 0.6 | 10.6 | 21 | 3923 | 150429 | 0.007 | 4 | 5 |
| `warren` | `sonnet` | 5/5 | yes | 1.0 | 0.0 | 9.2 | 32 | 4217 | 157578 | 0.196 | 5 | 5 |
| `warren` | `haiku` | 5/5 | yes | 2.2 | 0.2 | 12.6 | 80 | 17632 | 277995 | 0.019 | 4 | 5 |

Means over graded runs. Cost is Claude Code's estimate at API prices.

## Diagnostics and run-time errors

| Kind | Template | Count | Runs | Passed | Repeated | Ruby-shaped | Spans |
| --- | --- | --- | --- | --- | --- | --- | --- |
| error | `` `…` is a system and can't be called `` | 40 | 5 | 5 | 15 |  | arrive ×8, board ×8, report ×8, sail ×8, wait ×8 |
| midnight | `drive: key down ArrowLeft` | 30 | 10 | 5 | 0 |  |  |
| midnight | `drive: key up ArrowLeft` | 30 | 10 | 5 | 0 |  |  |
| error | `` unexpected `…` `` | 14 | 14 | 14 | 0 |  | { ×7, ) ×3, while ×2, < ×1, ] ×1 |
| midnight | `` can't read `…`: No such file or directory (os error 2) `` | 10 | 7 | 7 | 3 |  | solution.12am ×10 |
| midnight | `drive: blur` | 10 | 10 | 5 | 0 |  |  |
| midnight | `drive: close` | 10 | 10 | 5 | 0 |  |  |
| midnight | `drive: focus` | 10 | 10 | 5 | 0 |  |  |
| midnight | `drive: key ArrowRight` | 10 | 10 | 5 | 0 |  |  |
| midnight | `drive: key down ArrowRight` | 10 | 10 | 5 | 0 |  |  |
| midnight | `drive: tick 10` | 10 | 10 | 5 | 0 |  |  |
| midnight | `drive: tick 20` | 10 | 10 | 5 | 0 |  |  |
| midnight | `drive: tick 3` | 10 | 10 | 5 | 0 |  |  |
| midnight | `drive: tick 30` | 10 | 10 | 5 | 0 |  |  |
| midnight | `drive: tick 4` | 10 | 10 | 5 | 0 |  |  |
| midnight | `drive: tick 6` | 10 | 10 | 5 | 0 |  |  |
| midnight | `drive: tick 60` | 10 | 10 | 5 | 0 |  |  |
| midnight | `drive: tick 9` | 10 | 10 | 5 | 0 |  |  |
| error | `` can't assign to `…` `` | 8 | 5 | 5 | 3 |  | p.patience ×8 |
| error | `` can't infer the element type of `…` `` | 8 | 2 | 2 | 2 |  | [] ×8 |
| error | `this query has 1 term, but its block takes 2 parameters` | 8 | 5 | 5 | 3 |  |  |
| error | `` `…` is a keyword and can't be used as a name `` | 5 | 5 | 5 | 0 |  | on ×5 |
| error | `` `…` can't be the type of a parameter `` | 3 | 1 | 1 | 0 |  | Float32 ×2, Int32 ×1 |
| error | `` `…` can't be a term of an `…` in a GPU system `` | 1 | 1 | 1 | 0 |  | Particle / each ×1 |
| error | `not a constant expression` | 1 | 1 | 1 | 0 |  |  |
| error | `` unknown field or method `…` for `…` `` | 1 | 1 | 1 | 0 |  | despawn / Fox ×1 |
| error | `` unknown variable or function `…` `` | 1 | 1 | 1 | 0 |  | handles ×1 |
| midnight | `stopped after 30 seconds` | 1 | 1 | 0 | 0 |  |  |

## The reference as read

| File | Section | Runs |
| --- | --- | --- |
| `language.md` | (before the first section) | 101 |
| `language.md` | A first program | 95 |
| `language.md` | The model | 88 |
| `language.md` | Lexical structure | 108 |
| `language.md` | Types | 113 |
| `language.md` | Compact field types | 117 |
| `language.md` | Variables and values | 113 |
| `language.md` | What can be modified | 109 |
| `language.md` | Expressions | 114 |
| `language.md` | Control flow | 122 |
| `language.md` | Functions | 108 |
| `language.md` | Declaring data | 73 |
| `language.md` | Structs | 93 |
| `language.md` | Enums | 82 |
| `language.md` | Components and tags | 85 |
| `language.md` | Resources | 83 |
| `language.md` | Events | 88 |
| `language.md` | Constants | 86 |
| `language.md` | Entities | 90 |
| `language.md` | Queries | 90 |
| `language.md` | Structural changes inside `each` | 93 |
| `language.md` | Systems and ticks | 104 |
| `language.md` | The window | 102 |
| `language.md` | Without a display | 76 |
| `language.md` | Drawing | 74 |
| `language.md` | Which pixels a shape covers | 62 |
| `language.md` | Colour | 60 |
| `language.md` | Draws in a GPU system | 61 |
| `language.md` | Draws on several cores | 59 |
| `language.md` | Determinism of a frame | 67 |
| `language.md` | The command `draws` | 34 |
| `language.md` | Keyboard and mouse | 48 |
| `language.md` | Commands for the keyboard and the mouse | 21 |
| `language.md` | GPU systems | 36 |
| `language.md` | What a GPU block may contain | 49 |
| `language.md` | What a GPU system may touch | 33 |
| `language.md` | Snapshot semantics | 27 |
| `language.md` | Arithmetic at 32 bits | 45 |
| `language.md` | Runtime errors in a GPU system | 24 |
| `language.md` | Random numbers in a GPU system | 34 |
| `language.md` | Determinism | 30 |
| `language.md` | Strict GPU systems | 43 |
| `language.md` | Without a GPU | 16 |
| `language.md` | The transfer plan | 23 |
| `language.md` | Several cores | 49 |
| `language.md` | Events | 47 |
| `language.md` | Built-ins | 49 |
| `language.md` | Output | 65 |
| `language.md` | Numbers | 76 |
| `language.md` | Strings | 73 |
| `language.md` | Lists | 70 |
| `language.md` | Random numbers, time and exit | 60 |
| `language.md` | Programs and files | 50 |
| `language.md` | What things cost | 43 |
| `language.md` | What a GPU system costs | 24 |
| `language.md` | What several cores cost | 31 |
| `language.md` | What a draw costs | 11 |
| `language.md` | Errors | 18 |
| `language.md` | Grammar | 27 |
| `language.md` | Not in 0.8 | 40 |
| `README.md` | (before the first section) | 120 |
| `README.md` | Try it | 93 |
| `README.md` | The compiler | 89 |
| `README.md` | GPU systems | 89 |
| `README.md` | Several cores | 86 |
| `README.md` | A window | 84 |
| `README.md` | Drawing | 84 |
| `README.md` | Keyboard and mouse | 84 |
| `README.md` | Benchmarks | 85 |
| `README.md` | Working on it | 84 |
| `README.md` | Status | 84 |
| `README.md` | License | 80 |
| `examples/battle.12am` | (whole file) | 23 |
| `examples/battle.out` | (whole file) | 0 |
| `examples/bench.12am` | (whole file) | 6 |
| `examples/bounce.12am` | (whole file) | 17 |
| `examples/bounce.out` | (whole file) | 4 |
| `examples/flock.12am` | (whole file) | 5 |
| `examples/flock.drive` | (whole file) | 0 |
| `examples/flock.out` | (whole file) | 0 |
| `examples/flock.stats` | (whole file) | 0 |
| `examples/hello.12am` | (whole file) | 8 |
| `examples/hello.out` | (whole file) | 1 |
| `examples/input.12am` | (whole file) | 15 |
| `examples/input.drive` | (whole file) | 5 |
| `examples/input.out` | (whole file) | 5 |
| `examples/life.12am` | (whole file) | 15 |
| `examples/life.out` | (whole file) | 3 |
| `examples/life_rules.12am` | (whole file) | 0 |
| `examples/life_rules.out` | (whole file) | 0 |
| `examples/move.12am` | (whole file) | 16 |
| `examples/move.out` | (whole file) | 7 |
| `examples/nbody.12am` | (whole file) | 15 |
| `examples/nbody.out` | (whole file) | 5 |
| `examples/shapes.12am` | (whole file) | 1 |
| `examples/shapes.draws` | (whole file) | 0 |
| `examples/shapes.drive` | (whole file) | 0 |
| `examples/shapes.out` | (whole file) | 0 |
| `examples/window.12am` | (whole file) | 4 |
| `examples/window.drive` | (whole file) | 1 |
| `examples/window.out` | (whole file) | 3 |

Runs of 130 graded that a tool result showed a line of the section to, below its heading.

## Exceptions

- Denied tool calls: 13 in 12 runs: Bash `python3 - <<'E'`, Bash `python3 -c 1 2>&1 | head -1`, Bash `cat > /tmp/p.py <<'EOF'`, Bash `python3 - <<'E' 2>&1 | head -1`, Bash `midnight check solution.12am; echo "exit $?"`, Bash `grep -n "elsif\|^tag\|\`!\`\|&&\|\`next\`\|clamp" language.m`, Bash `grep -n "Commands for the keyboard and the mouse" language.m`, Bash `midnight check /tmp/eval.ZiP0yPnn/solution.12am; echo "exit=`, Bash `MIDNIGHT_GPU=off midnight run /tmp/eval.ZiP0yPnn/solution.12`, Bash `sed -i 's/\bon\b = /n_on = /; s/    on = 2/    n_on = 2/; s/`, Bash `sed -i 's/\bon\b = /n_on = /; s/on = 2/n_on = 2/; s/on += 1/`, Bash `sed -i 's/\bon\b/n_on/g; s/heaters n_on/heaters on/' solutio`, Bash `sed -i 's/\bon\b/n_on/g; s/heaters n_on:/heaters on:/' solut`

130 counted runs, 130 graded.
