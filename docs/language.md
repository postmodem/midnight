# The Midnight language

This document describes Midnight 0.1, the version specified by
[`specs/0001-language-core.md`](../specs/0001-language-core.md). It is the
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

`gpu` has no use in 0.1; it is reserved for GPU systems.

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
and all arithmetic happens at 64 bits. So `pos.x += vel.dx * Time.dt` is
written the same way whatever the fields' storage types.

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
iteration. This holds for every loop form, including `each` and `on`.

Blocks are not values in 0.1. `do |…| … end` is accepted only by the loop
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
  function that calls itself, directly or through other functions, must
  declare it.
- A function with no parameters is declared without parentheses and may be
  called without them: `greet` or `greet()`. Calls with arguments use
  parentheses: `clamp(x, 0, 1)`. The same holds for methods.
- A local variable hides a function of the same name when the name is used
  bare. `name(…)` is always a function call.
- Functions may be used before they are defined, and may do anything a system
  can: query, spawn, emit, and read or write resources.

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
System names share a namespace with functions.

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
| `Float`       | The shortest text that reads back as the same number, always with a fractional part: `1.0`, `0.1`, `1e+20`. Also `Infinity`, `-Infinity`, `NaN`. |
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
not call `srand` produces the same output each time.

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
minimise padding. `Bool` and enum fields take one byte. Tags take none.
Strings and lists take 8 bytes in the component and keep their contents
elsewhere.

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

There are no exceptions to catch in 0.1.

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
system      = "system" IDENT NL block "end"
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
document.

## Not in 0.1

These are deliberately absent from this version: optional values and `nil`,
maps and sets, ranges and slices, string indexing, user-defined generics,
blocks as values, modules and namespaces, exceptions, input (standard input,
files, command-line arguments), arithmetic at 32 bits, system ordering
constraints, parallel execution of systems, systems that run on the GPU,
graphics, relationships between entities, and calling C libraries.
