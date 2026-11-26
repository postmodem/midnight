## Swarm

Ten thousand particles fall and bounce inside a square box. The box runs
from 0 to 64 across (`x`) and from 0 to 64 up (`y`): `y` is the height,
and 0 is the floor.

The particles are numbered 0 to 9999. Particle `n` starts at

- `x` = (`n` * 37 mod 1024) / 16
- `y` = (`n` * 91 mod 1024) / 16

with a velocity, in units per tick, of

- `vx` = (`n` mod 7 - 3) / 4
- `vy` = (`n` mod 5 - 2) / 4

where `a mod b` is the remainder of the whole numbers `a` and `b`, from 0
to `b - 1`. Particle 9 starts at `x` = 333/16 = 20.8125 and `y` =
819/16 = 51.1875 with `vx` = -1/4 and `vy` = 2/4. Every position and
velocity in this task, at the start and later, is a whole number of
sixteenths.

Each particle is an entity. The particles are moved by a GPU system
named `move`, declared with `gpu system` (not `strict gpu system`), which
runs once a tick. In each tick, every particle, on its own:

1. Falls: `vy` goes down by 1/16.
2. Moves: `x` goes up by `vx`, then `y` goes up by `vy`.
3. Bounces off the side walls: if `x` is now below 0, `x` becomes `-x`
   and `vx` becomes `-vx`. Otherwise, if `x` is now above 64, `x` becomes
   128 - `x` and `vx` becomes `-vx`.
4. Bounces off the floor and the ceiling: if `y` is now below 0, `y`
   becomes `-y` and `vy` becomes `-vy`. Otherwise, if `y` is now above
   64, `y` becomes 128 - `y` and `vy` becomes `-vy`.

Each of steps 3 and 4 that changes a particle counts as one bounce of
that particle, so a particle can bounce twice in one tick. A particle
exactly at 0 or 64 does not bounce.

The box is divided into sixteen regions, four rows of four. A particle's
column is 1 when its `x` is below 16, 2 when `x` is at least 16 and below
32, 3 when `x` is at least 32 and below 48, and 4 when `x` is 48 or more.
Its row is 1 when its `y` is 48 or more, 2 when `y` is at least 32 and
below 48, 3 when `y` is at least 16 and below 32, and 4 when `y` is
below 16. Row 1 is the top of the box.

The program prints a report before the first tick, and then again after
every 10 ticks, until 60 ticks have run: seven reports in all. A report
is five lines:

- `tick T: B bounces`, where `T` is the number of ticks run so far and
  `B` is the number of bounces of all the particles since the start.
- Then one line for each row, from row 1 to row 4: `row R: ` followed by
  the number of particles in the regions of that row, from column 1 to
  column 4, separated by single spaces.

The program prints exactly 35 lines and nothing else. The first five are:

```
tick 0: 0 bounces
row 1: 616 634 625 625
row 2: 625 616 635 625
row 3: 627 625 614 636
row 4: 636 624 623 614
```
