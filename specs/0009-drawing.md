# 0009 — Drawing: shapes from systems, and the frame as a list

- **Status:** approved
- **Depends on:** 0008

## Goal

A Midnight program can open a window and paint its background, and that
is all a frame holds. After this spec a system draws:

```ruby
component Ball
  x: Float32
  y: Float32
  dx: Float32
  dy: Float32
end

system move
  each Ball do |ball|
    ball.x += ball.dx
    ball.y += ball.dy
    ball.dx = -ball.dx if ball.x < 4.0 || ball.x > 316.0
    ball.dy = -ball.dy if ball.y < 4.0 || ball.y > 196.0
  end
end

system show
  Draw.rect(0, 190, 320, 10, Color(40, 40, 60))
  each Ball do |ball|
    Draw.circle(ball.x, ball.y, 4, Color(255, 200, 0))
  end
end

Window.background = Color(16, 16, 32)
Window.open("Balls", 320, 200)
spawn Ball(20, 30, 2, 1.5)
spawn Ball(200, 90, -1, 2)
World.run
```

Drawing is a statement. `Draw.rect`, `Draw.circle`, `Draw.line` and
`Draw.triangle` each add one shape to the frame the tick ends with, and
what is drawn later covers what was drawn earlier: systems in their
order, statements in theirs. Nothing is drawn that the source does not
draw.

The same statement works in a GPU system, where the shape is recorded on
the device from data that is already there, so a million bodies that a
GPU system moves are drawn without their positions ever coming back to
the CPU.

A frame is a list before it is a picture: the window's size, its
background, and the draws of the tick, in order. The new command `draws`
prints that list as text. It needs no device, so a program's drawing can
be checked exactly on every machine, with `MIDNIGHT_GPU=off` too, by the
test suite and by an agent that has no GPU. `capture` goes on giving the
picture.

### How this serves the principles

- **Agents write, people review.** A draw is where the source says it is,
  in the order it happens. There is no drawing state: a shape's colour
  and size are its arguments. Position first and colour last is the order
  of the same call in the toolkits these names come from.
- **The domain is in the language.** The compiler sees every draw: it
  keeps a loop that draws on the cores, turns a draw in a GPU system into
  a record written on the device, and refuses a program that draws and
  never opens a window.
- **Fast by construction.** A draw in ordinary code costs 32 bytes. A
  draw statement of a GPU system costs 32 bytes for each entity, on the
  device or on the CPU, and from the device no transfer.
- **Observable and repeatable.** The list of draws is exact output on
  every machine, and which pixels a shape covers is a definition in the
  reference. This gives up one thing 0.7 has: a frame with a translucent
  draw is not the same bytes on every device. The list takes the frame's
  place as what is exact, the reference says how far a device may differ,
  and since no program can read a frame, no output depends on the
  difference.
- **Errors teach.** The names other toolkits use for these calls are
  recognised, and the message says what to write.

## Scope

- The built-in name `Draw` and its four statements.
- The frame as a list of draws, fixed when a tick ends; the order of
  draws; what a draw does while no window is open.
- Draws in an `each` on several cores, and draws in a GPU system.
- Which pixels each shape covers, and how a colour with an alpha below
  255 is laid over what is under it.
- The command `draws`.
- The runtime's shape pipeline on the device, for the CPU's list and for
  the records GPU blocks write; a frame texture and a surface that hold
  colour bytes as they are.
- A renderer in the test suite that makes the pixels a list of draws
  must give, which the frames of every device are held to.
- A comparison by the evals, before the pipeline is built, of this form
  of drawing with drawing components, each built as far as the list and
  `draws`; four drawing tasks; and the check **draws** of the harness,
  which grades them on what `draws` prints.
- The reference becomes Midnight 0.8 and the compiler 0.8.0.

## Non-goals

- Text, images and sprites. Each is a spec of its own.
- Outlines, rounded rectangles, ellipses, arcs, polygons of more than
  three corners, curves and paths.
- Any order but the program's in what ships: layers, sorting by a key,
  a depth buffer.
- A camera, a view transform, a logical size that differs from the
  window's, rotation of a shape as an argument.
- Smoothed edges. A pixel is covered or it is not.
- Blending other than laying a colour over what is under it.
- Drawing into anything but the window's frame, and keeping a frame's
  pixels for the next one.
- Reading a frame's pixels in a program. Nothing a program computes can
  depend on a picture.
- Commands that look at pixels: the colour at a position, a part of a
  frame, a comparison of two frames.
- Arguments of a draw given by name.
- Three dimensions.
- Code that runs for each pixel, written in Midnight.
- Lines for draws in `midnight plan`.

## Design

### The language

`Draw` becomes a built-in name and cannot be declared by a program. A
program that names it **has a window**, as one that names `Window` does.
`Draw` is not a value and has no fields. Its members are four statements:

| Statement | Draws |
| --------- | ----- |
| `Draw.rect(x, y, width, height, color)` | The rectangle whose top left corner is at `x`, `y`, `width` points wide and `height` points high. |
| `Draw.circle(x, y, radius, color)` | The disc around `x`, `y`. |
| `Draw.line(x1, y1, x2, y2, width, color)` | The line from `x1`, `y1` to `x2`, `y2`, `width` points thick, with round ends. |
| `Draw.triangle(x1, y1, x2, y2, x3, y3, color)` | The triangle with those corners, in either direction. |

- Every argument but the last is a `Float`, and an `Int` converts as
  everywhere. `color` is a `Color`.
- Positions and sizes are in **points**, from the window's top left
  corner, `x` to the right and `y` down: the space of `Mouse.x`,
  `Mouse.y`, `Window.width` and `Window.height`. A shape may lie partly
  or wholly outside the window; the part outside is not drawn.
- Each is a statement: it has no value. It is written with its
  parentheses and every argument, by position.
- A `width`, a `height` or a `radius` that is not greater than zero
  draws nothing, and so does a shape with a NaN among its numbers.
  Neither is an error.
- The numbers of a draw are kept as 32-bit `Float`s, each rounded to the
  nearest when the statement runs.

**Where.** A draw may stand wherever a statement can: in a system, a
function, top-level code, inside `each`, `on` and every loop. In a GPU
system it follows the rules of [Draws in a GPU system](#draws-in-a-gpu-system).

**The frame.** While a window is open, each draw is added to a list.
When a tick ends, the list becomes that tick's frame, with the window's
size and background as they are then, and a new list begins. So the frame
of a tick holds what was drawn since the tick before ended, in the order
it was drawn: the draws of top-level code between two ticks come first,
then those of the systems. `Window.open` begins a new list, and the frame
it shows is the background.

A draw while no window is open adds nothing to any list: its arguments
are evaluated, with everything that does, and the draw is dropped.

A program draws again every tick whatever it wants to stay on screen.
Nothing of a frame is carried into the next.

**Order.** A draw covers what was drawn before it. The order is the
order the statements run in: systems in source order, the statements of
a system in theirs, and the entities of an `each` in visiting order. A
draw is added when its arguments have been evaluated: in
`Draw.rect(f(), 0, 8, 8, c)`, what `f` draws comes before the rectangle.

The visiting order of a query is deterministic and not specified (see
"Queries"), and it changes when an entity is despawned: two shapes of one
`each` that overlap can change which is on top. What must be on top is
drawn by a later statement:

```ruby
system show
  each Position, with: Floor do |pos|
    Draw.rect(pos.x, pos.y, 16, 16, Color(60, 60, 60))
  end
  each Position, with: Player do |pos|
    Draw.circle(pos.x + 8, pos.y + 8, 6, Color(255, 255, 255))
  end
end
```

**A program that draws opens a window.** A program that has a `Draw`
statement and no `Window.open` anywhere is a compile error at its first
draw:

```
error: nothing opens a window for `Draw.rect` to draw in
  --> balls.12am:17:3
   |
17 |   Draw.rect(0, 190, 320, 10, Color(40, 40, 60))
   |   ^^^^^^^^^ open one with `Window.open(title, width, height)`
```

### Which pixels a shape covers

A frame has `Window.pixel_width` by `Window.pixel_height` pixels. The
pixel in column `i` and row `j`, counted from 0 at the top left, has its
**centre** at the point

```
((i + 0.5) * (width / pixel_width), (j + 0.5) * (height / pixel_height))
```

with `width` and `height` the window's size in points, each division and
each product one 32-bit operation: at a scale of 1 that is
`(i + 0.5, j + 0.5)`. **A shape covers the pixels whose centres are
inside it.** A pixel is covered or it is not; no edge is smoothed.

- **Rectangle.** A centre `(px, py)` is inside when
  `x <= px < x + width` and `y <= py < y + height`.
- **Circle.** Inside when the centre is nearer to `x`, `y` than `radius`.
- **Triangle.** Inside when the centre is on the inner side of all three
  edges. A centre exactly on an edge is inside when that edge is a left
  edge, one with the triangle to its right, or a top edge, one that is
  level with the triangle below it. An edge's test is computed from its
  two ends in one order, whichever triangle it belongs to, so two
  triangles that share an edge, with the same numbers for its two ends,
  cover each pixel along it once. A triangle with no area covers
  nothing.
- **Line.** The pixels of the rectangle that is `width` thick and runs
  along the line from end to end, a centre exactly on one of its two
  long edges counting for the left or the top one of the two, and the
  pixels of a circle of radius `width / 2` around each end. A line whose
  ends are the same point is that circle.

So at a scale of 1 `Draw.rect(10, 20, 30, 5, c)` covers exactly columns
10 to 39 of rows 20 to 24. Two rectangles of which the second's `x` is
the 32-bit sum of the first's `x` and `width`, as with whole numbers and
halves it always is, neither overlap nor leave a gap. And
`Draw.line(0, 10, 100, 10, 1, c)` is one row of pixels, columns 0 to 99
of row 9, whose centres lie at 9.5 on the upper of its two long edges.

**The tests, exactly.** Every step is one 32-bit `Float` operation,
rounded on its own, on the numbers as the draw keeps them. `(px, py)` is
the centre.

- **Nothing.** A draw covers no pixel when one of its numbers is a NaN;
  when a rectangle's `width` or `height`, a circle's `radius` or a
  line's `width` is not greater than zero; and when a triangle's `w`,
  below, is zero. The tests that follow are for the others.
- **Rectangle:** `x <= px`, `px < x + width`, `y <= py` and
  `py < y + height`.
- **Circle:** with `dx = px - x` and `dy = py - y`:
  `dx * dx + dy * dy < radius * radius`.
- **Triangle:** `w = (x2 - x1) * (y3 - y1) - (y2 - y1) * (x3 - x1)`.
  When `w` is negative, corners 2 and 3 change places. Then for each
  edge from a corner `a` to the next corner `b` (1 to 2, 2 to 3, 3 to
  1), with `l` the end of the edge that has the smaller `x`, or the
  smaller `y` when the `x`s are equal, and `h` the other end:
  `e = (hx - lx) * (py - ly) - (hy - ly) * (px - lx)`, negated when `l`
  is `b`. The centre is inside when every `e` is positive, or zero on an
  edge with `by < ay` (a left edge) or with `by == ay` and `bx > ax` (a
  top edge).
- **Line:** with `d` the vector from the first end to the second and `q`
  the vector from the first end to the centre:
  `t = q.x * d.x + q.y * d.y`, `s = q.x * d.y - q.y * d.x` and
  `l = d.x * d.x + d.y * d.y`. The centre is in the body when `l > 0`,
  `0 <= t`, `t <= l`, and `(4 * s) * s` is less than
  `(width * width) * l`, or equal to it with `s > 0` and `d` pointing up
  or level to the right (`d.y < 0`, or `d.y == 0` and `d.x > 0`), or
  equal to it with `s < 0` and `d` pointing down or level to the left
  (`d.y > 0`, or `d.y == 0` and `d.x < 0`). It is in an end when, with
  `h = width * 0.5` and `e` the vector from that end to the centre,
  `e.x * e.x + e.y * e.y < h * h`.

### Colour

- A draw whose colour has an `a` of 255 gives each pixel it covers its
  `r`, `g` and `b`.
- With an `a` from 1 to 254 the colour is laid over what the pixel
  holds. For each of red, green and blue, with `c` the byte of the
  colour and `d` the byte the pixel holds, the pixel becomes

  ```
  (a * c + (255 - a) * d) / 255, rounded to the nearest whole number
  ```

  The arithmetic is on the bytes as they are:
  `Color(255, 255, 255, 128)` over black is 128.
- An `a` of 0 changes nothing.
- A frame stays opaque: `capture` writes an alpha of 255 for every
  pixel.

### Draws in a GPU system

```ruby
gpu system show
  each Body do |body|
    Draw.circle(body.x, body.y, 1.5, Color(255, 255, 255))
  end
end
```

A draw may stand directly in a GPU block: in the block itself, in an arm
of an `if`, `unless` or `case` inside it, or before a modifier `if` or
`unless`. It is a compile error in a loop inside the block, in the
nested `each`, and in a function a GPU system calls, with the hint to
draw from the block itself. A run thus makes each of the block's draws
once at most.

- The arguments are computed by the rules of the block: 32-bit, and under
  the strict rules in a strict GPU system. A `Color` is plain, and may be
  built in the block or read from a component, a constant or a resource
  field.
- The draws of a GPU block take the place in the frame that the draws of
  an ordinary `each` would: entity after entity in visiting order, the
  draws of one entity in the order its run made them. A draw that a run
  does not reach, in an arm not taken or after a `next`, is not made.
- On a device the shapes are recorded in GPU memory and rendered from
  there: a draw moves nothing between CPU and GPU memory, and
  `midnight plan` has no line for it. Where the block runs on the CPU it
  adds the same draws to the list.
- A draw is not one of the things a block assigns. A block that only
  draws writes no component.

### Several cores

A draw gives an `each` no reason to stay on one core. Rule 5 of
"Independent loops" becomes "queued changes, events and draws": the
draws of a loop that is split are collected for each piece and joined in
visiting order when the loop ends, which is the order one thread makes
them in. A draw weighs what an `emit` does: its arguments and 8.
`midnight loops` has no reason for a draw.

### Determinism

The window's "Determinism" says of a frame:

- **The list of draws** is part of what a program computes, and follows
  the rules of everything else it computes: the same on every run and
  for every number of threads, and the same on every machine wherever
  the numbers drawn are. A number that a GPU system that is not strict
  computed, or a `Math` function, can differ between machines as it does
  in a program's other output.
- **Which pixels a shape covers** is defined above. A device may differ
  from it only at a pixel for which the two sides of one of the tests,
  computed as above, are equal or one 32-bit step apart, and for a shape
  with a number that is infinite, or below about 1.18e-38 in magnitude
  and not zero, or whose test overflows. On the devices measured, every
  pixel was the one the same arithmetic gives on the CPU.
- **A colour with an `a` of 255** gives exactly its bytes on every
  device.
- **A colour with an `a` from 1 to 254** may be rounded differently by a
  device: each of a pixel's three bytes may differ from the rule above
  by one for each such draw over the pixel since the last draw with an
  `a` of 255 covered it.
- **A program cannot read a frame.** What is drawn never reaches what a
  program computes, so a difference between two devices' pictures is
  never a difference in a program's output.

### The command `draws`

| Command | Meaning |
| ------- | ------- |
| `draws` | Print the last frame as text: its size, its background and its draws, in order. |

It is answered on standard error, like every command, and it is the one
command whose answer has more than one line: one for the frame, one for
the background and one for each draw. Every line begins
`midnight: drive: `, and the lines after the first go on with two
spaces:

```
midnight: drive: draws: 3 ticks, 320x200, 3 draws
midnight: drive:   Window.background = Color(16, 16, 32, 255)
midnight: drive:   Draw.rect(0.0, 190.0, 320.0, 10.0, Color(40, 40, 60, 255))
midnight: drive:   Draw.circle(26.0, 34.5, 4.0, Color(255, 200, 0, 255))
midnight: drive:   Draw.circle(197.0, 96.0, 4.0, Color(255, 200, 0, 255))
```

- The last frame is the one `capture` would write. The first line gives
  the value `Time.tick` had when the frame was made, the window's size in
  points, and the number of draws listed: `1 tick` and `1 draw` for one.
- The background is printed with the `a` the program gave it, which
  changes nothing in the picture.
- A draw is printed as the statement that made it, with the numbers it
  keeps: each as the shortest text that reads back as the same 32-bit
  `Float`, in the form `to_s` gives a `Float`, and the colour with its
  four bytes. A draw that covers nothing is printed like any other.
  `draws` prints the 32-bit number; `puts` of a `Float32` field holding
  the same value prints it as the 64-bit `Float` it is read as.
- `draws` needs no device and works with `MIDNIGHT_GPU=off`. Where the
  draws of a GPU block are in GPU memory they are brought back for it,
  behind one wait.
- It takes no argument, and it is an error, as for `capture`, when no
  window is open: `can't list the draws: no window is open`.
- It writes no file.

### What a draw costs

For "What things cost":

- A draw in ordinary code appends 32 bytes to the frame's list. A window
  that is shown sends the list to the device once, when the tick ends.
- Each draw statement of a GPU block has a record of 32 bytes among
  every run's records, used or not: a block with three draws keeps 96
  bytes for each entity. On a device they are written there and nothing
  travels.
- A shape costs the device a test for every pixel of the smallest
  upright rectangle that holds it, covered or not.
- A headless window renders nothing until a frame is captured. A window
  that is hidden renders nothing.
- A frame is handed to the device when its tick ends. The next frame is
  not handed over until the device has finished the one before, so a
  program whose frames take the device longer than a tick runs slow, as
  one whose systems do. `World.tick` returns without waiting for its own
  frame: a program that times its ticks with `clock` does not measure
  it.

What follows from this:

- **Draw from the system that has the data.** A draw in an ordinary
  system of a component that a GPU system keeps in GPU memory brings the
  array back in every tick: `midnight plan` lists the download. The same
  draw in the GPU system moves nothing.
- **Keep the draw statements of a GPU block few.** Each costs 32 bytes
  for every entity, reached or not.
- **Build a GPU block's colours from numbers the compiler can bound.** A
  `Color` built from a number it cannot bound is a store that can fail,
  and makes the block wait as any operation that can fail does: `clamp`
  each number to 0 and 255, or read the colour from a component, a
  constant or a resource field.
- **`draws` is for looking and for tests.** On a device it brings a GPU
  block's records back behind a wait.

The numbers under "Measured in planning" go into the reference, measured
again on the implementation.

### The compiler

- `Draw` is a built-in name that gives a program a window. It exists in
  the checker alone, as `Keyboard` does, and declares nothing.
- A draw is a statement of its own kind in the typed program: the shape
  and its arguments, converted to the parameter types. It is matched by
  its form, `Draw.` and a name directly followed by `(`, before
  expressions are checked, as `Window.open` is.
- The effect analysis counts a draw as it counts an `emit`: allowed in an
  independent `each`, with the weight of its arguments plus 8.
- In a GPU block a draw lowers to the stores of one record. A run begins
  by storing kind 0 in each of its records, in the shader and in the C
  form, so a draw the run does not reach is a draw of nothing, whatever
  the record held before. The block's description gains the number of
  its draw statements, and a block that draws binds one more buffer on
  the device. A block whose records for a table, or whose storage
  buffers with the one for records, are more than the device binds runs
  on the CPU, as any block that does not fit does.
- Diagnostics, each one line with its hint in the label:
  - a draw with the wrong number of arguments, or an argument of the
    wrong type; the label gives the statement's form, such as
    `` `Draw.circle(x, y, radius, color)` ``;
  - an argument given by name: the label gives the statement's form and
    says its arguments go by position;
  - a `Color` as the first argument:

    ```
    error: `Draw.circle` takes its colour last
       |               ^^^^^^^^^^^^^^ write `Draw.circle(x, y, radius, color)`
    ```

  - an unknown member of `Draw`, with the statement to write in the
    label: `rectangle`, `square` and `box` name `Draw.rect`, `ellipse`
    and `disc` name `Draw.circle`, `point` and `pixel` name
    `Draw.rect(x, y, 1, 1, color)`, and any other gets the nearest of the
    four:

    ```
    error: `Draw` has no `rectangle`
       |        ^^^^^^^^^ write `Draw.rect(x, y, width, height, color)`
    ```

  - an unknown type named `Rect`, `Rectangle`, `Circle`, `Line` or
    `Triangle` that is constructed or spawned:

    ```
    error: unknown type `Circle`
       |         ^^^^^^ a circle is drawn with `Draw.circle(x, y, radius, color)`
    ```

  - `Draw.text`, `Draw.sprite` and `Draw.image`:
    `` `Draw.text` is not in 0.8, which draws shapes only ``;
  - a draw used as a value, `Draw` used as a value, `Draw.rect` without
    its arguments, and a declaration named `Draw`;
  - a program that draws and has no `Window.open`, as shown above;
  - a draw in a loop inside a GPU block, in its nested `each`, or in a
    function a GPU system calls, with the usual note naming the GPU
    system and the call:

    ```
    error: a draw can't stand in a loop inside a GPU block
       |   ^^^^^^^^^ draw from the block itself: each run makes a draw once at most
    ```

  - a call of a function the program does not define whose name is
    `rect`, `circle`, `line` or `triangle`, or one of those after
    `draw_` or `fill_`:

    ```
    error: unknown function `draw_circle`
       |   ^^^^^^^^^^^ a circle is drawn with `Draw.circle(x, y, radius, color)`
    ```

- A program without `Draw` is built as in 0.7: its generated C and its
  shaders are the same text.

### The runtime

Words: a **record** is the 32 bytes of one draw. The list of a frame is
a sequence of **spans**, each an array of records. To **render** a frame
is to make its pixels on the device; a program draws, the runtime
renders.

**`midnight_rt.h`** gains a section "Drawing":

- `MnDraw`, the record: the kind as a 32-bit word (0 nothing, 1
  rectangle, 2 circle, 3 line, 4 triangle), six 32-bit floats in the
  order of the statement's arguments, unused ones zero, and the four
  bytes of the colour. All-zero bytes are a draw of nothing. Generated
  code asserts the layout, and it is the layout of the records a shader
  writes.
- The function generated code calls for a draw, which returns the record
  to fill. Generated code evaluates the arguments first, then asks for
  the record and fills it at once. The function is on the list of
  "Several threads": in a piece it keeps the record with the piece.
  While no window is open it returns a record that belongs to the
  calling thread and that nothing reads.
- In the description of a GPU block, the number of its draw statements.
  The record of row `r` and draw statement `s` of a block is record
  `r * statements + s` of the block's records for that table. `MnGpuRun`
  gains the records of the rows it is given, and the C form of a block
  writes them as well as its rows.

**`midnight_window.h`**: `MnFrame` gains the frame's spans and a number
that goes up with every frame. `midnight_window.c` renders a frame again
whenever the texture holds one with another number.

**`midnight_gpu.h`** gains what the window needs to find a block's
records for a table, and what `draws` needs to bring them back.

**In `midnight_rt.c`**, under `MN_WINDOW`, with libc alone:

- **The list.** The draws since the last frame, as spans in the order
  they were made: an array of records in CPU memory, or the records of
  one GPU block for one table, with the number of rows the table had
  when the block ran. The pieces of a split loop are joined as events
  are. A GPU block that runs on the CPU fills an array of
  `rows * statements` records in place. A GPU block over several tables
  gives one span for each, in the order its query visits them.
- **When a tick ends**, the list becomes the frame's with the window's
  size and background. A frame is its size, its background and its
  spans. The span of a GPU block is good until the block runs again,
  which no command comes before.
- **`draws`** prints the frame's spans in order, leaving out records of
  kind 0.
- While no window is open no draw is kept, and a GPU block's records
  make no span.
- **`stats`.** In `MIDNIGHT_GPU=stats`, an array of CPU records sent to
  the device counts as one upload, a frame as one submission, a wait for
  the frame before as one wait when the tick has not already waited for
  the device, and what `draws` brings back as one download for each
  span, behind one wait. A headless window that is asked for no frame
  and no draws adds nothing to a tick's line.

**In `midnight_gpu.c`**: for each GPU block that draws, and each table,
a buffer of the block's records, written by the block's shader, which
`midnight_window.c` renders from; and the copy back to CPU memory that
`draws` asks for.

**In `midnight_window.c`**:

- **The frame texture is `RGBA8Unorm`** and holds colour bytes as they
  are. The background is cleared as each byte divided by 255.
- **The surface** is configured with the first of `RGBA8Unorm` and
  `BGRA8Unorm` that it lists, and the pass that shows a frame copies
  each texel as it is, so a shown window gets the frame's bytes
  unchanged. A window whose surface lists neither cannot be shown, with
  the reason `DEVICE can't draw to the desktop`.
- **One pipeline renders every shape.** Each record is an instance. The
  vertex stage makes the smallest upright rectangle of pixels that holds
  the shape, one pixel larger on every side and cut to the frame before
  it is turned into the device's coordinates, so a shape of any size
  gives finite corners; a record with a NaN among its numbers, told from
  its bits, or with a `width`, `height` or `radius` that is not greater
  than zero, gets no pixels. A triangle's `w` is computed in the
  fragment stage, behind the barrier as every product there. The record
  is passed on unchanged. The fragment stage takes the pixel's
  centre from its position, turns it into points by multiplying each
  coordinate by a number the CPU computed (`width / pixel_width` and
  `height / pixel_height`, each one 32-bit division), makes the tests of
  "Which pixels a shape covers" operation for operation, and discards
  the pixel when it is outside. Every product in the fragment stage, the
  one that turns the position into points included, passes through the
  barrier of strict GPU systems (an exclusive-or of its bits with a word
  that is zero and known only when the shader runs) before it is added
  to, subtracted from or compared with anything, so a device rounds each
  operation on its own. The test uses no value the device interpolates,
  no square root and no division. The rasterizer's own edges, lines,
  samplers and multisampling take no part in which pixels are covered.
- **Colour** is the device's blending of the colour's bytes over the
  frame's, by the colour's `a`, with the frame's own alpha left at 255.
- **A frame's spans are rendered in order**: an array of CPU records is
  sent to a buffer and rendered, in parts when it is larger than the
  largest buffer, and a block's records are rendered from where the
  block wrote them. Nothing is sent to a buffer that commands already
  recorded for the frame read.
- **When.** A shown window renders each frame when its tick ends, having
  waited for the device to finish the frame before. It renders nothing
  while hidden. A `capture` first renders the last frame when it has not
  been rendered: in a headless window, and in a shown one that was
  hidden when the tick ended.
- Everything made for rendering is released when the program ends: a
  debug build of a program that draws ends with no sanitizer report.

`midnight_rt.c` keeps the list and never renders; `midnight_window.c`
renders and never decides what is in a frame.

### The reference renderer

The test suite holds a renderer of its own, in Rust: from the text of a
`draws` answer and a pixel size it makes the frame's pixels by "Which
pixels a shape covers" and "Colour", in 32-bit arithmetic, and for each
pixel the number of draws with an `a` from 1 to 254 that cover it since
the last draw with an `a` of 255 did. A frame from a device **agrees**
with it when every pixel's three bytes are within that number of the
renderer's: exactly, where the number is 0.

It is test code. It states the rules a second time, apart from the
runtime's shaders, so that each is a check on the other.

### The evals

The evals do two things in this spec, both once the language and `draws`
work on the CPU and before anything renders on a device: an agent of the
evals has no device, and `draws` needs none. They compare the
**statement form** of drawing, which is this spec's, with a **component
form**, and they hold the reference's new section to the thirteen tasks
of 0.7.

**The component form.** On a branch, `0009-components`, made from the
commit of `main` that has this slice's work on `main`, and never merged,
a program draws by giving entities built-in components, and the runtime
draws every entity that has one:

```ruby
component Ball
  dx: Float32
  dy: Float32
end

system move
  each Circle, Ball do |circle, ball|
    circle.x += ball.dx
    circle.y += ball.dy
    ball.dx = -ball.dx if circle.x < 4.0 || circle.x > 316.0
    ball.dy = -ball.dy if circle.y < 4.0 || circle.y > 196.0
  end
end

Window.background = Color(16, 16, 32)
Window.open("Balls", 320, 200)
spawn Rect(0, 190, 320, 10, Color(40, 40, 60))
spawn Circle(20, 30, 4, Color(255, 200, 0)), Ball(2, 1.5)
spawn Circle(200, 90, 4, Color(255, 200, 0)), Ball(-1, 2)
World.run
```

- `Rect`, `Circle`, `Line` and `Triangle` are built-in components,
  declared as Midnight source: the numbers of the statement of that
  name, in its order, each a `Float32`; then `color: Color`; then
  `layer: Int32 = 0`. They are components like any other: spawned,
  added, removed, read and assigned, and terms of queries. The branch
  has no `Draw`.
- When a tick ends while a window is open, the frame's list is one draw
  for each of those components of each living entity: by `layer`, lowest
  first; within a layer by the entity's index, lowest first, the index
  that `to_s` shows; for one entity `Rect`, `Circle`, `Line`,
  `Triangle`. A new entity may take the index of one that was despawned,
  so the order of spawning is the order of drawing only in a program
  that despawns nothing. The branch's "Drawing" says so in these words.
- `draws` prints each as its constructor with every field by position:
  `Circle(26.0, 34.5, 4.0, Color(255, 200, 0, 255), 0)`.
- A program that names one of the four has a window. Its diagnostics
  are those of the statement form, worded for the components:

  ```
  error: nothing opens a window for `Circle` to be drawn in
     |   ^^^^^^ open one with `Window.open(title, width, height)`
  error: unknown function `draw_circle`
     |   ^^^^^^^^^^^ a circle is drawn by an entity that has a `Circle`: `spawn Circle(x, y, radius, color)`
  error: unknown name `Draw`
     |   ^^^^ a shape is drawn by an entity that has a `Rect`, a `Circle`, a `Line` or a `Triangle`
  ```

- The list is made on the CPU when a tick ends, and the branch's
  `midnight plan` is as slice 1 left it. No agent of the evals has a
  device, and none of the four tasks needs a GPU system.
- "Drawing" is written for the components with the same examples in its
  form and within a tenth of `main`'s length in lines. "Which pixels a
  shape covers", "Colour", the command `draws` but for the lines it
  prints, and everything of the window are the same text. The README's
  paragraph on drawing and `examples/shapes.12am` are in its form.
- Slice 1's programs that draw are written in its form; the `Line` of
  the programs of `pantry` and `league` and the `Rect` of seven programs
  of `tests/fail` are renamed; and `nix develop -c cargo test` and
  `self-test` pass before the runs. Among its programs are one for the
  order by `layer` and by index, one with all four components on one
  entity, one that adds and removes a shape, and a `tests/fail` program
  for each of its diagnostics.
- The two tips that are run differ in `src/`, `runtime/`, `tests/`, in
  "Drawing" and the lists of built-in names of `docs/language.md`, in
  the README's paragraph on drawing, in `examples/shapes.12am` and its
  `.draws`, in the renamed programs of `pantry` and `league`, and in
  each drawing task's `solution.12am`, `right/`, `wrong/` and, for
  `retouch`, the given program. Every other file is the same bytes on
  both, among them `evals/eval.rb`, `evals/preamble.md`, `evals/kit`,
  `flake.nix`, `flake.lock`, `Cargo.toml`'s version, and each drawing
  task's `TASK.md`, `meta`, `expected.out`, `expected.draws`,
  `check.drive`, `systems` and given commands. `git diff --stat` of the
  two tips shows it, and the Outcome has that list.
- Nothing of it renders and it is in no release. Its runs are made with
  the branch checked out, and their results and report are committed on
  `main`, in the commit that has `main`'s own results of the four tasks:
  that commit is the "next commit" of both. The branch is removed when
  the Outcome is written. Its results then name a commit that no branch
  has, which the Outcome says, with the commit's hash: `grade --run` and
  `--resume` cannot be used on them after that.

**Tasks.** Four, by the rules of "Tasks" in spec 0008 but for what this
section says otherwise, with the areas `draw` and `window`, and `input`
for `gauge`; that spec's table of areas gains `draw`. They are in the
same directories on both branches:

- `poster`: a still picture with one of each shape at stated places,
  sizes and colours, and three shapes that overlap in a stated order;
- `drift`: entities that move and are drawn each tick, some of which
  leave the window and are despawned while new ones are spawned;
- `gauge`: a bar whose length follows a number that keys change, and a
  marker that is drawn only while a key is held;
- `retouch`: a given program whose picture is wrong in three ways (an
  order of overlap, a rectangle placed by its centre, two bytes of a
  colour exchanged), to repair.

The tasks are chosen so that neither form has the easier set: what is
alive is drawn (`drift`) and a picture stays (`poster`) with less to
write in the component form, and a shape is drawn or not by a condition
(`gauge`) with less in the statement form. The Outcome gives the passes
of each task by form beside the pooled counts.

`TASK.md` is the same file on both branches. It states what each frame
holds: every shape, with its numbers and its colour, when it is in the
frame and when it is not, and, for a task compared in order, which shape
comes after which. It says that a frame holds those shapes and no other:
one draw for each, none of no size, none with an `a` of 0 and none that
the task does not name. It names no statement, no component and no
system that draws. A task that names its systems names those both forms
have, in the order they run, and says that the program may have others
after them; this is where the four tasks leave spec 0008's rule that a
task lists the systems the program has. It states a frame as what there
is when its tick ends and where it is then, so that in both forms the
task, and not a choice of the agent, fixes what a frame holds.

`poster`, `gauge` and `retouch` are compared in order, and in each of
them no two shapes of one frame belong to two entities of one kind, so
the order of a query decides nothing. `drift` is compared in any order,
and asks for no overlap that the order decides. Each task uses numbers
that are not symmetric, so that two arguments exchanged, or a corner
taken for a centre, changes the list; every number a task draws is exact
in 24 bits; and the answers of a task's commands are under 1 MB
together.

The given program of `retouch` is written in each form with the same
three faults. The order of overlap is wrong by the order of two
statements of one system in the statement form, and by the order of two
`spawn`s, every `layer` left at 0, in the component form: in each the
two lines change places.

A task with `expected.draws` has a `wrong/` program that prints
`expected.out` and fails only the check **draws**; one that draws the
right picture with a shape of no size besides; and, where the task is
compared in order, one with the stated shapes in another order.

**Grading.** A task may have `expected.draws`, which holds each `draws`
answer of its `check.drive` as `main` prints it, with `midnight: drive: `
removed, as a `.draws` file of the suite does; it is the same file on
both branches. The check **draws** takes, from the standard error of the
run that the output check makes, the lines of every `draws` answer, and
turns each line, of either form, into what it draws: the kind of shape,
its numbers in the statement's order and the four bytes of its colour, a
`layer` left out; and the first two lines into the tick, the window's
size and the background. It does the same with the lines of
`expected.draws`, and requires the two to be equal: line for line with
`draws: in order` in `meta`; with `draws: any order`, each answer's
tick, size and background as they are and its shapes as a set with
counts. It fails when more than 1 MB of standard error was cut.

- The harness keeps the answers in `graded.draws`, beside `graded.out`.
  For a run that fails **draws**, the results record which answer first
  differs and how: a shape missing, a shape too many, the order, or the
  first shape that differs with the expected one. `report` lists, for
  each task, the failed runs whose first differing shape is an expected
  one with two numbers exchanged, and those with a rectangle at the
  expected place plus half its size.
- Of a `draws` answer in a compiler call's standard error the harness
  records the first line with its numbers left out (`drive: draws`), and
  not the lines after it.
- `expected.draws` joins the files a task's hash covers; a task without
  the file keeps the hash it has.
- `self-test` requires the `wrong/` program that fails only **draws**,
  and `eval_solutions_pass` compares the `draws` answers of each task's
  `solution.12am` and `right/` programs with `expected.draws` as the
  check does.
- A drawing task prints at least one line a tick, so `expected.out` and
  what `TASK.md` shows of it are as for every task. `TASK.md` also says
  that the frames are checked with `draws`, and shows the first two
  lines of the first answer its commands give: the tick, the size, the
  number of draws and the background, and no line of a shape. Those
  lines are the same in both forms.

**The programs agents compile.** The wrapper keeps the text of the
program at each `check`, `build` and `run` in the log directory, and the
harness copies them beside the call log under `evals/runs/`. The audit
below reads them.

**The runs.**

- Before any, each task is reviewed adversarially in both forms: a fresh
  agent, given both branches' directory of the task and both references,
  looks for a program of either form that draws what the task states and
  that the checks reject, a wrong one they pass, and wording an agent of
  one form reads differently from an agent of the other. Every item is
  taken on both branches.
- The four tasks on each branch, `sonnet` and `haiku`, 10 trials, with
  `--label draw`: 80 sessions for each form. On `main` this `run` is
  also the one a task added must have on the commit that adds it.
- After those two and before anything is decided, a fresh agent that has
  not seen the tasks' design reads the kept programs and the kept
  `draws` answers of every failed run of both forms, and of one passed
  run of each task, model and form, and reports every program that draws
  what the task states and failed, and every answer reached from outside
  the run's directory. If there is one, the task or the harness is fixed
  on both branches and both forms run again.
- The thirteen tasks of 0.7, named with `--tasks`, 5 trials, with
  `--label thirteen`, on `main` at the commit of its run of the four.
  The run before is at the commit before the first of slice 1; the
  baseline run of spec 0008 stands for it when every task, every file of
  the kit and the preamble have, at that commit, the hashes the baseline
  records, and `compare` of the baseline with the run after names no
  difference in the models as reported, effort, trials, the agent's
  tools or Claude Code's version.
- At the last commit, the thirteen again with 5 trials and the four with
  10: `examples/flock.12am` and the README's commands join the kit in
  slice 5. The Outcome has the `compare` of the run before with each run
  of the thirteen, and of the two runs of the four on `main`.

**What decides**, stated before the first run:

- **The form.** The two runs of the four tasks are made one after the
  other, with the same `--jobs` and nothing else running, and `compare`
  of them names no difference in the models as reported, effort, trials,
  the preamble, the agent's tools or Claude Code's version. No run of
  either is escaped: an escape is fixed and both run again. A graded run
  is one that is neither an infrastructure error nor stopped for
  isolation; a timeout and a run with no solution are graded and failed.
  `compare` gains a line under its table of tasks: over all its rows,
  the passes and the graded runs of A and of B, and the p of Fisher's
  exact test, two-sided, on the passes and failures of the two. The
  statements stand unless the components pass a larger share of their
  graded runs and that p is below 0.1.

  If the components pass by that test, work stops and the result is
  reported, with counts 1 to 3 below counted for both forms, the runs'
  first-try compiles, failed compiler calls, turns and the diagnostics
  each form met, and with what the evals do not measure: a program whose
  positions are in a component of its own keeps a second copy in step to
  be drawn by components; what is drawn is not written in any system;
  and the four names are taken from every program, among them programs
  of two of the thirteen tasks. Both forms' results are committed on
  `main` first; both branches stay as they are; `main` is not released
  in that state, since its reference describes pictures it does not yet
  make; and this spec stays approved with no Outcome until the form is
  decided. The finer measures are reported for both forms whichever
  stands, and decide nothing alone.
- Then, counts 1 to 3 over the graded runs of the statement form and
  count 4 over the thirteen tasks:
  1. In a tenth or more of the runs of `poster`, `drift` and `gauge`,
     the graded program builds, fails the check **draws**, and its first
     differing shape is the expected one with two of its numbers in each
     other's places: draws take their arguments by name as well.
  2. In a tenth or more of those runs, the first differing shape is a
     rectangle with the expected `width` and `height`, and an `x` and
     `y` that are the expected ones plus half the `width` and half the
     `height`: the reference's wording and example change, and the task
     runs again.
  3. A diagnostic of this spec is a compile error whose message is one
     of those under "The compiler", or the command error of `draws`. Of
     the runs in which one comes up, when there are five or more, a
     quarter or more have it repeated, as the results record that: its
     message is rewritten, and the tasks run again.
  4. `compare` marks more rows of the thirteen tasks, a task and a model
     each, that pass less often after than before, than the number of
     marks it says chance would give, rounded up: where the section
     stands in the reference and how long it is are looked at again.
     Count 4 is read on the run of slice 2 and decides whether work
     stops there; read again on the last `compare` it stops nothing, and
     the Outcome says what it was.

  With none of these reached, the Outcome says so. A count reached is
  weighed with what the evals do not measure, as any worse result is.

### The flake

- `gpuTestsFor` names `window_drawn`, so `checks.gpu` and
  `apps.gpu-test` run it.
- Nothing else changes: no new package, flag or app.

### Measured in planning

On a Ryzen 9 9950X3D: an RTX 5090 with NVIDIA's driver 595.71.05, the
processor's integrated Radeon with RADV, and `llvmpipe`, both of Mesa
26.1.8, through `wgpu-native` 27.0.4.0. Hand-written programs drew each
case into a texture with no window and compared the bytes read back, at
256 by 256 and at 250 by 200 pixels, 21 pictures of each case of
drawing with different random numbers. A shader with the tests of this
spec was drawn at five sizes and scales, 840 opaque pictures and as
many translucent.

- **Upright rectangles** gave the same bytes on the three devices with
  corners on whole pixels and on 1/16 and 1/256 of one, and shared edges
  left no gap and no pixel covered twice.
- **A fragment's position** was the pixel's centre, exactly, on all
  three, at both sizes.
- **The tests of this spec in a fragment stage**, every product behind
  the barrier, for 5,000 shapes of each kind, shapes with a pixel centre
  exactly on an edge, and fans of triangles that share edges, at scales
  of 1, 1.25, 1.5 and 2: the same bytes on the three devices, and the
  bytes the same arithmetic gives on the CPU, in every picture. Without
  the barrier `llvmpipe` still gave the CPU's bytes and the two others
  did not: in 14 of 100 pictures of shapes with a centre on an edge, by
  1 to 6 pixels a picture, all but one at scales of 1.25 and 1.5, and in
  1 of 100 pictures each of lines, of triangles and of the kinds mixed,
  by one pixel. A test fed by a value the device interpolated across the
  shape differed between every two devices, in 15 of 21 pictures of
  circles with numbers on a grid.
- **An edge between two triangles**, within rounding of a pixel centre,
  20 million times on the CPU: with each triangle computing the edge
  from its own corners the pixel was in both 849,455 times and in
  neither 834,576 times; with the edge computed from its two ends in one
  order, never.
- **Edges left to the rasterizer**: of 40,000 triangles with corners
  anywhere, at 250 by 200, 1 to 3 pixels differed between devices; in
  scenes where many triangles cross the edge of the target, `llvmpipe`
  differed from the two others on about 1 pixel for every 1,000
  triangles. Lines drawn as the device's own lines differed on a few to
  tens of pixels between the 5090 and `llvmpipe`, and on 800 to 2,100
  between RADV and each of them. A multisampled frame differed on 421 to
  9,424 pixels.
- **Blending by the device** on an `RGBA8Unorm` target: the three
  devices differed from one another in every translucent picture. The
  5090 gave the rounded value of the rule on every one of 80 million
  translucent pixels; RADV differed from the rule on 8 to 16% of them,
  by 2 at most, and `llvmpipe` on 7% of those under one translucent draw
  and most of those under more, by 4 at most. No pixel differed from the
  rule by more than the number of translucent draws over it, and with 1,
  2 and 3 such draws some pixel differed by that much. On an
  `RGBA8UnormSrgb` target the devices differed by 1 to 2 under one layer
  and by up to 4 under many. Colours with an alpha of 0 or 255 gave the
  same bytes everywhere.
- **Clearing an `RGBA8Unorm` texture** to a byte divided by 255 left
  that byte, for every value in every channel, on all three.
- **Showing an `RGBA8Unorm` frame**, in a compositor that showed nothing
  and was started for the purpose, on Wayland: the surface listed
  formats that are not sRGB on each device, and a picture of the
  compositor had the frame's bytes in every pixel, both with a plain
  copy to such a surface and through an sRGB view of the frame to an
  sRGB surface.
- **Smoothed edges** from a distance, blended: every two devices
  differed on thousands of edge pixels, the 5090 from each of the others
  by up to 6 and those two from each other by up to 3.
- **A compute pass that writes records and a render pass that draws
  them as its instances**, in one submission and across two: accepted
  with no copy on all three devices, with the bytes that records sent
  from the CPU give.
- **Cost.** A frame of 1920 by 1080 with N shapes of about 16 by 16
  pixels, of the four kinds mixed, their records already on the device,
  by a shader with the tests of this spec, from handing the frame over
  to the device having finished it:

  | N         | RTX 5090 | Integrated Radeon | `llvmpipe`, 32 threads |
  | --------- | -------- | ----------------- | ---------------------- |
  | 10,000    | 0.05 ms  | 0.95 ms           | 4.6 ms                 |
  | 100,000   | 0.13 ms  | 9.2 ms            | 38 ms                  |
  | 1,000,000 | 0.96 ms  | 92 ms             | 363 ms                 |

  Against the same tests with no barrier and each triangle computing its
  own edges, this cost nothing measurable on the 5090 and on `llvmpipe`;
  on the Radeon nothing for circles and a third more for the kinds
  mixed. Sending records from the CPU took 1.2 to 1.6 ms for a million
  on each device. Among circles, a record of kind 0 cost the 5090 a
  ninth of a shape and `llvmpipe` a third. One opaque rectangle over the
  whole frame and 100 translucent ones over half of it: 0.23 ms, 10.7 ms
  and 13 ms. Reading back a frame of that size took about half a
  millisecond on all three.

Not measured: a surface outside Wayland, and an output with colour
management or more than 8 bits; on a device, a shape with a NaN, with a
size that is not greater than zero, with an infinity or with numbers
large enough for a test to overflow, and a shape far larger than the
frame with the cut to the frame; any device through Metal; any GPU but
these three.

### Tests

- `NAME.draws` holds the lines of every `draws` answer of a run, with
  `midnight: drive: ` removed, and nothing else. `NAME.frames` holds
  every other answer: `window_frames` and `window_shown` leave the lines
  of `draws` answers out before they compare. `UPDATE_EXPECT=1` rewrites
  a `.draws` file from the release run of `draws_match_expected` and
  from no other test.
- `tests/run`, with `NAME.drive` and `NAME.draws`: `draws_match_expected`
  runs each headless with `MIDNIGHT_GPU=off`, in both build modes, and
  requires the `draws` answers to be exactly `NAME.draws`. The programs
  cover:
  - each shape; arguments that are `Int`; shapes outside the window;
    sizes of zero, a negative `width`, `height` and `radius`, and a NaN
    in each position of one shape; `Infinity`, `-0.0` and a number in
    exponent form among a draw's numbers;
  - the order of draws across systems, inside one system and from
    top-level code between ticks; a draw whose argument calls a function
    that draws; a frame that holds nothing of the frame before;
  - a draw before `Window.open` and after the window is closed, each
    with an argument that prints; a window opened again; `draws` before
    the first tick, in a program that never draws, and after the
    commands of a program that has ended; a `resize` and a `scale`
    between two frames, with `draws` after each;
  - a draw in a function, in `on` and in a counting loop;
  - draws from an `each` that runs on several cores, with rows in
    several tables and a `.split` file; a draw in an `each` nested in a
    split loop, and one in a function called from a split loop;
  - a GPU system that draws, with a draw in the arm of an `if` and a run
    that ends with `next`; one whose draw is reached in one tick and not
    in the next for the same row, with `draws` after each; one over
    several tables; a strict GPU system that draws; a GPU block that
    draws and writes nothing; an ordinary system that draws, a GPU
    system that draws over it, and an ordinary system that draws over
    both.
- `thread_programs` and `tsan_programs` run those with a split loop, and
  require the same `draws` answers for every setting.
- `tests/fail`: every diagnostic above. `tests/loops`: a loop that draws
  is independent.
- `drive_errors`: `draws` with an argument, and with no window open.
- An enum variant named `Draw` leaves the generated C of a program as it
  was without it.
- The reference renderer has unit tests of its own: the pixels of each
  example in "Which pixels a shape covers"; shapes that share edges; a
  line one point wide along whole and half coordinates in both
  directions and along a diagonal; a circle with a negative radius, a
  line with a negative width and a NaN in each position of one shape; a
  shape far larger than the frame; the colour rule for every `a` on a
  few bytes; scales of 1, 1.25, 1.5 and 2.
- `frames_are_the_colours_painted` stays a test that needs no device. It
  gives the reference renderer the size and the background of each frame
  of the two programs it holds today, with no draws, and requires the
  CRC-32 of its pixels to be the one in the `.frames` file.
- GPU mode:
  - `window_drawn` runs every program of `tests/run` and `examples`
    whose commands have a `draws` on the line before a `capture`,
    headless on the device, in both build modes, and requires each such
    capture to agree with the reference renderer's picture of the answer
    that `draws` gave in the same run, at the size the `capture`
    answered. It runs each program again with a `scale` command of 1.25,
    of 1.5 and of 2 put before the program's own commands. Among its
    programs: a rectangle and a circle far larger than the window, one
    corner of a triangle at 1e30, and a line that crosses the window
    from ends a million points away; translucent draws many layers deep;
    a frame with an ordinary system's draws, a GPU system's over them
    and an ordinary system's over both, all overlapping;
    a block that draws and does not fit the device.
  - `window_frames` goes on holding `.frames` files, which now include
    programs that draw with no `a` from 1 to 254: their CRCs are the
    same on every device.
  - `gpu_programs` requires the `draws` answers of a program with a GPU
    system that draws to be `NAME.draws`, by the rule for its output:
    exactly when its GPU systems are all strict, and otherwise within
    its `# tolerance:` for each number.
  - `gpu_stats_match_expected`: a GPU system that moves and draws
    100,000 entities uploads and downloads nothing after its first tick.
    Its commands have no `draws` and no `capture`.
- Window mode:
  - `window_shown` also runs every program `window_drawn` runs whose
    commands have no `resize` or `scale`: `info` must name `wayland`,
    each capture must agree with the reference renderer as in
    `window_drawn`, and the picture of the compositor must hold the last
    capture's pixels.
  - `window_keeps_time` runs a second program, which draws ten thousand
    shapes a tick, under the same three conditions and the same limits.
- On `main`, `eval_solutions_pass` and `checks.evals` cover the four
  drawing tasks and `expected.draws`; on `0009-components` its suite and
  `self-test` pass as "The component form" says. The harness's tests
  cover the check **draws** for the lines of both forms and both ways of
  comparing, what the results record of a failed check and of a `draws`
  answer, the pooled line of `compare`, and the kept programs.
- `tests/COVERAGE.md` gains "Drawing".
- `examples/shapes.12am`, with `.drive`, `.draws` and `.out`: each
  shape, an order of overlap and a translucent colour.
  `examples/flock.12am`, with `.drive` and `.out` and no `.draws`: a GPU
  system that moves 100,000 points and draws them, each coloured by its
  speed through `clamp`. Its commands have one `draws` before one
  `capture`, after the ticks that `MIDNIGHT_GPU=stats` is read for.

### Slices

1. **The language, with no device.** The name, the checker and its
   diagnostics, the typed program, effects and `midnight loops`, the
   emitter for ordinary code, the list in the runtime, split loops,
   `draws`, and `draws_match_expected` with its programs. In the
   reference: rule 5 of "Independent loops", the sentence of "Queries"
   on what a split loop queues and emits, and the weight of a draw in
   "What several cores cost". Draws in a GPU system as far as the CPU:
   the checker's rules, the lowering, the C form. Until slice 4 a block
   that draws is one that does not fit the device and runs on the CPU,
   and `gpu_programs` leaves out the programs whose GPU systems draw;
   slice 4 takes both away. Until slice 3 the picture of a frame is its
   background.
2. **The two forms, and the evals.** In this order:
   1. On `main`: the reference as it will ship, but for the numbers
      measured on the implementation (0.8 on its first line and in
      `Cargo.toml`, "Drawing" after "The window", and every sentence of
      the other sections that this spec changes, listed below);
      `examples/shapes.12am` with its `.drive`, `.draws` and `.out`; the
      README's paragraph; in the harness the check **draws**, what it
      records of a `draws` answer, the kept programs and the pooled line
      of `compare`; `evals/README.md`; the four tasks.
   2. On `0009-components`, made from that commit: `Draw` taken out, and
      the rest as "The component form" says, with its suite passing.
   3. The adversarial review of each task in both forms, every item
      taken on both branches.
   4. The run of the four tasks on each; the audit of the failed runs;
      both results committed on `main`.
   5. What decides the form. If the components pass by the test, work
      stops there and the result is reported.
   6. On `main`: the thirteen tasks, and the four counts. If count 1 or
      4 is reached, work stops there and the result is reported; counts
      2 and 3 are acted on and their tasks run again before the slice
      ends.
3. **Pixels.** The frame texture's and the surface's formats, the shape
   pipeline for the CPU's list, `capture`, the reference renderer and
   its tests, `window_drawn`, `window_shown`, `window_keeps_time`, when
   a frame is rendered and waited for, `stats`.
4. **Draws on the device.** The records of a GPU block in GPU memory,
   their spans in a frame, `draws` bringing them back, and the GPU-mode
   tests of programs with a GPU system that draws.
5. **Documents.** The measured costs in the reference, the README's
   commands, the repository's `CLAUDE.md`, `tests/COVERAGE.md`,
   `examples/flock.12am`, the last runs of the evals, the Outcome with
   both forms' results; then the branch `0009-components` is removed,
   and with it what the harness and its tests have for the lines of the
   component form.

The reference changes in these places beside the new section:

- the first paragraph's list of specs, and the version;
- the built-in names of "Lexical structure" and of "The window", with
  what gives a program a window;
- "Queries" and rule 5 of "Independent loops": what a split loop queues,
  emits and draws; what an independent `each` may do; the weight of a
  draw in "What several cores cost";
- "The window": the opening paragraph and "A frame for each tick" (what
  a frame holds; that a hidden window renders nothing; the wait for the
  device to finish the frame before), and its "Determinism";
- "Without a display": a command's answer is one line but for `draws`;
  with `MIDNIGHT_GPU=off` a program captures nothing and `draws` works;
  the table of commands and the list of command errors;
- "What a GPU block may contain" and "What a GPU system may touch": a
  draw directly in the block;
- "Snapshot semantics": `World.tick` returns when the device has
  finished the tick's computing, and a frame is handed over after that;
- the GPU systems' "Determinism": the draws of a GPU system that is not
  strict;
- "The transfer plan": a draw is no transfer and has no line, and what
  `draws` brings back is not in the plan;
- "What things cost", with "What a draw costs";
- "Errors": no new run-time error;
- the grammar's note on statements;
- "Not in 0.8": the non-goals above under the window, and under GPU
  systems a draw in a loop, in the nested `each` and in a called
  function.

## Acceptance criteria

All run on x86_64-linux, on the development machine.

1. `nix flake check` passes, and `nix run .#gpu-test` and
   `nix run .#window-test` pass on the hardware device.
2. Every `.out`, `.err`, `.plan`, `.loops`, `.split`, `.stats` and
   `.frames` file that existed in 0.7.0 is unchanged. For every program
   of `tests/`, `examples/` and `bench/` that existed in 0.7.0,
   `midnight emit-c` and the generated shaders are byte for byte what
   0.7.0 gives.
3. Every program with a `.draws` file gives exactly those answers
   headless with `MIDNIGHT_GPU=off`, in release and debug, on every run,
   and for every thread setting of `thread_programs`.
4. `window_drawn` passes on `llvmpipe` in the build sandbox and on the
   hardware device, in release and debug, at each scale. For every
   program of it whose draws all have an `a` of 0 or 255, the captured
   frames of the two devices are the same bytes.
5. `examples/flock.12am` on the hardware device: `MIDNIGHT_GPU=stats`
   shows no upload and no download in the ticks before its `draws`, and
   `window_drawn` passes for it.
6. Every statement, command, diagnostic and command error of this spec
   has a test that passes.
7. A debug build of every program that draws ends with no sanitizer
   report, headless and inside the compositor of the window tests.
8. No test, check or app of this spec opens a window on the desktop it
   is run from.
9. The evals ran as "The evals" says: the four tasks on both forms
   before slice 3, and the thirteen before and after. The audit of the
   failed runs found no correct program rejected. The Outcome has the
   passes of each form, by task and model and pooled, with the test's p;
   the finer measures of both; the files in which the two tips differed;
   the `compare` of the run before with each run of the thirteen; and
   the four counts, and says what each decided.
10. `examples/shapes.12am` and `examples/flock.12am`, started by hand on
    the development machine's desktop in release and in debug: both show
    their picture, `flock` keeps its pace, and both end cleanly when the
    window is closed.
11. The reference says 0.8 and `midnight --version` 0.8.0, every command
    in the README works as written, the reference's costs are measured
    on the implementation, and `tests/COVERAGE.md` maps every statement,
    command, diagnostic and command error of "Drawing" to a test.

## Decisions

- **A draw is a statement.** A program's components are its own, laid
  out for what it simulates, and there is no built-in position to hang a
  picture on. A statement in an `each` draws from whatever the program
  has, and says in the source what is drawn and over what.
- **One name, `Draw`.** Every shape this spec has lives under it, so
  programs keep the names `Line`, `Rect` and `Circle` for their own
  types and `draw` for their own system.
- **The arguments are the whole shape.** Colour, size and thickness are
  written at the draw. No setting made earlier in a program changes what
  a draw means.
- **Position first, colour last, by position.** It is the order of the
  same calls in the toolkits the names come from, and a colour in the
  wrong place is a type error with a message that says so.
- **Order is the order of the program.** It is total and costs nothing,
  and it is the order a reader sees.
- **Points, from the top left, y down.** It is where the pointer already
  is: a click at `Mouse.x`, `Mouse.y` lands on what was drawn there.
- **A rectangle by its corner, a circle by its centre.** A rectangle
  from `x` to `x + width` covers whole pixels at whole numbers; a circle
  is where its entity is.
- **The frame is a list.** A list fixed at the end of a tick is the same
  on every machine and needs no device to look at, which is what lets
  every test and every agent check drawing; the picture is made from it
  when one is wanted.
- **`draws` prints statements.** A line of the list is the line of
  source that would draw it, so there is one notation to learn and a
  frame can be read against the program.
- **Coverage is decided for each pixel, by rules in the reference.**
  Which pixels the edge of a slanted shape covers is the part of drawing
  devices were measured to disagree on when it is left to them. Decided
  from the pixel's centre it is a definition, the same on the devices
  measured and on the CPU, and a level line one point wide is one row of
  pixels wherever it lies.
- **Hard edges.** A covered-or-not pixel keeps an opaque frame exact on
  every device and keeps thin lines sharp.
- **Colour bytes as they are.** A colour reaches the frame with no
  conversion, so an opaque draw is exact everywhere, and a translucent
  one mixes the numbers a person typed.
- **Translucent pixels may differ by a step.** Devices round the mix
  differently. A picture is never read by a program, so the difference
  cannot reach what a program computes; the reference says how large it
  can be, and the tests hold devices to that.
- **A draw in a GPU system is a record written on the device.** Data a
  GPU system keeps on the device is drawn without leaving it, by the
  statement ordinary code uses, with the order ordinary code gives.
- **A program that draws must open a window.** The commonest way to see
  nothing is to have opened nothing, and the compiler can say so.
- **Two forms are measured, before the pipeline.** After slice 2 the
  tests' programs, the examples, the reference's costs and the records a
  GPU block writes are all made in the form of a draw, so that is when
  it is cheapest to change. Both forms can be built as far as `draws`
  without a device, and agents can be measured on them as soon as
  `draws` works.
- **Statements stand unless components pass clearly more.** The evals
  measure how well agents write a form. What a form costs a program
  whose data has a layout of its own, and a reader who wants to see what
  is drawn, they do not measure, so a result within chance changes
  nothing, and one beyond it is weighed by a person.

## Outcome

Not implemented.
