## Drift

Bubbles rise through water in a window. A new bubble appears at the left
every third tick, drifts to the right and up, and is gone once it has
passed the right edge of the window. The program prints how many bubbles
there are each tick.

The program opens a window titled `Drift`, 288 points wide and 176
points high, with the background colour (8, 30, 52), and runs until the
window is closed. Positions and sizes are in points: `x` from the left
edge of the window, `y` down from its top edge. A colour is given as its
red, green and blue bytes.

Each bubble is an entity. The bubbles are numbered 1, 2, 3, … in the
order they appear. The program has three systems, which run once a tick
in this order: `launch` adds the new bubble, `move` goes over every
bubble with an `each` and moves it, and `report` prints the tick's line.
The program may have other systems, which run after `report`.

**Launch.** In ticks 1, 4, 7, 10, … (every tick whose number, counting
from 1, leaves 1 when divided by 3), bubble `k` appears, where `k` is 1
in tick 1, 2 in tick 4, and so on. With `m` = `k` mod 4, it appears at
`x` = 6, `y` = 150 - 9 × `m`, with:

- a radius of 2 + (`k` mod 3);
- a velocity of `vx` = 8 + 3 × `m` and `vy` = -1 - (`k` mod 2): it moves
  `vx` points right and 1 or 2 points up each tick;
- the colour (100 + 30 × `m`, 210, 255).

So bubble 1 appears at (6, 141) with a radius of 3, moves 11 right and 2
up each tick, and has the colour (130, 210, 255). A bubble that appears
in a tick is moved in that tick too.

**Move.** Every bubble moves: `x` goes up by `vx` and `y` by `vy`. A
bubble whose `x` is then greater than 288 has passed the right edge and
is gone: it is despawned and is not in this tick's frame.

**Report.** The program prints `tick T: N bubbles, G gone`, where `T` is
the tick's number, counting from 1, `N` is the number of bubbles left
after this tick's move, and `G` is the number of bubbles gone since the
start. The words are always as shown, whatever the numbers.

**The picture.** When each tick ends, the window's frame holds:

- **the sand**: a rectangle whose top left corner is at `x` = 0, `y` =
  160, 288 points wide and 16 points high, colour (194, 178, 128);
- for each bubble left after this tick's move, a circle around its `x`
  and `y` with its radius and its colour. A bubble whose circle reaches
  past the edge of the window is in the frame until it is gone.

The frame holds those shapes and nothing else: one for each, none of no
size, none that is fully transparent, and no other shape. The order of
the shapes in a frame is not checked.

**Closing.** The program ends when its window is closed. The tick that
begins with the window closed still runs, all three systems included.
After it the program prints one last line, `closed after T ticks`, where
`T` is the number of ticks that ran, that last one included, and then
the program ends.

**Trying it.** With no display, the window is headless, and a file of
commands lets ticks run, prints what the frame holds, and closes the
window. The documentation describes the commands. `try.drive` in this
directory is such a file; run the program with it as

```sh
MIDNIGHT_DRIVE=try.drive midnight run solution.12am
```

Without a file of commands the window is never closed, and the program
runs until it is stopped. The program is checked with a different file
of commands of the same kinds, and its frames are checked with the
command `draws`. With `try.drive`, and with the commands it is checked
with, the first answer of `draws` begins with these two lines:

```
midnight: drive: draws: 40 ticks, 288x176, 9 draws
midnight: drive:   Window.background = Color(8, 30, 52, 255)
```

With `try.drive`, and with the commands it is checked with, the first
lines the program prints are:

```
tick 1: 1 bubbles, 0 gone
tick 2: 1 bubbles, 0 gone
tick 3: 1 bubbles, 0 gone
tick 4: 2 bubbles, 0 gone
```
