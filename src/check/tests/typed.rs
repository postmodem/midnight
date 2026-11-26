//! The typed program the checker produces, construct by construct.

use super::*;

const DECLS: &str = "\
component Position
  x: Float32
  y: Float32
end
component Velocity
  dx: Float
  dy: Float
end
component Health
  hp: UInt8 = 100
  max: Int = 100
end
component Inventory
  items: [Int] = []
end
tag Player
tag Frozen
struct Vec2
  x: Float = 0
  y: Float = 0
  def +(other: Vec2) -> Vec2
    Vec2(x + other.x, y + other.y)
  end
  def *(k: Float) -> Vec2
    Vec2(x * k, y * k)
  end
  def -@ -> Vec2
    Vec2(-x, -y)
  end
  def length -> Float
    Math.sqrt(x * x + y * y)
  end
  def unit -> Vec2
    self * (1 / length)
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
event Tick
end
def half(x: Float) -> Float
  x / 2
end
def one -> Int
  1
end
def two -> Int
  2
end
def greet
  puts \"hello\"
end
";

/// The last top-level statement of `body` after the shared declarations.
#[track_caller]
fn stmt(body: &str) -> String {
    last(&format!("{DECLS}{body}\n"))
}

/// Every top-level statement of `body` after the shared declarations.
#[track_caller]
fn stmts(body: &str) -> Vec<String> {
    main_stmts(&format!("{DECLS}{body}\n"))
}

/// The type of a top-level variable of `body`.
#[track_caller]
fn var_type(body: &str, name: &str) -> String {
    let program = ok(&format!("{DECLS}{body}\n"));
    let dump = Dump {
        program: &program,
        locals: &program.main.locals,
    };
    let local = program
        .main
        .locals
        .iter()
        .find(|l| l.name == name)
        .unwrap_or_else(|| panic!("no variable `{name}`"));
    dump.ty(&local.ty)
}

#[test]
fn hello_world() {
    assert_eq!(
        main_stmts("puts \"Hello, world!\"\n"),
        ["puts \"Hello, world!\""]
    );
}

#[test]
fn spans_are_copied() {
    let text = "x = 1 + 2\nputs x\n";
    let program = ok(text);
    let marked = |span: Span| &text[span.start as usize..span.end as usize];
    let first = &program.main.block.stmts[0];
    assert_eq!(marked(first.span), "x = 1 + 2");
    let StmtKind::Let { init, .. } = &first.kind else {
        panic!("not a let");
    };
    assert_eq!(marked(init.span), "1 + 2");
    let ExprKind::Binary { rhs, .. } = &init.kind else {
        panic!("not a binary");
    };
    assert_eq!(marked(rhs.span), "2");
    let StmtKind::Puts(args) = &program.main.block.stmts[1].kind else {
        panic!("not a puts");
    };
    let ExprKind::Read(place) = &args[0].kind else {
        panic!("not a read");
    };
    assert_eq!(marked(place.span), "x");
}

// ----- operators and conversions -----

#[test]
fn arithmetic() {
    assert_eq!(stmt("x = 1 + 2"), "let x = (+ 1 2)");
    assert_eq!(stmt("x = 7 / 2 % 3"), "let x = (% (/ 7 2) 3)");
    assert_eq!(stmt("x = 2 ** 3 ** 2"), "let x = (** 2 (** 3 2))");
    assert_eq!(stmt("x = 1.5 * 2.0"), "let x = (* 1.5 2.0)");
    assert_eq!(stmt("x = -3"), "let x = -3");
    assert_eq!(stmt("y = 2\nx = -y"), "let x = (- y)");
    assert_eq!(stmt("x = \"a\" + \"b\""), "let x = (+ \"a\" \"b\")");
    assert_eq!(stmt("x = \"-\" * 20"), "let x = (* \"-\" 20)");
    assert_eq!(stmt("x = [0] * 64"), "let x = (* [0]:[Int] 64)");
    assert_eq!(
        stmt("x = [1] + [2, 3]"),
        "let x = (+ [1]:[Int] [2, 3]:[Int])"
    );
    assert_eq!(var_type("x = 1 + 2", "x"), "Int");
    assert_eq!(var_type("x = 1 + 2.0", "x"), "Float");
    assert_eq!(var_type("x = 2 ** 3", "x"), "Int");
    assert_eq!(var_type("x = \"a\" * 3", "x"), "String");
}

#[test]
fn int_converts_to_float_where_a_float_is_expected() {
    assert_eq!(stmt("x = 1 + 2.5"), "let x = (+ (float 1) 2.5)");
    assert_eq!(stmt("x = 2.5 - 1"), "let x = (- 2.5 (float 1))");
    assert_eq!(stmt("x = 2 ** 0.5"), "let x = (** (float 2) 0.5)");
    assert_eq!(stmt("ratio: Float = 1"), "let ratio = (float 1)");
    assert_eq!(stmt("x = 1.5\nx = 2"), "x = (float 2)");
    assert_eq!(stmt("x = 1.5\nx += 1"), "x += (float 1)");
    assert_eq!(stmt("n = 3\nx = half(n)"), "let x = half((float n))");
    assert_eq!(stmt("v = Vec2(1, 2.5)"), "let v = Vec2((float 1), 2.5)");
    assert_eq!(stmt("v = Vec2()\nv.x = 3"), "v.x = (float 3)");
    assert_eq!(
        stmt("xs = [1, 2.5, 3]"),
        "let xs = [(float 1), 2.5, (float 3)]:[Float]"
    );
    assert_eq!(stmt("puts 1 == 1.0"), "puts (== (float 1) 1.0)");
    assert_eq!(stmt("puts 1.5 != 2"), "puts (!= 1.5 (float 2))");
    assert_eq!(stmt("puts 1 < 2.5"), "puts (< (float 1) 2.5)");
    assert_eq!(stmt("puts 1 < 2"), "puts (< 1 2)");
    assert_eq!(stmt("Time.dt = 1"), "Time.dt = (float 1)");
    assert_eq!(stmt("x = 3.to_f"), "let x = (float 3)");
}

#[test]
fn returned_ints_convert_to_float() {
    assert_eq!(
        func(
            "def f(n: Int) -> Float\n  return 1 if n == 0\n  n\nend\n",
            "f"
        ),
        "f(n: Int) -> Float { (if (== n 0) { return (float 1) } { }); => (float n) }"
    );
    // Inferred: an `Int` among `Float` results converts.
    assert_eq!(
        func("def f(n: Int)\n  return 1 if n == 0\n  2.5\nend\n", "f"),
        "f(n: Int) -> Float { (if (== n 0) { return (float 1) } { }); => 2.5 }"
    );
    assert_eq!(
        func("def f(n: Int)\n  return 1.5 if n == 0\n  n\nend\n", "f"),
        "f(n: Int) -> Float { (if (== n 0) { return 1.5 } { }); => (float n) }"
    );
}

#[test]
fn logic_and_comparison() {
    assert_eq!(
        stmt("a = true\nx = !a && 1 < 2 || a"),
        "let x = (|| (&& (! a) (< 1 2)) a)"
    );
    assert_eq!(stmt("x = \"a\" < \"b\""), "let x = (< \"a\" \"b\")");
    assert_eq!(stmt("x = \"a\" == \"b\""), "let x = (== \"a\" \"b\")");
    assert_eq!(
        stmt("x = Team::Red != Team::Blue"),
        "let x = (!= Team::Red Team::Blue)"
    );
    assert_eq!(
        stmt("x = Vec2() == Vec2(1, 2)"),
        "let x = (== Vec2((float 0), (float 0)) Vec2((float 1), (float 2)))"
    );
    assert_eq!(
        stmt("e = spawn\nx = e == Entity.none"),
        "let x = (== e Entity.none)"
    );
}

#[test]
fn operator_methods_become_calls() {
    assert_eq!(
        stmt("a = Vec2()\nb = Vec2()\nc = a + b"),
        "let c = Vec2.+(a, b)"
    );
    assert_eq!(
        stmt("a = Vec2()\nc = a * 2"),
        "let c = Vec2.*(a, (float 2))"
    );
    assert_eq!(stmt("a = Vec2()\nc = -a"), "let c = Vec2.-@(a)");
    assert_eq!(
        stmt("a = Vec2()\na += Vec2(1, 1)"),
        "a <Vec2.+>= Vec2((float 1), (float 1))"
    );
    assert_eq!(stmt("a = Vec2()\na *= 2"), "a <Vec2.*>= (float 2)");
}

#[test]
fn compound_assignment() {
    assert_eq!(stmt("n = 1\nn += 2"), "n += 2");
    assert_eq!(stmt("n = 1\nn %= 2"), "n %= 2");
    assert_eq!(stmt("s = \"a\"\ns += \"b\""), "s += \"b\"");
    assert_eq!(stmt("s = \"a\"\ns *= 3"), "s *= 3");
    assert_eq!(stmt("xs = [1.5]\nxs += [2]"), "xs += [(float 2)]:[Float]");
    assert_eq!(stmt("xs = [1]\nxs *= 2"), "xs *= 2");
    assert_eq!(stmt("xs = [1]\nxs[0] *= 2"), "xs[0] *= 2");
    assert_eq!(stmt("h = Health()\nh.hp -= 10"), "h.hp -= 10");
    assert_eq!(stmt("Score.points += 10"), "Score.points += 10");
}

// ----- strings and lists -----

#[test]
fn interpolation_becomes_a_concat() {
    assert_eq!(
        stmt("n = 1\ns = \"x\"\nt = \"a#{n}b#{s}\""),
        "let t = (concat \"a\" ToS(n) \"b\" s)"
    );
    assert_eq!(stmt("t = 'raw #{x}'"), "let t = \"raw #{x}\"");
    assert_eq!(stmt("t = \"\""), "let t = \"\"");
    assert_eq!(
        stmt("v = Vec2()\nt = \"#{v.x + 1}\""),
        "let t = (concat ToS((+ v.x (float 1))))"
    );
}

#[test]
fn a_list_literal_takes_its_type_from_where_it_is_used() {
    assert_eq!(stmt("xs = [1, 2]"), "let xs = [1, 2]:[Int]");
    assert_eq!(stmt("xs: [Int] = []"), "let xs = []:[Int]");
    assert_eq!(
        stmt("xs: [Float32] = [1, 2.5]"),
        "let xs = [(float 1), 2.5]:[Float32]"
    );
    assert_eq!(stmt("xs: [UInt8] = [1, 2]"), "let xs = [1, 2]:[UInt8]");
    assert_eq!(
        stmt("xs: [[Int8]] = [[1], []]"),
        "let xs = [[1]:[Int8], []:[Int8]]:[[Int8]]"
    );
    assert_eq!(
        stmt("xs = [[1], []]"),
        "let xs = [[1]:[Int], []:[Int]]:[[Int]]"
    );
    assert_eq!(stmt("xs = [1.5]\nxs = [2]"), "xs = [(float 2)]:[Float]");
    assert_eq!(stmt("xs: [Float] = []\nxs.push(1)"), "(push xs (float 1))");
    assert_eq!(
        stmt("xs: [UInt8] = []\nys = xs + [1]"),
        "let ys = (+ xs [1]:[UInt8])"
    );
    assert_eq!(
        stmt("xs: [UInt8] = []\nys = [1] + xs"),
        "let ys = (+ [1]:[UInt8] xs)"
    );
    assert_eq!(
        stmt("xs: [Float] = []\nputs xs == [1]"),
        "puts (== xs [(float 1)]:[Float])"
    );
    assert_eq!(
        stmt("xs: [Float] = []\nputs [] != xs"),
        "puts (!= []:[Float] xs)"
    );
    assert_eq!(
        stmt("xs: [Float32] = [0] * 3"),
        "let xs = (* [(float 0)]:[Float32] 3)"
    );
    assert_eq!(
        stmt("xs: [Float32] = [1] + [2]"),
        "let xs = (+ [(float 1)]:[Float32] [(float 2)]:[Float32])"
    );
    assert_eq!(
        stmt("Score.history = [1, 2]"),
        "Score.history = [1, 2]:[Int]"
    );
    assert_eq!(
        func("def f -> [Float]\n  [1]\nend\n", "f"),
        "f() -> [Float] { => [(float 1)]:[Float] }"
    );
    assert_eq!(
        func(
            "def f(c: Bool) -> [Float]\n  return [] if c\n  [1]\nend\n",
            "f"
        ),
        "f(c: Bool) -> [Float] { (if c { return []:[Float] } { }); => [(float 1)]:[Float] }"
    );
    assert_eq!(
        stmt("def total(xs: [Float]) -> Float\n  0\nend\nputs total([1, 2])"),
        "puts total([(float 1), (float 2)]:[Float])"
    );
    assert_eq!(
        stmt("xs: [Int] = true ? [] : [1]"),
        "let xs = (if true { => []:[Int] } { => [1]:[Int] })"
    );
    assert_eq!(var_type("xs: [Float32] = []\nys = xs", "ys"), "[Float32]");
}

#[test]
fn list_methods() {
    let list = "xs = [1, 2]\n";
    let with = |body: &str| stmt(&format!("{list}{body}"));
    assert_eq!(with("puts xs.length"), "puts ListLength(xs)");
    assert_eq!(with("puts xs.empty?"), "puts ListEmpty(xs)");
    assert_eq!(
        with("puts xs.first, xs.last"),
        "puts ListFirst(xs), ListLast(xs)"
    );
    assert_eq!(with("puts xs.include?(2)"), "puts ListInclude(xs, 2)");
    assert_eq!(with("xs.push(3)"), "(push xs 3)");
    assert_eq!(with("x = xs.pop"), "let x = (pop xs)");
    assert_eq!(with("x = xs.delete_at(0)"), "let x = (delete_at xs 0)");
    assert_eq!(with("xs.clear"), "(clear xs)");
    assert_eq!(with("puts xs[0], xs[-1]"), "puts xs[0], xs[-1]");
    assert_eq!(with("xs[1] = 5"), "xs[1] = 5");
    assert_eq!(with("puts xs.to_s"), "puts ToS(xs)");
    assert_eq!(with("puts [1, 2][0]"), "puts (index [1, 2]:[Int] 0)");
    assert_eq!(with("puts [1, 2].length"), "puts ListLength([1, 2]:[Int])");
    assert_eq!(
        stmt("grid: [[Float32]] = [[0]]\ngrid[0][0] = 1\ngrid[0].push(2)"),
        "(push grid[0] (float 2))"
    );
    assert_eq!(var_type("ys: [Float32] = [1]\ny = ys.first", "y"), "Float");
    assert_eq!(var_type("ys: [Int8] = [1]\ny = ys.pop", "y"), "Int");
    assert_eq!(var_type("ys: [Int8] = [1]\ny = ys[0]", "y"), "Int");
}

// ----- built-ins -----

#[test]
fn number_methods() {
    let vars = "n = 3\nx = 2.5\n";
    let with = |body: &str| stmt(&format!("{vars}{body}"));
    assert_eq!(with("puts n.abs, x.abs"), "puts IntAbs(n), FloatAbs(x)");
    assert_eq!(with("puts n.to_f"), "puts (float n)");
    assert_eq!(
        with("puts x.to_i, x.floor, x.ceil, x.round"),
        "puts FloatToI(x), FloatFloor(x), FloatCeil(x), FloatRound(x)"
    );
    assert_eq!(with("puts x.round(2)"), "puts FloatRoundDigits(x, 2)");
    assert_eq!(with("puts x.nan?"), "puts FloatNan(x)");
    assert_eq!(with("puts n.clamp(0, 10)"), "puts IntClamp(n, 0, 10)");
    assert_eq!(
        with("puts x.clamp(0, 1)"),
        "puts FloatClamp(x, (float 0), (float 1))"
    );
    assert_eq!(
        with("puts n.clamp(0, 1.5)"),
        "puts FloatClamp((float n), (float 0), 1.5)"
    );
    assert_eq!(
        with("puts min(n, 2), max(n, 2)"),
        "puts IntMin(n, 2), IntMax(n, 2)"
    );
    assert_eq!(
        with("puts min(n, x), max(1.5, 2)"),
        "puts FloatMin((float n), x), FloatMax(1.5, (float 2))"
    );
    assert_eq!(with("puts n.to_s, x.to_s"), "puts ToS(n), ToS(x)");
    assert_eq!(with("puts -1.abs"), "puts IntAbs(-1)");
}

#[test]
fn math() {
    assert_eq!(
        stmt("puts Math.sqrt(4), Math.sin(0.5), Math.cos(1), Math.tan(1)"),
        "puts Sqrt((float 4)), Sin(0.5), Cos((float 1)), Tan((float 1))"
    );
    assert_eq!(
        stmt("puts Math.atan2(1, 2.0), Math.exp(1), Math.log(2)"),
        "puts Atan2((float 1), 2.0), Exp((float 1)), Log((float 2))"
    );
    assert_eq!(
        stmt("puts Math::PI, Math::E"),
        "puts 3.141592653589793, 2.718281828459045"
    );
}

#[test]
fn string_methods() {
    let with = |body: &str| stmt(&format!("s = \"abc\"\n{body}"));
    assert_eq!(
        with("puts s.length, s.bytesize, s.empty?"),
        "puts StrLength(s), StrBytesize(s), StrEmpty(s)"
    );
    assert_eq!(
        with("puts s.upcase, s.downcase"),
        "puts StrUpcase(s), StrDowncase(s)"
    );
    assert_eq!(
        with("puts s.include?(\"b\"), s.start_with?(\"a\"), s.end_with?(\"c\")"),
        "puts StrInclude(s, \"b\"), StrStartWith(s, \"a\"), StrEndWith(s, \"c\")"
    );
    assert_eq!(
        with("puts s.ljust(5), s.rjust(5)"),
        "puts StrLjust(s, 5), StrRjust(s, 5)"
    );
    assert_eq!(with("puts s.to_i, s.to_f"), "puts StrToI(s), StrToF(s)");
    assert_eq!(with("puts s.to_s"), "puts s");
    assert_eq!(
        with("puts true.to_s, Team::Red.to_s, Vec2().to_s"),
        "puts ToS(true), ToS(Team::Red), ToS(Vec2((float 0), (float 0)))"
    );
}

#[test]
fn random_time_and_exit() {
    assert_eq!(stmt("puts rand, rand(6)"), "puts Rand(), RandInt(6)");
    assert_eq!(stmt("puts rand()"), "puts Rand()");
    assert_eq!(stmt("srand"), "SrandClock()");
    assert_eq!(stmt("srand(42)"), "Srand(42)");
    assert_eq!(stmt("puts clock"), "puts Clock()");
    assert_eq!(stmt("sleep(1)"), "Sleep((float 1))");
    assert_eq!(stmt("exit"), "Exit(0)");
    assert_eq!(stmt("exit(2)"), "Exit(2)");
}

#[test]
fn output() {
    assert_eq!(stmt("puts"), "puts");
    assert_eq!(
        stmt("puts 1, 2.5, \"a\", true, Vec2(), [1]"),
        "puts 1, 2.5, \"a\", true, Vec2((float 0), (float 0)), [1]:[Int]"
    );
    assert_eq!(stmt("print \"a\", 1"), "print \"a\", 1");
}

// ----- names -----

#[test]
fn bare_names_resolve_to_locals_then_functions() {
    assert_eq!(stmt("puts one"), "puts one()");
    assert_eq!(stmt("puts one()"), "puts one()");
    assert_eq!(stmt("greet"), "greet()");
    assert_eq!(stmt("one = 5\nputs one, one()"), "puts one, one()");
    assert_eq!(
        stmt("min = 3\nputs min(1, 2), min"),
        "puts IntMin(1, 2), min"
    );
    assert_eq!(stmt("rand = 3\nputs rand"), "puts rand");
    // A user function replaces a built-in one of the same name.
    assert_eq!(
        last("def max(a: Int, b: Int) -> Int\n  a\nend\nputs max(1, 2)\n"),
        "puts max(1, 2)"
    );
}

#[test]
fn functions_are_used_before_they_are_defined() {
    let text = "puts late + 1\ndef late\n  later * 2\nend\ndef later\n  21\nend\n";
    assert_eq!(last(text), "puts (+ late() 1)");
    assert_eq!(func(text, "late"), "late() -> Int { => (* later() 2) }");
}

#[test]
fn method_bodies_see_the_receiver() {
    let text = DECLS;
    assert_eq!(
        func(text, "Vec2.length"),
        "Vec2.length(self: Vec2) -> Float { => Sqrt((+ (* self.x self.x) (* self.y self.y))) }"
    );
    assert_eq!(
        func(text, "Vec2.+"),
        "Vec2.+(self: Vec2, other: Vec2) -> Vec2 { => Vec2((+ self.x other.x), (+ self.y other.y)) }"
    );
    assert_eq!(
        func(text, "Vec2.-@"),
        "Vec2.-@(self: Vec2) -> Vec2 { => Vec2((- self.x), (- self.y)) }"
    );
    // `self`, and a method of `self` called by its bare name.
    assert_eq!(
        func(text, "Vec2.unit"),
        "Vec2.unit(self: Vec2) -> Vec2 { => Vec2.*(self, (/ (float 1) Vec2.length(self))) }"
    );
    let program = ok(text);
    let method = program.funcs.iter().find(|f| f.name == "unit").unwrap();
    assert_eq!(method.params.len(), 1);
    assert_eq!(method.body.locals[0].kind, tir::LocalKind::Param);
    assert_eq!(program.records[method.owner.unwrap().0].name, "Vec2");
}

#[test]
fn methods_on_components_and_explicit_self() {
    let text = "\
component Health
  hp: Int
  max: Int = 100
  def ratio -> Float
    hp.to_f / self.max
  end
  def low?(limit: Float) -> Bool
    self.ratio < limit
  end
end
h = Health(50)
puts h.ratio, h.low?(0.5)
";
    assert_eq!(
        func(text, "Health.ratio"),
        "Health.ratio(self: Health) -> Float { => (/ (float self.hp) (float self.max)) }"
    );
    assert_eq!(
        func(text, "Health.low?"),
        "Health.low?(self: Health, limit: Float) -> Bool { => (< Health.ratio(self) limit) }"
    );
    assert_eq!(last(text), "puts Health.ratio(h), Health.low?(h, 0.5)");
}

#[test]
fn a_parameter_hides_a_field() {
    let text = "struct S\n  x: Int\n  def plus(x: Int) -> Int\n    x + self.x\n  end\nend\n";
    assert_eq!(
        func(text, "S.plus"),
        "S.plus(self: S, x: Int) -> Int { => (+ x self.x) }"
    );
}

// ----- places -----

#[test]
fn reads_of_places() {
    assert_eq!(stmt("v = Vec2()\nx = v.x"), "let x = v.x");
    assert_eq!(stmt("ORIGIN = Vec2()\nx = ORIGIN.x"), "let x = ORIGIN.x");
    assert_eq!(stmt("x = Score.points"), "let x = Score.points");
    assert_eq!(stmt("x = Score.history[0]"), "let x = Score.history[0]");
    assert_eq!(
        stmt("puts Time.tick, Time.dt, Time.elapsed"),
        "puts Time.tick, Time.dt, Time.elapsed"
    );
    // A field of something that is not a place.
    assert_eq!(
        stmt("x = Vec2(1, 2).x"),
        "let x = (field Vec2((float 1), (float 2)) x)"
    );
    assert_eq!(
        stmt("def origin -> Vec2\n  Vec2()\nend\nx = origin.y"),
        "let x = (field origin() y)"
    );
    assert_eq!(var_type("x = Time.tick", "x"), "Int");
    assert_eq!(var_type("x = Time.elapsed", "x"), "Float");
}

#[test]
fn compact_fields_read_as_int_and_float() {
    assert_eq!(var_type("h = Health()\nx = h.hp", "x"), "Int");
    assert_eq!(var_type("p = Position(1, 2)\nx = p.x", "x"), "Float");
    assert_eq!(stmt("h = Health()\nx = h.hp + 1"), "let x = (+ h.hp 1)");
    assert_eq!(stmt("p = Position(1, 2)\np.x += 1"), "p.x += (float 1)");
    assert_eq!(
        stmt("p = Position(1, 2)\np.x = p.y * 2"),
        "p.x = (* p.y (float 2))"
    );
    // The place keeps its storage type.
    let program = ok(&format!("{DECLS}h = Health()\nh.hp = 7\n"));
    let StmtKind::Assign { place, value } = &program.main.block.stmts[1].kind else {
        panic!("not an assignment");
    };
    assert_eq!(place.ty, Type::Compact(tir::Compact::UInt8));
    assert_eq!(value.ty, Type::Int);
}

#[test]
fn entity_components_are_places() {
    let e = "e = spawn Health\n";
    let with = |body: &str| stmt(&format!("{e}{body}"));
    assert_eq!(with("x = e[Health].hp"), "let x = e[Health].hp");
    assert_eq!(with("h = e[Health]"), "let h = e[Health]");
    assert_eq!(with("e[Health].hp -= 1"), "e[Health]!.hp -= 1");
    assert_eq!(with("e[Health].hp = 5"), "e[Health]!.hp = 5");
    assert_eq!(with("e[Health] = Health(5)"), "e[Health]! = Health(5, 100)");
    assert_eq!(
        with("e.add(Inventory)\ne[Inventory].items.push(1)"),
        "(push e[Inventory]!.items 1)"
    );
    assert_eq!(
        with("e.add(Inventory)\nputs e[Inventory].items.length"),
        "puts ListLength(e[Inventory].items)"
    );
    assert_eq!(
        stmt("on Hit do |hit|\n  hit.target[Health].hp -= hit.damage\nend"),
        "on Hit |hit| { hit.target[Health]!.hp -= hit.damage }"
    );
}

#[test]
fn copies_are_not_views() {
    assert_eq!(
        stmts("e = spawn Health\nhealth = e[Health]\nhealth.hp -= 10")[1..],
        ["let health = e[Health]", "health.hp -= 10"]
    );
}

// ----- constructors and entities -----

#[test]
fn constructors_fill_in_defaults() {
    assert_eq!(stmt("v = Vec2()"), "let v = Vec2((float 0), (float 0))");
    assert_eq!(stmt("v = Vec2(3)"), "let v = Vec2((float 3), (float 0))");
    assert_eq!(stmt("v = Vec2(y: 4)"), "let v = Vec2((float 0), (float 4))");
    assert_eq!(
        stmt("v = Vec2(y: 4, x: 3)"),
        "let v = Vec2((float 3), (float 4))"
    );
    assert_eq!(stmt("h = Health(hp: 50)"), "let h = Health(50, 100)");
    assert_eq!(stmt("e = spawn\nh = Hit(e, 3)"), "let h = Hit(e, 3)");
    assert_eq!(stmt("t = Tick()"), "let t = Tick()");
    assert_eq!(stmt("i = Inventory()"), "let i = Inventory([]:[Int])");
}

#[test]
fn named_fields_are_evaluated_in_the_order_written() {
    assert_eq!(
        stmt("v = Vec2(y: one, x: two)"),
        "let v = (block { let $y = (float one()); let $x = (float two()); => Vec2($x, $y) })"
    );
    // In field order already: no temporaries.
    assert_eq!(
        stmt("v = Vec2(x: one, y: two)"),
        "let v = Vec2((float one()), (float two()))"
    );
}

#[test]
fn defaults_can_use_constants_and_constructors() {
    let text = "\
SPEED = 2.5
struct Vec2
  x: Float = 0
  y: Float = SPEED * 2
end
struct Body
  pos: Vec2 = Vec2(1)
  team: Team = Team::Blue
  path: [Vec2] = [Vec2(), Vec2(1, 1)]
  owner: Entity = Entity.none
end
enum Team
  Red
  Blue
end
b = Body()
";
    assert_eq!(
        last(text),
        "let b = Body(Vec2((float 1), (* SPEED (float 2))), Team::Blue, \
         [Vec2((float 0), (* SPEED (float 2))), Vec2((float 1), (float 1))]:[Vec2], Entity.none)"
    );
}

#[test]
fn spawn_add_and_remove() {
    assert_eq!(stmt("spawn"), "(spawn )");
    assert_eq!(
        stmt("e = spawn Position(0, 0), Health(hp: 50), Player"),
        "let e = (spawn Position((float 0), (float 0)), Health(50, 100), Player)"
    );
    assert_eq!(stmt("e = spawn Health"), "let e = (spawn Health(100, 100))");
    assert_eq!(
        stmt("e = spawn(Player, Frozen)"),
        "let e = (spawn Player, Frozen)"
    );
    assert_eq!(
        stmt("START = Velocity(1, 2)\ne = spawn START"),
        "let e = (spawn START)"
    );
    let e = "e = spawn\n";
    let with = |body: &str| stmt(&format!("{e}{body}"));
    assert_eq!(
        with("e.add(Health(5), Player, Inventory)"),
        "(add e Health(5, 100), Player, Inventory([]:[Int]))"
    );
    assert_eq!(
        with("e.remove(Health, Player)"),
        "(remove e Health, Player)"
    );
    assert_eq!(with("e.despawn"), "(despawn e)");
    assert_eq!(
        with("puts e.alive?, e.has?(Health), e.has?(Player)"),
        "puts (alive e), (has e Health), (has e Player)"
    );
    assert_eq!(with("puts e, e.to_s"), "puts e, ToS(e)");
    assert_eq!(stmt("e = Entity.none"), "let e = Entity.none");
    assert_eq!(
        stmt("e = spawn\ne.despawn if !e.alive?"),
        "(if (! (alive e)) { (despawn e) } { })"
    );
}

#[test]
fn emit() {
    assert_eq!(stmt("e = spawn\nemit Hit(e, 3)"), "emit Hit Hit(e, 3)");
    assert_eq!(stmt("emit Tick"), "emit Tick Tick()");
    assert_eq!(stmt("emit Tick()"), "emit Tick Tick()");
    assert_eq!(stmt("e = spawn\nhit = Hit(e, 1)\nemit hit"), "emit Hit hit");
}

// ----- queries -----

#[test]
fn the_first_program_of_the_reference() {
    let text = "\
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
    puts \"#{e} is at (#{pos.x}, #{pos.y})\"
  end
end

spawn Position(0, 0), Velocity(1, 2)
spawn Position(5, 5), Velocity(-1, 0), Frozen

Time.dt = 0.5
3.times do
  World.tick
end
";
    assert_eq!(
        system(text, "move"),
        ["each (Position!, Velocity without Frozen) |pos, vel| { \
          pos.x += (* vel.dx Time.dt); pos.y += (* vel.dy Time.dt) }"]
    );
    assert_eq!(
        system(text, "report"),
        ["each (Entity, Position) |e, pos| { \
          puts (concat ToS(e) \" is at (\" ToS(pos.x) \", \" ToS(pos.y) \")\") }"]
    );
    assert_eq!(
        main_stmts(text),
        [
            "(spawn Position((float 0), (float 0)), Velocity((float 1), (float 2)))",
            "(spawn Position((float 5), (float 5)), Velocity((float -1), (float 0)), Frozen)",
            "Time.dt = 0.5",
            "count _ 0..3 { World.tick }",
        ]
    );
    let program = ok(text);
    let names: Vec<&str> = program.systems.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["move", "report"]);
    let locals = &program.systems[0].body.locals;
    assert_eq!(locals[0].kind, tir::LocalKind::View);
    assert_eq!(locals[1].kind, tir::LocalKind::View);
    let locals = &program.systems[1].body.locals;
    assert_eq!(locals[0].kind, tir::LocalKind::Var);
    assert_eq!(locals[0].ty, Type::Entity);
    assert_eq!(locals[1].kind, tir::LocalKind::View);
}

#[test]
fn query_terms_record_writes() {
    let each = |body: &str| {
        let text = format!(
            "each Entity, Position, Health, Inventory, with: Player, without: [Frozen] do |e, pos, health, inv|\n{body}\nend"
        );
        let printed = stmt(&text);
        printed[..printed.find(" |").unwrap()].to_string()
    };
    assert_eq!(
        each("puts pos.x"),
        "each (Entity, Position, Health, Inventory with Player without Frozen)"
    );
    assert_eq!(
        each("pos.x = 1"),
        "each (Entity, Position!, Health, Inventory with Player without Frozen)"
    );
    assert_eq!(
        each("health.hp -= 1"),
        "each (Entity, Position, Health!, Inventory with Player without Frozen)"
    );
    assert_eq!(
        each("pos = Position(1, 2)"),
        "each (Entity, Position!, Health, Inventory with Player without Frozen)"
    );
    assert_eq!(
        each("inv.items.push(1)\ninv.items[0] = 2"),
        "each (Entity, Position, Health, Inventory! with Player without Frozen)"
    );
    assert_eq!(
        each("x = inv.items.pop"),
        "each (Entity, Position, Health, Inventory! with Player without Frozen)"
    );
    assert_eq!(
        each("copy = health\ncopy.hp = 1\nputs inv.items.length"),
        "each (Entity, Position, Health, Inventory with Player without Frozen)"
    );
    // Writing through `e[C]` is recorded on that place, not on the term.
    assert_eq!(
        each("e[Health].hp = 1"),
        "each (Entity, Position, Health, Inventory with Player without Frozen)"
    );
}

#[test]
fn the_entity_parameter_is_an_ordinary_variable() {
    assert_eq!(
        stmt("each Entity, with: Player do |e|\n  e = Entity.none\nend"),
        "each (Entity with Player) |e| { e = Entity.none }"
    );
}

#[test]
fn queries_nest_and_run_anywhere() {
    assert_eq!(
        func(
            &format!(
                "{DECLS}def crowd -> Int\n  n = 0\n  each Position do |a|\n    each Entity, Position do |e, b|\n      n += 1\n    end\n  end\n  n\nend\n"
            ),
            "crowd"
        ),
        "crowd() -> Int { let n = 0; each (Position) |a| { each (Entity, Position) |e, b| { n += 1 } }; => n }"
    );
}

#[test]
fn world_operations() {
    assert_eq!(stmt("World.tick"), "World.tick");
    assert_eq!(stmt("World.run"), "World.run");
    assert_eq!(stmt("World.stop"), "World.stop");
    assert_eq!(stmt("puts World.count"), "puts World.count");
    assert_eq!(stmt("puts World.count()"), "puts World.count");
    assert_eq!(
        stmt("puts World.count(Position, Velocity)"),
        "puts (count Position, Velocity)"
    );
    assert_eq!(
        stmt("puts World.count(Entity, with: Player, without: [Frozen, Health])"),
        "puts (count Entity with Player without Frozen, Health)"
    );
    assert_eq!(
        stmt("puts World.count(Health, with: [Player])"),
        "puts (count Health with Player)"
    );
}

#[test]
fn on_blocks() {
    assert_eq!(
        stmt("on Hit do |hit|\n  next unless hit.target.alive?\n  puts hit.damage\nend"),
        "on Hit |hit| { (if (! (alive hit.target)) { next } { }); puts hit.damage }"
    );
    assert_eq!(stmt("on Tick do\n  break\nend"), "on Tick |_| { break }");
}

// ----- control flow -----

#[test]
fn if_and_its_shorthands() {
    let n = "n = 3\n";
    let with = |body: &str| stmt(&format!("{n}{body}"));
    assert_eq!(
        with("if n > 1\n  puts 1\nend"),
        "(if (> n 1) { puts 1 } { })"
    );
    assert_eq!(
        with("if n > 1\n  puts 1\nelsif n > 0\n  puts 2\nelse\n  puts 3\nend"),
        "(if (> n 1) { puts 1 } { (if (> n 0) { puts 2 } { puts 3 }) })"
    );
    assert_eq!(
        with("unless n > 1\n  puts 1\nelse\n  puts 2\nend"),
        "(if (! (> n 1)) { puts 1 } { puts 2 })"
    );
    assert_eq!(with("puts 1 if n > 1"), "(if (> n 1) { puts 1 } { })");
    assert_eq!(
        with("puts 1 unless n > 1"),
        "(if (! (> n 1)) { puts 1 } { })"
    );
    assert_eq!(with("n += 1 if n > 1"), "(if (> n 1) { n += 1 } { })");
    assert_eq!(
        with("x = n > 1 ? 1 : 2"),
        "let x = (if (> n 1) { => 1 } { => 2 })"
    );
    assert_eq!(
        with("x = n > 1 ? 1 : 2.5"),
        "let x = (if (> n 1) { => (float 1) } { => 2.5 })"
    );
    assert_eq!(
        with("n > 1 ? greet : greet"),
        "(if (> n 1) { greet() } { greet() })"
    );
}

#[test]
fn if_as_a_value() {
    let n = "n = 3\n";
    let with = |body: &str| stmt(&format!("{n}{body}"));
    assert_eq!(
        with("label = if n > 50 then \"healthy\" else \"hurt\" end"),
        "let label = (if (> n 50) { => \"healthy\" } { => \"hurt\" })"
    );
    assert_eq!(
        with("x = if n > 1\n  puts n\n  1\nelsif n > 0\n  2.5\nelse\n  3\nend"),
        "let x = (if (> n 1) { puts n; => (float 1) } { => (if (> n 0) { => 2.5 } { => (float 3) }) })"
    );
    assert_eq!(
        with("x: Float = if n > 1 then 1 else 2 end"),
        "let x = (if (> n 1) { => (float 1) } { => (float 2) })"
    );
    assert_eq!(
        with("x = unless n > 1 then 1 else 2 end"),
        "let x = (if (! (> n 1)) { => 1 } { => 2 })"
    );
    // As a statement the same `if` has no value.
    assert_eq!(
        with("if n > 50 then \"healthy\" else \"hurt\" end\nputs n").as_str(),
        "puts n"
    );
    assert_eq!(
        stmts("n = 3\nif n > 50 then \"healthy\" else \"hurt\" end")[1],
        "(if (> n 50) { \"healthy\" } { \"hurt\" })"
    );
}

#[test]
fn a_branch_that_leaves_needs_no_value() {
    assert_eq!(
        func(
            "def f(n: Int) -> Int\n  x = if n > 0 then n else return 0 end\n  x * 2\nend\n",
            "f"
        ),
        "f(n: Int) -> Int { let x = (if (> n 0) { => n } { return 0; => 0 }); => (* x 2) }"
    );
    assert_eq!(
        func(
            "def f(n: Int) -> Int\n  if n == 0\n    return 1\n  else\n    2\n  end\nend\n",
            "f"
        ),
        "f(n: Int) -> Int { => (if (== n 0) { return 1; => 0 } { => 2 }) }"
    );
    assert_eq!(
        func(
            "def f(n: Int) -> Int\n  if n == 0\n    return 1\n  else\n    return 2\n  end\nend\n",
            "f"
        ),
        "f(n: Int) -> Int { (if (== n 0) { return 1 } { return 2 }) }"
    );
}

#[test]
fn loops() {
    let n = "n = 3\n";
    let with = |body: &str| stmt(&format!("{n}{body}"));
    assert_eq!(
        with("while n > 0\n  n -= 1\nend"),
        "while (> n 0) { n -= 1 }"
    );
    assert_eq!(
        with("until n == 0\n  n -= 1\nend"),
        "while (! (== n 0)) { n -= 1 }"
    );
    assert_eq!(
        with("loop do\n  break if n > 5\n  n += 1\nend"),
        "while true { (if (> n 5) { break } { }); n += 1 }"
    );
    assert_eq!(
        with("3.times do |i|\n  puts i\nend"),
        "count i 0..3 { puts i }"
    );
    assert_eq!(with("n.times do\nend"), "count _ 0..n { }");
    assert_eq!(
        with("1.upto(n) do |i|\n  next if i == 2\nend"),
        "count i 1..=n { (if (== i 2) { next } { }) }"
    );
    assert_eq!(with("n.downto(1) do |i|\nend"), "count i n..=1 down { }");
    assert_eq!(with("-1.upto(1) do |i|\nend"), "count i -1..=1 { }");
}

#[test]
fn list_iteration() {
    let names = "names = [\"a\", \"b\"]\n";
    let with = |body: &str| stmt(&format!("{names}{body}"));
    assert_eq!(
        with("names.each do |name|\n  puts name\nend"),
        "for name, _ in names { puts name }"
    );
    assert_eq!(
        with("names.each_with_index do |name, i|\n  puts i\nend"),
        "for name, i in names { puts i }"
    );
    assert_eq!(with("names.each do\nend"), "for _, _ in names { }");
    assert_eq!(
        with("[1, 2].each do |x|\n  puts x\nend"),
        "for x, _ in [1, 2]:[Int] { puts x }"
    );
    assert_eq!(
        with("xs = [1]\nxs.length.times do |i|\n  xs[i] *= 2\nend"),
        "count i 0..ListLength(xs) { xs[i] *= 2 }"
    );
    // The element is a copy with the element's value type.
    let program = ok("xs: [Float32] = [1]\nxs.each_with_index do |x, i|\nend\n");
    let locals = &program.main.locals;
    assert_eq!(locals[1].name, "x");
    assert_eq!(locals[1].ty, Type::Float);
    assert_eq!(locals[1].kind, tir::LocalKind::Var);
    assert_eq!(locals[2].ty, Type::Int);
}

#[test]
fn case_becomes_a_chain_of_ifs() {
    let n = "n = 3\nt = Team::Red\n";
    let with = |body: &str| stmt(&format!("{n}{body}"));
    assert_eq!(
        with("case n\nwhen 1 then puts 1\nend"),
        "(block { let $case = n; (if (== $case 1) { puts 1 } { }) })"
    );
    assert_eq!(
        with("x = case n\nwhen 1 then \"one\"\nwhen 2, 3 then \"few\"\nelse \"many\"\nend"),
        "let x = (block { let $case = n; => (if (== $case 1) { => \"one\" } \
         { => (if (|| (== $case 2) (== $case 3)) { => \"few\" } { => \"many\" }) }) })"
    );
    // An enum with every variant covered: the last arm is the `else`.
    assert_eq!(
        with("case t\nwhen Team::Red then puts 1\nwhen Team::Blue, Team::Green\n  puts 2\nend"),
        "(block { let $case = t; (if (== $case Team::Red) { puts 1 } { puts 2 }) })"
    );
    assert_eq!(
        with(
            "x = case t\nwhen Team::Red then 1\nwhen Team::Blue then 2.5\nwhen Team::Green then 3\nend"
        ),
        "let x = (block { let $case = t; => (if (== $case Team::Red) { => (float 1) } \
         { => (if (== $case Team::Blue) { => 2.5 } { => (float 3) }) }) })"
    );
    assert_eq!(
        with("case \"a\" + \"b\"\nwhen \"ab\" then puts 1\nelse\n  puts 2\nend"),
        "(block { let $case = (+ \"a\" \"b\"); (if (== $case \"ab\") { puts 1 } { puts 2 }) })"
    );
    assert_eq!(
        with("case n > 1\nwhen true then puts 1\nwhen false then puts 2\nend"),
        "(block { let $case = (> n 1); (if (== $case true) { puts 1 } { (if (== $case false) { puts 2 } { }) }) })"
    );
    assert_eq!(
        func(
            "enum Team\n  Red\n  Blue\nend\ndef name(team: Team)\n  case team\n  when Team::Red then \"red\"\n  when Team::Blue then \"blue\"\n  end\nend\n",
            "name"
        ),
        "name(team: Team) -> String { => (block { let $case = team; \
         => (if (== $case Team::Red) { => \"red\" } { => \"blue\" }) }) }"
    );
}

// ----- functions -----

#[test]
fn return_types_are_inferred() {
    assert_eq!(func(DECLS, "greet"), "greet() -> Void { puts \"hello\" }");
    assert_eq!(
        func(DECLS, "half"),
        "half(x: Float) -> Float { => (/ x (float 2)) }"
    );
    assert_eq!(
        func("def f(n: Int)\n  n + 1\nend\n", "f"),
        "f(n: Int) -> Int { => (+ n 1) }"
    );
    assert_eq!(
        func(
            "def f(n: Int)\n  if n > 0 then \"pos\" else \"neg\" end\nend\n",
            "f"
        ),
        "f(n: Int) -> String { => (if (> n 0) { => \"pos\" } { => \"neg\" }) }"
    );
    assert_eq!(
        func(
            "def f(n: Int)\n  return \"zero\" if n == 0\n  \"other\"\nend\n",
            "f"
        ),
        "f(n: Int) -> String { (if (== n 0) { return \"zero\" } { }); => \"other\" }"
    );
    // No value on some branch: the function returns nothing.
    assert_eq!(
        func(
            "def f(n: Int)\n  if n > 0\n    puts 1\n  else\n    n\n  end\nend\n",
            "f"
        ),
        "f(n: Int) -> Void { (if (> n 0) { puts 1 } { n }) }"
    );
    assert_eq!(
        func("def f(n: Int)\n  puts n if n > 0\nend\n", "f"),
        "f(n: Int) -> Void { (if (> n 0) { puts n } { }) }"
    );
    // Only bare `return`s: the last expression is not a result.
    assert_eq!(
        func("def f(n: Int)\n  return if n == 0\n  n + 1\nend\n", "f"),
        "f(n: Int) -> Void { (if (== n 0) { return } { }); (+ n 1) }"
    );
    assert_eq!(func("def f\nend\n", "f"), "f() -> Void {  }");
    assert_eq!(
        func(
            "def f(xs: [Int8], t: Team, e: Entity) -> [[Float32]]\n  [[1]]\nend\nenum Team\n  A\nend\n",
            "f"
        ),
        "f(xs: [Int8], t: Team, e: Entity) -> [[Float32]] { => [[(float 1)]:[Float32]]:[[Float32]] }"
    );
}

#[test]
fn recursion_with_a_declared_type() {
    assert_eq!(
        func(
            "def fact(n: Int) -> Int\n  if n <= 1 then 1 else n * fact(n - 1) end\nend\n",
            "fact"
        ),
        "fact(n: Int) -> Int { => (if (<= n 1) { => 1 } { => (* n fact((- n 1))) }) }"
    );
}

#[test]
fn a_recursive_procedure_needs_no_declared_type() {
    assert_eq!(
        func(
            "def walk(n: Int)\n  return if n == 0\n  puts n\n  walk(n - 1)\nend\n",
            "walk"
        ),
        "walk(n: Int) -> Void { (if (== n 0) { return } { }); puts n; walk((- n 1)) }"
    );
    assert_eq!(
        func(
            "def walk(n: Int)\n  walk(n - 1) if n > 0\n  puts n\nend\n",
            "walk"
        ),
        "walk(n: Int) -> Void { (if (> n 0) { walk((- n 1)) } { }); puts n }"
    );
}

#[test]
fn a_call_typed_before_its_function_gets_the_type_afterwards() {
    let text = "\
def a(n: Int)
  b(n)
  1
end
def b(n: Int)
  a(n) if n > 0
  2.5
end
a(1)
";
    let program = ok(text);
    let b = program.funcs.iter().find(|f| f.name == "b").unwrap();
    assert_eq!(b.ret, Type::Float);
    let mut block = b.body.block.clone();
    let mut call_types = Vec::new();
    walk::block(&mut block, &mut |node| {
        if let walk::Node::Expr(expr) = node
            && let ExprKind::Call { .. } = expr.kind
        {
            call_types.push(expr.ty.clone());
        }
    });
    assert_eq!(call_types, [Type::Int]);
}

// ----- the program -----

#[test]
fn declarations_are_listed() {
    let program = ok(DECLS);
    let names = |components: &[tir::Component]| -> Vec<String> {
        components
            .iter()
            .map(|c| {
                format!(
                    "{}{}",
                    c.name,
                    if c.record.is_some() { "" } else { " (tag)" }
                )
            })
            .collect()
    };
    assert_eq!(
        names(&program.components),
        [
            "Position",
            "Velocity",
            "Health",
            "Inventory",
            "Player (tag)",
            "Frozen (tag)"
        ]
    );
    let records: Vec<String> = program
        .records
        .iter()
        .map(|r| format!("{}: {:?}", r.name, r.kind))
        .collect();
    assert_eq!(
        records,
        [
            "Position: Component(ComponentId(0))",
            "Velocity: Component(ComponentId(1))",
            "Health: Component(ComponentId(2))",
            "Inventory: Component(ComponentId(3))",
            "Vec2: Struct",
            "Hit: Event(EventId(0))",
            "Tick: Event(EventId(1))",
        ]
    );
    let health = &program.records[2];
    assert_eq!(health.fields[0].name, "hp");
    assert_eq!(health.fields[0].ty, Type::Compact(tir::Compact::UInt8));
    assert_eq!(health.fields[1].ty, Type::Int);
    assert_eq!(
        program.records[3].fields[0].ty,
        Type::List(Box::new(Type::Int))
    );
    assert_eq!(program.enums[0].name, "Team");
    assert_eq!(program.enums[0].variants, ["Red", "Blue", "Green"]);
    assert_eq!(program.events[0].name, "Hit");
    assert_eq!(program.events[1].record, RecordId(6));
    assert!(program.records[6].fields.is_empty());

    let dump = Dump {
        program: &program,
        locals: &[],
    };
    let score = &program.resources[0];
    assert_eq!(score.name, "Score");
    let fields: Vec<String> = score
        .fields
        .iter()
        .map(|f| format!("{}: {} = {}", f.name, dump.ty(&f.ty), dump.expr(&f.init)))
        .collect();
    assert_eq!(fields, ["points: Int = 0", "history: [Int] = []:[Int]"]);
}

#[test]
fn constants_are_listed_in_order() {
    let program = ok(&format!(
        "{DECLS}GRAVITY = 9.81\nORIGIN = Vec2(0, 0)\nDOUBLE = GRAVITY * 2\nNAMES = [\"a\"]\nputs DOUBLE\n"
    ));
    let dump = Dump {
        program: &program,
        locals: &[],
    };
    let consts: Vec<String> = program
        .consts
        .iter()
        .map(|c| format!("{}: {} = {}", c.name, dump.ty(&c.ty), dump.expr(&c.init)))
        .collect();
    assert_eq!(
        consts,
        [
            "GRAVITY: Float = 9.81",
            "ORIGIN: Vec2 = Vec2((float 0), (float 0))",
            "DOUBLE: Float = (* GRAVITY (float 2))",
            "NAMES: [String] = [\"a\"]:[String]",
        ]
    );
    assert_eq!(dump_body(&program, &program.main), ["puts DOUBLE"]);
}

#[test]
fn top_level_statements_and_systems_keep_their_order() {
    let text = "\
puts 1
system b
end
x = 2
def f
end
system a
end
puts x
";
    assert_eq!(main_stmts(text), ["puts 1", "let x = 2", "puts x"]);
    let program = ok(text);
    let names: Vec<&str> = program.systems.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["b", "a"]);
}

#[test]
fn variables_are_scoped_to_blocks() {
    // The same name in two blocks is two variables, each with its own type.
    let program = ok(
        "if true\n  x = 1\nelse\n  x = \"a\"\nend\n3.times do |i|\n  y = i\nend\n2.times do |i|\n  y = 1.5\nend\n",
    );
    let locals: Vec<String> = program
        .main
        .locals
        .iter()
        .map(|l| format!("{}: {:?}", l.name, l.ty))
        .collect();
    assert_eq!(
        locals,
        [
            "x: Int",
            "x: String",
            "i: Int",
            "y: Int",
            "i: Int",
            "y: Float"
        ]
    );
    // A variable created before a block is the same one inside it.
    assert_eq!(
        main_stmts("x = 1\nif true\n  x = 2\nend\n"),
        ["let x = 1", "(if true { x = 2 } { })"]
    );
}

#[test]
fn the_examples_of_the_reference_check() {
    ok(&format!(
        "{DECLS}{}",
        "\
component Attack
  target: Entity
  power: Int
end
tag Dead
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
system reap
  each Entity, Health do |e, health|
    e.despawn if health.hp <= 0
  end
end
def clamp(v: Float, lo: Float, hi: Float) -> Float
  return lo if v < lo
  return hi if v > hi
  v
end
GRAVITY = 9.81
ORIGIN = Vec2(0, 0)
count = 0
name = \"midnight\"
ratio: Float = 1
scores: [Int] = []
a = [1, 2, 3]
b = a
b.push(4)
puts a.length
player = spawn Position(0, 0), Health(hp: 50), Player
target = player
health = target[Health]
health.hp -= 10
target[Health].hp -= 10
each Entity, Health, with: Player, without: [Dead, Frozen] do |e, health2|
  puts \"#{e} has #{health2.hp} hp\"
end
Score.points += 10
xs = [1, 2]
xs.length.times do |i| xs[i] *= 2 end
puts clamp(count, 0, 1), -7 / 2, -7 % 3, -2 ** 2
World.run
"
    ));
}

#[test]
fn a_program_that_names_the_window_has_one() {
    let program = ok("system s\n  on WindowClosed do\n    puts 1\n  end\nend\n");
    let window = program.window.expect("a window");
    assert_eq!(program.resources[window.resource.0].name, "Window");
    let fields: Vec<&str> = program.resources[window.resource.0]
        .fields
        .iter()
        .map(|field| field.name.as_str())
        .collect();
    assert_eq!(
        fields,
        [
            "width",
            "height",
            "pixel_width",
            "pixel_height",
            "background",
            "open",
            "focused"
        ]
    );
    assert_eq!(program.records[window.color.0].name, "Color");
    assert_eq!(program.events[window.resized.0].name, "WindowResized");
    assert_eq!(program.events[window.closed.0].name, "WindowClosed");
    let mouse: Vec<&str> = program.resources[window.mouse.0]
        .fields
        .iter()
        .map(|field| field.name.as_str())
        .collect();
    assert_eq!(mouse, ["x", "y", "wheel_x", "wheel_y", "inside"]);
    assert_eq!(program.enums[window.key.0].name, "Key");
    assert_eq!(program.enums[window.key.0].variants.len(), 105);
    assert_eq!(program.enums[window.button.0].name, "MouseButton");
    let events = [
        (window.key_pressed, "KeyPressed"),
        (window.key_released, "KeyReleased"),
        (window.mouse_pressed, "MousePressed"),
        (window.mouse_released, "MouseReleased"),
        (window.focused, "WindowFocused"),
        (window.unfocused, "WindowUnfocused"),
    ];
    for (id, name) in events {
        assert_eq!(program.events[id.0].name, name);
    }
}

#[test]
fn a_program_that_names_the_keyboard_or_the_mouse_has_a_window() {
    for text in [
        "puts Keyboard.down?(Key::A)\n",
        "puts Mouse.x\n",
        "system s\n  on MousePressed do |m|\n    puts m.x\n  end\nend\n",
        "system s\n  on WindowFocused do\n    puts 1\n  end\nend\n",
    ] {
        assert!(ok(text).window.is_some(), "{text}");
    }
}

#[test]
fn the_enums_alone_give_no_window() {
    let program = ok("puts Key::A, MouseButton::Left\n");
    assert!(program.window.is_none() && program.resources.is_empty());
    let names: Vec<&str> = program.enums.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, ["Key", "MouseButton"]);
}

#[test]
fn color_alone_is_a_struct_and_no_window() {
    let program = ok("puts Color(1, 2, 3)\n");
    assert!(program.window.is_none());
    assert!(program.resources.is_empty() && program.events.is_empty());
    let names: Vec<&str> = program.records.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names, ["Color"]);
}

#[test]
fn a_program_that_names_none_of_them_has_no_built_in_declarations() {
    let program = ok("puts 1\n");
    assert!(program.window.is_none() && program.records.is_empty());
}

#[test]
fn window_open_is_a_statement_of_its_own() {
    assert_eq!(
        stmts("Window.open(\"t\", 3, 4)\nputs Window.open?"),
        ["Window.open \"t\" 3 4", "puts Window.open"]
    );
}
