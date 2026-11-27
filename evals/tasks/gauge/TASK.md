## Gauge

A gauge in a window shows a level from 0 to 12 as a bar. The up and down
arrow keys raise and lower the level, and while the space bar is held a
marker points at the end of the bar. The program prints the level each
tick.

The program opens a window titled `Gauge`, 200 points wide and 120
points high, with the background colour (30, 30, 34), and runs until the
window is closed. Positions and sizes are in points: `x` from the left
edge of the window, `y` down from its top edge. A colour is given as its
red, green and blue bytes, and as its alpha byte too where that is not
255.

The program has two systems, which run once a tick in this order:
`adjust` changes the level, and `report` prints the tick's line. The
program may have other systems, which run after `report`.

**Adjust.** The level starts at 5. In each tick:

- If the up arrow key was pressed since the previous tick began (it went
  down at least once, however many times), the level goes up by 1.
- If the down arrow key was pressed since the previous tick began, the
  level goes down by 1. With both pressed, the two cancel and the level
  does not change, at 0 and 12 too.
- The level stays between 0 and 12: a change that would take it below 0
  or above 12 leaves it at 0 or 12.
- A key that stays held does not change the level again in later ticks.

The **marker** is shown in a tick when the space bar is held down when
the tick begins, and hidden otherwise: a tap of the space bar that is
over before the tick begins does not show it.

**Report.** The program prints `tick T: level L, marker M`, where `T` is
the tick's number, counting from 1, `L` is the level after this tick's
change, and `M` is `shown` or `hidden`.

**The picture.** When each tick ends, with `L` the level then, the
window's frame holds these shapes, in this order, each later one
covering the earlier ones where they overlap:

1. **The track**: a rectangle whose top left corner is at `x` = 20, `y` =
   52, 148 points wide and 14 points high, colour (70, 70, 80).
2. **The bar**, only when `L` is greater than 0: a rectangle whose top
   left corner is at `x` = 22, `y` = 54, 12 * `L` points wide and 10
   points high. Its colour is (230, 70, 50) when `L` is 9 or more, and
   (60, 200, 90) otherwise.
3. **The limit**: a line from its first end at `x` = 130, `y` = 46 to its
   second end at `x` = 130, `y` = 72, 2 points thick, colour
   (255, 255, 255) with an alpha of 96.
4. **The light**: a circle around `x` = 184, `y` = 59, with a radius of
   7. Its colour is the bar's colour when `L` is greater than 0, and
   (90, 90, 90) when `L` is 0.
5. **The marker**, only in a tick in which it is shown: a triangle whose
   corners are, in this order, (`E`, 49), (`E` - 5, 40) and (`E` + 5,
   40), each written `(x, y)`, where `E` = 22 + 12 * `L` is the `x` of
   the bar's right end. Its colour is (250, 240, 120).

A frame holds those shapes and nothing else: one for each that is in it,
none of no size, none that is fully transparent, and no other shape.

**Closing.** The program ends when its window is closed. The tick that
begins with the window closed still runs, both systems included. After
it the program prints one last line, `closed after T ticks at level L`,
where `T` is the number of ticks that ran, that last one included, and
`L` is the level then; and then the program ends.

**Trying it.** With no display, the window is headless, and a file of
commands takes the place of the person at the keyboard: it presses and
releases keys, lets ticks run, prints what the frame holds, and closes
the window. The documentation describes the commands. `try.drive` in
this directory is such a file; run the program with it as

```sh
MIDNIGHT_DRIVE=try.drive midnight run solution.12am
```

Without a file of commands the window is never closed, and the program
runs until it is stopped. The program is checked with a different file
of commands of the same kinds, and its frames are checked with the
command `draws`: each answer must list the shapes above, in the order
above. With `try.drive`, and with the commands it is checked with, the
first answer of `draws` begins with these two lines:

```
midnight: drive: draws: 2 ticks, 200x120, 4 draws
midnight: drive:   Window.background = Color(30, 30, 34, 255)
```

With `try.drive`, and with the commands it is checked with, the first
lines the program prints are:

```
tick 1: level 5, marker hidden
tick 2: level 5, marker hidden
```
