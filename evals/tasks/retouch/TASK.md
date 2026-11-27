## Retouch

The file `solution.12am` in this directory shows a boat sailing across a
harbour under the sun. It prints what it should, but its picture is
wrong. Fix it, so that the program prints and draws exactly what is
described below.

The program opens a window titled `Harbour`, 224 points wide and 144
points high, with the background colour (180, 210, 235), and runs until
the window is closed. Positions and sizes are in points: `x` from the
left edge of the window, `y` down from its top edge. A colour is given as
its red, green and blue bytes.

The boat's place is given by its `x`, which starts at 12. The program
has two systems, which run once a tick in this order: `sail` moves the
boat 2 points to the right, so its `x` goes up by 2, and `report` prints
the tick's line, `tick T: boat at X`, where `T` is the tick's number,
counting from 1, and `X` is the boat's `x` after this tick's move. The
program may have other systems, which run after `report`. Keep the
program's two systems; what else it has may change.

**The picture.** When each tick ends, with `X` the boat's `x` then, the
window's frame holds these five shapes, in this order, each later one
covering the earlier ones where they overlap:

1. **The sea**: a rectangle whose top left corner is at `x` = 0, `y` =
   96, 224 points wide and 48 points high, colour (30, 80, 160).
2. **The sun**: a circle around `x` = 40, `y` = 30, with a radius of 14,
   colour (255, 220, 90).
3. **The hull**: a triangle whose corners are, in this order, (`X`, 92),
   (`X` + 40, 92) and (`X` + 30, 102), each written `(x, y)`, colour
   (120, 70, 40). Its lower part covers the top of the sea.
4. **The cabin**: a rectangle whose top left corner is at `x` = `X` + 6,
   `y` = 84, 12 points wide and 10 points high, colour (200, 60, 50). Its
   lower edge covers the top of the hull.
5. **The mast**: a line from its first end at `x` = `X` + 28, `y` = 92 to
   its second end at `x` = `X` + 28, `y` = 60, 2 points thick, colour
   (60, 40, 30).

Every frame holds exactly these five shapes and nothing else: one for
each, none of no size, none that is fully transparent, and no other
shape.

**Closing.** The program ends when its window is closed. The tick that
begins with the window closed still runs, both systems included. After
it the program prints one last line, `closed after T ticks`, where `T` is
the number of ticks that ran, that last one included, and then the
program ends.

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
command `draws`: each answer must list the five shapes above, in the
order above. With `try.drive`, and with the commands it is checked with,
the first answer of `draws` begins with these two lines:

```
midnight: drive: draws: 3 ticks, 224x144, 5 draws
midnight: drive:   Window.background = Color(180, 210, 235, 255)
```

With `try.drive`, and with the commands it is checked with, the first
lines the program prints are:

```
tick 1: boat at 14
tick 2: boat at 16
```
