//! The functions GPU code calls for what neither target has as an
//! operator: `Int` division that floors, powers, `Float` to `Int`
//! conversions that saturate, range checks and the random numbers. Each is
//! written once, in the intermediate form, and printed for both targets.

use super::ir::{
    BinOp, Expr, Func, Library, MathFn, Param, Stmt, Ty, UnOp, Uses, assign, binary, bits, call,
    cast, field, if_, let_, ret, unary, var,
};
use super::layout::{WORD_BLOCK, WORD_SEED, WORD_TICK};

pub const DIV: &str = "g32_div";
pub const DIV_CHECKED: &str = "g32_div_checked";
pub const MOD: &str = "g32_mod";
pub const MOD_CHECKED: &str = "g32_mod_checked";
pub const POW: &str = "g32_pow";
pub const POW_CHECKED: &str = "g32_pow_checked";
pub const FMOD: &str = "g32_fmod";
pub const FPOW: &str = "g32_fpow";
pub const TO_I: &str = "g32_to_i";
pub const FLOOR: &str = "g32_floor";
pub const CEIL: &str = "g32_ceil";
pub const ROUND: &str = "g32_round";
pub const ROUND_DIGITS: &str = "g32_round_digits";
pub const TO_INT8: &str = "g32_to_int8";
pub const TO_INT16: &str = "g32_to_int16";
pub const TO_UINT8: &str = "g32_to_uint8";
pub const TO_UINT16: &str = "g32_to_uint16";
pub const EQ_ENTITY: &str = "g32_eq_entity";
pub const KEY: &str = "g32_key";
pub const RAND: &str = "g32_rand";
pub const RAND_BELOW: &str = "g32_rand_below";
pub const RAND_INT: &str = "g32_rand_int";

const WHOLE: &str = "g32_whole";
const ROUND_HALF: &str = "g32_round_half";
const POW10: &str = "g32_pow10";
const HASH: &str = "g32_hash";
const DRAW: &str = "g32_draw";

/// `rand` is the top 24 bits of a draw times 2^-24.
const RAND_SCALE: f32 = 1.0 / 16_777_216.0;
const ABS_MASK: u32 = 0x7fff_ffff;
const INFINITY_BITS: u32 = 0x7f80_0000;
const NAN_BITS: u32 = 0x7fc0_0000;
/// The bits of 2^23, from which an `F32` has no fractional part.
const WHOLE_FROM_BITS: u32 = 0x4b00_0000;
/// The largest power of ten an `F32` holds.
const MAX_DIGITS: i32 = 38;

/// Adds a helper, and the helpers it calls, to the library.
pub fn require(library: &mut Library, name: &str) {
    if library.has(name) {
        return;
    }
    let func = define(name);
    for callee in &Uses::of_func(&func).calls {
        require(library, callee);
    }
    library.add(func);
}

/// Whether an `F32` is a NaN, by its bits: a shader compiler may assume
/// that a comparison never sees one.
pub fn is_nan(value: Expr) -> Expr {
    binary(
        BinOp::Gt,
        binary(BinOp::BitAnd, bits(Ty::U32, value), Expr::Hex(ABS_MASK)),
        Expr::Hex(INFINITY_BITS),
    )
}

fn func(name: &str, params: &[(&str, Ty)], ret: Ty, body: Vec<Stmt>) -> Func {
    Func {
        name: name.to_string(),
        params: params
            .iter()
            .map(|(name, ty)| Param {
                name: name.to_string(),
                ty: *ty,
                unused: false,
            })
            .collect(),
        ret,
        body,
    }
}

fn int(value: i32) -> Expr {
    Expr::I32(value)
}

fn uint(value: u32) -> Expr {
    Expr::U32(value)
}

fn float(value: f32) -> Expr {
    Expr::F32(value)
}

fn math(function: MathFn, args: Vec<Expr>) -> Expr {
    Expr::Math(function, args)
}

/// `r` is not zero and its sign differs from `b`'s.
fn signs_differ(r: &str, zero: Expr) -> Expr {
    binary(
        BinOp::And,
        binary(BinOp::Ne, var(r), zero.clone()),
        binary(
            BinOp::Ne,
            binary(BinOp::Lt, var(r), zero.clone()),
            binary(BinOp::Lt, var("b"), zero),
        ),
    )
}

/// `if (failed) { fail(site); return 0; }`
fn fail_if(failed: Expr) -> Stmt {
    if_(failed, vec![Stmt::Fail(var("site")), ret(int(0))])
}

fn define(name: &str) -> Func {
    use BinOp::*;
    let two = [("a", Ty::I32), ("b", Ty::I32)];
    let two_site = [("a", Ty::I32), ("b", Ty::I32), ("site", Ty::U32)];
    let a = || var("a");
    let b = || var("b");
    let v = || var("v");
    match name {
        // Int `/`: rounds toward negative infinity. `b` is not zero.
        DIV => func(
            name,
            &two,
            Ty::I32,
            vec![
                if_(
                    binary(Eq, b(), int(-1)),
                    vec![ret(binary(Sub, int(0), a()))],
                ),
                let_("q", Ty::I32, binary(Div, a(), b())),
                let_("r", Ty::I32, binary(Sub, a(), binary(Mul, var("q"), b()))),
                if_(
                    signs_differ("r", int(0)),
                    vec![ret(binary(Sub, var("q"), int(1)))],
                ),
                ret(var("q")),
            ],
        ),
        DIV_CHECKED => func(
            name,
            &two_site,
            Ty::I32,
            vec![
                fail_if(binary(Eq, b(), int(0))),
                ret(call(DIV, vec![a(), b()])),
            ],
        ),
        // Int `%`: the result has the sign of `b`, which is not zero.
        MOD => func(
            name,
            &two,
            Ty::I32,
            vec![
                if_(binary(Eq, b(), int(-1)), vec![ret(int(0))]),
                let_(
                    "r",
                    Ty::I32,
                    binary(Sub, a(), binary(Mul, binary(Div, a(), b()), b())),
                ),
                if_(
                    signs_differ("r", int(0)),
                    vec![ret(binary(Add, var("r"), b()))],
                ),
                ret(var("r")),
            ],
        ),
        MOD_CHECKED => func(
            name,
            &two_site,
            Ty::I32,
            vec![
                fail_if(binary(Eq, b(), int(0))),
                ret(call(MOD, vec![a(), b()])),
            ],
        ),
        // Int `**` by squaring, wrapping. `b` is not negative.
        POW => func(
            name,
            &two,
            Ty::I32,
            vec![
                let_("result", Ty::U32, uint(1)),
                let_("factor", Ty::U32, cast(Ty::U32, a())),
                let_("left", Ty::U32, cast(Ty::U32, b())),
                Stmt::While(
                    binary(Ne, var("left"), uint(0)),
                    vec![
                        if_(
                            binary(Ne, binary(BitAnd, var("left"), uint(1)), uint(0)),
                            vec![assign(
                                var("result"),
                                binary(Mul, var("result"), var("factor")),
                            )],
                        ),
                        assign(var("factor"), binary(Mul, var("factor"), var("factor"))),
                        assign(var("left"), binary(Shr, var("left"), uint(1))),
                    ],
                ),
                ret(cast(Ty::I32, var("result"))),
            ],
        ),
        POW_CHECKED => func(
            name,
            &two_site,
            Ty::I32,
            vec![
                fail_if(binary(Lt, b(), int(0))),
                ret(call(POW, vec![a(), b()])),
            ],
        ),
        // Float `%`: `a - b * trunc(a / b)`, moved to the sign of `b`;
        // NaN when `b` is zero.
        FMOD => func(
            name,
            &[("a", Ty::F32), ("b", Ty::F32)],
            Ty::F32,
            vec![
                if_(
                    binary(Eq, b(), float(0.0)),
                    vec![ret(Expr::F32Bits(NAN_BITS))],
                ),
                let_(
                    "r",
                    Ty::F32,
                    binary(
                        Sub,
                        a(),
                        binary(Mul, b(), math(MathFn::Trunc, vec![binary(Div, a(), b())])),
                    ),
                ),
                if_(
                    signs_differ("r", float(0.0)),
                    vec![assign(var("r"), binary(Add, var("r"), b()))],
                ),
                ret(var("r")),
            ],
        ),
        // Float `**`. The targets' own power function is only asked about
        // a positive base; a negative base has a power when the exponent
        // is a whole number.
        FPOW => func(
            name,
            &[("a", Ty::F32), ("b", Ty::F32)],
            Ty::F32,
            vec![
                if_(binary(Eq, b(), float(0.0)), vec![ret(float(1.0))]),
                // 1 to any power, and -1 to an infinite one, is 1.
                if_(binary(Eq, a(), float(1.0)), vec![ret(float(1.0))]),
                if_(
                    binary(
                        And,
                        binary(Eq, a(), float(-1.0)),
                        binary(
                            Eq,
                            binary(BitAnd, bits(Ty::U32, b()), Expr::Hex(ABS_MASK)),
                            Expr::Hex(INFINITY_BITS),
                        ),
                    ),
                    vec![ret(float(1.0))],
                ),
                if_(
                    binary(Gt, a(), float(0.0)),
                    vec![ret(math(MathFn::Pow, vec![a(), b()]))],
                ),
                if_(
                    binary(Eq, a(), float(0.0)),
                    vec![
                        if_(binary(Gt, b(), float(0.0)), vec![ret(float(0.0))]),
                        ret(Expr::F32Bits(INFINITY_BITS)),
                    ],
                ),
                if_(
                    binary(Ne, math(MathFn::Floor, vec![b()]), b()),
                    vec![ret(Expr::F32Bits(NAN_BITS))],
                ),
                let_(
                    "r",
                    Ty::F32,
                    math(MathFn::Pow, vec![unary(UnOp::Neg, a()), b()]),
                ),
                // An odd exponent keeps the sign of the base. An infinite
                // one is not odd.
                if_(
                    binary(
                        And,
                        binary(
                            Ne,
                            binary(BitAnd, bits(Ty::U32, b()), Expr::Hex(ABS_MASK)),
                            Expr::Hex(INFINITY_BITS),
                        ),
                        binary(
                            Ne,
                            binary(
                                Sub,
                                b(),
                                binary(
                                    Mul,
                                    float(2.0),
                                    math(MathFn::Floor, vec![binary(Mul, b(), float(0.5))]),
                                ),
                            ),
                            float(0.0),
                        ),
                    ),
                    vec![ret(unary(UnOp::Neg, var("r")))],
                ),
                ret(var("r")),
            ],
        ),
        // A Float with no fractional part as an Int: the nearest Int when
        // it is out of range.
        WHOLE => func(
            name,
            &[("v", Ty::F32)],
            Ty::I32,
            vec![
                if_(
                    binary(Ge, v(), float(2_147_483_648.0)),
                    vec![ret(int(i32::MAX))],
                ),
                if_(
                    binary(Le, v(), float(-2_147_483_648.0)),
                    vec![ret(int(i32::MIN))],
                ),
                ret(cast(Ty::I32, v())),
            ],
        ),
        TO_I | FLOOR | CEIL => {
            let rounding = match name {
                TO_I => MathFn::Trunc,
                FLOOR => MathFn::Floor,
                _ => MathFn::Ceil,
            };
            func(
                name,
                &[("v", Ty::F32)],
                Ty::I32,
                vec![
                    if_(is_nan(v()), vec![ret(int(0))]),
                    ret(call(WHOLE, vec![math(rounding, vec![v()])])),
                ],
            )
        }
        // The nearest whole number, halves away from zero.
        ROUND_HALF => func(
            name,
            &[("v", Ty::F32)],
            Ty::F32,
            vec![
                let_("t", Ty::F32, math(MathFn::Trunc, vec![v()])),
                if_(
                    binary(
                        Ge,
                        math(MathFn::Abs, vec![binary(Sub, v(), var("t"))]),
                        float(0.5),
                    ),
                    vec![Stmt::If(
                        binary(Lt, v(), float(0.0)),
                        vec![assign(var("t"), binary(Sub, var("t"), float(1.0)))],
                        vec![assign(var("t"), binary(Add, var("t"), float(1.0)))],
                    )],
                ),
                ret(var("t")),
            ],
        ),
        ROUND => func(
            name,
            &[("v", Ty::F32)],
            Ty::I32,
            vec![
                if_(is_nan(v()), vec![ret(int(0))]),
                ret(call(WHOLE, vec![call(ROUND_HALF, vec![v()])])),
            ],
        ),
        // Ten to the power `n`, by multiplying. `n` is not negative.
        POW10 => func(
            name,
            &[("n", Ty::I32)],
            Ty::F32,
            vec![
                let_("scale", Ty::F32, float(1.0)),
                Stmt::For {
                    var: "k".to_string(),
                    ty: Ty::I32,
                    from: int(0),
                    op: Lt,
                    to: var("n"),
                    down: false,
                    guard: None,
                    body: vec![assign(var("scale"), binary(Mul, var("scale"), float(10.0)))],
                },
                ret(var("scale")),
            ],
        ),
        // `v.round(digits)`.
        ROUND_DIGITS => {
            let magnitude = |value: Expr| binary(BitAnd, bits(Ty::U32, value), Expr::Hex(ABS_MASK));
            func(
                name,
                &[("v", Ty::F32), ("digits", Ty::I32)],
                Ty::F32,
                vec![
                    let_("m", Ty::U32, magnitude(v())),
                    // Zeros, infinities and NaN stay as they are.
                    if_(
                        binary(
                            Or,
                            binary(Ge, var("m"), Expr::Hex(INFINITY_BITS)),
                            binary(Eq, var("m"), uint(0)),
                        ),
                        vec![ret(v())],
                    ),
                    if_(
                        binary(Ge, var("digits"), int(0)),
                        vec![
                            if_(binary(Gt, var("digits"), int(MAX_DIGITS)), vec![ret(v())]),
                            let_("scale", Ty::F32, call(POW10, vec![var("digits")])),
                            let_("scaled", Ty::F32, binary(Mul, v(), var("scale"))),
                            // Nothing to round off, or out of range.
                            if_(
                                binary(Ge, magnitude(var("scaled")), Expr::Hex(WHOLE_FROM_BITS)),
                                vec![ret(v())],
                            ),
                            ret(binary(
                                Div,
                                call(ROUND_HALF, vec![var("scaled")]),
                                var("scale"),
                            )),
                        ],
                    ),
                    if_(
                        binary(Lt, var("digits"), int(-MAX_DIGITS)),
                        vec![ret(binary(Mul, v(), float(0.0)))],
                    ),
                    let_(
                        "step",
                        Ty::F32,
                        call(POW10, vec![binary(Sub, int(0), var("digits"))]),
                    ),
                    ret(binary(
                        Mul,
                        call(ROUND_HALF, vec![binary(Div, v(), var("step"))]),
                        var("step"),
                    )),
                ],
            )
        }
        // Stores into compact fields: the value wrapped to the field's
        // width, and a failure when that changed it.
        TO_INT8 | TO_INT16 | TO_UINT8 | TO_UINT16 => {
            let wrapped = match name {
                TO_INT8 | TO_INT16 => {
                    let shift = if name == TO_INT8 { 24 } else { 16 };
                    binary(
                        Shr,
                        cast(Ty::I32, binary(Shl, cast(Ty::U32, v()), uint(shift))),
                        uint(shift),
                    )
                }
                TO_UINT8 => binary(BitAnd, v(), int(0xff)),
                _ => binary(BitAnd, v(), int(0xffff)),
            };
            func(
                name,
                &[("v", Ty::I32), ("site", Ty::U32)],
                Ty::I32,
                vec![
                    let_("w", Ty::I32, wrapped),
                    if_(binary(Ne, var("w"), v()), vec![Stmt::Fail(var("site"))]),
                    ret(var("w")),
                ],
            )
        }
        EQ_ENTITY => func(
            name,
            &[("a", Ty::Entity), ("b", Ty::Entity)],
            Ty::Bool,
            vec![ret(binary(
                And,
                binary(Eq, field(a(), "index"), field(b(), "index")),
                binary(Eq, field(a(), "generation"), field(b(), "generation")),
            ))],
        ),
        // The lowbias32 integer hash.
        HASH => {
            let h = || var("h");
            let mix = |shift: u32| assign(h(), binary(BitXor, h(), binary(Shr, h(), uint(shift))));
            let scale = |by: u32| assign(h(), binary(Mul, h(), Expr::Hex(by)));
            func(
                name,
                &[("x", Ty::U32)],
                Ty::U32,
                vec![
                    let_("h", Ty::U32, var("x")),
                    mix(16),
                    scale(0x7feb_352d),
                    mix(15),
                    scale(0x846c_a68b),
                    mix(16),
                    ret(h()),
                ],
            )
        }
        // What a run's random numbers depend on besides the draw count:
        // the seed, the block, the tick and the run's entity, hashed in
        // that order.
        KEY => {
            let h = || var("h");
            let fold = |value: Expr| assign(h(), call(HASH, vec![binary(BitXor, h(), value)]));
            func(
                name,
                &[("index", Ty::U32), ("generation", Ty::U32)],
                Ty::U32,
                vec![
                    let_(
                        "h",
                        Ty::U32,
                        call(HASH, vec![Expr::Uniform(WORD_SEED, Ty::U32)]),
                    ),
                    fold(Expr::Uniform(WORD_BLOCK, Ty::U32)),
                    fold(cast(Ty::U32, Expr::Uniform(WORD_TICK, Ty::I32))),
                    fold(var("index")),
                    fold(var("generation")),
                    ret(h()),
                ],
            )
        }
        // The run's next 32 random bits.
        DRAW => func(
            name,
            &[],
            Ty::U32,
            vec![
                let_(
                    "h",
                    Ty::U32,
                    call(HASH, vec![binary(BitXor, Expr::Key, Expr::Draws)]),
                ),
                assign(Expr::Draws, binary(Add, Expr::Draws, uint(1))),
                ret(var("h")),
            ],
        ),
        // `rand`: in [0, 1).
        RAND => func(
            name,
            &[],
            Ty::F32,
            vec![ret(binary(
                Mul,
                cast(Ty::F32, binary(Shr, call(DRAW, vec![]), uint(8))),
                float(RAND_SCALE),
            ))],
        ),
        // `rand(n)` for a positive `n`: in [0, n).
        RAND_BELOW => func(
            name,
            &[("n", Ty::I32)],
            Ty::I32,
            vec![ret(cast(
                Ty::I32,
                binary(Rem, call(DRAW, vec![]), cast(Ty::U32, var("n"))),
            ))],
        ),
        RAND_INT => func(
            name,
            &[("n", Ty::I32), ("site", Ty::U32)],
            Ty::I32,
            vec![
                fail_if(binary(Le, var("n"), int(0))),
                ret(call(RAND_BELOW, vec![var("n")])),
            ],
        ),
        other => unreachable!("no GPU helper is called `{other}`"),
    }
}
