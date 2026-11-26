# Eval results 2026-11-26, a8f0a5f (no-structural)

- Commit: `a8f0a5fa5ff7b3cdf031337dc6866392ea997f69`, Midnight 0.7.0
- Claude Code 2.1.327, Ruby 3.4.12
- Kit: `evals/test/kit-no-structural/kit` (23 files)
- Agent's tools: `/nix/store/p7ip75rls2g87d1yy4568djpa244c7ww-coreutils-9.11/bin:/nix/store/9g53s4a9kv7k5xbfjhyaxjymgw62di6r-bash-interactive-5.3p9/bin:/nix/store/8kxpzdb09cbn252h8wl14zds9jgbw2q0-findutils-4.10.0/bin:/nix/store/hi1zxw2mg94cwrpbq1s8bbjm1k5y3vci-gnugrep-3.12/bin:/nix/store/r3fwyjb1k9v7y6qd5l2jq1sx3b46dx2y-gnused-4.10/bin:/nix/store/q7p8cvfxi5vplngni9a38n17ykhgcx1d-ripgrep-15.1.0/bin`; compiler's PATH: `/nix/store/p7ip75rls2g87d1yy4568djpa244c7ww-coreutils-9.11/bin`
- Models: sonnet, haiku (as reported: claude-haiku-5-5, claude-sonnet-5-5); effort: default; trials: 1

## Pass rates

| Model | pass@1 (± s.e. across tasks) | Tasks passed every time |
| --- | --- | --- |
| `sonnet` | 100.0% ± 0.0 | 3 of 3 |
| `haiku` | 100.0% ± 0.0 | 3 of 3 |

## Tasks

| Task | Model | Passed | Every time | Calls | Failed calls | Turns | Seconds | Output tokens | Input tokens | Cost (est. $) | First try | Verified |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `fixup` | `sonnet` | 1/1 | yes | 2.0 | 1.0 | 9.0 | 25 | 3484 | 165757 | 0.167 | 1 | 1 |
| `fixup` | `haiku` | 1/1 | yes | 3.0 | 1.0 | 12.0 | 54 | 11778 | 212250 | 0.013 | 1 | 1 |
| `thermostat` | `sonnet` | 1/1 | yes | 2.0 | 1.0 | 8.0 | 10 | 1224 | 73497 | 0.054 | 0 | 1 |
| `thermostat` | `haiku` | 1/1 | yes | 2.0 | 0.0 | 10.0 | 21 | 4197 | 139943 | 0.007 | 1 | 1 |
| `warren` | `sonnet` | 1/1 | yes | 1.0 | 0.0 | 10.0 | 30 | 3956 | 229072 | 0.189 | 1 | 1 |
| `warren` | `haiku` | 1/1 | yes | 2.0 | 0.0 | 8.0 | 73 | 15661 | 248605 | 0.020 | 1 | 1 |

Means over graded runs. Cost is Claude Code's estimate at API prices.

## Diagnostics and run-time errors

| Kind | Template | Count | Runs | Passed | Repeated | Ruby-shaped | Spans |
| --- | --- | --- | --- | --- | --- | --- | --- |
| error | `` `…` is a system and can't be called `` | 10 | 2 | 2 | 0 |  | arrive ×2, board ×2, report ×2, sail ×2, wait ×2 |
| error | `` can't assign to `…` `` | 2 | 2 | 2 | 0 |  | p.patience ×2 |
| error | `this query has 1 term, but its block takes 2 parameters` | 2 | 2 | 2 | 0 |  |  |
| error | `` `…` is a keyword and can't be used as a name `` | 1 | 1 | 1 | 0 |  | on ×1 |

## The reference as read

| File | Section | Runs |
| --- | --- | --- |
| `language.md` | (before the first section) | 4 |
| `language.md` | A first program | 4 |
| `language.md` | The model | 4 |
| `language.md` | Lexical structure | 5 |
| `language.md` | Types | 5 |
| `language.md` | Compact field types | 4 |
| `language.md` | Variables and values | 6 |
| `language.md` | What can be modified | 6 |
| `language.md` | Expressions | 6 |
| `language.md` | Control flow | 6 |
| `language.md` | Functions | 4 |
| `language.md` | Declaring data | 4 |
| `language.md` | Structs | 4 |
| `language.md` | Enums | 4 |
| `language.md` | Components and tags | 4 |
| `language.md` | Resources | 4 |
| `language.md` | Events | 4 |
| `language.md` | Constants | 4 |
| `language.md` | Entities | 4 |
| `language.md` | Queries | 4 |
| `language.md` | Systems and ticks | 5 |
| `language.md` | The window | 5 |
| `language.md` | Without a display | 3 |
| `language.md` | Keyboard and mouse | 3 |
| `language.md` | Commands for the keyboard and the mouse | 3 |
| `language.md` | GPU systems | 2 |
| `language.md` | What a GPU block may contain | 4 |
| `language.md` | What a GPU system may touch | 1 |
| `language.md` | Snapshot semantics | 0 |
| `language.md` | Arithmetic at 32 bits | 1 |
| `language.md` | Runtime errors in a GPU system | 0 |
| `language.md` | Random numbers in a GPU system | 0 |
| `language.md` | Determinism | 0 |
| `language.md` | Strict GPU systems | 1 |
| `language.md` | Without a GPU | 0 |
| `language.md` | The transfer plan | 1 |
| `language.md` | Several cores | 2 |
| `language.md` | Events | 2 |
| `language.md` | Built-ins | 1 |
| `language.md` | Output | 2 |
| `language.md` | Numbers | 2 |
| `language.md` | Strings | 2 |
| `language.md` | Lists | 1 |
| `language.md` | Random numbers, time and exit | 1 |
| `language.md` | Programs and files | 2 |
| `language.md` | What things cost | 1 |
| `language.md` | What a GPU system costs | 1 |
| `language.md` | What several cores cost | 2 |
| `language.md` | Errors | 1 |
| `language.md` | Grammar | 2 |
| `language.md` | Not in 0.7 | 2 |
| `README.md` | (before the first section) | 4 |
| `README.md` | Try it | 3 |
| `README.md` | The compiler | 3 |
| `README.md` | GPU systems | 3 |
| `README.md` | Several cores | 2 |
| `README.md` | A window | 2 |
| `README.md` | Keyboard and mouse | 2 |
| `README.md` | Benchmarks | 2 |
| `README.md` | Working on it | 2 |
| `README.md` | Status | 2 |
| `README.md` | License | 2 |
| `examples/battle.12am` | (whole file) | 1 |
| `examples/battle.out` | (whole file) | 0 |
| `examples/bench.12am` | (whole file) | 0 |
| `examples/bounce.12am` | (whole file) | 1 |
| `examples/bounce.out` | (whole file) | 1 |
| `examples/hello.12am` | (whole file) | 0 |
| `examples/hello.out` | (whole file) | 0 |
| `examples/input.12am` | (whole file) | 1 |
| `examples/input.drive` | (whole file) | 0 |
| `examples/input.out` | (whole file) | 0 |
| `examples/life.12am` | (whole file) | 1 |
| `examples/life.out` | (whole file) | 0 |
| `examples/life_rules.12am` | (whole file) | 0 |
| `examples/life_rules.out` | (whole file) | 0 |
| `examples/move.12am` | (whole file) | 0 |
| `examples/move.out` | (whole file) | 0 |
| `examples/nbody.12am` | (whole file) | 0 |
| `examples/nbody.out` | (whole file) | 0 |
| `examples/window.12am` | (whole file) | 0 |
| `examples/window.drive` | (whole file) | 0 |
| `examples/window.out` | (whole file) | 0 |

Runs of 6 graded that a tool result showed a line of the section to, below its heading.

## Exceptions

- Denied tool calls: 1 in 1 runs: Bash `sed -i 's/\bon\b = 0/n_on = 0/; s/    on = 2/    n_on = 2/; `

6 counted runs, 6 graded.
