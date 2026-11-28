# 0013 — Images

- **Status:** approved
- **Depends on:** 0012

## Goal

A Midnight program draws shapes and text. A character is a circle, and
art made in a paint program has no way in. After this spec a program
names its pictures and draws them:

```ruby
WHITE = Color(255, 255, 255)
MOTH = Sheet("art/moth.png", 16, 16)

component Moth
  x: Float32
  y: Float32
  dx: Float32
end

system drift
  each Moth do |moth|
    moth.x += moth.dx
    moth.dx = -moth.dx if moth.x < 0 || moth.x > 288
  end
end

system show
  each Moth do |moth|
    wings = MOTH[(Time.tick / 6) % MOTH.frames]
    wings = wings.flip_x if moth.dx < 0
    Draw.image(moth.x, moth.y, wings, 2, WHITE)
  end
end

Window.background = Color(16, 16, 32)
Window.open("Moths", 320, 200)
spawn Moth(40, 60, 1.5)
spawn Moth(200, 120, -1)
World.run
```

`MOTH = Sheet("art/moth.png", 16, 16)` is a declaration: a top-level
constant whose value names a PNG file and the size of its cells. The
compiler reads the file when the program is built, checks that it is a
PNG, that it divides into whole cells of 16 by 16 texels, and that
every frame it can see the source name exists; the built program has
the pixels. There is no call that loads a picture while a program
runs, so once a program has compiled there is no missing file, no
wrong size and no picture that is not there yet.

`Draw.image(x, y, image, scale, color)` is a sixth statement under
`Draw`, in the shape of `Draw.text`: the top left corner, what is
drawn, how large, and a color last. `scale` is the size of a texel in
points, and `color` multiplies each texel, so `Color(255, 255, 255)`
draws the image as it is.

An image is a value, of the built-in type `Image`: a component can
hold the picture of its entity, and a GPU system can choose and draw
one. Which pixels an image covers is decided as for text: each texel
is a small rectangle, and a pixel of the frame takes the texel whose
rectangle holds its center. That is a rule in the reference, and
`draws` lists an image by its name:

```
midnight: drive:   Draw.image(40.0, 52.5, MOTH[2].flip_x, 2.0, Color(255, 255, 255, 255))
```

A small picture can also be written in the source, as rows of
characters, so that a program with pictures can still be one file.

This spec also settles how a program names a file it is built from.
Sound and models, in later specs, are declared in the same form, by
the same rules for paths.

### How this serves the principles

- **Agents write, people review.** The files a program reads are its
  top-level constants, found without running anything, and a reviewer
  sees every one in the diff. A draw says everything in its arguments:
  no texture to bind, no origin that changes what the position means,
  no flip that moves the picture.
- **The domain is in the language.** The compiler reads the picture, so
  it checks what a library could not: a sheet's cells against the
  image's size, a frame against the sheet's count, a row of a written
  picture against its width.
- **Fast by construction.** An image is a number in a record of 32
  bytes; every image of a frame is drawn in the one ordered pass that
  draws the shapes, whatever the order of images, and a GPU system
  draws images without moving anything.
- **Observable and repeatable.** A program's pictures are fixed when it
  is built: the same source tree gives the same program on every
  machine. `draws` names each image and frame with no device,
  `midnight assets` says what each file holds, and which pixels an
  image covers is a definition the tests hold every device to.
- **Errors teach.** A missing file, a file that is not a PNG, cells
  that do not divide the image, a frame out of range, a ragged row, a
  path given at the draw and the names other toolkits use are each an
  error of one line that says what to write.
- **Grown by building things in it.** Games made of circles and
  rectangles are what Midnight can show today; pictures are the next
  thing every one of them wants.

One principle is worked against, and the design answers it: the
documentation's size is paid at the start of every session. How a PNG
file is read is a definition a program's author never needs, so it
lives in a document of its own, `docs/png.md`, and the reference says
what an author needs in one paragraph.

## Scope

- Declaring an image from a PNG file, a sheet of frames from a PNG
  file, and an image written as rows of characters.
- The rules for a path in source, for images and for `require`.
- The type `Image`, its members, and what a sheet's name allows.
- The statement `Draw.image`, where it may stand, in GPU systems too,
  and what it adds to a frame.
- Which pixels an image covers and what color they get, at every
  scale.
- Which PNG files are read and what their pixels are.
- Images in `draws`, `query`, `to_s` and their JSON forms.
- The command `midnight assets`.
- An example with moving pictures.
- The reference becomes Midnight 0.12 and the compiler 0.12.0.

## Non-goals

- Loading a picture while a program runs, from a path or from
  anywhere: user files, saved pictures, pictures made by a program.
- Rotation by an angle and quarter turns. They come together in a later
  spec, with a sine defined by the reference.
- Stretching an image to a width and a height of another shape, a
  pivot, drawing a part of an image chosen at the draw, tiling an image
  over an area, and nine-slice scaling.
- Colors other than a multiplied one: a silhouette, a flash of white,
  swapping colors at the draw.
- Sheets whose cells have margins or space between them, cells of
  unequal size, packed atlases described by another file, files of
  paint programs (Aseprite, Photoshop), and formats other than PNG.
- Options of a declaration, written by name.
- A sheet as a value: a field, a variable or a parameter that holds
  one.
- A sheet made of written pictures.
- An animation construct, and stepping from a frame to the next: a
  named run of frames that loops or holds. A frame is arithmetic on
  `Time.tick`.
- Tile maps.
- Smoothed edges and filtering. A pixel takes one texel's color.
- Choosing when a picture is brought into memory or onto the device,
  and releasing one.
- Libraries found by name, and a directory of files that several
  programs share.
- A contact sheet of a program's images, and commands that show an
  image in a window.
- Sound, models and fonts as assets. This spec fixes the form their
  declarations will take and nothing else of them.

## Design

### Declaring an image

Three forms make an image or a sheet. Each is the whole value of a
top-level constant and stands nowhere else:

```ruby
HERO = Image("art/hero.png")
WALK = Sheet("art/walk.png", 16, 16)
SLIME = Image(8, 6, GREENS, [
  "..0000..",
  ".011110.",
  "01211210",
  "01111110",
  "01111110",
  ".000000."
])
```

| Form | Declares |
| ---- | -------- |
| `Image(path)` | The picture in the PNG file at `path`. |
| `Sheet(path, cell_width, cell_height)` | The PNG file at `path` cut into cells `cell_width` texels wide and `cell_height` high. |
| `Image(width, height, colors, rows)` | The picture written in `rows` (see [A picture written in the source](#a-picture-written-in-the-source)). |

- `path` is a string literal, of either kind, with no interpolation,
  by the rules of [Paths](#paths).
- `cell_width`, `cell_height`, `width` and `height` are each a whole
  number the compiler knows, greater than zero. The compiler knows what
  "What a GPU system costs" in the reference says it knows: a literal,
  an `Int` constant made from literals and other such constants with
  `+`, `-`, `*`, `/`, `%` and `**`, and arithmetic on those.
- The arguments of a form go by position. An argument given by name is
  a compile error: names are kept for options, which this spec has
  none of.
- The constant's name is the image's name: what `draws`, `to_s` and
  `midnight assets` print.
- A **texel** is one pixel of a picture as its file or its rows hold
  it.
- The file is read when the program is compiled, by every command that
  compiles: `check`, `build`, `run`, `plan`, `loops`, `emit-c`,
  `emit-wgsl`, `assets` and `session start`. Each reads the whole file
  and gives the same diagnostics, so a file that passes `check` builds.
- The same file may be declared by more than one constant, as an image
  and as a sheet or with different cells. Each declaration is an image
  or a sheet of its own.
- The forms are not expressions. Written anywhere else (in a system, a
  function, an argument, a field's default, a list, as part of a larger
  expression, or as the value of a lowercase name) each is a compile
  error that says where it goes. Parentheses around a form change
  nothing. `Image()` alone, the [empty image](#the-type-image), is an
  expression and may stand anywhere.
- The word that begins the form says what is declared. The file's name
  says nothing: the file's own first bytes say what it is.

`Image` and `Sheet` are built-in names and can't be declared by a
program; as with every built-in name, an enum's variant may be spelled
the same. A program that names them does not thereby have a window, as
with `Color`; one that has a `Draw.image` statement does, as with every
draw.

**A sheet.** The image's width must be a whole multiple of
`cell_width` and its height of `cell_height`; anything else is a
compile error that gives the image's size and the cell's. The cells
are the sheet's **frames**, numbered from 0 at the top left, left to
right, then the next row down.

A sheet is used by its name, and only so:

| Written | Type | Meaning |
| ------- | ---- | ------- |
| `WALK[i]` | `Image` | Frame `i`, an `Int`. A negative index counts from the end, as a list's does. |
| `WALK.first`, `WALK.last` | `Image` | The first frame and the last, as a list's. |
| `WALK.frames` | `Int` | How many frames it has. |
| `WALK.columns` | `Int` | How many frames are in a row. |
| `WALK.rows` | `Int` | How many rows it has. |

- `frames`, `columns` and `rows` are numbers the compiler knows: they
  may stand wherever such a number is asked for, in a constant's value,
  and in a GPU system.
- An index is in range from `-frames` to `frames - 1`. One the
  compiler knows is checked when the program is compiled, and out of
  range it is a compile error. Any other index is checked when it is
  evaluated, and out of range it is a run-time error (see
  [Diagnostics](#diagnostics)). In a constant's value that is when the
  program starts, at the constant's line.
- A sheet's name written any other way is a compile error: alone as a
  value, as an argument, assigned to a variable or another constant, or
  with another member. `Sheet` is not a type: a field, a parameter or a
  list of it is a compile error. The hint is to hold a frame, which is
  an `Image`, or the frame's number.

**Constants of images.** A constant's value may also be an image
constant, `SHEET[i]` with any index that is a constant expression,
`SHEET.first` and `SHEET.last`, a mirror of any of these, a member of a
sheet, and `width` or `height` of any of them. So names for frames and
sequences are constants like any others:

```ruby
JUMP = WALK[8]
RUN = [WALK[4], WALK[5], WALK[6], WALK[7]]
```

Such a constant is a value, not a declaration: it prints as the image
it holds, and `midnight assets` does not list it. An element of a list
constant (`RUN[0]`) stays outside constant expressions, as indexing a
list is today; where the list's elements are images, the error's label
says to name the frame itself.

### Paths

A path in source, of an image, a sheet or a `require`, names a file by
the same rules:

- It is relative to the directory of the file it is written in.
- Its parts are separated by `/`. A part is made of ASCII letters,
  digits, `_`, `-`, `.` and spaces; it is not empty, does not begin or
  end with a space, and does not end with a `.`. So a path is never
  absolute, never has `.` or `..` as a part, and can be written on
  every system.
- The **program's directory** is the directory that holds the root
  file, found after every symbolic link in the path the root file was
  named by, the file's own included, is resolved. It is the same for
  every command, whatever the working directory.
- A file named in source must lie in the program's directory or below
  it once its own links, the last part's included, are resolved. A link
  whose target is inside is followed; the name in source is the link's
  name.
- Every part is the name on disk exactly. The compiler does not ask
  the file system whether the path opens: for each part in turn it
  lists the directory and takes the entry whose name is the part, byte
  for byte. With no such entry the path names no file, whatever the
  file system would open. So letter case never decides whether a
  program builds.
- A path of an image names the file with its extension. A `require`
  path is as before: the name without `.12am`.

A file is named in a message as `can't find` names a required file
today: by its path from the directory the root file was named in
(`examples/art/hero.png` for `midnight check examples/moths.12am`), so
that the name can be opened as printed. A path error of an image or a
sheet is at the string. A `require` keeps its `can't find` as it is,
at the statement, with its label; its other path errors are at the
string. The first rule broken, reading the path from the left, is the
one error:

| Rule broken | Message | Label |
| ----------- | ------- | ----- |
| begins with `/` | `a path in source is relative` | `name the file from this file's directory` |
| a `\` | `` a path separates its parts with `/` `` | `` write `/` on every system `` |
| an empty part (`""`, `a//b`, `art/`) | `a path has an empty part` | `parts are names with one "/" between them` |
| a part `.` or `..` | `` a path can't have `..` `` (or `` `.` ``) | `a file names files in its own directory or below it` |
| a character outside the set, or a part that begins or ends with a space or ends with `.` | `` a path can't have `é` `` (the character), or `a part of a path can't begin or end with a space`, or `` a part of a path can't end with `.` `` | `use letters, digits, "_", "-", "." and spaces, and rename the file to match` |
| no entry of that exact name | ``can't find `FILE` `` | for an image `declared here as "PATH"`; or, when entries differ from the part only in the case of ASCII letters, `` the file is named `Hero.png` ``, naming the first of them in byte order |
| the name is a directory, or cannot be read | `` `FILE` is a directory ``, or ``can't read `FILE`: REASON`` | none |
| the file lies outside the program's directory | `` `FILE` is outside the program's directory `` | `a symbolic link on the way leads out of DIRECTORY` |

These rules are new for `require`, which until now took any path the
file system took.

### A picture written in the source

```ruby
GREENS = [Color(20, 90, 40), Color(40, 170, 60), Color(250, 250, 250)]

SLIME = Image(8, 6, GREENS, [
  "..0000..",
  ".011110.",
  "01211210",
  "01111110",
  "01111110",
  ".000000."
])
```

`Image(width, height, colors, rows)` declares a picture `width` texels
wide and `height` high.

- `rows` is a list literal of `height` string literals, of either kind,
  with no escape and no interpolation, each of exactly `width`
  characters, the first row at the top.
- A `.` is a clear texel: `Color(0, 0, 0, 0)`. A digit `0` to `9` is
  the color at that place in `colors`, and `a` to `f` the colors at 10
  to 15.
- `colors` is one of two things: a list literal, or the name of a
  constant defined above, in any file loaded before, whose value is
  written as such a list literal. The list has 1 to 16 elements. Each
  element is `Color(r, g, b)` or `Color(r, g, b, a)` with every
  argument a whole number literal from 0 to 255, given by position, or
  the name of a constant defined above whose value is written as one
  such `Color(…)`. Nothing else: no arithmetic, no name of a name, no
  arguments by name, no argument left out but `a`. A constant used
  here is a list of `Color` or a `Color` like any other, and may be
  used as one.
- The compiler reads the form as it is written; nothing in it is
  computed when the program runs.

Rows and columns are counted from 0 in messages, as texels are. Each
is a compile error:

| Mistake | Where | Message | Label |
| ------- | ----- | ------- | ----- |
| A row longer or shorter than `width`, each such row | the row's string | `row 2 has 7 characters; the image is 8 wide` | `a row has one character for each texel` |
| More or fewer rows than `height` | the list | `the picture has 5 rows; the image is 6 high` | `a picture has one row for each texel of its height` |
| A character that is none of the seventeen | the character | `` `x` is not a texel `` | `` write `.` for a clear texel, or `0` to `9` and `a` to `f` for a color `` |
| An upper case `A` to `F` | the character | the same | `` write it in lower case: `a` `` |
| A digit at or past the number of colors | the character | `` `7` is past the picture's colors `` | `` `GREENS` has 3 colors: `0` to `2` `` (or `the list has 3 colors`) |
| `colors` or an element of it written another way | the argument | `the colors of a picture are written out` | `` a list of `Color(r, g, b)` with numbers, or a constant written so `` |

The same rows with another list of colors is the same picture in other
colors.

Guidance for the reference: write a picture in the source when it is
small, up to 16 by 16 texels and 24 at most; beyond that, counting rows
and characters goes wrong, for people and for agents. Draw a larger
thing with shapes, or use a PNG file. Look at a written picture with
`midnight assets --write` before trusting it.

### The type `Image`

`Image` is a built-in value type. A value of it is a picture to draw:
a declared image, a frame of a sheet, either of them mirrored, or the
empty image.

- It is a plain value: it may be a field of a struct, a component, a
  resource or an event, a variable, a parameter, a result, an element
  of a list, and it may be read, compared, mirrored and drawn in a GPU
  system.
- `Image()` is the **empty image**: no texels, 0 by 0. Drawing it draws
  nothing. A field of type `Image` is given a value or a default like
  any other field, and `Image()` is the default to write for no
  picture: `picture: Image = Image()`. Its bytes are all zero, as the
  runtime requires some value of every type to be.
- A value takes 8 bytes today, which the reference gives as a cost and
  not as a promise.

| Member | Type | Meaning |
| ------ | ---- | ------- |
| `image.width` | `Int` | Its width in texels: the cell's width for a frame, 0 for the empty image. |
| `image.height` | `Int` | Its height in texels. |
| `image.flip_x` | `Image` | The same picture mirrored left to right. |
| `image.flip_y` | `Image` | The same picture mirrored top to bottom. |

- A mirror changes neither size. Mirroring twice the same way gives
  the picture back. A mirror of the empty image is the empty image.
- `==` and `!=` compare two images: equal when they are the same
  declared image or the same frame of the same sheet, mirrored the same
  ways, or both empty. So `WALK[-1] == WALK.last`, and both print
  `WALK[23]` for a sheet of 24. Two declarations of one file are
  different images. An `Image` can't be the value of a `case`, as a struct can't.
- `to_s` of an image that comes from a declaration gives it as source
  that makes it: the declared name, `[n]` for a frame with `n` its
  number from 0, then `.flip_x` and `.flip_y` for its mirrors, in that
  order; `Image()` for the empty image. So with `JUMP = WALK[8]`,
  `JUMP.to_s` is `"WALK[8]"`, and `WALK[-1].to_s` of a sheet of 24 is
  `"WALK[23]"`. A later source of images says what its images print.
- `SHEET[i].width` and `SHEET[i].height` are the cell's size whatever
  `i` is: they are numbers the compiler knows. The index is still
  evaluated and checked.
- In a GPU system, and in a function while one calls it, `width` and
  `height` are read only of an image the compiler knows: a constant of
  type `Image`, `SHEET[i]`, `SHEET.first`, `SHEET.last`, `Image()`, and
  mirrors of these. Of any other expression (a field, a variable, a
  parameter) they are a compile error, and in a function the error has
  the usual note that names the system and the call.

### The statement

```ruby
Draw.image(x, y, image, scale, color)
```

| Argument | Type | Meaning |
| -------- | ---- | ------- |
| `x`, `y` | `Float` | The top left corner of the image, in points. |
| `image`  | `Image` | What is drawn. |
| `scale`  | `Float` | The width and height of a texel, in points. |
| `color`  | `Color` | Multiplies every texel. `Color(255, 255, 255)` draws the image as it is. |

- It follows every rule of the other draws but the one on a negative
  literal, below: a statement with no value, every argument given by
  position, an `Int` converting to `Float`, its numbers kept as 32-bit
  `Float`s, allowed wherever a statement can stand, and a draw while no
  window is open dropped after its arguments are evaluated. Its
  arguments are evaluated in the order written; the check of a sheet's
  index is part of evaluating the image, so in
  `Draw.image(a(), b(), WALK[c()], d(), e())` a failed index stops the
  program before `d` and `e` run.
- The image covers the rectangle from `x`, `y` that is
  `image.width * scale` points wide and `image.height * scale` high:
  where `Draw.rect(x, y, image.width * scale, image.height * scale, c)`
  lies when `scale` is a number 32 bits hold exactly, as whole numbers
  and halves are. A mirror does not move it.
- A `scale` that is not greater than zero draws nothing, and so do the
  empty image and a draw with a NaN among its numbers. None is an error
  while the program runs. One mistake is caught when the program is
  compiled, because it is how other toolkits mirror: a `scale` written
  as a number literal below zero is a compile error that says to mirror
  with `flip_x` or `flip_y`. A literal zero is not an error, and
  neither is any other draw's size.
- `color` multiplies each of a texel's four bytes (see
  [The color of a texel](#the-color-of-a-texel)): its `r`, `g` and `b`
  tint the picture and its `a` fades it. A program writes
  `WHITE = Color(255, 255, 255)` once and gives it to every draw that
  wants the picture as it is.

**Even texels.** On a window whose scale is `s`, a texel covers
`scale * s` pixels of the frame, and texels are all the same size only
when that is a whole number. An image at a `scale` of 2 looks even at
window scales of 1, 1.5 and 2, and a `scale` of 4 at 1.25 as well; a
`scale` of 1, at 1 and 2. At a `scale` of 2 a texel is the size of a
pixel of the font in text of size 24, so pictures and text line up.
Other scales are drawn by the same rule and are as exact, but some
rows and columns of texels are a pixel wider than others.

**Animation** is arithmetic. Frame `start + (Time.tick / ticks) %
count` shows `count` frames from frame `start`, each for `ticks` ticks,
and
an entity that starts its own sequence keeps the tick it began at in a
component. A list constant of frames, `RUN[i]`, reads well for a
sequence out of order, and keeps its `each` on one core as any list
does.

### Which pixels an image covers

This extends "Which pixels a shape covers", with its pixel centers and
its 32-bit steps. An image `W` texels wide and `H` high, drawn at `x`,
`y` with a scale `u`, has its texel in column `c` (0 to `W - 1`) and
row `r` (0 to `H - 1`) over the rectangle

```
from x + c * u        to x + (c + 1) * u
from y + r * u        to y + (r + 1) * u
```

each bound computed as the product of the whole number (`c`, `c + 1`,
`r` or `r + 1`, converted to the nearest 32-bit `Float`) and `u`,
rounded, then the sum, rounded. A pixel of the frame is covered when
its center is in a texel's rectangle, by the rectangle test
(`left <= px < right` and `top <= py < bottom`), and that texel gives
it its color.

- Every bound is computed from the image's corner: two texels side by
  side share the bound between them, so an image has no gap and no
  overlap between its texels, whatever the numbers, and at most one
  texel covers a pixel.
- The bounds never decrease as `c` and `r` grow. Where two bounds are
  equal, the texel between them covers nothing.
- `u` is the argument itself. Nothing is divided.
- The whole numbers are exact as 32-bit `Float`s: a declared image has
  at most 8192 texels a side.
- At a window scale of 1, `Draw.image(10, 20, HERO, 1, WHITE)` of a 16
  by 16 image puts each texel on one pixel: columns 10 to 25, rows 20
  to 35. With a `scale` of 2 each texel is a square of four pixels.

**Which texel.** The texel in column `c` and row `r` of the rule is
the texel of the picture in column `c`, or `W - 1 - c` when the image
is mirrored left to right, and row `r`, or `H - 1 - r` when mirrored
top to bottom. For frame `f` of a sheet with `n` frames in a row, the
picture is the cell whose top left texel is in column
`(f % n) * cell_width` and row `(f / n) * cell_height` of the file.

**Nothing.** Extending the list of "The tests, exactly": a draw of an
image covers no pixel when one of its numbers is a NaN, when `scale` is
not greater than zero, and when the image is empty.

### The color of a texel

Extending "Color": a pixel that an image covers gets the color `m`
made from the texel `t` and the draw's color `k`, each of the four
bytes on its own:

```
m = (t * k + 127) / 255, the whole number part
```

Then `m` is laid on the pixel as the color of a shape is: with an `a`
of 255 the pixel becomes `m`'s `r`, `g` and `b`; with an `a` of 0
nothing changes; in between it is laid over what the pixel holds, by
the rule of "Color".

- With `k` white and opaque, `m` is `t` exactly.
- A clear texel changes nothing, whatever `k` is.
- A texel's color is what its file or its rows give, with no other
  step: alpha is not multiplied into the colors beforehand.

### Determinism of a frame

- The list of draws holds an image as a value the program computed,
  exact as every such value.
- Which pixels an image covers follows the rule above on every device,
  with the allowance the reference already gives a shape: a device may
  differ only at a pixel for which the two sides of a test of one
  texel's rectangle are equal or one 32-bit step apart, and for a draw
  with a number that is infinite, or below about 1.18e-38 in magnitude
  and not zero, or whose test overflows.
- A pixel whose `m` has an `a` of 255 gets exactly `m`'s bytes on every
  device. One whose `m` has an `a` from 1 to 254 may differ as a
  translucent shape's does; an image is one draw for that count, since
  no two of its texels cover one pixel.

### Reading a PNG file

`docs/png.md`, a new document, is the definition of how a PNG file is
read; what follows is its content. The reference's "Images" says what
an author needs and links there: every PNG is read; its bytes are
taken as sRGB colors with nothing converted; 16-bit samples are rounded
to the nearest byte; an animated file is its still picture; a side is
at most 8192 texels.

The file is a PNG as the PNG specification (third edition) defines it.
The same bytes give the same image on every machine.

- **Accepted:** every color type and bit depth the specification pairs
  (grayscale at 1, 2, 4, 8 and 16 bits; truecolor at 8 and 16; indexed
  at 1, 2, 4 and 8; grayscale with alpha and truecolor with alpha at 8
  and 16), interlaced or not.
- **Chunks.** `IHDR`, `PLTE`, `tRNS`, `IDAT` and `IEND` are read. Every
  chunk's CRC is checked. Any other chunk the specification's naming
  marks as critical is an error; every other chunk is skipped.
- **Order.** `IHDR` comes first, once, 13 bytes long. `PLTE` comes at
  most once, before the first `IDAT`, with 1 to 256 entries of 3 bytes;
  in a grayscale file it is an error, and in a truecolor file it is
  skipped. `tRNS` comes at most once, after `PLTE` and before the first
  `IDAT`; in a file with alpha samples it is skipped. The `IDAT` chunks
  follow one another with no chunk between them. `IEND` ends the
  picture, and what follows it is not read. A chunk's length is at most
  2^31 - 1. Anything else is an error of a damaged file.
- **The header.** A width or height of 0, a color type and bit depth
  the specification does not pair, and a compression, filter or
  interlace method it does not have are errors of a damaged file.
- **The data.** The `IDAT` chunks hold one zlib stream (RFC 1950, with
  method 8, a window of at most 32,768 bytes and no preset dictionary)
  of deflate data (RFC 1951). Its Adler-32 is checked. A stream that
  ends early, that holds more or fewer bytes than the header's size
  needs, or that is followed by more bytes in `IDAT`, is an error. Each
  row's filter is one of the five.
- **To four bytes a texel.** A gray sample `g` is the color `g`, `g`,
  `g`. An indexed sample is its entry of `PLTE`; an index with no entry
  is an error. A sample of `d` bits, `d` below 8, becomes
  `v * 255 / (2^d - 1)`, which is exact. A sample of 16 bits becomes
  `(v * 255 + 32767) / 65535`, the whole number part: the nearest byte.
- **Alpha.** An alpha sample is the texel's `a`, converted as a color
  sample is. With no alpha samples, `a` is 255, but for `tRNS`. For an
  indexed image it gives the `a` of the first entries of the palette,
  and the entries it does not reach have 255; one longer than the
  palette is an error. For grayscale it is 2 bytes and for truecolor 6,
  another length is an error, and it names one color, compared at the
  file's own bit depth, whose texels get an `a` of 0.
- **The bytes are sRGB**, as a `Color`'s are. `gAMA`, `cHRM`, `iCCP`,
  `sRGB` and `cICP` are not read, and neither is anything else that
  would change a color. That is a statement about an `Image`'s texels
  as colors; reading changes no byte of the file's samples beyond the
  conversions above.
- **An animated PNG** is the picture its `IDAT` chunks hold, the one a
  viewer without animation shows.
- **Size.** Each side is from 1 to 8192 texels. The header is checked
  before anything is decoded, and no more memory is asked for than the
  header's size needs.

A file that is not read gives one compile error at the string that
names it:

| The file | Message | Label |
| -------- | ------- | ----- |
| begins `FF D8 FF` | `` `art/hero.jpg` is a JPEG file `` | `images are PNG files` |
| begins `GIF87a` or `GIF89a`; `BM` followed by a length that is the file's; `RIFF` with `WEBP` at byte 8; `qoif` | the same with `a GIF file`, `a BMP file`, `a WebP file`, `a QOI file` | the same |
| is empty, or begins any other way than a PNG does | `` `art/hero.png` is not a PNG file `` | the same |
| has a side above 8192 | `` `art/map.png` is 9000 by 600 texels `` | `an image has at most 8192 texels a side` |
| breaks any other rule above | `` `art/hero.png` is damaged: `` and what failed (a chunk's CRC, a chunk out of place, the header, the compressed data, more or fewer pixels than the header says) | `save it again as a PNG` |

### Limits

- A declared image has from 1 to 8192 texels on a side.
- A program's images and sheets hold at most 33,554,432 texels in all,
  128 MiB at four bytes a texel. The total counts every declaration,
  drawn or not, each with its whole file, and a file declared twice
  twice.
- A program has at most 65,535 declarations of images and sheets.

Every program within these limits builds, and draws on every device:
the second limit is the largest buffer that every device is required
to take whole, so nothing else about a program's pictures, their sizes
or their number, decides whether it runs. A later version may raise
it.

A declaration gets one error, the first of these that applies: its
path's rules; no such file; not a PNG; a damaged header; a side above
8192; for a sheet, cells that do not divide the size; the total of
texels; the rest of the file. So sizes come from headers: the total is
known before any picture is decoded. The error of the total is at the
declaration that takes it past the limit, `the program's images hold
40000000 texels`, with the label `a program has at most 33554432`.
The 65,536th declaration is the error `a program has at most 65535
images and sheets`.

### Where an image may be drawn

Wherever a draw of a shape may stand, in a GPU system too:

```ruby
gpu system show
  each Moth do |moth|
    Draw.image(moth.x, moth.y, MOTH[(Time.tick / 6) % MOTH.frames], 2, WHITE)
  end
end
```

- **Several cores.** A draw of an image is a draw under rule 5 of
  independent loops and weighs what every draw does: its arguments and
  8. An `Image` is a plain value under rule 1. Indexing a sheet weighs
  1 besides its index, as reading a field does; `first`, `last` and a
  mirror weigh 1; `width` and `height` weigh 1 for a value and nothing
  for an image the compiler knows. A loop whose index fails stops with
  the error of the earliest failing row. `midnight loops` has no reason
  for an image.
- **GPU systems.** `Draw.image` follows "Draws in a GPU system" as the
  shapes do: directly in the block, a slot of 32 bytes for each
  statement, the record written on the device. `Image` joins the types
  a query term's fields may have. A block may read an `Image` from a
  component, a constant or a resource field, index a sheet or take its
  `first` or `last`, which cannot fail, mirror an image, compare two,
  and assign one to a field of its own components.
- **A sheet's index in a GPU system** is an operation that can fail,
  as integer division is: a run whose index is out of range goes on
  with the empty image, and the program stops when the block ends. In
  the block's CPU form such an index counts what its check costs, as an
  `Int` `/` does. An index cannot fail when the compiler can bound it
  within `-frames` to `frames - 1`, by the rule "What a GPU system
  costs" gives for a store into a compact field: `i % SHEET.frames` is
  in range for any `i`, since `%` takes the sign of the divisor. A
  negative index is the frame `frames` later, on the device as on the
  CPU, so the image a block computes holds a frame from 0. The value of
  a local
  variable is not followed: write the bounded expression inside the
  brackets, as above. "What a draw costs" says so under what follows.

### Diagnostics

The examples show the form. A message names a file as "Paths" says.

```
error: `art/walk.png` is 130 by 48 texels, which is not a whole number of cells 16 wide
  --> moths.12am:12:14
   |
12 | WALK = Sheet("art/walk.png", 16, 16)
   |              ^^^^^^^^^^^^^^ a sheet's width is a multiple of its cell's width
```

```
error: index 24 is out of range for `WALK`, which has 24 frames
  --> moths.12am:19:8
   |
19 | JUMP = WALK[24]
   |        ^^^^^^^^ the last frame is `WALK[23]`, or `WALK.last`
```

The number is the index as written, as for a list. Below `-frames` the
label points to the other end:

```
error: index -25 is out of range for `WALK`, which has 24 frames
  --> moths.12am:20:9
   |
20 | FIRST = WALK[-25]
   |         ^^^^^^^^^ the first frame is `WALK[-24]`, or `WALK.first`
```

```
error: row 2 has 7 characters; the image is 8 wide
  --> moths.12am:19:3
   |
19 |   "0121121",
   |   ^^^^^^^^^ a row has one character for each texel
```

```
error: an image is declared at the top level
  --> moths.12am:14:12
   |
14 |     hero = Image("art/hero.png")
   |            ^^^^^^^^^^^^^^^^^^^^^ write `HERO = Image("art/hero.png")` outside every system and function
```

**Declarations.**

| Written | Message | Label |
| ------- | ------- | ----- |
| `Image(path)`, `Image(w, h, colors, rows)` or `Sheet(…)` in a system, a function, an argument, a default or a list | `an image is declared at the top level` (`a sheet …`) | `` write `HERO = Image("art/hero.png")` outside every system and function ``: the form as written, and for the name the variable's in capitals when the form is assigned to one, `HERO` (`WALK` for a sheet) otherwise |
| `hero = Image("a.png")` at the top level | `an image is declared as a constant` | `` write `HERO = Image("a.png")`: a constant's name begins with a capital `` |
| `HERO = Image("a.png").flip_x`, or any larger expression | `a declaration is the whole value of its constant` | `` declare `HERO = Image("a.png")` and write `HERO.flip_x` where it is drawn `` |
| a path that is not a string literal, or has interpolation | `the path of an image is written out` | `the compiler reads the file when the program is built` |
| `Image` with 2, 3, 5 or more arguments, or one that is not a string | `` `Image` takes a path, or a width, a height, colors and rows `` | `` `Image(path)` or `Image(width, height, colors, rows)` `` |
| `Sheet` with another count | `` `Sheet` takes a path and the width and height of a cell `` | `` `Sheet(path, cell_width, cell_height)` `` |
| an argument of a form given by name | `` the arguments of `Sheet` go by position `` | the form |
| a size that is not a whole number the compiler knows, or not greater than zero | `` `Sheet` takes its cell's size as whole numbers `` (`` `Image` takes its size … ``) | `write the number, or a constant made from numbers` |
| a width that does not divide | as the first example | as the first example |
| a height that does not divide, the width dividing | `` `art/walk.png` is 128 by 50 texels, which is not a whole number of cells 16 high `` | `a sheet's height is a multiple of its cell's height` |
| an index the compiler knows, out of range | as the second and third examples; `which has 1 frame` for one | `` the last frame is `WALK[23]`, or `WALK.last` `` for an index at or past `frames`; `` the first frame is `WALK[-24]`, or `WALK.first` `` for one below `-frames`; `` its one frame is `ONE[0]` `` for a sheet of one frame |
| `WALK` alone, as an argument or assigned | `` `WALK` is a sheet, not a value `` | `` hold a frame, `WALK[i]`, which is an `Image`, or the frame's number `` |
| `Sheet` as the type of a field, a parameter or a list | `` `Sheet` is not a type `` | the same |
| `WALK.width`, `WALK.height` | `` a sheet has `frames`, `columns` and `rows` `` | `` the size of a frame is `WALK[0].width` `` |
| `WALK.length`, `.size`, `.count` | the same | `` the number of frames is `WALK.frames` `` |
| `HERO[2]`, `HERO.frames`, `.columns`, `.rows`, `.first`, `.last` on an image | `` `HERO` is one image, not a sheet `` | `` declare it with `Sheet(path, cell_width, cell_height)` `` |
| `RUN[0]` in a constant's value, `RUN` a list of images | `not a constant expression`, as today | `` name the frame itself: `WALK[4]` `` |
| `width` or `height`, in a GPU system, of an image the compiler does not know | `a GPU system can't measure an image it does not know` | `keep the size in the component` |
| an `Image` as the value of a `case` | as for a struct today | as today |

**The statement.** For the arguments, in this order, the first that
applies is the statement's one error about the form of its arguments:

1. an argument given by name: as for the shapes, with the label
   `` its arguments go by position: `Draw.image(x, y, image, scale, color)` ``;
2. a `Color` as the first argument: as for the shapes,
   `` `Draw.image` takes its color last ``;
3. an `Image`, a sheet's name or a `String` as the first or the second
   argument, whatever the count: `` `Draw.image` takes its image third ``,
   label `` write `Draw.image(x, y, image, scale, color)` ``;
4. four arguments, the last not a `Color`:
   `` `Draw.image` takes a color last ``, label
   `` `Color(255, 255, 255)` draws the image as it is ``;
5. six arguments, the fourth and the fifth numbers:
   `` `Draw.image` takes one scale ``, label
   `the size of a texel in points: an image 16 texels wide at a scale of 2 is 32 points wide`;
6. any other count: as for the shapes,
   `` `Draw.image` takes 5 arguments, but N were given ``.

With five arguments given by position, each that does not have its
type is reported, in the same pass:

- a third argument that is a `String`:
  `` `Draw.image` takes an `Image` to draw ``, label
  `` declare the image at the top level, as `HERO = Image("…")`, and draw `HERO` ``,
  with the string as written in place of `…` when it is a literal;
- a third argument that is a number: the same message, label
  `` a frame of a sheet is written `SHEET[n]` ``;
- a third argument that is a sheet's name:
  `` `WALK` is a sheet of 24 frames ``, label `` draw one: `WALK[i]` ``;
- a `scale` written as a number literal below zero:
  `a scale is greater than zero`, label
  `` mirror with `image.flip_x` or `image.flip_y` ``;
- any other argument, as for the shapes.

**Names other toolkits use**, each an error that says what to write,
where the program defines or declares nothing by the name:

| Written | The label says |
| ------- | -------------- |
| `Draw.sprite`, `Draw.texture`, `Draw.picture`, `Draw.bitmap`, `Draw.blit`, `Draw.spr`; functions named `draw_image`, `draw_sprite`, `draw_texture`, `blit`, `spr` | `` write `Draw.image(x, y, image, scale, color)` `` |
| `Sprite`, `Texture`, `Bitmap` or `Surface` constructed or spawned | the same: an image is drawn by a statement each tick |
| Functions named `load_image`, `load_texture`, `load_sprite`; `Image.load`, `Image.open`, `Image.new` | `` images are read when the program is built: `HERO = Image("hero.png")` at the top level `` |
| `.flip_h`, `.flip_v`, `.flipped`, `.mirror`, `.flip(…)` on an image | `` `.flip_x` mirrors left to right and `.flip_y` top to bottom `` |
| `.frame(i)`, `.cell(i)`, `.get(i)`, `.sub(…)` on a sheet | `` a frame is `WALK[i]` `` |
| `.next`, `.prev`, `.previous` on an image or a sheet | `` a frame is chosen by its number: `WALK[i]` `` |
| `.w`, `.h` on an image | `` `.width`, `.height` `` |

The message for a draw the language lacks, which `Draw.sprite` and
`Draw.image` alone got, is removed. `tests/fail/draw_not_shapes.12am`,
its `# expect:` line and its `.err` are rewritten as the test of
`Draw.sprite` and its kin, and `tests/COVERAGE.md` follows.

**Run-time errors.** One is new: a sheet indexed out of range.

```
moths.12am:14: runtime error: index 24 out of range for WALK, which has 24 frames
```

The number is the index as written: `index -25 out of range for WALK,
which has 24 frames`. `which has 1 frame` for one. In a GPU system the
message names no value, as every error there: `index out of range for
WALK`.

### `draws`, `query` and JSON

A draw of an image is listed as the statement that made it, its
numbers as every draw's and its image as `to_s` gives it:

```
midnight: drive:   Draw.image(40.0, 52.5, MOTH[2].flip_x, 2.0, Color(255, 255, 255, 255))
```

`query` and `resource` print a field of type `Image` the same way.

In JSON an image value is an object of optional members, in this
order: `name` (the declared name), `frame` (for a frame of a sheet,
from 0), `flip_x` and `flip_y` (`true` or `false`). The empty image has
none of them and is `{}`; every other image of this version has `name`,
`flip_x` and `flip_y`. A shape is
`{"shape":"image","x":…,"y":…,"image":{…},"scale":…,"color":[…]}`.
The `format` stays 2. "Stability" in `docs/json.md` gains a fourth
thing a later version may add, the form of a value of a type the
language gains, and its paragraph on shapes names `image` and `scale`.

### `midnight assets`

```
$ midnight assets moths.12am
moths.12am:2   MOTH  sheet  64x16  "art/moth.png"  4 frames of 16x16, 4 a row  crc 9a0c41d7
moths.12am:3   MOON  image  24x24  "art/moon.png"                              crc 0f3be2a1
moths.12am:6   LAMP  image  8x12   written                                     crc 51c7d09e
moths.12am:21  STAR  image  5x5    written                                     crc 77e0a6b2
4 declarations, 1721 texels of 33554432
```

`midnight assets FILE` compiles the program as `check` does; with an
error it prints the diagnostics, lists nothing and ends with status 1.
Otherwise it prints, on standard output, a line for each image and
sheet, in the order declared, files in the order loaded, of seven
columns: where it is declared (`FILE:LINE`), its name, `sheet` or
`image`, its size in texels (`64x16`), its path as written in the
source in double quotes or the word `written`, for a sheet `4 frames
of 16x16, 4 a row` and nothing otherwise, and `crc` with the CRC-32 of
its texels, four bytes each (red, green, blue, alpha), rows from the
top, as `capture` gives a frame's. Every column but the last is padded
with spaces to its widest entry and followed by two spaces, as
`midnight loops` pads; the sixth is there, empty, when the program has
no sheet. The last line is `4 declarations, 1721 texels of 33554432`:
the count, and the texels against the limit; `1 declaration`, `1
texel` and `1 frame` for one. A program with none prints `0
declarations, 0 texels of 33554432`.

- `--format json` writes, on standard output as `plan` and `loops` do,
  a line for each declaration, of kind `asset`: `file`, `line`, `name`,
  `type` (`image` or `sheet`; a later version adds types), then, each
  optional, `width`, `height` and `crc` (for an image or a sheet),
  `path` (absent for a written picture), and `cell_width`,
  `cell_height`, `frames` and `columns` (for a sheet); and a last line
  of kind `assets` with `count`, and optional `texels` and `limit`,
  which this version always writes and a later kind of asset may leave
  out. A
  program with errors writes its first line and its `error` lines as
  `plan` does. `docs/json.md` lists `assets` among the commands that
  take `--format`.
- `--write DIR` also writes each declaration's picture to
  `DIR/NAME.png`, a sheet whole, as a PNG of 8 bits each of red, green,
  blue and alpha, in the form `capture` writes. `DIR` is made if it is
  not there; a file of that name is replaced. Two declarations whose
  names differ only in the case of ASCII letters are an error of
  `--write` before anything is written. The same texels always give
  the same file.
- With `--write`, `--scale N`, a whole number from 1 to 64, makes each
  texel `N` by `N` pixels; a picture that would be wider or higher than
  8192 pixels is an error that names it and the largest `N` that fits.
  `--scale` without `--write` is a usage error.
- It writes nothing else and needs no device.

It is where an agent learns a sheet's size without guessing, where a
reviewer sees what a program reads, and how a written picture is
looked at. To learn a file's size before a program is written, check a
file of one line, `X = Image("art/walk.png")`, and run `midnight
assets` on it: the skill says so.

### What this spec must not preclude

Each of these is left for later, and none is made harder:

| Later | What keeps it open |
| ----- | ------------------ |
| Bringing a picture into memory or onto the device only when needed, and releasing it | The reference names no moment at which an image is loaded and gives a program nothing to observe of it. When the pixels reach the device is in `docs/running.md`, as what this runtime does. |
| Pixels kept in a file beside the executable, a cache between builds, a compressed form | The reference says what an image is, its texels, and not where they are kept. What `midnight build` writes is described in `docs/building.md`, as what this compiler does. Reading a file depends on its bytes alone. |
| Rotation and quarter turns | The record of a draw and the layout of an `Image` value belong to one build of a program and can be repacked. The statement's five arguments need not change. |
| Smoothing | A texel is read by its index; nothing depends on a sampler. Filtering computed from texel reads gave the same bytes on the three devices as on the CPU in planning. |
| Margins and spacing in sheets, a filter, later a sound's loop or a font's size | Options are written by name after the positional arguments, `name: value`. This spec has none and makes an argument given by name in a form a compile error, so the place is free. |
| Packed atlases, names and tags from a paint program's file, cells of unequal size, runs of frames that loop or hold, the frame after a frame | A frame is a member of the sheet that is an `Image`, and other members can be. Nothing in `Image` assumes a grid. |
| A sheet as a value | It is a restriction, lifted by saying so. |
| Sound, models, fonts | The same form: a top-level constant whose value is a built-in form that names a file. The same paths. The same command lists them: every member of an `asset` line that belongs to pictures is optional. Their names (`Sound`, `Model`, `Font`) are not reserved now; the spec that takes one says which programs it breaks. |
| Assets made of several files | Paths are confined to the program's directory, which also confines the paths a file names inside itself. |
| A library found by name, and a directory several programs share | The rule confines a path to a root. A later spec adds roots; nothing here says the program's directory is the only one. |
| Pictures made or loaded while a program runs | `Image` is a type with values, not a name for declarations; `to_s`, `draws` and JSON say what a declared image prints and leave other sources to say their own. A source that can fail needs a way to say so, which the language lacks today. |
| Images that hold numbers, not colors (a model's normal map) | "The bytes are sRGB" is a statement about an `Image`'s texels as colors. Reading a PNG converts depth and nothing else, so a later kind reads the same file as numbers. |
| More texels, larger images | The table gives each declaration the place of its texels and the shader reads a texel through it, so texels may later lie in several buffers or in pages of textures. Nothing a program writes names where its texels are, and the limits only rise. |

### The compiler

- A new module reads PNG files: the zlib and inflate formats and the
  PNG reader, in the compiler's own Rust, with no `unsafe`. Every
  failure is an error value; nothing panics on any input.
- The checker recognizes the three forms as the whole value of a
  top-level constant and reads the file or the rows there. The typed
  program gains the list of declarations: name, where declared, kind,
  size, cells, texels. This changes `src/tir.rs`, a contract.
- Declarations are numbered from 1 in the order of their constants,
  files in the order loaded. An `Image` value is two 32-bit words: the
  first holds the declaration's number in its low 16 bits, bit 16 for
  a mirror left to right and bit 17 for one top to bottom, the rest
  zero; the second holds the frame, from 0, zero for an image that is
  not a frame. The empty image is all zero.
- The typed program gains a sixth shape, `Image`, with its arguments
  in source order. Every reader of a draw walks them as before.
- Constant expressions gain the forms of "Constants of images". The
  label of `not a constant expression` is unchanged but where it says
  to name a frame.
- The emitter writes, in `program.c`, the table of declarations the
  runtime prints and measures with: name, kind, size, cell size, frames
  in a row, frames, and the offset of its texels. It calls
  `mn_draw_image(x, y, image, scale, color)`.
- In a GPU block the two words of an `Image` are two 32-bit words of a
  column or a record, held as bits: the draw statement of the GPU's
  intermediate form gains typed words, in its WGSL and its C.
- In a program that has a `Draw.image` statement, the driver writes
  `midnight_images.bin` beside `midnight_window.c`: every declaration's
  texels, four bytes each (red, green, blue, alpha), rows from the top,
  in the order of declaration; and builds the program with
  `-DMN_IMAGES`, which only `midnight_window.c` reads: it includes the
  file's bytes with `#embed`, under a pragma that keeps the build's
  flags as they are. Generated code never includes it. `#embed` is one
  feature of C23; a C compiler without it fails with its own message,
  which `midnight` passes on as it does any failure of the C compiler.
- A program with no `Draw.image` is built without `MN_IMAGES`: its
  generated shaders are byte for byte 0.11's, and so is its generated
  C, but for the version it carries, when it declares no image.
- `midnight assets`, with a PNG writer that keeps to the form `capture`
  writes, and `--write`.
- `require` paths are checked by the rules of "Paths".
- The built-in names gain `Image` and `Sheet`.
- The evals' harness: the `draws` check reads a `Draw.image` line.
  Grading runs `midnight assets --format json` on the solution where
  the agent left it and copies each `path` it lists, by the path rules,
  beside the copy of the solution it builds. `evals/README.md` says a
  task may give PNG files.

### The runtime

- **The record.** A draw of an image is one record of 32 bytes, of kind
  `MN_DRAW_IMAGE`, which is 7; 6 is the window's own kind for a glyph,
  and `midnight_rt.h` says so. Its words: `x`, `y` and `scale` as
  32-bit `Float`s, then the image's two words as bits, then a word of
  zero, then the color. It holds no pointer and nothing outside itself,
  so pieces join it and a GPU block writes it as they do a shape's.
- `midnight_rt.h`, a contract, gains `MN_DRAW_IMAGE`, the record's
  layout, the type of an image value, the table of declarations and
  `mn_draw_image`, which may be called in a piece: "Several threads"
  lists it. The check of a sheet's index is generated code.
- `midnight_rt.c` prints an image in `draws`, `query`, `resource` and
  `to_s`, in text and JSON, from the table, builds lines of any length
  for them, and never renders: it has no texels. Its tables of draws by
  kind, the test of which kinds `draws` lists and the count of draws
  take the new kind.
- **Rendering.** Built with `MN_IMAGES`, the shape pipeline has the
  texels as binding 2, a storage buffer of one 32-bit word a texel, and
  the table of declarations as binding 3, a storage buffer that the
  vertex and the fragment stages read. Both numbers are fixed, whether
  or not the program draws text. A program with no declaration has one
  clear texel and a table of the empty image alone, so that both
  buffers have something in them. Built without `MN_IMAGES`, the
  pipeline is 0.11's.
- **The shader.** A record of an image becomes a quad over the image's
  rectangle, made one pixel larger and clamped, as for the shapes. The
  fragment stage finds the column of the texel that holds the center,
  among columns 0 to `W - 1`: an estimate, which may use a division,
  clamped to them; then at most four steps to the neighbor, each taken
  while a bound's test says so; then, where `left <= px < right` still
  does not hold, a binary search over the bounds with the same tests,
  at most 13 halvings. The bounds never decrease from one column to the
  next, so the search ends at the one texel whose rectangle holds the
  center, whatever the numbers. Rows likewise. Every bound is computed
  as the rule says, and every product and every sum of the rule goes
  through the xor barrier. The tests alone decide. A center outside the
  first or last bound is not covered. The image's number, its frame and
  the texel's index are each checked against the table before anything
  is read: a record that names what does not exist draws nothing. The
  color is computed in whole numbers and leaves as a shape's does.
- The texels and the table are written to the device once, when the
  shape pipeline is made: at the first frame that is rendered, as the
  font is. `MIDNIGHT_GPU=stats` does not count that upload, as it does
  not count the font's. Before it makes the buffer, the runtime
  compares the texels' size with what the device binds; a device that
  cannot hold them stops the program with the GPU error `the device
  can't hold the program's pictures`, and
  `lines_no_program_writes_on_demand` has that line in both forms.

### The reference renderer

`tests/render/mod.rs` reads an image's name, frame and mirrors from a
`draws` answer, takes the texels of each declaration from the PNG that
`midnight assets --write` writes for the program, read by the suite's
own reader of `capture`'s files, and draws by the rules above, apart
from the shader: which texel, the color of a texel, then "Color". Its
unit tests: two texels side by side and two rows share their bounds; a
scale of 1 puts each texel on a pixel; scales that make the bounds
inexact; an image whose bounds repeat; each mirror and both; a frame in
the last column and the last row; a texel of each kind of alpha under
each kind of color; the empty image, a NaN, and a scale of zero and
below; and, at each scale "Even texels" names, on windows of 320, 333,
401 and 1,280 points, every column of an image as wide as the window
covering the same number of pixels.

### Tests

All run on x86_64-linux, on the development machine. Binary files
(`tests/png`, the pictures of `tests/run`, `tests/fail` and
`tests/assets`, `examples/art`) are added to git before any `nix`
command, with `*.png binary` in `.gitattributes`.

- **The PNG reader.** `tests/png/` holds the 176 files of PngSuite
  (2017jul19) with its `PngSuite.LICENSE`, and `expected.txt`: for each
  of the 162 valid files its size and the CRC-32 of its texels as
  ImageMagick 7.1.2-32 writes them with `-depth 8 rgba:`, and for each
  of the 14 corrupt files the message this spec gives it. The file's
  first lines say how it was made. `png_suite_is_read` requires
  `midnight assets --format json` of a program that declares every
  valid file to give exactly those sizes and CRCs, and `check` of a
  program that declares each corrupt file to give exactly its message.
  The suite's `png_files_are_read`, the test of its own reader, stays.
  Unit tests in the module cover what PngSuite does not: each error's
  message, a file cut short at every length, the size limits, each rule
  of order, an animated file, a 16-bit sample at each rounding
  boundary, a `tRNS` color at 16 bits, a `tRNS` shorter and longer than
  its palette. A test with a fixed seed runs a hundred thousand damaged
  inputs through the reader and requires an error or an image from
  each.
- **Paths.** The function that matches a part against a directory's
  entries has unit tests on listings a file system of either kind
  would give: an entry of another case is not a match, and the hint
  names the first such entry in byte order. A `tests/fail` program
  whose directory holds `Hero.png` and `HERO.png` for `"hero.png"`
  expects `HERO.png` in the label. The link cases build their tree in
  a scratch directory (a link out, a link that stays inside, a root
  file named through a linked directory); nothing commits a symbolic
  link. `check` gives the same diagnostics from the repository's root
  and from the program's directory.
- `tests/run`, with `.drive` and `.draws`, run by `draws_match_expected`
  and in JSON by `run_both_forms`:
  - an image, a frame of a sheet in the first and last cells, by a
    negative index, by `first` and `last`, each mirror and both, an
    `Int` and a `Float` among the numbers, a color that tints and one
    that fades, the empty image, a scale of zero, a negative scale
    computed while the program runs, a NaN in each number, a draw
    before `Window.open` whose arguments print, and a draw whose five
    arguments each call a function that prints;
  - images held in a component, a resource and a list, passed to and
    returned by a function, compared, printed with `to_s`, read by
    `query` and `resource`;
  - a written picture with sixteen colors, clear texels and a
    translucent color, colors given as a list and by name, and the
    same rows with two lists of colors;
  - the members of an image and a sheet in constants and in code;
    constants that name frames and a list of frames;
  - one file declared as an image and as two sheets;
  - a program that declares images and never draws one, which is built
    without the window's flags and reads no window variable;
  - a program that declares no image and draws `Image()` and a field
    that holds it;
  - images drawn from an `each`, a function, `on` and top-level code,
    in order with shapes and text over and under them.
- `tests/run`: a split loop that draws images and indexes a sheet
  (`.split`, `thread_programs`); a GPU system that reads an `Image`
  from a component, indexes a sheet with a bounded and an unbounded
  index, mirrors and draws (`gpu_programs` with `.draws`, on the CPU
  and on the device); `.stats` programs whose GPU block draws with a
  bounded index and counts no wait, and with an unbounded one and
  counts one.
- `tests/panic`: a sheet indexed past its end and below `-frames`, in
  ordinary code, in a constant's value, in a split loop where a later
  row fails first in time, and in a GPU system; a failed index in a
  draw whose later arguments would print.
- `tests/fail`: every diagnostic of this spec, in the order of their
  rules: each path rule for an image and for `require`, each kind of
  file that is not a PNG, a damaged file, a file too large, the texel
  limit, each row of the tables above, each form error of the statement
  in its order, `Image` and `Sheet` as declared names, and `Draw.image`
  in a program that opens no window. A generated program holds the
  limit of 65,535 declarations.
- A missing file gives the same error from `check`, `plan`, `loops`,
  `emit-c`, `emit-wgsl`, `assets`, `build` and `session start`.
- `tests/loops`: an `each` that draws images runs on several cores;
  one that reads a list of images runs on one, with the list's reason.
- `tests/json`: a `run` whose `draws` lists images and whose `query`
  prints one; `assets --format json`. `tests/json_lines/mod.rs` writes
  an image value back as its text, so `run_both_forms` holds the two
  forms to each other; the suite's test of the commands that take
  `--format` has `assets` among them.
- `tests/session`: a program with a sheet, with `draws` and `query`.
- `tests/assets/NAME.12am` with `NAME.assets`: `assets_match_expected`
  requires exactly that output; `UPDATE_EXPECT=1` rewrites it.
  `assets_writes_its_images` requires the files of `--write` to hold
  the declared texels, at scales 1 and 3, to be the same bytes twice,
  and each error of `--write` and `--scale`.
- `window_drawn`: programs that draw images in the ways above, `draws`
  before each `capture`, as given and after a `scale` of 1.25, 1.5 and
  2, held to the reference renderer: every byte equal where every
  color laid on a pixel is opaque, within the allowance elsewhere.
  Among them: an image 8,192 texels wide at a scale of 0.00005 and of
  0.00001, drawn a fraction of a point left of a pixel's center near
  the right edge of a window 8,192 points wide, where the estimate is
  five columns and more from the texel; and the same image at a scale
  of 128 drawn from `x` = -1,000,000, whose bounds are rounded to
  sixteenths of a point. `window_shown`: those with no `resize` or
  `scale`, in the window tests' compositor.
- `image_stats`: `tests/run/image_stats.12am` draws shapes, text and
  images, at least one shape in every span that holds an image, and
  captures a frame each tick; with `MIDNIGHT_GPU=stats` it gives the
  lines of `tests/run/image_stats_shapes.12am`, the same program with
  every `Draw.image` taken out. A span of images alone counts one
  upload, as a span of text alone does; `docs/running.md` says so.
- `images_are_the_files`: the bytes of `midnight_images.bin` the driver
  writes are the texels `midnight assets` reports, in order.
- `the_limit_of_texels`: a program of two pictures of 4,096 by 4,096,
  the whole 33,554,432 texels, made by the test and not kept in git,
  builds, lists its total and draws its last texel on `llvmpipe` and on
  the hardware device; with a picture of 1 by 1 more it is the compile
  error of the limit.
- `tests/harness.rs` has a task with a sheet, graded with its file
  copied.
- `skill_is_short_and_works` runs the skill's new lines.
- `tests/COVERAGE.md` lists each.

### What an image costs

- A draw of an image appends 32 bytes to the frame's list, in ordinary
  code, and has a record of 32 bytes among a GPU block's.
- The device tests every pixel of the image's rectangle, made one
  pixel larger, and reads one texel for each pixel covered.
- Which images a frame draws, and in what order, costs nothing: every
  draw of a frame is in one pass whatever its image.
- An `Image` in a component takes 8 bytes for each entity.
- An index of a sheet that the compiler cannot bound is an operation
  that can fail, and makes a GPU block wait as any such operation does:
  write the bounded expression inside the brackets.
- `bench/draws.sh` gains images, and "What a draw costs" their numbers
  on the RTX 5090, the integrated Radeon and `llvmpipe`: images of 16
  by 16 pixels, up to a million a frame, and ten thousand over an
  image that fills the window.

Outside the reference, as what this compiler and runtime do:
`docs/building.md` says that a program's texels are in its executable,
four bytes each, that a debug executable adds the sanitizer's padding,
and that `#embed` is used; `docs/running.md` says that the texels are
copied to the device once, before the first picture is made, so the
first frame waits for them (about 0.3 s for a gibibyte in planning),
that the device then holds four bytes for each texel, and that a
session's program is kept in the runtime directory, which a program
with many texels may fill.

### The example

`examples/moths.12am`: moths are drawn toward a lamp that follows the
mouse. Each moth is an entity with a frame of a sheet, flapping by
`Time.tick` and mirrored by its direction; the moon is a PNG, and the
lamp and the stars are pictures written in the source. Its motion uses
`+`, `-`, `*`, `/` and `Math.sqrt` only, so its draws are the same on
every machine. Its art, `examples/art/moth.png` and
`examples/art/moon.png`, is drawn for this repository and is under the
repository's license, and a comment beside each declaration says where
its picture comes from. It runs headless with its `.drive`, `.draws`
and `.out`.

### The spike

After slice 2, when pictures are drawn and `capture` shows them:
twelve sessions, Sonnet and Haiku, two each on three tasks (a character
that walks and turns, from a sheet given as a file; a pickup drawn as a
written picture that a character must touch; an existing program of
circles to be given pictures), from the reference, the skill and the
commands alone, read by hand. It is a spike under the repository's
rules for evals: it decides nothing by a number, and may inform. Stated
before the first session, what each result would lead to:

- in most sessions an image is declared in a system or a path is given
  at the draw, and again after the error: the errors' wording first; if
  that does not help, the form of the declaration goes back to review
  before the documents are finished, and slices 1 and 2 change with it;
- pictures placed off by half their size: the reference's wording and
  example on the top left corner;
- frames off by one, or cells measured wrong, after the compiler's
  errors and `midnight assets`: the errors and the command's output;
- `flip_x` not found, or a negative scale tried again after its error:
  where the mirror is written;
- a written picture whose digits name the wrong colors, or that takes
  more than two rounds of `check` to be well-formed: the guidance on
  size first, then the form of the rows;
- members an agent reaches for that a sheet or an image lacks (`next`,
  `length`): the list of names the compiler answers, and a note for the
  spec that adds runs of frames.

What it finds goes into the diagnostics, the reference and the skill,
and into the Outcome.

### Slices

1. **The language, with no device.** The PNG reader and its tests;
   paths, for images and `require`; the three forms, `Image` and a
   sheet's name in the checker, with every diagnostic; the typed
   program; the emitter, the table and `mn_draw_image`; the record;
   `draws`, `query`, `to_s` and JSON; several cores and GPU systems on
   the CPU; `midnight assets` and `--write`; the harness; the tests
   that need no device; the reference's sections on declaring and
   drawing, and the skill's lines on images and `midnight assets`.
   Until slice 2 the picture of a frame has no images: the window's
   shader shows no record of a kind it does not know. The commands of
   slice 1's programs have no `capture` until then.
2. **Pixels.** `midnight_images.bin` and `MN_IMAGES`, the bindings and
   the shader, GPU blocks on the device, the reference renderer and
   its unit tests, `window_drawn`, `window_shown`, `image_stats`,
   `images_are_the_files`.
3. **The spike**, and what it changes.
4. **Documents.** The reference (below), `docs/png.md`,
   `docs/json.md`, `docs/building.md`, `docs/running.md` (`midnight
   assets`, and what the runtime does with texels), the costs measured
   with nothing else running, `examples/moths.12am`, the README, the
   skill as the spike left it, `evals/README.md`, the repository's
   `CLAUDE.md` (`docs/png.md` in "What defines what";
   `midnight_images.bin`, `MN_IMAGES` and `#embed` in its rules, where
   `midnight_window.c` is said to be C11; `tests/png` and
   `tests/assets` in its tests), `tests/COVERAGE.md`, the Outcome,
   which also says whether the macOS outputs of the flake build a
   program with an image.

The reference changes in these places beside a new section "Images"
after "Text":

- the version;
- "Lexical structure": the built-in names;
- "Types": `Image`;
- "Expressions": comparison of images; "Control flow": the value of a
  `case`;
- "Constants": the three forms, and the constant expressions of images
  and sheets;
- "The window": the built-in names, and which of them give a program a
  window;
- "Drawing": its table gains `Draw.image`; its members are "six
  statements"; its rules on arguments and sizes name the image and
  `scale`; the names of other toolkits it explains;
- "The tests, exactly", whose **Nothing.** names images; "Color";
- "Draws in a GPU system", "What a GPU block may contain", "What a GPU
  system may touch" and "Runtime errors in a GPU system";
- "Draws on several cores"; "Determinism of a frame"; "The command
  `draws`"; `query` in "Driving";
- "Built-ins": `to_s`;
- "Programs and files": the rules for paths, and that a program is its
  source files and the files they declare;
- "What things cost": an `Image` among an entity's bytes; "What a GPU
  system costs": an index that can fail; "What several cores cost":
  the weights; "What a draw costs";
- "Errors"; "Grammar", whose last paragraph counts the draws;
- "Not in 0.12": under drawing, images and sprites are no longer left
  out and the non-goals above are.

## Acceptance criteria

1. Every program of `tests/run` and `examples` with a `.draws` file
   gives exactly those answers, in release and debug, and in JSON with
   the same content; the image programs above among them.
2. Every diagnostic of this spec has a `tests/fail` program, and the
   run-time error its `tests/panic` programs.
3. `png_suite_is_read` passes: every valid PngSuite file gives the size
   and texels of `expected.txt`, every corrupt one exactly its message,
   and no damaged input makes the compiler panic.
4. A `require` or an image path that breaks a rule of "Paths" is the
   compile error its table gives; every `require` of the suite compiles
   as before; and the function that matches a part against a
   directory's entries passes its unit tests, so that a name of another
   case is not a match on any file system.
5. `thread_programs` gives the same output and `draws` answers for
   every setting for the programs that draw images from a split loop,
   and `midnight loops` gives no reason for an image.
6. `assets_match_expected` and `assets_writes_its_images` pass.
7. The reference renderer's unit tests for images pass.
8. `window_drawn` passes with the image programs at each scale on
   `llvmpipe` in the build sandbox, on the hardware device and on RADV
   (chosen with `VK_DRIVER_FILES`), `window_shown` passes with them,
   and `gpu_programs` passes with the GPU system that draws images.
9. `image_stats` passes on `llvmpipe` and on the hardware device.
10. "What a draw costs" gives the cost of images on the three devices,
    measured by `bench/draws.sh`.
11. `examples/moths.12am`, started by hand on the development machine's
    desktop in release and in debug, shows its moths flapping and
    turning toward the lamp.
12. The spike's twelve sessions are read and the Outcome says what each
    stated result came to and what was changed.
13. Every suite, the GPU and window modes and `nix flake check` pass.
14. The reference says 0.12 and `midnight --version` prints `midnight
    0.12.0`; `docs/png.md` is there; `docs/json.md` has the image shape
    and value and the `asset` lines; the README's and the skill's
    commands work as written; `tests/COVERAGE.md` has every construct,
    diagnostic and error of this spec.
15. Every `.out`, `.err`, `.plan`, `.loops`, `.split`, `.stats`,
    `.frames`, `.draws`, `.queries`, `.session` and `.json` file that
    existed in 0.11.0 is unchanged, but for
    `tests/fail/draw_not_shapes.err`, the program that declared a
    struct named `Image` (`tests/run/compact_lists.12am`), renamed
    there, with its expected file, and the version in `.json` files.
    For every program that existed in 0.11.0 and declares no image,
    `midnight emit-c` and `midnight emit-wgsl` give what the compiler
    built from 0.11.0 gives, the version string in the generated C set
    aside; the Outcome gives the count.
16. A debug build of every program that draws images ends with no
    sanitizer report and no live buffer, headless and inside the
    compositor of the window tests.
17. No test, check or app of this spec opens a window on the desktop it
    is run from.

## Decisions

- **Pictures are declared, and read when a program is built.** A
  program's output then never depends on what is on a disk when it
  runs, the compiler can check a program against its pictures, and
  there is no state in which a picture is not there.
- **A declaration is a constant whose value is a built-in form.** The
  grammar does not change, a name for a frame is one more constant,
  and it is the shape of the declarations agents have written most.
  The form is allowed nowhere else, so the files a program reads are
  found by reading its top level.
- **The form names the kind.** `Image`, `Sheet`, and later the forms
  of sound and models, say what the file is to the program, whatever
  its name.
- **Paths are relative to the file that writes them and never leave
  the program's directory.** A program is then a directory that can be
  copied, built in a sandbox and trusted not to read anything else,
  and the same rule will hold for a file that names other files.
  `require` takes the rule too: one rule for every path in source.
- **Names match exactly, in characters every system can hold.** Letter
  case and the form a file system stores a name in never decide whether
  a program builds.
- **An image is a value; a sheet is a name.** A component holds the
  picture of its entity. A sheet's every use then has a sheet the
  compiler knows, so an index is checked against a number it knows.
- **A sheet reads as a list does.** Frames from 0, left to right, then
  down, with one index; a negative index from the end; `first` and
  `last`. What is written like a list means what a list means.
- **A sheet is given its cell's size, and must divide.** A count of
  cells divides into fractions, and a cell size that leaves pixels over
  fails without a word elsewhere.
- **`Draw.image(x, y, image, scale, color)`.** The shape of
  `Draw.text`, so there is one order to learn, and no two arguments of
  one type stand side by side but `x` and `y`.
- **The top left corner, whatever else is given.** It is where
  `Draw.rect` and `Draw.text` put theirs, a picture takes the place of
  the rectangle that stood in for it, and nothing an argument adds
  later moves it.
- **A scale, the size of a texel.** The record then holds what was
  written and nothing is divided anywhere, on the CPU or on a device;
  pixel art wants whole texels, which a scale says and a width hides.
- **A color last, always.** Every draw ends with one, a tint and a
  fade are the commonest things done to a picture, and an argument
  added later would change every draw written.
- **The frame and the mirror are in the value.** The statement's
  arguments stay as they are when an orientation is added.
- **Texels are rectangles under 0009's rule.** Pictures are then exact
  on every device by the same definition and the same tests as shapes
  and text.
- **Every PNG is read, and the places readers differ are stated.** A
  file from any tool works as it is; a 16-bit sample and a transparent
  color have one answer.
- **Colors are the file's bytes.** They are sRGB as a `Color` is, and
  no machine's color management enters a program.
- **How a PNG is read is a document of its own.** A program's author
  needs a paragraph of it; whoever writes or checks the reader needs
  all of it.
- **Written pictures state their size.** A row too many or too few is
  then an error and not another picture.
- **Hexadecimal digits into the program's own colors**, as PICO-8 and
  Pyxel write pictures, with no palette built into the language.
- **One buffer of texels, with a limit every device takes.** The
  limits a program meets are three numbers, the same on every machine;
  the device holds the texels and nothing more; and the limit can only
  rise.
- **In the executable today, and not promised.** It costs a build and
  a start nothing; where the pixels are kept stays the compiler's to
  change.
- **The command is `midnight assets`.** Sounds and models join the
  same list.

## Measured in planning

From the planning probes, on the development machine: an RTX 5090
(NVIDIA's driver), RADV on an integrated Radeon, and `llvmpipe`.

- **The rule.** The probe drew an image to a width and a height: `w /
  W` and `h / H`, divided exactly, stood where `scale` stands in this
  spec, and the bounds, the tests and the color were the ones above.
  Its shader took an estimate, at most four steps and then a binary
  search, which never ran. Over 2,016 opaque pictures a device (targets
  at window scales of 1, 1.25, 1.5 and 2; images from 1 by 1 to 2,048
  by 2,048 and rectangles of a sheet; positions whole, half and
  anywhere near the window; texels from a fraction of a pixel to many)
  it gave byte for byte the rule computed on the CPU on all three
  devices; counters of covered pixels agreed in the translucent
  pictures too. One `scale` read from the record, images up to 8,192
  texels, and scales so small that several columns share a bound, where
  the search is needed, are held to the rule by `window_drawn`.
- Without the steps after the estimate RADV differed in 316 pictures.
  Finding the texel as `floor((px - x) * W / w)` differed in 233
  pictures on the 5090 and 354 on RADV, most at whole positions and
  sizes: hence bounds and tests. A size of a texel computed with the
  device's division differed in 31 and 43: hence nothing is divided.
- Without the barrier the 5090 and RADV fuse a product and a sum. The
  NVIDIA compiler also regroups a chain of sums that the barrier on
  products does not guard; with the barrier on sums too, the pictures
  are the same and the cost is not measurable.
- Computed in 32-bit steps as the rule states them, an image 8,192
  texels wide at `x` = 8000.3 with a scale of 0.00005 has about ten
  columns to each step of its bounds, and for the pixel center at
  8000.5 the estimate is five columns from the texel the rule names:
  hence the search after the steps.
- Mirrors as index arithmetic, and the color `(t * k + 127) / 255` in
  whole numbers, were identical on the three devices and equal to the
  CPU. Translucent results kept 0009's allowance, the 5090 exact.
- A record written by a compute shader rendered as the same record
  written by the CPU.
- Texels in a storage buffer gave the same pictures as texels in a
  texture, at the same cost within a few percent for small images.
  Over an image that fills the window the 5090 took 0.15 ms from a
  storage buffer where it took 0.11 from a texture at 10,000 images,
  and 0.91 against 0.47 at 100,000; the other two devices did not
  differ.
- 33,554,432 texels are 128 MiB, the largest storage buffer binding
  WebGPU grants by default and the smallest Vulkan lets a device have.
  A buffer of that size was made, filled, read back right and bound
  whole on each of the three devices, in 18 to 64 ms; `llvmpipe` binds
  exactly that much and no more.
- What a frame of 1920 by 1080 costs, records uploaded each frame,
  opaque images of 16 by 16 pixels with a white color, texels in a
  texture:

  | | RTX 5090 | RADV | `llvmpipe` |
  |---|---|---|---|
  | 10,000 images | 0.08 ms | 1.7 ms | 6.4 ms |
  | 100,000 images | 0.38 ms | 17 ms | 54 ms |
  | 100,000 rectangles of 16 by 16 | 0.34 ms | 5.7 ms | 39 ms |

- Copying texels to a device took 150 to 320 ms a gigabyte.
- "Even texels" is 0011's arithmetic for the font's pixels applied to
  texels; the renderer's unit tests hold it.
- **The reader.** 807 lines of Rust without tests. All 162 valid
  PngSuite files gave the pixels ImageMagick, which reads through
  libpng, gives with `-depth 8`, and all 14 corrupt ones an error; 12
  million damaged inputs gave no panic. An image of 64 by 64 decodes in
  15 to 35 µs, one of 4,096 by 4,096 in 120 ms.
- **Carrying texels.** With `#embed`, 256 MB added 0.5 s to a build,
  and 1,000 images of 64 by 64 about 0.03 s. A program with 256 MB
  embedded had 1.8 MB resident when it started. One executable holds
  about 2 GiB before the linker fails.
- **Written pictures.** Sonnet and Haiku, one answer each with no
  tools, 319 answers, square pictures written alone, a letter for each
  color from a legend of eleven: every computed picture right at 16 by
  16 (60 of 60), 55 of 60 at 24, and with plain rows at 32 and 48, 15
  of 20 for Sonnet and 3 of 20 for Haiku. Drawn pictures (a heart, a
  key, a slime, a tree) were well-formed 7 times of 8 at 16 and 4 of 8
  at 32. Of 67 failures, 63 had a row count or a row length a size
  check reports, and 34 had the wrong number of rows. Told which row
  was wrong, a model fixed 4 of 15 miscounts in one step. Digits into
  a list of the program's colors, pictures that are not square and
  pictures inside a program were not measured: the spike's second task
  is their first test.
