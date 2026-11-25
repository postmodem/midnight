# 0003 — Moving less: rows in place of columns, one submission in place of many

- **Status:** implemented
- **Depends on:** 0002
- **Tested on:** x86_64-linux, 2026-11-25

## Goal

Ordinary code that touches data a GPU system keeps in GPU memory pays for
whole columns today, and for one blocking round trip per column per table:

```ruby
gpu system move
  each Position, Velocity do |pos, vel|
    pos.x += vel.dx * Time.dt
    pos.y += vel.dy * Time.dt
  end
end

system hatch
  if Time.tick % 10 == 0
    spawn Position(0.0, 0.0), Velocity(rand, rand)
  end
end
```

Among a million entities that one `spawn` brings 8 MB back from the GPU and
sends 16 MB up again. After this spec it sends the new row. The same holds
for the other small things ordinary code does to GPU data, and the GPU
blocks of a tick are handed over together.

Nothing a program computes or prints changes. What changes is what moves
and how often the program waits for the GPU. On an RTX 5090, ms per tick,
with the estimate the planning probe gives:

| Program (`bench/`)                                         | 0.2.7  | Estimate |
| ---------------------------------------------------------- | ------ | -------- |
| `gpu_spawn`: one spawn a tick into 1,000,000 rows          | 1.349  | 0.03     |
| `gpu_despawn`: one despawn a tick                          | 1.351  | 0.035    |
| `gpu_tag`: one `add` of a tag a tick                       | 1.383  | 0.06     |
| `gpu_poke`: one `e[C]` read and one write a tick           | 0.858  | 0.05     |
| `gpu_overwrite`: both sides assign a component whole       | 0.880  | 0.08     |
| `gpu_rare_write`: a loop that assigns only under a condition | 1.182 | 0.75    |
| `gpu_read_tables`: a reader of 2 components over 64 tables | 2.064  | 0.13     |
| `gpu_blocks8`: eight blocks a tick                         | 0.135  | 0.04     |

## Scope

- **Structural changes patch the GPU copy.** A `spawn` sends the new row. A
  `despawn` moves one row where its data is. An `add` or a `remove` brings
  back the row it moves and sends it to its new table.
- **`e[C]` moves one row:** a read brings one row back, a write sends one
  row with the next block.
- **Nothing moves for what is overwritten:** no download before an ordinary
  loop, and no upload before a GPU block, that assigns a whole component for
  every entity before reading it.
- **A loop that can assign uploads only after it did.** A query `each` that
  assigns through a parameter under a condition leaves the GPU copy alone
  in a tick where no assignment ran.
- **One round trip per loop.** An ordinary `each` brings back every column
  it needs, in every table, behind one wait.
- **One submission for consecutive GPU blocks,** sharing one compute pass
  where they can.
- A rule for when a row travels alone and when the whole column is cheaper.
- `midnight plan` says which lines move rows; `MIDNIGHT_GPU=stats` counts
  rows, submissions and waits; `MIDNIGHT_GPU=rows` for the tests.
- Four benchmarks for costs that have none: `gpu_despawn`, `gpu_tag`,
  `gpu_rare_write`, `gpu_overwrite`.
- The reference becomes Midnight 0.3 and the compiler 0.3.0.

## Non-goals

- Tracking which copy is current per field (a reader of one field still
  moves the component), and uploading only the changed rows of a column
  that a query `each` wrote.
- Moving a row from one table's GPU copy to another's in GPU memory. A row
  that `add` or `remove` moves travels through CPU memory.
- Reporting a GPU block's run-time error later than when its `each` ends.
  A block that can fail still ends with a wait.
- Declared entity counts. The rule for rows against columns uses the sizes
  the tables have when the program runs.
- Folds on the GPU, grids for the nested `each`, structural changes inside
  GPU blocks, `wgpu-native` 29, running ordinary systems while the GPU
  works on something they do not touch.
- Comparing row numbers for `next if other == me`, hoisting the `rand` key
  hashes, and the struct `var`s of the unrolled nested loop.

## Design

### The language

No syntax changes and no program means anything new. The reference changes
in "The transfer plan" and in the two cost sections.

**What needs what.** The reference's rule stays: a GPU block needs the GPU
copy of what it uses, ordinary code the CPU copy. What "the copy" is gets
finer:

| Ordinary code                                       | Needs in CPU memory        | Moves, when only the GPU copy is current |
| --------------------------------------------------- | -------------------------- | ---------------------------------------- |
| A query term whose parameter is read                | the array, in each table   | a download of the array                  |
| A query term whose parameter is assigned            | the array                  | the download; the array is uploaded later only if an assignment ran |
| A query term whose parameter is overwritten         | nothing                    | nothing                                  |
| `e[C]`, read                                        | the entity's row           | one row down                             |
| `e[C].f = v`, `e[C].f += v`                         | the entity's row           | one row down, and one row up for the next block |
| `e[C] = v`, and `e.add(C(v))` when `e` has a `C`    | nothing                    | one row up for the next block            |
| `spawn`                                             | nothing                    | the new row up                           |
| `despawn`                                           | nothing                    | at most one row up per component         |
| `add`, `remove` that move the entity to another table | the entity's row         | one row down, the row up in its new table, at most one more row up, per component |

A row up is also what an `e[C]` write, a `spawn` and the others cost when
both copies are current. When rows would cost more than the array, the
array moves instead; "The runtime" has the rule.

**Overwritten.** A parameter of an `each` is overwritten when the block
begins by assigning it whole, or by assigning every one of its fields,
before anything can read it:

```ruby
each Force do |force|
  force = Force(0.0, 0.0)
end

each Position, Screen do |pos, screen|
  scale = Config.scale
  screen.x = pos.x * scale
  screen.y = pos.y * scale
end
```

Exactly:

1. The body starts with a run of plain statements (declarations of locals,
   assignments, calls) that are not inside a branch or a loop. Among them
   the parameter is assigned as a whole, or each field of the component is
   assigned as a whole. `p.v.x = 1` assigns part of the field `v` and does
   not count.
2. No expression in that run, up to the last of those assignments, reads
   the parameter. A compound assignment reads.
3. In an ordinary `each`, nothing in the whole body can reach the component
   another way: no `e[C]` with that component, no query with it as a term
   whose parameter is used, and no call of a function that contains either,
   directly or through other functions. The body has no `break` that leaves
   this `each` and no `return`: the entities not visited would keep values
   that were never brought back.
4. In a GPU block, the nested `each` does not use the component.

The compiler decides this, and the reference describes it as a cost rule:
a program is free not to know it.

**The plan.** A line is still one direction, one component and two systems,
and stands for one transfer per table that both systems reach. Lines now
come in two sizes:

- **A line with no mark** copies the whole array, in every tick.
- **A line marked `rows`** moves only the rows concerned: the row an `e[C]`
  names, the rows a structural change adds or moves. It moves the whole
  array when that is cheaper, and nothing in a tick where no row changed.
- **`rows on spawn`, `rows on add`, `rows on remove`, `rows on despawn`**
  are `rows` lines that happen only in a tick where that operation ran.
  Every line a structural change causes is a `rows` line.

The reference's example becomes:

```
upload    Velocity  reap -> move     rows on despawn
download  Position  move -> report
```

`reap` despawns. `Position` is current only on the GPU, so the row that
takes the despawned entity's place is moved in GPU memory and nothing
travels. `Velocity` is current on both sides: the row is moved in CPU
memory and that one row is sent with the next `move`. `report` downloads
`Position` in every tick, as before.

The plan drops the upload of a component a GPU block overwrites and the
download of one an ordinary loop overwrites. Its limits change in two
places:

- An `if` now makes every kind of transfer rare when the program runs: an
  `e[C]` or a structural change that does not run moves nothing, and a
  query `each` causes an upload only after a tick in which an assignment
  through the parameter ran. The plan still takes every statement to run.
- A marked download no longer takes the place of an unmarked one later in
  the tick: bringing back rows does not make the whole array current.

**Costs.** "What things cost" and "What a GPU system costs" get the new
table rows and figures: a row moved; a round trip (a wait for the GPU) as
the fixed cost of anything brought back; one submission for consecutive
blocks. The advice changes with them. "Make structural changes sparingly
where GPU systems work" goes: a change costs a row. "Read the plan" says
that an unmarked line is an array in every tick and a `rows` download is a
wait. New: a query term reads the whole array, so `e[C]` is the cheaper way
to look at a few entities of a large table, and the query term the cheaper
way to look at many.

### The compiler

**The typed program** (`src/tir.rs`). `Term::Component` gains
`overwritten`, set by the checker by the rule above. The emitters and the
plan read it. A whole `e[C] = value` is told from a partial write by the
shape of the statement; no field is added for that.

**An ordinary `each`.** For the terms whose component some GPU block names:

- Before the table loop, one call hands the runtime the query and, for each
  such term whose parameter is used, the component and its use: read,
  assigned or overwritten. The per-table `mn_sync` calls go.
- For an assigned term the loop sets a flag where it stores through the
  parameter. After the rows of a table, and wherever the loop is left
  early, a table whose flag is set is reported as written, and the flag is
  cleared. An overwritten term needs no flag.
- Terms of components no GPU block names are emitted as today, so a program
  with no GPU system does not change.

**`e[C]`** on a component some GPU block names passes one of three uses:
read, part written, whole written.

**A GPU block.** `MnGpuColumn` gains `overwrite` for an outer column whose
parameter is overwritten. After the last block of a GPU system that an
ordinary system follows in the tick, generated code tells the runtime to
submit.

**`midnight plan`** follows, per column and per kind of table as today,
which copy is current, and also the system that last left rows to send:

- A GPU block that uses a column: an unmarked upload when only the CPU copy
  is current (none if it overwrites the column), and a `rows` upload, named
  after the system that left them, when rows are waiting.
- An ordinary query term: an unmarked download when only the GPU copy is
  current (none if overwritten). If the body can assign, the CPU copy is
  then the only current one, as today.
- `e[C]`: a `rows` download for a read or a partial write when only the GPU
  copy is current. A write leaves rows to send unless only the CPU copy is
  current.
- `spawn`, in the tables it spawns into: rows to send, unless only the CPU
  copy is current. `despawn`, in every table: rows to send where both
  copies are current. `add` and `remove`, in every table: a `rows` download
  where only the GPU copy is current, and rows to send unless only the CPU
  copy is current.

The two ticks (none of the structural changes, all of them) stay.

### The runtime

**What the GPU copy of a column knows.** As today: which copy is current,
the CPU's, the GPU's or both. New, and meaningful only while the GPU copy
is current:

- **The tail.** The GPU copy holds a number of rows; rows the table has
  beyond that exist only in CPU memory. A `spawn`, and a row arriving from
  another table, lengthen the tail and do nothing else.
- **Rows to send.** Rows the GPU copy holds whose value in CPU memory is
  newer.
- **Rows brought back.** While only the GPU copy is current: rows whose
  value has also been copied to CPU memory since a block last wrote the
  column.

The CPU *has* a row when the CPU copy is current, or the row is in the
tail, to send, or brought back. Lookups in the two lists happen on every
`e[C]`, so they must not grow with the lists.

**What each event does** to a column whose GPU copy is current (when only
the CPU copy is current, ordinary code does nothing, as today):

| Event | Effect |
| ----- | ------ |
| A block uses the column | The tail and the rows to send are written to the GPU copy; rows next to each other go in one write. The buffer is made larger first if the table outgrew it: the rows it held are copied to the new buffer in GPU memory. If only the CPU copy was current the column is uploaded whole, as today. If the block writes the column, only the GPU copy is current afterwards and no row counts as brought back. |
| A block overwrites the column | Nothing is uploaded, sent or copied; the buffer is made large enough. Only the GPU copy is current afterwards. |
| An ordinary `each` reads or assigns | If only the GPU copy is current, the rows it holds are downloaded, except the rows to send, which keep their CPU values. Both copies are current; the tail and the rows to send stay. After the rows of a table in which an assignment ran, only the CPU copy is current. |
| An ordinary `each` overwrites | No download. Only the CPU copy is current, before the first row. |
| `e[C]`, read | If the CPU lacks the row, it is brought back: one row, one round trip. |
| `e[C]`, part written | The same, and then the row is to send. |
| `e[C]`, whole written | The row is to send. Nothing is brought back. |
| A row leaves a table (`despawn`, `add`, `remove`) | The table's last row takes its place. If the CPU has the last row, it is moved in CPU memory and the place becomes a row to send (or stays part of the tail). If not, it is moved in GPU memory: two copies through a small scratch buffer, since a copy inside one buffer is not allowed. Such a column is one a block writes, so its rows are whole words. Unless the last row was in the tail, the GPU copy holds one row fewer afterwards. |
| `add` or `remove` moves a row to another table | The CPU must have the row in every column both tables hold; what it lacks is brought back first. The row then joins the tail of its new table and leaves the old one as above. |

The one call of an ordinary loop brings back every column the loop needs,
in every table of its query that has rows, in one round trip.

The handles of a table's entities are a column like the others, always
current in CPU memory.

A row to send in a column whose rows are not whole words is written as the
words that cover it, from CPU memory: such a column is never current on the
GPU alone.

**Rows for a queue of changes come back together.** When the outermost
`each` ends and its queued changes are applied, the runtime first walks
the queue, finds the rows that the `add`s and `remove`s will move and the
CPU lacks, and brings them all back in one round trip. A row brought back
stays known as it is moved about, so applying the queue needs no further
round trip. A change applied at once, outside any `each`, brings back its
own row.

**Rows or the column: the charge.** A row handled alone costs about as much
as 25,000 bytes of a column moved whole, and a round trip about as much as
400,000 (see "Measured in planning"). So every GPU copy of a column has a
charge, in bytes:

- 25,000 for each row marked to send, each row brought back, and each of
  the two copies of a move in GPU memory;
- 400,000 for each round trip that brings rows of the column back.

When the next step would take the charge above the size of the column
(rows times bytes per row), the column is handled whole instead:

- in place of bringing a row back, the column is downloaded;
- in place of marking a row to send, the CPU copy becomes the only current
  one (after a download, if only the GPU copy was current);
- in place of a move in GPU memory, the same.

The charge returns to 0 when a block uses the column and when the column
travels whole. The tail is not charged: it is one write however long it is.
So a column under 25,000 bytes always travels whole, as today; a row is
brought back alone only from a column of at least 425,000 bytes; and
between two blocks that use a column its rows cost at most about twice
what the column would have. The two numbers are fixed, not measured when
the program runs: the same program moves the same things on every machine.

**One submission, one pass.** The runtime keeps one command encoder open,
and one compute pass in it. A block's dispatches are recorded into the
pass. A command that cannot be in a pass (a snapshot's copy, a row's move,
a download's copy, a buffer's growth) ends the pass, and the next block
begins another. The encoder is submitted:

1. when a GPU system ends and an ordinary system follows it in the tick,
   without waiting, so the GPU works while the ordinary system runs;
2. when a block that can fail ends: its error word is copied in the same
   submission, the program waits and reads it, and a failure is reported
   there, as today;
3. when ordinary code needs something brought back: the copies are recorded
   in the same encoder, and the program waits once;
4. before a queue write to a buffer that a recorded command uses, without
   waiting (see below);
5. when the tick ends, with the wait the reference promises;
6. when the program ends.

A GPU failure (`midnight: GPU error`, status 70) is reported when the work
is submitted, which can be later in the tick than today.

**Queue writes come first.** `wgpuQueueWriteBuffer` takes effect at the
start of the next submission, before every command recorded so far,
whenever it was called. An upload, a row sent or a block's uniform words
written to a buffer that the open encoder already uses would therefore
overtake the commands recorded before it. Each buffer remembers whether
the open encoder uses it, and a queue write to such a buffer submits the
encoder first.

**Waits.** Every wait is a callback that sets a flag
(`wgpuQueueOnSubmittedWorkDone`, the map callback) and
`device_poll_until`, as today. The return value of a non-blocking
`wgpuDevicePoll` is not used.

**Buffers.** A buffer replaced while the encoder is open is released, not
destroyed: destroying a buffer that an unsubmitted command names aborts in
`wgpuQueueSubmit`. One readback buffer serves a round trip: every column
and row wanted is copied to its own place in it, at offsets that are
multiples of 8, and it is mapped once. A round trip larger than the
largest buffer is made in several. A column's version changes whenever its
GPU bytes do, a row sent or moved included, so a nested `each`'s copy is
made again.

**A block that does not fit the device** brings back what it uses through
the same call an ordinary loop makes: one round trip, and nothing for a
column it overwrites.

**`MIDNIGHT_GPU`.**

- `stats` prints, for each tick:

  ```
  midnight: gpu: tick 7: 0 uploads, 1 download, 2 rows up, 1 row down, 2 submissions, 1 wait
  ```

  Uploads and downloads are whole columns of one table, as today. Rows up
  are rows written to a GPU copy otherwise (tail rows and rows to send);
  rows down are rows brought back alone. As today, handles, uniform words,
  error words and snapshots are not counted in these four. Submissions and
  waits count everything.
- `rows` is new: the charge is not applied, so rows always travel alone.
  It is for tests: the small tables of the test programs would otherwise
  always travel whole.

**The contracts.**

- `runtime/midnight_rt.h`: `mn_sync` gives way to the one call per loop and
  the report of a written table; `mn_get_synced` takes one of three uses;
  `MnGpuColumn` gains `overwrite`; a call that submits; and beside
  `mn_gpu_active` a second flag, true exactly while the GPU copy of some
  column is the only current one, which is all that a read has to test
  before it calls the runtime.
- `runtime/midnight_gpu.h`: `mn_gpu_reclaim` goes. In its place
  `midnight_rt.c` tells the device code that a row is about to leave a
  table, and which rows a queue of changes will move.
  `mn_gpu_device_run` records and submits only for a block that can fail.
  A submit without a wait is added, and the counters for rows, submissions
  and waits.
- Each change goes in with both of its sides in one commit.

### Tests

- **Existing output does not change.** No `.out` or `.err` file changes.
  `gpu_churn`, `gpu_stress` and the rest stay byte-identical on the CPU
  implementation, `llvmpipe` and the hardware device.
- **`rows`.** `gpu_programs` runs every program a second time with
  `MIDNIGHT_GPU=rows`, in both build modes. That is what takes the churn
  and stress programs through the tail, the lists, the moves in GPU memory
  and the queue's round trip.
- **Exact counts.** A program of `tests/run` may have a `NAME.stats` file.
  In GPU mode a new test, `gpu_stats_match_expected`, runs it with `stats`
  (without `rows`) and requires exactly those lines, on both devices and
  in both build modes. New programs, with tables large enough for rows to
  travel alone (200,000 rows of 8 bytes) and small enough for `llvmpipe`:
  - a spawn, a despawn, an `add` and a `remove` per tick, in an `each` and
    at the top level;
  - several `add`s queued in one `each`: one wait for all their rows;
  - `e[C]` read, part written, whole written; the same row read twice (one
    round trip); a fourth row read in one tick (the column comes whole);
  - a table that outgrows its buffer while only the GPU copy is current;
  - a despawn of the last row, of a row in the tail, and of a row to send;
  - an overwritten parameter on each side, and the nearest cases that are
    not overwritten: a field left out, a read first, a `break`, an `e[C]`
    in a called function, a nested `each` that reads the component;
  - a loop whose assignment runs in some ticks only;
  - a GPU system of several blocks before an ordinary system, one of the
    blocks with a nested `each` over several tables, one that can fail.
- **`gpu_stats_match_plan`** keeps its meaning: the unmarked lines of the
  plan against the uploads and downloads counted. Its program must assign
  in every tick wherever it can assign (`wrap` in `gpu_transfers.12am`
  assigns under a condition today), and the test also requires 0 rows each
  way, since the plan has no `rows` line.
- **Plan tests.** New expected files, each reviewed against its
  `# expect:` comment, which is rewritten:

  | `tests/plan/` | Plan |
  | ------------- | ---- |
  | `spawn`       | `upload Position hatch -> move rows on spawn`, `upload Velocity hatch -> move rows on spawn` |
  | `despawn`     | `upload Velocity reap -> move rows on despawn`, `download Position move -> report` |
  | `add_remove`  | `upload Heat thaw -> cool rows on remove`, `upload Rate thaw -> cool rows on remove`, `download Heat cool -> freeze rows on add`, `download Heat cool -> thaw rows on remove` |
  | `top_level`   | `upload Velocity top level -> move rows`, `download Position move -> top level rows` |

  New plan programs for an overwritten parameter on each side and for each
  kind of `e[C]`.
- **Errors keep their place.** A new `tests/panic` program: a GPU system
  whose second block fails, with a block after it and an ordinary system
  that prints. Nothing after the failing block runs, on both devices.
- **The order of queue writes.** A new `tests/run` program in which a
  column is used by a block, written by `e[C]` and by a whole upload, and
  used by a second block before anything is submitted in between.
- `tests/COVERAGE.md` lists the new cost rules and the tests for each.

### Measured in planning

On an RTX 5090 (driver 595.71.05) beside a Ryzen 9 9950X3D, `wgpu-native`
27.0.4.0, with a hand-written probe and nothing else running. Times are ms,
the fastest mean of 5.

**How `wgpu-native` behaves.**

- A queue write made after a copy was recorded takes effect before that
  copy when the encoder is submitted, on the 5090 and on `llvmpipe`. wgpu
  documents it: writes "begin GPU execution only on the next call to
  `Queue::submit()`, just before the explicitly submitted commands"
  (https://docs.rs/wgpu/latest/wgpu/struct.Queue.html).
- A copy inside one buffer is refused ("Source and destination cannot be
  the same buffer"), as MDN describes for WebGPU
  (https://developer.mozilla.org/en-US/docs/Web/API/GPUCommandEncoder/copyBufferToBuffer),
  and the refusal aborts the process in `wgpuQueueSubmit`.
- Releasing a buffer that an unsubmitted encoder names is fine. Destroying
  it aborts in `wgpuQueueSubmit`.
- A copy into the readback buffer can be recorded while the buffer is
  mapped, if it is unmapped before the submit.
- Three blocks that each read what the one before wrote give the same
  bytes in three submissions, in three passes of one encoder, and in one
  pass, on both devices, and the CPU's result. WebGPU checks buffer use
  per dispatch in a compute pass, not per pass.

**A column of 1,000,000 rows of 8 bytes.** Whole: download 0.376, upload
0.459, copy to a larger buffer in GPU memory 0.433. An upload of 4 KB
takes 0.012 and of 256 KB 0.028. K rows, with one submission and one wait:

| K      | Brought back | Written, a queue write each | Moved in GPU memory |
| ------ | ------------ | --------------------------- | ------------------- |
| 1      | 0.012        | 0.012                       | 0.014               |
| 16     | 0.022        | 0.029                       | 0.039               |
| 256    | 0.209        | 0.321                       | 0.540               |
| 4,096  | 3.80         | 5.99                        | 9.59                |
| 65,536 | 59.9         | 93.1                        | 152.0               |

A copy command costs about 0.9 µs and a small queue write about 1.4 µs:
as much as 20 to 25 KB of a column moved whole. The whole 8 MB column is
cheaper than about 400 rows. Hence the 25,000.

**Ticks on that table,** one block:

| Tick                                           | ms     |
| ---------------------------------------------- | ------ |
| The block and the wait                         | 0.0273 |
| The same, then one row brought back            | 0.0461 |
| One row written to each of two columns first   | 0.0315 |
| One row moved in each of two columns first     | 0.0341 |
| One row brought back, one written              | 0.0480 |

A round trip of its own costs 0.019, as much as about 400,000 bytes of
download. Hence the 400,000.

**Blocks of 1,000 rows per tick,** one wait at the end:

| Blocks | A submission each (today) | One encoder, a pass each | One encoder, one pass |
| ------ | ------------------------- | ------------------------ | --------------------- |
| 1      | 0.0234                    | 0.0205                   | 0.0235                |
| 2      | 0.0392                    | 0.0312                   | 0.0257                |
| 4      | 0.0675                    | 0.0449                   | 0.0280                |
| 8      | 0.1401                    | 0.0756                   | 0.0349                |

A further block costs 0.0167 today, 0.0079 with a pass of its own and
0.0016 in a shared pass. A submission without a wait between two blocks
costs about 0.012.

**Reading back 128 columns of 400 bytes after a block:** 1.880 with a
round trip each, 0.125 with one submission and one wait.

**The four new benchmarks on 0.2.7** are in the table under "Goal". Today
`stats` counts 2 uploads and 1 download a tick for `gpu_despawn`, 4 and 2
for `gpu_tag`, 1 and 1 for `gpu_rare_write` and for `gpu_overwrite`.

### Slices

Each is one or more commits with both sides of every contract it changes,
its tests first, and the suite green on the CPU implementation, `llvmpipe`
and the hardware device.

1. **Counting.** `stats` prints rows (0 for now), submissions and waits;
   `NAME.stats` files and `gpu_stats_match_expected`; the four benchmarks
   in `bench/` and `bench-gpu`. Expected files record today's behaviour and
   change in the diffs of the slices that follow.
2. **One submission, one pass.** The open encoder and pass, the six points
   of submission, the rule for queue writes, the submit call after a GPU
   system.
3. **One round trip per loop.** The call before the table loop, the flag
   and the report of a written table, the second flag for reads, a block
   that does not fit.
4. **Overwritten.** The checker's rule, `overwritten` in the typed
   program, both emitters, the runtime, the plan.
5. **`e[C]` moves a row.** Rows to send, rows brought back, the charge,
   `rows`, the three uses, the plan's `rows` lines.
6. **Structural changes.** The tail, a row leaving, buffers that grow in
   GPU memory, the queue's round trip, `mn_gpu_reclaim` removed, the
   plan's `rows on` lines.
7. **The documents.** The reference at 0.3, README, `tests/COVERAGE.md`,
   `CLAUDE.md`, the benchmark figures, version 0.3.0, the Outcome.

## Acceptance criteria

All run on x86_64-linux. Criteria 3, 4 and 9 need a hardware GPU.

1. `nix flake check` passes, and `nix run .#gpu-test` passes on the
   hardware device.
2. No `.out` or `.err` file of `tests/` or `examples/` differs from 0.2.7.
3. `MIDNIGHT_GPU=stats` counts, in every tick but the first three and the
   last, with `-` for a count the criterion leaves open:

   | Program           | Uploads | Downloads | Rows up | Rows down | Submissions | Waits |
   | ----------------- | ------- | --------- | ------- | --------- | ----------- | ----- |
   | `gpu_move`        | 0       | 0         | 0       | 0         | 1           | 1     |
   | `gpu_move_read`   | 0       | 1         | 0       | 0         | 2           | 1     |
   | `gpu_blocks8`     | 0       | 0         | 0       | 0         | 1           | 1     |
   | `gpu_fallible`    | 0       | 0         | 0       | 0         | 1           | 1     |
   | `gpu_read_tables` | 0       | 128       | 0       | 0         | 2           | 1     |
   | `gpu_poke`        | 0       | 0         | 1       | 1         | 2           | 2     |
   | `gpu_spawn`       | 0       | 0         | 2       | 0         | 1           | 1     |
   | `gpu_despawn`     | 0       | 0         | 1       | 0         | 1           | 1     |
   | `gpu_tag`         | 0       | 0         | 3       | 1         | -           | 2     |
   | `gpu_rare_write`  | 0       | 1         | 0       | 0         | 2           | 1     |
   | `gpu_overwrite`   | 0       | 0         | 0       | 0         | 1           | 1     |

4. `nix run .#bench-gpu` on a quiet machine, fastest of 5, on the RTX 5090:
   `gpu_spawn`, `gpu_despawn` and `gpu_poke` under 0.1 ms per tick,
   `gpu_tag` under 0.15, `gpu_overwrite` under 0.2, `gpu_read_tables`
   under 0.3, `gpu_blocks8` under 0.06, `gpu_rare_write` under 0.95. No
   other row of its output is more than 10 % slower than on 0.2.7, the CPU
   rows included. The Outcome records every figure beside its 0.2.7 one.
5. `nix run .#bench` gives the ratio to `baseline.c` it gives on 0.2.7,
   within its noise.
6. `midnight plan` prints the plans listed under "Tests" for the four
   changed programs, and the reference's example prints the two lines
   shown under "The language".
7. Every program of `gpu_programs` passes with `MIDNIGHT_GPU=rows` on
   `llvmpipe` and on the hardware device, in both build modes.
8. `gpu_stats_match_expected` passes on both devices in both build modes,
   with a program for each case listed under "Tests".
9. A fourth `e[C]` read of one 1,600,000-byte column in a tick downloads
   the column, and the three before it bring back a row each; with `rows`
   all four bring back a row.
10. `ldd` on the release build of `examples/hello.12am` lists what it
    lists on 0.2.7.
11. The reference says 0.3 and `midnight --version` 0.3.0, and every
    command in the README works as written.

## Outcome

Implemented as specified, in the seven slices, with the differences listed
under "Differences from the design". All eleven criteria pass on
x86_64-linux: an RTX 5090 (driver 595.71.05) beside a Ryzen 9 9950X3D,
`wgpu-native` 27.0.4.0, clang 21.1.8, and Mesa's `llvmpipe` for software
Vulkan.

### Acceptance criteria

| #  | Result |
| -- | ------ |
| 1  | `nix flake check` passes; `checks.gpu` runs `gpu_programs` (74 programs, release and debug, each as it is and with `rows`), `gpu_stats_match_plan`, `gpu_stats_match_expected` and `gpu_driver_finds_its_parts` on `llvmpipe` in the sandbox. `nix run .#gpu-test` passes the same four on the RTX 5090. |
| 2  | Against `786e399` (0.2.7), no `.out` or `.err` file of `tests/` or `examples/` differs; eleven new ones were added. `gpu_transfers.12am` now assigns in every tick (`pos.x -= pos.x > 20 ? 20 : 0`) and prints what it printed. |
| 3  | Every count of the table, in every tick but the first three and the last of each program. `nix run .#bench-gpu` now checks this itself and fails otherwise. `gpu_tag` makes 2 submissions a tick. |
| 4  | See the table below. Every target is met, and no other row is more than 10 % slower than 0.2.7; the largest drift is the move on the CPU, 0.097 to 0.103. |
| 5  | `nix run .#bench`: ratio 1.00 (0.2.7 in the same session: 1.03). |
| 6  | `tests/plan/spawn`, `despawn`, `add_remove` and `top_level` print exactly the plans under "Tests". The reference's example prints the two lines under "The language"; the new test `the_reference_shows_the_plan_of_its_example` checks it. |
| 7  | `gpu_programs` runs every program a second time with `rows`, on both devices, in both build modes, and all pass. |
| 8  | `gpu_stats_match_expected` passes on both devices in both build modes, with ten programs (below). |
| 9  | `gpu_rows_poke`, tick 5: 3 rows down and 1 download without `rows`, 4 rows down with it; both are expected files (`gpu_rows_poke.stats`, `gpu_rows_poke.rows.stats`). |
| 10 | `ldd` on the release build of `examples/hello.12am` lists `libm.so.6`, `libc.so.6` and the loader, as on 0.2.7. |
| 11 | The reference says 0.3 and `midnight --version` prints `midnight 0.3.0`. Every command in the README was run as written, `./move` built into a scratch directory. |

### Measured

`RUNS=5 nix run .#bench-gpu`, fastest of 5, ms per tick, with nothing else
running. The 0.2.7 column is the same script and the same programs, built
by 0.2.7 from a worktree, run in the same session just before.

| Row | 0.2.7 GPU | 0.3.0 GPU | 0.2.7 CPU | 0.3.0 CPU |
| --- | --------- | --------- | --------- | --------- |
| Gravity, 16,384 bodies | 0.836 | 0.840 | 354.9 | 354.2 |
| Gravity, 65,536 bodies | 3.643 | 3.652 | | |
| Move 1,000,000, data stays (`gpu_move`) | 0.039 | 0.028 | 0.097 | 0.103 |
| Move 1,000,000, read each tick (`gpu_move_read`) | 0.803 | 0.823 | 0.505 | 0.513 |
| `gpu_block` | 0.0228 | 0.0236 | 0.0 | 0.0 |
| `gpu_blocks8` | 0.1394 | 0.0465 | 0.0003 | 0.0003 |
| `gpu_fallible` | 0.0299 | 0.0293 | 0.0011 | 0.0011 |
| `gpu_read_tables` | 1.9812 | 0.1469 | 0.0038 | 0.0039 |
| `gpu_poke` | 0.8703 | 0.0460 | 0.0999 | 0.0971 |
| `gpu_spawn` | 1.3210 | 0.0311 | 0.0966 | 0.0997 |
| `gpu_despawn` | 1.3385 | 0.0343 | 0.1021 | 0.1021 |
| `gpu_tag` | 1.3407 | 0.0570 | 0.1010 | 0.0978 |
| `gpu_rare_write` | 1.1686 | 0.5855 | 0.4530 | 0.2866 |
| `gpu_overwrite` | 0.8743 | 0.0731 | 0.1666 | 0.1667 |

`nix run .#bench` in the same session: `bench.12am` 0.284 against
`baseline.c` 0.285 ms per tick (0.2.7: 0.286 against 0.278); churn, in
operations per second, spawn 83.1 million (87.6), lookup 281 million (176),
add 36.8 million (23.3), remove 58.7 million (56.7), despawn 125 million
(130).

Where the planning estimates were off: `gpu_rare_write` comes to 0.59, not
0.75 (its CPU run also got faster, 0.45 to 0.29: the loop with the flag
compiles to a faster loop), and `gpu_blocks8` to 0.047, not 0.04. The rest
are within the estimates' precision.

### The `.stats` programs

`gpu_rows_spawn`, `gpu_rows_despawn`, `gpu_rows_add_remove`,
`gpu_rows_poke`, `gpu_rows_grow`, `gpu_overwrite_cpu`, `gpu_overwrite_gpu`,
`gpu_rare_assign`, `gpu_batch` and `gpu_queue_order`, each with a `.stats`
file that was recorded in slice 1 on 0.2.7's behaviour and changed in the
diff of each slice that followed. Each line of the final files was checked
by hand against the design. Two things they show that the design did not
spell out:

- A small table travels whole. In `gpu_rows_add_remove` the tagged table
  holds a few rows, so its columns always move whole and its rows count
  nothing; `gpu_rows_add_remove.rows.stats` shows the same program with
  every row alone.
- A buffer that grows makes the next write to it a submission of its own:
  the copy of the old rows into the new buffer is recorded, and the tail
  written after it is a queue write to a buffer the open encoder names
  (`gpu_rows_grow`, tick 1, 3 submissions).

### Differences from the design

- **Names in the contracts.** `midnight_rt.h`: an ordinary `each` calls
  `mn_gpu_use(query, uses, count)` with `MnUse { component, use }` and
  `MN_USE_READ`, `MN_USE_ASSIGN`, `MN_USE_OVERWRITE`, guarded by
  `mn_gpu_only` (the second flag) or, when a use overwrites, by
  `mn_gpu_active`; it reports a written table with `mn_wrote`;
  `mn_get_synced` takes `MN_ROW_READ`, `MN_ROW_PART` or `MN_ROW_WHOLE` and
  calls `mn_gpu_row`; `mn_gpu_submit` hands the work over after a GPU
  system. `mn_sync` and `mn_gpu_sync` are gone. `midnight_gpu.h`:
  `mn_gpu_leave` (a row is about to leave a table), `mn_gpu_want_row` and
  `mn_gpu_fetch` (the rows a move needs, one round trip for a queue),
  `mn_gpu_device_submit`, `mn_gpu_rows` and the counters.
- **The queue's round trip does not look up destination tables.** Finding
  them could create a table that applying the queue would not (an entity
  despawned earlier in the queue), and table ids decide query order. An
  `add` is taken to move the row when the entity's table lacks one of the
  components, a `remove` when it has one; the columns of the components a
  `remove` drops are not brought back.
- **The checker's pass** is a module of its own, `src/check/overwrite.rs`,
  run after the GPU checks; it matches its results to `each` statements by
  their source span. In the plan, a step's first use of a component decides
  whether it overwrites, an `e[C]` included: an `e[C]` read before an
  overwriting loop in the same system keeps the download.
- **`NAME.rows.stats`.** `gpu_stats_match_expected` also runs a program with
  `stats,rows` when it has such a file. Criterion 9's half with `rows` is
  checked this way, and the tagged table of `gpu_rows_add_remove`.
- **Expected `.stats` files are written by `UPDATE_EXPECT=1` in GPU mode**,
  the only expected files GPU mode writes. `CLAUDE.md` says so.
- **Plan programs.** Besides the four changed ones: `plan/overwrite_cpu`,
  `plan/overwrite_gpu` and `plan/entity_component`. The overwritten cases on
  each side are two run programs, `gpu_overwrite_cpu` and
  `gpu_overwrite_gpu`, one component per case, and the checker has unit
  tests for each rule.
- **`bench-gpu` checks criterion 3** and the flake's GPU test list names
  `gpu_stats_match_expected`.
- **Rows marked and the tail exist from slice 5.** The bookkeeping for the
  tail was written with `e[C]`'s rows, so slice 6 only stopped bringing
  columns back and added the moves.
