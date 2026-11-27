## Poster

A window shows a still picture: a sky, a sun, a mountain that stands in
front of the sun, the ground, and a path across the ground.

The program opens a window titled `Poster`, 240 points wide and 180
points high, with the background colour (20, 24, 36), and runs until the
window is closed. Positions and sizes are in points: `x` from the left
edge of the window, `y` down from its top edge. A colour is given as its
red, green and blue bytes, and as its alpha byte too where that is not
255.

**The picture.** When each tick ends, the window's frame holds these five
shapes, one of each, in this order, each later one covering the earlier
ones where they overlap:

1. **The sky**: a rectangle whose top left corner is at `x` = 8, `y` =
   12, 224 points wide and 100 points high, colour (40, 70, 130).
2. **The sun**: a circle around `x` = 182, `y` = 40, with a radius of 18,
   colour (250, 210, 60).
3. **The mountain**: a triangle whose corners are, in this order,
   (100, 112), (172, 34) and (226, 112), each written `(x, y)`, colour
   (90, 80, 70). Its peak covers part of the sun.
4. **The ground**: a rectangle whose top left corner is at `x` = 8, `y` =
   112, 224 points wide and 56 points high, colour (50, 110, 40).
5. **The path**: a line from its first end at `x` = 64, `y` = 166 to its
   second end at `x` = 118, `y` = 114, 3 points thick, colour
   (200, 180, 140) with an alpha of 160, so the ground shows through it.

The frame of every tick, the first tick included, holds exactly these
five shapes and nothing else: one for each, none of no size, none that is
fully transparent, and no other shape. The picture is the same in every
tick's frame.

**Each tick** the program prints one line, `tick T`, where `T` is the
tick's number, counting from 1. The program has one system,
`report`, which prints that line; it may have other systems, which run
after `report`.

**Closing.** The program ends when its window is closed. The tick that
begins with the window closed still runs and prints its line. After it
the program prints one last line, `closed after T ticks`, where `T` is
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
midnight: drive: draws: 2 ticks, 240x180, 5 draws
midnight: drive:   Window.background = Color(20, 24, 36, 255)
```

With `try.drive`, the program prints 5 lines, and the first is:

```
tick 1
```
