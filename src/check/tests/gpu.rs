//! GPU systems: programs that follow the rules check and give the same
//! typed tree as an ordinary system, and the rules reach exactly the code
//! a GPU system runs. `tests/fail/gpu_*.12am` has a program for each
//! diagnostic.

use super::*;

const GRAVITY: &str = "\
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
";

/// A program that uses everything a GPU block may contain.
const ALLOWED: &str = "\
enum Team
  Red
  Blue
end

struct Vec2
  x: Float32 = 0
  y: Float32 = 0

  def +(other: Vec2) -> Vec2
    Vec2(x + other.x, y + other.y)
  end

  def -@ -> Vec2
    Vec2(-x, -y)
  end

  def scale(k: Float) -> Vec2
    Vec2(x * k, y * k)
  end

  def length -> Float
    Math.sqrt(x * x + y * y)
  end
end

struct Wide
  total: Float = 0
  count: Int = 0
end

component Mover
  pos: Vec2
  vel: Vec2
  team: Team
  target: Entity
  alive: Bool = true
  energy: Int32 = 0
  heat: Int16 = 0
  step: Int8 = 0
  kind: UInt16 = 0
  level: UInt8 = 0
end

component Wide64
  value: Float
  name: String
  tags: [Int]
end

tag Frozen

resource Field
  wind: Vec2 = Vec2(1, 0)
  strength: Float = 2
  rounds: Int = 3
  team: Team = Team::Red
  leader: Entity = Entity.none
  active: Bool = true
end

LIMIT = 100
ORIGIN = Vec2(0, 0)

def clamp_speed(v: Vec2, top: Float) -> Vec2
  speed = v.length
  return v if speed <= top
  v.scale(top / speed)
end

def bump(n: Int) -> Int
  step = 0
  n.times do |i|
    step += i % 2
  end
  step + Field.rounds
end

gpu system simulate
  each Entity, Mover, with: Wide64, without: Frozen do |e, m|
    next unless m.alive && Field.active
    next if e == Field.leader || m.target == Entity.none

    wide = Wide(m.pos.x, bump(2))
    push = Field.wind.scale(Field.strength) + ORIGIN
    push = -push if m.team != Field.team
    m.vel += push
    m.vel = clamp_speed(m.vel, LIMIT)
    m.pos = m.pos + m.vel.scale(Time.dt)
    m.pos.x += rand - 0.5

    jitter = rand(6)
    case jitter
    when 0 then m.step = 1
    when 1, 2
      m.step = 2
    else
      m.step = 0
    end
    spin = case m.team
           when Team::Red then 1.0
           when Team::Blue then -1.0
           end
    drag = if m.vel.length > 10 then 0.5 else 1.0 end
    sign = m.pos.x < 0 ? -1 : 1

    while wide.total > 1
      wide.total /= 2
      break if wide.total.nan?
    end
    until wide.count >= 3
      wide.count += 1
      next if wide.count == 2
    end
    loop do
      wide.count -= 1
      break if wide.count <= 0
    end
    1.upto(3) do |i|
      m.energy += i
    end
    3.downto(1) do |i|
      m.energy -= i
    end

    near = 0
    each Entity, Mover do |other, o|
      next if other == e
      gap = (o.pos + -m.pos).length
      break if gap.nan?
      near += 1 if gap < 1
      m.heat = min(m.heat + 1, 100)
    end

    m.energy = max(m.energy, 0).clamp(0, 1000)
    m.level = (m.pos.x.abs.floor % 4 + m.pos.y.ceil.abs % 2).clamp(0, 5)
    m.kind = (spin * drag * sign).round.abs + near.to_f.to_i + (Time.tick % 2)
    angle = Math.atan2(m.vel.y, m.vel.x) + Math::PI * Time.elapsed
    m.vel.x = Math.cos(angle) * Math.exp(-drag) + Math.sin(angle) + Math.tan(0.1)
    m.vel.y = Math.log(m.vel.x.round(2) ** 2 + Math::E)
    m.alive = !(m.energy == 0) && m.vel != Vec2()
  end

  each Mover do |m|
    m.step = 0
  end
end
";

#[test]
fn the_gravity_system_checks() {
    let program = ok(GRAVITY);
    assert!(program.systems[0].gpu);
}

#[test]
fn everything_a_gpu_block_may_contain_checks() {
    let program = ok(ALLOWED);
    assert!(program.systems[0].gpu);
}

#[test]
fn a_gpu_system_is_typed_like_an_ordinary_one() {
    for (text, name) in [(GRAVITY, "gravity"), (ALLOWED, "simulate")] {
        let ordinary = text.replace("gpu system", "system");
        assert!(!ok(&ordinary).systems[0].gpu);
        assert_eq!(system(text, name), system(&ordinary, name));
    }
}

#[test]
fn the_rules_apply_to_gpu_code_only() {
    // Everything a GPU system may not do, in an ordinary system, in a
    // function only ordinary code calls, and at the top level.
    ok("\
component Body
  x: Float32
end
component Wide
  x: Float
  name: String
end
resource Stats
  total: Float = 0
end
def depth(n: Int) -> Int
  return 0 if n <= 0
  depth(n - 1) + 1
end
def report(x: Float) -> String
  puts x
  Stats.total += x
  \"#{x}\"
end
def double(x: Float) -> Float
  x * 2
end
gpu system grow
  each Body do |body|
    body.x = double(body.x)
  end
end
system audit
  big = 4294967296
  each Entity, Wide do |e, wide|
    wide.name = report(double(wide.x)) + depth(3).to_s
    e.despawn if wide.x > big
    return if World.count(Wide) == 0
  end
end
puts depth(2), 4294967296
");
}

#[test]
fn a_function_may_be_called_from_both_kinds_of_system() {
    ok("\
component Body
  x: Float32
end
component Wide
  x: Float
end
def half(x: Float) -> Float
  x / 2
end
gpu system narrow
  each Body do |body|
    body.x = half(body.x)
  end
end
system wide
  each Wide do |wide|
    wide.x = half(wide.x)
  end
end
");
}

#[test]
fn ordinary_errors_come_first() {
    // The GPU rules are checked on a program that is otherwise correct.
    assert_error(
        "\
component Body
  x: Float32
end
gpu system grow
  each Body do |body|
    puts speed
  end
end
",
        "unknown variable or function `speed`",
        "speed",
        "",
    );
}

#[test]
fn a_list_method_on_a_resource_is_one_error() {
    assert_error(
        "\
component Body
  x: Float32
end
resource Log
  seen: [Float] = []
end
gpu system record
  each Body do |body|
    Log.seen.push(body.x)
  end
end
",
        "resource field `Log.seen` can't be read in a GPU system",
        "Log.seen",
        "it is a `[Float]`; a GPU system reads numbers, `Bool`s, enums, entities and structs of those",
    );
}

#[test]
fn a_default_that_does_not_fit_is_reported_once() {
    assert_error(
        "\
component Body
  n: Int32
end
struct Range
  top: Int = 4294967296
end
gpu system clamp
  each Body do |body|
    a = Range()
    b = Range()
    body.n = min(body.n, a.top + b.top)
  end
end
",
        "4294967296 does not fit in 32 bits",
        "4294967296",
        "in a GPU system an `Int` holds -2147483648 to 2147483647",
    );
}

#[test]
fn a_recursive_call_is_reported_at_each_call_in_the_block() {
    let text = "\
component Body
  n: Int32
end
def down(n: Int) -> Int
  return 0 if n <= 0
  down(n - 1)
end
def start(n: Int) -> Int
  down(n)
end
gpu system count
  each Body do |body|
    body.n = down(body.n)
    body.n = start(body.n)
  end
end
";
    assert_errors(
        text,
        &[
            (
                "call to recursive function `down` in a GPU system",
                "down(body.n)",
                "`down` calls itself; a function called from a GPU system must not recurse",
            ),
            (
                "call to `start` in a GPU system reaches recursive function `down`",
                "start(body.n)",
                "`start` calls `down`, which calls itself; a function called from a GPU system must not recurse",
            ),
        ],
    );
}

#[test]
fn a_note_says_which_system_reaches_a_function() {
    let text = "\
component Body
  x: Float32
end
def inner(x: Float) -> Float
  puts x
  x
end
def outer(x: Float) -> Float
  inner(x)
end
gpu system grow
  each Body do |body|
    body.x = outer(body.x)
  end
end
";
    let found = diagnostics(text);
    assert_eq!(found.len(), 1);
    assert_eq!(
        found[0].message,
        "`puts` is not allowed in a function called from a GPU system"
    );
    let note = &found[0].notes[0];
    assert_eq!(
        note.message,
        "GPU system `grow` reaches `inner` from here, through `outer`"
    );
    assert_eq!(
        &text[note.span.start as usize..note.span.end as usize],
        "outer(body.x)"
    );
}

/// Why GPU code does not use a `UInt32` field, as every label about one says
/// it.
const UNSIGNED: &str = "which the signed 32-bit `Int` of a GPU system can't hold";

#[test]
fn a_uint32_field_beside_gpu_code_checks() {
    // A GPU block reads the other fields of a resource that has a
    // `UInt32`, filters by a component that has one, and calls a function
    // that ordinary code calls too. Ordinary code uses the `UInt32` fields
    // and the struct that has one.
    ok("\
struct Stamp
  serial: UInt32
  weight: Int32
end
resource Counter
  total: UInt32 = 0
  step: Int = 3
end
component Body
  n: Int32
end
component Seen
  stamp: Stamp
end
def weigh(stamp: Stamp) -> Int
  stamp.weight * 2
end
def twice(n: Int) -> Int
  n * 2
end
gpu system advance
  each Body, with: Seen do |body|
    body.n += twice(Counter.step)
  end
end
system count
  each Body, Seen do |body, seen|
    Counter.total += twice(body.n)
    seen.stamp = Stamp(Counter.total, weigh(seen.stamp))
  end
end
");
}

#[test]
fn a_struct_with_a_uint32_field_is_a_gpu_value() {
    // GPU code reads the other fields of such a struct in a resource field,
    // in a constant and in a local, copies it, assigns its other field,
    // passes it, takes it back, compares it, calls its method and puts it
    // inside another struct.
    ok("\
component Body
  n: Int32
  same: Bool
end
struct Cell
  mask: UInt32 = 0
  age: Int32 = 0

  def older -> Int
    age + 1
  end
end
struct Grid
  width: Int = 0
  cell: Cell = Cell()
end
resource Board
  grid: Grid = Grid()
  cell: Cell = Cell()
end
HOME = Cell(4000000000, 2)
def newer(a: Cell, b: Cell) -> Cell
  a.age < b.age ? a : b
end
gpu system mark
  each Body do |body|
    cell = Board.cell
    cell.age = body.n
    grid = Grid(body.n, cell)
    grid.cell = newer(HOME, Board.grid.cell)
    body.n += Board.grid.width + Board.grid.cell.age + HOME.age
    body.n += grid.cell.older + newer(cell, HOME).age
    body.same = cell == HOME
  end
end
");
}

#[test]
fn a_uint32_field_is_not_used_in_gpu_code() {
    // Read as a resource field, inside a resource field, inside a constant
    // and on the result of a call; assigned, with `=` and with `+=`; and
    // built, by a constructor and by a default that a constructor uses.
    let it = format!("it is a `UInt32`, {UNSIGNED}; declare it `Int32`");
    let its = format!("its field `mask` is a `UInt32`, {UNSIGNED}; declare it `Int32`");
    let both =
        format!("its fields `low` and `high` are `UInt32`s, {UNSIGNED}; declare them `Int32`");
    assert_errors(
        "\
component Body
  n: Int32
end
struct Cell
  mask: UInt32 = 0
  age: Int32 = 0
end
struct Grid
  width: Int = 0
  cell: Cell = Cell()
end
struct Span
  low: UInt32 = 0
  high: UInt32 = 0
  step: Int = 1
end
resource Board
  grid: Grid = Grid()
  mask: UInt32 = 0
  span: Span = Span()
end
HOME = Cell(1, 2)
def same(cell: Cell) -> Cell
  cell
end
gpu system mark
  each Body do |body|
    body.n = Board.mask
    body.n += Board.grid.cell.mask
    body.n += HOME.mask
    body.n += same(HOME).mask
    cell = HOME
    cell.mask = 3
    cell.mask += 1
    body.n += Cell(1, 2).age
    body.n += Grid(4).width
    body.n += Span(1, 2).step + Board.span.step
  end
end
",
        &[
            ("`Cell` can't be built in a GPU system", "Cell()", &its),
            (
                "resource field `Board.mask` can't be read in a GPU system",
                "Board.mask",
                &it,
            ),
            (
                "field `mask` of `Cell` can't be read in a GPU system",
                "Board.grid.cell.mask",
                &it,
            ),
            (
                "field `mask` of `Cell` can't be read in a GPU system",
                "HOME.mask",
                &it,
            ),
            (
                "field `mask` of `Cell` can't be read in a GPU system",
                "same(HOME).mask",
                &it,
            ),
            (
                "can't assign to `cell.mask` in a GPU system",
                "cell.mask",
                &it,
            ),
            (
                "can't assign to `cell.mask` in a GPU system",
                "cell.mask",
                &it,
            ),
            ("`Cell` can't be built in a GPU system", "Cell(1, 2)", &its),
            ("`Span` can't be built in a GPU system", "Span(1, 2)", &both),
        ],
    );
}

#[test]
fn a_uint32_field_in_a_function_is_reported_with_the_call() {
    let found = diagnostics(
        "\
component Body
  n: Int32
end
struct Cell
  mask: UInt32 = 0
  age: Int32 = 0

  def masked -> Int
    age % (mask + 1)
  end
end
resource Board
  cell: Cell = Cell()
end
gpu system mark
  each Body do |body|
    body.n = Board.cell.masked
  end
end
puts Board.cell.masked
",
    );
    assert_eq!(found.len(), 1);
    assert_eq!(
        found[0].message,
        "field `mask` of `Cell` can't be read in a function called from a GPU system"
    );
    assert_eq!(
        found[0].notes[0].message,
        "GPU system `mark` calls `Cell.masked` here"
    );
}

#[test]
fn a_string_field_is_what_keeps_a_struct_out_of_gpu_code() {
    // A struct with a string and a `UInt32` is reported once, for the
    // string, whichever field comes first.
    let string = "its field `name` is a `String`; the GPU works on numbers, `Bool`s, enums, entities and structs of those";
    assert_errors(
        "\
component Body
  n: Int32
end
struct Tagged
  id: UInt32 = 0
  name: String = \"\"
end
struct Named
  name: String = \"\"
  id: UInt32 = 0
end
gpu system mark
  each Body do |body|
    tagged = Tagged()
    named = Named()
  end
end
",
        &[
            (
                "`Tagged` values are not allowed in a GPU system",
                "Tagged()",
                string,
            ),
            (
                "`Named` values are not allowed in a GPU system",
                "Named()",
                string,
            ),
        ],
    );
}

#[test]
fn a_component_with_a_uint32_field_is_a_term() {
    // The outer and the nested `each` take such a component. The block
    // uses its other fields and copies and compares the struct inside it.
    let program = "\
struct Stamp
  serial: UInt32 = 0
  weight: Int16 = 0
end
component Cell
  id: UInt32
  n: Int32
  made: Stamp
  seen: Stamp
end
gpu system mark
  each Cell do |cell|
    cell.seen = cell.made
    each Cell do |other|
      cell.n += other.n if other.made == cell.made
      LINE
    end
  end
end
";
    ok(&program.replace("LINE", "cell.n += other.made.weight"));

    let it = format!("it is a `UInt32`, {UNSIGNED}; declare it `Int32`");
    assert_error(
        &program.replace("LINE", "cell.n += cell.id"),
        "field `id` of `Cell` can't be read in a GPU system",
        "cell.id",
        &it,
    );
    assert_error(
        &program.replace("LINE", "cell.n += other.made.serial"),
        "field `serial` of `Stamp` can't be read in a GPU system",
        "other.made.serial",
        &it,
    );
    assert_error(
        &program.replace("LINE", "cell.id = 1"),
        "can't assign to `cell.id` in a GPU system",
        "cell.id",
        &it,
    );
    assert_error(
        &program.replace("LINE", "cell.made.serial += 1"),
        "can't assign to `cell.made.serial` in a GPU system",
        "cell.made.serial",
        &it,
    );
}

#[test]
fn a_known_constant_fits_in_32_bits_where_gpu_code_reads_it() {
    let program = "\
component Body
  n: Int32
end
struct Limits
  high: Int = 0
end
BIG = 2 ** 40
EDGE = BIG / 512 - 1
LOW = -EDGE - 1
BELOW = LOW - 1
LIMITS = Limits(BIG)
gpu system clamp
  each Body do |body|
    body.n = max(min(body.n, EDGE), LOW) + LIMITS.high
    LINE
  end
end
puts BIG
";
    // A constant made from one that does not fit is judged by its own
    // value, and a struct constant is not worked out here.
    ok(&program.replace("LINE", "body.n += EDGE / 4"));

    let holds = "in a GPU system an `Int` holds -2147483648 to 2147483647";
    assert_error(
        &program.replace("LINE", "body.n += BIG / 4"),
        "constant `BIG` is 1099511627776, which does not fit in 32 bits",
        "BIG",
        holds,
    );
    assert_error(
        &program.replace("LINE", "body.n = max(body.n, BELOW)"),
        "constant `BELOW` is -2147483649, which does not fit in 32 bits",
        "BELOW",
        holds,
    );
}
