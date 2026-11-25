//! Every diagnostic the checker reports: its message, the text it marks
//! and its label.

use super::*;

/// Declarations shared by the tests.
const DECLS: &str = "\
component Position
  x: Float
  y: Float
end
component Velocity
  dx: Float
  dy: Float
end
component Health
  hp: UInt8 = 100
  max: Int = 100
end
component Armor
  value: Int
end
tag Player
tag Frozen
struct Vec2
  x: Float = 0
  y: Float = 0
  def +(other: Vec2) -> Vec2
    Vec2(x + other.x, y + other.y)
  end
  def length -> Float
    Math.sqrt(x * x + y * y)
  end
  def scale(k: Float) -> Vec2
    Vec2(x * k, y * k)
  end
end
enum Team
  Red
  Blue
  Green
end
resource Score
  points: Int = 0
  history: [Int] = []
end
event Hit
  target: Entity
  damage: Int
end
def clamp(v: Float, lo: Float, hi: Float) -> Float
  return lo if v < lo
  return hi if v > hi
  v
end
def greet
  puts \"hello\"
end
";

const FLOAT_TO_INT: &str =
    "a `Float` does not convert to `Int`; use `to_i`, `floor`, `ceil` or `round`";

/// `body` after the shared declarations.
fn with(body: &str) -> String {
    format!("{DECLS}{body}\n")
}

/// Checks that `body`, after the shared declarations, has exactly one
/// error.
#[track_caller]
fn one(body: &str, message: &str, at: &str, label: &str) {
    assert_error(&with(body), message, at, label);
}

/// The message of the first note of the only error, and the text it marks.
#[track_caller]
fn note(text: &str) -> (String, String) {
    let errors = diagnostics(text);
    assert_eq!(errors.len(), 1, "{errors:?}");
    let note = errors[0].notes.first().expect("a note");
    (
        note.message.clone(),
        text[note.span.start as usize..note.span.end as usize].to_string(),
    )
}

#[test]
fn the_shared_declarations_check() {
    ok(DECLS);
}

// ----- unknown names -----

#[test]
fn unknown_variable() {
    one(
        "count = 0\nputs cont",
        "unknown variable or function `cont`",
        "cont",
        "did you mean `count`?",
    );
    one(
        "puts zzzzzz",
        "unknown variable or function `zzzzzz`",
        "zzzzzz",
        "",
    );
    one(
        "x = gret",
        "unknown variable or function `gret`",
        "gret",
        "did you mean `greet`?",
    );
}

#[test]
fn unknown_variable_that_exists_at_the_top_level() {
    one(
        "total = 0\ndef bump\n  puts total\nend",
        "unknown variable or function `total`",
        "total",
        "top-level variables can't be seen from functions and systems; use a resource",
    );
}

#[test]
fn unknown_function() {
    one(
        "puts clmp(1, 0, 1)",
        "unknown function `clmp`",
        "clmp",
        "did you mean `clamp`?",
    );
    one(
        "puts mxa(1, 2)",
        "unknown function `mxa`",
        "mxa",
        "did you mean `max`?",
    );
    one(
        "frobnicate(1)",
        "unknown function `frobnicate`",
        "frobnicate",
        "",
    );
}

#[test]
fn a_method_is_not_called_as_a_function() {
    let text = "\
struct A
  n: Int
  def twice(k: Int) -> Int
    n * k
  end
  def four -> Int
    twice(2)
  end
end
";
    assert_error(
        text,
        "unknown function `twice`",
        "twice",
        "`twice(…)` is always a function call; the method is `self.twice(…)`",
    );
}

#[test]
fn unknown_type() {
    one(
        "v: Vec3 = Vec2()",
        "unknown type `Vec3`",
        "Vec3",
        "did you mean `Vec2`?",
    );
    one(
        "v = Vec3(1, 2)",
        "unknown type `Vec3`",
        "Vec3",
        "did you mean `Vec2`?",
    );
    one(
        "def f(x: Flaot)\nend",
        "unknown type `Flaot`",
        "Flaot",
        "did you mean `Float`?",
    );
    assert_error(
        "struct A\n  b: Quaternion\nend\n",
        "unknown type `Quaternion`",
        "Quaternion",
        "",
    );
}

#[test]
fn unknown_field() {
    one(
        "v = Vec2()\nputs v.z",
        "unknown field or method `z` for `Vec2`",
        "z",
        "did you mean `x`?",
    );
    one(
        "v = Vec2(z: 1)",
        "unknown field `z` for `Vec2`",
        "z",
        "did you mean `x`?",
    );
    one(
        "Score.pionts += 1",
        "unknown field `pionts` for resource `Score`",
        "pionts",
        "did you mean `points`?",
    );
    one(
        "puts Time.dtt",
        "unknown field `dtt` for `Time`",
        "dtt",
        "did you mean `dt`?",
    );
}

#[test]
fn unknown_method() {
    one(
        "v = Vec2()\nputs v.lenght",
        "unknown field or method `lenght` for `Vec2`",
        "lenght",
        "did you mean `length`?",
    );
    one(
        "v = Vec2()\nputs v.scael(2)",
        "unknown method `scael` for `Vec2`",
        "scael",
        "did you mean `scale`?",
    );
    one(
        "puts \"abc\".lenght",
        "unknown method `lenght` for `String`",
        "lenght",
        "did you mean `length`?",
    );
    one(
        "xs = [1]\nxs.psuh(2)",
        "unknown method `psuh` for `[Int]`",
        "psuh",
        "did you mean `push`?",
    );
    one(
        "e = spawn\ne.despwan",
        "unknown method `despwan` for `Entity`",
        "despwan",
        "did you mean `despawn`?",
    );
    one(
        "puts 1.to_i",
        "unknown method `to_i` for `Int`",
        "to_i",
        "it is already Int: drop the call",
    );
    one(
        "World.tik",
        "unknown method `tik` for `World`",
        "tik",
        "did you mean `tick`?",
    );
    one(
        "puts Math.sqr(2)",
        "unknown function `Math.sqr`",
        "sqr",
        "did you mean `sqrt`?",
    );
}

#[test]
fn unknown_enum_variant() {
    one(
        "t = Team::Rd",
        "unknown variant `Rd` for enum `Team`",
        "Rd",
        "did you mean `Red`?",
    );
    one(
        "t = Team::Purple",
        "unknown variant `Purple` for enum `Team`",
        "Purple",
        "",
    );
    one(
        "t = Math::Pi",
        "unknown constant `Math::Pi`",
        "Pi",
        "did you mean `PI`?",
    );
}

#[test]
fn unknown_constant() {
    one(
        "LIMIT = 3\nputs LIMT",
        "unknown constant `LIMT`",
        "LIMT",
        "did you mean `LIMIT`?",
    );
}

#[test]
fn unknown_component_as_in_the_reference() {
    let text = with("each Postion, Velocity do |pos, vel|\nend");
    assert_error(
        &text,
        "unknown component `Postion`",
        "Postion",
        "did you mean `Position`?",
    );
    one(
        "e = spawn\nputs e.has?(Helth)",
        "unknown component `Helth`",
        "Helth",
        "did you mean `Health`?",
    );
    one(
        "spawn Plyer",
        "unknown component `Plyer`",
        "Plyer",
        "did you mean `Player`?",
    );
}

#[test]
fn unknown_event() {
    one(
        "on Hti do |hit|\nend",
        "unknown event `Hti`",
        "Hti",
        "did you mean `Hit`?",
    );
    one(
        "on Vec2 do |v|\nend",
        "`Vec2` is a struct, not an event",
        "Vec2",
        "`on` receives events, which are declared with `event`",
    );
}

// ----- types -----

#[test]
fn type_mismatch_states_both_types() {
    one(
        "x: Float = \"a\"",
        "expected `Float`, found `String`",
        "\"a\"",
        "",
    );
    one(
        "puts clamp(\"a\", 0, 1)",
        "expected `Float`, found `String`",
        "\"a\"",
        "",
    );
    one(
        "def f -> Int\n  \"a\"\nend",
        "expected `Int`, found `String`",
        "\"a\"",
        "",
    );
    one(
        "def f -> Int\n  return 1.5\nend",
        "expected `Int`, found `Float`",
        "1.5",
        FLOAT_TO_INT,
    );
    one(
        "v = Vec2()\nv.x = \"a\"",
        "expected `Float`, found `String`",
        "\"a\"",
        "",
    );
    one(
        "v = Vec2(true, 1)",
        "expected `Float`, found `Bool`",
        "true",
        "",
    );
    one(
        "Score.points = Vec2()",
        "expected `Int`, found `Vec2`",
        "Vec2()",
        "",
    );
    one("t: Team = 1", "expected `Team`, found `Int`", "1", "");
}

#[test]
fn list_types_do_not_convert() {
    one(
        "xs = [1.5]\nys: [Float32] = xs",
        "expected `[Float32]`, found `[Float]`",
        "xs",
        "list types do not convert; a list literal takes its type from where it is used",
    );
    one(
        "xs = [1, \"a\"]",
        "expected `Int`, found `String`",
        "\"a\"",
        "the elements of a list share one type",
    );
    one(
        "xs: [Int] = [1, 2.5]",
        "expected `Int`, found `Float`",
        "2.5",
        FLOAT_TO_INT,
    );
    one("x: Int = []", "expected `Int`, found `[]`", "[]", "");
}

#[test]
fn operators_need_matching_types() {
    one(
        "x = 1 + \"a\"",
        "expected `Int` or `Float`, found `String`",
        "\"a\"",
        "",
    );
    one(
        "x = \"a\" + 1",
        "expected `String`, found `Int`",
        "1",
        "convert it with `to_s`, or interpolate: `\"#{…}\"`",
    );
    one(
        "x = \"a\" * \"b\"",
        "expected `Int`, found `String`",
        "\"b\"",
        "",
    );
    one(
        "x = [1] + [\"a\"]",
        "expected `Int`, found `String`",
        "\"a\"",
        "",
    );
    one("x = true + 1", "`Bool` has no `+` operator", "true + 1", "");
    one(
        "x = \"a\" - \"b\"",
        "`String` has no `-` operator",
        "\"a\" - \"b\"",
        "",
    );
    one(
        "x = Vec2() / 2",
        "`Vec2` has no `/` operator",
        "Vec2() / 2",
        "define it in `Vec2` with `def /(other: …)`",
    );
    one(
        "x = 2 + Vec2()",
        "expected `Int` or `Float`, found `Vec2`",
        "Vec2()",
        "`Vec2` defines `+` for a `Vec2` on the left-hand side",
    );
    one("x = Vec2() + 1", "expected `Vec2`, found `Int`", "1", "");
    one("x = -\"a\"", "`String` has no unary `-`", "-\"a\"", "");
    one(
        "x = -Vec2()",
        "`Vec2` has no unary `-`",
        "-Vec2()",
        "define it in `Vec2` with `def -@`",
    );
}

#[test]
fn logic_needs_bools() {
    one(
        "x = !1",
        "expected `Bool`, found `Int`",
        "1",
        "`!` needs a `Bool`",
    );
    one(
        "x = 1 && true",
        "expected `Bool`, found `Int`",
        "1",
        "`&&` needs a `Bool` on both sides",
    );
    one(
        "x = true || \"a\"",
        "expected `Bool`, found `String`",
        "\"a\"",
        "`||` needs a `Bool` on both sides",
    );
}

#[test]
fn comparisons() {
    one(
        "x = 1 == \"a\"",
        "expected `Int`, found `String`",
        "\"a\"",
        "`==` compares two values of the same type",
    );
    one(
        "x = Team::Red != 0",
        "expected `Team`, found `Int`",
        "0",
        "`!=` compares two values of the same type",
    );
    one(
        "x = 1 < \"a\"",
        "expected `Int` or `Float`, found `String`",
        "\"a\"",
        "",
    );
    one("x = \"a\" < 1", "expected `String`, found `Int`", "1", "");
    one(
        "x = Vec2() < Vec2()",
        "`<` can't compare `Vec2` values",
        "Vec2() < Vec2()",
        "`<`, `<=`, `>` and `>=` work on numbers and strings",
    );
    one(
        "a = 1\nx = (0 < a) < 5",
        "`<` can't compare `Bool` values",
        "(0 < a) < 5",
        "`<`, `<=`, `>` and `>=` work on numbers and strings",
    );
}

// ----- arguments -----

#[test]
fn wrong_number_of_arguments() {
    one(
        "puts clamp(1, 2)",
        "`clamp` takes 3 arguments, but 2 were given",
        "clamp(1, 2)",
        "",
    );
    one(
        "greet(1)",
        "`greet` takes no arguments, but 1 was given",
        "greet(1)",
        "",
    );
    one(
        "puts clamp",
        "`clamp` takes 3 arguments, but none were given",
        "clamp",
        "",
    );
    one(
        "puts \"a\".upcase(1)",
        "`upcase` takes no arguments, but 1 was given",
        "\"a\".upcase(1)",
        "",
    );
    one(
        "puts \"a\".ljust",
        "`ljust` takes 1 argument, but none were given",
        "\"a\".ljust",
        "",
    );
    one(
        "puts min(1)",
        "`min` takes 2 arguments, but 1 was given",
        "min(1)",
        "",
    );
    one(
        "puts rand(1, 2)",
        "`rand` takes at most 1 argument, but 2 were given",
        "rand(1, 2)",
        "",
    );
    one(
        "v = Vec2().scale",
        "`scale` takes 1 argument, but none were given",
        "Vec2().scale",
        "",
    );
    one(
        "puts Math.atan2(1)",
        "`Math.atan2` takes 2 arguments, but 1 was given",
        "Math.atan2(1)",
        "",
    );
    one(
        "v = Vec2(1, 2, 3)",
        "`Vec2` has 2 fields, but 3 were given",
        "Vec2(1, 2, 3)",
        "",
    );
    one(
        "xs = [1]\nxs.push",
        "`push` takes 1 argument, but none were given",
        "xs.push",
        "",
    );
}

#[test]
fn keyword_arguments() {
    one("v = Vec2(x: 1, x: 2)", "field `x` is given twice", "x", "");
    one(
        "v = Vec2(1, y: 2)",
        "positional and named fields can't be mixed",
        "y",
        "give every field of `Vec2` by position, or every one by name",
    );
    one(
        "puts clamp(1, 2, hi: 3)",
        "`clamp` takes no keyword arguments",
        "hi",
        "pass the value by position",
    );
    one(
        "puts World.count(Position, only: Player)",
        "unknown keyword `only`",
        "only",
        "`World.count` takes `with:` and `without:`",
    );
}

#[test]
fn missing_fields_in_a_constructor() {
    one(
        "a = Armor()",
        "missing field `value` in `Armor(…)`",
        "Armor()",
        "`value` has no default",
    );
    one(
        "h = Hit()",
        "missing fields `target` and `damage` in `Hit(…)`",
        "Hit()",
        "`target` and `damage` have no default",
    );
    one(
        "h = Hit(damage: 1)",
        "missing field `target` in `Hit(…)`",
        "Hit(damage: 1)",
        "`target` has no default",
    );
    one(
        "p = Position(1)",
        "missing field `y` in `Position(…)`",
        "Position(1)",
        "`y` has no default",
    );
}

#[test]
fn only_records_are_constructed() {
    one(
        "p = Player()",
        "`Player` is a tag and can't be constructed",
        "Player",
        "a tag has no value; name it bare, as in `spawn Player`",
    );
    one(
        "s = Score()",
        "`Score` is a resource and can't be constructed",
        "Score",
        "a resource is one global instance; use its fields, like `Score.field`",
    );
    one(
        "t = Team()",
        "`Team` is an enum and can't be constructed",
        "Team",
        "write a variant, like `Team::Red`",
    );
    one(
        "x = Int(1.5)",
        "`Int` is built in and has no constructor",
        "Int",
        "only structs, components and events are constructed",
    );
}

// ----- conditions -----

#[test]
fn a_condition_must_be_a_bool() {
    one(
        "x = 1\nif x\n  puts x\nend",
        "condition must be `Bool`, found `Int`",
        "x",
        "there is no truthiness: compare it, like `x != 0`",
    );
    one(
        "s = \"a\"\nwhile s\nend",
        "condition must be `Bool`, found `String`",
        "s",
        "there is no truthiness: test it, like `!x.empty?`",
    );
    one(
        "e = spawn\nputs 1 unless e",
        "condition must be `Bool`, found `Entity`",
        "e",
        "there is no truthiness: test it, like `e.alive?`",
    );
    one(
        "v = Vec2()\nx = v ? 1 : 2",
        "condition must be `Bool`, found `Vec2`",
        "v",
        "",
    );
    one(
        "xs = [1]\nuntil xs\nend",
        "condition must be `Bool`, found `[Int]`",
        "xs",
        "there is no truthiness: test it, like `!x.empty?`",
    );
}

// ----- read-only -----

#[test]
fn parameters_are_read_only() {
    one(
        "def f(n: Int)\n  n = 1\nend",
        "can't assign to `n`",
        "n",
        "`n` is a parameter, and parameters are read-only",
    );
    one(
        "def f(v: Vec2)\n  v.x += 1\nend",
        "can't assign to `v.x`",
        "v.x",
        "`v` is a parameter, and parameters are read-only",
    );
    one(
        "def f(xs: [Int])\n  xs[0] = 1\nend",
        "can't assign to `xs[0]`",
        "xs[0]",
        "`xs` is a parameter, and parameters are read-only",
    );
    one(
        "def f(xs: [Int])\n  xs.push(1)\nend",
        "can't modify `xs`",
        "xs",
        "`xs` is a parameter, and parameters are read-only",
    );
}

#[test]
fn the_receiver_is_read_only() {
    let method =
        |body: &str| format!("struct A\n  n: Int\n  xs: [Int]\n  def m\n    {body}\n  end\nend\n");
    assert_error(
        &method("n += 1"),
        "can't assign to `n`",
        "n",
        "`n` is a field of `self`, and a method can't modify its receiver",
    );
    assert_error(
        &method("n = 1"),
        "can't assign to `n`",
        "n",
        "`n` is a field of `self`, and a method can't modify its receiver",
    );
    assert_error(
        &method("self.n = 1"),
        "can't assign to `self.n`",
        "self.n",
        "`self` is read-only: a method can't modify its receiver",
    );
    assert_error(
        &method("xs.push(1)"),
        "can't modify `xs`",
        "xs",
        "`xs` is a field of `self`, and a method can't modify its receiver",
    );
}

#[test]
fn constants_are_read_only() {
    one(
        "ORIGIN = Vec2()\nORIGIN.x = 1",
        "can't assign to `ORIGIN.x`",
        "ORIGIN.x",
        "`ORIGIN` is a constant",
    );
    one(
        "LIMIT = 3\ndef f\n  LIMIT = 4\nend",
        "can't assign to `LIMIT`",
        "LIMIT",
        "`LIMIT` is a constant",
    );
    one(
        "NAMES = [\"a\"]\nNAMES.push(\"b\")",
        "can't modify `NAMES`",
        "NAMES",
        "`NAMES` is a constant",
    );
    one(
        "def f\n  LIMIT = 4\nend",
        "constant `LIMIT` can't be defined here",
        "LIMIT",
        "a constant is defined by an assignment at the top level",
    );
}

#[test]
fn block_parameters_other_than_query_parameters_are_read_only() {
    one(
        "on Hit do |hit|\n  hit.damage = 0\nend",
        "can't assign to `hit.damage`",
        "hit.damage",
        "`hit` is the parameter of an `on` block, a read-only copy of the event",
    );
    one(
        "xs = [1]\nxs.each do |x|\n  x = 2\nend",
        "can't assign to `x`",
        "x",
        "`x` is a parameter of a list iteration, a read-only copy; loop over indexes to change the list",
    );
    one(
        "xs = [1]\nxs.each_with_index do |x, i|\n  i += 1\nend",
        "can't assign to `i`",
        "i",
        "`i` is the index of a list iteration, which is read-only",
    );
    one(
        "3.times do |i|\n  i += 1\nend",
        "can't assign to `i`",
        "i",
        "`i` is the counter of a counting loop, which is read-only",
    );
}

#[test]
fn time_fields_other_than_dt_are_read_only() {
    one(
        "Time.tick = 5",
        "can't assign to `Time.tick`",
        "Time.tick",
        "`Time.tick` is read-only; only `Time.dt` can be assigned",
    );
    one(
        "Time.elapsed += 1",
        "can't assign to `Time.elapsed`",
        "Time.elapsed",
        "`Time.elapsed` is read-only; only `Time.dt` can be assigned",
    );
}

#[test]
fn only_places_are_assigned_or_modified() {
    const LABEL: &str = "only variables, query parameters, `entity[Component]`, resource fields and their parts can be assigned";
    one(
        "Vec2().x = 1",
        "can't assign to this expression",
        "Vec2().x",
        LABEL,
    );
    one(
        "x = 1\nx.abs = 2",
        "can't assign to `x.abs`",
        "x.abs",
        LABEL,
    );
    one(
        "[1, 2].push(3)",
        "`push` needs a list that can be modified",
        "[1, 2]",
        "this list is a temporary value; put it in a variable first",
    );
    one(
        "def xs -> [Int]\n  [1]\nend\nxs.clear",
        "`clear` needs a list that can be modified",
        "xs",
        "this list is a temporary value; put it in a variable first",
    );
}

// ----- variables -----

#[test]
fn a_variable_keeps_its_type() {
    let text = with("count = 0\ncount = \"zero\"");
    assert_error(
        &text,
        "can't assign `String` to `count`, which is `Int`",
        "\"zero\"",
        "a variable keeps the type of its first value",
    );
    assert_eq!(
        note(&text),
        (
            "`count` gets its type here".to_string(),
            "count".to_string()
        )
    );
    one(
        "count = 0\ncount = 1.5",
        "can't assign `Float` to `count`, which is `Int`",
        "1.5",
        FLOAT_TO_INT,
    );
    assert_eq!(
        note(&with("count = 0\ncount = 1.5")),
        (
            "a variable keeps the type of its first value: `count` gets its type here".to_string(),
            "count".to_string()
        )
    );
    one(
        "xs = [1]\nxs = [1.5]",
        "expected `Int`, found `Float`",
        "1.5",
        FLOAT_TO_INT,
    );
}

#[test]
fn a_variable_is_declared_once() {
    let text = "x = 1\nx: Int = 2\n";
    assert_error(
        text,
        "variable `x` already exists",
        "x",
        "a type is given only where a variable is created",
    );
    assert_eq!(note(text).0, "`x` is created here");
}

#[test]
fn block_parameters_must_not_reuse_names() {
    let text = "i = 0\n3.times do |i|\nend\n";
    assert_error(
        text,
        "block parameter `i` reuses a name that is already in scope",
        "i",
        "",
    );
    assert_eq!(
        note(text),
        ("`i` is defined here".to_string(), "i".to_string())
    );
    one(
        "pos = 1\neach Position do |pos|\nend",
        "block parameter `pos` reuses a name that is already in scope",
        "pos",
        "",
    );
}

#[test]
fn variables_belong_to_their_block() {
    let text = with("if true\n  inner = 1\nend\nputs inner");
    assert_error(
        &text,
        "unknown variable or function `inner`",
        "inner",
        "`inner` was created inside the `if` and ends with it; create it before the `if`",
    );
    assert_eq!(
        note(&text),
        ("`inner` is created here".to_string(), "inner".to_string())
    );
    one(
        "3.times do |i|\n  last = i\nend\nputs last",
        "unknown variable or function `last`",
        "last",
        "`last` was created inside the `do` block and ends with it; create it before the `do` block",
    );
    // Each construct is named.
    for (body, construct) in [
        ("unless false\n  v = 1\nend", "the `unless`"),
        ("if false\n  puts 1\nelse\n  v = 1\nend", "the `if`"),
        ("v = 1 if true", "the `if`"),
        ("case 1\nwhen 1 then v = 1\nend", "the `case`"),
        ("while false\n  v = 1\nend", "the `while`"),
        ("until true\n  v = 1\nend", "the `until`"),
        ("loop do\n  v = 1\n  break\nend", "the `loop`"),
        ("[1].each do |x|\n  v = x\nend", "the `do` block"),
        ("each Position do |pos|\n  v = 1\nend", "the `each`"),
        ("on Hit do |hit|\n  v = 1\nend", "the `on`"),
    ] {
        one(
            &format!("{body}\nputs v"),
            "unknown variable or function `v`",
            "v",
            &format!(
                "`v` was created inside {construct} and ends with it; create it before {construct}"
            ),
        );
    }
    // The explanation wins over a suggestion of a similar name.
    one(
        "counts = 0\nif true\n  count = 1\nend\nputs count",
        "unknown variable or function `count`",
        "count",
        "`count` was created inside the `if` and ends with it; create it before the `if`",
    );
    // Blocks of other functions do not count.
    one(
        "def f\n  if true\n    hidden = 1\n  end\nend\nputs hidden",
        "unknown variable or function `hidden`",
        "hidden",
        "",
    );
}

#[test]
fn block_parameters_end_with_their_block() {
    const LABEL: &str = "`i` is a block parameter and ends with its block";
    one(
        "3.times do |i|\n  puts i\nend\nputs i",
        "unknown variable or function `i`",
        "i",
        LABEL,
    );
    one(
        "[1].each_with_index do |x, i|\nend\ni += 1",
        "unknown variable or function `i`",
        "i",
        LABEL,
    );
    one(
        "each Entity do |i|\nend\nputs i",
        "unknown variable or function `i`",
        "i",
        LABEL,
    );
    let errors = diagnostics(&with("on Hit do |i|\nend\nputs i"));
    assert_eq!(errors[0].label.as_deref(), Some(LABEL));
    assert!(errors[0].notes.is_empty());
}

// ----- queries -----

#[test]
fn a_tag_is_not_a_query_term() {
    one(
        "each Position, Frozen do |pos, f|\nend",
        "tag `Frozen` can't be a query term",
        "Frozen",
        "a tag has no data to bind; filter with `with: Frozen`",
    );
}

#[test]
fn a_query_names_components() {
    const LABEL: &str = "a query names components, tags and `Entity`";
    one(
        "each Vec2 do |v|\nend",
        "`Vec2` is a struct, not a component",
        "Vec2",
        LABEL,
    );
    one(
        "each Position, with: Team do |pos|\nend",
        "`Team` is an enum, not a component",
        "Team",
        LABEL,
    );
    one(
        "each Position, without: Score do |pos|\nend",
        "`Score` is a resource, not a component",
        "Score",
        LABEL,
    );
    one(
        "each Hit do |hit|\nend",
        "`Hit` is an event, not a component",
        "Hit",
        LABEL,
    );
    one(
        "each Int do |n|\nend",
        "`Int` is not a component",
        "Int",
        LABEL,
    );
    one(
        "each Position, with: Entity do |pos|\nend",
        "`Entity` can't be a filter",
        "Entity",
        "every entity is an `Entity`; filters name components and tags",
    );
    one(
        "puts World.count(Vec2)",
        "`Vec2` is a struct, not a component",
        "Vec2",
        LABEL,
    );
}

#[test]
fn a_query_names_each_type_once() {
    const LABEL: &str = "each type may be named once across terms, `with:` and `without:`";
    one(
        "each Position, Position do |a, b|\nend",
        "`Position` appears twice in this query",
        "Position",
        LABEL,
    );
    one(
        "each Position, with: Position do |pos|\nend",
        "`Position` appears twice in this query",
        "Position",
        LABEL,
    );
    one(
        "each Position, with: Player, without: [Frozen, Player] do |pos|\nend",
        "`Player` appears twice in this query",
        "Player",
        LABEL,
    );
    one(
        "each Entity, Entity do |a, b|\nend",
        "`Entity` appears twice in this query",
        "Entity",
        LABEL,
    );
}

#[test]
fn block_parameters_match_the_terms() {
    const LABEL: &str = "the block takes one parameter per term, in order";
    one(
        "each Position, Velocity do |pos|\nend",
        "this query has 2 terms, but its block takes 1 parameter",
        "pos",
        LABEL,
    );
    one(
        "each Position do |pos, vel|\nend",
        "this query has 1 term, but its block takes 2 parameters",
        "pos, vel",
        LABEL,
    );
    one(
        "each Entity, Position do\nend",
        "this query has 2 terms, but its block takes no parameters",
        "Entity, Position",
        LABEL,
    );
}

// ----- entities -----

#[test]
fn a_bare_component_needs_defaults() {
    one(
        "spawn Armor",
        "component `Armor` has fields without defaults",
        "Armor",
        "construct it: `Armor(value: …)`",
    );
    one(
        "e = spawn\ne.add(Position)",
        "component `Position` has fields without defaults",
        "Position",
        "construct it: `Position(x: …, y: …)`",
    );
}

#[test]
fn spawn_and_add_take_components() {
    one(
        "spawn Position(0, 0), Position(1, 1)",
        "`Position` is listed twice",
        "Position(1, 1)",
        "an entity has at most one component of each type",
    );
    one(
        "spawn Player, Player",
        "`Player` is listed twice",
        "Player",
        "an entity has at most one component of each type",
    );
    one(
        "spawn 5",
        "expected a component, found `Int`",
        "5",
        "an entity is made of component values and tags",
    );
    one(
        "spawn Vec2()",
        "expected a component, found `Vec2`",
        "Vec2()",
        "an entity is made of component values and tags",
    );
    one(
        "spawn Team",
        "`Team` is an enum, not a component",
        "Team",
        "only components and tags are attached to entities",
    );
    one(
        "e = spawn\ne.add",
        "`add` takes at least 1 argument, but none were given",
        "e.add",
        "",
    );
    one(
        "e = spawn\ne.remove(Vec2)",
        "`Vec2` is a struct, not a component",
        "Vec2",
        "only components and tags are attached to entities",
    );
    one(
        "e = spawn\nputs e.has?(Health(1))",
        "expected the name of a component or tag",
        "Health(1)",
        "write the type's name, like `Health`",
    );
}

#[test]
fn an_entity_is_indexed_by_a_component() {
    one(
        "e = spawn\nputs e[Player]",
        "`Player` is a tag and has no value",
        "Player",
        "test for it with `has?(Player)`",
    );
    one(
        "e = spawn\nputs e[0]",
        "expected the name of a component",
        "0",
        "an entity is indexed by component: `entity[Health]`",
    );
    one(
        "e = spawn\nputs e[Vec2]",
        "`Vec2` is a struct, not a component",
        "Vec2",
        "only components and tags are attached to entities",
    );
    one(
        "x = 5\nputs x[0]",
        "`Int` can't be indexed",
        "x",
        "indexing works on lists, and on entities as `entity[Component]`",
    );
    one(
        "xs = [1]\nputs xs[\"a\"]",
        "expected `Int`, found `String`",
        "\"a\"",
        "",
    );
}

// ----- compact types -----

#[test]
fn compact_types_are_for_fields_and_elements() {
    const INT: &str = "variables, parameters and return values are `Int` or `Float`; use `Int`";
    const FLOAT: &str = "variables, parameters and return values are `Int` or `Float`; use `Float`";
    one(
        "k: Int8 = 3",
        "`Int8` can't be the type of a variable",
        "Int8",
        INT,
    );
    one(
        "def f(x: Float32)\nend",
        "`Float32` can't be the type of a parameter",
        "Float32",
        FLOAT,
    );
    one(
        "def f -> UInt16\n  1\nend",
        "`UInt16` can't be the type of a return value",
        "UInt16",
        INT,
    );
    ok("xs: [Int8] = [1]\ndef f(xs: [[Float32]]) -> [UInt8]\n  [1]\nend\n");
}

#[test]
fn a_constant_must_fit_its_compact_type() {
    const U8: &str = "`UInt8` holds 0 to 255";
    assert_error(
        "component C\n  hp: UInt8 = 300\nend\n",
        "300 does not fit in `UInt8`",
        "300",
        U8,
    );
    one(
        "h = Health(hp: 300)",
        "300 does not fit in `UInt8`",
        "300",
        U8,
    );
    one(
        "h = Health(256, 1)",
        "256 does not fit in `UInt8`",
        "256",
        U8,
    );
    one(
        "h = Health()\nh.hp = -1",
        "-1 does not fit in `UInt8`",
        "-1",
        U8,
    );
    one(
        "xs: [UInt8] = [1, 300]",
        "300 does not fit in `UInt8`",
        "300",
        U8,
    );
    one(
        "xs: [UInt8] = []\nxs.push(256)",
        "256 does not fit in `UInt8`",
        "256",
        U8,
    );
    one(
        "xs: [UInt8] = [0]\nxs[0] = 2 ** 8",
        "256 does not fit in `UInt8`",
        "2 ** 8",
        U8,
    );
    one(
        "BIG = 2 ** 10\nh = Health()\nh.hp = BIG - 1",
        "1023 does not fit in `UInt8`",
        "BIG - 1",
        U8,
    );
    one(
        "xs: [Int8] = [-129]",
        "-129 does not fit in `Int8`",
        "-129",
        "`Int8` holds -128 to 127",
    );
    assert_error(
        "resource R\n  n: Int16 = 40000\nend\n",
        "40000 does not fit in `Int16`",
        "40000",
        "`Int16` holds -32768 to 32767",
    );
    ok(&with(
        "h = Health(hp: 255)\nh.hp = 0\nxs: [Int8] = [-128, 127]\nn = 300\nh.hp = n",
    ));
}

// ----- inference -----

#[test]
fn an_empty_list_needs_a_type() {
    const LABEL: &str = "give it a type, like `xs: [Int] = []`";
    one(
        "xs = []",
        "can't infer the element type of `[]`",
        "[]",
        LABEL,
    );
    one(
        "puts []",
        "can't infer the element type of `[]`",
        "[]",
        LABEL,
    );
    one(
        "xs = [[]]",
        "can't infer the element type of `[]`",
        "[[]]",
        LABEL,
    );
}

#[test]
fn a_recursive_function_needs_a_return_type() {
    let text = "def fact(n: Int)\n  if n <= 1 then 1 else n * fact(n - 1) end\nend\n";
    assert_error(
        text,
        "recursive function `fact` needs a declared return type",
        "fact(n - 1)",
        "this call needs the type `fact` returns, which is still being inferred",
    );
    assert_eq!(
        note(text),
        (
            "add `-> Type` to the definition of `fact`".to_string(),
            "fact".to_string()
        )
    );
}

#[test]
fn mutual_recursion_needs_a_return_type() {
    let text = "\
def even(n: Int)
  if n == 0 then true else odd(n - 1) end
end
def odd(n: Int)
  if n == 0 then false else even(n - 1) end
end
";
    assert_error(
        text,
        "recursive function `even` needs a declared return type",
        "even(n - 1)",
        "this call needs the type `even` returns, which is still being inferred",
    );
}

#[test]
fn a_recursive_call_as_the_last_expression_of_a_function_with_a_value() {
    let text = "\
def find(n: Int)
  return 0 if n == 0
  find(n - 1)
end
";
    assert_error(
        text,
        "recursive function `find` needs a declared return type",
        "find(n - 1)",
        "this call needs the type `find` returns, which is still being inferred",
    );
}

#[test]
fn the_value_of_a_recursive_procedure() {
    let text = "\
def walk(n: Int)
  return if n == 0
  walk(n - 1)
end
x = walk(3)
";
    assert_error(
        text,
        "this expression has no value",
        "walk(3)",
        "`walk` returns nothing",
    );
    let text = "\
def walk(n: Int)
  puts n
  walk(n - 1)
end
x = walk(3)
";
    assert_error(
        text,
        "this expression has no value",
        "walk(3)",
        "`walk` is recursive: it returns a value only with a declared return type",
    );
}

#[test]
fn return_values_agree() {
    one(
        "def f(n: Int)\n  return \"none\" if n == 0\n  n\nend",
        "expected `String`, found `Int`",
        "n",
        "`f` returns `String` earlier",
    );
    one(
        "def f(n: Int)\n  return 1 if n == 0\n  return if n == 1\n  2\nend",
        "`return` needs a value",
        "return",
        "`f` returns `Int` elsewhere",
    );
    one(
        "def f(n: Int) -> Int\n  return if n == 0\n  2\nend",
        "`return` needs a value",
        "return",
        "this function returns `Int`",
    );
    one(
        "def f(n: Int)\n  return 1 if n == 0\n  puts n\nend",
        "`f` can end without returning a value",
        "f",
        "it returns `Int`: end every path with a value or a `return`",
    );
    one(
        "def f(n: Int) -> Int\n  if n == 0\n    return 1\n  end\nend",
        "`f` can end without returning a value",
        "f",
        "it returns `Int`: end every path with a value or a `return`",
    );
    one(
        "def f(n: Int) -> Int\n  while n > 0\n    return 1\n  end\nend",
        "`f` can end without returning a value",
        "f",
        "it returns `Int`: end every path with a value or a `return`",
    );
}

#[test]
fn every_path_returns_or_leaves() {
    ok("\
enum Team
  Red
  Blue
end
def a(n: Int) -> Int
  if n == 0
    return 1
  else
    return 2
  end
end
def b(t: Team) -> Int
  case t
  when Team::Red then return 1
  when Team::Blue then return 2
  end
end
def c(n: Int) -> Int
  loop do
    return n if n > 3
  end
end
def d(n: Int) -> Int
  exit(1)
end
def e(n: Int) -> Int
  if n == 0
    return 1
  else
    2
  end
end
def f(n: Int) -> Int
  return 5 if n == 0
  n
end
");
}

#[test]
fn return_outside_a_function_has_no_value() {
    one(
        "return 5",
        "`return` can't have a value here",
        "5",
        "the top level returns nothing",
    );
    one(
        "system s\n  return 5\nend",
        "`return` can't have a value here",
        "5",
        "a system returns nothing",
    );
    ok("return\n");
    ok("system s\n  return\nend\n");
}

// ----- case -----

#[test]
fn case_over_an_enum_covers_every_variant() {
    one(
        "t = Team::Red\ncase t\nwhen Team::Red then puts \"r\"\nend",
        "`case` does not cover `Team::Blue` and `Team::Green`",
        "t",
        "add a `when` for each, or an `else`",
    );
    one(
        "t = Team::Red\ncase t\nwhen Team::Red, Team::Green then puts \"r\"\nend",
        "`case` does not cover `Team::Blue`",
        "t",
        "add a `when` for each, or an `else`",
    );
    ok(&with(
        "t = Team::Red\ncase t\nwhen Team::Red then puts \"r\"\nelse\n  puts \"other\"\nend",
    ));
    ok(&with(
        "t = Team::Red\ncase t\nwhen Team::Red then puts \"r\"\nwhen Team::Blue, Team::Green then puts \"c\"\nend",
    ));
}

#[test]
fn case_values() {
    one(
        "case Vec2()\nwhen Vec2() then puts 1\nend",
        "`case` can't compare `Vec2` values",
        "Vec2()",
        "the value of a `case` is an `Int`, a `String`, a `Bool` or an enum",
    );
    one(
        "case 1\nwhen \"a\" then puts 1\nend",
        "expected `Int`, found `String`",
        "\"a\"",
        "`when` values are compared with the `case` value, which is `Int`",
    );
    one(
        "x = case 1\nwhen 1 then \"a\"\nend",
        "`case` used as a value needs an `else`",
        "case 1\nwhen 1 then \"a\"\nend",
        "without one there is no value when no `when` matches",
    );
}

// ----- declarations -----

#[test]
fn duplicate_definitions_point_at_the_first() {
    let text = "tag A\ntag A\n";
    assert_error(text, "`A` is already defined", "A", "");
    let errors = diagnostics(text);
    assert_eq!(errors[0].span.start, 10);
    assert_eq!(errors[0].notes[0].message, "first defined here");
    assert_eq!(errors[0].notes[0].span.start, 4);

    for text in [
        "struct A\n  x: Int\nend\nenum A\n  B\nend\n",
        "A = 1\ntag A\n",
        "def f\nend\ndef f\nend\n",
        "def f\nend\nsystem f\nend\n",
        "struct S\n  x: Int\n  x: Float\nend\n",
        "enum E\n  A\n  A\nend\n",
        "struct S\n  x: Int\n  def m\n  end\n  def m\n  end\nend\n",
        "struct S\n  x: Int\n  def x\n  end\nend\n",
        "def f(a: Int, a: Int)\nend\n",
        "resource R\n  x: Int = 1\n  x: Int = 2\nend\n",
    ] {
        let errors = diagnostics(text);
        assert_eq!(errors.len(), 1, "{text}\n{errors:?}");
        assert!(
            errors[0].message.ends_with("is already defined"),
            "{text}\n{errors:?}"
        );
        assert_eq!(errors[0].notes[0].message, "first defined here", "{text}");
        assert!(
            errors[0].notes[0].span.start < errors[0].span.start,
            "{text}"
        );
    }
}

#[test]
fn built_in_names_are_not_redefined() {
    assert_error(
        "struct Int\n  x: Float\nend\n",
        "`Int` is a built-in name and can't be redefined",
        "Int",
        "",
    );
    assert_error(
        "tag World\n",
        "`World` is a built-in name and can't be redefined",
        "World",
        "",
    );
    assert_error(
        "Math = 3\n",
        "`Math` is a built-in name and can't be redefined",
        "Math",
        "",
    );
    assert_error(
        "struct S\n  x: Int\n  def to_s -> String\n    \"s\"\n  end\nend\n",
        "`to_s` is provided for every type and can't be redefined",
        "to_s",
        "",
    );
}

#[test]
fn declarations_have_the_fields_they_need() {
    assert_error(
        "component Marker\nend\n",
        "component `Marker` needs at least one field",
        "Marker",
        "a component with no data is a tag: `tag Marker`",
    );
    assert_error(
        "resource Score\n  points: Int\nend\n",
        "field `points` of resource `Score` needs a default",
        "points",
        "a resource exists from the start, so every field has a default",
    );
    ok("event Tick\nend\nemit Tick\nemit Tick()\n");
}

#[test]
fn a_struct_does_not_contain_itself() {
    assert_error(
        "struct Node\n  value: Int\n  link: Node\nend\n",
        "struct `Node` contains itself",
        "link",
        "a value can contain its own type only through a list, like `[Node]`",
    );
    assert_error(
        "struct A\n  b: B\nend\nstruct B\n  c: C\nend\nstruct C\n  a: A\nend\n",
        "struct `A` contains itself through `B` and `C`",
        "b",
        "a value can contain its own type only through a list, like `[A]`",
    );
    ok("struct Node\n  value: Int\n  children: [Node]\nend\n");
}

#[test]
fn tags_and_resources_are_not_value_types() {
    one(
        "p: Player = 1",
        "`Player` is a tag, not a value type",
        "Player",
        "a tag has no value: it can only be attached to entities and named in queries",
    );
    one(
        "def f(s: Score)\nend",
        "`Score` is a resource, not a value type",
        "Score",
        "a resource is one global instance; use its fields, like `Score.field`",
    );
    assert_error(
        "struct S\n  w: World\nend\n",
        "`World` is not a type",
        "World",
        "",
    );
    one(
        "p = Player",
        "`Player` is a tag and has no value",
        "Player",
        "a tag can be attached, removed, tested with `has?` and used to filter queries",
    );
    one(
        "s = Score",
        "resource `Score` is not a value",
        "Score",
        "read and write its fields, like `Score.field`",
    );
    one(
        "p = Position",
        "`Position` is a component, not a value",
        "Position",
        "construct one with `Position(…)`",
    );
    one(
        "t = Team",
        "`Team` is an enum, not a value",
        "Team",
        "write a variant, like `Team::Red`",
    );
    one(
        "t = Team.Red",
        "`Team` is an enum, not a value",
        "Team",
        "did you mean `Team::Red`?",
    );
    one(
        "w = World",
        "`World` is not a value",
        "World",
        "use its members, like `World.…`",
    );
}

#[test]
fn operator_methods_have_fixed_parameters() {
    assert_error(
        "struct S\n  x: Int\n  def +(a: S, b: S) -> S\n    a\n  end\nend\n",
        "operator method `+` takes 1 parameter, but 2 are declared",
        "+",
        "",
    );
    assert_error(
        "struct S\n  x: Int\n  def - -> S\n    S(-x)\n  end\nend\n",
        "operator method `-` takes 1 parameter, but none are declared",
        "-",
        "unary minus is defined with `def -@`",
    );
}

// ----- constant expressions -----

#[test]
fn defaults_and_constants_are_constant_expressions() {
    const DEFAULT: &str = "a field default can use literals, constants, enum variants, operators, constructors, list literals and `Math` functions";
    const CONSTANT: &str = "a constant's value can use literals, constants, enum variants, operators, constructors, list literals and `Math` functions";
    assert_error(
        "def seed -> Int\n  4\nend\nstruct S\n  x: Int = seed\nend\n",
        "not a constant expression",
        "seed",
        &format!("`seed` is a function call; {DEFAULT}"),
    );
    assert_error(
        "struct S\n  x: Float = rand\nend\n",
        "not a constant expression",
        "rand",
        &format!("`rand` is a function call; {DEFAULT}"),
    );
    one(
        "N = clamp(1, 2, 3)",
        "not a constant expression",
        "clamp(1, 2, 3)",
        &format!("`clamp(…)` is a function call; {CONSTANT}"),
    );
    // One report for the whole expression, at its first offending part.
    one(
        "size = 4\nAREA = size * size",
        "not a constant expression",
        "size",
        &format!("`size` is a variable; {CONSTANT}"),
    );
    assert_eq!(
        diagnostics(&with("size = 4\nAREA = size * size"))[0]
            .span
            .start as usize,
        DECLS.len() + "size = 4\nAREA = ".len()
    );
    one(
        "size = 4\nAREA = [size, rand(3), \"abc\".length]",
        "not a constant expression",
        "size",
        &format!("`size` is a variable; {CONSTANT}"),
    );
    // Each constant expression gets its own report.
    assert_eq!(
        errors(&with("size = 4\nA = size\nB = size + size")).len(),
        2
    );
    assert_error(
        "resource R\n  e: Entity = spawn\nend\n",
        "not a constant expression",
        "spawn",
        DEFAULT,
    );
    one(
        "N = \"abc\".length",
        "not a constant expression",
        "\"abc\".length",
        CONSTANT,
    );
    one(
        "N = World.count",
        "not a constant expression",
        "World.count",
        CONSTANT,
    );
    one(
        "V = Vec2(1, 2) + Vec2(3, 4)",
        "not a constant expression",
        "Vec2(1, 2) + Vec2(3, 4)",
        CONSTANT,
    );
    one(
        "N = if true then 1 else 2 end",
        "not a constant expression",
        "if true then 1 else 2 end",
        CONSTANT,
    );
    ok(&with(
        "A = 2\nB = A * 3 + 1\nC = [A, B]\nD = Vec2(A, B)\nE = Math.sqrt(2.0) * Math::PI\nF = Entity.none\nG = Team::Red\nH = \"n=#{A}\"\nI = A > 1 ? 1 : 2\nJ = -A",
    ));
}

#[test]
fn a_constant_uses_only_constants_above_it() {
    assert_error(
        "A = B + 1\nB = 2\n",
        "constant `B` is used before it is defined",
        "B",
        "a constant can use only the constants defined above it",
    );
    assert_error(
        "A = A + 1\n",
        "constant `A` is used in its own value",
        "A",
        "",
    );
    let text = "struct S\n  n: Int = LIMIT\nend\nFIRST = S()\nLIMIT = 3\n";
    assert_error(
        text,
        "constant `FIRST` uses `LIMIT` before it is defined",
        "FIRST",
        "a field default in its value reads `LIMIT`; define `LIMIT` above `FIRST`",
    );
    assert_eq!(
        note(text),
        ("`LIMIT` is read here".to_string(), "LIMIT".to_string())
    );
    ok(
        "LIMIT = 3\nstruct S\n  n: Int = LIMIT\nend\nFIRST = S()\ndef f -> Int\n  LATER\nend\nLATER = 1\n",
    );
}

// ----- control flow -----

#[test]
fn break_and_next_belong_in_loops() {
    const LABEL: &str = "it belongs in `while`, `until`, `loop`, `each`, `on` or a `do` block";
    one("break", "`break` outside a loop", "break", LABEL);
    one("def f\n  next\nend", "`next` outside a loop", "next", LABEL);
    ok(&with(
        "loop do\n  break\nend\n3.times do\n  next\nend\neach Position do |p|\n  break\nend\non Hit do |h|\n  next\nend\nwhile true\n  break if true\nend",
    ));
}

#[test]
fn do_blocks_belong_to_loops() {
    one(
        "v = Vec2()\nv.length do\nend",
        "`length` does not take a `do` block",
        "length",
        "only `times`, `upto`, `downto`, `each` and `each_with_index` take one",
    );
    one(
        "x = 3.times do\nend",
        "a `times` loop has no value",
        "times",
        "a loop is a statement",
    );
    one(
        "3.times",
        "`times` needs a `do` block",
        "times",
        "write `times do … end`",
    );
    one(
        "xs = [1]\nxs.each",
        "`each` needs a `do` block",
        "each",
        "write `each do … end`",
    );
    one(
        "3.times do |i, j|\nend",
        "the block of `times` takes at most 1 parameter, but 2 were given",
        "j",
        "",
    );
    one(
        "xs = [1]\nxs.each do |x, i|\nend",
        "the block of `each` takes at most 1 parameter, but 2 were given",
        "i",
        "",
    );
    one(
        "on Hit do |a, b|\nend",
        "the block of `on` takes at most 1 parameter, but 2 were given",
        "b",
        "",
    );
    one(
        "1.5.times do\nend",
        "expected `Int`, found `Float`",
        "1.5",
        FLOAT_TO_INT,
    );
    one(
        "5.each do |x|\nend",
        "`each` iterates a list, found `Int`",
        "5",
        "only lists are iterated with a `do` block",
    );
    one(
        "1.upto do\nend",
        "`upto` takes 1 argument, but none were given",
        "upto",
        "",
    );
}

#[test]
fn values_are_needed_where_they_are_used() {
    one(
        "x = greet",
        "this expression has no value",
        "greet",
        "`greet` returns nothing",
    );
    one(
        "xs = [1]\nx = xs.push(2)",
        "this expression has no value",
        "xs.push(2)",
        "`push` returns nothing",
    );
    one(
        "puts World.tick",
        "this expression has no value",
        "World.tick",
        "this returns nothing",
    );
    one(
        "x = if true then 1 end",
        "`if` used as a value needs an `else`",
        "if true then 1 end",
        "without one there is no value when the other branch would run",
    );
    one(
        "x = if true\n  1\nelse\n  greet\nend",
        "this branch has no value",
        "greet",
        "every branch of an `if` used as a value ends in one",
    );
    one(
        "x = if true then 1 else \"a\" end",
        "expected `Int`, found `String`",
        "\"a\"",
        "every branch of an `if` has the same type",
    );
    one(
        "x = true ? 1 : \"a\"",
        "expected `Int`, found `String`",
        "\"a\"",
        "every branch of a conditional has the same type",
    );
}

#[test]
fn systems_are_not_called() {
    one(
        "system move\nend\nmove",
        "`move` is a system and can't be called",
        "move",
        "systems run when `World.tick` is called",
    );
    one(
        "system move\nend\nmove()",
        "`move` is a system and can't be called",
        "move",
        "systems run when `World.tick` is called",
    );
}

#[test]
fn self_belongs_in_methods() {
    one(
        "x = self",
        "`self` can only be used in a method",
        "self",
        "there is no receiver here",
    );
}

#[test]
fn emit_takes_an_event() {
    one(
        "emit Vec2()",
        "`emit` needs an event, found `Vec2`",
        "Vec2()",
        "events are declared with `event`",
    );
    one(
        "emit Hit",
        "event `Hit` has fields without defaults",
        "Hit",
        "construct it: `Hit(target: …, damage: …)`",
    );
}

// ----- reporting -----

#[test]
fn every_independent_error_is_reported_in_source_order() {
    let text = with("def late\n  puts aaa\nend\nputs bbb\nx: Int = \"s\"\nputs ccc");
    let messages: Vec<String> = errors(&text).into_iter().map(|e| e.0).collect();
    assert_eq!(
        messages,
        [
            "unknown variable or function `aaa`",
            "unknown variable or function `bbb`",
            "expected `Int`, found `String`",
            "unknown variable or function `ccc`",
        ]
    );
}

#[test]
fn a_failed_expression_does_not_cascade() {
    // The variable is declared even though its value failed, and uses of
    // it stay quiet.
    one(
        "x = nothing_here\ny = x + 1\nputs y.abs\nx = 5",
        "unknown variable or function `nothing_here`",
        "nothing_here",
        "",
    );
    one(
        "v = Vec3(1, 2)\nputs v.x\nv.x = 3\nw = v + v",
        "unknown type `Vec3`",
        "Vec3",
        "did you mean `Vec2`?",
    );
    one(
        "each Postion, Velocity do |pos, vel|\n  pos.x += vel.dx\nend",
        "unknown component `Postion`",
        "Postion",
        "did you mean `Position`?",
    );
    assert_error(
        "struct S\n  a: Missing\nend\ns = S(1)\nputs s.a + 1\n",
        "unknown type `Missing`",
        "Missing",
        "",
    );
    assert_error(
        "def f(n: Int) -> Missing\n  n\nend\nx = f(1) + 1\n",
        "unknown type `Missing`",
        "Missing",
        "",
    );
}

#[test]
fn every_error_is_returned_however_many() {
    let text: String = (0..30).map(|i| format!("puts zzzzzzzz{i}\n")).collect();
    let errors = diagnostics(&text);
    assert_eq!(errors.len(), 30);
    assert_eq!(
        errors[0].message,
        "unknown variable or function `zzzzzzzz0`"
    );
    assert_eq!(
        errors[29].message,
        "unknown variable or function `zzzzzzzz29`"
    );
}

#[test]
fn mutual_recursion_through_last_expressions() {
    // `h` needs the value of `k`, which ends in a call back to `h`.
    let text = "def h(n: Int)\n  return 1 if n == 0\n  k(n)\nend\ndef k(n: Int)\n  h(n - 1)\nend\n";
    assert_error(
        text,
        "recursive function `k` needs a declared return type",
        "k(n)",
        "this call needs the type `k` returns, which is still being inferred",
    );
    // Procedures that end by calling each other return nothing.
    let text = "def f(n: Int)\n  g(n)\nend\ndef g(n: Int)\n  f(n)\nend\nx = f(1)\n";
    assert_error(
        text,
        "this expression has no value",
        "f(1)",
        "`f` is recursive: it returns a value only with a declared return type",
    );
}

#[test]
fn every_containment_cycle_is_reported_once() {
    let text = "\
struct A
  b: B
end
struct B
  a: A
  me: B
end
def pick(b: B, c: Bool) -> B
  x = if c then b else return b end
  x
end
";
    assert_errors(
        text,
        &[
            (
                "struct `A` contains itself through `B`",
                "b",
                "a value can contain its own type only through a list, like `[A]`",
            ),
            (
                "struct `B` contains itself",
                "me",
                "a value can contain its own type only through a list, like `[B]`",
            ),
        ],
    );
}

#[test]
fn a_default_does_not_depend_on_itself() {
    assert_error(
        "struct Loop\n  n: [Loop] = [Loop()]\nend\n",
        "the default of `n` depends on itself",
        "n",
        "",
    );
}

#[test]
fn a_query_has_a_term() {
    one(
        "puts World.count(with: Player)",
        "a query needs at least one term",
        "World.count(with: Player)",
        "to match on filters alone, start with `Entity`",
    );
}

#[test]
fn only_function_and_method_names_end_in_a_mark() {
    const LABEL: &str = "only function and method names can";
    assert_error(
        "ready? = true\nputs 1\n",
        "variable name `ready?` can't end in `?`",
        "ready?",
        LABEL,
    );
    assert_error(
        "go!: Int = 1\n",
        "variable name `go!` can't end in `!`",
        "go!",
        LABEL,
    );
    assert_error(
        "def f(ok?: Bool)\nend\n",
        "parameter name `ok?` can't end in `?`",
        "ok?",
        LABEL,
    );
    assert_error(
        "3.times do |i!|\nend\n",
        "block parameter name `i!` can't end in `!`",
        "i!",
        LABEL,
    );
    assert_error(
        "struct S\n  done?: Bool\nend\n",
        "field name `done?` can't end in `?`",
        "done?",
        LABEL,
    );
    assert_error(
        "system move!\nend\n",
        "system name `move!` can't end in `!`",
        "move!",
        LABEL,
    );
    // The variable still exists, so its uses are not reported again.
    assert_error(
        "ready? = true\nputs ready?\n",
        "variable name `ready?` can't end in `?`",
        "ready?",
        LABEL,
    );
    ok(
        "def ready? -> Bool\n  true\nend\ndef go!\nend\nstruct S\n  n: Int\n  def zero? -> Bool\n    n == 0\n  end\nend\ngo!\nputs ready?, S(0).zero?\n",
    );
}

#[test]
fn a_variable_is_not_called() {
    one(
        "total = 5\nputs total(1)",
        "unknown function `total`",
        "total",
        "`total` is a variable; `total(…)` always calls a function",
    );
    // A function of that name is still called.
    ok(&with("greet = 5\ngreet()\nputs greet"));
}

#[test]
fn a_float_does_not_convert_to_int() {
    one(
        "def double(n: Int) -> Int\n  n * 2\nend\nputs double(2.5)",
        "expected `Int`, found `Float`",
        "2.5",
        FLOAT_TO_INT,
    );
    one(
        "count: Int = 2.5",
        "expected `Int`, found `Float`",
        "2.5",
        FLOAT_TO_INT,
    );
    one(
        "Score.points = 1.5",
        "expected `Int`, found `Float`",
        "1.5",
        FLOAT_TO_INT,
    );
    // Compact integer fields and elements.
    one(
        "h = Health()\nh.hp = 1.5",
        "expected `Int`, found `Float`",
        "1.5",
        FLOAT_TO_INT,
    );
    one(
        "h = Health(hp: 1.5)",
        "expected `Int`, found `Float`",
        "1.5",
        FLOAT_TO_INT,
    );
    one(
        "xs: [UInt8] = []\nxs.push(0.5)",
        "expected `Int`, found `Float`",
        "0.5",
        FLOAT_TO_INT,
    );
    one(
        "xs = [1]\nputs xs[0.5]",
        "expected `Int`, found `Float`",
        "0.5",
        FLOAT_TO_INT,
    );
    // A label that says more about the place takes precedence.
    one(
        "case 1\nwhen 1.5 then puts 1\nend",
        "expected `Int`, found `Float`",
        "1.5",
        "`when` values are compared with the `case` value, which is `Int`",
    );
}

#[test]
fn a_compound_assignment_that_would_store_a_float_in_an_int() {
    let text = with("n = 1\nn += 0.5");
    assert_error(
        &text,
        "can't assign `Float` to `n`, which is `Int`",
        "n += 0.5",
        "`n + 0.5` is a `Float`",
    );
    assert_eq!(
        note(&text),
        ("`n` gets its type here".to_string(), "n".to_string())
    );
    one(
        "n = 1\nx = 2.5\nn *= x",
        "can't assign `Float` to `n`, which is `Int`",
        "n *= x",
        "`n * x` is a `Float`",
    );
    one(
        "n = 1\nn -= 1 + 0.5",
        "can't assign `Float` to `n`, which is `Int`",
        "n -= 1 + 0.5",
        "`n - (1 + 0.5)` is a `Float`",
    );
    one(
        "n = 7\nn /= 2.0",
        "can't assign `Float` to `n`, which is `Int`",
        "n /= 2.0",
        "`n / 2.0` is a `Float`",
    );
    one(
        "n = 7\nn %= 2.0",
        "can't assign `Float` to `n`, which is `Int`",
        "n %= 2.0",
        "`n % 2.0` is a `Float`",
    );
    // Fields and elements, with the type as declared; no note.
    let text = with("h = Health()\nh.max += 0.5");
    assert_error(
        &text,
        "can't assign `Float` to `h.max`, which is `Int`",
        "h.max += 0.5",
        "`h.max + 0.5` is a `Float`",
    );
    assert!(diagnostics(&text)[0].notes.is_empty());
    one(
        "h = Health()\nh.hp -= 0.5",
        "can't assign `Float` to `h.hp`, which is `UInt8`",
        "h.hp -= 0.5",
        "`h.hp - 0.5` is a `Float`",
    );
    one(
        "Score.points += 0.5",
        "can't assign `Float` to `Score.points`, which is `Int`",
        "Score.points += 0.5",
        "`Score.points + 0.5` is a `Float`",
    );
    one(
        "xs = [1]\nxs[0] += 0.5",
        "can't assign `Float` to `xs[0]`, which is `Int`",
        "xs[0] += 0.5",
        "`xs[0] + 0.5` is a `Float`",
    );
    // Other mismatches keep the plain wording.
    one(
        "n = 1\nn += \"a\"",
        "expected `Int`, found `String`",
        "\"a\"",
        "",
    );
    ok(&with("n = 1\nn += 2\nx = 1.5\nx += 1\nx -= 0.5"));
}
