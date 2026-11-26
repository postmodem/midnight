## Countdown

A rocket counts down to launch. The countdown is a whole number of seconds,
and it starts at 20. The fuel tanks have a pressure, a whole number, which
starts at 30. There is no hold at the start.

The countdown is in one of four stages, named by the number it shows:

| Countdown   | Stage          | Pressure change when the countdown reaches it |
| ----------- | -------------- | --------------------------------------------- |
| 11 or more  | `fueling`      | up by 2                                       |
| 4 to 10     | `pressurizing` | up by 6                                       |
| 1 to 3      | `ignition`     | up by 8                                       |
| 0           | `liftoff`      | none                                          |

The program runs one tick at a time. Each tick has three steps, in this
order:

1. **Count.** If a hold is in progress, the tick is spent holding: the
   hold has one tick fewer left, the countdown does not change, and the
   pressure goes down by 2. Otherwise the countdown goes down by 1, and the
   pressure changes as the table says for the stage of the new countdown.
   When the countdown goes down to 15, a hold of 3 ticks begins with the
   next tick; when it goes down to 6, a hold of 2 ticks begins with the
   next tick.
2. **Abort.** If the pressure is now above 88, the launch aborts: the
   countdown goes back to 10, the pressure is set to 40, and a hold of 4
   ticks begins with the next tick, in place of any hold in progress. If
   this is the second time the launch aborts, the launch is scrubbed
   instead.
3. **Report.** The program prints one line:
   `tick N: T-C S, pressure P`, where `N` is the tick's number, counting
   from 1; `C` is the countdown; `P` is the pressure; and `S` is `abort` if
   the launch aborted in this tick, `hold` if this tick was spent holding,
   and otherwise the name of the stage the countdown is in. In the tick in
   which the launch is scrubbed, the line is `tick N: scrubbed` instead.

The program stops after the tick in which the countdown reaches 0 or the
launch is scrubbed, and prints nothing else. Its first three lines are:

```
tick 1: T-19 fueling, pressure 32
tick 2: T-18 fueling, pressure 34
tick 3: T-17 fueling, pressure 36
```
