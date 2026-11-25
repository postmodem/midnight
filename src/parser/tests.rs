//! Parser tests. Trees are compared in a compact printed form in which
//! every operator is parenthesized and every block is in braces.

use super::*;

const FILE: FileId = FileId(0);

fn items(text: &str) -> Vec<Item> {
    parse(FILE, text).unwrap_or_else(|error| {
        panic!(
            "{text:?}: {} at {}..{} ({:?})",
            error.message, error.span.start, error.span.end, error.label
        )
    })
}

/// Each item of `text`, printed.
fn dump_items(text: &str) -> Vec<String> {
    items(text).iter().map(item).collect()
}

/// The items of `text`, printed and joined with `; `.
fn dump(text: &str) -> String {
    dump_items(text).join("; ")
}

/// The first statement of `text`.
fn first_stmt(text: &str) -> Stmt {
    match items(text).into_iter().next() {
        Some(Item::Stmt(stmt)) => stmt,
        other => panic!("not a statement: {other:?}"),
    }
}

/// The expression that is the first statement of `text`.
fn first_expr(text: &str) -> Expr {
    match first_stmt(text).kind {
        StmtKind::Expr(expr) => expr,
        other => panic!("not an expression: {other:?}"),
    }
}

fn error(text: &str) -> Diagnostic {
    parse(FILE, text).expect_err(text)
}

/// The source text a span covers.
fn marked(text: &str, span: Span) -> &str {
    &text[span.start as usize..span.end as usize]
}

/// Checks the first error of `text`: its message, the text it marks and its
/// label.
#[track_caller]
fn assert_error(text: &str, message: &str, at: &str, label: &str) {
    let error = error(text);
    assert_eq!(error.message, message, "{text:?}");
    assert_eq!(marked(text, error.span), at, "{text:?}");
    assert_eq!(error.label.as_deref(), Some(label), "{text:?}");
}

// Printing

fn list<V>(items: &[V], show: impl Fn(&V) -> String) -> String {
    items.iter().map(show).collect::<Vec<_>>().join(", ")
}

fn braces(parts: Vec<String>) -> String {
    if parts.is_empty() {
        "{}".to_string()
    } else {
        format!("{{ {} }}", parts.join("; "))
    }
}

fn names(idents: &[Ident]) -> String {
    list(idents, |ident| ident.name.clone())
}

fn args(args: &[Arg]) -> String {
    list(args, |arg| match &arg.name {
        Some(name) => format!("{}: {}", name.name, expr(&arg.value)),
        None => expr(&arg.value),
    })
}

fn block(block: &Block) -> String {
    braces(block.stmts.iter().map(stmt).collect())
}

fn ty(ty_expr: &TypeExpr) -> String {
    match &ty_expr.kind {
        TypeExprKind::Name(name) => name.clone(),
        TypeExprKind::List(element) => format!("[{}]", ty(element)),
    }
}

fn expr(e: &Expr) -> String {
    match &e.kind {
        ExprKind::Int(value) => value.to_string(),
        ExprKind::Float(value) => format!("{value:?}"),
        ExprKind::Bool(value) => value.to_string(),
        ExprKind::Str(parts) => {
            let mut out = String::from("\"");
            for part in parts {
                match part {
                    StrPart::Lit(text) => out.push_str(text),
                    StrPart::Expr(inner) => out.push_str(&format!("#{{{}}}", expr(inner))),
                }
            }
            out.push('"');
            out
        }
        ExprKind::List(items) => format!("[{}]", list(items, expr)),
        ExprKind::SelfRef => "self".to_string(),
        ExprKind::Var(name) | ExprKind::Const(name) => name.clone(),
        ExprKind::Call { name, args: a } | ExprKind::Construct { name, args: a } => {
            format!("{}({})", name.name, args(a))
        }
        ExprKind::Member {
            recv,
            name,
            args: a,
            block: b,
        } => {
            let mut out = format!("{}.{}", expr(recv), name.name);
            if let Some(a) = a {
                out.push_str(&format!("({})", args(a)));
            }
            if let Some(b) = b {
                out.push_str(&format!(" do |{}| {}", names(&b.params), block(&b.body)));
            }
            out
        }
        ExprKind::Index { recv, index } => format!("{}[{}]", expr(recv), expr(index)),
        ExprKind::Path { recv, name } => format!("{}::{}", expr(recv), name.name),
        ExprKind::Unary { op, expr: operand } => {
            let op = match op {
                UnaryOp::Neg => "-",
                UnaryOp::Not => "!",
            };
            format!("({op}{})", expr(operand))
        }
        ExprKind::Binary { op, lhs, rhs } => {
            let op = match op {
                BinaryOp::Add => "+",
                BinaryOp::Sub => "-",
                BinaryOp::Mul => "*",
                BinaryOp::Div => "/",
                BinaryOp::Mod => "%",
                BinaryOp::Pow => "**",
                BinaryOp::Eq => "==",
                BinaryOp::Ne => "!=",
                BinaryOp::Lt => "<",
                BinaryOp::Le => "<=",
                BinaryOp::Gt => ">",
                BinaryOp::Ge => ">=",
                BinaryOp::And => "&&",
                BinaryOp::Or => "||",
            };
            format!("({} {op} {})", expr(lhs), expr(rhs))
        }
        ExprKind::Ternary { cond, then, else_ } => {
            format!("({} ? {} : {})", expr(cond), expr(then), expr(else_))
        }
        ExprKind::Spawn(components) => format!("spawn({})", list(components, expr)),
        ExprKind::If {
            cond,
            negate,
            then,
            else_,
        } => {
            let word = if *negate { "unless" } else { "if" };
            let mut out = format!("{word} {} {}", expr(cond), block(then));
            if let Some(else_) = else_ {
                out.push_str(&format!(" else {}", block(else_)));
            }
            out
        }
        ExprKind::Case {
            subject,
            arms,
            else_,
        } => {
            let mut out = format!("case {}", expr(subject));
            for arm in arms {
                out.push_str(&format!(
                    " when {} {}",
                    list(&arm.values, expr),
                    block(&arm.body)
                ));
            }
            if let Some(else_) = else_ {
                out.push_str(&format!(" else {}", block(else_)));
            }
            out
        }
    }
}

fn command(word: &str, values: &[Expr]) -> String {
    if values.is_empty() {
        word.to_string()
    } else {
        format!("{word} {}", list(values, expr))
    }
}

fn stmt(s: &Stmt) -> String {
    match &s.kind {
        StmtKind::Expr(e) => expr(e),
        StmtKind::VarDecl {
            name,
            ty: declared,
            value,
        } => format!("{}: {} = {}", name.name, ty(declared), expr(value)),
        StmtKind::Assign { target, op, value } => {
            let op = match op {
                AssignOp::Set => "=",
                AssignOp::Add => "+=",
                AssignOp::Sub => "-=",
                AssignOp::Mul => "*=",
                AssignOp::Div => "/=",
                AssignOp::Mod => "%=",
            };
            format!("{} {op} {}", expr(target), expr(value))
        }
        StmtKind::Return(None) => "return".to_string(),
        StmtKind::Return(Some(value)) => format!("return {}", expr(value)),
        StmtKind::Break => "break".to_string(),
        StmtKind::Next => "next".to_string(),
        StmtKind::Puts(values) => command("puts", values),
        StmtKind::Print(values) => command("print", values),
        StmtKind::Emit(event) => format!("emit {}", expr(event)),
        StmtKind::While { cond, negate, body } => {
            let word = if *negate { "until" } else { "while" };
            format!("{word} {} {}", expr(cond), block(body))
        }
        StmtKind::Loop { body } => format!("loop {}", block(body)),
        StmtKind::Each {
            query,
            params,
            body,
        } => {
            let mut out = format!("each {}", names(&query.terms));
            if !query.with.is_empty() {
                out.push_str(&format!(" with [{}]", names(&query.with)));
            }
            if !query.without.is_empty() {
                out.push_str(&format!(" without [{}]", names(&query.without)));
            }
            out.push_str(&format!(" |{}| {}", names(params), block(body)));
            out
        }
        StmtKind::On {
            event,
            params,
            body,
        } => format!("on {} |{}| {}", event.name, names(params), block(body)),
    }
}

fn func(f: &FuncDecl) -> String {
    let params = list(&f.params, |param| {
        format!("{}: {}", param.name.name, ty(&param.ty))
    });
    let ret = match &f.ret {
        Some(ret) => format!(" -> {}", ty(ret)),
        None => String::new(),
    };
    format!("def {}({params}){ret} {}", f.name.name, block(&f.body))
}

fn item(item: &Item) -> String {
    match item {
        Item::Require(require) => format!("require {:?}", require.path),
        Item::Record(record) => {
            let word = match record.kind {
                RecordKind::Struct => "struct",
                RecordKind::Component => "component",
                RecordKind::Resource => "resource",
                RecordKind::Event => "event",
            };
            let mut members: Vec<String> = record
                .fields
                .iter()
                .map(|field| {
                    let default = match &field.default {
                        Some(default) => format!(" = {}", expr(default)),
                        None => String::new(),
                    };
                    format!("{}: {}{default}", field.name.name, ty(&field.ty))
                })
                .collect();
            members.extend(record.methods.iter().map(func));
            format!("{word} {} {}", record.name.name, braces(members))
        }
        Item::Enum(decl) => format!("enum {} {{ {} }}", decl.name.name, names(&decl.variants)),
        Item::Tag(tag) => format!("tag {}", tag.name.name),
        Item::Def(f) => func(f),
        Item::System(system) => format!("system {} {}", system.name.name, block(&system.body)),
        Item::Const(constant) => {
            format!("const {} = {}", constant.name.name, expr(&constant.value))
        }
        Item::Stmt(s) => stmt(s),
    }
}

/// Checks that each source prints as expected.
#[track_caller]
fn assert_dumps(cases: &[(&str, &str)]) {
    for (source, expected) in cases {
        assert_eq!(dump(source), *expected, "{source:?}");
    }
}

// Expressions

#[test]
fn binary_operator_precedence() {
    assert_dumps(&[
        ("1 + 2 * 3", "(1 + (2 * 3))"),
        ("1 * 2 + 3", "((1 * 2) + 3)"),
        ("1 - 2 - 3", "((1 - 2) - 3)"),
        ("a % b / c", "((a % b) / c)"),
        ("a || b && c", "(a || (b && c))"),
        ("a && b || c && d", "((a && b) || (c && d))"),
        ("a == b && c != d", "((a == b) && (c != d))"),
        ("a < b == c >= d", "((a < b) == (c >= d))"),
        ("a == b == c", "((a == b) == c)"),
        ("a + b <= c * d", "((a + b) <= (c * d))"),
        ("a > b", "(a > b)"),
        ("(a + b) * c", "((a + b) * c)"),
    ]);
}

#[test]
fn power_and_unary_operators() {
    assert_dumps(&[
        ("2 ** 3 ** 2", "(2 ** (3 ** 2))"),
        ("a * b ** c", "(a * (b ** c))"),
        ("-a ** b", "(-(a ** b))"),
        ("!a ** b", "((!a) ** b)"),
        ("2 ** -a", "(2 ** (-a))"),
        ("!a.b", "(!a.b)"),
        ("!!a", "(!(!a))"),
        ("-a.b", "(-a.b)"),
        ("- -a", "(-(-a))"),
        ("x * -y", "(x * (-y))"),
        ("!a && b", "((!a) && b)"),
        ("-x", "(-x)"),
    ]);
}

#[test]
fn ternary() {
    assert_dumps(&[
        ("ok ? 1 : 2", "(ok ? 1 : 2)"),
        ("a ? b : c ? d : e", "(a ? b : (c ? d : e))"),
        ("a ? b ? c : d : e", "(a ? (b ? c : d) : e)"),
        ("a || b ? c + 1 : d", "((a || b) ? (c + 1) : d)"),
        ("x.alive? ? 1 : 2", "(x.alive? ? 1 : 2)"),
    ]);
}

#[test]
fn a_minus_before_a_number_is_part_of_the_literal() {
    assert_dumps(&[
        ("-1", "-1"),
        ("- 1", "(-1)"),
        ("-1.5", "-1.5"),
        ("-0x10", "-16"),
        ("-1.abs", "-1.abs"),
        ("-1.upto(1)", "-1.upto(1)"),
        ("[-1, -2]", "[-1, -2]"),
        ("f(-1)", "f(-1)"),
        ("2 ** -1", "(2 ** -1)"),
        ("a - -1", "(a - -1)"),
        ("-9223372036854775808", "-9223372036854775808"),
        ("9223372036854775807", "9223372036854775807"),
    ]);
    assert_eq!(first_expr("-1").kind, ExprKind::Int(-1));
    assert_eq!(
        first_expr("-9223372036854775808").kind,
        ExprKind::Int(i64::MIN)
    );
    assert_eq!(first_expr("-2.5").kind, ExprKind::Float(-2.5));
}

#[test]
fn power_binds_tighter_than_a_negative_literal() {
    assert_dumps(&[
        ("-2 ** 2", "(-(2 ** 2))"),
        ("-2.5 ** 2", "(-(2.5 ** 2))"),
        ("-2 ** 3 ** 2", "(-(2 ** (3 ** 2)))"),
        ("(-2) ** 2", "(-2 ** 2)"),
        ("-2.abs ** 2", "(-2.abs ** 2)"),
    ]);
}

#[test]
fn a_minus_after_a_value_is_subtraction() {
    assert_dumps(&[
        ("a - 1", "(a - 1)"),
        ("a-1", "(a - 1)"),
        ("a -1", "(a - 1)"),
        ("1 -1", "(1 - 1)"),
    ]);
}

#[test]
fn postfix_forms() {
    assert_dumps(&[
        ("a.b.c(1)[2]::D", "a.b.c(1)[2]::D"),
        ("Team::Red", "Team::Red"),
        ("Math::PI * 2", "(Math::PI * 2)"),
        ("e[Health].hp", "e[Health].hp"),
        ("xs[i + 1]", "xs[(i + 1)]"),
        ("xs.each", "xs.each"),
        ("e.has?(Health)", "e.has?(Health)"),
        ("Entity.none", "Entity.none"),
        ("Math.sqrt(x * x + y * y)", "Math.sqrt(((x * x) + (y * y)))"),
        (
            "World.count(Position, with: Player, without: [A, B])",
            "World.count(Position, with: Player, without: [A, B])",
        ),
    ]);
}

#[test]
fn names_and_calls() {
    assert_eq!(first_expr("greet").kind, ExprKind::Var("greet".to_string()));
    assert_eq!(
        first_expr("Health").kind,
        ExprKind::Const("Health".to_string())
    );
    assert!(matches!(first_expr("greet()").kind, ExprKind::Call { args, .. } if args.is_empty()));
    assert!(
        matches!(first_expr("Vec2()").kind, ExprKind::Construct { args, .. } if args.is_empty())
    );
    assert!(matches!(
        first_expr("x.length").kind,
        ExprKind::Member {
            args: None,
            block: None,
            ..
        }
    ));
    assert!(matches!(
        first_expr("x.pop()").kind,
        ExprKind::Member { args: Some(args), .. } if args.is_empty()
    ));
    assert_dumps(&[
        ("clamp(x, 0, 1)", "clamp(x, 0, 1)"),
        ("foo(1, x: 2)", "foo(1, x: 2)"),
        ("Vec2(y: 4, x: 3)", "Vec2(y: 4, x: 3)"),
        ("Health(hp: 50)", "Health(hp: 50)"),
        ("f(a ? b : c)", "f((a ? b : c))"),
    ]);
    let ExprKind::Call { args, .. } = first_expr("f(a, k: 1)").kind else {
        panic!("not a call");
    };
    assert_eq!(args[0].name, None);
    assert_eq!(
        args[1].name.as_ref().map(|name| name.name.as_str()),
        Some("k")
    );
}

#[test]
fn literals() {
    assert_dumps(&[
        (
            "[1, 2.5, 1e3, true, false, self, \"s\", 'r', []]",
            "[1, 2.5, 1000.0, true, false, self, \"s\", \"r\", []]",
        ),
        ("0xff + 0b1010 + 1_000", "((255 + 10) + 1000)"),
        ("[[1], [2, 3]]", "[[1], [2, 3]]"),
    ]);
}

#[test]
fn strings() {
    assert_eq!(first_expr("\"\"").kind, ExprKind::Str(vec![]));
    assert_eq!(first_expr("''").kind, ExprKind::Str(vec![]));
    assert_eq!(
        first_expr(r#""a\tb""#).kind,
        ExprKind::Str(vec![StrPart::Lit("a\tb".to_string())])
    );
    assert_eq!(
        first_expr(r##""\#{x}""##).kind,
        ExprKind::Str(vec![StrPart::Lit("#{x}".to_string())])
    );
    assert_eq!(
        first_expr(r##"'#{x}\n'"##).kind,
        ExprKind::Str(vec![StrPart::Lit("#{x}\\n".to_string())])
    );
}

#[test]
fn interpolation() {
    assert_dumps(&[
        (r##""a#{b + 1}c""##, r##""a#{(b + 1)}c""##),
        (r##""#{a}#{b}""##, r##""#{a}#{b}""##),
        (
            r##""#{e} is at (#{pos.x}, #{pos.y})""##,
            r##""#{e} is at (#{pos.x}, #{pos.y})""##,
        ),
        (r##""a#{ "b#{c}" }d""##, r##""a#{"b#{c}"}d""##),
        (r##""#{h["k"]}""##, r##""#{h["k"]}""##),
        (
            r##""#{if a then 1 else 2 end}""##,
            r##""#{if a { 1 } else { 2 }}""##,
        ),
        (r##""#{xs.length - 1}!""##, r##""#{(xs.length - 1)}!""##),
    ]);
    let ExprKind::Str(parts) = first_expr(r##""a#{b}""##).kind else {
        panic!("not a string");
    };
    assert_eq!(parts.len(), 2);
    assert_eq!(parts[0], StrPart::Lit("a".to_string()));
}

#[test]
fn interpolated_expressions_keep_their_place_in_the_file() {
    let text = r##"puts "é#{pos.x} and #{ "in #{n + 1}" }""##;
    let StmtKind::Puts(values) = first_stmt(text).kind else {
        panic!("not puts");
    };
    let ExprKind::Str(parts) = &values[0].kind else {
        panic!("not a string");
    };
    let StrPart::Expr(first) = &parts[1] else {
        panic!("not an interpolation");
    };
    assert_eq!(marked(text, first.span), "pos.x");
    let StrPart::Expr(second) = &parts[3] else {
        panic!("not an interpolation");
    };
    assert_eq!(marked(text, second.span), r##""in #{n + 1}""##);
    let ExprKind::Str(inner) = &second.kind else {
        panic!("not a string");
    };
    let StrPart::Expr(sum) = &inner[1] else {
        panic!("not an interpolation");
    };
    assert_eq!(marked(text, sum.span), "n + 1");
}

// Statements

#[test]
fn assignments() {
    assert_dumps(&[
        ("x = 1", "x = 1"),
        ("x += 1", "x += 1"),
        ("x -= 1", "x -= 1"),
        ("x *= 2", "x *= 2"),
        ("x /= 2", "x /= 2"),
        ("x %= 2", "x %= 2"),
        ("pos.x += vel.dx * Time.dt", "pos.x += (vel.dx * Time.dt)"),
        ("e[Health] = Health(5)", "e[Health] = Health(5)"),
        ("target[Health].hp -= 10", "target[Health].hp -= 10"),
        ("xs[0] = 1", "xs[0] = 1"),
        ("path.points[0].x = 2", "path.points[0].x = 2"),
        ("Score.points += 10", "Score.points += 10"),
        ("Time.dt = 0.5", "Time.dt = 0.5"),
        ("x = a == b", "x = (a == b)"),
        ("X += 1", "X += 1"),
    ]);
}

#[test]
fn typed_declarations() {
    assert_dumps(&[
        ("ratio: Float = 1", "ratio: Float = 1"),
        ("scores: [Int] = []", "scores: [Int] = []"),
        ("grid: [[Float32]] = [[0.5]]", "grid: [[Float32]] = [[0.5]]"),
    ]);
}

#[test]
fn return_break_and_next() {
    assert_dumps(&[
        ("return", "return"),
        ("return x + 1", "return (x + 1)"),
        ("return -1", "return -1"),
        ("return [1]", "return [1]"),
        ("break", "break"),
        ("next", "next"),
    ]);
}

#[test]
fn output_statements() {
    assert_dumps(&[
        ("puts", "puts"),
        ("puts a, b", "puts a, b"),
        ("puts(a, b)", "puts a, b"),
        ("puts()", "puts"),
        ("puts (a), b", "puts a, b"),
        ("puts (a + b) * 2", "puts ((a + b) * 2)"),
        ("puts -1", "puts -1"),
        ("puts [1]", "puts [1]"),
        ("puts a.length", "puts a.length"),
        ("print a", "print a"),
        ("print(a, \"\\n\")", "print a, \"\n\""),
        (
            "emit Hit(attack.target, attack.power)",
            "emit Hit(attack.target, attack.power)",
        ),
        ("emit(Hit(1))", "emit Hit(1)"),
    ]);
}

#[test]
fn modifiers_become_ifs() {
    assert_dumps(&[
        ("puts \"low\" if hp < 20", "if (hp < 20) { puts \"low\" }"),
        (
            "return unless target.alive?",
            "unless target.alive? { return }",
        ),
        ("return x if y", "if y { return x }"),
        ("x = 1 if y", "if y { x = 1 }"),
        ("n: Int = 1 unless y", "unless y { n: Int = 1 }"),
        ("break if finished?", "if finished? { break }"),
        ("puts if quiet", "if quiet { puts }"),
        (
            "e.despawn if health.hp <= 0",
            "if (health.hp <= 0) { e.despawn }",
        ),
        (
            "next unless hit.target.alive? && hit.target.has?(Health)",
            "unless (hit.target.alive? && hit.target.has?(Health)) { next }",
        ),
    ]);
}

#[test]
fn if_and_unless() {
    assert_dumps(&[
        (
            "if hp <= 0\n  puts \"dead\"\nelsif hp < 20\n  puts \"hurt\"\nelse\n  puts \"fine\"\nend",
            "if (hp <= 0) { puts \"dead\" } else { if (hp < 20) { puts \"hurt\" } else { puts \"fine\" } }",
        ),
        (
            "if a\n  1\nelsif b\n  2\nelsif c\n  3\nend",
            "if a { 1 } else { if b { 2 } else { if c { 3 } } }",
        ),
        (
            "unless ready\n  puts \"waiting\"\nend",
            "unless ready { puts \"waiting\" }",
        ),
        ("unless a\n  1\nelse\n  2\nend", "unless a { 1 } else { 2 }"),
        ("if a then b else c end", "if a { b } else { c }"),
        ("if a then b end", "if a { b }"),
        ("if a then\n  b\nend", "if a { b }"),
        ("if a\nend", "if a {}"),
        ("if a\n  x = 1\n  y = 2\nend", "if a { x = 1; y = 2 }"),
        (
            "label = if hp > 50 then \"healthy\" else \"hurt\" end",
            "label = if (hp > 50) { \"healthy\" } else { \"hurt\" }",
        ),
        (
            "x = if a then 1 else 2 end + 3",
            "x = (if a { 1 } else { 2 } + 3)",
        ),
        (
            "if a then 1 else 2 end if b",
            "if b { if a { 1 } else { 2 } }",
        ),
    ]);
}

#[test]
fn case() {
    assert_dumps(&[
        (
            "case team\nwhen Team::Red then puts \"red\"\nwhen Team::Blue, Team::Green\n  puts \"cool colors\"\nend",
            "case team when Team::Red { puts \"red\" } when Team::Blue, Team::Green { puts \"cool colors\" }",
        ),
        (
            "x = case n\n  when 1 then \"one\"\n  when 2,\n       3 then \"few\"\n  else \"many\"\nend",
            "x = case n when 1 { \"one\" } when 2, 3 { \"few\" } else { \"many\" }",
        ),
        (
            "case n\n\nwhen 1\n  a\n  b\nelse\n  c\nend",
            "case n when 1 { a; b } else { c }",
        ),
        ("case n\nend", "case n"),
    ]);
}

#[test]
fn while_until_and_loop() {
    assert_dumps(&[
        ("while n > 0\n  n -= 1\nend", "while (n > 0) { n -= 1 }"),
        (
            "until done\n  done = step\nend",
            "until done { done = step }",
        ),
        (
            "loop do\n  break if finished?\nend",
            "loop { if finished? { break } }",
        ),
        ("loop do break end", "loop { break }"),
        (
            "while a\n  while b\n    next\n  end\nend",
            "while a { while b { next } }",
        ),
    ]);
}

#[test]
fn each_and_on() {
    assert_dumps(&[
        (
            "each Position, Velocity do |pos, vel|\n  pos.x += vel.dx\nend",
            "each Position, Velocity |pos, vel| { pos.x += vel.dx }",
        ),
        (
            "each Entity, Health, with: Player, without: [Dead, Frozen] do |e, health|\n  puts e\nend",
            "each Entity, Health with [Player] without [Dead, Frozen] |e, health| { puts e }",
        ),
        (
            "each A, with: B, with: [C],\n  without: D do |a|\nend",
            "each A with [B, C] without [D] |a| {}",
        ),
        (
            "each Entity do |e| e.despawn end",
            "each Entity |e| { e.despawn }",
        ),
        ("each A do\nend", "each A || {}"),
        (
            "on Hit do |hit|\n  hit.target[Health].hp -= hit.damage\nend",
            "on Hit |hit| { hit.target[Health].hp -= hit.damage }",
        ),
        (
            "each A do |a|\n  each B do |b|\n    next\n  end\nend",
            "each A |a| { each B |b| { next } }",
        ),
    ]);
}

#[test]
fn method_calls_with_blocks() {
    assert_dumps(&[
        ("3.times do |i|\n  puts i\nend", "3.times do |i| { puts i }"),
        (
            "3.times do\n  World.tick\nend",
            "3.times do || { World.tick }",
        ),
        ("1.upto(3) do |i|  end", "1.upto(3) do |i| {}"),
        (
            "3.downto(1) do |i| puts i end",
            "3.downto(1) do |i| { puts i }",
        ),
        (
            "names.each do |name| puts name end",
            "names.each do |name| { puts name }",
        ),
        (
            "names.each_with_index do |name, i|\n  puts i, name\nend",
            "names.each_with_index do |name, i| { puts i, name }",
        ),
        (
            "xs.length.times do |i| xs[i] *= 2 end",
            "xs.length.times do |i| { xs[i] *= 2 }",
        ),
        ("-1.upto(1) do |i|\nend", "-1.upto(1) do |i| {}"),
    ]);
}

#[test]
fn spawn() {
    assert_dumps(&[
        ("spawn", "spawn()"),
        (
            "spawn Position(0, 0), Velocity(1, 2)",
            "spawn(Position(0, 0), Velocity(1, 2))",
        ),
        (
            "spawn Position(5, 5), Velocity(-1, 0), Frozen",
            "spawn(Position(5, 5), Velocity(-1, 0), Frozen)",
        ),
        (
            "player = spawn Position(0, 0), Health(hp: 50), Player",
            "player = spawn(Position(0, 0), Health(hp: 50), Player)",
        ),
        ("e: Entity = spawn A", "e: Entity = spawn(A)"),
        ("spawn(A, B)", "spawn(A, B)"),
        ("spawn()", "spawn()"),
        ("spawn (A), B", "spawn(A, B)"),
        ("spawn(A).add(B)", "spawn(A).add(B)"),
        ("e = spawn(A).tap", "e = spawn(A).tap"),
        ("e = spawn", "e = spawn()"),
        ("foo(spawn, spawn(A))", "foo(spawn(), spawn(A))"),
        ("xs = [spawn, spawn]", "xs = [spawn(), spawn()]"),
        ("spawn.despawn", "spawn().despawn"),
        ("spawn A if x", "if x { spawn(A) }"),
        ("spawn if x", "if x { spawn() }"),
        ("e = spawn A, B unless x", "unless x { e = spawn(A, B) }"),
        ("puts spawn", "puts spawn()"),
    ]);
}

#[test]
fn newlines_that_continue_a_statement() {
    assert_eq!(
        dump(
            "x = 1 +\n  2\ny = foo(1,\n        2)\nz = xs\n  .first\n  .abs\nw =\n  3\nputs a,\n  b\n"
        ),
        "x = (1 + 2); y = foo(1, 2); z = xs.first.abs; w = 3; puts a, b"
    );
    assert_eq!(
        dump("xs = [\n  1,\n  2\n]\nok = a &&\n  b\n"),
        "xs = [1, 2]; ok = (a && b)"
    );
}

#[test]
fn comments_and_blank_lines() {
    assert_eq!(
        dump("# top\n\nx = 1 # trailing\n\n\n  # indented\ny = 2\n# end\n"),
        "x = 1; y = 2"
    );
    assert_eq!(dump(""), "");
    assert_eq!(dump("\n# nothing\n"), "");
    assert_eq!(dump("x = 1"), "x = 1");
}

// Declarations

#[test]
fn structs() {
    let text = "\
struct Vec2
  x: Float = 0
  y: Float = 0

  def +(other: Vec2) -> Vec2
    Vec2(x + other.x, y + other.y)
  end

  def -@
    Vec2(-x, -y)
  end

  def *(k: Float) -> Vec2
    Vec2(x * k, y * k)
  end

  def length -> Float
    Math.sqrt(x * x + y * y)
  end
end
";
    assert_eq!(
        dump(text),
        "struct Vec2 { x: Float = 0; y: Float = 0; \
         def +(other: Vec2) -> Vec2 { Vec2((x + other.x), (y + other.y)) }; \
         def -@() { Vec2((-x), (-y)) }; \
         def *(k: Float) -> Vec2 { Vec2((x * k), (y * k)) }; \
         def length() -> Float { Math.sqrt(((x * x) + (y * y))) } }"
    );
    assert_dumps(&[
        (
            "struct P\n  def -(o: P) -> P\n    o\n  end\n  def /(k: Int)\n    self\n  end\n  def %(k: Int)\n    self\n  end\nend",
            "struct P { def -(o: P) -> P { o }; def /(k: Int) { self }; def %(k: Int) { self } }",
        ),
        (
            "struct Path\n  points: [Vec2]\n  name: String = \"p\" + \"q\"\nend",
            "struct Path { points: [Vec2]; name: String = (\"p\" + \"q\") }",
        ),
    ]);
}

#[test]
fn components_resources_events_enums_and_tags() {
    assert_dumps(&[
        (
            "component Health\n  hp: Int\n  max: Int = 100\n  def dead? -> Bool\n    hp <= 0\n  end\nend",
            "component Health { hp: Int; max: Int = 100; def dead?() -> Bool { (hp <= 0) } }",
        ),
        (
            "resource Score\n  points: Int = 0\n  best: Int = 0\nend",
            "resource Score { points: Int = 0; best: Int = 0 }",
        ),
        (
            "event Hit\n  target: Entity\n  damage: Int\nend",
            "event Hit { target: Entity; damage: Int }",
        ),
        ("event Tick\nend", "event Tick {}"),
        (
            "enum Team\n  Red\n  Blue\n  Green\nend",
            "enum Team { Red, Blue, Green }",
        ),
        (
            "enum Team\n  Red, Blue\n  Green\nend",
            "enum Team { Red, Blue, Green }",
        ),
        ("enum Team\n  Red,\n  Blue\nend", "enum Team { Red, Blue }"),
        ("tag Player", "tag Player"),
        ("tag A\ntag B\n", "tag A; tag B"),
    ]);
    let Item::Record(record) = &items("component C\n  x: Int\nend")[0] else {
        panic!("not a record");
    };
    assert_eq!(record.kind, RecordKind::Component);
}

#[test]
fn functions_and_systems() {
    assert_dumps(&[
        (
            "def clamp(v: Float, lo: Float, hi: Float) -> Float\n  return lo if v < lo\n  return hi if v > hi\n  v\nend",
            "def clamp(v: Float, lo: Float, hi: Float) -> Float { if (v < lo) { return lo }; if (v > hi) { return hi }; v }",
        ),
        (
            "def greet\n  puts \"hello\"\nend",
            "def greet() { puts \"hello\" }",
        ),
        ("def f()\nend", "def f() {}"),
        (
            "def total(xs: [Int]) -> [Int]\n  xs\nend",
            "def total(xs: [Int]) -> [Int] { xs }",
        ),
        ("def ready?\n  true\nend", "def ready?() { true }"),
        ("def save!(n: Int)\nend", "def save!(n: Int) {}"),
        (
            "system move\n  each Position do |pos|\n    pos.x += 1\n  end\nend",
            "system move { each Position |pos| { pos.x += 1 } }",
        ),
        ("system idle\nend", "system idle {}"),
    ]);
}

#[test]
fn requires_and_constants() {
    assert_dumps(&[
        ("require \"physics\"", "require \"physics\""),
        (
            "require 'lib/math'\nrequire \"b\"",
            "require \"lib/math\"; require \"b\"",
        ),
        ("GRAVITY = 9.81", "const GRAVITY = 9.81"),
        ("ORIGIN = Vec2(0, 0)", "const ORIGIN = Vec2(0, 0)"),
        ("Limit = 2 * GRAVITY", "const Limit = (2 * GRAVITY)"),
        ("X == 1", "(X == 1)"),
    ]);
    // Inside a block it is an assignment for the checker to reject.
    assert_eq!(dump("def f\n  X = 1\nend"), "def f() { X = 1 }");
}

// Spans

#[test]
fn expression_spans() {
    let text = "x = (a + b) * c";
    let StmtKind::Assign { target, value, .. } = first_stmt(text).kind else {
        panic!("not an assignment");
    };
    assert_eq!(marked(text, target.span), "x");
    assert_eq!(marked(text, value.span), "(a + b) * c");
    let ExprKind::Binary { lhs, rhs, .. } = value.kind else {
        panic!("not binary");
    };
    assert_eq!(marked(text, lhs.span), "(a + b)");
    assert_eq!(marked(text, rhs.span), "c");

    let text = "foo.bar(1, 2).baz[3]::Q";
    let path = first_expr(text);
    assert_eq!(marked(text, path.span), text);
    let ExprKind::Path { recv: index, name } = path.kind else {
        panic!("not a path");
    };
    assert_eq!(marked(text, name.span), "Q");
    assert_eq!(marked(text, index.span), "foo.bar(1, 2).baz[3]");
    let ExprKind::Index { recv: baz, .. } = index.kind else {
        panic!("not an index");
    };
    assert_eq!(marked(text, baz.span), "foo.bar(1, 2).baz");
    let ExprKind::Member {
        recv: bar, name, ..
    } = baz.kind
    else {
        panic!("not a member");
    };
    assert_eq!(marked(text, name.span), "baz");
    assert_eq!(marked(text, bar.span), "foo.bar(1, 2)");

    let text = "y = -1.abs + -x";
    let StmtKind::Assign { value, .. } = first_stmt(text).kind else {
        panic!("not an assignment");
    };
    let ExprKind::Binary { lhs, rhs, .. } = value.kind else {
        panic!("not binary");
    };
    assert_eq!(marked(text, rhs.span), "-x");
    let ExprKind::Member { recv, .. } = lhs.kind else {
        panic!("not a member");
    };
    assert_eq!(marked(text, recv.span), "-1");

    let text = "a ? [1, 2] : spawn(A)";
    let ExprKind::Ternary { then, else_, .. } = first_expr(text).kind else {
        panic!("not a ternary");
    };
    assert_eq!(marked(text, then.span), "[1, 2]");
    assert_eq!(marked(text, else_.span), "spawn(A)");

    let text = "-2 ** 2";
    let negation = first_expr(text);
    assert_eq!(marked(text, negation.span), text);
    let ExprKind::Unary { expr: power, .. } = negation.kind else {
        panic!("not unary");
    };
    assert_eq!(marked(text, power.span), "2 ** 2");
}

#[test]
fn statement_and_block_spans() {
    let text = "puts \"low\" if hp < 20  # note";
    let stmt = first_stmt(text);
    assert_eq!(marked(text, stmt.span), "puts \"low\" if hp < 20");
    let StmtKind::Expr(Expr {
        kind: ExprKind::If { cond, then, .. },
        span,
    }) = stmt.kind
    else {
        panic!("not an if");
    };
    assert_eq!(span, stmt.span);
    assert_eq!(marked(text, cond.span), "hp < 20");
    assert_eq!(marked(text, then.span), "puts \"low\"");
    assert_eq!(marked(text, then.stmts[0].span), "puts \"low\"");

    let text = "if a\n  x = 1\n  y = 2\nelsif b\n  z\nend\n";
    let outer = first_expr(text);
    assert_eq!(marked(text, outer.span), text.trim_end());
    let ExprKind::If { then, else_, .. } = outer.kind else {
        panic!("not an if");
    };
    assert_eq!(marked(text, then.span), "x = 1\n  y = 2");
    let else_ = else_.expect("an else block");
    assert_eq!(marked(text, else_.span), "elsif b\n  z\nend");
    assert_eq!(else_.stmts[0].span, else_.span);

    // An empty block sits where it would begin.
    let text = "if ready\nend";
    let ExprKind::If { then, .. } = first_expr(text).kind else {
        panic!("not an if");
    };
    assert!(then.stmts.is_empty());
    assert_eq!((then.span.start, then.span.end), (8, 8));

    let text = "while n > 0\n  n -= 1\nend\nputs n";
    let stmt = first_stmt(text);
    assert_eq!(marked(text, stmt.span), "while n > 0\n  n -= 1\nend");
    let StmtKind::While { body, .. } = stmt.kind else {
        panic!("not a while");
    };
    assert_eq!(marked(text, body.span), "n -= 1");
}

#[test]
fn block_and_query_spans() {
    let text = "3.times do |i|\n  puts i\nend\n";
    let call = first_expr(text);
    assert_eq!(marked(text, call.span), text.trim_end());
    let ExprKind::Member {
        block: Some(block), ..
    } = call.kind
    else {
        panic!("not a call with a block");
    };
    assert_eq!(marked(text, block.span), "do |i|\n  puts i\nend");
    assert_eq!(marked(text, block.params[0].span), "i");
    assert_eq!(marked(text, block.body.span), "puts i");

    let text = "each A, B, with: C, without: [D, E] do |a, b|\nend";
    let stmt = first_stmt(text);
    assert_eq!(marked(text, stmt.span), text);
    let StmtKind::Each { query, params, .. } = stmt.kind else {
        panic!("not an each");
    };
    assert_eq!(marked(text, query.span), "A, B, with: C, without: [D, E]");
    assert_eq!(marked(text, query.terms[1].span), "B");
    assert_eq!(marked(text, query.without[1].span), "E");
    assert_eq!(marked(text, params[1].span), "b");
}

#[test]
fn declaration_spans() {
    let text = "struct Vec2\n  x: Float = 0\n  def +(other: Vec2) -> Vec2\n    other\n  end\n  def length\n    1\n  end\nend\n";
    let Item::Record(record) = &items(text)[0] else {
        panic!("not a record");
    };
    assert_eq!(marked(text, record.span), "struct Vec2");
    assert_eq!(marked(text, record.name.span), "Vec2");
    assert_eq!(marked(text, record.fields[0].name.span), "x");
    assert_eq!(marked(text, record.fields[0].ty.span), "Float");
    assert_eq!(
        marked(text, record.methods[0].span),
        "def +(other: Vec2) -> Vec2"
    );
    assert_eq!(marked(text, record.methods[0].name.span), "+");
    assert_eq!(marked(text, record.methods[0].body.span), "other");
    assert_eq!(marked(text, record.methods[1].span), "def length");

    let text = "def clamp(v: Float, lo: [Float])\nend\nenum Team\n  Red\nend\ntag Player\nsystem move\nend\nrequire \"physics\"\nxs: [Int] = []\n";
    let items = items(text);
    let Item::Def(def) = &items[0] else {
        panic!("not a def");
    };
    assert_eq!(marked(text, def.span), "def clamp(v: Float, lo: [Float])");
    assert_eq!(marked(text, def.params[1].ty.span), "[Float]");
    let Item::Enum(decl) = &items[1] else {
        panic!("not an enum");
    };
    assert_eq!(marked(text, decl.span), "enum Team");
    assert_eq!(marked(text, decl.variants[0].span), "Red");
    let Item::Tag(tag) = &items[2] else {
        panic!("not a tag");
    };
    assert_eq!(marked(text, tag.span), "tag Player");
    let Item::System(system) = &items[3] else {
        panic!("not a system");
    };
    assert_eq!(marked(text, system.span), "system move");
    let Item::Require(require) = &items[4] else {
        panic!("not a require");
    };
    assert_eq!(marked(text, require.span), "require \"physics\"");
    let Item::Stmt(Stmt {
        kind: StmtKind::VarDecl { ty, .. },
        ..
    }) = &items[5]
    else {
        panic!("not a declaration");
    };
    assert_eq!(marked(text, ty.span), "[Int]");
}

// The reference

/// The `ruby` code blocks of the language reference.
fn reference_programs() -> Vec<String> {
    include_str!("../../docs/language.md")
        .split("```ruby\n")
        .skip(1)
        .map(|rest| rest.split("```").next().unwrap_or_default().to_string())
        .collect()
}

#[test]
fn the_first_program_in_the_reference() {
    let programs = reference_programs();
    assert_eq!(
        dump_items(&programs[0]),
        [
            "component Position { x: Float; y: Float }",
            "component Velocity { dx: Float; dy: Float }",
            "tag Frozen",
            "system move { each Position, Velocity without [Frozen] |pos, vel| \
             { pos.x += (vel.dx * Time.dt); pos.y += (vel.dy * Time.dt) } }",
            "system report { each Entity, Position |e, pos| \
             { puts \"#{e} is at (#{pos.x}, #{pos.y})\" } }",
            "spawn(Position(0, 0), Velocity(1, 2))",
            "spawn(Position(5, 5), Velocity(-1, 0), Frozen)",
            "Time.dt = 0.5",
            "3.times do || { World.tick }",
        ]
    );
}

#[test]
fn every_program_in_the_reference_parses() {
    let programs = reference_programs();
    assert!(programs.len() >= 20, "found {}", programs.len());
    for program in programs {
        // The reference elides loop bodies with `...`.
        let program = program.replace("...", "");
        if let Err(error) = parse(FILE, &program) {
            panic!(
                "{} at {}..{} ({:?}) in:\n{program}",
                error.message, error.span.start, error.span.end, error.label
            );
        }
    }
}

#[test]
fn the_hello_example() {
    assert_eq!(
        dump(include_str!("../../examples/hello.grem")),
        "puts \"Hello, world!\""
    );
}

// Errors

#[test]
fn unexpected_tokens() {
    assert_error("x = )", "unexpected `)`", ")", "expected an expression");
    assert_error("puts 1 2", "unexpected `2`", "2", "expected a newline");
    assert_error("foo() bar", "unexpected `bar`", "bar", "expected a newline");
    assert_error("x = {", "unexpected `{`", "{", "expected an expression");
    assert_error("foo(a b)", "unexpected `b`", "b", "expected `,` or `)`");
    assert_error("[1 2]", "unexpected `2`", "2", "expected `,` or `]`");
    assert_error("[a b]", "unexpected `b`", "b", "expected `,` or `]`");
    assert_error(
        "x = 1 \"s\"",
        "unexpected string",
        "\"s\"",
        "expected a newline",
    );
    assert_error("puts(1) + 2", "unexpected `+`", "+", "expected a newline");
    assert_error(
        "a.b::c",
        "unexpected `c`",
        "c",
        "expected a constant or variant name, which begins with an uppercase letter",
    );
    assert_error(
        "x = gpu",
        "unexpected `gpu`",
        "gpu",
        "`gpu` is reserved for GPU systems",
    );
    assert_error(
        "x = puts 1",
        "unexpected `puts`",
        "puts",
        "expected an expression, and `puts` is a statement",
    );
    assert_error(
        "x = 1 + while",
        "unexpected `while`",
        "while",
        "expected an expression, and `while` is a statement",
    );
}

#[test]
fn unexpected_ends() {
    let at_end = error("x = 1 +\n");
    assert_eq!(at_end.message, "unexpected end of file");
    assert_eq!(at_end.label.as_deref(), Some("expected an expression"));
    assert_eq!((at_end.span.start, at_end.span.end), (7, 7));

    let at_newline = error("foo.\nbar");
    assert_eq!(at_newline.message, "unexpected end of line");
    assert_eq!(
        at_newline.label.as_deref(),
        Some("expected a field or method name")
    );
    assert_eq!((at_newline.span.start, at_newline.span.end), (4, 4));

    assert_error(
        "a ? b",
        "unexpected end of file",
        "",
        "expected `:` and the other branch",
    );
    assert_error(
        "x: Int",
        "unexpected end of file",
        "",
        "expected `=` and a value",
    );
    assert_error(
        "print\n",
        "`print` needs at least one argument",
        "print",
        "`puts` alone writes an empty line",
    );
}

#[test]
fn stray_closers() {
    assert_error(
        "x = 1\nend\n",
        "unexpected `end`",
        "end",
        "there is no open block to close",
    );
    assert_error(
        "puts 1 end",
        "unexpected `end`",
        "end",
        "there is no open block to close",
    );
    assert_error(
        "else\n",
        "unexpected `else`",
        "else",
        "there is no `if` this belongs to",
    );
    assert_error(
        "when 1\n",
        "unexpected `when`",
        "when",
        "there is no `case` this belongs to",
    );
}

#[test]
fn a_block_that_is_never_closed_points_at_its_opener() {
    for (text, word) in [
        ("if x\n  puts 1\n", "if"),
        ("unless x\n  puts 1\n", "unless"),
        ("while x\n  puts 1\n", "while"),
        ("until x\n", "until"),
        ("case x\nwhen 1\n  puts 1\n", "case"),
        ("loop do\n  break\n", "do"),
        ("each A do |a|\n  puts a\n", "do"),
        ("on Hit do |hit|\n", "do"),
        ("3.times do\n  puts 1\n", "do"),
        ("def f\n  1\n", "def"),
        ("struct A\n  x: Int\n", "struct"),
        ("component A\n  x: Int\n", "component"),
        ("resource A\n  x: Int = 0\n", "resource"),
        ("event A\n", "event"),
        ("enum E\n  A\n", "enum"),
        ("system s\n  puts 1\n", "system"),
        ("x = if a then 1 else 2", "if"),
        ("if a\n  1\nelsif b\n  2\n", "if"),
        ("puts \"#{if a then 1}\"", "if"),
        ("def f\n  if x\n    1\n  end\n", "def"),
        ("def f\n  while x\n    1\nend\n", "while"),
    ] {
        let error = error(text);
        assert_eq!(
            error.message,
            format!("this `{word}` is never closed"),
            "{text:?}"
        );
        assert_eq!(marked(text, error.span), word, "{text:?}");
        assert_eq!(
            error.span.start as usize,
            text.find(word).unwrap(),
            "{text:?}"
        );
        assert_eq!(
            error.label.as_deref(),
            Some("expected a matching `end`"),
            "{text:?}"
        );
    }
}

#[test]
fn a_misaligned_end_finds_the_block_that_is_never_closed() {
    // The `if` has no `end`; the two that follow close it and the `do`.
    let text = "system s\n  each A do |a|\n    if a.x\n      foo\n  end\nend\n";
    assert_error(
        text,
        "this `if` is never closed",
        "if",
        "expected a matching `end`",
    );

    // The same, noticed at the next declaration instead of the end of file.
    let text = "system s\n  each A do |a|\n    if a.x\n      foo\n  end\nend\n\nsystem t\nend\n";
    assert_error(
        text,
        "this `if` is never closed",
        "if",
        "expected a matching `end`",
    );

    let text = "system s\n  each A do |a|\n    if a.x\n      foo\n    end\nend\n";
    assert_error(
        text,
        "this `do` is never closed",
        "do",
        "expected a matching `end`",
    );

    // A block that closes in line clears what was noticed inside it.
    let text = "def f\n  if x\n    1\n    end\nend\ndef g\n  tag T\nend\n";
    assert_error(
        text,
        "`tag` is only allowed at the top level",
        "tag",
        "this is inside a block",
    );

    // An `end` in the column of its `if` is in line.
    let text = "def f\n  x = if a\n        1\n      end\n";
    assert_error(
        text,
        "this `def` is never closed",
        "def",
        "expected a matching `end`",
    );
}

#[test]
fn brackets_that_are_never_closed() {
    assert_error(
        "foo(1, 2",
        "this `(` is never closed",
        "(",
        "expected a matching `)`",
    );
    assert_error(
        "x = (1 + 2\n",
        "this `(` is never closed",
        "(",
        "expected a matching `)`",
    );
    assert_error(
        "xs = [1, 2\n",
        "this `[` is never closed",
        "[",
        "expected a matching `]`",
    );
    assert_error(
        "xs[1\n",
        "this `[` is never closed",
        "[",
        "expected a matching `]`",
    );
    assert_error(
        "def f(a: Int\nend",
        "unexpected `end`",
        "end",
        "expected `,` or `)`",
    );
}

#[test]
fn declarations_inside_blocks() {
    let text = "def foo\n  puts 1\n\ndef bar\nend\n";
    let error = error(text);
    assert_eq!(
        error.message,
        "`def` is only allowed at the top level and in a struct or component"
    );
    assert_eq!(error.span.start, 18);
    assert_eq!(marked(text, error.span), "def");
    assert_eq!(error.label.as_deref(), Some("this is inside a block"));
    assert_eq!(error.notes.len(), 1);
    assert_eq!(error.notes[0].message, "the enclosing `def` starts here");
    assert_eq!((error.notes[0].span.start, error.notes[0].span.end), (0, 3));

    for word in [
        "struct",
        "component",
        "resource",
        "event",
        "enum",
        "tag",
        "system",
    ] {
        let text = format!("system s\n  {word} A\n  end\nend\n");
        let error = parse(FILE, &text).expect_err(word);
        assert_eq!(
            error.message,
            format!("`{word}` is only allowed at the top level")
        );
        assert_eq!(marked(&text, error.span), word);
        assert_eq!(error.notes[0].message, "the enclosing `system` starts here");
    }
    assert_error(
        "if x\n  require \"a\"\nend",
        "`require` is only allowed at the top level",
        "require",
        "this is inside a block",
    );
    assert_error(
        "struct A\n  x: Int\n  struct B\n  end\nend",
        "`struct` is only allowed at the top level",
        "struct",
        "this is inside a block",
    );
    assert_error(
        "enum E\n  A\n  tag T\nend",
        "`tag` is only allowed at the top level",
        "tag",
        "this is inside a block",
    );
}

#[test]
fn methods_in_resources_and_events() {
    assert_error(
        "resource R\n  x: Int = 0\n  def f\n  end\nend",
        "a resource can't have methods",
        "def",
        "only structs and components have methods",
    );
    assert_error(
        "event E\n  def f\n  end\nend",
        "an event can't have methods",
        "def",
        "only structs and components have methods",
    );
}

#[test]
fn lexical_errors_are_reported() {
    assert_error(
        "puts \"abc\nx = 1\n",
        "unterminated string",
        "\"",
        "the closing `\"` is missing",
    );
    assert_error(
        "puts 'abc",
        "unterminated string",
        "'",
        "the closing `'` is missing",
    );
    assert_error(
        "puts \"a\\qb\"",
        "unknown escape `\\q`",
        "\\q",
        r#"the escapes are \n, \t, \r, \0, \e, \\, \" and \#"#,
    );
    assert_error(
        "x = 12abc",
        "invalid number literal",
        "12abc",
        "this is not a valid `Int` or `Float`",
    );
    assert_error(
        "x = 007",
        "invalid number literal",
        "007",
        "a number can't begin with extra zeros",
    );
    assert_error(
        "puts \"n = #{n\"",
        "this `#{` is never closed",
        "#{",
        "expected a matching `}`",
    );
    assert_error(
        "a & b",
        "unexpected character `&`",
        "&",
        "did you mean `&&`?",
    );
}

#[test]
fn integers_out_of_range() {
    let label = "an `Int` holds -9223372036854775808 to 9223372036854775807";
    assert_error(
        "x = 9223372036854775808",
        "integer literal is too large",
        "9223372036854775808",
        label,
    );
    assert_error(
        "x = -9223372036854775808 ** 2",
        "integer literal is too large",
        "9223372036854775808",
        label,
    );
    assert_error(
        "x = 99999999999999999999",
        "integer literal is too large",
        "99999999999999999999",
        label,
    );
}

#[test]
fn interpolation_errors() {
    assert_error(
        "puts \"#{}\"",
        "unexpected `}`",
        "}",
        "expected an expression",
    );
    assert_error("puts \"#{1 b}\"", "unexpected `b`", "b", "expected `}`");
    assert_error(
        "puts \"#{a +}\"",
        "unexpected `}`",
        "}",
        "expected an expression",
    );
    assert_error("puts \"#{(a}\"", "unexpected `}`", "}", "expected `)`");
}

#[test]
fn assignment_targets() {
    let message = "can't assign to this expression";
    assert_error("1 = 2", message, "1", "a literal is a value, not a place");
    assert_error(
        "\"s\" += 2",
        message,
        "\"s\"",
        "a literal is a value, not a place",
    );
    assert_error(
        "foo(1) = 2",
        message,
        "foo(1)",
        "the result of a call is a value, not a place",
    );
    assert_error(
        "x.f(1) = 2",
        message,
        "x.f(1)",
        "the result of a call is a value, not a place",
    );
    let other = "only a variable, a field, a list element or a component can be assigned to";
    assert_error("a + b = 3", message, "a + b", other);
    assert_error("Team::Red = 1", message, "Team::Red", other);
    assert_error("-x = 1", message, "-x", other);
}

#[test]
fn assignment_inside_an_expression() {
    let message = "assignment is a statement, not an expression";
    let apart = "assign on a line of its own, then use the variable";
    let compare = "to compare, write `==`; to assign, use a line of its own";
    for (text, start, label) in [
        ("y = (x = 2)", 7, apart),
        ("puts(y = 2)", 7, apart),
        ("f(a, b = 2)", 7, apart),
        ("xs = [a = 1]", 8, apart),
        ("xs[i = 0]", 5, apart),
        ("a = b = 1", 6, apart),
        ("X = y = 1", 6, apart),
        ("puts x = 1", 7, apart),
        ("c ? a = 1 : 2", 6, apart),
        ("puts \"#{n = 1}\"", 10, apart),
        ("case x = 1\nend", 7, apart),
        ("case x\nwhen y = 1\nend", 14, apart),
        ("if x = 5\nend", 5, compare),
        ("if (x = 5)\nend", 6, compare),
        ("unless x = 5\nend", 9, compare),
        ("if a\nelsif x = 5\nend", 13, compare),
        ("while x = next_one\nend", 8, compare),
        ("until (x = 5) > 3\nend", 9, compare),
        ("puts 1 if x = 5", 12, compare),
        ("if x += 1\nend", 5, apart),
        ("if a\n  y = (x = 2)\nend", 14, apart),
    ] {
        let error = error(text);
        assert_eq!(error.message, message, "{text:?}");
        assert_eq!(error.span.start, start, "{text:?}");
        assert_eq!(
            error.span.end - error.span.start,
            1 + u32::from(text.contains("+=")),
            "{text:?}"
        );
        assert_eq!(error.label.as_deref(), Some(label), "{text:?}");
    }
    // Where `=` can't follow for another reason, that reason is given.
    assert_error(
        "def f(a = 1)\nend",
        "unexpected `=`",
        "=",
        "expected `:` and the parameter's type",
    );
    assert_error("x = = 1", "unexpected `=`", "=", "expected an expression");
}

#[test]
fn calls_without_parentheses() {
    let message = "a call with arguments needs parentheses";
    assert_error("x = double 4", message, "4", "write `double(4)`");
    assert_error("foo bar", message, "bar", "write `foo(bar)`");
    assert_error(
        "add 1, 2 + 3 if ready",
        message,
        "1",
        "write `add(1, 2 + 3)`",
    );
    assert_error("xs.push 4", message, "4", "write `xs.push(4)`");
    assert_error(
        "puts greet \"hi\"",
        message,
        "\"hi\"",
        "write `greet(\"hi\")`",
    );
    assert_error(
        "x = wrap Vec2(1, 2), true",
        message,
        "Vec2",
        "write `wrap(Vec2(1, 2), true)`",
    );
    assert_error("puts \"#{double n}\"", message, "n", "write `double(n)`");
    assert_error("if ready go end", message, "go", "write `ready(go)`");
    assert_error("3.times do |i| show i end", message, "i", "write `show(i)`");
    assert_error(
        "a.b\n  .c d",
        message,
        "d",
        "put the arguments in parentheses",
    );
    // Brackets and `-` after a name are indexing, a call and subtraction.
    assert_dumps(&[
        ("x [1]", "x[1]"),
        ("f (1)", "f(1)"),
        ("x -1", "(x - 1)"),
        ("f(a,\n  b)", "f(a, b)"),
    ]);
    // Inside brackets a missing comma is the likelier mistake.
    assert_error("f(a\n  b)", "unexpected `b`", "b", "expected `,` or `)`");
}

#[test]
fn do_blocks_on_calls_that_take_none() {
    let label = "only `times`, `upto`, `downto`, `each` and `each_with_index` take one";
    assert_error(
        "repeat(3) do\n  puts 1\nend",
        "`repeat` does not take a `do` block",
        "repeat",
        label,
    );
    assert_error(
        "repeat do\nend",
        "`repeat` does not take a `do` block",
        "repeat",
        label,
    );
    assert_error(
        "x = build(1) do |b|\nend",
        "`build` does not take a `do` block",
        "build",
        label,
    );
    assert_error(
        "v: Vec2 = Vec2(1, 2) do\nend",
        "`Vec2` does not take a `do` block",
        "Vec2",
        label,
    );
    assert_error(
        "while ready do\nend",
        "unexpected `do`",
        "do",
        "expected a newline",
    );
}

#[test]
fn keywords_as_names() {
    for (text, word) in [
        ("tag = \"boss\"", "tag"),
        ("end = 5", "end"),
        ("loop += 1", "loop"),
        ("each: Int = 1", "each"),
        ("def f\n  on = true\nend", "on"),
        ("def f(event: Int)\nend", "event"),
        ("def end\nend", "end"),
        ("def loop(n: Int)\nend", "loop"),
        ("struct A\n  tag: Int\nend", "tag"),
        ("struct A\n  end: Int\nend", "end"),
        ("3.times do |on|\nend", "on"),
        ("system each\nend", "each"),
    ] {
        let error = error(text);
        assert_eq!(
            error.message,
            format!("`{word}` is a keyword and can't be used as a name"),
            "{text:?}"
        );
        assert_eq!(marked(text, error.span), word, "{text:?}");
        assert_eq!(
            error.span.start as usize,
            text.find(word).unwrap(),
            "{text:?}"
        );
        assert_eq!(
            error.label.as_deref(),
            Some("choose a different name"),
            "{text:?}"
        );
    }
    assert_error(
        "gpu = 1",
        "`gpu` is a keyword and can't be used as a name",
        "gpu",
        "`gpu` is reserved for GPU systems",
    );
    // `self` is left for the checker, which says why it is read-only.
    assert_eq!(dump("self = 1"), "self = 1");
}

#[test]
fn equality_can_not_be_defined() {
    for (text, op) in [
        (
            "struct V\n  def ==(other: V) -> Bool\n    true\n  end\nend",
            "==",
        ),
        ("def !=(other: V)\nend", "!="),
    ] {
        let error = error(text);
        assert_eq!(
            error.message,
            format!("`{op}` is provided for every type and can't be redefined")
        );
        assert_eq!(marked(text, error.span), op);
        assert_eq!(error.label, None);
    }
}

#[test]
fn systems_take_no_parameters() {
    assert_error(
        "system move(dt: Float)\nend",
        "a system takes no parameters",
        "(",
        "systems read what they need from resources and queries",
    );
    assert_error(
        "system move()\nend",
        "a system takes no parameters",
        "(",
        "systems read what they need from resources and queries",
    );
}

#[test]
fn comparisons_do_not_chain() {
    let error = error("a < b < c");
    assert_eq!(error.message, "comparisons can't be chained");
    assert_eq!((error.span.start, error.span.end), (6, 7));
    assert_eq!(
        error.label.as_deref(),
        Some("join two comparisons with `&&`")
    );
    assert_error(
        "a <= b > c",
        "comparisons can't be chained",
        ">",
        "join two comparisons with `&&`",
    );
}

#[test]
fn spawn_and_command_errors() {
    assert_error(
        "x = y + spawn A",
        "`spawn` with arguments needs parentheses here",
        "spawn",
        "write `spawn(…)` inside a larger expression",
    );
    assert_error(
        "foo(spawn Position(0, 0))",
        "`spawn` with arguments needs parentheses here",
        "spawn",
        "write `spawn(…)` inside a larger expression",
    );
    assert_error(
        "emit A, B",
        "`emit` takes exactly one event",
        "emit A, B",
        "write one `emit` for each event",
    );
    assert_error(
        "emit",
        "`emit` takes exactly one event",
        "emit",
        "expected an event after `emit`",
    );
    assert_error("puts x: 1", "unexpected `:`", ":", "expected a newline");
}

#[test]
fn query_errors() {
    assert_error(
        "each with: A do |a|\nend",
        "a query needs at least one term",
        "with: A",
        "name a component or `Entity` before the filters",
    );
    assert_error(
        "each A, with: B, C do |a|\nend",
        "query terms come before the filters",
        "C",
        "move this before `with:` and `without:`",
    );
    assert_error(
        "each A, wit: B do |a|\nend",
        "unknown query filter `wit`",
        "wit",
        "expected `with:` or `without:`",
    );
    assert_error(
        "each a do |x|\nend",
        "unexpected `a`",
        "a",
        "expected a component or `Entity`, which begins with an uppercase letter",
    );
    assert_error("each A |a|\nend", "unexpected `|`", "|", "expected `do`");
    assert_error(
        "each A, with: [B do |a|\nend",
        "unexpected `do`",
        "do",
        "expected `,` or `]`",
    );
    assert_error(
        "on hit do |h|\nend",
        "unexpected `hit`",
        "hit",
        "expected an event type, which begins with an uppercase letter",
    );
    assert_error("loop\nend", "unexpected end of line", "", "expected `do`");
    assert_error(
        "loop do |x|\nend",
        "`loop` takes no block parameters",
        "x",
        "remove these",
    );
}

#[test]
fn control_flow_errors() {
    assert_error(
        "unless a\n  1\nelsif b\n  2\nend",
        "unexpected `elsif`",
        "elsif",
        "`unless` can't have an `elsif`",
    );
    assert_error(
        "case x\n  puts 1\nend",
        "unexpected `puts`",
        "puts",
        "expected `when`",
    );
    assert_error(
        "while x puts 1 end",
        "unexpected `puts`",
        "puts",
        "expected a newline",
    );
    assert_error(
        "if a > 0 b end",
        "unexpected `b`",
        "b",
        "expected `then` or a newline",
    );

    let text = "while x\n  1\nelse\n  2\nend";
    let error = error(text);
    assert_eq!(error.message, "unexpected `else`");
    assert_eq!(error.label.as_deref(), Some("expected `end`"));
    assert_eq!(error.notes[0].message, "to close this `while`");
    assert_eq!(marked(text, error.notes[0].span), "while");
}

#[test]
fn declaration_errors() {
    assert_error(
        "def Foo\nend",
        "unexpected `Foo`",
        "Foo",
        "function names begin with a lowercase letter",
    );
    assert_error(
        "def <(o: A)\nend",
        "unexpected `<`",
        "<",
        "a method can define `+`, `-`, `*`, `/`, `%` and `-@`",
    );
    assert_error(
        "def (o: A)\nend",
        "unexpected `(`",
        "(",
        "expected a function name",
    );
    assert_error(
        "def f(a)\nend",
        "unexpected `)`",
        ")",
        "expected `:` and the parameter's type",
    );
    assert_error(
        "def f(a: Int) Int\nend",
        "unexpected `Int`",
        "Int",
        "expected `->` or a newline",
    );
    assert_error(
        "struct foo\nend",
        "unexpected `foo`",
        "foo",
        "expected a type name, which begins with an uppercase letter",
    );
    assert_error(
        "struct A\n  x Int\nend",
        "unexpected `Int`",
        "Int",
        "expected `:` and the field's type",
    );
    assert_error(
        "struct A\n  X: Int\nend",
        "unexpected `X`",
        "X",
        "field names begin with a lowercase letter",
    );
    assert_error(
        "struct A\n  puts 1\nend",
        "unexpected `puts`",
        "puts",
        "expected a field, `def` or `end`",
    );
    assert_error(
        "resource R\n  1\nend",
        "unexpected `1`",
        "1",
        "expected a field or `end`",
    );
    assert_error(
        "enum E\n  a\nend",
        "unexpected `a`",
        "a",
        "expected a variant name, which begins with an uppercase letter",
    );
    assert_error(
        "enum E\n  A B\nend",
        "unexpected `B`",
        "B",
        "expected `,` or a newline",
    );
    assert_error(
        "tag x",
        "unexpected `x`",
        "x",
        "expected a type name, which begins with an uppercase letter",
    );
    assert_error(
        "system Move\nend",
        "unexpected `Move`",
        "Move",
        "expected a system name, which begins with a lowercase letter",
    );
    assert_error(
        "x: int = 1",
        "unexpected `int`",
        "int",
        "expected a type, which begins with an uppercase letter",
    );
    assert_error(
        "require \"a#{b}\"",
        "`require` takes a plain string",
        "\"a#{b}\"",
        "interpolation is not allowed here",
    );
    assert_error(
        "require foo",
        "unexpected `foo`",
        "foo",
        "expected a file name in quotes",
    );
}
