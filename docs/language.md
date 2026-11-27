# The Midnight language

This document describes Midnight 0.7: the language core specified by
[`specs/0001-language-core.md`](../specs/0001-language-core.md), the GPU
systems specified by
[`specs/0002-gpu-systems.md`](../specs/0002-gpu-systems.md), what moves
between CPU and GPU memory, specified by
[`specs/0003-moving-less.md`](../specs/0003-moving-less.md), strict GPU
systems, specified by
[`specs/0004-strict-gpu-systems.md`](../specs/0004-strict-gpu-systems.md),
loops on several cores, specified by
[`specs/0005-several-cores.md`](../specs/0005-several-cores.md), a
window, specified by [`specs/0006-a-window.md`](../specs/0006-a-window.md),
and the keyboard and the mouse, specified by
[`specs/0007-keyboard-and-mouse.md`](../specs/0007-keyboard-and-mouse.md).
It is the reference for what a program means. Where the compiler and this
document disagree, one of them has a bug.

Midnight is a compiled, statically typed language built around the
entity-component-system (ECS) pattern. Its syntax borrows from Ruby. Source
files end in `.12am`.

## A first program

```ruby
component Position
  x: Float
  y: Float
end

component Velocity
  dx: Float
  dy: Float
end

tag Frozen

system move
  each Position, Velocity, without: Frozen do |pos, vel|
    pos.x += vel.dx * Time.dt
    pos.y += vel.dy * Time.dt
  end
end

system report
  each Entity, Position do |e, pos|
    puts "#{e} is at (#{pos.x}, #{pos.y})"
  end
end

spawn Position(0, 0), Velocity(1, 2)
spawn Position(5, 5), Velocity(-1, 0), Frozen

Time.dt = 0.5
3.times do
  World.tick
end
```

The top-level statements run once, in order. Each `World.tick` runs every
system once, in the order the systems appear in the source.

## The model

A running program has one **world**. The world holds:

- **Entities.** An entity is an identity and nothing else. A value of type
  `Entity` is a handle to one.
- **Components.** A component is a piece of plain data attached to an entity.
  An entity has at most one component of each type. A **tag** is a component
  with no data.
- **Resources.** A resource is a single, global instance of a data type.
- **Events.** An event is a message that systems emit and other systems
  receive.

Behavior lives in **systems** and **functions**. A system is a named block of
code that the world runs once per tick. Systems find the data they work on with
**queries**: "every entity that has a `Position` and a `Velocity`".

Two rules shape the rest of the language:

1. **Everything is a value.** Numbers, strings, lists, structs and components
   are all copied on assignment. There are no object references, no `nil`, and
   no garbage collector.
2. **An `Entity` is the only way to refer to something.** To point at a thing,
   store its `Entity` handle. A handle can outlive its entity; `alive?` says
   whether it has.

There are no classes. Data is declared with `struct` and `component`; behavior
is written in `def` and `system`.

## Lexical structure

Source is UTF-8. A newline ends a statement, except:

- inside `(` … `)` and `[` … `]`,
- after a binary operator, a comma, `=`, or a compound assignment operator,
- when the next line begins with `.` (a method chain).

A `#` starts a comment that runs to the end of the line.

**Identifiers.** Names of variables, functions, systems, methods and fields
begin with a lowercase letter or `_`, followed by letters, digits and `_`.
Function and method names may end in `?` or `!`. Names of types and constants
begin with an uppercase letter.

A `?` or `!` directly after an identifier is part of the identifier, with one
exception: a `!` followed by `=` is not, so `a!=b` is `a != b`. Write the
conditional operator with spaces: `ok ? 1 : 2`.

**Keywords.** These words are reserved:

```
break case component def do each else elsif emit end enum event false gpu
if loop next on print puts require resource return self spawn strict struct
system tag then true unless until when while
```

`gpu` is used in one place, before `system`: see [GPU systems](#gpu-systems).

The names `Int`, `Float`, `Bool`, `String`, `Entity`, `World`, `Time`,
`Math`, `Window`, `Color`, `WindowResized`, `WindowClosed`, `Key`,
`MouseButton`, `Keyboard`, `Mouse`, `KeyPressed`, `KeyReleased`,
`MousePressed`, `MouseReleased`, `WindowFocused` and `WindowUnfocused`,
and the compact type names (`Float32`, `Int8`, `Int16`, `Int32`, `UInt8`,
`UInt16`, `UInt32`), are built in and can't be redefined.

**Literals.**

| Kind    | Examples                                  |
| ------- | ----------------------------------------- |
| `Int`   | `42`, `1_000_000`, `0xff`, `0b1010`       |
| `Float` | `3.14`, `1_000.5`, `1e-3`, `2.5e6`        |
| `Bool`  | `true`, `false`                           |
| String  | `"hello"`, `'raw'`                        |
| List    | `[1, 2, 3]`, `[]`                         |

A double-quoted string supports the escapes `\n`, `\t`, `\r`, `\0`, `\e`,
`\\`, `\"` and `\#`, and interpolation: `"#{expr}"` inserts `expr.to_s`. A
single-quoted string is taken as written; only `\'` and `\\` are escapes.

## Types

| Type        | Description                                              |
| ----------- | -------------------------------------------------------- |
| `Int`       | 64-bit signed integer. Arithmetic wraps on overflow.     |
| `Float`     | 64-bit IEEE 754 floating point.                          |
| `Bool`      | `true` or `false`.                                       |
| `String`    | Immutable UTF-8 text.                                    |
| `Entity`    | A handle to an entity.                                   |
| `[T]`       | A list of `T`, for any value type `T`.                   |
| struct      | Declared with `struct`.                                  |
| component   | Declared with `component`.                               |
| event       | Declared with `event`.                                   |
| enum        | Declared with `enum`.                                    |

These are the **value types**: any of them can be the type of a variable, a
field, a parameter or a list element. Tags and resources are not value types.

Every expression has a type known at compile time. Types are written out in
three places only: fields, function parameters, and (optionally) function
return values. Local variables take the type of the value they are first
assigned.

**Int to Float.** An `Int` converts to `Float` wherever a `Float` is
expected: in arithmetic with a `Float`, when assigned to a `Float` variable or
field, when passed as a `Float` argument, and when returned from a function
that returns `Float`. Nothing else converts implicitly. To go the other way,
use `to_i`, `floor`, `ceil` or `round`.

**No truthiness.** A condition must be a `Bool`.

**In a GPU system** `Int` and `Float` are 32 bits wide. See
[Arithmetic at 32 bits](#arithmetic-at-32-bits).

### Compact field types

A field or a list element can be stored in less space than an `Int` or a
`Float` takes:

| Type      | Stored as                | Reads as | Range                          |
| --------- | ------------------------ | -------- | ------------------------------ |
| `Float32` | 32-bit IEEE 754          | `Float`  | about 7 significant digits     |
| `Int8`    | 8-bit signed integer     | `Int`    | -128 to 127                    |
| `Int16`   | 16-bit signed integer    | `Int`    | -32,768 to 32,767              |
| `Int32`   | 32-bit signed integer    | `Int`    | about ±2.1 billion             |
| `UInt8`   | 8-bit unsigned integer   | `Int`    | 0 to 255                       |
| `UInt16`  | 16-bit unsigned integer  | `Int`    | 0 to 65,535                    |
| `UInt32`  | 32-bit unsigned integer  | `Int`    | 0 to about 4.3 billion         |

```ruby
component Position
  x: Float32
  y: Float32
end

component Health
  hp: UInt8 = 100
end
```

Compact types describe storage, not arithmetic. They are allowed as the type
of a field (in a struct, component, resource or event) and as the element
type of a list, and nowhere else: variables, parameters and return values are
always `Int` or `Float`. Reading a compact field gives an `Int` or a `Float`,
and all arithmetic happens at 64 bits (at 32 in a GPU system). So
`pos.x += vel.dx * Time.dt` is written the same way whatever the fields'
storage types.

Writing converts back:

- A `Float` stored as `Float32` is rounded to the nearest representable
  value.
- An `Int` stored in a compact integer type must fit. A constant that does
  not fit is a compile error; a computed value that does not fit is a runtime
  error.

`[Float32]` and `[Float]` are different list types and do not convert to each
other. A list literal takes its element type from where it is used.

## Variables and values

A variable is created by its first assignment and keeps that type:

```ruby
count = 0            # Int
name = "midnight"     # String
ratio: Float = 1     # annotated; the Int converts
scores: [Int] = []   # an empty list needs a type from somewhere
```

A variable belongs to the block it was created in (a function body, a branch
of an `if`, a loop body, a `do` block) and to blocks nested inside that one.
To use a value after an `if`, create the variable before it. Block parameters
must not reuse a name that is already in scope.

Assignment copies the value. After `b = a`, changing `b` does not change `a`,
whatever the type:

```ruby
a = [1, 2, 3]
b = a
b.push(4)
puts a.length   # 3
```

Copies of strings and lists are cheap: storage is shared until one copy is
modified.

### What can be modified

Something that can be assigned to or modified in place is a **place**:

- a local variable,
- a block parameter of a query `each` (see [Queries](#queries)),
- a component of an entity, written `entity[Component]`,
- a field of a resource, written `Resource.field`,
- a field or list element of any place: `pos.x`, `path.points[0].x`.

These are read-only: function and method parameters, `self`, constants, and
the block parameters of `on`, of list iteration and of counting loops.
Assigning to one, or to a part of one, is a compile error.

The parameters of a query `each` are the only variables that are views into
the world. Anything else read out of the world is a copy:

```ruby
health = target[Health]    # a copy
health.hp -= 10            # changes the copy only
target[Health].hp -= 10    # changes the component
```

## Expressions

Operators, from tightest to loosest:

| Operators                       | Notes                                   |
| ------------------------------- | --------------------------------------- |
| `f(x)`, `x.name`, `x[i]`, `A::B` | calls, members, indexing, enum variants |
| `!`                             | logical not                             |
| `**`                            | power, right-associative                |
| unary `-`                       | `-2 ** 2` is `-4`                       |
| `*`, `/`, `%`                   |                                         |
| `+`, `-`                        |                                         |
| `<`, `<=`, `>`, `>=`            | do not chain                            |
| `==`, `!=`                      |                                         |
| `&&`                            | short-circuit                           |
| `\|\|`                          | short-circuit                           |
| `c ? a : b`                     | conditional                             |

Assignment (`=`, `+=`, `-=`, `*=`, `/=`, `%=`) is a statement, not an
expression. `a += b` means `a = a + b` with the place `a` located once, for
every type that has `+`: numbers, strings, lists, and structs that define it.
The other compound forms follow the same rule.

A `-` directly before a number literal is part of the literal, so `-1.abs` is
`1` and `-1.upto(1)` counts from `-1`. The one exception is `**`, which binds
tighter: `-2 ** 2` is `-4`.

**Arithmetic.** `+`, `-`, `*`, `/`, `%` and `**` work on `Int` and `Float`.
If either operand is a `Float`, so is the result. Integer division rounds
toward negative infinity and `%` takes the sign of the divisor, as in Ruby:
`-7 / 2` is `-4` and `-7 % 3` is `2`. Integer division or modulo by zero is a
runtime error. `Int ** Int` is an `Int` and a negative exponent is a runtime
error. `x ** 2.0` is `x * x`. A program gives the same `Float` results in a
release build and in a debug build.

`+` also joins two strings or two lists. `*` repeats a string or a list:
`"-" * 20`, `[0] * 64`.

**Comparison.** `==` and `!=` compare any two values of the same type (or an
`Int` with a `Float`), by content: strings by their bytes, lists element by
element, structs field by field, entities by identity. `<`, `<=`, `>` and `>=`
work on numbers and on strings.

**Evaluation order.** Operands and arguments are evaluated left to right. The
right-hand side of an assignment is evaluated before the place on the left is
written.

**`if`, `unless` and `case` are expressions** when every branch ends in a
value of the same type (after `Int` to `Float` conversion) and, for `if`, an
`else` is present:

```ruby
label = if hp > 50 then "healthy" else "hurt" end
```

## Control flow

```ruby
if hp <= 0
  puts "dead"
elsif hp < 20
  puts "hurt"
else
  puts "fine"
end

unless ready
  puts "waiting"
end

puts "low" if hp < 20          # modifier form
return unless target.alive?    # works with `unless` too

while n > 0
  n -= 1
end

until done
  done = step
end

loop do
  break if finished?
end
```

`then` lets a branch share the line: `if a then b else c end`.

**`case`** compares one value against candidates with `==`. The value may be
an `Int`, a `String`, a `Bool` or an enum.

```ruby
case team
when Team::Red then puts "red"
when Team::Blue, Team::Green
  puts "cool colors"
end
```

When the value is an enum and there is no `else`, every variant must be
covered; the compiler names the missing ones.

**Counting loops and list iteration** use `do` blocks:

```ruby
3.times do |i|            # 0, 1, 2; the parameter is optional
  puts i
end

1.upto(3) do |i| ... end      # 1, 2, 3
3.downto(1) do |i| ... end    # 3, 2, 1

names.each do |name| ... end
names.each_with_index do |name, i| ... end
```

List iteration walks the list as it was when the loop started; changing the
list inside the loop does not affect the walk. To update elements in place,
loop over indexes: `xs.length.times do |i| xs[i] *= 2 end`.

`break` leaves the nearest enclosing loop and `next` skips to its next
iteration. This holds for every loop form, including `each` and `on`. The
one exception is the `each` of a [GPU system](#gpu-systems), which `break`
cannot leave.

Blocks are not values in 0.7. `do |…| … end` is accepted only by the loop
forms in this document.

## Functions

```ruby
def clamp(v: Float, lo: Float, hi: Float) -> Float
  return lo if v < lo
  return hi if v > hi
  v
end

def greet
  puts "hello"
end
```

- Parameter types are required. Parameters are read-only.
- The value of the last expression is the return value. `return` exits early.
- The return type may be left out; it is then inferred from the body. A
  function whose value depends on a call to itself, directly or through
  other functions, must declare it.
- A function with no parameters is declared without parentheses and may be
  called without them: `greet` or `greet()`. Calls with arguments use
  parentheses: `clamp(x, 0, 1)`. The same holds for methods.
- A local variable hides a function of the same name when the name is used
  bare. `name(…)` is always a function call.
- Functions may be used before they are defined, and may do anything a system
  can: query, spawn, emit, and read or write resources. A function that a
  GPU system calls is held to the rules of [GPU systems](#gpu-systems).

There is no overloading: each function name is defined once.

## Declaring data

### Structs

A struct is a plain value with named fields and, optionally, methods.

```ruby
struct Vec2
  x: Float = 0
  y: Float = 0

  def +(other: Vec2) -> Vec2
    Vec2(x + other.x, y + other.y)
  end

  def *(k: Float) -> Vec2
    Vec2(x * k, y * k)
  end

  def length -> Float
    Math.sqrt(x * x + y * y)
  end
end
```

**Construction.** `Vec2(3, 4)` gives the fields in order. `Vec2(y: 4, x: 3)`
names them, in any order. The two forms don't mix. A field with a default may
be left out: by name in the keyword form, or from the end in the positional
form. `Vec2()` uses every default.

**Defaults** are constant expressions: literals, constants, enum variants,
operators, constructors, list literals, `Entity.none`, and `Math` functions
and constants.

**Methods.** Inside a method, the fields of the receiver are in scope by name
and `self` is the whole value. Methods cannot modify the receiver; a method
that computes a changed value returns it. Call a method with `value.name` or
`value.name(args)`.

A method named `+`, `-`, `*`, `/` or `%` defines that operator for the struct
on the left-hand side: `a + b` calls `a.+(b)`. `-@` defines unary minus.
`==` and `to_s` are provided for every struct and can't be redefined.

A struct can't contain itself, except through a list.

### Enums

```ruby
enum Team
  Red
  Blue
  Green
end
```

Variants are written `Team::Red`. Enums support `==`, `!=`, `case` and
`to_s` (which gives `"Red"`).

### Components and tags

```ruby
component Health
  hp: Int
  max: Int = 100
end

tag Player
```

A `component` is declared, constructed and used like a struct, methods
included, and has at least one field. In addition, it can be attached to
entities and named in queries.

A `tag` has no fields. It is attached to and removed from entities like any
component, and filters queries, but has no value: it can't be constructed,
stored in a variable or read from an entity.

### Resources

```ruby
resource Score
  points: Int = 0
  best: Int = 0
end

Score.points += 10
```

A resource is one global instance. Every field needs a default. Fields are
read and written as `Score.points`, from anywhere. A resource as a whole is
not a value.

### Events

```ruby
event Hit
  target: Entity
  damage: Int
end
```

An event is declared and constructed like a struct. See [Events](#events) for
how it is sent and received. An event with no fields is written with an empty
body.

### Constants

```ruby
GRAVITY = 9.81
ORIGIN = Vec2(0, 0)
```

A top-level assignment to a capitalized name defines a constant. Its value is
a constant expression that may use constants defined above it. Constants are
visible everywhere and can't be changed.

## Entities

```ruby
player = spawn Position(0, 0), Health(hp: 50), Player
```

`spawn` creates an entity with the listed components and returns its handle.
Each argument is a component value, or a bare type name: a tag, or a
component whose fields all have defaults. Listing a type twice is a compile
error. `spawn` alone creates an entity with no components.

`spawn`, like `puts`, `print`, `emit`, `return` and `require`, takes its
arguments without parentheses. `spawn(…)`, with the parenthesis directly
after the keyword, is the same thing and is the form to use inside a larger
expression.

| Operation              | Meaning                                                    |
| ---------------------- | ---------------------------------------------------------- |
| `e[Health]`            | The entity's `Health`, as a place: `e[Health].hp -= 1`.    |
| `e[Health] = Health(5)` | Replace the component's value. The entity must have one.  |
| `e.has?(Health)`       | Whether the entity has the component or tag.               |
| `e.add(Health(5), Player)` | Attach components. One that is already present is replaced. |
| `e.remove(Health, Player)` | Detach components. Absent ones are ignored.            |
| `e.despawn`            | Destroy the entity and its components.                     |
| `e.alive?`             | Whether the handle still refers to a living entity.        |
| `Entity.none`          | A handle that is never alive.                              |

Handles are safe to keep. When an entity is destroyed, every handle to it
stops being alive, and a new entity never revives an old handle.

Operations on a dead handle are forgiving where they can be: `has?` is
`false`, and `add`, `remove` and `despawn` do nothing. Reading or writing
`e[C]` when `e` is dead or has no `C` is a runtime error, because there is no
value to give. Check with `alive?` and `has?` first when unsure.

## Queries

```ruby
each Position, Velocity do |pos, vel|
  pos.x += vel.dx
end

each Entity, Health, with: Player, without: [Dead, Frozen] do |e, health|
  puts "#{e} has #{health.hp} hp"
end
```

`each` runs its block once for every entity that matches. The block takes one
parameter per term, in order.

- A **term** is a component type or `Entity`. The entity must have every
  component listed. A component parameter is a view of that entity's
  component: assigning to it or to its fields changes the world. An `Entity`
  parameter is the entity's handle.
- **`with:`** adds types the entity must have, without binding them.
  **`without:`** lists types the entity must not have. Each takes one type or
  a bracketed list. Tags go here; they are not terms.
- There is at least one term. `each Entity do |e|` visits every entity.

Queries are checked at compile time: every name must be a component or tag,
no type may appear twice, and the number of block parameters must match the
number of terms.

The order in which entities are visited is not specified, but it is
deterministic: the same program with the same input visits them in the same
order. A loop that runs on [several cores](#several-cores) gives what
visiting the entities in that order gives, and what it queues, emits and
draws is in that order.

`each` can be used anywhere statements can: in systems, in functions and at
the top level. Queries nest, which is how pairs of entities are compared.

`World.count(…)` takes the same terms and filters and returns the number of
matching entities without visiting them. `World.count` with no arguments is
the number of living entities.

## Structural changes inside `each`

`spawn`, `despawn`, `add` and `remove` change which entities a query matches.
Outside a query they take effect immediately. **Inside an `each` block they
are queued, and applied in order when the outermost `each` finishes.** This
includes code in functions called from the block.

While changes are queued:

- An entity returned by `spawn` is alive but has no components yet.
- A despawned entity is still alive, and removed components are still
  present.
- `has?`, `e[C]` and other queries see the world as it was before the queued
  changes.

Writing to component fields is not a structural change and always takes
effect at once.

```ruby
system reap
  each Entity, Health do |e, health|
    e.despawn if health.hp <= 0    # applied when this loop ends
  end
end
```

`break`, `next` and `return` inside `each` behave as usual; queued changes are
applied when the outermost `each` is left, however it is left.

The changes a loop on [several cores](#several-cores) queues are applied in
the order one thread makes them in.

## Systems and ticks

```ruby
system move
  each Position, Velocity do |pos, vel|
    pos.x += vel.dx * Time.dt
  end
end
```

A system is a named block of statements with no parameters. It has its own
local variables, and `return` ends it early. Systems are not called by name.
System names share a namespace with functions. A system declared with
`gpu system` runs its queries on the GPU and follows rules of its own; see
[GPU systems](#gpu-systems).

- `World.tick` runs every system once, in source order (see
  [Programs and files](#programs-and-files) for the order across files).
- `World.run` calls `World.tick` repeatedly until a tick during which
  `World.stop` was called has finished.
- `World.stop` asks `World.run` to return. It has no effect outside
  `World.run`.

`World.tick` and `World.run` are called from top-level code, outside any
`each`. Calling either while a tick is in progress, or inside the body of an
`each`, is an error: a compile error where the call stands in the body, a
run-time error where it is reached through a function.

The built-in resource `Time` describes the clock of the simulation:

| Field          | Type    | Meaning                                              |
| -------------- | ------- | ---------------------------------------------------- |
| `Time.tick`    | `Int`   | Ticks completed. `0` during the first tick. Read-only. |
| `Time.dt`      | `Float` | Simulated seconds per tick. Starts at `1.0 / 60`. Writable. |
| `Time.elapsed` | `Float` | Simulated seconds completed. Read-only.              |

At the end of each tick, `Time.tick` goes up by one and `Time.elapsed` by
`Time.dt`. Ticks run as fast as they can; nothing waits for real time,
unless a window is open: see [The window](#the-window).

## The window

```ruby
Window.background = Color(16, 16, 32)
Window.open("Drift", 800, 600)

system report
  on WindowResized do |size|
    puts "#{size.width} by #{size.height}"
  end
end

World.run
puts "closed after #{Time.tick} ticks"
```

A program can open a window, keep real time while it is open, learn that
it was resized or closed, and show a frame at the end of every tick. A
frame is the window's background colour and nothing more.

The names `Window`, `Color`, `WindowResized` and `WindowClosed` are built
in and can't be declared by a program. A program that names `Window`, one
of the two events, or something of the
[keyboard and the mouse](#keyboard-and-mouse) other than the enums `Key`
and `MouseButton` **has a window**. `Color` alone is a struct like any
other. A name counts where it stands for the built-in: an enum variant
spelled the same, declared in an `enum` or written after `::`, does not
count.

**`Color`** is a struct, as if declared

```ruby
struct Color
  r: UInt8 = 0
  g: UInt8 = 0
  b: UInt8 = 0
  a: UInt8 = 255
end
```

Its components are the bytes of an sRGB colour, the numbers a paint
program or a style sheet shows: `Color(255, 128, 0)` is orange. It is a
plain value with the rules of any struct, and may be a field of a
component. A component out of range is the error it is for any `UInt8`.

**`Window`** is a resource with one instance, like `Time`:

| Member                 | Type    | Meaning                                         |
| ---------------------- | ------- | ----------------------------------------------- |
| `Window.open(title, width, height)` | | Opens the window. `title` is a `String`; `width` and `height` are `Int`, in points. A statement: it has no value. |
| `Window.open?`         | `Bool`  | Whether a window is open. Read-only.            |
| `Window.width`         | `Int`   | Width in points. Read-only.                     |
| `Window.height`        | `Int`   | Height in points. Read-only.                    |
| `Window.pixel_width`   | `Int`   | Width of a frame in pixels. Read-only.          |
| `Window.pixel_height`  | `Int`   | Height of a frame in pixels. Read-only.         |
| `Window.background`    | `Color` | What a frame is filled with. Starts as `Color(0, 0, 0)`. Its `a` is ignored: a window is opaque. |
| `Window.focused?`      | `Bool`  | Keys go to the window. Read-only. See [Keyboard and mouse](#keyboard-and-mouse). |

A **point** is the unit a desktop measures windows in. On most displays a
point is a pixel; on a display with a scale of 2 a window of 800 by 600
points has frames of 1600 by 1200 pixels. A program asks for a size in
points, so a window is the same size to the eye on every display, and
reads the pixel size where it needs the exact grid.

Before a window is opened the four sizes are 0. After one is closed they
keep their last values. The fields are read wherever a resource field is,
in a GPU system too. Assigning to any member but `background` is a compile
error.

**`Window.open`** follows the rule of `World.tick`: it is called from
top-level code; it is a compile error in the body of an `each` and in a
GPU system, and a run-time error while a tick is in progress or when it is
reached through a function inside an `each`. It is also a run-time error
when a window is already open, when `width` or `height` is less than 1 or
more than 8192, and when no window can be shown (see
[Without a display](#without-a-display)). When it returns, the window is
open, shows a frame of the background colour, and the four sizes hold what
the desktop gave it, which need not be what was asked for. A window may be
opened again after it was closed.

**Events.** What happens to the window reaches the program when a tick
begins, before the first system runs, as events:

```ruby
event WindowResized
  width: Int
  height: Int
  pixel_width: Int
  pixel_height: Int
end

event WindowClosed
end
```

- `WindowResized` is emitted when any of the four sizes has changed since
  the last tick began, once, with the sizes the window has now. The fields
  of `Window` change at the same moment and at no other: during a tick
  they do not change.
- `WindowClosed` is emitted when the person at the desktop has closed the
  window. The window is gone when the tick begins: `Window.open?` is
  false.

They are received with `on`, by the rules of every event: emitted at the
start of a tick, they are seen by every `on` of that tick. Emitting one of
them is a compile error.

**Closing the window ends `World.run`.** In the tick that begins with
`WindowClosed`, the runtime also calls `World.stop`. So a program whose
last statement is `World.run` ends when its window is closed, having run
one last tick in which it can react, and a program that ticks in a loop of
its own writes

```ruby
while Window.open?
  World.tick
end
```

When a program ends, by its last statement, `exit` or a run-time error,
its window closes.

**Ticks keep real time while a window is open.** A tick does not begin
until `Time.dt` real seconds after the tick before it began. With the
default `Time.dt` that is sixty ticks a second, on every display. A tick
that takes longer than `Time.dt` is followed at once by the next, and the
time lost is not made up: the program runs slow, never in a burst. The
first tick after `Window.open` begins at once. `Time.dt` is read each time,
so a program that changes it changes its pace.

Nothing about a tick depends on that wait. `Time.tick`, `Time.dt` and
`Time.elapsed` are the simulated values they always were, and a program
with no window open, a headless one, or a driven one ticks as fast as it
can.

**A frame for each tick.** When a tick ends while a window is open, the
window shows a new **frame**: the picture of the world after that tick, of
`Window.pixel_width` by `Window.pixel_height` pixels. In 0.7 the picture is
`Window.background`. Showing a frame never waits for the display, so the
display's rate changes neither the pace nor the output of a program, and a
window that is hidden, minimized or on another workspace costs the program
nothing: its ticks go on. The frames are drawn on the device that GPU
systems use; a window needs one (see [Without a display](#without-a-display)).

**Determinism.** A window brings its sizes, its closing, its focus and the
keyboard and the mouse into a program from outside, and all of them enter
only when a tick begins. A headless run takes all of them from its
commands alone, so it prints the same output on every machine and for
every run; a driven window on the desktop takes its focus, the keyboard
and the mouse from its commands too. Everything the desktop can tell a program of the keyboard and
the mouse is something a command can say. A frame of 0.7 is the same
picture, pixel for pixel, on every device: each byte of the background is
cleared as the linear value whose sRGB encoding it is.

### Without a display

`MIDNIGHT_WINDOW` is a comma-separated list of words, read when the
program starts, by programs with a window only:

| Word       | Meaning                                                       |
| ---------- | ------------------------------------------------------------- |
| `headless` | Show no window. `Window.open` gives a window of exactly the size asked for, at a scale of 1, that exists in the program only. Ticks are not held to real time. |
| `info`     | Print one line to standard error when a window opens: its sizes, and where it is shown (`wayland`, `x11` or `cocoa`) and how frames reach it (`mailbox`, `immediate` or `fifo`), or `headless`. A driven window adds `midnight: window: input from commands`. A shown window that is not driven prints `midnight: window: focused` each time it gains the focus and `midnight: window: pointer inside` each time the pointer enters it. |

A word that is not in the table is reported once, when the program starts.
A headless window touches no desktop, and no device until a frame is
captured. Nothing closes a headless window but its commands: without
`MIDNIGHT_DRIVE`, a program that waits for it with `World.run` runs until
it is stopped from outside.

Without `headless`, a program that cannot show a window stops at
`Window.open` with a run-time error that says why and names
`MIDNIGHT_WINDOW=headless`:

```
drift.12am:2: runtime error: Window.open: no window can be shown: no display was found; set MIDNIGHT_WINDOW=headless to run without one
```

The reasons are: `the program was built without the window flags`,
`` MIDNIGHT_GPU has `off` ``, `no display was found`, `no device can draw
to the desktop`, `DEVICE can't draw to the desktop`, and failures of the
window system that name what failed. On Linux a display is found only
through `WAYLAND_DISPLAY`, `WAYLAND_SOCKET` or `DISPLAY`; Wayland is tried
before X11.

`MIDNIGHT_DRIVE=FILE` gives a program with a window its **commands**: one
to a line, read from a file or a named pipe. A line that is empty or
begins with `#` is skipped, and the spaces around a command are. With
commands a program is **driven**: its ticks are not held to real time,
headless or not, it takes its keyboard, mouse and focus from its commands
alone, and before each tick begins, while a window is open, the runtime
carries out commands until one lets the tick run. The table below has the
commands of the window; those of the keyboard and the mouse are in
[Commands for the keyboard and the mouse](#commands-for-the-keyboard-and-the-mouse).

| Command        | Meaning                                                    |
| -------------- | ---------------------------------------------------------- |
| `tick`, `tick N` | Let the next tick, or the next `N` ticks, run.           |
| `resize W H`   | The window becomes `W` by `H` points, each from 1 to 8192, as if resized at the desktop. Headless only. |
| `scale S`      | A point becomes `S` pixels, a decimal number from 0.5 to 4 with at most six places; a pixel size is the size in points times `S`, halves rounded up. Headless only. |
| `close`        | The window is closed, as if at the desktop.                |
| `capture FILE` | Write the last frame to `FILE` as a PNG.                   |

- `resize`, `scale` and `close` take effect in the tick that begins next,
  through the events above. `close` is carried out in its place among the
  commands of the keyboard and the mouse: in the tick it closes, what
  those commands do after it reaches no window. A `tick N` counts every tick of the program
  while it is carried out, window open or not.
- The last frame is the frame of the last tick that ended while the window
  was open, or, before such a tick, the one `Window.open` showed.
  `capture` writes it as a PNG of 8 bits each of red, green, blue and
  alpha, `Window.pixel_width` by `Window.pixel_height`, alpha 255. The
  same pixels always give the same file.
- When the commands run out the window is closed, as by `close`, and from
  then on ticks run freely.
- When the program ends, the commands up to the next `tick` are carried
  out.
- Each command is answered with one line on standard error, when it has
  been carried out: `midnight: drive: ` and the command as it was written.
  For `tick` the line comes when its last tick has finished, and says how
  many ticks have run if the program ended first. For `capture` it adds
  the value `Time.tick` had when the frame was made, its size in pixels and
  the CRC-32 of its pixels, four bytes each, rows from the top:

  ```
  midnight: drive: tick 3
  midnight: drive: capture after.png: 3 ticks, 320x200, crc 5f1d02c9
  midnight: drive: tick 5: the program ended after 2 ticks
  ```

- A command that is not in the table, or has the wrong arguments, a
  `resize` or `scale` in a program that is not headless or that would make
  a frame larger than 8192 pixels on a side, and a `capture` that cannot
  be made (no window is open, no device, the file cannot be written) stop
  the program with status 1 and a line that names the file and the line
  number:

  ```
  midnight: drive: steps.drive:4: can't capture: no window is open
  ```

- A file of commands that cannot be opened stops the program with status
  1 when the first command is wanted.

`MIDNIGHT_GPU=off` means what it says for frames as well: the program uses
no device, so it shows no window and captures nothing. A window that is
shown uses the device the program's GPU systems use, or, in a program
without one, asks for an adapter that can draw to it; a headless window
opens the device at its first `capture`. A failure of the window or its
surface after `Window.open` has returned prints `midnight: window error:
…` and ends the program with status 70, as an error from the device does;
so does a window the desktop makes larger than 8192 pixels on a side.

## Keyboard and mouse

```ruby
Window.open("Steer", 640, 480)

resource Ship
  x: Float = 320
end

system steer
  Ship.x -= 4 if Keyboard.down?(Key::A)
  Ship.x += 4 if Keyboard.down?(Key::D)
  World.stop if Keyboard.pressed?(Key::Escape)
end

system aim
  on MousePressed do |click|
    puts "#{click.button} at #{click.x}, #{click.y}" if click.button == MouseButton::Left
  end
end

World.run
```

A program with a window reads the state of the keyboard and the mouse
from two built-in resources, `Keyboard` and `Mouse`, and receives what a
person did as events. What happens at the desktop enters a program only
when a tick begins, as the window's sizes do, so a tick sees one fixed
state of the keyboard and the mouse. Keys are named by where they are on
the keyboard, not by what the layout prints on them.

The names `Key`, `MouseButton`, `Keyboard`, `Mouse`, `KeyPressed`,
`KeyReleased`, `MousePressed`, `MouseReleased`, `WindowFocused` and
`WindowUnfocused` are built in and can't be declared by a program. A
program that names any of them but `Key` and `MouseButton` has a window,
with everything this section and [The window](#the-window) declare. A
program that names `Key` or `MouseButton` alone has those two enums and
nothing else, as `Color` alone gives the struct.

**`Key`** is an enum with a variant for each key below. A variant names a
**position** on the keyboard: the key that is there on a US keyboard,
spelled as the W3C's `KeyboardEvent.code` spells it, without the `Key`
before a letter. `Key::W` is the key above `Key::S`, whatever the layout
prints on it: on a French keyboard it is the key that types Z, and WASD
lies under the same fingers on every layout.

| Group | Variants |
| ----- | -------- |
| Letters | `A` to `Z` |
| Digits | `Digit0` to `Digit9` |
| Punctuation | `Backquote`, `Minus`, `Equal`, `BracketLeft`, `BracketRight`, `Backslash`, `Semicolon`, `Quote`, `Comma`, `Period`, `Slash`, `IntlBackslash` |
| Whitespace and editing | `Space`, `Enter`, `Tab`, `Backspace`, `Escape`, `Insert`, `Delete` |
| Modifiers and locks | `ShiftLeft`, `ShiftRight`, `ControlLeft`, `ControlRight`, `AltLeft`, `AltRight`, `MetaLeft`, `MetaRight`, `CapsLock`, `NumLock`, `ScrollLock` |
| Navigation | `ArrowUp`, `ArrowDown`, `ArrowLeft`, `ArrowRight`, `Home`, `End`, `PageUp`, `PageDown` |
| Function | `F1` to `F12`, `PrintScreen`, `Pause`, `ContextMenu` |
| Numeric keypad | `Numpad0` to `Numpad9`, `NumpadAdd`, `NumpadSubtract`, `NumpadMultiply`, `NumpadDivide`, `NumpadDecimal`, `NumpadEnter` |

That is 105 keys, those of a full-size ISO keyboard. The enum declares
them in the order of their USB usage numbers, as if written

```ruby
enum Key
  A, B, C, D, E, F, G, H, I, J, K, L, M, N, O, P, Q, R, S, T, U, V, W, X, Y, Z
  Digit1, Digit2, Digit3, Digit4, Digit5, Digit6, Digit7, Digit8, Digit9, Digit0
  Enter, Escape, Backspace, Tab, Space, Minus, Equal, BracketLeft, BracketRight
  Backslash, Semicolon, Quote, Backquote, Comma, Period, Slash, CapsLock
  F1, F2, F3, F4, F5, F6, F7, F8, F9, F10, F11, F12
  PrintScreen, ScrollLock, Pause, Insert, Home, PageUp, Delete, End, PageDown
  ArrowRight, ArrowLeft, ArrowDown, ArrowUp
  NumLock, NumpadDivide, NumpadMultiply, NumpadSubtract, NumpadAdd, NumpadEnter
  Numpad1, Numpad2, Numpad3, Numpad4, Numpad5, Numpad6, Numpad7, Numpad8, Numpad9
  Numpad0, NumpadDecimal, IntlBackslash, ContextMenu
  ControlLeft, ShiftLeft, AltLeft, MetaLeft
  ControlRight, ShiftRight, AltRight, MetaRight
end
```

A key the table does not have never reaches a program. Modifiers are keys
like any other: Shift held is
`Keyboard.down?(Key::ShiftLeft) || Keyboard.down?(Key::ShiftRight)`.

**`MouseButton`** is an enum: `Left`, `Middle`, `Right`, `Back`, `Forward`,
in that order.

**`Keyboard`** is a resource with one instance and no fields. Its members
are three queries:

| Member | Type | Meaning |
| ------ | ---- | ------- |
| `Keyboard.down?(key)` | `Bool` | The key was held when the tick began. |
| `Keyboard.pressed?(key)` | `Bool` | The key went down at least once between the last tick's beginning and this one's. |
| `Keyboard.released?(key)` | `Bool` | The key came up at least once between the last tick's beginning and this one's. |

**`Mouse`** is a resource with one instance:

| Member | Type | Meaning |
| ------ | ---- | ------- |
| `Mouse.x` | `Float` | Points from the left edge of the window. Read-only. |
| `Mouse.y` | `Float` | Points from the top edge of the window. Read-only. |
| `Mouse.inside?` | `Bool` | The pointer is over the window. Read-only. |
| `Mouse.wheel_x` | `Float` | Notches scrolled to the right between the last tick's beginning and this one's. Read-only. |
| `Mouse.wheel_y` | `Float` | Notches scrolled up between the last tick's beginning and this one's. Read-only. |
| `Mouse.down?(button)` | `Bool` | The button was held when the tick began. |
| `Mouse.pressed?(button)` | `Bool` | The button went down at least once between the last tick's beginning and this one's. |
| `Mouse.released?(button)` | `Bool` | The button came up at least once between the last tick's beginning and this one's. |

The argument of a query is any expression of type `Key` or `MouseButton`.
A query is written with its parentheses; `Mouse.inside?` and
`Window.focused?` are written without them. `Keyboard` and `Mouse` are not
values, and assigning to a field of `Mouse` or to `Window.focused?` is a
compile error.

- The position is in the units of `Window.width` and `Window.height`, with
  a fraction where the desktop gives one: on a display with a scale of 1.5
  a point is a pixel and a half, and the pointer can be between two
  points.
- `Mouse.inside?` is true when the last thing the pointer did was move,
  not leave, and its position is within the window's size as it is when
  the tick begins: `0 <= x < width` and `0 <= y < height`.
- When the pointer leaves the window, the position keeps the last value
  the desktop gave, at the window's edge. While a button is held the
  desktop goes on reporting the pointer outside the window, so the
  position can be negative or larger than the window, and `inside?` is
  false.
- The wheel's notches are those of a wheel that clicks; a smooth wheel or
  a touchpad gives fractions of one. The sign is the one the desktop gives
  after the person's own setting: `wheel_y` is positive when the person
  asks to see what is above, whichever movement of the hand their setting
  makes that.

**Events.** What a person did between the last tick's beginning and this
one's reaches the program when the tick begins, before the first system
runs:

```ruby
event KeyPressed
  key: Key
end

event KeyReleased
  key: Key
end

event MousePressed
  button: MouseButton
  x: Float
  y: Float
end

event MouseReleased
  button: MouseButton
  x: Float
  y: Float
end

event WindowFocused
end

event WindowUnfocused
end
```

- The events of one type are in the order they happened. Events of
  different types are received by different `on` blocks, so their order
  relative to each other is that of the program's `on` blocks.
- A key or a button pressed and released between two ticks gives both
  events, and in that tick `pressed?` and `released?` are true and `down?`
  is false. Nothing a person does is lost, however short.
- The position of `MousePressed` and `MouseReleased` is where the pointer
  was at that moment; `Mouse.x` and `Mouse.y` are where it was last.
- A key held down gives one `KeyPressed`, however long it is held. The
  presses a desktop repeats while a key is held do not reach a program.
- **Focus.** When the window loses keyboard focus every key and button
  that is held is released, keys in the order of `Key`, then buttons in
  the order of `MouseButton`: their `KeyReleased` and `MouseReleased` are
  among the tick's events, `released?` is true for them, `WindowUnfocused`
  arrives and `focused?` is false. When the focus comes back,
  `WindowFocused` arrives; a key that is still held is not down until it
  is pressed again, except that the desktop may report a held Shift,
  Control, Alt or Meta key as pressed at that moment. The pointer goes on
  moving over a window without focus.
- **Opening.** A window opens without the focus, with the pointer outside
  it at 0, 0 and nothing held: until a tick begins, `Window.focused?` and
  `Mouse.inside?` are false. A headless or driven window is given the
  focus as the first thing that happens to it, so its first tick begins
  with `WindowFocused` and `focused?` true. A window on the desktop that
  is not driven gets the focus when the desktop gives it. The click that
  gives it the focus reaches the program.
- **Closing.** In the tick that begins with `WindowClosed`, everything
  held is released, as on a loss of focus, and `focused?` and `inside?`
  are false; no `WindowUnfocused` is emitted. `Mouse.x` and `Mouse.y` keep
  their last values until a window opens again.
- At the beginning of every tick, whether a window is open or not, what
  was pressed and released and the wheel are cleared. Before a window is
  opened, and in the ticks after the one that begins with `WindowClosed`,
  nothing is held, pressed or released, and the wheel is 0.
- The fields and the queries change when a tick begins and at no other
  time: during a tick they do not change.
- Emitting one of these events is a compile error.

**Where they can be read.** The queries and the fields are read wherever a
resource field is, in an independent `each` on
[several cores](#several-cores) too: they give a loop no reason to stay on
one core, and their argument is judged as any expression. The fields of
`Mouse` and `Window.focused?` are read in a GPU system; a query there, or
in a function a GPU system calls, is a compile error. Answer it in an
ordinary system and copy it into a resource field:

```ruby
resource Held
  boost: Bool = false
end

system keys
  Held.boost = Keyboard.down?(Key::Space)
end

gpu system move
  each Ship do |ship|
    ship.x += Held.boost ? 8.0 : 4.0
  end
end
```

### Commands for the keyboard and the mouse

A driven program, with a window on the desktop or headless, takes its
keyboard, mouse and focus from the commands of `MIDNIGHT_DRIVE` alone:
keys, buttons, motion, the wheel and changes of focus at the desktop do
not reach it. Resizes and closes at the desktop still reach a driven
window that is shown. A headless program without commands has no input.

| Command | Meaning |
| ------- | ------- |
| `key down K` | The key `K` goes down. |
| `key up K` | The key `K` comes up. |
| `key K` | The key `K` goes down and comes up. |
| `mouse move X Y` | The pointer moves to `X`, `Y` in points. |
| `mouse down B` | The button `B` goes down where the pointer is. |
| `mouse up B` | The button `B` comes up where the pointer is. |
| `mouse B` | The button `B` goes down and comes up where the pointer is. |
| `mouse leave` | The pointer leaves the window and keeps its position. Buttons stay as they are. |
| `scroll DX DY` | The wheel turns `DX` notches to the right and `DY` up. |
| `blur` | The window loses keyboard focus. |
| `focus` | The window gets keyboard focus. |

- `K` is the name of a `Key` variant and `B` that of a `MouseButton`
  variant, as the source writes them after `::`.
- A number is an optional `-`, one or more digits, and optionally a point
  followed by one to eight digits. It is read as its digits, as a whole
  number, divided by the power of ten of its places, which gives the
  `Float` nearest to it on every machine; `-0` and its like are 0.0. `X`
  and `Y` are from -8192 to 8192, `DX` and `DY` from -1000 to 1000.
  `scroll 0 0` is allowed and changes nothing.
- A command is carried out, and answered with `midnight: drive: ` and the
  command as it was written, when it is read, as `resize` is; what it
  does reaches the program when the next tick begins, in the order of the
  file.
- A command is judged against the state the commands before it leave, in
  the order of the file: after `key down A`, `key up A` is right before
  the same tick. A headless or driven window begins as "Opening" says,
  with the focus already given; `blur` and `close` leave nothing held for
  the commands after them, and after `close` the window has no focus and
  the pointer is not over it. The commands carried out after the program
  ends are judged the same way.

These stop the program with status 1 and a line that names the file and
the line number, as a wrong command of the window does, and in this order
when more than one applies:

1. a name that is not a key or a button, or a number that is not as
   above or out of range;
2. a `key` command while the window does not have the focus;
3. `key down` of a key that is down, `key up` of a key that is up, and
   the same for buttons;
4. `mouse move` outside the window while no button is held: the desktop
   reports such a pointer as having left, so the line names `mouse leave`;
5. `mouse down` or `mouse B` while the pointer is not over the window and
   no button is held;
6. `mouse leave` while the pointer is not over the window;
7. `blur` without the focus and `focus` with it.

```
midnight: drive: steer.drive:3: `key A` while the window does not have the focus
midnight: drive: steer.drive:7: `mouse move 700 20` is outside the window of 640x480 points while no button is held; a pointer that leaves is `mouse leave`
```

## GPU systems

```ruby
component Body
  x: Float32
  y: Float32
  vx: Float32
  vy: Float32
  mass: Float32
end

SOFTENING = 0.01

gpu system gravity
  each Body do |a|
    ax = 0.0
    ay = 0.0
    each Body do |b|
      dx = b.x - a.x
      dy = b.y - a.y
      d2 = dx * dx + dy * dy + SOFTENING
      pull = b.mass / (d2 * Math.sqrt(d2))
      ax += dx * pull
      ay += dy * pull
    end
    a.vx += ax * Time.dt
    a.vy += ay * Time.dt
    a.x += a.vx * Time.dt
    a.y += a.vy * Time.dt
  end
end
```

A system declared with `gpu system` runs its queries on the GPU, as compute
shaders over the same component data the rest of the program uses. It is
written in ordinary Midnight and takes its turn in the tick like any other
system. The compiler checks that it is data-parallel: that the block of each
`each` can run for every entity at once. Where no GPU can be used, the same
system runs on the CPU and means the same thing.

The body of a GPU system is one or more `each` statements and nothing else.
Each of them is a **GPU block**, and one execution of a block, for one
entity, is a **run**.

### What a GPU block may contain

A value is **plain** when it is a number, a `Bool`, an enum, an `Entity`, or
a struct or component whose fields are all plain: no string and no list, at
any depth. A GPU block works on plain values only. Inside one, these are
allowed:

- local variables, arithmetic, comparison, `&&`, `||` and `!`;
- `if`, `unless`, `case` and the conditional operator;
- `while`, `until`, `loop` and the counting loops, with `break` and `next`;
- constructing structs and components, reading and assigning their fields,
  and calling their methods and operators;
- reading constants, resource fields and `Time`;
- everything under [Numbers](#numbers), and `rand` and `rand(n)`;
- `==` and `!=` on plain values, and `Entity.none`;
- calling functions that follow the rules below;
- one nested `each`, directly inside the block: not inside a loop or a
  branch, and with no `each` inside it.

Everything else is a compile error that names the construct and gives the
reason: strings and lists in any form, `to_s`, `puts` and `print`, `spawn`,
`despawn`, `add` and `remove`, `emit` and `on`, `e[C]`, `has?` and `alive?`,
`World.count`, `World.tick`, `World.run` and `World.stop`, `srand`, `clock`,
`sleep` and `exit`.

**`next`, `break` and `return`.** `next` directly inside a GPU block ends the
run for that entity and keeps what the run has assigned so far. `break`
directly inside a GPU block, and `return` anywhere in one, are errors: runs
have no order, so there is nothing to stop. In a loop inside the block, the
nested `each` included, `break` and `next` belong to that loop as usual. The
nested `each` visits entities in the order an ordinary `each` would.

### What a GPU system may touch

- **Query terms.** The terms of a GPU block's `each`, and of its nested
  `each`, are `Entity` or components whose fields are `Float32`, `Int32`,
  `Int16`, `Int8`, `UInt32`, `UInt16`, `UInt8`, `Bool`, enums, `Entity`, or
  structs of these. A component with a `Float` or `Int` field, at any depth,
  is an error with the hint to declare the field `Float32` or `Int32`. So is
  one with a string or a list. A `UInt32` field may be there, but the block
  cannot use it, as described below. Types named in `with:` and `without:`
  are not restricted.
- **Writes.** A GPU block assigns to its own local variables and to the
  component parameters of its `each`, and to nothing else. The parameters of
  the nested `each`, every `Entity` parameter, resources and `Time` are
  read-only.
- **Reads.** The constants and resource fields it reads are plain.
- **Functions.** A function called from a GPU block takes and returns plain
  values, does not recurse, directly or through other functions, and
  contains no `each` and nothing else that a GPU block may not contain. It
  may use `return`, and it may read constants, resource fields and `Time`.
  The methods and operators of a struct are functions in this sense. A
  diagnostic inside such a function carries a note that names the GPU system
  and the call that reaches it.
- **`UInt32`.** A `UInt32` field itself cannot be used in GPU code: the
  32-bit `Int` there is signed and cannot hold every value of one. In a GPU
  system, and in a function called from one, it is an error to read a
  `UInt32` field, to assign to one, and to construct a struct that has one
  among its own fields, also when the constructor is the default of another
  struct's field. The hint is to declare the field `Int32`. A struct that
  has a `UInt32` field is otherwise a value like any other, and the field
  keeps its value: such a struct can be read from a constant or a resource
  field, copied, passed to a function and returned, compared with `==`, and
  put inside another struct, and its other fields can be read and assigned.
  `Config.cell.age` is fine when `Cell` also has a `UInt32` field `mask`,
  and `Config.cell.mask` is the error. The same holds for a query term: a
  component with a `UInt32` field can be one, the block reads and assigns
  its other fields and may copy a struct inside it whole, and the `UInt32`
  field of an entity keeps its value while a GPU block works on the rest.

### Snapshot semantics

The block of a GPU `each` runs once for every matching entity, as if all the
runs happened at once.

- A run reads and writes the components of its own entity through the
  parameters of the block's `each`, and sees its own writes.
- The parameters of the nested `each` show every matching entity, the run's
  own included, as it was when the block began.
- Resource fields and `Time` are read once, when the block begins.
- The GPU blocks of one system run in order. Each sees what the ones before
  it wrote.

So the result does not depend on the order in which entities are processed.

```ruby
component Cell
  value: Int32
end

gpu system sum_all
  each Cell do |cell|
    total = 0
    each Cell do |other|
      total += other.value
    end
    cell.value = total
  end
end
```

With three cells holding 1, 2 and 3, every cell holds 6 after one tick. As
an ordinary system the same code would leave 6, 11 and 20, because each
entity would see what the ones before it wrote.

`World.tick` returns when the GPU has finished the work the tick gave it, so
a program that times its ticks with `clock` measures that work.

### Arithmetic at 32 bits

In a GPU block, and in a function while a GPU block calls it, `Float` is a
32-bit IEEE 754 number and `Int` is a 32-bit signed integer that wraps on
overflow. The source does not change: the same `def` computes at 64 bits
when ordinary code calls it and at 32 bits when a GPU block does.

- **Literals.** A `Float` literal is rounded to 32 bits, and one beyond the
  range is an infinity. An `Int` literal outside -2,147,483,648 to
  2,147,483,647 is a compile error, in a GPU block, in a function one calls,
  and in a field default that a constructor there uses, except where a
  `Float` is expected: there it is a `Float` literal.
- **Values from outside.** Constants, resource fields and `Time` are
  converted as they are read. A `Float` is rounded to 32 bits. An `Int`
  must fit in 32 bits:
  - An `Int` constant that does not fit is a compile error where GPU code
    reads it.
  - An `Int` resource field, and each `Int` field of a struct read from a
    resource field or a constant, is checked when the `each` begins. One
    that does not fit is a runtime error; see
    [Runtime errors in a GPU system](#runtime-errors-in-a-gpu-system).
    A field declared `Int32` or smaller always fits.
  - `Time.tick` is the exception: it wraps.
- **`Float` to `Int`.** `to_i`, `floor`, `ceil` and `round` give the nearest
  32-bit `Int` when the `Float` is out of range, and 0 for NaN.
- **Storing.** Every `Int` fits an `Int32` field. A value stored in an
  `Int8`, `Int16`, `UInt8` or `UInt16` field must fit, as in ordinary code.
- **`/` and `%` on `Int`** round toward negative infinity and take the sign
  of the divisor, as at 64 bits. The smallest `Int` divided by -1 wraps to
  itself, with remainder 0.
- **`**` on `Int`** wraps.
- **`**` on `Float`.** Anything to the power 0 is 1, and so is 1 to any
  power and -1 to an infinite one. A square, `x ** 2.0`, is `x * x`. A
  positive base uses the power function of the GPU, or of C on the CPU,
  which need not be exact: `6.0 ** 3.0` may be 215.99994 on a GPU. A zero base gives 0 for a
  positive exponent and `Infinity` for a negative one. A negative base gives
  NaN unless the exponent is a whole number; then the result is the power of
  the base without its sign, negated when the exponent is odd.
- **`%` on `Float`** is `a - b * trunc(a / b)`, to which `b` is added when
  the result is not zero and its sign differs from the sign of `b`. It is
  NaN when `b` is zero. Every step is rounded to 32 bits, so this is not the
  exact remainder that `%` gives at 64 bits. The product `b * trunc(a / b)`
  is close to `a` and is rounded like `a`, to about one part in 16 million,
  which need not be small beside `b`: with `a` 1000000.3125 and `b` 0.1 the
  product rounds to `a` and the result is 0.0, where the exact remainder is
  about 0.098. Once `a / b` reaches 16,777,216 the quotient has no
  fractional part left to cut off and the result carries no information. A
  GPU may also combine the multiplication and the subtraction, which gives
  the exact remainder in the example, or round the division differently,
  which can move a result by one `b` when `a / b` is close to a whole
  number.

### Runtime errors in a GPU system

The operations that can fail in ordinary code can fail in a GPU system: `/`
and `%` by zero on `Int`, an `Int` raised to a negative power, `rand(n)`
with `n <= 0`, and storing an `Int` that does not fit in an `Int8`, `Int16`,
`UInt8` or `UInt16` field.

A run cannot stop the others. A run that fails keeps going, with 0 as the
result of the failed operation (a failed store keeps the low bits of the
value), and every other run finishes. The program then stops, when the block
ends, with the usual form of message and status 1. Nothing after the block
runs: not the next block of the system, not the next system.

When several operations fail, in one run or in different ones, the error
reported is that of the operation that comes first in the source text: by
file, in the order the files were loaded, then by position in the file. An
operation in a function counts where the function is written, not where it
is called. The message has the file and line and names neither the entity
nor the value:

```
swarm.12am:41: runtime error: value does not fit in UInt8
```

The messages are `division by zero`, `modulo by zero`, `negative exponent`,
`rand: the argument must be positive` and `value does not fit in` followed
by the field's type.

One error comes before any run. When an `each` begins, every `Int` the block
reads from outside, itself or through the functions it calls, must fit in
32 bits: a resource field, and each `Int` field of a struct read from a
resource field or a constant, whichever of the struct's fields the block
uses. If one does not, the program stops there with status 1, before the
block has run for any entity, and whether or not a run would have used the
value. The message has the line of the `each` and names the value:

```
swarm.12am:38: runtime error: Config.limit is 5000000000, which does not fit in the 32-bit Int of a GPU system
```

### Random numbers in a GPU system

`rand` and `rand(n)` in a GPU system do not use the generator of the rest of
the program, and drawing them does not advance it. Every run has a sequence
of its own, which depends on the seed, the block, the tick, the entity, and
how many numbers the run has drawn so far, in the block and in the functions
it calls. A run therefore draws the same numbers whatever order the runs
happen in, on a GPU and on the CPU.

All of it is arithmetic on unsigned 32-bit integers that wrap. With `L` the
integer hash known as lowbias32:

```
L(x):     x ^= x >> 16;  x *= 0x7feb352d
          x ^= x >> 15;  x *= 0x846ca68b
          x ^= x >> 16
key     = L(L(L(L(L(seed) ^ block) ^ tick) ^ index) ^ generation)
h       = L(key ^ k)        for the run's draw number k, counted from 0
rand    = (h >> 8) / 16777216
rand(n) = h % n
```

- `seed` is the seed last given to `srand`, or the one the program starts
  with, as its low 32 bits exclusive-or its high 32 bits. It is read when
  the block begins.
- `block` is the number of the GPU block. The GPU blocks of a program are
  numbered from 0 in the order they run in a tick.
- `tick` is `Time.tick`.
- `index` and `generation` are the entity's, as `to_s` shows them:
  `Entity(3v1)` has index 3 and generation 1.

`rand(n)` takes a remainder, so its values are not exactly equally likely
unless `n` divides 4,294,967,296. A `rand(n)` that fails draws no number.

### Determinism

- On the CPU a GPU system gives the same result on every run, in release
  and in debug builds, and for every number of threads.
- A GPU gives the same result from run to run on the same machine and
  driver.
- A strict GPU system gives the same results on every device and on the
  CPU, in both build modes, to the last bit. See
  [Strict GPU systems](#strict-gpu-systems). The rest of this list is about
  GPU systems that are not strict.
- Between a GPU and the CPU, or between two different GPUs, `Float` results
  agree closely but not to the last bit. `+`, `-` and `*` on their own
  are rounded as on the CPU. Beyond that, a GPU may:
  - compute `/`, `Math.sqrt` and `1.0 / Math.sqrt(x)` to within 2 units
    in the last place, the other `Math` functions and `**` less exactly
    still, and `Math.sin`, `Math.cos` and `Math.tan` closely only for
    arguments between -π and π;
  - combine a multiplication and an addition or subtraction into one
    operation with a single rounding, and in a sum of products choose
    which product to combine;
  - rearrange arithmetic as exact algebra allows, for example computing
    `a * b + b * c` as `b * (a + c)`;
  - treat a subnormal number, one of magnitude below about 1.18e-38, as
    zero of the same sign, whether it is a value the block reads, a
    result it stores or a step in between: with `x` equal to 1.0e-20,
    `x * x * 1.0e20` may be 0.0 on a GPU and 1.0e-20 on the CPU;
  - give 0 for a division by a number of magnitude 2 ** 126 (about
    8.5e37) or more, and NaN for an infinity divided by one.

  Compare such results with a tolerance. Where a result is the difference
  of nearly equal values, such as `a * b - c` with `a * b` close to `c`,
  the two sides may differ by as much as the rounding of the values that
  were subtracted, which can be far larger than the result itself.
- `Int` arithmetic, `rand` and `rand(n)` give the same results everywhere,
  strict or not.
- On a GPU, a comparison with NaN and a `Math` function outside its domain
  (`Math.sqrt` of a negative number, `Math.log` of zero or less) may give
  any result: for a NaN, `x != x` is true on the CPU and may be false on a
  GPU. `nan?` is reliable everywhere.

### Strict GPU systems

```ruby
strict gpu system gravity
  each Body do |a|
    # the block of the gravity example above
  end
end
```

A GPU system declared with `strict gpu system` gives the same bits on every
device it runs on and on the CPU. Everything a GPU system may and may not
contain still holds. `strict system name` without `gpu` is an error with the
hint ``write `strict gpu system name` ``.

In a strict GPU system, and in a function while one calls it, `Float` is
the 32-bit `Float` of [Arithmetic at 32 bits](#arithmetic-at-32-bits) with
these rules:

- **Subnormals count as zero.** An operand of `+`, `-`, `*`, `/`, `%`,
  `Math.sqrt`, a comparison or a conversion whose magnitude is below
  2 ** -126 (about 1.18e-38) is zero with its sign. A result of `+`, `-`,
  `*`, `/` or `Math.sqrt` is rounded to 24 bits first; if it is then below
  2 ** -126 in magnitude, it is zero with its sign. So a product whose exact
  value lies just below 2 ** -126 and rounds to it is 2 ** -126, and
  `1.0e-30 * 1.0e-10` is 0.0. A value that is only copied, stored, passed or
  returned keeps its bits, and so do `-x` and `x.abs`, which change only the
  sign.
- **`+`, `-` and `*`** are each rounded to nearest, ties to even, on their
  own. They are never combined into one operation with a single rounding,
  and never rearranged: `a * b + b * c` is two products and a sum, and
  `x - x` is NaN when `x` is an infinity.
- **`/` and `Math.sqrt`** are correctly rounded, for every operand.
- **`%`** is the formula of [Arithmetic at 32 bits](#arithmetic-at-32-bits),
  every step under these rules, and so is `x.round(digits)`.
- **Comparisons** are IEEE: with a NaN, `==`, `<`, `<=`, `>` and `>=` are
  false and `!=` is true. `-0.0 == 0.0`, and a subnormal equals zero.
  `min`, `max` and `clamp` follow from them.
- **`n.to_f`** is correctly rounded, ties to even. `to_i`, `floor`, `ceil`
  and `round` are as at 32 bits, with a subnormal operand counting as zero.
- **`x ** 2.0`** is `x * x`. Every other `**` with a `Float` result is an
  error, and so are `Math.sin`, `Math.cos`, `Math.tan`, `Math.atan2`,
  `Math.exp` and `Math.log`, whose results differ between devices:

  ```
  error: `Math.sin` cannot be used in a strict GPU system
  ```

  In a function the message says `in a function called from a strict GPU
  system` and carries the usual note naming the GPU system and the call.
- **NaN.** Which NaN a result is cannot be seen, as everywhere.

A function called from a strict GPU system and from one that is not means
the strict thing in the first and the ordinary thing in the second, as the
same `def` already computes at 64 bits in ordinary code and at 32 in a GPU
block. Ordinary code, and GPU systems that are not strict, keep subnormals
on the CPU.

**When to use it.** `strict` trades speed for results that do not depend on
the machine. Use it for a GPU system when:

- the program must replay a run exactly on another machine: a simulation
  stepped in lockstep over a network, a recorded run played back, a saved
  world that must evolve the same way when loaded elsewhere;
- the GPU run must match the CPU run, because the same program may run
  without a GPU, or because a machine without one checks the results;
- the system is chaotic, as gravity among many bodies is, so that a
  difference in the last bit grows until the runs no longer look alike;
- a test compares the output exactly: a program whose GPU systems are all
  strict needs no tolerance.

Leave it off when the results only need to agree closely: motion, particles
and other effects whose small differences nobody sees, and statistics read
with a tolerance. Leave it off when the system needs `Math.sin`, `Math.cos`,
`Math.tan`, `Math.atan2`, `Math.exp`, `Math.log` or a `**` other than a
square, which strict code cannot use.

Things to know when writing one:

- The guarantee covers what strict code computes from the same inputs. A
  value a non-strict GPU system wrote, or one an ordinary system computed
  with a `Math` function, can differ between machines, and a strict system
  that reads it carries the difference on. Make every GPU system strict that
  writes the state which must agree, and keep ordinary code that writes it
  to `+`, `-`, `*`, `/`, `Math.sqrt` and comparisons.
- Ordinary code computes at 64 bits and keeps subnormals; the strict
  system's results are 32-bit values with subnormals flushed. A strict
  system and ordinary code that compute the same formula do not give the
  same bits.
- Numbers below about 1.18e-38 are zero in strict code. Scale quantities so
  that they stay well above that.
- It costs time. On an RTX 5090, gravity among 16,384 bodies takes 7.2 ms
  per tick as a strict GPU system and 0.83 ms as an ordinary one. Most of
  the difference is `/` and `Math.sqrt`, which strict code computes as
  integer arithmetic; a system that only adds and multiplies costs much
  less extra. A smaller GPU pays a larger factor. On the CPU a strict
  block takes about 1.6 times as long as one that is not strict, and the
  start-up check adds a few milliseconds to starting the GPU.

**The start-up check.** The rules rest on how drivers compile shaders, which
no specification promises. So a program with a strict GPU system checks the
device when it opens it: it runs a fixed piece of strict code, chosen to
fail where a driver fuses, rearranges, folds or rounds differently, on the
device and on the CPU, and compares the bits. When they differ, every
strict block runs on the CPU, with the same results, and the other GPU
systems still use the device. See [Without a GPU](#without-a-gpu).

### Without a GPU

A GPU system does not need a GPU. When the program finds no device it can
use, every GPU block runs on the CPU instead, split across the cores (see
[Several cores](#several-cores)), and means what this section says: the
snapshot, the 32-bit arithmetic, the random numbers and the errors are the
same. A single block
also runs on the CPU when it does not fit the device, for example when one
of its columns is larger than the largest buffer the device binds. The
blocks of strict GPU systems run on the CPU when the device fails the
start-up check of [Strict GPU systems](#strict-gpu-systems), and follow the
strict rules there, subnormals included. The README says how the device is
chosen and how to see where each block runs.

### The transfer plan

The data of a component that some GPU block names as a term can be in two
places, CPU memory and GPU memory, and at any moment one copy or both are
current, table by table. A GPU block needs the GPU copy of every component
whose query parameter it uses, in the tables its queries reach, and leaves
the ones it writes current on the GPU only. Ordinary code needs the CPU
copy, and leaves what it writes current on the CPU only. A term whose
parameter the body never uses only picks the tables. `with:` and `without:`
filters, `has?` and `World.count` need neither copy.

When the copy that is needed is not current, the data is copied first: an
**upload** to GPU memory or a **download** from it. How much is copied
depends on what the code does:

| Ordinary code                                       | Needs in CPU memory        | Moves, when only the GPU copy is current |
| --------------------------------------------------- | -------------------------- | ---------------------------------------- |
| A query term whose parameter is read                | the array, in each table   | a download of the array                  |
| A query term whose parameter is assigned            | the array                  | the download; the array is uploaded later only if an assignment ran |
| A query term whose parameter is overwritten         | nothing                    | nothing                                  |
| `e[C]`, read                                        | the entity's row           | one row down                             |
| `e[C].f = v`, `e[C].f += v`                         | the entity's row           | one row down, and one row up for the next block |
| `e[C] = v`, and `e.add(C(v))` when `e` has a `C`    | nothing                    | one row up for the next block            |
| `spawn`                                             | nothing                    | the new row up                           |
| `despawn`                                           | nothing                    | at most one row up per component         |
| `add`, `remove` that move the entity to another table | the entity's row         | one row down, the row up in its new table, at most one more row up, per component |

A row up is also what an `e[C]` write, a `spawn` and the others cost when
both copies are current. When rows would cost more than the array, the
array moves instead: see [What a GPU system costs](#what-a-gpu-system-costs).
Reading an array leaves both copies current. Bringing back rows does not:
the GPU copy is still the only current one, and the next query term that
reads the component downloads the array.

**Overwritten.** A parameter of an `each` is overwritten when the block
begins by assigning it whole, or by assigning every one of its fields,
before anything can read it:

```ruby
each Force do |force|
  force = Force(0.0, 0.0)
end

each Position, Screen do |pos, screen|
  scale = Config.scale
  screen.x = pos.x * scale
  screen.y = pos.y * scale
end
```

Exactly:

1. The body starts with a run of plain statements (declarations of locals,
   assignments, calls) that are not inside a branch or a loop. Among them
   the parameter is assigned as a whole, or each field of the component is
   assigned as a whole. `p.v.x = 1` assigns part of the field `v` and does
   not count.
2. No expression in that run, up to the last of those assignments, reads
   the parameter. A compound assignment reads.
3. In an ordinary `each`, nothing in the whole body can reach the component
   another way: no `e[C]` with that component, no query with it as a term
   whose parameter is used, and no call of a function that contains either,
   directly or through other functions. The body has no `break` that leaves
   this `each` and no `return`: the entities not visited would keep values
   that were never brought back.
4. In a GPU block, the nested `each` does not use the component.

What the component held before is then never needed: a GPU block that
overwrites it needs no upload, and an ordinary loop no download. This is a
rule about cost; a program means the same whether or not a parameter is
overwritten.

**The plan.** Systems run in a fixed order, so the transfers follow from
the source. `midnight plan FILE` prints the transfers of one tick, as ticks
are once the program has settled, one line per transfer in the order they
happen:

```ruby
component Position
  x: Float32
  y: Float32
end

component Velocity
  dx: Float32
  dy: Float32
end

component Age
  ticks: Int
end

gpu system move
  each Position, Velocity do |pos, vel|
    pos.x += vel.dx * Time.dt
    pos.y += vel.dy * Time.dt
  end
end

system reap
  each Entity, Age do |e, age|
    age.ticks += 1
    e.despawn if age.ticks > 3
  end
end

system report
  each Position do |pos|
    puts pos.x
  end
end

spawn Position(0.0, 0.0), Velocity(1.0, 1.0), Age(0)
5.times do
  World.tick
end
```

```
upload    Velocity  reap -> move     rows on despawn
download  Position  move -> report
```

A line gives the direction, the component, and two systems: the one that
left the other copy as the only current one, or left rows to send, and the
one that needs the copy. A strict GPU system is named `strict name`. A line
stands for one transfer per table that both systems reach.

- **A line with no mark** copies the whole array, in every tick. Here
  `report` reads what `move` wrote.
- **A line marked `rows`** moves only the rows concerned: the row an `e[C]`
  names, the rows a structural change adds or moves. It moves the whole
  array when that is cheaper, and nothing in a tick where no row changed.
- **`rows on spawn`, `rows on add`, `rows on remove`, `rows on despawn`**
  are `rows` lines that happen only in a tick where that operation ran.
  Every line a structural change causes is a `rows` line. When a system
  makes several kinds of structural change, the mark names the first that
  touches the component.
- **`top level`** stands for top-level code that runs between ticks: the
  body of a loop that calls `World.tick` or `World.run`. It is the last
  step of a tick. Code before and after such a loop runs once and is no
  part of a tick.

Here `reap` despawns. `Position` is current only on the GPU, so the row
that takes the despawned entity's place is moved in GPU memory and nothing
travels. `Velocity` is current on both sides: the row is moved in CPU
memory and that one row is sent with the next `move`. `report` downloads
`Position` in every tick. `Age` has no GPU copy and never appears.

An empty plan means that the data the GPU systems use stays in GPU memory.
The plan has no upload of a component a GPU block overwrites and no
download of one an ordinary loop overwrites.

The plan has limits:

- Every statement of every system is taken to run, so a transfer that an
  `if` makes rare is listed like one that happens in every tick. When the
  program runs, an `if` makes every kind of transfer rare: an `e[C]` or a
  structural change that does not run moves nothing, and a query `each`
  causes an upload only after a tick in which an assignment through the
  parameter ran.
- It is worked out for two ticks, one in which no structural change runs
  and one in which every structural change runs. A tick in which only some
  of them run is not listed.
- Top-level code counts only inside a loop that ticks. Ticks written out
  one after another, with code between them, are not followed.
- The tables are those the program's `spawn`s make and its `add`s and
  `remove`s make of them; a query reaches a table through its terms and its
  `with:` and `without:`. Two queries that can share no table transfer
  nothing between them. At run time only the tables that have rows, or
  whose rows changed, transfer anything.
- The first ticks, in which everything a GPU block needs is uploaded for
  the first time, are not listed. Neither are the entity handles a block
  needs for an `Entity` term or for `rand`, or the copy the nested `each`
  reads, which is made in GPU memory.
- It describes a program that runs on a GPU. On the CPU nothing is
  transferred.

## Several cores

```ruby
system age
  each Entity, Life do |e, life|
    life.left -= Time.dt
    if life.left <= 0.0
      Stats.expired += 1
      e.despawn
    end
  end
end
```

A program uses the cores of the machine for two kinds of loop: a GPU block
that runs on the CPU, and an ordinary `each` whose runs cannot affect one
another, such as the one above. The compiler decides which loops those are.
Nothing in the source says so, and no construct changes its meaning.

**The guarantee.** A program's output, its exit status and the runtime
error it stops with, if any, do not depend on the number of threads. A loop
that runs on several cores gives the result this document defines for it:
for an ordinary `each`, what visiting the entities one after another gives;
for a GPU block, what [Snapshot semantics](#snapshot-semantics) says. Which
loops are split, into how many pieces, and on how many threads, are matters
of cost, as the [transfer plan](#the-transfer-plan) is.

**Words.** A loop is **split** when its rows are divided into **pieces**,
runs of consecutive rows in visiting order, and the pieces are run by
several threads at once. Only a loop that is not inside another loop's run
is split: a GPU block, or an `each` reached when no other `each` is running.
An `each` nested in a block, or in a function called from one, runs where it
is reached, inside the piece.

**GPU blocks.** When a GPU block runs on the CPU (see
[Without a GPU](#without-a-gpu)), it is split. Its meaning already makes
every run separate from every other: a run writes only its own entity's
components, reads the snapshot, draws random numbers from a sequence of its
own, and an error is the earliest in the source text whichever run meets it.

**Independent loops.** An ordinary `each` is split when it is
**independent**: when no run of its block can see what another run does.
Exactly, the block, together with every function it calls, directly or
through other functions, keeps to all of these:

1. **Plain values.** It reads, builds and assigns only plain values, as
   [GPU systems](#what-a-gpu-block-may-contain) defines them. A component
   with a string or a list among its fields can be a term, and a variable
   of the code around the loop can hold one; the block then uses their
   other fields.
2. **Its own rows.** It assigns to the component parameters of this `each`
   and to variables created inside the block. It assigns nothing else,
   apart from rules 4 and 5: no `e[C]`, no parameter of a nested `each`, in
   the block or in a function it calls, no resource field, no `Time.dt`, no
   variable of the code around the loop.
3. **Nothing another run writes.** A component the block reads by `e[C]`,
   or through a parameter of a nested `each`, is not one that this `each`
   assigns through its own parameters.
4. **Sums of whole numbers.** `R.f += x` and `R.f -= x`, where `f` is an
   `Int` field of a resource, and `v += x` and `v -= x`, where `v` is an
   `Int` variable of the code around the loop, are allowed when they stand
   in the block itself, its loops and nested `each`es included, and nothing
   the block reaches reads that field or variable, `x` included, or assigns
   it in any other way. A sum is written with `+=` or `-=`:
   `R.f = R.f + x` is an assignment under rule 2. Several sums into one
   field are fine. `Int` addition wraps, so the total is the same in any
   order.
5. **Queued changes, events and draws.** `despawn`, `add` and `remove`, on
   any entity, `emit`, and the draws of `Draw`. They are collected per piece
   and joined in visiting order when the loop ends, which is the order one
   thread makes them in.
   A component that is added, and an event, are plain values under rule 1.
6. **None of these:** `spawn`; `on`; `rand`, `rand(n)` and `srand`; `puts`
   and `print`; `clock`, `sleep` and `exit`; `World.stop`; `World.tick`,
   `World.run` and `Window.open`, which a function may hold and which stop
   the program when reached inside an `each`; a `break` that leaves this
   `each`; and a
   `return` anywhere in the block, loops inside it included. A `return` in
   a called function belongs to that function.
7. **Device memory.** A component that some GPU block names as a term is
   reached only through this `each`'s own parameters: the block has no
   `e[C]` of it and no nested `each` that uses it through a parameter.

An independent `each` may read its parameters, constants, `Time`, resource
fields, the queries of `Keyboard` and `Mouse`, variables of the code around
it, `e[C]`, `has?`, `alive?`, `World.count`, and the parameters of nested
`each`es, to any depth; it may
use `next`, loops and branches, and call functions, recursive ones
included.

These are rules about cost. A program means the same whether or not a loop
is independent, and a loop that is not independent runs on one core.

```ruby
resource Stats
  expired: Int = 0
  drift: Float = 0.0
end

system move                         # independent
  each Position, Velocity do |pos, vel|
    pos.x += vel.dx * Time.dt
    pos.y += vel.dy * Time.dt
  end
end

system flee                         # independent: reads Position, writes Velocity
  each Position, Velocity, with: Prey do |pos, vel|
    each Position, with: Hunter do |hunter|
      vel.dx += (pos.x - hunter.x) * 0.01
      vel.dy += (pos.y - hunter.y) * 0.01
    end
  end
end

system drift                        # one core: a `Float` sum
  each Velocity do |vel|
    Stats.drift += vel.dx
  end
end

system jitter                       # one core: `rand`
  each Velocity do |vel|
    vel.dx += rand - 0.5
  end
end
```

**When a loop is split.** Each time the loop runs, by the work it has: the
number of rows, times the compiler's estimate of one run. A loop with
little work runs on the thread that reached it, and so does every loop when
the program has one thread. The numbers are fixed, not measured when the
program runs, and the size of a piece depends on the loop and on its rows,
never on the number of threads: a program makes the same pieces on every
machine. [What several cores cost](#what-several-cores-cost) has the
numbers.

**`midnight loops FILE`** prints one line for each `each` that is not
written inside another `each`, in source order:

```
swarm.12am:35  gpu system steer  each Boid                gpu
swarm.12am:41  system age        each Entity, Life        cores
swarm.12am:51  system flee       each Position, Velocity  cores
swarm.12am:60  system drift      each Velocity            one core: `Stats.drift += …` at line 61 is a `Float` sum
swarm.12am:66  system jitter     each Velocity            one core: `rand` at line 67
swarm.12am:72  def nearest       each Position            one core: `return` at line 73
swarm.12am:78  top level         each Entity, Position    one core: `puts` at line 79
```

A line gives the place of the `each`, what it stands in, its terms, and
where it runs. What it stands in is `system name`, `gpu system name`,
`strict gpu system name`, `def name`, `def Type.name` for a method, or
`top level`. Where it runs is one of:

- **`gpu`**: a GPU block. It runs on the device, and in pieces on the
  cores when it runs on the CPU.
- **`cores`**: independent. An `each` in a function is split when the
  function is called from outside any `each`.
- **`one core`**, with the first thing in the block, in source order, that
  breaks a rule, and its line. Where that is in a function, the line names
  the function it is written in and the call in the block that leads
  there: ``one core: `rand` in `pick` (line 120), called at line 68``.

The reasons, one for each way of breaking a rule. Each is printed with its
place, `at line N` or ``in `f` (line N), called at line K``:

| Reason | Rule |
| --- | --- |
| ``uses a `String` `` / ``uses a list`` | 1 |
| ``assigns `e[C]` ``, with the `e[C]` as the source writes it, or `…[C]` when that is long | 2 |
| ``assigns `C` through the nested `each` `` | 2 |
| ``assigns `R.f` `` / ``assigns `Time.dt` `` / ``assigns `v` `` | 2 |
| ``assigns `R.f` at line N; write `R.f += …` for a sum``, where an `Int` field or variable is assigned itself plus or minus something | 2 |
| ``reads `C`, which this loop assigns, by `e[C]` `` / ``… in the nested `each` `` | 3 |
| `` `R.f += …` at line N is a `Float` sum`` / ``… is a sum into a compact field`` | 4 |
| `` `R.f` is summed and also read`` / ``… and also assigned`` | 4 |
| `` `R.f += …` is in a called function: `f` (line N), called at line K`` | 4 |
| `` `spawn` ``, `` `on` ``, `` `rand` ``, `` `srand` ``, `` `puts` ``, `` `print` ``, `` `clock` ``, `` `sleep` ``, `` `exit` ``, `` `World.stop` ``, `` `World.tick` ``, `` `World.run` ``, `` `Window.open` ``, `` `break` ``, `` `return` `` | 6 |
| ``reads `C`, which the GPU system `name` uses, by `e[C]` `` / ``… in the nested `each` `` | 7 |

A sum written with `-=` is printed as `+= …` too. For a nested `each` the
line is that of the nested `each`. The output is the same on every machine
and for every number of threads.

**Threads.** A program with a loop that can be split uses up to one thread
for each processor core it is allowed to run on, the thread that runs the
rest of the program among them. The other threads are created when a loop
is first split. `MIDNIGHT_THREADS=N` sets the number; with 1 no thread is
created and no loop is split. A program with no GPU system and no
independent `each` has no threads and does not read the variable. The
README describes the variable and its other words.

**Runtime errors.** An independent block can fail as any ordinary code
can: `e[C]` on a dead entity or one without the component, division by
zero, a value that does not fit a compact field, a negative exponent. When
a split loop fails, the program stops with the error of the earliest
failing row in visiting order, with the message that row produces: the
error the loop stops with on one thread. Rows after it may have run;
nothing they did can be seen, since the program ends. Errors in a GPU
block follow
[Runtime errors in a GPU system](#runtime-errors-in-a-gpu-system), as
on one thread.

## Events

```ruby
system combat
  each Entity, Attack do |e, attack|
    emit Hit(attack.target, attack.power)
  end
end

system damage
  on Hit do |hit|
    next unless hit.target.alive? && hit.target.has?(Health)
    hit.target[Health].hp -= hit.damage
  end
end
```

`emit` sends an event. It takes effect at once, inside `each` too. The
events a loop on [several cores](#several-cores) emits are in the order one
thread emits them in.

`on` runs its block once for each event of that type that this `on` has not
seen yet, oldest first. The parameter is a read-only copy.

- Every `on` in the source keeps its own position, so each `on` sees each
  event exactly once.
- An `on` that runs after the `emit` sees the event in the same tick. One that
  runs before it sees the event in the next tick.
- An event is discarded at the end of the tick after the one it was emitted
  in. Events emitted from top-level code count as emitted in the next tick.
- Events emitted while an `on` block is running are left for the next time
  that `on` runs. `break` skips the rest of the current batch.

`on` is a loop over pending events, not a callback: it runs when control
reaches it.

## Built-ins

### Output

`puts a, b` writes each argument's `to_s` followed by a newline. `puts` alone
writes an empty line. `print` writes its arguments with nothing added.

Every value has `to_s`:

| Type          | `to_s`                                                  |
| ------------- | ------------------------------------------------------- |
| `Int`         | `42`                                                    |
| `Float`       | The shortest text that reads back as the same number, always with a fractional part: `1.0`, `0.1`, `1e+20`. Exponent form is used below `0.00001` and from `1e+16` up. Also `Infinity`, `-Infinity`, `NaN`. |
| `Bool`        | `true`, `false`                                         |
| `String`      | The string itself.                                      |
| `Entity`      | `Entity(3v1)`: index and generation. `Entity(none)`.    |
| enum          | `Red`                                                   |
| struct, component, event | `Vec2(x: 1.0, y: 2.0)`                       |
| list          | `[1, 2, 3]`                                             |

A string inside a struct or list is shown quoted, with escapes.

### Numbers

| Expression                 | Result                                            |
| -------------------------- | ------------------------------------------------- |
| `n.abs`, `x.abs`           | Absolute value.                                   |
| `n.to_f`                   | `Int` to `Float`.                                 |
| `x.to_i`                   | `Float` to `Int`, toward zero.                    |
| `x.floor`, `x.ceil`, `x.round` | `Float` to `Int`.                             |
| `x.round(digits)`          | `Float` rounded to that many decimal places.      |
| `x.nan?`                   | Whether a `Float` is NaN.                         |
| `v.clamp(lo, hi)`          | `v` limited to the range, for `Int` or `Float`.   |
| `min(a, b)`, `max(a, b)`   | `Int` if both are `Int`, otherwise `Float`.       |

`Math.sqrt`, `Math.sin`, `Math.cos`, `Math.tan`, `Math.atan2(y, x)`,
`Math.exp` and `Math.log` take and return `Float`. `Math::PI` and `Math::E`
are constants.

### Strings

| Expression                  | Result                                           |
| --------------------------- | ------------------------------------------------ |
| `s.length`                  | Number of characters (Unicode code points).      |
| `s.bytesize`                | Number of bytes.                                 |
| `s.empty?`                  | Whether the length is zero.                      |
| `s.upcase`, `s.downcase`    | ASCII letters converted; others unchanged.       |
| `s.include?(t)`, `s.start_with?(t)`, `s.end_with?(t)` | `Bool`.                |
| `s.ljust(n)`, `s.rjust(n)`  | Padded with spaces to at least `n` characters.   |
| `s.to_i`, `s.to_f`          | The number at the start of the string, or zero.  |
| `s + t`, `s * n`            | Concatenation and repetition.                    |

### Lists

| Expression                  | Result                                           |
| --------------------------- | ------------------------------------------------ |
| `xs.length`, `xs.empty?`    |                                                  |
| `xs[i]`, `xs[i] = v`        | Element at an index. A negative index counts from the end. |
| `xs.first`, `xs.last`       |                                                  |
| `xs.push(v)`                | Append.                                          |
| `xs.pop`                    | Remove and return the last element.              |
| `xs.delete_at(i)`           | Remove and return the element at an index.       |
| `xs.clear`                  | Remove every element.                            |
| `xs.include?(v)`            | Whether any element `== v`.                      |
| `xs + ys`, `xs * n`         | Concatenation and repetition.                    |
| `xs.each`, `xs.each_with_index` | Iteration; see [Control flow](#control-flow). |

The elements of a list literal share one type; an `Int` among `Float`s
converts. `[]` takes its type from where it is used and is an error where
nothing supplies one. `push`, `pop`, `delete_at`, `clear` and `xs[i] = v`
need a list that is a place.

### Random numbers, time and exit

| Expression     | Result                                                       |
| -------------- | ------------------------------------------------------------ |
| `rand`         | A `Float` in `[0, 1)`.                                       |
| `rand(n)`      | An `Int` in `[0, n)`. `n` must be positive.                  |
| `srand(seed)`  | Seed the generator with an `Int`.                            |
| `srand`        | Seed the generator from the system clock.                    |
| `clock`        | Real seconds since the program started, as a `Float`.        |
| `sleep(secs)`  | Pause for that many real seconds.                            |
| `exit`, `exit(code)` | End the program with that status (default `0`).        |

The generator starts from the same seed in every run, so a program that does
not call `srand` produces the same output each time. In a GPU system `rand`
and `rand(n)` follow the seed but draw from sequences of their own; see
[Random numbers in a GPU system](#random-numbers-in-a-gpu-system).

## Programs and files

A program is one root file plus the files it requires. A file contains
declarations and statements, in any mix.

- **Declarations** (`struct`, `enum`, `component`, `tag`, `resource`,
  `event`, `def`, `system`) are visible throughout the program, before and
  after the point of definition, in every file. All files share one
  namespace.
- **Top-level statements** run once, in order, when the program starts. The
  program ends when the last one finishes. Top-level variables are local to
  the top level: functions and systems can't see them. State that systems
  need belongs in a resource or on an entity.

`require "physics"` loads `physics.12am`, found relative to the file
containing the `require`. It is allowed at the top level only. A file is
loaded at most once; its declarations join the program and its top-level
statements run at the point of the first `require`. Systems are ordered the
same way: a required file's systems come where its `require` appears.

## What things cost

Midnight stores data so that the common case, a system visiting every entity
with certain components, is a walk over contiguous memory. Knowing the layout
is part of using the language well.

**Layout.** Entities with exactly the same set of components are stored
together in a table. Within a table, each component type has its own array,
and an entity is a row across those arrays. A query visits the tables that
match it and reads only the arrays of the components named as terms.

**Memory per entity** is the size of its components plus 20 bytes. A
component's size is the size of its fields, which the compiler orders to
minimise padding (construction and `to_s` keep the declared order). `Bool` and enum fields take one byte. Tags take none.
Strings and lists take 8 bytes in the component and keep their contents
elsewhere. The size of a component that a GPU block writes is rounded up to
a multiple of 4 bytes, wherever the program runs: a GPU stores whole 32-bit
words, and two entities must not share one. A component that GPU blocks only
read keeps its size.

| Operation                                   | Cost                                    |
| ------------------------------------------- | --------------------------------------- |
| Visiting entities with `each`               | A pass over arrays. The fastest thing in the language. |
| `with:` and `without:` filters              | Decided once per table, not per entity. |
| Reading and writing fields of a query parameter | A memory access.                    |
| `World.count`                               | Adds up table sizes. No entities are visited. |
| `emit`                                      | An append.                              |
| `e[C]`, `e.has?(C)`, `e.alive?`             | Constant time, but each use is a lookup that can land anywhere in memory. |
| `spawn`, `despawn`                          | Constant time per component.            |
| `add`, `remove`                             | Moves the entity's row to another table, copying every component it has. |
| A query nested in a query                   | The product of the two.                 |
| A GPU block                                 | A fixed cost for each table its query matches, whatever the number of entities; less for each further block that runs before ordinary code does. |
| An upload or a download                     | A copy of a whole array, for each table that holds the component. |
| A row moved on its own between CPU and GPU memory | About as much as 25,000 bytes of an array copied whole. |
| Anything brought back from the GPU          | A wait for the GPU, about as much as 400,000 bytes of an array copied whole; everything one loop needs comes back behind one wait. |
| An operation that can fail, in a GPU block  | One word read back from the GPU, and a wait, each time the block runs. |
| An independent `each`, or a GPU block on the CPU, with enough work | Split across the cores. Handing the pieces out costs a few microseconds, more when the threads have been idle. |
| The same, with little work                  | Runs on the thread that reached it, at no cost. |

What follows from this:

- **Keep hot components plain.** A component made only of numbers, `Bool`s,
  enums, entities and structs of those is stored inline, and a loop over it
  can use SIMD. A string or list field puts a pointer in the row.
- **Use compact types for data that exists in bulk.** Halving a field's size
  halves the memory a loop over it must pull through the cache.
- **Split components by use.** Fields that systems read together belong in
  one component; a field that only one rare system needs belongs in its own,
  so the common loops do not carry it.
- **Prefer a query term to `e[C]` inside loops** where the choice exists.
- **Toggle state with a field, not a tag, when it flips often.** Adding and
  removing a tag moves the entity each time. Tags suit state that changes
  occasionally and that queries filter on.

### What a GPU system costs

A GPU does a great deal of arithmetic at once, but the work has to be handed
to it and the data has to be where the work is. Handing over a GPU block
costs about the same for a thousand entities as for a million. A transfer
costs in proportion to the bytes moved. An array that a GPU block names also
takes its size again in GPU memory, and a program with a GPU system spends a
fraction of a second starting the GPU before its first statement runs.

Three kinds of work are worth that:

- heavy arithmetic per entity;
- entity-against-entity work, a nested `each`, where the GPU's advantage
  grows with the number of entities;
- data that stays in GPU memory from tick to tick.

A light system whose components ordinary code also uses in every tick is
not: moving the data costs more than the arithmetic saves.

Measured on an RTX 5090 beside a Ryzen 9 9950X3D:

- A GPU block costs about 0.02 ms each time it runs, whatever the number of
  entities up to a million, and each further block before ordinary code
  runs about 0.003 ms. A block with an operation that can fail costs about
  0.03 ms.
- A transfer costs about 0.04 to 0.06 ms per megabyte, an upload a little
  more than a download. Anything brought back costs a wait of about
  0.02 ms besides.
- One `e[C]` read and one write, one `spawn`, one `despawn` or one `add` of
  a tag a tick, on a table of a million entities that a GPU block keeps in
  GPU memory, adds 0.003 to 0.03 ms to the tick. An ordinary loop that
  reads two components over 64 tables of 100 entities brings back all 128
  arrays behind one wait: 0.15 ms a tick.
- Moving entities (`pos.x += vel.dx * Time.dt`, on two pairs of `Float32`)
  is faster on the GPU than on one CPU thread from roughly 200,000 entities
  when the data stays there: a million take 0.10 ms per tick on one CPU
  thread and 0.03 ms on the GPU. On the sixteen cores a million take
  0.02 ms, which is as fast as the GPU. When an ordinary system reads the
  result in every tick the GPU is slower at every number of entities
  measured, up to four million: a million then take 0.82 ms per tick,
  against 0.51 ms for the same two systems on one CPU thread and 0.44 ms
  on the sixteen cores, where the system that reads adds up a `Float` and
  stays on one.
- Gravity among 16,384 bodies, every body against every other, takes 0.84 ms
  per tick on the GPU, 354 ms on one CPU thread and 24 ms on the sixteen
  cores; among 65,536 bodies, 3.7 ms on the GPU.
- The same gravity as a strict GPU system takes 7.2 ms per tick on the GPU,
  573 ms on one CPU thread and 40 ms on the sixteen cores.

`nix run .#bench-gpu` measures these, and the other costs it lists, on the
machine it runs on; with `MIDNIGHT_THREADS=1` its CPU timings are those of
one thread. [What several cores cost](#what-several-cores-cost) has more
timings on all cores.

**Rows or the array.** Between two blocks that use a column, ordinary code
moves its rows one at a time until they would cost more than the array:
each row moved, and each of the two copies of a row moved inside GPU
memory, counts as 25,000 bytes, each wait that brings rows of the column
back as 400,000, and when the total would pass the size of the array in
that table, the array moves whole instead. So an array under 25,000 bytes
always moves whole, and a row comes back on its own only from an array of
at least 425,000 bytes. The numbers are fixed, not measured when the
program runs: a program moves the same things on every machine. The rows a
table gains count nothing: they are sent in one piece, however many.

What follows from this:

- **Read the plan.** Every unmarked line of `midnight plan` is a copy of
  whole arrays in every tick, and every `rows` download is a wait for the
  GPU. See [The transfer plan](#the-transfer-plan).
- **Split components by side.** A field that ordinary code needs in every
  tick belongs in a component of its own, so that only its array travels.
- **Look at a few entities with `e[C]`, at many with a query.** A query
  term brings back the whole array of every table it reaches; `e[C]` brings
  back one row, behind a wait of its own. So `e[C]` is the cheaper way to
  look at a few entities of a large table, and the query term the cheaper
  way to look at many.
- **Assign a component whole when you compute it.** A loop that overwrites
  a parameter, as [The transfer plan](#the-transfer-plan) defines it,
  needs nothing from the other side, whichever side runs it.
- **Keep operations that can fail out of blocks that run in every tick.**
  Reading the result back makes the program wait for the block. `Float`
  arithmetic cannot fail, and neither can a store into an `Int32` field, a
  store of a value the compiler knows fits the field, or an operation whose
  right-hand number the compiler knows: `/` or `%` by one
  that is not zero, `**` with an exponent that is not
  negative, `rand(n)` with a positive `n`. The compiler knows a literal, an
  `Int` constant made from literals and other such constants with `+`, `-`,
  `*`, `/`, `%` and `**`, and arithmetic on those. It does not know a
  resource field, a field of a struct constant, or a constant made by a
  call.
  A value fits a compact field when the compiler can bound it: from
  literals and known constants, the compact fields it reads, and `+`, `-`,
  `*`, `/` by a positive number, `%` by a number of one sign, `clamp`,
  `min`, `max`, `abs`, unary `-`, `rand(n)` and a conditional's two values,
  when no step can wrap. `(c.small + 1).clamp(-128, 127)` and
  `(c.wide + 1) % 65536` fit; `c.small + 1` does not, and the value of a
  local variable is not followed.
- **Leave small worlds on the CPU.** For a light system on few entities, the
  fixed cost of a block is more than the loop it replaces.
- **A nested `each` reads a copy** of the arrays it names, made in GPU memory
  when their contents have changed. No copy is needed when the query matches
  one table and the block does not write that component. When it matches
  several tables and the component's size is not a multiple of 4 bytes, the
  copy is put together in CPU memory instead.

### What several cores cost

A loop that is split finishes sooner by about the number of cores it uses,
as long as the work is arithmetic and not waiting for memory. Splitting has
a price of its own: the pieces have to be handed to threads, which takes a
few microseconds, and more when the threads have been idle for a while. So
a loop is split only when it has enough work to pay for that.

**How a loop is divided.** The compiler gives every independent `each` and
every GPU block a **weight**, an estimate of the work of one run, in
operations:

- 1 for an arithmetic operation, a comparison, `&&`, `||`, `!`, a read of
  a field of a component or a struct, a store into anything but a whole
  variable, `has?`, `alive?`, and the simple methods of numbers (`abs`,
  `min`, `max`, `clamp`, `floor`, `ceil`, `round`, `to_i`, `nan?`);
- nothing for reading a variable, a constant, a resource field or a field
  of `Time`, and nothing for assigning a whole variable;
- 4 for `e[C]` and `World.count`;
- 8 for `/`, `%`, `**` and a `Math` function, and for `despawn`, `add`,
  `remove`, `emit` and a draw;
- for a branch, its condition and the heaviest of its arms;
- for a loop that is not a query, 8 times its body;
- for a call, the weight of the function, and 64 for one that recurses;
- for a nested `each`, the weight of its block times the rows its query
  has when the loop begins, wherever in the block it stands. In a GPU block
  those are the rows of the snapshot.

A weight is never less than 1. A GPU block is weighed in the same way, on
the form it is compiled to for the CPU, so its numbers differ a little:
every field of its own row that it reads or assigns counts once for the
run, and an `Int` `/` or `%` that can fail counts what its check costs. The
**work** of a loop is its rows times its weight; sums and products stop at
2 ** 62. Each time a loop runs:

- A piece has as many rows as 25,000 divided by the weight, and at least
  one. The rows of a query are numbered through its tables in visiting
  order, and a piece may cross from one table into the next.
- The loop is split when its work is at least 800,000, it has two rows or
  more and the program has more than one thread.
- One thread takes part for every 200,000 of work: at least two, and at
  most the number of threads and the number of pieces.

The `move` of [Several cores](#several-cores) has a weight of 10: its
pieces are 2,500 rows, and it is split from 80,000 rows. A loop of
thirty-one runs that each visit a million rows of a nested `each` is split
too, into thirty-one pieces. These numbers are fixed, not measured when the
program runs: a program makes the same pieces on every machine.

Measured on a Ryzen 9 9950X3D, sixteen cores, per tick:

| Loop                                                           | One thread | Sixteen cores |
| -------------------------------------------------------------- | ---------- | ------------- |
| Gravity among 16,384 bodies, a GPU block on the CPU            | 354 ms     | 23.9 ms       |
| The same as a strict GPU system                                | 573 ms     | 39.9 ms       |
| 100,000 entities that each look at 64 others, a nested `each`  | 1.25 ms    | 0.085 ms      |
| `move`, an ordinary `each` over a million entities             | 0.287 ms   | 0.030 ms      |
| The same `move` as a GPU block on the CPU                      | 0.103 ms   | 0.019 ms      |
| A million entities age and a thousand of them are despawned    | 0.25 ms    | 0.066 ms      |
| `move` over 6,400 entities in 64 tables, which is not split    | 0.0020 ms  | 0.0020 ms     |

An RTX 5090 takes 0.84 ms for the gravity of the first row.
`nix run .#bench-cores` measures these on the machine it runs on.

What follows from this:

- **Read `midnight loops`.** A `one core` line on a loop over many
  entities names the one thing to move out of it.
- **Keep the loop that computes apart from the loop that reports.** A
  `puts`, a `Float` total or a draw from `rand` keeps a whole loop on one
  core; in a second, small loop it costs nothing.
- **Count with `Int`.** A count or a whole-number total into a resource is
  free on several cores; a `Float` total is not.
- **Random numbers per entity belong in a GPU system**, whose `rand` is
  made for runs that happen at once.
- **Read what you do not write.** A loop that reads other entities'
  `Position` and assigns its own `Velocity` is independent. One that reads
  and assigns `Position` is not: split the component, or compute into a
  second component and copy.
- **Queued changes are applied by one thread.** What a split loop queues
  is applied when it ends, one change after another. The loop over a
  million entities in the table above takes 0.023 ms on sixteen cores when
  nothing expires; despawning a thousand adds about 0.04 ms, so the tick
  is 4 times as fast as on one thread, not 10. With a hundred it is 7
  times as fast.
- **Cores do not help a loop that waits for memory.** A light loop over
  arrays larger than the processor's cache runs at the speed of memory on
  one core, and on sixteen.
- **A GPU is still far ahead where it applies.** Gravity among 16,384
  bodies on sixteen cores takes about 30 times as long as on an RTX 5090.

## Errors

**Compile-time errors** stop the build. Each one names the file, line and
column, shows the source line with the offending part marked, and where it
can, suggests a fix:

```
error: unknown component `Postion`
  --> move.12am:14:8
   |
14 |   each Postion, Velocity do |pos, vel|
   |        ^^^^^^^ did you mean `Position`?
```

**Runtime errors** print a message with the source location to standard error
and end the program with status 1:

```
battle.12am:41: runtime error: Entity(7v1) has no Health
```

The runtime errors are:

- reading or writing `e[C]` when the entity is dead or has no `C`,
- a list index out of range, or `pop`, `first` or `last` on an empty list,
- integer division or modulo by zero,
- storing an `Int` that does not fit in a compact integer field or element,
- an `Int` raised to a negative power,
- `rand(n)` with `n <= 0`,
- `World.tick`, `World.run` or `Window.open` called while a tick is in
  progress, or from a function called inside the body of an `each`,
- `Window.open` while a window is open, with a width or height less than 1
  or more than 8192, or when no window can be shown,
- an `Int` that does not fit in 32 bits where a GPU system reads it from a
  resource field or a constant.

The commands of `MIDNIGHT_DRIVE` stop a program with errors of their own;
see [Without a display](#without-a-display) and
[Commands for the keyboard and the mouse](#commands-for-the-keyboard-and-the-mouse).

In a GPU system an error is reported when the `each` it happened in ends,
and its message names no value; the last error in the list is reported
when the `each` begins. See
[Runtime errors in a GPU system](#runtime-errors-in-a-gpu-system).

A loop that runs on several cores stops with the error it stops with on
one thread: that of the earliest failing row in visiting order. See
[Several cores](#several-cores).

There are no exceptions to catch in 0.7.

## Grammar

`NL` is a statement-ending newline. Keywords and punctuation are quoted.

```
program     = { item NL }
item        = declaration | require | constant | statement
require     = "require" STRING
constant    = CONST "=" expr

declaration = struct | enum | component | tag | resource | event | def | system
struct      = "struct" TYPE NL { field NL | def NL } "end"
component   = "component" TYPE NL { field NL | def NL } "end"
resource    = "resource" TYPE NL { field NL } "end"
event       = "event" TYPE NL { field NL } "end"
enum        = "enum" TYPE NL { TYPE { "," TYPE } NL } "end"
tag         = "tag" TYPE
field       = IDENT ":" type [ "=" expr ]
def         = "def" (IDENT | OPERATOR) [ "(" [ param { "," param } ] ")" ]
              [ "->" type ] NL block "end"
param       = IDENT ":" type
system      = [ "strict" ] [ "gpu" ] "system" IDENT NL block "end"
type        = TYPE | "[" type "]"

block       = { statement NL }
statement   = simple [ ("if" | "unless") expr ]
            | if | unless | case | while | until | loop | each | on
simple      = IDENT ":" type "=" expr
            | place ASSIGN_OP expr
            | "return" [ expr ] | "break" | "next"
            | "puts" [ args ] | "print" args | "emit" expr
            | expr

if          = "if" expr then block { "elsif" expr then block }
              [ "else" block ] "end"
unless      = "unless" expr then block [ "else" block ] "end"
case        = "case" expr NL { "when" expr { "," expr } then block }
              [ "else" block ] "end"
then        = NL | "then"
while       = "while" expr NL block "end"
until       = "until" expr NL block "end"
loop        = "loop" do_block
each        = "each" query do_block
on          = "on" TYPE do_block
query       = term { "," term } { "," filter }
term        = TYPE
filter      = ("with" | "without") ":" ( TYPE | "[" TYPE { "," TYPE } "]" )
do_block    = "do" [ "|" IDENT { "," IDENT } "|" ] NL block "end"

expr        = ternary
ternary     = or [ "?" expr ":" expr ]
or          = and { "||" and }
and         = equality { "&&" equality }
equality    = compare { ("==" | "!=") compare }
compare     = additive { ("<" | "<=" | ">" | ">=") additive }
additive    = multiply { ("+" | "-") multiply }
multiply    = unary { ("*" | "/" | "%") unary }
unary       = "-" unary | power
power       = not [ "**" unary ]
not         = "!" not | postfix
postfix     = primary { "." NAME [ "(" [ args ] ")" ] [ do_block ]
                      | "[" expr "]" | "::" TYPE }
primary     = INT | FLOAT | STRING | "true" | "false" | "self"
            | IDENT [ "(" [ args ] ")" ]
            | TYPE [ "(" [ args ] ")" ]
            | "spawn" [ args ] | "spawn(" [ args ] ")"
            | "[" [ expr { "," expr } ] "]"
            | "(" expr ")"
            | if | unless | case
args        = arg { "," arg }
arg         = [ IDENT ":" ] expr
place       = postfix
```

`IDENT` is a lowercase name, `TYPE` and `CONST` are capitalized names, and
`NAME` is any name, keywords included. `OPERATOR` is one of `+ - * / % -@`
and `ASSIGN_OP` is one of `= += -= *= /= %=`. A place that is a bare `IDENT`
not yet in scope creates the variable.
A `do_block` after a call is accepted only for the loop forms listed in this
document. The `NL` after `do |…|`, and the one after the last statement of a
block, may be left out when the block goes on along the same line:
`xs.length.times do |i| xs[i] *= 2 end`.

## Not in 0.7

These are deliberately absent from this version: optional values and `nil`,
maps and sets, ranges and slices, string indexing, user-defined generics,
blocks as values, modules and namespaces, exceptions, input (standard input,
files, command-line arguments), arithmetic at 32 bits outside GPU systems,
system ordering constraints, running several systems at the same time,
relationships between entities, and calling C libraries.

A window leaves these out: drawing anything but the background; sound;
more than one window at a time; closing, resizing, moving or renaming the
window from the program, full screen and an icon; refusing a close the
user asked for; running faster or slower than real time while a window is
open; and reading entities, components or resources over the commands'
pipe.

The keyboard and the mouse leave these out: text input (characters, the
layout's names for keys, dead keys, input methods), key repeat and the
clipboard; lock states other than as keys going down and up; hiding or
changing the cursor, locking the pointer, motion that is not a change of
position, and grabbing the keyboard; double clicks as such, which a
program counts in ticks; touch, pens and controllers, and touch and pens
acting as a mouse; queries of the keyboard and the mouse in a GPU system;
and recording what a person does at the desktop as a file of commands.

A loop on several cores leaves these out: `Float` sums, strings and lists,
`spawn`, `rand` and output. A loop that has one of them runs on one core.

GPU systems leave these out: structural changes, events, output, strings,
lists, `e[C]`, `has?`, `alive?` and `World.count` inside one; collecting a
result across entities, such as a sum or a minimum, into a resource; spatial
structures, so a nested `each` visits every pair; `Math` functions other
than `Math.sqrt`, and `**` other than a square, in a strict GPU system; and
the use of more than one GPU.
