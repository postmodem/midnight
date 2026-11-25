//! Tests of the generated C: typed programs built by hand, checked as
//! text, by the C compiler with warnings as errors and, when the runtime
//! is complete, by running them in both build modes.

mod build;

use std::path::PathBuf;
use std::process::Command;

use crate::source::{FileId, SourceMap, Span};
use crate::tir::*;
use build::*;

const RUNTIME_H: &str = include_str!("../../runtime/midnight_rt.h");
const RUNTIME_C: &str = include_str!("../../runtime/midnight_rt.c");

const SOURCE: &str = "first\nsecond\nthird\nfourth\n";

fn sources() -> SourceMap {
    let mut sources = SourceMap::new();
    sources.add(
        "test.12am".to_string(),
        PathBuf::from("test.12am"),
        SOURCE.to_string(),
    );
    sources
}

fn emit(program: &Program) -> String {
    super::emit(program, &sources())
}

/// The code with indentation, blank lines and `#line` directives removed,
/// for comparing sequences of statements.
fn squash(c: &str) -> String {
    let mut out = String::new();
    for line in c.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with("#line") {
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// Asserts that the lines appear one directly after another.
#[track_caller]
fn assert_seq(c: &str, lines: &[&str]) {
    let wanted = lines.join("\n");
    assert!(
        squash(c).contains(&wanted),
        "the generated code lacks\n{wanted}\n\n{}",
        numbered(c)
    );
}

#[track_caller]
fn assert_has(c: &str, text: &str) {
    assert!(
        c.contains(text),
        "the generated code lacks `{text}`\n\n{}",
        numbered(c)
    );
}

#[track_caller]
fn assert_lacks(c: &str, text: &str) {
    assert!(
        !c.contains(text),
        "the generated code has `{text}`\n\n{}",
        numbered(c)
    );
}

fn numbered(c: &str) -> String {
    c.lines()
        .enumerate()
        .map(|(index, line)| format!("{:4} {line}\n", index + 1))
        .collect()
}

/// The text of one generated function, from its definition to its end.
fn function_text<'c>(c: &'c str, header: &str) -> &'c str {
    let start = c
        .find(&format!("{header} {{"))
        .unwrap_or_else(|| panic!("no definition of `{header}`\n\n{}", numbered(c)));
    let length = c[start..].find("\n}\n").expect("the function ends");
    &c[start..start + length + 3]
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("midnight-emit-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create a scratch directory");
    std::fs::write(dir.join("midnight_rt.h"), RUNTIME_H).expect("write the runtime header");
    std::fs::write(dir.join("midnight_rt.c"), RUNTIME_C).expect("write the runtime");
    dir
}

/// Has the C compiler check the code with warnings as errors. Does
/// nothing when `MIDNIGHT_CC` is not set.
#[track_caller]
fn check(name: &str, c: &str) {
    if std::env::var_os("MIDNIGHT_DUMP_C").is_some() {
        eprintln!("==== {name}\n{c}");
    }
    let Ok(cc) = std::env::var("MIDNIGHT_CC") else {
        return;
    };
    let dir = scratch(name);
    std::fs::write(dir.join("program.c"), c).expect("write the program");
    let output = Command::new(cc)
        .args(["-std=c11", "-fsyntax-only", "-Wall", "-Wextra", "-Werror"])
        // A compiler wrapper may pass linker flags, which go unused here.
        .arg("-Wno-unused-command-line-argument")
        .arg(dir.join("program.c"))
        .output()
        .expect("run the C compiler");
    let _ = std::fs::remove_dir_all(&dir);
    assert!(
        output.status.success(),
        "{name}: the C compiler rejected the generated code\n{}\n{}",
        String::from_utf8_lossy(&output.stderr),
        numbered(c)
    );
}

/// Whether the runtime next to the compiler implements the whole header.
fn runtime_is_complete() -> bool {
    RUNTIME_C.contains("mn_query_refresh") && RUNTIME_C.contains("mn_list_push")
}

/// Builds the program with the runtime in both modes and runs it. Each
/// run must exit with status 0 and write nothing to standard error; the
/// debug run must agree with the release run. Returns standard output, or
/// `None` when there is no C compiler or the runtime is incomplete.
#[track_caller]
fn run(name: &str, c: &str) -> Option<String> {
    check(name, c);
    let cc = std::env::var("MIDNIGHT_CC").ok()?;
    if !runtime_is_complete() {
        return None;
    }
    let dir = scratch(name);
    std::fs::write(dir.join("program.c"), c).expect("write the program");
    let modes: [&[&str]; 2] = [
        &["-O2", "-fwrapv"],
        &[
            "-O0",
            "-g",
            "-fwrapv",
            "-fsanitize=address,undefined",
            "-DGR_DEBUG",
        ],
    ];
    let mut outputs = Vec::new();
    for (index, flags) in modes.iter().enumerate() {
        let exe = dir.join(format!("program{index}"));
        let build = Command::new(&cc)
            .arg("-std=c11")
            .args(*flags)
            .args(["-Wall", "-Wextra", "-Werror", "-o"])
            .arg(&exe)
            .arg(dir.join("program.c"))
            .arg(dir.join("midnight_rt.c"))
            .arg("-lm")
            .output()
            .expect("run the C compiler");
        assert!(
            build.status.success(),
            "{name} {flags:?}: the build failed\n{}\n{}",
            String::from_utf8_lossy(&build.stderr),
            numbered(c)
        );
        let output = Command::new(&exe).output().expect("run the program");
        assert!(
            output.status.code() == Some(0) && output.stderr.is_empty(),
            "{name} {flags:?}: exited with {:?}\n--- stdout\n{}--- stderr\n{}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
            numbered(c)
        );
        outputs.push(String::from_utf8_lossy(&output.stdout).into_owned());
    }
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(outputs[0], outputs[1], "{name}: the two modes disagree");
    outputs.pop()
}

/// Runs the program and compares its output, when it can be run.
#[track_caller]
fn expect_output(name: &str, c: &str, expected: &str) {
    if let Some(output) = run(name, c) {
        assert_eq!(output, expected, "{name}\n{}", numbered(c));
    }
}

fn tick() -> Stmt {
    es(ex(ExprKind::WorldTick, Type::Void))
}

// ---- The first program of the reference ----

/// `examples/move.12am` with fields of the given storage type.
fn move_program(fty: &Type) -> Program {
    let mut p = empty_program();
    let pair = |a: &'static str, b: &'static str| [(a, fty.clone()), (b, fty.clone())];
    let (position, position_ty) = component(&mut p, "Position", &pair("x", "y"));
    let (velocity, velocity_ty) = component(&mut p, "Velocity", &pair("dx", "dy"));
    let frozen = tag(&mut p, "Frozen");

    let mut l = Locals::default();
    let pos = l.view("pos", &position_ty);
    let vel = l.view("vel", &velocity_ty);
    let step = |axis: usize| {
        cset(
            &fld(&pos, axis, fty),
            BinOp::Add,
            bin(
                BinOp::Mul,
                rd(&fld(&vel, axis, fty)),
                rd(&time(TimeField::Dt)),
            ),
        )
    };
    let body = vec![step(0), step(1)];
    let terms = vec![Some((position, true)), Some((velocity, false))];
    let stmt = each(terms, vec![], vec![frozen], vec![&pos, &vel], body);
    system(&mut p, "move", l.body(blk(vec![stmt])));

    let mut l = Locals::default();
    let e = l.var("e", &Type::Entity);
    let pos = l.view("pos", &position_ty);
    let line = concat(vec![
        to_s(rd(&e)),
        text(" is at ("),
        to_s(rd(&fld(&pos, 0, fty))),
        text(", "),
        to_s(rd(&fld(&pos, 1, fty))),
        text(")"),
    ]);
    let terms = vec![None, Some((position, false))];
    let stmt = each(
        terms,
        vec![],
        vec![],
        vec![&e, &pos],
        vec![puts(vec![line])],
    );
    system(&mut p, "report", l.body(blk(vec![stmt])));

    let position_of = |x: f64, y: f64| Some(make(&position_ty, vec![flt(x), flt(y)]));
    let velocity_of = |x: f64, y: f64| Some(make(&velocity_ty, vec![flt(x), flt(y)]));
    p.main = Locals::default().body(blk(vec![
        es(spawn(vec![
            (position, position_of(0.0, 0.0)),
            (velocity, velocity_of(1.0, 2.0)),
        ])),
        es(spawn(vec![
            (position, position_of(5.0, 5.0)),
            (velocity, velocity_of(-1.0, 0.0)),
            (frozen, None),
        ])),
        set(&time(TimeField::Dt), flt(0.5)),
        times(None, int(3), vec![tick()]),
    ]));
    p
}

#[test]
fn move_is_a_loop_over_columns() {
    let c = emit(&move_program(&Type::Float));
    let system = function_text(&c, "static void S_move(void)");
    assert_seq(
        system,
        &[
            "mn_query_update(&q0);",
            "mn_defer_enter();",
            "const double time_dt = mn_time.dt;",
            "for (uint32_t t = 0; t < q0.table_count; t++) {",
            "MnTable *table = q0.tables[t];",
            "uint32_t n = table->count;",
            "if (n == 0) continue;",
            "T_Position *restrict c0 = mn_column(table, 0);",
            "const T_Velocity *restrict c1 = mn_column(table, 1);",
            "for (uint32_t i = 0; i < n; i++) {",
            "c0[i].x = c0[i].x + c1[i].dx * time_dt;",
            "c0[i].y = c0[i].y + c1[i].dy * time_dt;",
            "}",
            "}",
            "mn_defer_leave();",
        ],
    );
    assert_has(&c, "static const uint32_t q0_with[] = {0, 1};");
    assert_has(&c, "static const uint32_t q0_without[] = {2};");
    assert_has(
        &c,
        "static MnQuery q0 = {q0_with, 2, q0_without, 1, NULL, 0, 0, 0};",
    );
    assert_has(&c, "{\"Frozen\", 0, 1, NULL},");
    let report = function_text(&c, "static void S_report(void)");
    assert_seq(
        report,
        &[
            "const T_Position *restrict c1 = mn_column(table, 0);",
            "const MnEntity *ents = table->entities;",
            "for (uint32_t i = 0; i < n; i++) {",
            "MnEntity e_0 = ents[i];",
            "mn_out_entity(e_0);",
            "mn_out_bytes(\" is at (\", 8);",
            "mn_out_float(c1[i].x);",
            "mn_out_bytes(\", \", 2);",
            "mn_out_float(c1[i].y);",
            "mn_out_bytes(\")\\n\", 2);",
        ],
    );
    expect_output(
        "move",
        &c,
        "Entity(0v1) is at (0.5, 1.0)\nEntity(1v1) is at (5.0, 5.0)\n\
         Entity(0v1) is at (1.0, 2.0)\nEntity(1v1) is at (5.0, 5.0)\n\
         Entity(0v1) is at (1.5, 3.0)\nEntity(1v1) is at (5.0, 5.0)\n",
    );
}

#[test]
fn compact_fields_convert_on_load_and_store() {
    let c = emit(&move_program(&f32_type()));
    assert_has(&c, "    float x;\n    float y;\n} T_Position;");
    assert_has(
        &c,
        "c0[i].x = (float)((double)c0[i].x + (double)c1[i].dx * time_dt);",
    );
    let c = format!("{c}_Static_assert(sizeof(T_Position) == 8, \"two floats\");\n");
    check("move32", &c);
}

// ---- Numbers ----

#[test]
fn arithmetic_and_conversions() {
    let mut p = empty_program();
    let mut l = Locals::default();
    let a = l.param("a", &Type::Int);
    let b = l.param("b", &Type::Int);
    let x = l.param("x", &Type::Float);
    let q = l.var("q", &Type::Int);
    let big = l.var("big", &Type::Int);
    let n = l.var("n", &Type::Int);
    let y = l.var("y", &Type::Float);
    let ok = l.var("ok", &Type::Bool);
    let f = |op: Builtin, args: Vec<Expr>| bi(op, args, Type::Float);
    let i = |op: Builtin, args: Vec<Expr>| bi(op, args, Type::Int);
    let sum = |terms: Vec<Expr>| {
        terms
            .into_iter()
            .reduce(|left, right| bin(BinOp::Add, left, right))
            .expect("at least one term")
    };
    let body = blkv(
        vec![
            let_(
                &q,
                sum(vec![
                    bin(BinOp::Div, rd(&a), rd(&b)),
                    bin(BinOp::Mod, rd(&a), rd(&b)),
                    bin(BinOp::Pow, rd(&a), int(2)),
                    bin(
                        BinOp::Sub,
                        bin(BinOp::Mul, neg(rd(&a)), int(3)),
                        bin(BinOp::Sub, rd(&a), neg(rd(&b))),
                    ),
                ]),
            ),
            let_(
                &big,
                bin(
                    BinOp::Add,
                    bin(BinOp::Mul, int(100_000), int(100_000)),
                    neg(int(i64::MIN)),
                ),
            ),
            let_(
                &n,
                sum(vec![
                    i(Builtin::FloatToI, vec![rd(&x)]),
                    i(Builtin::FloatFloor, vec![rd(&x)]),
                    i(Builtin::FloatCeil, vec![rd(&x)]),
                    i(Builtin::FloatRound, vec![rd(&x)]),
                    i(Builtin::IntAbs, vec![rd(&a)]),
                    i(Builtin::IntMin, vec![rd(&a), rd(&b)]),
                    i(Builtin::IntMax, vec![rd(&a), int(7)]),
                    i(Builtin::IntMax, vec![int(3), int(7)]),
                    i(Builtin::IntClamp, vec![rd(&a), int(0), int(10)]),
                    i(Builtin::IntAbs, vec![int(-4)]),
                ]),
            ),
            let_(
                &y,
                sum(vec![
                    bin(BinOp::Div, rd(&x), flt(2.0)),
                    bin(BinOp::Mod, rd(&x), flt(3.0)),
                    bin(BinOp::Pow, rd(&x), flt(2.0)),
                    neg(neg(rd(&x))),
                    bin(BinOp::Mul, to_f(rd(&a)), to_f(int(2))),
                    f(Builtin::Sqrt, vec![rd(&x)]),
                    f(Builtin::Sin, vec![rd(&x)]),
                    f(Builtin::Cos, vec![rd(&x)]),
                    f(Builtin::Tan, vec![rd(&x)]),
                    f(Builtin::Atan2, vec![rd(&x), flt(1.0)]),
                    f(Builtin::Exp, vec![rd(&x)]),
                    f(Builtin::Log, vec![rd(&x)]),
                    f(Builtin::FloatAbs, vec![rd(&x)]),
                    f(Builtin::FloatRoundDigits, vec![rd(&x), int(2)]),
                    f(Builtin::FloatClamp, vec![rd(&x), flt(0.0), flt(1.0)]),
                    f(Builtin::FloatMin, vec![rd(&x), flt(-1.5)]),
                    f(Builtin::FloatMax, vec![rd(&x), flt(f64::INFINITY)]),
                    flt(1e300),
                    flt(-0.0),
                ]),
            ),
            let_(
                &ok,
                or(
                    and(
                        bin(BinOp::Lt, rd(&a), rd(&b)),
                        bin(BinOp::Ge, rd(&x), flt(1.0)),
                    ),
                    not(bin(BinOp::Eq, rd(&a), rd(&b))),
                ),
            ),
            when(
                bi(Builtin::FloatNan, vec![rd(&x)], Type::Bool),
                vec![ret(Some(flt(f64::NAN)))],
            ),
        ],
        iff(
            rd(&ok),
            blkv(vec![], rd(&y)),
            blkv(vec![], to_f(sum(vec![rd(&q), rd(&big), rd(&n), int(1)]))),
            Type::Float,
        ),
    );
    let calc = function(&mut p, "calc", None, 3, Type::Float, l.body(body));
    p.main = Locals::default().body(blk(vec![puts(vec![to_s(call(
        calc,
        vec![int(7), int(2), flt(2.5)],
        &Type::Float,
    ))])]));

    let c = emit(&p);
    assert_has(
        &c,
        "static double F_calc(int64_t a_0, int64_t b_1, double x_2)",
    );
    assert_has(&c, "mn_int_div(a_0, b_1, (MnLoc){src0, 1})");
    assert_has(&c, "mn_int_mod(a_0, b_1, (MnLoc){src0, 1})");
    assert_has(&c, "mn_int_pow(a_0, 2, (MnLoc){src0, 1})");
    assert_has(&c, "-a_0 * 3 - (a_0 - -b_1)");
    assert_has(&c, "int64_t big_4 = -9223372026854775808;");
    assert_has(&c, "(a_0 < 0 ? (int64_t)(0 - (uint64_t)a_0) : a_0)");
    assert_has(&c, "(a_0 > 7 ? a_0 : (int64_t)7)");
    assert_has(
        &c,
        "(a_0 < 0 ? (int64_t)0 : (a_0 > 10 ? (int64_t)10 : a_0))",
    );
    assert_has(&c, "mn_float_to_i(x_2) + mn_float_floor(x_2)");
    assert_has(&c, "mn_float_mod(x_2, 3.0)");
    assert_has(&c, "pow(x_2, 2.0)");
    assert_has(&c, "(double)a_0 * 2.0");
    assert_has(&c, "(x_2 < -1.5 ? x_2 : -1.5)");
    assert_has(
        &c,
        "bool ok_7 = (a_0 < b_1 && x_2 >= 1.0) || !(a_0 == b_1);",
    );
    assert_has(&c, "if (x_2 != x_2) {");
    assert_has(&c, "return (double)NAN;");
    assert_has(&c, "return (ok_7 ? y_6 : (double)(q_3 + big_4 + n_5 + 1));");
    assert_lacks(&c, "(int64_t)x_2");
    check("arithmetic", &c);
}

// ---- Strings ----

fn string_program() -> Program {
    let mut p = empty_program();
    let string = Type::String;
    let mut l = Locals::default();
    let name = l.param("name", &string);
    let count = l.param("n", &Type::Int);
    let s = l.var("s", &string);
    let t = l.var("t", &string);
    let u = l.var("u", &string);
    let len = l.var("len", &Type::Int);
    let v = l.var("v", &string);
    let b = |op: Builtin, args: Vec<Expr>| bi(op, args, Type::Bool);
    let st_ = |op: Builtin, args: Vec<Expr>| bi(op, args, Type::String);
    let body = blkv(
        vec![
            let_(&s, bin(BinOp::Add, rd(&name), text("!"))),
            let_(
                &t,
                concat(vec![
                    rd(&name),
                    text(" has "),
                    to_s(rd(&count)),
                    text(" items"),
                ]),
            ),
            let_(&u, st_(Builtin::StrUpcase, vec![rd(&s)])),
            when(bin(BinOp::Eq, rd(&u), text("X!")), vec![ret(Some(rd(&t)))]),
            when(
                bin(BinOp::Lt, rd(&s), rd(&t)),
                vec![ret(Some(st_(Builtin::StrDowncase, vec![rd(&u)])))],
            ),
            let_(
                &len,
                bin(
                    BinOp::Add,
                    bi(Builtin::StrLength, vec![rd(&t)], Type::Int),
                    bi(Builtin::StrBytesize, vec![rd(&name)], Type::Int),
                ),
            ),
            while_(
                bin(BinOp::Gt, rd(&len), int(0)),
                vec![
                    let_(&v, st_(Builtin::StrRjust, vec![rd(&t), rd(&len)])),
                    when(
                        b(Builtin::StrEmpty, vec![rd(&v)]),
                        vec![st(StmtKind::Break)],
                    ),
                    cset(&len, BinOp::Sub, int(1)),
                    when(
                        b(Builtin::StrInclude, vec![rd(&v), text("a")]),
                        vec![st(StmtKind::Next)],
                    ),
                    when(
                        b(Builtin::StrStartWith, vec![rd(&v), text("zz")]),
                        vec![ret(Some(rd(&v)))],
                    ),
                    set(&s, rd(&v)),
                ],
            ),
            set(&s, bin(BinOp::Mul, rd(&s), int(2))),
            cset(&s, BinOp::Add, rd(&u)),
            cset(
                &s,
                BinOp::Add,
                st_(Builtin::StrLjust, vec![text("."), int(3)]),
            ),
            es(st_(Builtin::StrUpcase, vec![rd(&s)])),
            when(
                b(Builtin::StrEndWith, vec![rd(&s), rd(&u)]),
                vec![set(&s, text(""))],
            ),
        ],
        rd(&s),
    );
    let label = function(&mut p, "label", None, 2, Type::String, l.body(body));

    let mut l = Locals::default();
    let r = l.var("r", &string);
    p.main = l.body(blk(vec![
        let_(&r, call(label, vec![text("ab"), int(2)], &string)),
        puts(vec![rd(&r)]),
        puts(vec![call(label, vec![rd(&r), int(1)], &string)]),
        puts(vec![call(label, vec![text("x"), int(0)], &string)]),
        puts(vec![
            to_s(bi(Builtin::StrToI, vec![text(" 42abc")], Type::Int)),
            to_s(bi(Builtin::StrToF, vec![text("1.5x")], Type::Float)),
        ]),
        st(StmtKind::Print(vec![
            text("quote \" backslash \\ question ??/ tab \t nul \0 bell \x07 \u{e9}\n"),
            concat(vec![]),
        ])),
    ]));
    p
}

#[test]
fn strings_are_released_on_every_path() {
    let c = emit(&string_program());
    let label = function_text(&c, "static MnString F_label(MnString name_0, int64_t n_1)");
    assert_has(
        label,
        "MnString s_2 = mn_str_concat(name_0, ((MnString)&str0));",
    );
    assert_seq(
        label,
        &[
            "MnBuilder sb0 = {0};",
            "mn_sb_str(&sb0, name_0);",
            "mn_sb_bytes(&sb0, \" has \", 5);",
            "mn_sb_int(&sb0, n_1);",
            "mn_sb_bytes(&sb0, \" items\", 6);",
            "MnString t_3 = mn_sb_finish(&sb0);",
        ],
    );
    // An early return of a local: a reference for the caller, then every
    // local is released.
    assert_seq(
        label,
        &[
            "mn_str_retain(t1);",
            "mn_str_release(u_4);",
            "mn_str_release(t_3);",
            "mn_str_release(s_2);",
            "return t1;",
        ],
    );
    assert_has(label, "if (mn_str_cmp(s_2, t_3) < 0) {");
    // `break`, `next` and `return` in the loop release the body's local.
    assert_seq(
        label,
        &[
            "if (mn_str_bytesize(v_6) == 0) {",
            "mn_str_release(v_6);",
            "break;",
        ],
    );
    assert_seq(
        label,
        &[
            "if (mn_str_include(v_6, ((MnString)&str2))) {",
            "mn_str_release(v_6);",
            "continue;",
        ],
    );
    // Assignment releases the old value after the new one exists.
    assert_seq(
        label,
        &["mn_str_release(s_2);", "s_2 = t4;", "mn_str_release(v_6);"],
    );
    // Parameters are borrowed: never released.
    assert_lacks(label, "mn_str_release(name_0)");
    let main = function_text(&c, "static void run_main(void)");
    assert_has(main, "MnString r_0 = F_label(((MnString)&str");
    assert_has(
        &c,
        "\"quote \\\" backslash \\\\ question \\?\\?/ tab \\t nul \\000 bell \\007 \\303\\251\\n\"",
    );
    expect_output(
        "strings",
        &c,
        "ab!ab!AB!.  \nab!ab!AB!.  !ab!ab!AB!.  !AB!AB!AB!.  !.  \nx has 0 items\n42\n1.5\nquote \" backslash \\ question ??/ tab \t nul \0 bell \x07 \u{e9}\n",
    );
}

// ---- Lists ----

fn list_program() -> Program {
    let mut p = empty_program();
    let int_list = list_of(&Type::Int);
    let string_list = list_of(&Type::String);
    let grid_type = list_of(&int_list);
    let bytes_type = list_of(&Type::Compact(Compact::UInt8));
    let floats_type = list_of(&f32_type());
    let mut l = Locals::default();
    let xs = l.var("xs", &int_list);
    let ys = l.var("ys", &int_list);
    let n = l.var("n", &Type::Int);
    let names = l.var("names", &string_list);
    let first = l.var("first", &Type::String);
    let name = l.var("name", &Type::String);
    let i = l.var("i", &Type::Int);
    let grid = l.var("grid", &grid_type);
    let bytes = l.var("bytes", &bytes_type);
    let floats = l.var("fs", &floats_type);
    let all = l.var("all", &int_list);
    let row = l.var("row", &int_list);
    let cell = l.var("cell", &Type::Int);
    let length = |list: &Place| bi(Builtin::ListLength, vec![rd(list)], Type::Int);
    let delete_at = |list: &Place, index: Expr| {
        ex(
            ExprKind::ListDeleteAt {
                list: list.clone(),
                index: Box::new(index),
            },
            Type::Int,
        )
    };
    p.main = l.body(blk(vec![
        let_(&xs, list(&Type::Int, vec![int(1), int(2), int(3)])),
        let_(&ys, rd(&xs)),
        es(push(&ys, int(4))),
        set(
            &at(&xs, int(0)),
            bin(BinOp::Add, rd(&at(&xs, int(1))), int(10)),
        ),
        cset(&at(&xs, int(-1)), BinOp::Add, int(5)),
        puts(vec![to_s(rd(&xs)), to_s(rd(&ys))]),
        let_(
            &n,
            [
                bi(Builtin::ListFirst, vec![rd(&ys)], Type::Int),
                bi(Builtin::ListLast, vec![rd(&ys)], Type::Int),
                pop(&xs),
                delete_at(&ys, int(0)),
                length(&xs),
            ]
            .into_iter()
            .reduce(|left, right| bin(BinOp::Add, left, right))
            .expect("terms"),
        ),
        puts(vec![to_s(rd(&n)), to_s(rd(&xs)), to_s(rd(&ys))]),
        let_(&names, list(&Type::String, vec![text("a"), text("b")])),
        es(push(&names, bin(BinOp::Add, text("c"), text("d")))),
        set(&at(&names, int(0)), rd(&at(&names, int(1)))),
        let_(&first, rd(&at(&names, int(0)))),
        cset(&at(&names, int(2)), BinOp::Add, rd(&first)),
        list_each(
            rd(&names),
            Some(&name),
            Some(&i),
            vec![
                when(bin(BinOp::Eq, rd(&i), int(0)), vec![st(StmtKind::Next)]),
                es(push(&names, rd(&name))),
                puts(vec![concat(vec![to_s(rd(&i)), text(": "), rd(&name)])]),
                when(
                    bin(BinOp::Eq, rd(&name), text("cdb")),
                    vec![st(StmtKind::Break)],
                ),
            ],
        ),
        puts(vec![to_s(rd(&names)), es_pop_string(&names)]),
        let_(
            &grid,
            list(
                &int_list,
                vec![
                    list(&Type::Int, vec![int(1)]),
                    list(&Type::Int, vec![int(2), int(3)]),
                ],
            ),
        ),
        es(push(&at(&grid, int(1)), int(4))),
        set(&at(&at(&grid, int(0)), int(0)), int(7)),
        es(push(&grid, list(&Type::Int, vec![]))),
        list_each(
            rd(&grid),
            Some(&row),
            None,
            vec![list_each(
                rd(&row),
                Some(&cell),
                None,
                vec![st(StmtKind::Print(vec![to_s(rd(&cell)), text(" ")]))],
            )],
        ),
        puts(vec![to_s(rd(&grid))]),
        let_(
            &bytes,
            list(
                &Type::Compact(Compact::UInt8),
                vec![int(1), int(255), rd(&n)],
            ),
        ),
        set(&at(&bytes, int(0)), length(&xs)),
        cset(&at(&bytes, int(1)), BinOp::Sub, int(55)),
        es(push(&bytes, int(9))),
        let_(&floats, list(&f32_type(), vec![flt(1.5)])),
        cset(&at(&floats, int(0)), BinOp::Add, flt(1.0)),
        es(push(&floats, flt(0.25))),
        puts(vec![to_s(rd(&bytes)), to_s(rd(&floats))]),
        let_(
            &all,
            bin(BinOp::Add, rd(&xs), bin(BinOp::Mul, rd(&ys), int(2))),
        ),
        puts(vec![
            to_s(rd(&all)),
            to_s(bin(BinOp::Eq, rd(&xs), rd(&ys))),
            to_s(bin(BinOp::Ne, rd(&names), rd(&names))),
            to_s(bi(
                Builtin::ListInclude,
                vec![rd(&names), text("b")],
                Type::Bool,
            )),
            to_s(bi(
                Builtin::ListInclude,
                vec![rd(&bytes), int(200)],
                Type::Bool,
            )),
            to_s(bi(Builtin::ListEmpty, vec![rd(&xs)], Type::Bool)),
        ]),
        es(ex(ExprKind::ListClear { list: ys.clone() }, Type::Void)),
        puts(vec![to_s(rd(&ys))]),
    ]));
    p
}

/// `names.pop` as an argument of `puts`.
fn es_pop_string(names: &Place) -> Expr {
    pop(names)
}

#[test]
fn lists_copy_on_write_and_keep_counts() {
    let c = emit(&list_program());
    assert_seq(
        &c,
        &[
            "MnList t0 = mn_list_alloc(&Elem_Int, 3);",
            "int64_t *p1 = mn_list_data(t0);",
            "p1[0] = 1;",
            "p1[1] = 2;",
            "p1[2] = 3;",
            "MnList xs_0 = t0;",
            "MnList ys_1 = xs_0;",
            "mn_list_retain(ys_1);",
            "*(int64_t *)mn_list_push(&ys_1, &Elem_Int) = 4;",
        ],
    );
    // The right-hand side is read before the list is made private and
    // the element is looked up.
    assert_seq(
        &c,
        &[
            "int64_t t2 = mn_list_index(xs_0, 1, (MnLoc){src0, 1});",
            "const int64_t *p3 = (const int64_t *)mn_list_data(xs_0) + t2;",
            "int64_t t4 = *p3;",
            "mn_list_unique(&xs_0, &Elem_Int);",
            "int64_t t5 = mn_list_index(xs_0, 0, (MnLoc){src0, 1});",
            "int64_t *p6 = (int64_t *)mn_list_data(xs_0) + t5;",
            "*p6 = t4 + 10;",
        ],
    );
    assert_has(
        &c,
        "static const MnElem Elem_String = {sizeof(MnString), Retain_String, Drop_String};",
    );
    assert_has(
        &c,
        "static const MnElem Elem_L_Int = {sizeof(MnList), Retain_L_Int, Drop_L_Int};",
    );
    assert_has(
        &c,
        "static const MnElem Elem_UInt8 = {sizeof(uint8_t), NULL, NULL};",
    );
    assert_has(&c, "mn_to_uint8(n_2, (MnLoc){src0, 1})");
    assert_has(&c, "[1] = 255;");
    // A list loop walks a retained snapshot and gives each element a
    // reference of its own.
    assert_has(&c, "MnString name_5 = ((const MnString *)mn_list_data(");
    assert_seq(&c, &["mn_str_release(name_5);", "continue;"]);
    assert_seq(&c, &["mn_str_release(name_5);", "break;"]);
    expect_output(
        "lists",
        &c,
        "[12, 2, 8]\n[1, 2, 3, 4]\n16\n[12, 2]\n[2, 3, 4]\n\
         1: b\n2: cdb\n[\"b\", \"b\", \"cdb\", \"b\", \"cdb\"]\ncdb\n\
         7 2 3 4 [[7], [2, 3, 4], []]\n[2, 200, 16, 9]\n[2.5, 0.25]\n\
         [12, 2, 2, 3, 4, 2, 3, 4]\nfalse\nfalse\ntrue\ntrue\nfalse\n[]\n",
    );
}

// ---- Records ----

fn record_program() -> Program {
    let mut p = empty_program();
    let team = enumeration(&mut p, "Team", &["Red", "Blue"]);
    let vec2 = structure(&mut p, "Vec2", &[("x", Type::Float), ("y", Type::Float)]);
    let (_, mixed) = component(
        &mut p,
        "Mixed",
        &[
            ("flag", Type::Bool),
            ("small", Type::Compact(Compact::UInt8)),
            ("f", Type::Float),
            ("team", team.clone()),
            ("i", Type::Compact(Compact::Int16)),
            ("g", f32_type()),
        ],
    );
    let named = structure(
        &mut p,
        "Named",
        &[
            ("name", Type::String),
            ("tags", list_of(&Type::String)),
            ("pos", vec2.clone()),
            ("team", team.clone()),
        ],
    );
    let (_, nothing) = event(&mut p, "Nothing", &[]);
    let odd = structure(
        &mut p,
        "Odd",
        &[("int", Type::Int), ("_x", Type::Int), ("f_y", Type::Int)],
    );

    // def +(other: Vec2) -> Vec2
    let mut l = Locals::default();
    let this = l.param("self", &vec2);
    let other = l.param("other", &vec2);
    let x = |place: &Place| rd(&fld(place, 0, &Type::Float));
    let y = |place: &Place| rd(&fld(place, 1, &Type::Float));
    let body = blkv(
        vec![],
        make(
            &vec2,
            vec![
                bin(BinOp::Add, x(&this), x(&other)),
                bin(BinOp::Add, y(&this), y(&other)),
            ],
        ),
    );
    let plus = function(&mut p, "+", Some(&vec2), 2, vec2.clone(), l.body(body));

    // def length -> Float
    let mut l = Locals::default();
    let this = l.param("self", &vec2);
    let body = blkv(
        vec![],
        bi(
            Builtin::Sqrt,
            vec![bin(
                BinOp::Add,
                bin(BinOp::Mul, x(&this), x(&this)),
                bin(BinOp::Mul, y(&this), y(&this)),
            )],
            Type::Float,
        ),
    );
    let length = function(&mut p, "length", Some(&vec2), 1, Type::Float, l.body(body));

    // def renamed(suffix: String) -> Named, a method of Named
    let mut l = Locals::default();
    let this = l.param("self", &named);
    let suffix = l.param("suffix", &Type::String);
    let body = blkv(
        vec![],
        make(
            &named,
            vec![
                bin(BinOp::Add, rd(&fld(&this, 0, &Type::String)), rd(&suffix)),
                rd(&fld(&this, 1, &list_of(&Type::String))),
                rd(&fld(&this, 2, &vec2)),
                rd(&fld(&this, 3, &team)),
            ],
        ),
    );
    let renamed = function(&mut p, "*", Some(&named), 2, named.clone(), l.body(body));

    let mut l = Locals::default();
    let a = l.var("a", &vec2);
    let b = l.var("b", &vec2);
    let n = l.var("n", &named);
    let m = l.var("m", &named);
    let mix = l.var("mix", &mixed);
    let k = l.var("k", &Type::Int);
    let o = l.var("o", &odd);
    let people = l.var("people", &list_of(&named));
    let red = || ex(ExprKind::EnumVariant(0), team.clone());
    let blue = || ex(ExprKind::EnumVariant(1), team.clone());
    let method = |place: &Place, func: FuncId, value: Expr| {
        st(StmtKind::CompoundAssign {
            place: place.clone(),
            op: CompoundOp::Method(func),
            value,
        })
    };
    p.main = l.body(blk(vec![
        let_(&a, make(&vec2, vec![flt(1.0), flt(2.0)])),
        let_(
            &b,
            call(
                plus,
                vec![rd(&a), make(&vec2, vec![flt(2.0), flt(2.0)])],
                &vec2,
            ),
        ),
        method(&a, plus, rd(&b)),
        let_(
            &n,
            make(
                &named,
                vec![
                    text("x"),
                    list(&Type::String, vec![text("t")]),
                    rd(&a),
                    blue(),
                ],
            ),
        ),
        let_(&m, rd(&n)),
        set(&fld(&m, 0, &Type::String), text("y")),
        es(push(&fld(&m, 1, &list_of(&Type::String)), text("z\n"))),
        method(&m, renamed, text("!")),
        puts(vec![
            to_s(rd(&n)),
            to_s(rd(&m)),
            to_s(bin(BinOp::Eq, rd(&m), rd(&n))),
            to_s(bin(BinOp::Ne, rd(&a), rd(&b))),
            to_s(rd(&fld(&n, 3, &team))),
            to_s(call(length, vec![rd(&b)], &Type::Float)),
            to_s(ex(
                ExprKind::Field {
                    base: Box::new(call(plus, vec![rd(&a), rd(&b)], &vec2)),
                    field: 0,
                },
                Type::Float,
            )),
            ex(
                ExprKind::Field {
                    base: Box::new(call(renamed, vec![rd(&n), text("?")], &named)),
                    field: 0,
                },
                Type::String,
            ),
        ]),
        let_(&k, int(3)),
        let_(
            &mix,
            make(
                &mixed,
                vec![boolean(true), int(200), flt(1.0), red(), rd(&k), flt(0.1)],
            ),
        ),
        set(
            &fld(&mix, 4, &Type::Compact(Compact::Int16)),
            bin(BinOp::Mul, rd(&k), int(-5)),
        ),
        cset(
            &fld(&mix, 1, &Type::Compact(Compact::UInt8)),
            BinOp::Add,
            int(1),
        ),
        cset(&fld(&mix, 5, &f32_type()), BinOp::Mul, flt(2.0)),
        puts(vec![
            to_s(rd(&mix)),
            to_s(make(&nothing, vec![])),
            to_s(bin(
                BinOp::Eq,
                make(&nothing, vec![]),
                make(&nothing, vec![]),
            )),
        ]),
        let_(&o, make(&odd, vec![int(1), int(2), int(3)])),
        puts(vec![
            to_s(rd(&o)),
            to_s(bin(
                BinOp::Add,
                rd(&fld(&o, 0, &Type::Int)),
                bin(
                    BinOp::Add,
                    rd(&fld(&o, 1, &Type::Int)),
                    rd(&fld(&o, 2, &Type::Int)),
                ),
            )),
        ]),
        let_(&people, list(&named, vec![rd(&n), rd(&m)])),
        set(
            &fld(&at(&people, int(0)), 0, &Type::String),
            bin(
                BinOp::Add,
                rd(&fld(&at(&people, int(1)), 0, &Type::String)),
                text("+"),
            ),
        ),
        method(&at(&people, int(1)), renamed, text("#")),
        puts(vec![
            to_s(rd(&people)),
            to_s(bi(
                Builtin::ListInclude,
                vec![rd(&people), rd(&n)],
                Type::Bool,
            )),
            to_s(bin(BinOp::Eq, rd(&people), rd(&people))),
        ]),
    ]));
    p
}

#[test]
fn records_are_plain_structs_ordered_by_alignment() {
    let c = emit(&record_program());
    assert_has(
        &c,
        "typedef struct T_Mixed {\n    double f;\n    float g;\n    int16_t i;\n    \
         bool flag;\n    uint8_t small;\n    E_Team team;\n} T_Mixed;",
    );
    assert_has(&c, "typedef uint8_t E_Team;");
    assert_has(
        &c,
        "typedef struct T_Nothing {\n    char empty;\n} T_Nothing;",
    );
    assert_has(
        &c,
        "    int64_t f_int;\n    int64_t f__x;\n    int64_t f_f_y;\n",
    );
    // Vec2 is defined before the record that holds one by value.
    assert!(c.find("} T_Vec2;").unwrap() < c.find("} T_Named;").unwrap());
    assert_has(
        &c,
        "static T_Vec2 M0_Vec2_add(T_Vec2 self_0, T_Vec2 other_1)",
    );
    assert_has(
        &c,
        "return (T_Vec2){.x = self_0.x + other_1.x, .y = self_0.y + other_1.y};",
    );
    assert_has(&c, "a_0 = M0_Vec2_add(a_0, b_1);");
    assert_seq(
        &c,
        &[
            "static void Drop_T_Named(void *p) {",
            "T_Named *v = p;",
            "mn_str_release(v->name);",
            "mn_list_release(v->tags, &Elem_String);",
            "}",
        ],
    );
    assert_seq(
        &c,
        &[
            "static void Retain_T_Named(void *p) {",
            "T_Named *v = p;",
            "mn_str_retain(v->name);",
            "mn_list_retain(v->tags);",
            "}",
        ],
    );
    assert_has(&c, "mn_sb_bytes(sb, \"Named(name: \", 12);");
    assert_has(&c, "mn_sb_quoted(sb, v->name);");
    assert_has(&c, "mn_sb_bytes(sb, \"Nothing()\", 9);");
    assert_has(
        &c,
        "static bool Eq_T_Vec2(const T_Vec2 *a, const T_Vec2 *b)",
    );
    assert_has(&c, ".small = 200,");
    assert_has(&c, "mn_to_int16(k_5, (MnLoc){src0, 1})");
    assert_lacks(&c, "Drop_T_Vec2");
    assert_lacks(&c, "Drop_T_Mixed");
    let c = format!(
        "{c}_Static_assert(sizeof(T_Mixed) == 24, \"fields and tail padding\");\n\
         _Static_assert(sizeof(T_Vec2) == 16 && sizeof(T_Nothing) == 1, \"no header\");\n"
    );
    expect_output(
        "records",
        &c,
        "Named(name: \"x\", tags: [\"t\"], pos: Vec2(x: 4.0, y: 6.0), team: Blue)\n\
         Named(name: \"y!\", tags: [\"t\", \"z\\n\"], pos: Vec2(x: 4.0, y: 6.0), team: Blue)\n\
         false\ntrue\nBlue\n5.0\n7.0\nx?\n\
         Mixed(flag: true, small: 201, f: 1.0, team: Red, i: -15, g: 0.20000000298023224)\n\
         Nothing()\ntrue\nOdd(int: 1, _x: 2, f_y: 3)\n6\n\
         [Named(name: \"y!+\", tags: [\"t\"], pos: Vec2(x: 4.0, y: 6.0), team: Blue), \
         Named(name: \"y!#\", tags: [\"t\", \"z\\n\"], pos: Vec2(x: 4.0, y: 6.0), team: Blue)]\n\
         false\ntrue\n",
    );
}

// ---- Queries ----

/// Components shared by the query tests.
struct World {
    p: Program,
    health: ComponentId,
    health_ty: Type,
    named: ComponentId,
    named_ty: Type,
    frozen: ComponentId,
}

fn world() -> World {
    let mut p = empty_program();
    let (health, health_ty) = component(&mut p, "Health", &[("hp", Type::Int)]);
    let (named, named_ty) = component(
        &mut p,
        "Named",
        &[("name", Type::String), ("log", list_of(&Type::Int))],
    );
    let frozen = tag(&mut p, "Frozen");
    World {
        p,
        health,
        health_ty,
        named,
        named_ty,
        frozen,
    }
}

fn hp(place: &Place) -> Place {
    fld(place, 0, &Type::Int)
}

#[test]
fn a_block_that_calls_a_function_gets_no_restrict_and_no_hoisting() {
    let mut w = world();
    let mut l = Locals::default();
    let amount = l.param("amount", &Type::Int);
    let body = blkv(vec![], bin(BinOp::Mul, rd(&amount), int(2)));
    let double = function(&mut w.p, "double", None, 1, Type::Int, l.body(body));

    let mut l = Locals::default();
    let h = l.view("h", &w.health_ty);
    let stmt = each(
        vec![Some((w.health, true))],
        vec![],
        vec![],
        vec![&h],
        vec![cset(
            &hp(&h),
            BinOp::Add,
            bin(
                BinOp::Add,
                call(double, vec![rd(&hp(&h))], &Type::Int),
                rd(&time(TimeField::Tick)),
            ),
        )],
    );
    system(&mut w.p, "grow", l.body(blk(vec![stmt])));
    let c = emit(&w.p);
    let system = function_text(&c, "static void S_grow(void)");
    assert_has(system, "T_Health *c0 = mn_column(table, 0);");
    assert_lacks(system, "restrict");
    assert_lacks(system, "time_tick");
    assert_seq(
        system,
        &[
            "int64_t t0 = c0[i].hp;",
            "int64_t t1 = F_double(c0[i].hp);",
            "c0[i].hp = t0 + (t1 + mn_time.tick);",
        ],
    );
    check("no_restrict", &c);
}

#[test]
fn break_next_and_return_leave_an_each_cleanly() {
    let mut w = world();
    let mut l = Locals::default();
    let limit = l.param("limit", &Type::Int);
    let e = l.var("e", &Type::Entity);
    let h = l.view("h", &w.health_ty);
    let label = l.var("label", &Type::String);
    let other = l.view("other", &w.health_ty);
    let inner = each(
        vec![Some((w.health, false))],
        vec![w.frozen],
        vec![],
        vec![&other],
        vec![when(
            bin(BinOp::Gt, rd(&hp(&other)), rd(&hp(&h))),
            vec![ret(Some(bi(
                Builtin::StrLength,
                vec![rd(&label)],
                Type::Int,
            )))],
        )],
    );
    let outer = each(
        vec![None, Some((w.health, false))],
        vec![],
        vec![],
        vec![&e, &h],
        vec![
            let_(&label, bin(BinOp::Add, text("hp"), to_s(rd(&hp(&h))))),
            when(
                bin(BinOp::Lt, rd(&hp(&h)), int(0)),
                vec![st(StmtKind::Next)],
            ),
            when(
                bin(BinOp::Eq, rd(&hp(&h)), int(0)),
                vec![st(StmtKind::Break)],
            ),
            when(
                bin(BinOp::Gt, rd(&hp(&h)), rd(&limit)),
                vec![ret(Some(rd(&hp(&h))))],
            ),
            when(
                ex(
                    ExprKind::Has {
                        entity: Box::new(rd(&e)),
                        component: w.frozen,
                    },
                    Type::Bool,
                ),
                vec![es(ex(ExprKind::Despawn(Box::new(rd(&e))), Type::Void))],
            ),
            inner,
        ],
    );
    let body = blkv(vec![outer], int(-1));
    let find = function(&mut w.p, "find", None, 1, Type::Int, l.body(body));

    let health_of = |value: i64| Some(make(&w.health_ty, vec![int(value)]));
    w.p.main = Locals::default().body(blk(vec![
        puts(vec![to_s(call(find, vec![int(5)], &Type::Int))]),
        es(spawn(vec![(w.health, health_of(-3))])),
        es(spawn(vec![(w.health, health_of(4)), (w.frozen, None)])),
        es(spawn(vec![(w.health, health_of(3))])),
        puts(vec![to_s(call(find, vec![int(5)], &Type::Int))]),
        puts(vec![to_s(ex(ExprKind::WorldCount(None), Type::Int))]),
        es(spawn(vec![(w.health, health_of(9))])),
        puts(vec![to_s(call(find, vec![int(5)], &Type::Int))]),
        es(spawn(vec![(w.health, health_of(0))])),
        puts(vec![to_s(call(find, vec![int(50)], &Type::Int))]),
    ]));
    let c = emit(&w.p);
    let find = function_text(&c, "static int64_t F_find(int64_t limit_0)");
    assert_seq(find, &["mn_str_release(label_3);", "continue;"]);
    assert_seq(find, &["mn_str_release(label_3);", "goto brk0;"]);
    assert_seq(
        find,
        &[
            "int64_t t1 = c1[i].hp;",
            "mn_str_release(label_3);",
            "mn_defer_leave();",
            "return t1;",
        ],
    );
    // A return from the inner loop leaves both.
    assert_seq(
        find,
        &[
            "int64_t t2 = mn_str_length(label_3);",
            "mn_defer_leave();",
            "mn_str_release(label_3);",
            "mn_defer_leave();",
            "return t2;",
        ],
    );
    assert_seq(find, &["brk0:", "mn_defer_leave();", "}", "return -1;"]);
    assert_has(find, "const T_Health *cb0 = mn_column(tableb, 0);");
    assert_has(find, "if (cb0[ib].hp > c1[i].hp) {");
    expect_output("each_exits", &c, "-1\n3\n3\n3\n3\n");
}

#[test]
fn entities_and_their_components() {
    let mut w = world();
    let mut l = Locals::default();
    let e = l.var("e", &Type::Entity);
    let f = l.var("f", &Type::Entity);
    let copy = l.var("copy", &w.health_ty);
    let s = l.var("s", &Type::String);
    let unused = l.var("unused", &Type::Entity);
    let i = l.var("i", &Type::Int);
    let health_of = |value: Expr| Some(make(&w.health_ty, vec![value]));
    let named_of = |name: Expr| {
        Some(make(
            &w.named_ty,
            vec![name, list(&Type::Int, vec![int(1)])],
        ))
    };
    let health = |entity: &Place, write: bool| of(rd(entity), w.health, &w.health_ty, write);
    let named = |entity: &Place, write: bool| of(rd(entity), w.named, &w.named_ty, write);
    let name = |entity: &Place, write: bool| fld(&named(entity, write), 0, &Type::String);
    let log = |entity: &Place, write: bool| fld(&named(entity, write), 1, &list_of(&Type::Int));
    let has = |entity: &Place, component: ComponentId| {
        to_s(ex(
            ExprKind::Has {
                entity: Box::new(rd(entity)),
                component,
            },
            Type::Bool,
        ))
    };
    let alive = |entity: &Place| to_s(ex(ExprKind::Alive(Box::new(rd(entity))), Type::Bool));
    let count = |terms: Vec<Option<(ComponentId, bool)>>, without: Vec<ComponentId>| {
        to_s(ex(
            ExprKind::WorldCount(Some(query(terms, vec![], without))),
            Type::Int,
        ))
    };
    w.p.main = l.body(blk(vec![
        let_(
            &e,
            spawn(vec![
                (w.health, health_of(int(10))),
                (w.named, named_of(bin(BinOp::Add, text("a"), text("b")))),
            ]),
        ),
        let_(&f, spawn(vec![(w.frozen, None)])),
        let_(&unused, spawn(vec![])),
        cset(&hp(&health(&e, true)), BinOp::Sub, int(3)),
        let_(&copy, rd(&health(&e, false))),
        cset(&hp(&copy), BinOp::Mul, int(2)),
        set(&health(&e, true), make(&w.health_ty, vec![rd(&hp(&copy))])),
        let_(&s, rd(&name(&e, false))),
        set(&name(&e, true), bin(BinOp::Add, rd(&s), text("c"))),
        cset(&name(&e, true), BinOp::Add, rd(&name(&e, false))),
        es(push(&log(&e, true), rd(&hp(&health(&e, false))))),
        set(&at(&log(&e, true), int(0)), int(5)),
        times(
            Some(&i),
            int(2),
            vec![cset(&at(&log(&e, true), rd(&i)), BinOp::Add, rd(&i))],
        ),
        puts(vec![
            to_s(rd(&health(&e, false))),
            to_s(rd(&named(&e, false))),
            rd(&s),
        ]),
        es(ex(
            ExprKind::Add {
                entity: Box::new(rd(&f)),
                components: inits_of(vec![
                    (w.health, health_of(int(1))),
                    (w.named, named_of(rd(&s))),
                ]),
            },
            Type::Void,
        )),
        puts(vec![
            has(&f, w.health),
            has(&f, w.frozen),
            count(vec![Some((w.health, false))], vec![]),
            count(vec![Some((w.health, false))], vec![w.frozen]),
        ]),
        es(ex(
            ExprKind::Remove {
                entity: Box::new(rd(&f)),
                components: vec![w.frozen, w.named],
            },
            Type::Void,
        )),
        puts(vec![has(&f, w.frozen), has(&f, w.named), alive(&f)]),
        es(ex(ExprKind::Despawn(Box::new(rd(&f))), Type::Void)),
        puts(vec![
            alive(&f),
            to_s(ex(ExprKind::WorldCount(None), Type::Int)),
            to_s(bin(BinOp::Eq, rd(&e), rd(&f))),
            to_s(bin(
                BinOp::Ne,
                rd(&f),
                ex(ExprKind::EntityNone, Type::Entity),
            )),
            to_s(ex(ExprKind::EntityNone, Type::Entity)),
        ]),
    ]));
    let c = emit(&w.p);
    assert_seq(
        &c,
        &[
            "MnString t0 = mn_str_concat(((MnString)&str0), ((MnString)&str1));",
            "MnList t1 = mn_list_alloc(&Elem_Int, 1);",
            "int64_t *p2 = mn_list_data(t1);",
            "p2[0] = 1;",
            "T_Named t3 = {.name = t0, .log = t1};",
            "T_Health t4 = (T_Health){.hp = 10};",
            "void *const v5[] = {&t4, &t3};",
            "MnEntity e_0 = mn_spawn(&b0, v5);",
            "MnEntity f_1 = mn_spawn(&b1, NULL);",
        ],
    );
    assert_has(&c, "static MnBundle b0 = {b0_ids, 2, 0, 0, false};");
    assert_has(&c, "static MnBundle b2 = {NULL, 0, 0, 0, false};");
    assert_has(
        &c,
        "MnEntity unused_4 __attribute__((unused)) = mn_spawn(&b2, NULL);",
    );
    assert_seq(
        &c,
        &[
            "T_Health *p6 = mn_get(e_0, 0, sizeof(T_Health), (MnLoc){src0, 1});",
            "p6->hp = p6->hp - 3;",
            "const T_Health *p7 = mn_get(e_0, 0, sizeof(T_Health), (MnLoc){src0, 1});",
            "T_Health copy_2 = *p7;",
        ],
    );
    // A string read out of a component is retained; the old value of a
    // component field is released when it is replaced.
    assert_seq(
        &c,
        &[
            "MnString s_3 = p9->name;",
            "mn_str_retain(s_3);",
            "MnString t10 = mn_str_concat(s_3, ((MnString)&str2));",
            "T_Named *p11 = mn_get(e_0, 1, sizeof(T_Named), (MnLoc){src0, 1});",
            "mn_str_release(p11->name);",
            "p11->name = t10;",
        ],
    );
    assert_has(
        &c,
        "{\"Named\", sizeof(T_Named), _Alignof(T_Named), Drop_T_Named},",
    );
    assert_has(
        &c,
        "{\"Health\", sizeof(T_Health), _Alignof(T_Health), NULL},",
    );
    assert_has(&c, "mn_remove(f_1, ids");
    assert_has(&c, "mn_out_bool(eq_entity(e_0, f_1));");
    expect_output(
        "entities",
        &c,
        "Health(hp: 14)\nNamed(name: \"abcabc\", log: [5, 15])\nab\n\
         true\ntrue\n2\n1\nfalse\nfalse\ntrue\nfalse\n2\nfalse\ntrue\nEntity(none)\n",
    );
}

// ---- Events ----

#[test]
fn events_are_copied_out_one_at_a_time() {
    let mut w = world();
    let (hit, hit_ty) = event(
        &mut w.p,
        "Hit",
        &[("target", Type::Entity), ("damage", Type::Int)],
    );
    let (note, note_ty) = event(&mut w.p, "Note", &[("text", Type::String)]);
    let (pulse, pulse_ty) = event(&mut w.p, "Pulse", &[]);

    // system combat
    let mut l = Locals::default();
    let e = l.var("e", &Type::Entity);
    let h = l.view("h", &w.health_ty);
    let stmt = each(
        vec![None, Some((w.health, false))],
        vec![],
        vec![],
        vec![&e, &h],
        vec![
            st(StmtKind::Emit {
                event: hit,
                value: make(&hit_ty, vec![rd(&e), rd(&hp(&h))]),
            }),
            st(StmtKind::Emit {
                event: note,
                value: make(
                    &note_ty,
                    vec![concat(vec![text("hit "), to_s(rd(&hp(&h)))])],
                ),
            }),
        ],
    );
    let pulse_stmt = st(StmtKind::Emit {
        event: pulse,
        value: make(&pulse_ty, vec![]),
    });
    system(&mut w.p, "combat", l.body(blk(vec![stmt, pulse_stmt])));

    // system damage
    let mut l = Locals::default();
    let got = l.var("hit", &hit_ty);
    let target = rd(&fld(&got, 0, &Type::Entity));
    let damage = rd(&fld(&got, 1, &Type::Int));
    let usable = and(
        ex(ExprKind::Alive(Box::new(target.clone())), Type::Bool),
        ex(
            ExprKind::Has {
                entity: Box::new(target.clone()),
                component: w.health,
            },
            Type::Bool,
        ),
    );
    let stmt = st(StmtKind::On {
        event: hit,
        param: Some(id(&got)),
        body: blk(vec![
            when(not(usable), vec![st(StmtKind::Next)]),
            cset(
                &hp(&of(target.clone(), w.health, &w.health_ty, true)),
                BinOp::Sub,
                damage.clone(),
            ),
            when(bin(BinOp::Gt, damage, int(100)), vec![st(StmtKind::Break)]),
        ]),
    });
    system(&mut w.p, "damage", l.body(blk(vec![stmt])));

    // system journal
    let mut l = Locals::default();
    let got = l.var("note", &note_ty);
    let unread = l.var("ignored", &pulse_ty);
    let on_note = st(StmtKind::On {
        event: note,
        param: Some(id(&got)),
        body: blk(vec![
            when(
                bin(BinOp::Eq, rd(&fld(&got, 0, &Type::String)), text("hit 0")),
                vec![st(StmtKind::Next)],
            ),
            puts(vec![rd(&fld(&got, 0, &Type::String))]),
        ]),
    });
    let on_pulse = st(StmtKind::On {
        event: pulse,
        param: Some(id(&unread)),
        body: blk(vec![puts(vec![text("pulse")])]),
    });
    let on_pulse_again = st(StmtKind::On {
        event: pulse,
        param: None,
        body: blk(vec![puts(vec![text("again")])]),
    });
    system(
        &mut w.p,
        "journal",
        l.body(blk(vec![on_note, on_pulse, on_pulse_again])),
    );

    let mut l = Locals::default();
    let e = l.var("e", &Type::Entity);
    w.p.main = l.body(blk(vec![
        let_(
            &e,
            spawn(vec![(w.health, Some(make(&w.health_ty, vec![int(8)])))]),
        ),
        tick(),
        tick(),
        puts(vec![to_s(rd(&of(rd(&e), w.health, &w.health_ty, false)))]),
    ]));
    let c = emit(&w.p);
    let damage = function_text(&c, "static void S_damage(void)");
    assert_seq(
        damage,
        &["uint64_t end0 = mn_event_end(0);", "uint64_t seq0 = cur"],
    );
    assert_seq(
        damage,
        &[
            "for (; seq0 < end0; seq0++) {",
            "const T_Hit *ev0 = mn_event_at(0, seq0);",
            "if (!ev0) continue;",
            "T_Hit hit_0 = *ev0;",
            "if (!(mn_alive(hit_0.target) && mn_has(hit_0.target, 0))) {",
            "continue;",
            "}",
            "T_Health *p1 = mn_get(hit_0.target, 0, sizeof(T_Health), (MnLoc){src0, 1});",
            "p1->hp = p1->hp - hit_0.damage;",
        ],
    );
    let journal = function_text(&c, "static void S_journal(void)");
    assert_seq(
        journal,
        &[
            "T_Note note_0 = *ev0;",
            "Retain_T_Note(&note_0);",
            "if (mn_str_eq(note_0.text, ((MnString)&str0))) {",
            "Drop_T_Note(&note_0);",
            "continue;",
            "}",
            "mn_out_str(note_0.text);",
            "mn_out_newline();",
            "Drop_T_Note(&note_0);",
            "}",
        ],
    );
    assert_has(journal, "if (!mn_event_at(2, seq");
    assert_has(&c, "*(T_Hit *)mn_emit(0) = t");
    assert_has(
        &c,
        "{\"Note\", sizeof(T_Note), _Alignof(T_Note), Drop_T_Note},",
    );
    assert_has(&c, "{\"Pulse\", sizeof(T_Pulse), _Alignof(T_Pulse), NULL},");
    expect_output(
        "events",
        &c,
        "hit 8\npulse\nagain\npulse\nagain\nHealth(hp: 0)\n",
    );
}

// ---- Blocks and conditions as expressions ----

#[test]
fn blocks_and_conditions_as_expressions() {
    let mut p = empty_program();
    let team = enumeration(&mut p, "Team", &["Red", "Blue", "Green"]);

    let mut l = Locals::default();
    let n = l.param("n", &Type::Int);
    let body = blkv(
        vec![puts(vec![concat(vec![text("check "), to_s(rd(&n))])])],
        bin(BinOp::Gt, rd(&n), int(1)),
    );
    let check_fn = function(&mut p, "big?", None, 1, Type::Bool, l.body(body));

    let mut l = Locals::default();
    let n = l.param("n", &Type::Int);
    let side = l.param("team", &team);
    let subject = l.var("$subject", &Type::Int);
    let word = l.var("word", &Type::String);
    let twice = l.var("twice", &Type::Int);
    let shown = l.var("shown", &Type::Int);
    let both = l.var("both", &Type::Bool);
    let either = l.var("either", &Type::Bool);
    let plain = l.var("plain", &Type::Bool);
    let color = l.var("color", &Type::String);
    let is = |value: i64| bin(BinOp::Eq, rd(&subject), int(value));
    let variant = |index: usize| ex(ExprKind::EnumVariant(index), team.clone());
    let case = ex(
        ExprKind::Block(blkv(
            vec![let_(&subject, bin(BinOp::Mul, rd(&n), int(1)))],
            iff(
                is(1),
                blkv(vec![], text("one")),
                blkv(
                    vec![],
                    iff(
                        is(2),
                        blkv(vec![], bin(BinOp::Add, text("tw"), text("o"))),
                        blkv(vec![], text("many")),
                        Type::String,
                    ),
                ),
                Type::String,
            ),
        )),
        Type::String,
    );
    let big = |value: Expr| call(check_fn, vec![value], &Type::Bool);
    let body = blkv(
        vec![
            let_(&word, case),
            let_(
                &twice,
                iff(
                    bin(BinOp::Gt, rd(&n), int(0)),
                    blkv(vec![], bin(BinOp::Mul, rd(&n), int(2))),
                    blkv(vec![], neg(rd(&n))),
                    Type::Int,
                ),
            ),
            let_(
                &shown,
                iff(
                    bin(BinOp::Gt, rd(&n), int(1)),
                    blkv(vec![puts(vec![text("positive")])], rd(&n)),
                    blkv(vec![], int(0)),
                    Type::Int,
                ),
            ),
            let_(&both, and(bin(BinOp::Gt, rd(&n), int(0)), big(rd(&n)))),
            let_(&either, or(big(rd(&twice)), big(int(5)))),
            let_(
                &plain,
                or(
                    bin(BinOp::Gt, rd(&n), int(0)),
                    bin(BinOp::Lt, rd(&n), int(-5)),
                ),
            ),
            let_(&color, text("")),
            when_else(
                bin(BinOp::Eq, rd(&side), variant(0)),
                vec![set(&color, text("red"))],
                vec![when_else(
                    bin(BinOp::Eq, rd(&side), variant(1)),
                    vec![set(&color, text("blue"))],
                    vec![when(big(int(0)), vec![set(&color, to_s(rd(&side)))])],
                )],
            ),
            when(
                and(rd(&both), not(rd(&either))),
                vec![ret(Some(text("unreachable")))],
            ),
        ],
        concat(vec![
            rd(&word),
            text(" "),
            to_s(rd(&twice)),
            text(" "),
            to_s(rd(&shown)),
            text(" "),
            to_s(rd(&both)),
            to_s(rd(&plain)),
            text(" "),
            rd(&color),
            to_s(iff(
                rd(&plain),
                blkv(vec![], int(1)),
                blkv(vec![], int(2)),
                Type::Int,
            )),
        ]),
    );
    let describe = function(&mut p, "describe", None, 2, Type::String, l.body(body));
    let unused_body = Locals::default().body(blk(vec![]));
    function(&mut p, "never_called!", None, 0, Type::Void, unused_body);

    let show = |n: i64, side: usize| {
        puts(vec![call(
            describe,
            vec![int(n), ex(ExprKind::EnumVariant(side), team.clone())],
            &Type::String,
        )])
    };
    p.main = Locals::default().body(blk(vec![show(1, 0), show(2, 1), show(-7, 2)]));
    let c = emit(&p);
    let describe = function_text(&c, "static MnString F_describe(int64_t n_0, E_Team team_1)");
    assert_seq(
        describe,
        &[
            "MnString t0 = NULL;",
            "{",
            "int64_t h_subject_2 = n_0 * 1;",
            "MnString t1 = NULL;",
            "if (h_subject_2 == 1) {",
            "t1 = ((MnString)&str0);",
            "} else {",
            "MnString t2 = NULL;",
            "if (h_subject_2 == 2) {",
            "t2 = mn_str_concat(((MnString)&str1), ((MnString)&str2));",
            "} else {",
            "t2 = ((MnString)&str3);",
            "}",
            "t1 = t2;",
            "}",
            "t0 = t1;",
            "}",
            "MnString word_3 = t0;",
        ],
    );
    assert_has(describe, "int64_t twice_4 = (n_0 > 0 ? n_0 * 2 : -n_0);");
    assert_seq(
        describe,
        &[
            "int64_t t3 = 0;",
            "if (n_0 > 1) {",
            "mn_out_bytes(\"positive\\n\", 9);",
            "t3 = n_0;",
            "} else {",
            "t3 = 0;",
            "}",
            "int64_t shown_5 = t3;",
        ],
    );
    // The right operand of `&&` runs only when the left one is true.
    assert_seq(
        describe,
        &[
            "bool t4 = n_0 > 0;",
            "if (t4) {",
            "t4 = Fq_big(n_0);",
            "}",
            "bool both_6 = t4;",
        ],
    );
    assert_seq(
        describe,
        &[
            "bool t5 = Fq_big(twice_4);",
            "if (!t5) {",
            "t5 = Fq_big(5);",
            "}",
        ],
    );
    assert_has(describe, "bool plain_8 = n_0 > 0 || n_0 < -5;");
    assert_seq(
        describe,
        &[
            "if (team_1 == 0) {",
            "mn_str_release(color_9);",
            "color_9 = ((MnString)&str",
        ],
    );
    assert_has(describe, "} else if (team_1 == 1) {");
    assert_has(describe, "(plain_8 ? (int64_t)1 : 2)");
    assert_has(
        &c,
        "static void Fb_never_called(void) __attribute__((unused));",
    );
    assert_has(&c, "static bool Fq_big(int64_t n_0);");
    expect_output(
        "expressions",
        &c,
        "check 1\ncheck 2\none 2 0 falsetrue red1\n\
         positive\ncheck 2\ncheck 4\ntwo 4 2 truetrue blue1\n\
         check 7\ncheck 0\nmany 7 0 falsetrue 1\n",
    );
}

// ---- Constants, resources and time ----

#[test]
fn globals_are_set_up_once_and_released() {
    let mut p = empty_program();
    let vec2 = structure(&mut p, "Vec2", &[("x", Type::Float), ("y", Type::Float)]);
    let (position, position_ty) = component(&mut p, "Position", &[("y", Type::Float)]);
    let gravity = constant(&mut p, "GRAVITY", flt(9.81));
    let size = constant(&mut p, "SIZE", bin(BinOp::Mul, int(4), int(8)));
    let half = constant(&mut p, "HALF", bin(BinOp::Div, to_f(rd(&size)), flt(64.0)));
    let title = constant(&mut p, "TITLE", bin(BinOp::Add, text("mid"), text("night")));
    let origin = constant(&mut p, "ORIGIN", make(&vec2, vec![flt(0.0), rd(&half)]));
    let primes = constant(&mut p, "PRIMES", list(&Type::Int, vec![int(2), int(3)]));
    let root = constant(
        &mut p,
        "ROOT",
        bi(Builtin::Sqrt, vec![flt(2.0)], Type::Float),
    );
    let score = resource(
        &mut p,
        "Score",
        vec![
            ("points", Type::Int, int(0)),
            ("best", Type::Int, rd(&size)),
            ("title", Type::String, text("none")),
            ("history", list_of(&Type::Int), list(&Type::Int, vec![])),
            ("ratio", f32_type(), flt(0.5)),
            ("level", Type::Compact(Compact::UInt8), int(3)),
        ],
    );
    let points = res(score, 0, &Type::Int);
    let best = res(score, 1, &Type::Int);
    let score_title = res(score, 2, &Type::String);
    let history = res(score, 3, &list_of(&Type::Int));
    let ratio = res(score, 4, &f32_type());
    let level = res(score, 5, &Type::Compact(Compact::UInt8));

    let mut l = Locals::default();
    let pos = l.view("pos", &position_ty);
    let fall = cset(
        &fld(&pos, 0, &Type::Float),
        BinOp::Sub,
        bin(
            BinOp::Mul,
            bin(BinOp::Mul, rd(&gravity), rd(&time(TimeField::Dt))),
            bin(
                BinOp::Add,
                bin(BinOp::Mul, rd(&ratio), rd(&root)),
                to_f(bin(BinOp::Add, rd(&at(&history, int(0))), rd(&level))),
            ),
        ),
    );
    let stmt = each(
        vec![Some((position, true))],
        vec![],
        vec![],
        vec![&pos],
        vec![fall, cset(&points, BinOp::Add, int(1))],
    );
    system(&mut p, "fall", l.body(blk(vec![stmt])));

    p.main = Locals::default().body(blk(vec![
        cset(&points, BinOp::Add, int(10)),
        set(&score_title, rd(&title)),
        cset(&score_title, BinOp::Add, text("!")),
        es(push(&history, rd(&points))),
        set(&level, bin(BinOp::Add, rd(&level), rd(&best))),
        cset(&ratio, BinOp::Mul, flt(0.5)),
        es(spawn(vec![(
            position,
            Some(make(&position_ty, vec![flt(100.0)])),
        )])),
        set(&time(TimeField::Dt), flt(1.0)),
        tick(),
        puts(vec![
            to_s(rd(&time(TimeField::Tick))),
            to_s(rd(&time(TimeField::Elapsed))),
            to_s(rd(&origin)),
            to_s(rd(&primes)),
            rd(&score_title),
            to_s(rd(&points)),
            to_s(rd(&history)),
            to_s(rd(&level)),
            to_s(rd(&ratio)),
            to_s(bin(BinOp::Add, rd(&size), int(1))),
        ]),
    ]));
    let c = emit(&p);
    // Constants with a known value are written where they are used.
    assert_lacks(&c, "C_GRAVITY");
    assert_lacks(&c, "C_SIZE");
    assert_lacks(&c, "C_HALF");
    assert_has(&c, "static MnString C_TITLE;");
    assert_has(&c, "static T_Vec2 C_ORIGIN;");
    assert_seq(
        &c,
        &[
            "static struct {",
            "int64_t points;",
            "int64_t best;",
            "MnString title;",
            "MnList history;",
            "float ratio;",
            "uint8_t level;",
            "} R_Score;",
        ],
    );
    let init = function_text(&c, "static void init_globals(void)");
    assert_seq(
        init,
        &[
            "C_TITLE = mn_str_concat(((MnString)&str1), ((MnString)&str2));",
            "C_ORIGIN = (T_Vec2){.x = 0.0, .y = 0.5};",
        ],
    );
    assert_seq(
        init,
        &[
            "C_ROOT = sqrt(2.0);",
            "R_Score.points = 0;",
            "R_Score.best = 32;",
            "R_Score.title = ((MnString)&str3);",
            "R_Score.history = NULL;",
            "R_Score.ratio = (float)0.5;",
            "R_Score.level = 3;",
        ],
    );
    let drop = function_text(&c, "static void drop_globals(void)");
    assert_seq(
        drop,
        &[
            "mn_str_release(C_TITLE);",
            "mn_list_release(C_PRIMES, &Elem_Int);",
            "mn_str_release(R_Score.title);",
            "mn_list_release(R_Score.history, &Elem_Int);",
        ],
    );
    // Fields the loop only reads are loaded before it; the one it writes
    // is not.
    let fall = function_text(&c, "static void S_fall(void)");
    assert_seq(
        fall,
        &[
            "mn_defer_enter();",
            "const double time_dt = mn_time.dt;",
            "const float R0_ratio = R_Score.ratio;",
            "const MnList R0_history = R_Score.history;",
            "const uint8_t R0_level = R_Score.level;",
            "for (uint32_t t = 0; t < q0.table_count; t++) {",
        ],
    );
    assert_has(fall, "T_Position *restrict c0 = mn_column(table, 0);");
    assert_has(
        fall,
        "9.81 * time_dt * ((double)R0_ratio * C_ROOT + (double)(",
    );
    assert_has(fall, "R_Score.points = R_Score.points + 1;");
    assert_has(
        &c,
        "mn_to_uint8((int64_t)R_Score.level + R_Score.best, (MnLoc){src0, 1})",
    );
    assert_has(&c, "mn_time.dt = 1.0;");
    expect_output(
        "globals",
        &c,
        "1\n1.0\nVec2(x: 0.0, y: 0.5)\n[2, 3]\nmidnight!\n11\n[10]\n35\n0.25\n33\n",
    );
}

// ---- Loops and the rest of the built-ins ----

#[test]
fn loops_and_world_control() {
    let mut p = empty_program();
    let (counter, counter_ty) = component(&mut p, "Counter", &[("n", Type::Int)]);

    let mut l = Locals::default();
    let c0 = l.view("c", &counter_ty);
    let stmt = each(
        vec![Some((counter, true))],
        vec![],
        vec![],
        vec![&c0],
        vec![
            cset(&fld(&c0, 0, &Type::Int), BinOp::Add, int(1)),
            when(
                bin(BinOp::Ge, rd(&fld(&c0, 0, &Type::Int)), int(3)),
                vec![es(ex(ExprKind::WorldStop, Type::Void))],
            ),
        ],
    );
    system(&mut p, "count", l.body(blk(vec![stmt])));

    // def collatz(n: Int) -> Int, with a `loop do` that only returns
    let mut l = Locals::default();
    let n = l.param("n", &Type::Int);
    let steps = l.var("steps", &Type::Int);
    let m = l.var("m", &Type::Int);
    let body = blk(vec![
        let_(&steps, int(0)),
        let_(&m, rd(&n)),
        while_(
            boolean(true),
            vec![
                when(bin(BinOp::Le, rd(&m), int(1)), vec![ret(Some(rd(&steps)))]),
                set(
                    &m,
                    iff(
                        bin(BinOp::Eq, bin(BinOp::Mod, rd(&m), int(2)), int(0)),
                        blkv(vec![], bin(BinOp::Div, rd(&m), int(2))),
                        blkv(
                            vec![],
                            bin(BinOp::Add, bin(BinOp::Mul, rd(&m), int(3)), int(1)),
                        ),
                        Type::Int,
                    ),
                ),
                cset(&steps, BinOp::Add, int(1)),
            ],
        ),
    ]);
    let collatz = function(&mut p, "collatz", None, 1, Type::Int, l.body(body));

    let mut l = Locals::default();
    let i = l.var("i", &Type::Int);
    let j = l.var("j", &Type::Int);
    let total = l.var("total", &Type::Int);
    let r = l.var("r", &Type::Float);
    let k = l.var("k", &Type::Int);
    let started = l.var("started", &Type::Float);
    let range = |var: &Place, from: Expr, to: Expr, down: bool, body: Vec<Stmt>| {
        st(StmtKind::Count {
            var: Some(id(var)),
            from,
            to,
            inclusive: true,
            down,
            body: blk(body),
        })
    };
    p.main = l.body(blk(vec![
        let_(&total, int(0)),
        times(
            Some(&i),
            int(4),
            vec![
                when(bin(BinOp::Eq, rd(&i), int(1)), vec![st(StmtKind::Next)]),
                cset(&total, BinOp::Add, rd(&i)),
            ],
        ),
        range(
            &j,
            int(1),
            call(collatz, vec![int(6)], &Type::Int),
            false,
            vec![
                when(bin(BinOp::Gt, rd(&j), int(3)), vec![st(StmtKind::Break)]),
                cset(&total, BinOp::Mul, rd(&j)),
            ],
        ),
        range(
            &j,
            int(3),
            int(1),
            true,
            vec![st(StmtKind::Print(vec![to_s(rd(&j))]))],
        ),
        puts(vec![]),
        while_(
            bin(
                BinOp::Lt,
                call(collatz, vec![rd(&total)], &Type::Int),
                int(100),
            ),
            vec![cset(&total, BinOp::Add, int(1))],
        ),
        puts(vec![to_s(rd(&total))]),
        es(bi(Builtin::Srand, vec![int(42)], Type::Void)),
        let_(&r, bi(Builtin::Rand, vec![], Type::Float)),
        let_(&k, bi(Builtin::RandInt, vec![int(10)], Type::Int)),
        puts(vec![to_s(and(
            and(
                bin(BinOp::Ge, rd(&r), flt(0.0)),
                bin(BinOp::Lt, rd(&r), flt(1.0)),
            ),
            and(
                bin(BinOp::Ge, rd(&k), int(0)),
                bin(BinOp::Lt, rd(&k), int(10)),
            ),
        ))]),
        let_(&started, bi(Builtin::Clock, vec![], Type::Float)),
        es(bi(Builtin::Sleep, vec![flt(0.0)], Type::Void)),
        puts(vec![to_s(bin(
            BinOp::Ge,
            bi(Builtin::Clock, vec![], Type::Float),
            rd(&started),
        ))]),
        es(spawn(vec![(
            counter,
            Some(make(&counter_ty, vec![int(0)])),
        )])),
        es(ex(ExprKind::WorldRun, Type::Void)),
        puts(vec![to_s(rd(&time(TimeField::Tick)))]),
        when(
            bin(BinOp::Eq, rd(&time(TimeField::Tick)), int(3)),
            vec![ret(None)],
        ),
        es(bi(Builtin::SrandClock, vec![], Type::Void)),
        es(bi(Builtin::Exit, vec![int(3)], Type::Void)),
    ]));
    let c = emit(&p);
    let collatz = function_text(&c, "static int64_t F_collatz(int64_t n_0)");
    assert_has(collatz, "while (true) {");
    assert_seq(
        collatz,
        &[
            "int64_t t0 = mn_int_mod(m_2, 2, (MnLoc){src0, 1});",
            "int64_t t1 = 0;",
            "if (t0 == 0) {",
            "t1 = mn_int_div(m_2, 2, (MnLoc){src0, 1});",
            "} else {",
            "t1 = m_2 * 3 + 1;",
            "}",
            "m_2 = t1;",
        ],
    );
    let main = function_text(&c, "static void run_main(void)");
    assert_has(main, "for (int64_t i_0 = 0; i_0 < 4; i_0++) {");
    assert_seq(
        main,
        &[
            "int64_t t0 = F_collatz(6);",
            "for (int64_t j_1 = 1; j_1 <= t0; j_1++) {",
        ],
    );
    assert_has(main, "for (int64_t j_1 = 3; j_1 >= 1; j_1--) {");
    assert_seq(
        main,
        &[
            "while (1) {",
            "int64_t t1 = F_collatz(total_2);",
            "if (!(t1 < 100)) break;",
            "total_2 = total_2 + 1;",
            "}",
        ],
    );
    assert_has(main, "mn_run((MnLoc){src0, 1});");
    assert_has(main, "mn_exit(3);");
    assert_has(&c, "mn_stop();");
    expect_output("loops", &c, "321\n31\ntrue\ntrue\n3\n");
}

// ---- Locations ----

#[test]
fn statements_carry_their_source_lines() {
    let mut p = empty_program();
    let line = |number: usize| {
        let start = SOURCE
            .split_inclusive('\n')
            .take(number - 1)
            .map(str::len)
            .sum::<usize>() as u32;
        Span::new(FileId(0), start, start + 1)
    };
    let mut l = Locals::default();
    let n = l.var("n", &Type::Int);
    let mut divide = bin(BinOp::Div, int(7), rd(&n));
    divide.span = line(3);
    let mut first = let_(&n, int(2));
    first.span = line(2);
    let mut second = puts(vec![to_s(divide)]);
    second.span = line(3);
    let mut third = puts(vec![]);
    third.span = line(4);
    p.main = l.body(blk(vec![first, second, third]));
    let c = emit(&p);
    assert_has(&c, "static const char src0[] = \"test.12am\";");
    assert_has(
        &c,
        "#line 2 \"test.12am\"\n    int64_t n_0 = 2;\n#line 3 \"test.12am\"\n    \
         int64_t t0 = mn_int_div(7, n_0, (MnLoc){src0, 3});\n",
    );
    assert_has(&c, "#line 4 \"test.12am\"\n    mn_out_newline();\n");
    // After the program's code, lines are numbered as in the C file.
    let position = c.find("static void init_globals").expect("init_globals");
    let directive = c[..position].lines().last().expect("a directive");
    let number = c[..position].lines().count() + 1;
    assert_eq!(directive, format!("#line {number} \"program.c\""));
    expect_output("lines", &c, "3\n\n");
}
