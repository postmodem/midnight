## Paddle

A paddle at the bottom of a window keeps two balls in play. The player
moves it with the left and right arrow keys. Every time a ball reaches
the paddle's line the program prints whether the paddle hit it or missed
it.

The program opens a window titled `Paddle`, 320 points wide and 240
points high, and runs until the window is closed. Positions are in whole
points: `x` from the left edge of the window, `y` down from its top edge.

The paddle and each ball are entities. The program has three systems,
which run once a tick in this order: `steer` moves the paddle, `move`
goes over every ball with an `each` and moves it, and `score` goes over
every ball with an `each` and finds the hits and the misses.

**The paddle** lies along the line `y` = 220. Its left end starts at
`x` = 136 and it is 48 points long: it covers every `x` from its left end
to its left end + 48, both included. In `steer`:

- If the left arrow key is held down when the tick begins and the right
  arrow key is not, the left end moves 6 points to the left.
- If the right arrow key is held down when the tick begins and the left
  arrow key is not, the left end moves 6 points to the right.
- With both keys held, or neither, the paddle does not move.
- The paddle stays inside the window: its left end never goes below 0 or
  above 272. A move that would take it past one of those stops it there.

A key that is pressed and released again between two ticks is not held
down when the next tick begins, so it does not move the paddle.

**The balls.** There are two, ball 2 and ball 1. Ball 2 starts at
`x` = 250, `y` = 106, moving 3 points left and 6 down each tick: its
velocity is `vx` = -3, `vy` = 6. Ball 1 starts at `x` = 40, `y` = 30,
moving 4 points right and 5 down each tick. In `move`, each ball:

1. Moves: `x` goes up by `vx`, and `y` by `vy`.
2. Bounces off the sides: if `x` is now below 0, `x` becomes `-x` and
   `vx` becomes `-vx`. Otherwise, if `x` is now above 320, `x` becomes
   640 - `x` and `vx` becomes `-vx`.
3. Bounces off the top: if `y` is now below 0, `y` becomes `-y` and `vy`
   becomes `-vy`.

In `score`, a ball whose `y` is 220 or more has reached the paddle's
line:

- If the paddle covers the ball's `x`, it is a **hit**: `y` becomes
  440 - `y` and `vy` becomes `-vy`, and the program prints
  `tick T: ball N hit at X`, where `X` is the ball's `x`.
- Otherwise it is a **miss**: the ball starts again where it started at
  the beginning, with the velocity it had then, and the program prints
  `tick T: ball N missed`.

`T` is the tick's number, counting from 1, and `N` is the ball's number.
When both balls reach the line in the same tick, ball 1's line comes
first.

**Closing.** The program ends when its window is closed. The tick that
begins with the window closed still runs, all three systems included.
After it the program prints one last line,
`H hits, M misses, T ticks, paddle at P`, where `H` and `M` count every
hit and miss of the run, `T` is the number of ticks that ran, that last
one included, and `P` is where the paddle's left end is then; and then
the program ends.

**Trying it.** With no display, the window is headless, and a file of
commands takes the place of the player: it presses and releases keys,
takes the focus away from the window and gives it back, lets ticks run
and closes the window. The documentation describes the commands.
`try.drive` in this directory is such a file; run the program with it as

```sh
MIDNIGHT_DRIVE=try.drive midnight run solution.12am
```

Without a file of commands the window is never closed, and the program
runs until it is stopped. The program is checked with a different file
of commands of the same kinds.

With `try.drive`, the program prints 7 lines, and the last is
`2 hits, 4 misses, 143 ticks, paddle at 136`. With `try.drive`, and with
the commands it is checked with, the first line is:

```
tick 19: ball 2 missed
```
