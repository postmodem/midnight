## Fixup

The file `solution.12am` in this directory simulates two ferries that
carry passengers away from a dock. It was written by a Ruby programmer new
to Midnight and does not work. Fix it, so that `midnight run solution.12am`
prints exactly what is described below. Keep its data as it is: the
ferries, spawned in the order the program spawns them, and the list of
arrivals, in its order.

Each passenger and each ferry is an entity. Every passenger has a number
and a patience; every ferry has a number and a number of seats. Ferry 1
has 1 seat and ferry 2 has 2 seats. Passengers arrive at the ticks the
program's list of arrivals gives.

The program runs 8 ticks. Each tick has five steps, in this order. Each
step is a system of its own, named after the step, and the five systems
run once a tick, in the order given: `arrive`, `board`, `sail`, `wait`,
`report`. Each step finishes before the next one begins.

1. **Arrive.** Every passenger whose arrival tick is this tick (counting
   from 1) arrives at the dock, as a new entity.
2. **Board.** The ferries board one after the other, ferry 1 first. Each
   ferry fills its seats with the passengers on the dock who have the
   lowest numbers, one passenger to a seat. A ferry with more seats than
   there are passengers on the dock leaves seats empty. A passenger who
   boards a ferry is no longer on the dock.
3. **Sail.** Both ferries sail. Every passenger aboard a ferry leaves with
   it and is despawned. Each ferry counts the passengers it took this
   tick.
4. **Wait.** Every passenger still on the dock loses 1 patience. A
   passenger whose patience is now 0 gives up, leaves and is despawned.
5. **Report.** The program prints one line:
   `tick T: ferry 1 took A, ferry 2 took B, L left, waiting: LIST`, where
   `T` is the tick's number, counting from 1; `A` and `B` are the numbers
   of passengers ferry 1 and ferry 2 took this tick; `L` is the number of
   passengers who gave up this tick; and `LIST` is the numbers of the
   passengers still on the dock, lowest first, separated by a comma and a
   space, or `none` when there are none. The words are always as shown,
   whatever the numbers.

The program prints exactly 8 lines and nothing else. The first line is:

```
tick 1: ferry 1 took 1, ferry 2 took 2, 1 left, waiting: 5, 6, 7, 8, 9
```
