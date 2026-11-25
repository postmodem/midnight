# The Midnight language

This document describes Midnight 0.2: the language core specified by
[`specs/0001-language-core.md`](../specs/0001-language-core.md) and the GPU
systems specified by
[`specs/0002-gpu-systems.md`](../specs/0002-gpu-systems.md). It is the
reference for what a program means. Where the compiler and this document
disagree, one of them has a bug.

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
if loop next on print puts require resource return self spawn struct system
tag then true unless until when while
```

`gpu` is used in one place, before `system`: see [GPU systems](#gpu-systems).

The names `Int`, `Float`, `Bool`, `String`, `Entity`, `World`, `Time` and
`Math`, and the compact type names (`Float32`, `Int8`, `Int16`, `Int32`,
`UInt8`, `UInt16`, `UInt32`), are built in and can't be redefined.

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
error.

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

Blocks are not values in 0.2. `do |…| … end` is accepted only by the loop
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
order.

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

`World.tick` and `World.run` are called from top-level code. Calling either
while a tick is in progress is a runtime error.

The built-in resource `Time` describes the clock of the simulation:

| Field          | Type    | Meaning                                              |
| -------------- | ------- | ---------------------------------------------------- |
| `Time.tick`    | `Int`   | Ticks completed. `0` during the first tick. Read-only. |
| `Time.dt`      | `Float` | Simulated seconds per tick. Starts at `1.0 / 60`. Writable. |
| `Time.elapsed` | `Float` | Simulated seconds completed. Read-only.              |

At the end of each tick, `Time.tick` goes up by one and `Time.elapsed` by
`Time.dt`. Ticks run as fast as they can; nothing waits for real time.

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
  `Int16`, `Int8`, `UInt16`, `UInt8`, `Bool`, enums, `Entity`, or structs of
  these. A component with a `Float`, `Int` or `UInt32` field, at any depth,
  is an error with the hint to declare the field `Float32` or `Int32`. So is
  one with a string or a list. Types named in `with:` and `without:` are not
  restricted.
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
  and `Config.cell.mask` is the error. A component with a `UInt32` field,
  at any depth, still cannot be a query term.

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
  and in a field default that a constructor there uses.
- **Values from outside.** Constants, resource fields and `Time` are
  converted as they are read: a `Float` is rounded to 32 bits and an `Int`
  wraps.
- **`Float` to `Int`.** `to_i`, `floor`, `ceil` and `round` give the nearest
  32-bit `Int` when the `Float` is out of range, and 0 for NaN.
- **Storing.** Every `Int` fits an `Int32` field. A value stored in an
  `Int8`, `Int16`, `UInt8` or `UInt16` field must fit, as in ordinary code.
- **`/` and `%` on `Int`** round toward negative infinity and take the sign
  of the divisor, as at 64 bits. The smallest `Int` divided by -1 wraps to
  itself, with remainder 0.
- **`**` on `Int`** wraps.
- **`**` on `Float`.** Anything to the power 0 is 1. A positive base uses
  the power function of the GPU, or of C on the CPU, which need not be
  exact: `6.0 ** 3.0` may be 215.99994 on a GPU. A zero base gives 0 for a
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
  and in debug builds.
- A GPU gives the same result from run to run on the same machine and
  driver.
- Between a GPU and the CPU, or between two different GPUs, `Float` results
  agree closely but not to the last bit. A GPU is allowed small differences
  in division, in `Math.sqrt` and the other `Math` functions and in `**`,
  and may combine a multiplication and an addition into one operation.
  Compare such results with a tolerance.
- `Int` arithmetic, `rand` and `rand(n)` give the same results everywhere.
- On a GPU, a comparison with NaN and a `Math` function outside its domain
  (`Math.sqrt` of a negative number, `Math.log` of zero or less) may give
  any result: for a NaN, `x != x` is true on the CPU and may be false on a
  GPU. `nan?` is reliable everywhere.

### Without a GPU

A GPU system does not need a GPU. When the program finds no device it can
use, every GPU block runs on the CPU instead, one entity after another on
one thread, and means what this section says: the snapshot, the 32-bit
arithmetic, the random numbers and the errors are the same. A single block
also runs on the CPU when it does not fit the device, for example when one
of its columns is larger than the largest buffer the device binds. The
README says how the device is chosen and how to see where each block runs.

### The transfer plan

The data of a component that some GPU block names as a term can be in two
places, CPU memory and GPU memory, and at any moment one copy or both are
current. A GPU block needs the GPU copy of every component its queries
name, and leaves the ones it writes current on the GPU only. Ordinary code
(a query term, `e[C]`, a structural change) needs the CPU copy, and leaves
what it writes or changes current on the CPU only. When the copy that is
needed is not current, the data is copied first: an **upload** to the GPU
or a **download** from it. Reading leaves both copies current. `with:` and
`without:` filters, `has?` and `World.count` need neither copy.

Systems run in a fixed order, so the transfers follow from the source.
`midnight plan FILE` prints the transfers of one tick, as ticks are once the
program has settled, one line per transfer in the order they happen:

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
```

```
upload    Position  reap -> move     on despawn
upload    Velocity  reap -> move     on despawn
download  Position  move -> reap     on despawn
download  Position  move -> report
```

A line gives the direction, the component, and two systems: the one that
left the other copy as the only current one, and the one that needs the
copy.

- **A line with no mark** happens in every tick. Here `report` reads what
  `move` wrote.
- **A line marked `on spawn`, `on add`, `on remove` or `on despawn`** is what
  a structural change adds. It happens only when that operation has run: a
  download in the same tick, an upload in the next. A structural change
  needs the CPU copy and leaves it the only current one: `spawn` for the
  components it lists, the other three for every component with a GPU copy.
  Here a despawn makes `reap` download `Position`, and `move` upload both
  components in the tick after. In a tick with a marked download, an
  unmarked download of the same component later in the tick does not
  happen: `report` finds the CPU copy current. When a system makes several
  kinds of structural change, the mark names the first that touches the
  component.
- **`top level`** stands for top-level code that runs between ticks: the
  body of a loop that calls `World.tick` or `World.run`. It is the last
  step of a tick. Code before and after such a loop runs once and is no
  part of a tick.

An empty plan means that the data the GPU systems use stays in GPU memory.
`Age` has no GPU copy and never appears.

The plan has limits:

- Every statement of every system is taken to run, so a transfer that an
  `if` makes rare is listed like one that happens in every tick.
- It is worked out for two ticks, one in which no structural change runs
  and one in which every structural change runs. A tick in which only some
  of them run is not listed.
- Top-level code counts only inside a loop that ticks. Ticks written out
  one after another, with code between them, are not followed.
- A line stands for one transfer per table that holds the component, and at
  run time a structural change brings back only the tables whose rows it
  changes.
- The first ticks, in which everything a GPU block needs is uploaded for
  the first time, are not listed. Neither are the entity handles a block
  needs for an `Entity` term or for `rand`, or the copy the nested `each`
  reads, which is made in GPU memory.
- It describes a program that runs on a GPU. On the CPU nothing is
  transferred.

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

`emit` sends an event. It takes effect at once, inside `each` too.

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
| A GPU block                                 | A fixed cost for each table its query matches, whatever the number of entities. |
| An upload or a download                     | A copy of a whole array, for each table that holds the component. |
| An operation that can fail, in a GPU block  | One word read back from the GPU each time the block runs. |

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

- A GPU block costs about 0.04 ms each time it runs, whatever the number of
  entities up to a million.
- A transfer costs about 0.04 to 0.06 ms per megabyte, an upload a little
  more than a download.
- Moving entities (`pos.x += vel.dx * Time.dt`, on two pairs of `Float32`)
  is faster on the GPU from roughly 300,000 entities when the data stays
  there: a million take 0.13 ms per tick on the CPU and 0.04 ms on the GPU.
  When an ordinary system reads the result in every tick the GPU is slower
  at every number of entities measured, up to four million: a million then
  take 0.78 ms per tick, against 0.53 ms for the same two systems on the
  CPU.
- Gravity among 16,384 bodies, every body against every other, takes 1.33 ms
  per tick on the GPU and 356 ms on one CPU thread.

`nix run .#bench-gpu` measures the gravity and the million entities on the
machine it runs on.

What follows from this:

- **Read the plan.** Every unmarked line of `midnight plan` is a copy of
  whole arrays in every tick. See [The transfer plan](#the-transfer-plan).
- **Split components by side.** A field that ordinary code needs in every
  tick belongs in a component of its own, so that only its array travels.
- **Make structural changes sparingly where GPU systems work.** A structural
  change brings every array of the tables it changes back to CPU memory, and
  the next GPU block uploads what it needs again.
- **Keep operations that can fail out of blocks that run in every tick.**
  Reading the result back makes the program wait for the block. `Float`
  arithmetic cannot fail, and neither can a store into an `Int32` field, or
  an operation whose right-hand number the compiler knows: `/` or `%` by one
  that is not zero as a 32-bit value, `**` with an exponent that is not
  negative, `rand(n)` with a positive `n`. The compiler knows a literal, an
  `Int` constant made from literals and other such constants with `+`, `-`,
  `*`, `/`, `%` and `**`, and arithmetic on those. It does not know a
  resource field, a field of a struct constant, or a constant made by a
  call.
- **Leave small worlds on the CPU.** For a light system on few entities, the
  fixed cost of a block is more than the loop it replaces.
- **A nested `each` reads a copy** of the arrays it names, made in GPU memory
  when their contents have changed. No copy is needed when the query matches
  one table and the block does not write that component. When it matches
  several tables and the component's size is not a multiple of 4 bytes, the
  copy is put together in CPU memory instead.

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
- `World.tick` or `World.run` called while a tick is in progress.

In a GPU system an error is reported when the `each` it happened in ends,
and its message names no value; see
[Runtime errors in a GPU system](#runtime-errors-in-a-gpu-system).

There are no exceptions to catch in 0.2.

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
system      = [ "gpu" ] "system" IDENT NL block "end"
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

## Not in 0.2

These are deliberately absent from this version: optional values and `nil`,
maps and sets, ranges and slices, string indexing, user-defined generics,
blocks as values, modules and namespaces, exceptions, input (standard input,
files, command-line arguments), arithmetic at 32 bits outside GPU systems,
system ordering constraints, running systems on several CPU threads,
windows, graphics and audio, relationships between entities, and calling C
libraries.

GPU systems leave these out: structural changes, events, output, strings,
lists, `e[C]`, `has?`, `alive?` and `World.count` inside one; collecting a
result across entities, such as a sum or a minimum, into a resource; spatial
structures, so a nested `each` visits every pair; results that are the same
to the last bit on every GPU and on the CPU; and the use of more than one
GPU.
