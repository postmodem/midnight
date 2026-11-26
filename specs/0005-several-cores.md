# 0005 — Several cores: loops split across CPU threads, with the output of one

- **Status:** implemented
- **Depends on:** 0004
- **Tested on:** x86_64-linux, 2026-11-26

## Goal

A Midnight program runs on one CPU thread. A GPU system on a machine with
no usable GPU runs its blocks "one entity after another on one thread", and
so does every ordinary `each`, however many entities it visits and however
many cores sit idle.

After this spec two kinds of loop run on several cores:

- **A GPU block on the CPU.** Gravity among 16,384 bodies, 355 ms per tick
  on one core of a Ryzen 9 9950X3D, takes about 24 ms on its sixteen.
- **An ordinary `each` whose runs cannot affect one another.** The compiler
  decides which loops those are:

  ```ruby
  system age
    each Entity, Life do |e, life|
      life.left -= Time.dt
      if life.left <= 0.0
        Stats.expired += 1
        e.despawn
      end
    end
  end
  ```

A program prints exactly what it prints on one thread, whatever number of
threads it uses. Nothing is added to the syntax, and no program changes its
meaning: the number of threads is a matter of cost, as the transfer plan
is.

`midnight loops FILE` prints where each loop of a program runs, and for a
loop that stays on one core, what keeps it there.

Measurements on the development machine, summarised under "Measured in
planning", fixed the shape of the design.

## Scope

- The rule the whole spec rests on: a loop is run on several threads only
  when the result is exactly the result of running it on one.
- GPU blocks on the CPU, strict ones included, run in pieces on several
  threads.
- "Independent" as a property of an ordinary `each`, decided by the
  compiler: what the block may do and still be split. It covers writes to
  the loop's own rows, reads of anything the loop does not write, sums of
  whole numbers, queued structural changes and `emit`.
- One analysis of what every function and every `each` reads, writes and
  does, kept in the typed program.
- `midnight loops FILE`.
- A thread pool in the runtime, in a file of its own that only programs
  with a loop to split are built with.
- `MIDNIGHT_THREADS`: the number of threads, and words for the tests.
- Run-time errors in a split loop: the error reported is the one the loop
  reaches first on one thread.
- Tests that hold every program to its expected output for a range of
  thread counts, with pieces of one row and with pieces handed out in
  reverse, and a build of the suite under ThreadSanitizer.
- Benchmarks of the loops above on one thread and on all cores.
- The reference becomes Midnight 0.5 and the compiler 0.5.0.

## Non-goals

- Running several systems at the same time. Systems run one after another,
  in source order.
- `Float` sums on several cores. `total += pos.x` with a `Float` total
  means a sum from the first row to the last, and a loop that has one runs
  on one core.
- New things inside a GPU block: a sum collected across entities, `emit`,
  structural changes.
- A data-parallel system that computes at 64 bits.
- Strings and lists in a split loop. Reference counts stay as they are.
- `spawn`, `rand` and output in a split loop.
- An `each` on several cores when it is reached from inside another
  `each`.
- Declared entity counts. The runtime decides by the number of rows a loop
  has when it runs.
- Pinning threads to cores, and any use of more than one machine.
- macOS and AArch64 are built for and not tested here.

## Design

### The language

Nothing changes in the grammar, and no construct changes its meaning. The
reference gains a section, "Several cores", after "GPU systems", and the
changes to other sections listed at the end of this one.

**The guarantee.** A program's output, its exit status and the run-time
error it stops with, if any, do not depend on the number of threads. A
loop that runs on several cores gives the result the reference defines for
it: for an ordinary `each`, what visiting the entities one after another
gives; for a GPU block, its snapshot meaning. Which loops are split, into
how many pieces, and on how many threads, are matters of cost.

**Words.** A loop is **split** when its rows are divided into **pieces**,
runs of consecutive rows in visiting order, and the pieces are run by
several threads at once. Only a loop that is not inside another loop's run
is split: a GPU block, or an `each` reached when no other `each` is
running. An `each` nested in a block, or in a function called from one,
runs where it is reached, inside the piece.

**GPU blocks.** When a GPU block runs on the CPU (see "Without a GPU"), it
is split. Its meaning already makes every run separate from every other:
a run writes only its own entity's components, reads the snapshot, draws
random numbers from a sequence of its own, and an error is the earliest in
the source text whichever run meets it.

**Independent loops.** An ordinary `each` is split when it is
**independent**: when no run of its block can see what another run does.
Exactly, the block, together with every function it calls, directly or
through other functions, keeps to all of these:

1. **Plain values.** It reads, builds and assigns only plain values, as
   "GPU systems" defines them. A component with a string or a list among
   its fields can be a term; the block then uses its other fields.
2. **Its own rows.** It assigns to the component parameters of this `each`
   and to variables created inside the block. It assigns nothing else,
   apart from rules 4 and 5: no `e[C]`, no parameter of a nested `each`,
   no resource field, no `Time.dt`, no variable of the code around the
   loop.
3. **Nothing another run writes.** A component the block reads by `e[C]`,
   or through a parameter of a nested `each`, is not one that this `each`
   assigns through its own parameters.
4. **Sums of whole numbers.** `R.f += x` and `R.f -= x`, where `f` is an
   `Int` field of a resource, and `v += x` and `v -= x`, where `v` is an
   `Int` variable of the code around the loop, are allowed when they stand
   in the block itself and nothing the block reaches reads that field or
   variable, `x` included, or assigns it in any other way. A sum is
   written with `+=` or `-=`: `R.f = R.f + x` is an assignment under rule
   2. Several sums into one field are fine. `Int` addition wraps, so the
   total is the same in any order.
5. **Queued changes and events.** `despawn`, `add` and `remove`, on any
   entity, and `emit`. They are collected per piece and joined in visiting
   order when the loop ends, which is the order one thread makes them in.
   A component that is added, and an event, are plain values under rule 1.
6. **None of these:** `spawn`; `on`; `rand`, `rand(n)` and `srand`; `puts`
   and `print`; `clock`, `sleep` and `exit`; `World.stop`; `World.tick`
   and `World.run`, which a function may hold and which stop the program
   when reached inside an `each`; a `break` that leaves this `each`; and a
   `return` anywhere in the block, loops inside it included. A `return` in
   a called function belongs to that function.
7. **Device memory.** A component that some GPU block names as a term is
   reached only through this `each`'s own parameters: the block has no
   `e[C]` of it and no nested `each` that uses it.

An independent `each` may read its parameters, constants, `Time`, resource
fields, variables of the code around it, `e[C]`, `has?`, `alive?`,
`World.count`, and the parameters of nested `each`es, to any depth; it may
use `next`, loops and branches, and call functions, recursive ones
included.

These are rules about cost. A program means the same whether or not a loop
is independent, and a loop that is not independent runs on one core as it
always has.

```ruby
resource Stats
  expired: Int = 0
  drift: Float = 0.0
end

system move                         # independent
  each Position, Velocity do |pos, vel|
    pos.x += vel.dx * Time.dt
    pos.y += vel.dy * Time.dt
  end
end

system flee                         # independent: reads Position, writes Velocity
  each Position, Velocity, with: Prey do |pos, vel|
    each Position, with: Hunter do |hunter|
      vel.dx += (pos.x - hunter.x) * 0.01
      vel.dy += (pos.y - hunter.y) * 0.01
    end
  end
end

system drift                        # one core: a `Float` sum
  each Velocity do |vel|
    Stats.drift += vel.dx
  end
end

system jitter                       # one core: `rand`
  each Velocity do |vel|
    vel.dx += rand - 0.5
  end
end
```

**When an independent loop is split.** Each time the loop runs, by the work
it has: the number of rows, times the compiler's estimate of one run. A
loop with little work runs on the thread that reached it. The numbers are
fixed, not measured when the program runs, and the size of a piece depends
on the loop and on its rows, never on the number of threads: a program
makes the same pieces on every machine. "What things cost" has the
numbers.

**`midnight loops FILE`** prints one line for each `each` that is not
written inside another `each`, in source order:

```
swarm.12am:21  gpu system steer  each Boid               gpu
swarm.12am:40  system age        each Entity, Life       cores
swarm.12am:52  system flee       each Position, Velocity cores
swarm.12am:61  system drift      each Velocity           one core: `Stats.drift += …` at line 62 is a `Float` sum
swarm.12am:67  system jitter     each Velocity           one core: `rand` at line 68
swarm.12am:80  def nearest       each Position           one core: `return` at line 83
swarm.12am:95  top level         each Entity, Position   one core: `puts` at line 96
```

A line gives the place of the `each`, what it stands in, its terms, and
where it runs:

- **`gpu`**: a GPU block. It runs on the device, and in pieces on the
  cores when it runs on the CPU.
- **`cores`**: independent. An `each` in a function is split when the
  function is called from outside any `each`.
- **`one core`**, with the first thing in the block, in source order, that
  breaks a rule. Where that is in a function, the line names the function
  and the call: ``one core: `rand` in `pick` (line 120), called at line
  68``.

The reasons, one for each way of breaking a rule:

| Reason | Rule |
| --- | --- |
| ``uses a `String` `` / ``uses a list``, with the expression's line | 1 |
| ``assigns `e[C]` `` | 2 |
| ``assigns `C` through the nested `each` at line N`` | 2 |
| ``assigns `R.f` `` / ``assigns `Time.dt` `` / ``assigns `v` `` | 2 |
| ``assigns `R.f`; write `R.f += …` for a sum``, where an `Int` field or variable is assigned itself plus or minus something | 2 |
| ``reads `C`, which this loop assigns, by `e[C]` `` / ``… in the nested `each` at line N`` | 3 |
| `` `R.f += …` is a `Float` sum`` / ``… is a sum into a compact field`` | 4 |
| `` `R.f` is summed and also read`` / ``… and also assigned`` | 4 |
| `` `R.f += …` is in a called function`` | 4 |
| `` `spawn` ``, `` `on` ``, `` `rand` ``, `` `srand` ``, `` `puts` ``, `` `print` ``, `` `clock` ``, `` `sleep` ``, `` `exit` ``, `` `World.stop` ``, `` `World.tick` ``, `` `World.run` ``, `` `break` ``, `` `return` `` | 6 |
| ``reads `C`, which the GPU system `name` uses, by `e[C]` `` / ``… in the nested `each` at line N`` | 7 |

The output is the same on every machine and for every number of threads.

**Threads.** A program with a loop that can be split uses one thread for
each processor core it is allowed to run on, the thread that runs the rest
of the program among them. `MIDNIGHT_THREADS=N` sets the number; with 1 no
thread is created. A program with no GPU system and no independent `each`
has no threads and does not read the variable. The README describes the
variable and its other words.

**Run-time errors.** An independent block can fail as any ordinary code
can: `e[C]` on a dead entity or one without the component, division by
zero, a value that does not fit a compact field, a negative exponent. When
a split loop fails, the program stops with the error of the earliest
failing row in visiting order, with the message that row produces: the
error the loop stops with on one thread. Rows after it may have run;
nothing they did can be seen, since the program ends. Errors in a GPU
block follow "Runtime errors in a GPU system", as before.

**What things cost.** The table gains two rows:

| Operation | Cost |
| --- | --- |
| An independent `each`, or a GPU block on the CPU, with enough work | Split across the cores. Handing the pieces out costs a few microseconds, more when the threads have been idle. |
| The same, with little work | Runs on the thread that reached it, at no cost. |

and a section, "What several cores cost", with the measurements of the
Outcome and what follows from them:

- **Read `midnight loops`.** A `one core` line on a loop over many
  entities names the one thing to move out of it.
- **Keep the loop that computes apart from the loop that reports.** A
  `puts`, a `Float` total or a draw from `rand` keeps a whole loop on one
  core; in a second, small loop it costs nothing.
- **Count with `Int`.** A count or a whole-number total into a resource is
  free on several cores; a `Float` total is not.
- **Random numbers per entity belong in a GPU system**, whose `rand` is
  made for runs that happen at once.
- **Read what you do not write.** A loop that reads other entities'
  `Position` and assigns its own `Velocity` is independent. One that reads
  and assigns `Position` is not: split the component, or compute into a
  second component and copy.
- **Cores do not help a loop that waits for memory.** A light loop over
  arrays larger than the processor's cache runs at the speed of memory on
  one core, and on sixteen.
- **A GPU is still far ahead where it applies.** Gravity among 16,384
  bodies on sixteen cores takes about 30 times as long as on an RTX 5090.

**Other sections.**

- "Queries": the order of visiting is unchanged, and a split loop joins
  what its pieces queue and emit in that order.
- "Structural changes inside `each`" and "Events": one sentence each, that
  changes queued and events emitted by a split loop are in the order one
  thread makes them in.
- "Determinism": on the CPU a GPU system gives the same result on every
  run and for every number of threads.
- "Without a GPU": every GPU block runs on the CPU, split across the
  cores.
- "What a GPU system costs": the CPU timings stay, named as one thread's,
  with the timings on all cores beside them.
- "Errors": the paragraph above.
- "Not in 0.5": "running systems on several CPU threads" becomes "running
  several systems at the same time"; `Float` sums, strings and lists,
  `spawn`, `rand` and output in a loop on several cores join the list.

### The compiler

**One effect analysis** (`src/check/effects.rs`). The call graph that
`src/check/gpu.rs` builds moves to a module both use. On it, a fixpoint
gives every function a summary of what it does, directly or through the
functions it calls:

- components read and assigned by `e[C]`, and by query terms, each with
  the place of the first use;
- resource fields read and assigned, and `Time.dt` assigned;
- structural changes (`spawn` apart from the queued three), `emit`, `on`;
- output, `rand` and `srand`, `clock`, `sleep`, `exit`, `World.stop`;
- whether it handles a value that is not plain.

The same walk gives each `each` that is not inside another `each` its
verdict: independent, with its sums and its weight, or the first reason it
is not, with the place and the call path. `src/tir.rs` carries the
verdict on the `each` and the summaries on the program; both sides of that
contract change together. `midnight loops` and the emitter read the
verdict and nothing else.

**Weight.** An estimate of the work of one run, in operations, and never
less than 1:

- 1 for an arithmetic operation, a comparison, a field read or a field
  write, `has?` and `alive?`;
- 4 for `e[C]` and `World.count`;
- 8 for `/`, `%`, `**` and a `Math` function, and for `despawn`, `add`,
  `remove` and `emit`;
- for a branch, its condition and the heaviest of its arms;
- for a loop that is not a query, 8 times its body;
- for a call, the weight of the function, and 64 for one that recurses;
- for a nested `each`, the weight of its block times the rows its query
  has when the loop begins, wherever in the block it stands: in a branch,
  in a counted loop (times 8), inside another nested `each` (times that
  one's rows as well) or in a called function.

So a weight is a constant plus products of constants and row counts. The
compiler emits it as a C function over the row counts of the queries the
block can reach, with sums and products that stop at 2 ** 62. A GPU block
gets a weight the same way, from its lowered form.

**An independent `each` becomes a function.** Today an `each` is a loop
inside its system's C function. An independent one becomes
`static void LN(MnPiece *piece, const void *env)`, which runs the rows of
one piece, with the body as it is emitted today: column pointers,
`restrict` and hoisted reads under the existing conditions. Every value
of the surrounding code that the block reads is copied into a struct,
`env`, before the loop: variables, the parameters of the function, `self`
and the fields of a method's receiver, the parameters of a counted loop
or an `on` around it, and the resource and `Time` fields it reads. A sum
adds into `piece->sums[k]`. The statement itself becomes: fill `env`, call
`mn_loop(&loopN, &env)`, add the totals `mn_loop` returns to the fields
and variables they belong to. `loopN` holds the function, the query, the
weight, the number of sums, and the queries of every nested `each` and
`World.count` the block can reach, which `mn_loop` brings up to date
before any piece runs. On one thread the same function runs once, over
all the rows.

**GPU blocks.** `GbN_cpu` runs the rows of one table from `run->first` up
to `run->end`. The error word it lowers belongs to the piece. Everything
else in the generated C of a GPU block stays as it is, and the shader does
not change.

**`midnight loops`** (`src/loops.rs`, and the command in `src/main.rs`)
prints the verdicts.

**The driver** builds a program that has a GPU system or an independent
`each` with `-DMN_THREADS -pthread` and with `runtime/midnight_pool.c`.
Other programs are built as today.

### The runtime

**The pool** (`runtime/midnight_pool.c`, with `runtime/midnight_pool.h`
for what it and `midnight_rt.c` need from each other). C11 with libc and
POSIX threads; generated code never includes its header.

- Created at the first loop that is split: one thread less than the thread
  count, each with every signal blocked and a stack as large as the main
  thread's limit, or 8 MiB when that limit is unlimited.
- One loop at a time. A loop is a function, a context, a number of pieces
  and the number of threads that take part, the calling thread among
  them.
- Each thread taking part has a region of its own, an equal share of the
  pieces in order, with a counter it claims from. A thread whose region is
  empty claims from the regions of the others. The same thread has the
  same region whenever the loop runs with the same rows, so rows stay in
  the cache of the core that had them last; and no loop depends on a
  particular thread arriving, since any thread finishes any region.
- A thread with nothing to do polls for 50 µs, as a tick waiting for the
  GPU does, and then blocks: on a futex on Linux, on a mutex and a
  condition variable elsewhere and in a ThreadSanitizer build. The caller
  wakes the threads taking part that are blocked, always the
  lowest-numbered ones, and no others.
- The thread count is `MIDNIGHT_THREADS` when it is set. Otherwise it is
  the number of cores among the processors the process may run on: on
  Linux, the affinity mask with one processor counted per core, from
  `/sys/devices/system/cpu`, and every processor of the mask where that
  cannot be read; on macOS, the performance cores.
- `mn_finish` stops the threads before the debug build counts live
  buffers.

**Pieces.** The rows of a loop are numbered in visiting order across the
tables of its query, and piece `k` is rows `k * P` up to `(k + 1) * P`,
whatever table they are in. This holds for a GPU block as for an ordinary
loop: a piece that crosses from one table into the next runs the rows of
each in turn. With `W` the weight of one run and `work` the rows times
`W`, both stopping at 2 ** 62:

- `P` is 25,000 divided by `W`, and at least 1. For the `move` above,
  with a weight of 10: pieces of 2,500 rows, about 0.7 µs of work each.
- A loop is split when `work` is at least 800,000, it has two rows or
  more and the thread count is above one. For that `move`: from 80,000
  rows, 22 µs of work. Thirty-one bodies that each visit a million are
  split, into thirty-one pieces.
- One thread takes part for every 200,000 of `work`: at least two, and at
  most the thread count and the number of pieces.

All three depend on the loop and its rows only. The numbers are set from
"Measured in planning" and may be tuned while the benchmarks are built;
the Outcome records them. With `small` in `MIDNIGHT_THREADS`, `P` is 1, a
loop of two rows or more is split, and as many threads take part as there
are pieces, up to the thread count.

**When a loop is not split.** `mn_loop` and `mn_gpu_run` run a loop on the
thread that reached it, as one piece, when its work is under the
threshold, when the thread count is one, and when it is reached inside
another `each` or inside a piece. What it queues and emits then goes where
it goes today: to the world's queue and buffers, or to the piece that
thread is running. Nothing is applied before the outermost `each` ends.

**An ordinary loop** (`mn_loop` in `midnight_rt.c`). Before the pieces:
bring the loop's queries up to date, raise `defer_depth`, which stays
raised until the pieces have been joined, and with a device in use, make
the CPU copy of the loop's own terms current, as the loop does today.
Each piece has:

- its queued structural changes and its emitted events, written into the
  buffers of the thread that runs it, with the piece's stretch of each
  recorded;
- its sums;
- for each assigned term, whether an assignment ran, per table.

`mn_despawn`, `mn_add`, `mn_remove` and `mn_emit` find the piece a thread
is running through a thread-local pointer and append there;
`mn_defer_enter` and `mn_defer_leave` do nothing inside a piece. After the
last piece: the changes are appended to the world's queue and the events
to their buffers, piece by piece in order; the sums are added up;
`mn_wrote` is called for each table and term that was assigned; then
`defer_depth` is lowered and the queue is applied as it is today when the
outermost `each` ends.

**A GPU block on the CPU** (`mn_gpu_run`). The header, `fill`, the device
decision and the snapshot stay on the calling thread. The column pointers
of every table are prepared before the pieces, so that pieces of different
tables can run at once. Each piece has its own error word; the smallest
is the block's.

**Errors in an ordinary loop.** A thread sets a jump point before each
piece it runs. `mn_panic` on a thread that is running a piece records the
row and the message, if no earlier row has failed, and jumps back there.
From then on every thread, that one included, claims only pieces before
the earliest failure, from any region. The first thread to find all of
them finished reports the earliest failure: it flushes, prints and ends
the program, as `mn_panic` does today. Nothing waits for a piece that
comes after the failure, whichever thread is running it.

**The environment.** `MIDNIGHT_THREADS` is a comma-separated list, read
once, by programs built with `MN_THREADS`:

| Word | Effect |
| --- | --- |
| a number, 1 to 256 | the thread count |
| `small` | every piece is one row, every loop of two rows or more that can be split is, and a thread takes part for each piece |
| `reverse` | every region is claimed from its last piece to its first |
| `jitter` | each thread yields at random, from a fixed seed, before each claim |
| `awake` | threads never block: one with nothing to do keeps polling, and uses a core to do it |
| `info` | prints the thread count on standard error, and one line for each loop the first time it is split |

`small`, `reverse` and `jitter` are for the tests. `awake` is for a
program whose split loops come more than a few milliseconds apart, where
a thread that blocked returns on a slowed core. No word changes a
program's output.

**The contracts.** `runtime/midnight_rt.h` gains `MnPiece`, `MnLoop`,
`mn_loop`, the two fields of `MnGpuRun`, and comments that say which
functions a piece may call. Its sentence "Nothing here is thread-safe"
becomes the list of what is: the functions a piece may call, on state no
other piece writes.

### Measured in planning

On the development machine: a Ryzen 9 9950X3D, 16 cores and 32 hardware
threads in two groups of eight cores with separate caches (96 MiB and
32 MiB), Linux 6.18, clang 21, glibc 2.42. Hand-written C in the layout
Midnight emits, built with the flags of a release build.

**A loop of heavy runs.** Gravity among 16,384 bodies, the block of
`bench/gpu_gravity.12am`, with the snapshot copied by the calling thread
and pieces of 64 rows; ms per tick, median:

| Threads | 1 | 2 | 4 | 8 | 16 | 32 |
| ------- | ----- | ----- | ---- | ---- | ---- | ---- |
| ms      | 355.8 | 178.0 | 90.7 | 46.6 | 24.3 | 24.4 |
| × one   | 1.00  | 2.00  | 3.92 | 7.64 | 14.7 | 14.6 |

The checksum over all bodies is the same for every thread count, piece
size and placement of threads. Sixteen threads placed two to a core on
eight cores take 47 to 48 ms, as eight threads do: the second thread of a
core adds nothing. With as many pieces as threads, sixteen threads take
34 to 36 ms, since the loop then waits for its slowest thread; with four
other busy threads in the process, 44.7 ms against 32.8 for pieces of 64
rows. 4,096 bodies: 22.3 ms on one thread, 1.48 on sixteen; 65,536:
5,691 and 385. The RTX 5090 takes 0.84 ms for 16,384.

**A loop of light runs.** `pos.x += vel.dx * Time.dt` and the same for
`y`, as an ordinary `each` emits it (64-bit arithmetic on `Float32`
fields); 0.28 ns per row on one thread while the arrays fit the cache.
Times as fast as one thread, with a region per thread:

| Rows        | 8 threads | 16 threads |
| ----------- | --------- | ---------- |
| 300,000     |           | 9.2        |
| 1,000,000   | 7.0 to 7.4 | 11.8 to 12.9 |
| 3,000,000   | 8.8       | 15.9       |
| 10,000,000  |           | 3.25       |

At 100,000,000 rows, 2.4 GB of traffic per pass, no thread count is more
than 10 % faster than one: one thread already moves 57 GB/s. With every
thread claiming from one shared counter, sixteen threads reach 4.5 times
at 1,000,000 rows where eight on one cache group reach 7.3: rows change
cores from pass to pass, and half of those moves cross the two cache
groups. Threads were not pinned in any of these runs.

**What a split costs.** With an empty body, the time until sixteen
threads are all in a loop and out again: 3.1 µs when the loops follow one
another, 8.4 µs after 1 ms without one, 21 µs after 10 ms; with eight,
2.2, 3.9 and 16. A condition variable costs the same as a futex up to
eight threads and about three times as much at sixteen when the loops
follow one another. Creating the threads takes 66 µs for eight and 154 µs
for sixteen, which the pool pays once.

A core that has been idle for 2 ms or more runs at about half speed for
its first half millisecond (driver `amd-pstate-epp`, governor
`powersave`, preference `balance_performance`, as this machine is set
up). So after 4 to 16 ms without a split loop, every gain is about half
what it is after 1 ms: 4 ms of work run 7.4 times as fast on sixteen
threads, against 15.2. Threads that poll through the gap keep the whole
gain.

Which blocked thread wakes matters for the same reason. A loop for two
threads on a pool of sixteen, once every millisecond, runs 0.77 to 1.10
times as fast as one thread when the kernel chooses the thread, since it
takes the one that has slept longest, on the coldest core; 1.76 to 2.01
times when the same threads are woken every time.

**From how much work a split pays.** A body of 52 ns per row, pieces
handed to threads that had been blocked for 1 ms; times as fast as one
thread:

| Work on one thread | 2 threads | 4 | 8 | 16 |
| ------------------ | ---- | ---- | ---- | ---- |
| 8 µs               | 1.31 | 1.77 | 1.82 | 1.32 |
| 16 µs              | 1.57 | 2.44 | 2.73 | 2.12 |
| 32 µs              | 1.77 | 2.66 | 4.04 | 3.56 |
| 64 µs              | 1.87 | 3.45 | 5.38 | 5.54 |
| 256 µs             | 1.97 | 3.81 | 7.03 | 10.7 |
| 1 ms               | 1.99 | 3.96 | 7.75 | 14.0 |

Bodies of 0.3, 8, 52 and 500 ns per row cross at the same amount of work:
a split is even from 4 to 8 µs and half as fast again from 8 to 16 µs.
After 16 ms idle, 16 µs of work gains nothing and 64 µs gains 1.3 to 2.5
times. The best number of threads is about one for every 4 to 8 µs of
work. With threads that never block the crossing is at 1.5 µs, and a tick
that is nine tenths one thread's work then uses 9 times the processor
time at sixteen threads for the same time on the clock; a 50 µs poll uses
1.5 times, and blocking at once 1.03.

**Pieces.** A piece worth a quarter of a microsecond or more costs under
10 % to claim from a thread's own counter. Sixty-four tables of 100 rows
with a light body take 1.8 µs on one thread, and 51 to 137 µs if each
table is handed to the threads on its own; a thousand tables of a
thousand rows, numbered through as one run of rows, take what one table
of a million takes. Piece boundaries off the cache line cost nothing
measurable at 1,024 rows and above.

**What pieces join.** A loop over a million rows of which half append a
16-byte record to their piece's buffer, joined in piece order afterwards:
the result is byte for byte the one-thread loop's, the join costs 0.4 ns
per record, and one thread loses 6 % to it. With 1 % of rows appending,
sixteen threads take 80 µs where one takes 1,076.

**Sums.** A 64-bit integer sum over a million rows takes 59 µs on one
thread (clang vectorises it) and 9.9 on eight, with the same total. A
`Float` sum is 362 µs on one thread and is not vectorised. Summed in
pieces of 4,096 rows joined in order, it gives one result for every
thread count from 1 to 32, and that result differs from the one-thread
loop's in the last bits, for `Float32` values and for `Float` alike.

**Reference counts.** One retain and release of a string: 0.49 ns with
the plain count, 6.99 ns with an atomic one on a single thread, and 37 to
418 ns each when 2 to 16 threads share the buffer.

**The toolchain.** With clang 21 and glibc 2.42 a program that uses
POSIX threads links with no flag added, and `<threads.h>` is present; it
is absent on macOS.

### Tests

- **Every program, several ways.** Every program of `tests/run`,
  `tests/panic` and `examples` that has a GPU system or an independent
  `each` runs, in release and debug, with `MIDNIGHT_THREADS` set to `1`,
  to `4`, to `3,small`, to `7,small,reverse`, to `64,small,jitter` and to
  `2,small,awake`, and must print its `.out` or stop with its `.err` each
  time. With `small` a table of five rows is five pieces on up to five
  threads, so the small programs of the suite are split.
- **That loops are split.** Each new run program below has a `NAME.split`
  file: the lines `MIDNIGHT_THREADS=4,small,info` prints on standard
  error, one for each loop the first time it is split. The suite requires
  exactly those lines, so a loop that `midnight loops` calls `cores` and
  the runtime never splits fails a test.
- **On a device.** In GPU mode, `gpu_programs`, `gpu_stats_match_plan` and
  `gpu_stats_match_expected` run once more with
  `MIDNIGHT_THREADS=4,small`: the output and the `.stats` counts must be
  the ones they are without it.
- **`tests/loops/NAME.12am` with `NAME.loops`:** `midnight loops` must
  print exactly that. Each program begins with an `# expect:` comment. One
  program for every reason in the table above, one for each thing an
  independent loop may do, and one for an `each` in a function.
- **New run programs**, each with rows in several tables and counts on
  both sides of a piece boundary: a loop that queues `despawn`, `add` and
  `remove` on its own and on other entities, with the order of the result
  visible in later output; a loop that emits, with an `on` that prints the
  events in order; sums into a resource and into a variable, with a total
  that wraps; a loop that reads other entities by `e[C]` and by a nested
  `each`; a loop in a function called from outside and from inside an
  `each`; a loop in a method that reads `self`, and one inside an `on`
  that reads its parameter; a strict and a non-strict GPU block with rows
  in several tables; a GPU block with `rand`.
- **New panic programs:** two rows that fail with different messages, in
  different pieces, the later one first in the source; every row failing;
  a failing row with, in a later piece, a row that never ends; a GPU
  block with two failing operations; `World.tick` in a function called
  from a loop that keeps every other rule. A panic test that has not
  stopped after ten seconds fails.
- **ThreadSanitizer.** With `MIDNIGHT_TEST_TSAN=1` the suite builds every
  program above in release mode with `-fsanitize=thread` through
  `MIDNIGHT_CFLAGS`, runs it with `4,small` and `7,small,reverse`, and
  fails on any report. `checks.tsan` does this in the build sandbox.
- **The pool on its own** (`tests/pool/stress.c`, built with
  `midnight_pool.c`): a million loops of a few pieces back to back, loops
  after an idle time, more threads than processors, one processor
  (`taskset`), start and stop a thousand times.
- **Unit tests** of the effect summaries and of each rule.
- `tests/COVERAGE.md` gains the section and its rules.
- **Benchmarks.** `bench/cores.sh`, run by `nix run .#bench-cores`, runs
  with `MIDNIGHT_GPU=off`, on one thread and on the default number:
  `gpu_gravity`, `gpu_gravity_strict`, `gpu_move`, `examples/bench.12am`
  (the ordinary `move`), `gpu_read_tables` (a GPU block over 64 tables),
  and three new programs: `cores_age` (the `age` system above over a
  million entities, of which about a thousand expire in each tick),
  `cores_flee` (a nested `each`), and `cores_small` (64 tables of 100
  rows, which must stay on one thread). It prints the time per tick of
  each and requires the two checksums of each program to be equal.

### Slices

1. **The pool, and whether it can be tested here.** `midnight_pool.c`,
   its stress test, and a ThreadSanitizer build of that test, on this
   machine and in the build sandbox. If ThreadSanitizer cannot run in
   either place, stop and report before going on.
2. **GPU blocks in pieces.** The contract, the emitter, `mn_gpu_run`,
   `MIDNIGHT_THREADS`, the suite's thread runs, the gravity benchmarks.
3. **The effect analysis and `midnight loops`.** Summaries, verdicts,
   `tir`, the command, `tests/loops`. No change to generated code.
4. **Independent loops that only assign their own rows.** Loop functions,
   `mn_loop`, errors.
5. **Sums, queued changes and events.**
6. Benchmarks, reference 0.5, README, COVERAGE, the repository's
   `CLAUDE.md`, Outcome.

## Acceptance criteria

All run on x86_64-linux, on the development machine.

1. `nix flake check` passes, with `checks.tsan` in it, and
   `nix run .#gpu-test` passes on the hardware device.
2. Every `.out`, `.err`, `.plan` and `.stats` file that existed in 0.4.0
   is unchanged. For every program of `tests/` and `examples/` that
   existed in 0.4.0, the generated shaders are byte for byte what 0.4.0
   gives, and so is `midnight emit-c` for the programs with no GPU system
   and no independent `each`.
3. Every program of `tests/run`, `tests/panic` and `examples` with a GPU
   system or an independent `each` prints exactly its expected output, in
   release and debug, for each of the six settings of `MIDNIGHT_THREADS`
   under "Tests"; every new run program prints its `.split` lines; and on
   the hardware device and on llvmpipe the GPU-mode tests pass with
   `MIDNIGHT_THREADS=4,small` as without.
4. The ThreadSanitizer runs finish with no report.
5. `bench/gpu_gravity.12am` with `MIDNIGHT_GPU=off` prints the same
   checksum for 1, 2, 16 and 32 threads, and on the default number of
   threads a tick takes at most a twelfth of what it takes on one. The
   same holds for `gpu_gravity_strict`.
6. `cores_age`, `cores_flee` and `examples/bench.12am` print the same
   checksum on one thread and on the default number. On the default
   number `cores_flee` runs at least ten times as fast as on one thread,
   and `cores_age` and `examples/bench.12am` at least six times.
7. With `MIDNIGHT_THREADS=1`, `nix run .#bench` and `nix run .#bench-gpu`
   show no row of 0.4.0's output more than 10 % slower.
8. `cores_small`, `gpu_read_tables` with `MIDNIGHT_GPU=off`, and every
   row of `nix run .#bench`, take no more than 10 % longer on the default
   number of threads than on one.
9. The panic programs with failing rows stop with the error of the
   earliest row for every setting of criterion 3, and the one with a row
   that never ends stops within the ten seconds.
10. Every reason `midnight loops` can print has a `tests/loops` program,
    and `tests/COVERAGE.md` maps the rules to tests.
11. A program with no GPU system and no independent `each` is built
    without `midnight_pool.c`: `nm` on the release build of
    `examples/hello.12am` shows no `pthread_create`, and `ldd` lists what
    it lists on 0.4.0.
12. The reference says 0.5 and `midnight --version` 0.5.0, and every
    command in the README works as written.

## Decisions

- **Exactly the result of one thread, or one core.** With that rule the
  number of threads, the size of a piece and the choice to split can each
  be made for speed alone, by the runtime, from the rows a loop has. It is
  also why no expected output changes.
- **The compiler finds the loops.** Splitting cannot change a result, so
  which loops are split is no part of what a program means, and the
  source says nothing about it. The compiler sees the whole program and
  every access in it, so the verdict is exact, static and the same on
  every machine, and `midnight loops` prints it with its reason.
- **Loops, one at a time.** A loop over many rows is where the work is,
  and a piece of one is the right size to hand to a thread. Systems stay
  in source order, with nothing running beside them.
- **`Float` sums stay on one core.** A sum computed in pieces is a
  different number from the sum from first row to last, and the meaning of
  `total += x` stays what it is.
- **Whole-number sums, queued changes and events are in**, because the
  loops that count, expire and report by event are the common ones, and
  joining pieces in order costs next to nothing.
- **Reference counts stay plain.** An atomic count costs fourteen times a
  plain one on every program, split or not, so a block that handles a
  string or a list is left on one core.
- **Pieces of a fixed number of rows, a region of them for each thread.**
  The same pieces on every machine; more pieces than threads, so that a
  late thread costs little; rows that stay on the core that had them
  last; and no thread whose arrival a loop depends on, since any thread
  finishes any region.
- **The same threads are woken every time**, the lowest-numbered: a
  thread that ran a moment ago is on a core still at speed.
- **One thread per core.** A second thread on a core adds nothing to the
  loops measured and slows the thread it shares the core with.
- **Polling briefly, then blocking**, as the wait for the GPU does: loops
  that follow each other closely find the threads awake, and a tick that
  is mostly one thread's work burns nothing.
- **The pool in a file of its own**, built only into programs with a loop
  to split, as the GPU code is: `midnight_rt.c` stays C11 with libc and
  libm, and a program without such a loop is built and runs as today.

## Outcome

Implemented, with the differences listed under "Differences from the
design". Eleven of the twelve criteria pass on x86_64-linux: a Ryzen 9
9950X3D (sixteen cores) beside an RTX 5090, `llvmpipe` from Mesa 26.1.8,
clang 21.1.8, glibc 2.42. Criterion 6 passes for two of its three programs:
`cores_age` gains 3.6 to 4.0 times where it asks for six.

### Acceptance criteria

| #  | Result |
| -- | ------ |
| 1  | `nix flake check` passes: the package's suite in the sandbox, `checks.gpu` on `llvmpipe` and `checks.tsan` (`tsan_pool_stress` and `tsan_programs`). `nix run .#gpu-test` passes on the RTX 5090. |
| 2  | Against `5287dcd` (0.4.0): none of its `.out`, `.err`, `.plan` and `.stats` files and none of its programs differs. `midnight emit-wgsl` prints the same bytes for all 394 programs of `tests/run`, `tests/panic`, `tests/plan`, `examples` and `bench` that 0.4.0 has, and `midnight emit-c` for the 257 of them with no GPU system and no independent `each`. |
| 3  | `thread_programs`: 119 programs, release and debug, each of the six settings. `splits_match_expected`: the eight new run programs. `gpu_programs`, `gpu_stats_match_plan` and `gpu_stats_match_expected` pass with `MIDNIGHT_THREADS=4,small` on the 5090 and on `llvmpipe`. |
| 4  | No report: the pool's stress test and 119 programs with `4,small` and `7,small,reverse`, on this machine and in the sandbox. |
| 5  | `gpu_gravity` and `gpu_gravity_strict` with `MIDNIGHT_GPU=off` print 1074603.7585074664 for 1, 2, 16 and 32 threads. On sixteen threads a tick takes a 14.8th and a 14.4th of what it takes on one. |
| 6  | The three programs print the same checksum on one thread and on sixteen. `cores_flee` runs 14.5 to 14.7 times as fast and `examples/bench.12am` 9.0 to 10.2 times. **`cores_age` runs 3.6 to 4.0 times as fast, not six**: see "Measured". |
| 7  | With `MIDNIGHT_THREADS=1`, no row of `nix run .#bench-gpu` or `nix run .#bench` is more than 10 % slower than 0.4.0. Rows that differed by more in a single run were run again, the two versions in turn: see "Measured". |
| 8  | `cores_small` and `gpu_read_tables` take the same time on sixteen threads as on one (0.0020 and 0.0038 ms). In `nix run .#bench`, `bench.12am` takes 0.03 ms against 0.29; `churn.12am` has no loop to split and no threads. |
| 9  | `cores_two_failures`, `cores_every_row_fails`, `cores_failure_in_function` and `cores_gpu_two_failures` stop with the error of the earliest row for every setting, and `cores_failure_before_endless` within the ten seconds. The first two and the last were also run 640 times each with `64,small,jitter` and with `7,small,reverse` while 32 copies ran at once. |
| 10 | 37 programs in `tests/loops`, one or more for every reason; `tests/COVERAGE.md` has the section. |
| 11 | `nm` on the release build of `examples/hello.12am` shows no `pthread_create` and no `mn_pool_` symbol, and `ldd` lists `libm.so.6`, `libc.so.6` and the loader, as on 0.4.0. The program prints nothing for an unknown word in `MIDNIGHT_THREADS`. |
| 12 | The reference says 0.5 and `midnight --version` prints `midnight 0.5.0`. Every command in the README was run as written, `./move` built into a scratch directory. |

### Measured

All with nothing else running. The numbers of "Pieces" are as designed:
25,000 of work to a piece, a split from 800,000, a thread for every
200,000.

`nix run .#bench-cores`, fastest of 5, ms per tick, two runs:

| Loop | 1 thread | 16 threads | Times |
| ---- | -------- | ---------- | ----- |
| `gpu_gravity`, a GPU block | 354.0, 354.3 | 23.89, 23.89 | 14.8 |
| `gpu_gravity_strict` | 572.6, 574.6 | 39.86, 39.94 | 14.4 |
| `gpu_move`, a GPU block over 1,000,000 | 0.104, 0.102 | 0.019, 0.020 | 5.1 to 5.5 |
| `examples/bench.12am`, an `each` over 1,000,000 | 0.285, 0.289 | 0.028, 0.032 | 9.0 to 10.2 |
| `cores_age` | 0.2535, 0.2444 | 0.0636, 0.0682 | 3.6 to 4.0 |
| `cores_flee` | 1.256, 1.243 | 0.0854, 0.0858 | 14.5 to 14.7 |
| `gpu_read_tables` | 0.0038 | 0.0038 | 1.00 |
| `cores_small` | 0.0020 | 0.0020 | 1.01 |

`gpu_gravity` with `MIDNIGHT_GPU=off` takes 177.3 ms on 2 threads and
24.1 ms on 32; `gpu_gravity_strict` 287.5 and 30.7.

**`cores_age`.** What a split loop queues is applied by one thread when
the loop ends. With the number of entities that expire in a tick changed,
fastest of 5, ms per tick:

| Expire in a tick | 1 thread | 16 threads | Times |
| ---------------- | -------- | ---------- | ----- |
| none             | 0.2232   | 0.0229     | 9.7   |
| 100              | 0.2243   | 0.0336     | 6.7   |
| 1,000            | 0.2448   | 0.0706     | 3.5   |
| 10,000           | 0.2997   | 0.2155     | 1.4   |

On one thread a thousand despawns add 22 µs to the tick; after a loop on
sixteen threads they add 48 µs, the rows they touch having last been
written on other cores. That is a tenth of the one-thread tick: with the
loop itself at 9.7 times, six times cannot be reached at a thousand a
tick.

`nix run .#bench-gpu`, fastest of 3, ms per tick: 0.4.0 built from
`5287dcd`, then 0.5.0 with `MIDNIGHT_THREADS=1`, then 0.5.0 on its sixteen
threads.

| Row | 0.4.0 GPU | 0.5.0 GPU | 0.4.0 CPU | 0.5.0 CPU, 1 thread | 0.5.0 CPU, 16 |
| --- | --------- | --------- | --------- | ------------------- | ------------- |
| Gravity, 16,384 bodies | 0.842 | 0.842 | 355.3 | 355.4 | 23.8 |
| Gravity, 16,384 bodies, strict | 7.149 | 7.122 | 574.6 | 576.5 | 40.2 |
| Gravity, 65,536 bodies | 3.668 | 3.636 | | | |
| Move 1,000,000, data stays | 0.028 | 0.029 | 0.102 | 0.105 | 0.019 |
| Move 1,000,000, read each tick | 0.810 | 0.810 | 0.504 | 0.512 | 0.435 |
| `gpu_block` | 0.0226 | 0.0227 | 0.0 | 0.0 | 0.0 |
| `gpu_blocks8` | 0.0438 | 0.0487 | 0.0003 | 0.0003 | 0.0003 |
| `gpu_fallible` | 0.0288 | 0.0288 | 0.0011 | 0.0011 | 0.0011 |
| `gpu_read_tables` | 0.1630 | 0.1439 | 0.0038 | 0.0037 | 0.0037 |
| `gpu_poke` | 0.0464 | 0.0466 | 0.1040 | 0.1020 | 0.0191 |
| `gpu_spawn` | 0.0447 | 0.0306 | 0.0969 | 0.1063 | 0.0190 |
| `gpu_despawn` | 0.0359 | 0.0345 | 0.1025 | 0.1008 | 0.0209 |
| `gpu_tag` | 0.0598 | 0.0615 | 0.1060 | 0.0975 | 0.0205 |
| `gpu_rare_write` | 0.5831 | 0.5849 | 0.2946 | 0.2993 | 0.1103 |
| `gpu_overwrite` | 0.0661 | 0.0704 | 0.1537 | 0.1535 | 0.0753 |

The GPU times of the small rows vary by a tenth or more from run to run.
Forty runs of each version in turn, on the GPU, ms per tick as minimum and
median: `gpu_blocks8` 0.0437 and 0.0480 on 0.4.0, 0.0438 and 0.0491 on
0.5.0; `gpu_block` 0.0225 and 0.0240, 0.0225 and 0.0237; `gpu_tag` 0.0578
and 0.0614, 0.0582 and 0.0636; `gpu_overwrite` 0.0690 and 0.0825, 0.0639
and 0.0752. Twelve runs of `gpu_spawn` on the CPU: 0.0968 and 0.1031,
0.0984 and 0.1055. On its sixteen threads 0.5.0 runs the ordinary loops of
`gpu_overwrite` and `gpu_rare_write` in pieces, and their GPU rows take
0.0285 and 0.5155 ms.

`nix run .#bench`, two runs: `bench.12am` 0.291 and 0.281 ms per tick on
0.4.0, 0.287 and 0.288 on 0.5.0 with one thread, 0.03 on sixteen;
`baseline.c` 0.285 to 0.288. `churn.12am` compiles to the same C on both
versions and its single runs vary by half; over twenty runs of each in
turn the best and the median, in millions per second, are: `spawn` 96.3
and 94.8 on 0.4.0, 96.4 and 92.9 on 0.5.0; `lookup` 465.9 and 200.4, 465.9
and 201.0; `add` 39.9 and 24.2, 39.7 and 23.9; `remove` 58.9 and 57.0, 58.9
and 57.4; `despawn` 133.9 and 128.6, 134.2 and 128.5.

The pool's stress test: a million loops of 2 to 8 pieces, one after
another on four threads, take 0.51 s, and 7.4 s under ThreadSanitizer.

Two checks of the tests themselves. With the pieces joined in the reverse
order, `cores_queued` and `cores_emit` fail with `4` and with `4,small`.
With a pool whose waiting threads go on claiming, the stress test's
`swarm` mode fails to end in 480 runs of 480 while 32 copies run at once;
with the pool as it is, it ends in all 480.

### Differences from the design

- **A loop's function runs the tables of a piece itself.** `MnPiece` is
  the tables to run, the first row of the first and the end of the last,
  the sums and the flags; the function holds the loop over the tables and
  counts the rows of each from 0, as the code of an `each` that is not
  split does. So a loop that is not split is one call, and the C compiler
  treats the rows of both kinds of loop alike.
- **`env` holds locals only.** A loop's function reads the resource fields
  and the fields of `Time` where they are: nothing assigns them while
  pieces run. A local that holds a string is copied by its bits, and the
  function uses its other fields.
- **Sums** are collected in variables of the function and added to
  `piece->sums` when it returns.
- **Failure is the pool's.** `mn_pool_fail` lowers the piece no thread
  starts at or after; a thread that has been through every region waits,
  claiming nothing, until every piece before that one has finished, and
  calls the loop's `failed`. `mn_panic` in a piece keeps the message with
  the earliest piece under a mutex and leaves the piece with
  `siglongjmp`.
- **`midnight_rt.c` with `MN_THREADS`** uses a POSIX mutex, thread-local
  storage (`mn_loop_thread`, which `mn_defer_enter` and `mn_defer_leave`
  test) and `sigsetjmp`. Without it the file is C11 with libc and libm.
- **What pieces queue and emit** is kept in buffers of the thread that
  runs them, as stretches by piece, and handed to the world piece after
  piece through a list for each piece; stretches for few of very many
  pieces are sorted instead.
- **The flags of assigned terms** are stored without a branch
  (`piece->wrote[…] |= w`): a test of the flag after the row loop kept
  the C compiler from unrolling it.
- **With one thread taking part** the pieces run in order, whatever
  `reverse` says.
- **`MnGpuBlock`** gains `weight`, `inner_weight` and `loc`. A GPU block
  has one nested `each`, so its weight is a number plus a number for each
  row of the snapshot; the strict `/` and `sqrt` weigh what the hardware
  operations do.
- **The weight** counts nothing for a read of a variable, a constant, a
  resource field or a field of `Time`, nor for assigning a whole
  variable. The `move` of the design weighs 10 and `age` 15.
- **`info`** prints `midnight: threads: N` and, for a loop that is split
  for the first time, `midnight: threads: FILE:LINE: R rows in P pieces on
  T threads`. A `.split` file holds those lines without the prefix.
- **`midnight loops`** puts two spaces between its columns, and every
  reason has its place: `at line N`, or ``in `f` (line N), called at line
  K``. A loop in a method stands in `def Type.name`. A sum written with
  `-=` is printed as `+= …`.
- **Rules.** A variable of the code around the loop may hold a string
  when the block reads its other fields. A nested `each` counts for rules
  3 and 7 when its block uses the parameter of that component.
- **Tests beyond the list:** `cores_failure_in_function`; the stress
  test's `swarm` mode; `pieces_and_threads_follow_the_work` and
  `only_programs_that_split_read_the_threads_variable` in the suite; unit
  tests of the block weight and of what an independent loop generates. On
  a device the runs with `4,small` are the plain run and, for a strict
  program, the `strict-cpu` run.
- **`nix run .#bench-gpu`** times its CPU rows on the program's own
  threads; `MIDNIGHT_THREADS=1` gives one thread's.
