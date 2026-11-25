//! GPU blocks: the parts of their emission that are easy to get wrong.
//! Programs are parsed from source text; `tests/run/gpu_*.12am` and
//! `tests/panic/gpu_*.12am` run whole programs.

use std::path::PathBuf;

use crate::emit::Cx;
use crate::emit::gpu::ir::{BinOp, Expr, Storage, Ty, choose, compare, float_arith, int_arith};
use crate::emit::gpu::layout::{Layouts, Source};
use crate::emit::gpu::{Plan, lower, wgsl};
use crate::source::SourceMap;
use crate::tir::Program;

use super::{assert_has, assert_lacks, assert_seq, check, function_text};

fn compile(text: &str) -> (Program, SourceMap) {
    let mut sources = SourceMap::new();
    let file = sources.add(
        "test.12am".to_string(),
        PathBuf::from("test.12am"),
        text.to_string(),
    );
    let items = crate::parser::parse(file, text)
        .unwrap_or_else(|error| panic!("syntax error in a test: {}\n{text}", error.message));
    let program =
        crate::check::check(&crate::ast::Program { items }, &sources).unwrap_or_else(|errors| {
            panic!(
                "errors in a test:\n{}",
                crate::diag::render(&sources, &errors, false)
            )
        });
    (program, sources)
}

/// The generated C, checked by the C compiler when there is one.
#[track_caller]
fn emit_c(name: &str, text: &str) -> String {
    let (program, sources) = compile(text);
    let c = crate::emit::emit(&program, &sources);
    check(name, &c);
    c
}

/// The shader of each block.
fn shaders(text: &str) -> Vec<String> {
    let (program, sources) = compile(text);
    let cx = Cx::new(&program, &sources);
    let lowered = lower(&program, &cx.folded, cx.gpu.wide.clone());
    lowered
        .blocks
        .iter()
        .map(|block| wgsl::module(&lowered, block))
        .collect()
}

const PACKED: &str = "\
enum Mode
  Idle
  Busy
end

struct Pair
  lo: Int8
  hi: UInt16
end

component Packed
  f: Float32
  i8: Int8
  i16: Int16
  i32: Int32
  u8: UInt8
  u16: UInt16
  flag: Bool
  mode: Mode
  friend: Entity
  pair: Pair
end

gpu system touch
  each Packed do |p|
    copy = p
    p = copy
  end
end
";

/// Members are ordered by alignment, largest first, and keep their source
/// order within one alignment: `f`, `i32` and `friend`; then `i16`, `u16`
/// and `pair`; then the single bytes.
#[test]
fn field_offsets_follow_the_c_layout() {
    let (program, _) = compile(PACKED);
    let plan = Plan::new(&program);
    let layouts = Layouts {
        prog: &program,
        wide: &plan.wide,
    };
    let packed = program
        .records
        .iter()
        .position(|record| record.name == "Packed")
        .expect("the component's record");
    assert_eq!(layouts.record(packed).size, 28);
    let leaves: Vec<(String, u32, Storage)> = layouts
        .leaves(packed)
        .into_iter()
        .map(|leaf| (leaf.member, leaf.offset, leaf.storage))
        .collect();
    let expected = [
        (".f", 0, Storage::F32),
        (".i8", 24, Storage::I8),
        (".i16", 16, Storage::I16),
        (".i32", 4, Storage::I32),
        (".u8", 25, Storage::U8),
        (".u16", 18, Storage::U16),
        (".flag", 26, Storage::Bool),
        (".mode", 27, Storage::Enum(1)),
        (".friend", 8, Storage::Entity),
        (".pair.lo", 22, Storage::I8),
        (".pair.hi", 20, Storage::U16),
    ];
    let expected: Vec<(String, u32, Storage)> = expected
        .iter()
        .map(|(member, offset, storage)| (member.to_string(), *offset, *storage))
        .collect();
    assert_eq!(leaves, expected);
}

/// The C compiler confirms the offsets the shader was generated with.
#[test]
fn offsets_are_asserted_against_the_structs() {
    let c = emit_c("gpu_offsets", PACKED);
    assert_has(
        &c,
        "_Static_assert(sizeof(T_Packed) == 28, \"size of Packed\");",
    );
    assert_has(
        &c,
        "_Static_assert(offsetof(T_Packed, pair) == 20, \"offset of Packed.pair\");",
    );
    assert_has(
        &c,
        "_Static_assert(offsetof(T_Pair, lo) == 2, \"offset of Pair.lo\");",
    );
}

#[test]
fn fields_are_read_and_written_by_shifting_words() {
    let shader = &shaders(PACKED)[0];
    // One byte at offset 25: the second byte of word 6.
    assert_has(shader, "var copy_1: G_Packed = p_0;");
    assert_has(shader, "p_0.f_u8 = i32((c0[row * 7u + 6u] >> 8u) & 0xffu);");
    assert_has(
        shader,
        "c0[row * 7u + 6u] = (c0[row * 7u + 6u] & 0xffff00ffu) | \
         ((bitcast<u32>(p_0.f_u8) & 0xffu) << 8u);",
    );
    // A signed byte in a struct at offset 22: sign-extended.
    assert_has(
        shader,
        "p_0.f_pair.f_lo = (i32((c0[row * 7u + 5u] >> 16u) << 24u) >> 24u);",
    );
    // An entity is two words.
    assert_has(
        shader,
        "p_0.f_friend = MnEntity(c0[row * 7u + 2u], c0[row * 7u + 3u]);",
    );
    assert_has(shader, "c0[row * 7u + 3u] = p_0.f_friend.generation;");
}

const HEAL: &str = "\
component Health
  hp: UInt8
end

component Armor
  rating: UInt8
  bonus: Int8
  worn: Bool
end

component Plain
  n: UInt8
end

gpu system heal
  each Health, Armor do |health, armor|
    health.hp = armor.rating
  end
end

system count
  each Plain do |plain|
    plain.n += 1
  end
end
";

/// A component a GPU block writes is aligned to 4 bytes, so each row is
/// whole words; one it only reads, and one it does not name, are not.
#[test]
fn a_written_component_is_whole_words() {
    let c = emit_c("gpu_whole_words", HEAL);
    assert_seq(
        &c,
        &[
            "typedef struct T_Health {",
            "_Alignas(4) uint8_t hp;",
            "} T_Health;",
        ],
    );
    assert_seq(&c, &["typedef struct T_Armor {", "uint8_t rating;"]);
    assert_seq(&c, &["typedef struct T_Plain {", "uint8_t n;"]);
    assert_has(
        &c,
        "_Static_assert(sizeof(T_Health) == 4, \"size of Health\");",
    );
    assert_has(
        &c,
        "_Static_assert(sizeof(T_Armor) == 3, \"size of Armor\");",
    );
    assert_has(&c, "{0, sizeof(T_Health), true, true},");
    assert_has(&c, "{1, sizeof(T_Armor), false, false},");
}

/// In a column with rows of 3 bytes, the word and the shift of a field
/// depend on the row.
#[test]
fn a_read_only_column_can_have_rows_of_any_size() {
    let shader = &shaders(HEAL)[0];
    assert_has(shader, "var<storage, read_write> c0: array<u32>;");
    assert_has(shader, "var<storage, read> c1: array<u32>;");
    assert_has(
        shader,
        "armor_1.f_rating = i32((c1[(row * 3u) >> 2u] >> (((row * 3u) & 3u) << 3u)) & 0xffu);",
    );
    assert_has(
        shader,
        "c0[row] = (c0[row] & 0xffffff00u) | (bitcast<u32>(health_0.f_hp) & 0xffu);",
    );
    // Only the field the block reads is loaded.
    assert_lacks(shader, "f_bonus =");
}

const UNIFORMS: &str = "\
enum Dir
  North
  South
end

struct Vec2
  x: Float
  y: Float
end

resource Wind
  speed: Float = 1.0
  origin: Vec2 = Vec2(1.0, 2.0)
  blowing: Bool = true
  dir: Dir = Dir::South
  leader: Entity = Entity.none
  gusts: UInt8 = 3
end

LIMIT = 6
ROOT = Math.sqrt(16.0)

component Point
  x: Float32
  n: Int32
end

def drift -> Float
  Wind.origin.y
end

gpu system first
  each Point do |point|
    point.x += ROOT + drift + LIMIT
  end
end

gpu system second
  each Entity, Point do |me, point|
    point.x += Wind.speed
    point.n += Wind.gusts if Wind.blowing && Wind.dir == Dir::South && me != Wind.leader
  end
end
";

/// Words are given out in the order the lowering first reads a source:
/// the blocks in order, then the functions they call. A source takes all
/// its words even when one field is read. A constant the compiler knows
/// takes none.
#[test]
fn uniform_words_are_laid_out_once_for_the_program() {
    let (program, sources) = compile(UNIFORMS);
    let cx = Cx::new(&program, &sources);
    let lowered = lower(&program, &cx.folded, cx.gpu.wide.clone());
    let resource = |name: &str| {
        let index = program.resources[0]
            .fields
            .iter()
            .position(|field| field.name == name)
            .expect("a field of Wind");
        Source::Resource(0, index)
    };
    let root = program
        .consts
        .iter()
        .position(|item| item.name == "ROOT")
        .expect("the constant");
    let entries: Vec<(Source, u32, u32)> = lowered
        .uniforms
        .entries
        .iter()
        .map(|entry| (entry.source, entry.base, entry.words))
        .collect();
    assert_eq!(
        entries,
        [
            (Source::Const(root), 8, 1),
            (resource("speed"), 9, 1),
            (resource("blowing"), 10, 1),
            (resource("dir"), 11, 1),
            (resource("leader"), 12, 2),
            (resource("gusts"), 14, 1),
            (resource("origin"), 15, 2),
        ]
    );
    assert_eq!(
        lowered.uniforms.words[8..],
        [
            Ty::F32,
            Ty::F32,
            Ty::U32,
            Ty::U32,
            Ty::U32,
            Ty::U32,
            Ty::I32,
            Ty::F32,
            Ty::F32
        ]
    );
    // The first block reads ROOT and, through `drift`, Wind.origin.
    assert_eq!(lowered.blocks[0].entries, [0, 6]);
    assert_eq!(lowered.blocks[0].uniform_words, 17);
    assert_eq!(lowered.blocks[1].entries, [1, 2, 3, 4, 5]);
    assert_eq!(lowered.blocks[1].uniform_words, 15);
}

#[test]
fn a_block_fills_the_words_it_reads() {
    let c = emit_c("gpu_fill", UNIFORMS);
    assert_seq(
        function_text(&c, "static void Gb0_fill(uint32_t *words)"),
        &[
            "words[8] = g32_bits((float)C_ROOT);",
            "words[15] = g32_bits((float)R_Wind.origin.x);",
            "words[16] = g32_bits((float)R_Wind.origin.y);",
            "}",
        ],
    );
    assert_seq(
        function_text(&c, "static void Gb1_fill(uint32_t *words)"),
        &[
            "words[9] = g32_bits((float)R_Wind.speed);",
            "words[10] = (uint32_t)R_Wind.blowing;",
            "words[11] = (uint32_t)R_Wind.dir;",
            "words[12] = R_Wind.leader.index;",
            "words[13] = R_Wind.leader.generation;",
            "words[14] = (uint32_t)R_Wind.gusts;",
            "}",
        ],
    );
    // The shared function reads the same word in C and in the shader.
    assert_has(&c, "return g32_float(g->u[16]);");
    assert_has(&c, "\"    return u.w16;\\n\"");
    // The folded constant is a literal.
    assert_has(&c, "+ 6.0f);");
    assert_has(&c, ".uniform_words = 17,");
    assert_has(&c, ".uniform_words = 15,");
}

const SITES: &str = "\
def late(a: Int, b: Int) -> Int
  a / b
end

component Cell
  n: Int32
  m: UInt8
end

gpu system first
  each Cell do |cell|
    cell.m = cell.n
    cell.n = cell.n % cell.m + 7 / cell.n
    cell.n = late(cell.n, 3) / 4
  end
end
";

/// Sites are numbered by where they are in the source, not by when the
/// lowering met them: the division in `late` is lowered last and is site 0.
/// A division by a constant that is not zero is no site.
#[test]
fn sites_are_in_source_order() {
    let (program, sources) = compile(SITES);
    let cx = Cx::new(&program, &sources);
    let lowered = lower(&program, &cx.folded, cx.gpu.wide.clone());
    let sites: Vec<(u32, &str)> = lowered
        .sites
        .iter()
        .map(|site| (sources.line(site.span), site.message.as_str()))
        .collect();
    assert_eq!(
        sites,
        [
            (2, "division by zero"),
            (12, "value does not fit in UInt8"),
            (13, "modulo by zero"),
            (13, "division by zero"),
        ]
    );
    assert!(lowered.blocks[0].uses.fails);

    let c = crate::emit::emit(&program, &sources);
    check("gpu_sites", &c);
    assert_seq(
        &c,
        &[
            "static const MnGpuSite gpu_sites[] = {",
            "{{src0, 2}, \"division by zero\"},",
            "{{src0, 12}, \"value does not fit in UInt8\"},",
            "{{src0, 13}, \"modulo by zero\"},",
            "{{src0, 13}, \"division by zero\"},",
            "};",
        ],
    );
    assert_has(&c, "return g32_div_checked(g, a_0, b_1, 0u);");
    assert_has(&c, "cell_0.f_m = g32_to_uint8(g, cell_0.f_n, 1u);");
    assert_has(
        &c,
        "g32_mod_checked(g, cell_0.f_n, cell_0.f_m, 2u) + g32_div_checked(g, 7, cell_0.f_n, 3u);",
    );
    assert_has(&c, "cell_0.f_n = t0 >> 2u;");
    assert_has(&c, ".fallible = true,");
    assert_has(&c, ".sites = gpu_sites,");

    let shader = wgsl::module(&lowered, &lowered.blocks[0]);
    assert_has(&shader, "return g32_div_checked(a_0, b_1, 0u);");
    assert_has(&shader, "cell_0.f_m = g32_to_uint8(cell_0.f_n, 1u);");
    assert_has(&shader, "var<storage, read_write> g_error: atomic<u32>;");
}

const SHARED: &str = "\
component Body
  x: Float32
end

component Other
  n: Int
end

gpu system drift
  each Body do |body|
    body.x += 1.0
  end
end

system report
  each Body, Other do |body, other|
    other.n += 1 if body.x > 0.0
  end
  each Body do |body|
    body.x = 0.0
  end
end

first = spawn Body(1.0), Other(0)
first[Body].x = 2.0
puts first[Body].x, first[Other].n
";

/// Ordinary code brings the CPU copy of a component up to date before it
/// uses it, when some GPU block names the component, and says whether it
/// writes. Other components are reached as in a program without GPU
/// systems.
#[test]
fn ordinary_code_syncs_what_gpu_blocks_name() {
    let c = emit_c("gpu_sync", SHARED);
    let report = function_text(&c, "static void S_report(void)");
    assert_seq(
        report,
        &[
            "static const MnUse uses0[] = {{0, MN_USE_READ}};",
            "if (mn_gpu_only) mn_gpu_use(&q0, uses0, 1);",
            "for (uint32_t t = 0; t < q0.table_count; t++) {",
            "MnTable *table = q0.tables[t];",
            "uint32_t n = table->count;",
            "if (n == 0) continue;",
            "const T_Body *restrict c0 = mn_column(table, 0);",
            "T_Other *restrict c1 = mn_column(table, 1);",
        ],
    );
    // The second loop overwrites Body: no flag, and the call is made
    // whenever a GPU copy is current, to mark it stale.
    assert_seq(
        report,
        &[
            "mn_defer_enter();",
            "static const MnUse uses1[] = {{0, MN_USE_OVERWRITE}};",
            "if (mn_gpu_active) mn_gpu_use(&q1, uses1, 1);",
        ],
    );
    assert_seq(
        report,
        &[
            "for (uint32_t i = 0; i < n; i++) {",
            "c0[i].x = (float)0.0;",
            "}",
            "}",
            "mn_defer_leave();",
        ],
    );
    assert_lacks(report, "mn_wrote");
    let main = function_text(&c, "static void run_main(void)");
    assert_has(main, "= mn_get_synced(first_0, 0, sizeof(T_Body), true, ");
    assert_has(main, "= mn_get_synced(first_0, 0, sizeof(T_Body), false, ");
    assert_has(main, "= mn_get(first_0, 1, sizeof(T_Other), ");
    assert_seq(
        function_text(&c, "static void S_drift(void)"),
        &[
            "static void S_drift(void) {",
            "mn_gpu_run(&Gb0);",
            "mn_gpu_submit();",
            "}",
        ],
    );
}

/// A GPU system submits its work when an ordinary system follows it, and
/// only then: the next GPU system records into the same submission, and
/// the end of the tick submits after the last system.
#[test]
fn a_gpu_system_submits_before_an_ordinary_system() {
    let c = emit_c(
        "gpu_submit",
        "component Body\n  x: Float32\nend\n\n\
         gpu system a\n  each Body do |body|\n    body.x += 1.0\n  end\nend\n\n\
         gpu system b\n  each Body do |body|\n    body.x += 2.0\n  end\n  each Body do |body|\n    body.x *= 2.0\n  end\nend\n\n\
         system c\n  each Body do |body|\n    puts body.x\n  end\nend\n\n\
         gpu system d\n  each Body do |body|\n    body.x -= 1.0\n  end\nend\n\n\
         spawn Body(1.0)\nWorld.tick\n",
    );
    let a = function_text(&c, "static void S_a(void)");
    assert_lacks(a, "mn_gpu_submit();");
    assert_seq(
        function_text(&c, "static void S_b(void)"),
        &[
            "mn_gpu_run(&Gb1);",
            "mn_gpu_run(&Gb2);",
            "mn_gpu_submit();",
            "}",
        ],
    );
    assert_lacks(
        function_text(&c, "static void S_d(void)"),
        "mn_gpu_submit();",
    );
}

/// A loop left early by `break` or `return` reports the table it was in
/// when the body had assigned there, before the queued changes are
/// applied. A nested loop that assigns through the outer parameter sets the
/// outer loop's flag.
#[test]
fn a_loop_left_early_reports_what_it_wrote() {
    let c = emit_c(
        "gpu_early",
        "component Body\n  x: Float32\nend\n\n\
         gpu system drift\n  each Body do |body|\n    body.x += 1.0\n  end\nend\n\n\
         def first_big -> Float\n  each Body do |body|\n    body.x = 1.0\n    return body.x if body.x > 9.0\n  end\n  0.0\nend\n\n\
         system cap\n  each Body do |body|\n    body.x = 2.0\n    break if body.x > 5.0\n  end\nend\n\n\
         system nest\n  each Body do |body|\n    each Body do |other|\n      body.x += other.x\n    end\n  end\nend\n\n\
         puts first_big\n",
    );
    assert_seq(
        function_text(&c, "static void S_cap(void)"),
        &[
            "if (w_0) mn_wrote(table, 0);",
            "goto brk1;",
            "}",
            "}",
            "if (w_0) { mn_wrote(table, 0); w_0 = false; }",
            "}",
            "brk1:",
            "mn_defer_leave();",
        ],
    );
    assert_seq(
        function_text(&c, "static double F_first_big(void)"),
        &[
            "double t1 = (double)c0[i].x;",
            "if (w_0) mn_wrote(table, 0);",
            "mn_defer_leave();",
            "return t1;",
        ],
    );
    let nest = function_text(&c, "static void S_nest(void)");
    assert_seq(
        nest,
        &[
            "w_0 = true;",
            "c0[i].x = (float)((double)c0[i].x + (double)cb0[ib].x);",
        ],
    );
    assert_seq(
        nest,
        &["}", "if (w_0) { mn_wrote(table, 0); w_0 = false; }"],
    );
    assert_lacks(nest, "wb_0");
}

/// A program without GPU systems gets none of this.
#[test]
fn a_program_without_gpu_systems_is_unchanged() {
    let c = emit_c(
        "gpu_none",
        "component Body\n  x: UInt8\nend\n\nsystem move\n  each Body do |body|\n    body.x += 1\n  end\nend\n\n\
         e = spawn Body(1)\nputs e[Body].x\n",
    );
    for absent in [
        "GPU systems",
        "FP_CONTRACT",
        "Gcx",
        "mn_gpu_use",
        "mn_wrote",
        "mn_get_synced",
        "_Alignas",
        "_Static_assert",
    ] {
        assert_lacks(&c, absent);
    }
}

const GUARDED: &str = "\
component Cell
  n: Int32
  out: Int32
end

gpu system guard
  each Cell do |cell|
    cell.out = cell.n == 0 ? 0 : 60 / cell.n
    cell.out += 1 if cell.n != 0 && 60 / cell.n > 1
    cell.out += cell.n > 5 ? 2 : 3
  end
end
";

/// A shader's `select` evaluates both sides, and a shader compiler may
/// evaluate both sides of `&&`. So an operation that can fail is put under
/// an `if` wherever the source evaluates it conditionally; a side that
/// can't fail is left to the target.
#[test]
fn an_operation_that_can_fail_runs_only_when_the_source_runs_it() {
    let shader = &shaders(GUARDED)[0];
    assert_seq(
        shader,
        &[
            "var t0: i32;",
            "if (cell_0.f_n == 0i) {",
            "t0 = 0i;",
            "} else {",
            "t0 = g32_div_checked(60i, cell_0.f_n, 0u);",
            "}",
            "cell_0.f_out = t0;",
            "var t1: bool = cell_0.f_n != 0i;",
            "if (t1) {",
            "t1 = g32_div_checked(60i, cell_0.f_n, 1u) > 1i;",
            "}",
        ],
    );
    assert_has(
        shader,
        "cell_0.f_out = cell_0.f_out + select(3i, 2i, cell_0.f_n > 5i);",
    );
    let c = emit_c("gpu_guarded", GUARDED);
    assert_has(&c, "t0 = g32_div_checked(g, 60, cell_0.f_n, 0u);");
    assert_has(
        &c,
        "cell_0.f_out = cell_0.f_out + ((cell_0.f_n > 5) ? 2 : 3);",
    );
}

/// A shader compiler rejects a constant expression that overflows, so
/// arithmetic on literals is done here, at 32 bits.
#[test]
fn literals_are_folded_at_32_bits() {
    let int = |value: i32| Expr::I32(value);
    let float = |value: f32| Expr::F32(value);
    assert_eq!(int_arith(BinOp::Add, int(i32::MAX), int(1)), int(i32::MIN));
    assert_eq!(int_arith(BinOp::Mul, int(65536), int(65536)), int(0));
    assert_eq!(
        float_arith(BinOp::Mul, float(1e30), float(1e30)),
        Expr::F32Bits(f32::INFINITY.to_bits())
    );
    assert_eq!(
        float_arith(BinOp::Add, float(0.5), float(0.25)),
        float(0.75)
    );
    assert_eq!(compare(BinOp::Lt, int(1), int(2)), Expr::Bool(true));
    assert_eq!(
        compare(BinOp::Eq, float(0.5), float(0.25)),
        Expr::Bool(false)
    );
    assert_eq!(choose(Expr::Bool(false), int(1), int(2)), int(2));
    // Anything else is left for the target.
    let x = || Expr::Var("x".to_string());
    assert!(matches!(
        int_arith(BinOp::Add, x(), int(1)),
        Expr::Binary(BinOp::Add, ..)
    ));
}

const DIVISORS: &str = "\
W = 64
H = W / 4
ODD = H % 5 + 2 ** 3
NONE = W % 8

component Cell
  n: Int32
  q: Int32
end

gpu system split
  each Cell do |cell|
    cell.q = cell.n / H + cell.n % ODD + cell.n / (W / 8)
  end
end

gpu system by_none
  each Cell do |cell|
    cell.q = cell.n / NONE
  end
end
";

/// A constant made by `/`, `%` or `**` that can't fail is known here, and
/// so is such an operation on two constants in a block. A division by one
/// that is not zero is no site, and a block without sites has no error
/// word. A division by a constant known to be zero is still a site.
#[test]
fn a_division_by_a_known_constant_is_not_checked() {
    let (program, sources) = compile(DIVISORS);
    let cx = Cx::new(&program, &sources);
    let lowered = lower(&program, &cx.folded, cx.gpu.wide.clone());
    assert!(lowered.uniforms.entries.is_empty());
    let sites: Vec<(u32, &str)> = lowered
        .sites
        .iter()
        .map(|site| (sources.line(site.span), site.message.as_str()))
        .collect();
    assert_eq!(sites, [(19, "division by zero")]);
    assert!(!lowered.blocks[0].uses.fails);
    assert!(lowered.blocks[1].uses.fails);

    let split = wgsl::module(&lowered, &lowered.blocks[0]);
    assert_has(
        &split,
        "((cell_0.f_n >> 4u) + g32_mod(cell_0.f_n, 9i)) + (cell_0.f_n >> 3u);",
    );
    assert_lacks(&split, "g_error");
    let by_none = wgsl::module(&lowered, &lowered.blocks[1]);
    assert_has(&by_none, "g32_div_checked(cell_0.f_n, 0i, 0u);");

    let c = emit_c("gpu_divisors", DIVISORS);
    assert_lacks(&c, "C_H");
    assert_seq(
        &c,
        &[".wgsl = Gb0_wgsl,", ".fallible = false,", ".sites = NULL,"],
    );
    assert_seq(
        &c,
        &[
            ".wgsl = Gb1_wgsl,",
            ".fallible = true,",
            ".sites = gpu_sites,",
        ],
    );
}

const STAMPED: &str = "\
struct Stamp
  serial: UInt32 = 0
  weight: Int16 = 0
end

component Cell
  id: UInt32
  x: Float32
  made: Stamp
  seen: Stamp
end

gpu system step
  each Cell do |cell|
    cell.x += 1.0
    cell.seen = cell.made
  end
end
";

#[test]
fn a_uint32_field_of_a_term_is_carried_as_its_bits() {
    let (program, _) = compile(STAMPED);
    let wide = vec![true; program.records.len()];
    let layouts = Layouts {
        prog: &program,
        wide: &wide,
    };
    let cell = program
        .records
        .iter()
        .position(|record| record.name == "Cell")
        .expect("the component's record");
    let unsigned: Vec<String> = layouts
        .leaves(cell)
        .into_iter()
        .filter(|leaf| leaf.storage == Storage::U32)
        .map(|leaf| leaf.member)
        .collect();
    assert_eq!(unsigned, [".id", ".made.serial", ".seen.serial"]);
    assert_eq!(Storage::U32.bytes(), 4);
    assert_eq!(Storage::U32.ty(), Ty::I32);

    // A struct copied whole takes the field along, one word to one word.
    let shader = &shaders(STAMPED)[0];
    assert_has(
        shader,
        "cell_0.f_made.f_serial = bitcast<i32>(c0[row * 6u + 2u]);",
    );
    assert_has(
        shader,
        "c0[row * 6u + 4u] = bitcast<u32>(cell_0.f_seen.f_serial);",
    );
    // The field the block does not name is neither loaded nor stored.
    assert_lacks(shader, "f_id =");
    assert_lacks(shader, "c0[row * 6u] =");

    let c = emit_c("stamped", STAMPED);
    assert_has(&c, "cell_0.f_made.f_serial = (int32_t)c0[row].made.serial;");
    assert_has(
        &c,
        "c0[row].seen.serial = (uint32_t)cell_0.f_seen.f_serial;",
    );
    assert_lacks(&c, "c0[row].id");
}

const OUTSIDE: &str = "\
struct Limits
  low: Int
  high: Int32
  scale: Float
end

resource Budget
  limit: Int = 5
  small: Int16 = 5
end

LIMITS = Limits(1, 2, 0.5)
EDGE = 2 ** 31 - 1

component Cell
  n: Int32
end

gpu system clamp
  each Cell do |cell|
    cell.n = min(cell.n, Budget.limit + Budget.small + LIMITS.high + EDGE)
  end
end
";

/// An `Int` that GPU code reads from a resource field or from a constant
/// the compiler has not worked out is checked against 32 bits where the
/// block's words are filled, with the name the source gives it and the
/// line of the `each`. A field that is 32 bits or fewer is not checked.
#[test]
fn an_int_from_outside_is_checked_when_the_words_are_filled() {
    let c = emit_c("gpu_outside", OUTSIDE);
    assert_seq(
        &c,
        &[
            "static void Gb0_fill(uint32_t *words) {",
            "words[8] = (uint32_t)g32_int(R_Budget.limit, \"Budget.limit\", (MnLoc){src0, 20});",
            "words[9] = (uint32_t)R_Budget.small;",
            "words[10] = (uint32_t)g32_int(C_LIMITS.low, \"LIMITS.low\", (MnLoc){src0, 20});",
            "words[11] = (uint32_t)C_LIMITS.high;",
            "words[12] = g32_bits((float)C_LIMITS.scale);",
        ],
    );
    // The known constant is a literal in the block.
    assert_has(&c, "2147483647");
    assert_lacks(&c, "C_EDGE");
}

const NESTED: &str = "\
component Item
  id: Int32
  seen: Int32
end

gpu system count
  each Item do |me|
    seen = 0
    each Item do |other|
      next if other.id == me.id
      seen += 1
    end
    me.seen = seen
  end
end

gpu system first
  each Item do |me|
    each Item do |other|
      me.seen = other.id
      break
    end
  end
end
";

/// The nested `each` runs eight rows per round of its loop and then the
/// rest, each copy of its body in a `loop` that a `next` leaves with
/// `break`. A body with a `break` keeps the plain loop.
#[test]
fn the_nested_each_runs_eight_rows_per_round() {
    let shaders = shaders(NESTED);
    let rounds = &shaders[0];
    assert_has(rounds, "let j_whole = u.w1 & ~7u;");
    assert_has(rounds, "for (; j < j_whole; ) {");
    assert_eq!(rounds.matches("j = j + 1u;").count(), 8, "{rounds}");
    assert_eq!(rounds.matches("loop {").count(), 8, "{rounds}");
    assert_has(rounds, "for (; j < u.w1; j = j + 1u) {");
    let plain = &shaders[1];
    assert!(!plain.contains("j_whole"), "{plain}");
    assert_has(plain, "for (var j: u32 = 0u; j < u.w1; j = j + 1u) {");
}

const RANGES: &str = "\
LIMIT = 100

component C
  i8: Int8
  u8: UInt8
  i16: Int16
  u16: UInt16
  n: Int32
end

gpu system fit_clamp
  each C do |c|
    c.i8 = (c.n * 3).clamp(-128, 127)
  end
end

gpu system check_clamp
  each C do |c|
    c.i8 = c.n.clamp(-128, 128)
  end
end

gpu system fit_mod
  each C do |c|
    c.u8 = c.n % 256
  end
end

gpu system check_mod
  each C do |c|
    c.u8 = c.n % 257
  end
end

gpu system fit_mod_negative_divisor
  each C do |c|
    c.i8 = c.n % -128
  end
end

gpu system check_mod_negative_divisor
  each C do |c|
    c.u8 = c.n % -2
  end
end

gpu system fit_div
  each C do |c|
    c.i8 = c.i16 / 300
  end
end

gpu system check_div
  each C do |c|
    c.i8 = c.i16 / 200
  end
end

gpu system fit_arithmetic
  each C do |c|
    c.i16 = c.i8 * 256 + c.u8
  end
end

gpu system check_arithmetic
  each C do |c|
    c.i16 = c.i8 * 256 + c.u8 + 1
  end
end

gpu system fit_compound
  each C do |c|
    c.u8 %= 7
  end
end

gpu system check_compound
  each C do |c|
    c.i8 %= 200
  end
end

gpu system fit_min_max_abs
  each C do |c|
    c.u8 = max(min(c.n, 255), 0)
    c.u16 = c.i16.abs
    c.i8 = -(c.n % LIMIT)
  end
end

gpu system check_min
  each C do |c|
    c.u8 = min(c.n, 255)
  end
end

gpu system check_abs
  each C do |c|
    c.i16 = c.i16.abs
  end
end

gpu system fit_rand_and_if
  each C do |c|
    c.u8 = rand(256)
    c.i8 = c.n > 0 ? 127 : -128
  end
end

gpu system check_rand
  each C do |c|
    c.u8 = rand(257)
  end
end

gpu system check_if
  each C do |c|
    c.i8 = c.n > 0 ? 127 : c.u8
  end
end

gpu system check_local
  each C do |c|
    k = c.n % 10
    c.i8 = k
  end
end
";

/// A store into a compact field is checked unless the value's range,
/// worked out from literals, constants, compact fields and the operations
/// on them, fits the field. Each `fit_` block is the case of a rule that
/// fits, each `check_` block the nearest case that does not.
#[test]
fn a_store_whose_range_fits_is_not_checked() {
    let (program, _) = compile(RANGES);
    let shaders = shaders(RANGES);
    for (system, shader) in program.systems.iter().zip(&shaders) {
        let checked = shader.contains("g32_to_");
        if system.name.starts_with("fit_") {
            assert!(!checked, "{}\n{shader}", system.name);
        } else {
            assert!(checked, "{}\n{shader}", system.name);
        }
    }
    assert_eq!(shaders.len(), 19);
}
