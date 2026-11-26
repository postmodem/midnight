## Crowd

Two hundred thousand walkers go round a ring road of 400 places, numbered
0 to 399. Place 399 is followed by place 0 again.

The walkers are numbered 0 to 199999. Walker `n` starts with

- place = 100 + (`n` * 37 mod 300)
- speed = 1 + (`n` mod 5)
- stamina = `n` mod 6

where `a mod b` is the remainder of the whole numbers `a` and `b`, from 0
to `b - 1`. Walker 7 starts at place 359 with speed 3 and stamina 1.

Each walker is an entity. The program has two systems, which run once a
tick in this order: `walk` goes over every walker and moves it, in an
`each` written in `walk` itself, not in a function it calls, and `report`
prints the tick's line.

In `walk`, each walker, on its own:

- If its stamina is more than 0, it walks: it moves forward as many
  places as its speed (from place 398 with speed 3 it reaches place 1),
  and its stamina goes down by 1.
- Otherwise it rests: it does not move, and its stamina becomes 6 minus
  its speed.

After it has walked or rested, the walker counts in the quarter of the
road it is in: quarter 1 is places 0 to 99, quarter 2 places 100 to 199,
quarter 3 places 200 to 299 and quarter 4 places 300 to 399.

The work of `walk` is spread over many entities, so it must run on
several cores: `midnight loops solution.12am` must say `cores` on the
line of every `each` in `walk`.

The program runs 12 ticks. `report` prints one line a tick:

`tick T: S steps, R resting, quarters A B C D, W in all`

where `T` is the tick's number, counting from 1; `S` is the number of
places all the walkers moved in this tick; `R` is the number of walkers
that rested in this tick; `A`, `B`, `C` and `D` are the numbers of
walkers in quarters 1, 2, 3 and 4 at the end of the tick; and `W` is the
number of places all the walkers have moved since the start.

The program prints exactly 12 lines and nothing else. The first two are:

```
tick 1: 500000 steps, 33334 resting, quarters 2000 65334 66667 65999, 500000 in all
tick 2: 499996 steps, 33334 resting, quarters 3334 63334 66667 66665, 999996 in all
```
