//! The functions GPU code calls for what neither target has as an
//! operator: `Int` division that floors, powers, `Float` to `Int`
//! conversions that saturate, range checks and the random numbers. Each is
//! written once, in the intermediate form, and printed for both targets.
//!
//! Strict code has its own form of every helper with `Float` arithmetic,
//! named `s32_` where the plain one is `g32_`, and helpers of its own that
//! do on the bits what plain code leaves to the target: comparisons,
//! `trunc`, `floor`, `ceil`, `Int` to `Float`, `/` and `sqrt`. In C, `/`
//! and `sqrt` are the hardware operations, run with subnormals flushed;
//! their helpers are printed only into shaders.

use super::ir::{
    BinOp, Expr, Func, Library, MathFn, Param, Stmt, Ty, UnOp, Uses, assign, binary, bits, call,
    cast, field, if_, let_, opaque, ret, select, unary, var,
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

/// Strict code only.
pub const CMP: &str = "s32_cmp";
pub const TO_F: &str = "s32_to_f";
pub const DIV_F: &str = "s32_div";
pub const SQRT_F: &str = "s32_sqrt";
const KEY_F: &str = "s32_key";
const TRUNC_F: &str = "s32_truncf";
const FLOOR_F: &str = "s32_floorf";
const CEIL_F: &str = "s32_ceilf";

/// The helpers C does not define: it uses the hardware operation instead.
pub const SHADER_ONLY: [&str; 2] = [DIV_F, SQRT_F];

/// `rand` is the top 24 bits of a draw times 2^-24.
const RAND_SCALE: f32 = 1.0 / 16_777_216.0;
const SIGN_BIT: u32 = 0x8000_0000;
/// The bits of the smallest normal `F32`, 2^-126.
const NORMAL_FROM_BITS: u32 = 0x0080_0000;
/// The bits of 1.0.
const ONE_BITS: u32 = 0x3f80_0000;
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

/// The name of a helper in plain or in strict code: a helper with `Float`
/// arithmetic has a strict form.
pub fn in_mode(name: &str, strict: bool) -> String {
    if strict && STRICT_FORMS.contains(&name) {
        name.replacen("g32_", "s32_", 1)
    } else {
        name.to_string()
    }
}

/// The plain helpers that have a strict form.
const STRICT_FORMS: [&str; 9] = [
    FMOD,
    TO_I,
    FLOOR,
    CEIL,
    ROUND,
    ROUND_DIGITS,
    WHOLE,
    ROUND_HALF,
    POW10,
];

/// `F32` operations as plain or strict code has them.
#[derive(Clone, Copy)]
pub struct F {
    pub strict: bool,
}

impl F {
    /// `+`, `-` or `*`. In strict code each operand is opaque.
    pub fn arith(self, op: BinOp, a: Expr, b: Expr) -> Expr {
        if self.strict {
            binary(op, opaque(0, a), opaque(1, b))
        } else {
            binary(op, a, b)
        }
    }

    pub fn div(self, a: Expr, b: Expr) -> Expr {
        if self.strict {
            call(DIV_F, vec![a, b])
        } else {
            binary(BinOp::Div, a, b)
        }
    }

    /// One of the six comparisons. Strict code compares the bits: a
    /// subnormal is zero, and with a NaN only `!=` holds.
    pub fn cmp(self, op: BinOp, a: Expr, b: Expr) -> Expr {
        if !self.strict {
            return binary(op, a, b);
        }
        let (op, order) = match op {
            BinOp::Lt => (BinOp::Lt, call(CMP, vec![a, b])),
            BinOp::Le => (BinOp::Le, call(CMP, vec![a, b])),
            BinOp::Gt => (BinOp::Lt, call(CMP, vec![b, a])),
            BinOp::Ge => (BinOp::Le, call(CMP, vec![b, a])),
            BinOp::Eq | BinOp::Ne => (op, call(CMP, vec![a, b])),
            _ => unreachable!("not a comparison"),
        };
        binary(op, order, Expr::I32(0))
    }

    /// `-a`: in strict code, the sign bit flipped.
    pub fn neg(self, a: Expr) -> Expr {
        if self.strict {
            bits(
                Ty::F32,
                binary(BinOp::BitXor, bits(Ty::U32, a), Expr::Hex(SIGN_BIT)),
            )
        } else {
            unary(UnOp::Neg, a)
        }
    }

    /// `a.abs`: in strict code, the sign bit cleared.
    pub fn abs(self, a: Expr) -> Expr {
        if self.strict {
            bits(
                Ty::F32,
                binary(BinOp::BitAnd, bits(Ty::U32, a), Expr::Hex(ABS_MASK)),
            )
        } else {
            math(MathFn::Abs, vec![a])
        }
    }

    /// `Trunc`, `Floor` or `Ceil` of `a`, as an `F32`.
    pub fn whole(self, rounding: MathFn, a: Expr) -> Expr {
        if !self.strict {
            return math(rounding, vec![a]);
        }
        let name = match rounding {
            MathFn::Trunc => TRUNC_F,
            MathFn::Floor => FLOOR_F,
            MathFn::Ceil => CEIL_F,
            _ => unreachable!("not a rounding"),
        };
        call(name, vec![a])
    }

    fn helper(self, name: &str) -> String {
        in_mode(name, self.strict)
    }
}

const PLAIN: F = F { strict: false };

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
        strict: false,
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
fn signs_differ(f: F, r: &str, zero: Expr) -> Expr {
    binary(
        BinOp::And,
        f.cmp(BinOp::Ne, var(r), zero.clone()),
        binary(
            BinOp::Ne,
            f.cmp(BinOp::Lt, var(r), zero.clone()),
            f.cmp(BinOp::Lt, var("b"), zero),
        ),
    )
}

/// `if (failed) { fail(site); return 0; }`
fn fail_if(failed: Expr) -> Stmt {
    if_(failed, vec![Stmt::Fail(var("site")), ret(int(0))])
}

fn define(name: &str) -> Func {
    if let Some(func) = define_strict(name) {
        return func;
    }
    match name.strip_prefix("s32_") {
        Some(rest) => {
            let mut func = define_in(&format!("g32_{rest}"), F { strict: true });
            func.name = name.to_string();
            func.strict = true;
            func
        }
        None => define_in(name, PLAIN),
    }
}

fn define_in(name: &str, f: F) -> Func {
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
                    signs_differ(PLAIN, "r", int(0)),
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
                    signs_differ(PLAIN, "r", int(0)),
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
                    f.cmp(Eq, b(), float(0.0)),
                    vec![ret(Expr::F32Bits(NAN_BITS))],
                ),
                let_(
                    "r",
                    Ty::F32,
                    f.arith(
                        Sub,
                        a(),
                        f.arith(Mul, b(), f.whole(MathFn::Trunc, f.div(a(), b()))),
                    ),
                ),
                if_(
                    signs_differ(f, "r", float(0.0)),
                    vec![assign(var("r"), f.arith(Add, var("r"), b()))],
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
                    f.cmp(Ge, v(), float(2_147_483_648.0)),
                    vec![ret(int(i32::MAX))],
                ),
                if_(
                    f.cmp(Le, v(), float(-2_147_483_648.0)),
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
                    ret(call(&f.helper(WHOLE), vec![f.whole(rounding, v())])),
                ],
            )
        }
        // The nearest whole number, halves away from zero.
        ROUND_HALF => func(
            name,
            &[("v", Ty::F32)],
            Ty::F32,
            vec![
                let_("t", Ty::F32, f.whole(MathFn::Trunc, v())),
                if_(
                    f.cmp(Ge, f.abs(f.arith(Sub, v(), var("t"))), float(0.5)),
                    vec![Stmt::If(
                        f.cmp(Lt, v(), float(0.0)),
                        vec![assign(var("t"), f.arith(Sub, var("t"), float(1.0)))],
                        vec![assign(var("t"), f.arith(Add, var("t"), float(1.0)))],
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
                ret(call(
                    &f.helper(WHOLE),
                    vec![call(&f.helper(ROUND_HALF), vec![v()])],
                )),
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
                    body: vec![assign(
                        var("scale"),
                        f.arith(Mul, var("scale"), float(10.0)),
                    )],
                },
                ret(var("scale")),
            ],
        ),
        // `v.round(digits)`.
        ROUND_DIGITS => {
            let magnitude = |value: Expr| binary(BitAnd, bits(Ty::U32, value), Expr::Hex(ABS_MASK));
            let (pow10, round_half) = (f.helper(POW10), f.helper(ROUND_HALF));
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
                            let_("scale", Ty::F32, call(&pow10, vec![var("digits")])),
                            let_("scaled", Ty::F32, f.arith(Mul, v(), var("scale"))),
                            // Nothing to round off, or out of range.
                            if_(
                                binary(Ge, magnitude(var("scaled")), Expr::Hex(WHOLE_FROM_BITS)),
                                vec![ret(v())],
                            ),
                            ret(f.div(call(&round_half, vec![var("scaled")]), var("scale"))),
                        ],
                    ),
                    if_(
                        binary(Lt, var("digits"), int(-MAX_DIGITS)),
                        vec![ret(f.arith(Mul, v(), float(0.0)))],
                    ),
                    let_(
                        "step",
                        Ty::F32,
                        call(&pow10, vec![binary(Sub, int(0), var("digits"))]),
                    ),
                    ret(f.arith(
                        Mul,
                        call(&round_half, vec![f.div(v(), var("step"))]),
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

/// A helper that only strict code calls, or `None`.
fn define_strict(name: &str) -> Option<Func> {
    use BinOp::*;
    let v = || var("v");
    let u = || var("u");
    let m = || var("m");
    let s = || var("s");
    let hex = Expr::Hex;
    let mut func = match name {
        // An `F32` as an `I32` in the same order, every subnormal and both
        // zeros as 0. Not called with a NaN.
        KEY_F => func(
            name,
            &[("v", Ty::F32)],
            Ty::I32,
            vec![
                let_("u", Ty::U32, bits(Ty::U32, v())),
                let_("m", Ty::U32, binary(BitAnd, u(), hex(ABS_MASK))),
                if_(binary(Lt, m(), hex(NORMAL_FROM_BITS)), vec![ret(int(0))]),
                if_(
                    binary(Ne, u(), m()),
                    vec![ret(binary(Sub, int(0), cast(Ty::I32, m())))],
                ),
                ret(cast(Ty::I32, m())),
            ],
        ),
        // -1, 0 or 1 as `a` is below, equal to or above `b`, and 2 when
        // either is a NaN.
        CMP => func(
            name,
            &[("a", Ty::F32), ("b", Ty::F32)],
            Ty::I32,
            vec![
                if_(
                    binary(Or, is_nan(var("a")), is_nan(var("b"))),
                    vec![ret(int(2))],
                ),
                let_("ka", Ty::I32, call(KEY_F, vec![var("a")])),
                let_("kb", Ty::I32, call(KEY_F, vec![var("b")])),
                if_(binary(Lt, var("ka"), var("kb")), vec![ret(int(-1))]),
                if_(binary(Gt, var("ka"), var("kb")), vec![ret(int(1))]),
                ret(int(0)),
            ],
        ),
        // `trunc`, `floor` and `ceil` on the bits. A subnormal is zero
        // with its sign.
        TRUNC_F | FLOOR_F | CEIL_F => {
            // Toward negative infinity for `floor`, positive for `ceil`:
            // the sign bit of the values that move away from zero.
            let away = match name {
                FLOOR_F => Some(SIGN_BIT),
                CEIL_F => Some(0),
                _ => None,
            };
            let signed_zero = || bits(Ty::F32, s());
            let below_one = match away {
                None => signed_zero(),
                Some(sign) => select(
                    binary(Eq, s(), hex(sign)),
                    Expr::F32Bits(sign | ONE_BITS),
                    signed_zero(),
                ),
            };
            // The bits of `v` with its fraction cleared.
            let cut = || binary(BitXor, u(), binary(BitAnd, u(), var("mask")));
            let whole = match away {
                None => bits(Ty::F32, cut()),
                Some(sign) => select(
                    binary(Eq, s(), hex(sign)),
                    bits(
                        Ty::F32,
                        binary(Add, binary(Add, cut(), var("mask")), uint(1)),
                    ),
                    bits(Ty::F32, cut()),
                ),
            };
            func(
                name,
                &[("v", Ty::F32)],
                Ty::F32,
                vec![
                    let_("u", Ty::U32, bits(Ty::U32, v())),
                    let_("m", Ty::U32, binary(BitAnd, u(), hex(ABS_MASK))),
                    let_("s", Ty::U32, binary(BitAnd, u(), hex(SIGN_BIT))),
                    if_(
                        binary(Lt, m(), hex(NORMAL_FROM_BITS)),
                        vec![ret(signed_zero())],
                    ),
                    // Whole already, or an infinity or a NaN.
                    if_(binary(Ge, m(), hex(WHOLE_FROM_BITS)), vec![ret(v())]),
                    if_(binary(Lt, m(), hex(ONE_BITS)), vec![ret(below_one)]),
                    let_(
                        "mask",
                        Ty::U32,
                        binary(
                            Shr,
                            hex(0x007f_ffff),
                            binary(Sub, binary(Shr, m(), uint(23)), uint(127)),
                        ),
                    ),
                    if_(
                        binary(Eq, binary(BitAnd, m(), var("mask")), uint(0)),
                        vec![ret(v())],
                    ),
                    ret(whole),
                ],
            )
        }
        // An `Int` as the nearest `Float`, ties to even.
        TO_F => {
            let n = || var("n");
            let a = || var("a");
            let shift = || var("shift");
            func(
                name,
                &[("n", Ty::I32)],
                Ty::F32,
                vec![
                    // Up to 2^24 in magnitude, every `Int` is a `Float`.
                    if_(
                        binary(
                            And,
                            binary(Ge, n(), int(-16_777_216)),
                            binary(Le, n(), int(16_777_216)),
                        ),
                        vec![ret(cast(Ty::F32, n()))],
                    ),
                    let_("s", Ty::U32, uint(0)),
                    let_("a", Ty::U32, cast(Ty::U32, n())),
                    if_(
                        binary(Lt, n(), int(0)),
                        vec![
                            assign(s(), hex(SIGN_BIT)),
                            assign(a(), binary(Sub, uint(0), a())),
                        ],
                    ),
                    // The 24 bits kept are `a >> shift`.
                    let_("shift", Ty::U32, uint(1)),
                    Stmt::While(
                        binary(Ge, binary(Shr, a(), shift()), hex(0x0100_0000)),
                        vec![assign(shift(), binary(Add, shift(), uint(1)))],
                    ),
                    let_("keep", Ty::U32, binary(Shr, a(), shift())),
                    let_(
                        "rest",
                        Ty::U32,
                        binary(
                            BitAnd,
                            a(),
                            binary(Sub, binary(Shl, uint(1), shift()), uint(1)),
                        ),
                    ),
                    let_(
                        "half",
                        Ty::U32,
                        binary(Shl, uint(1), binary(Sub, shift(), uint(1))),
                    ),
                    if_(
                        binary(
                            Or,
                            binary(Gt, var("rest"), var("half")),
                            binary(
                                And,
                                binary(Eq, var("rest"), var("half")),
                                binary(Ne, binary(BitAnd, var("keep"), uint(1)), uint(0)),
                            ),
                        ),
                        vec![assign(var("keep"), binary(Add, var("keep"), uint(1)))],
                    ),
                    // `keep` is 2^23 to 2^24, times 2^shift. Rounding up to
                    // 2^24 carries into the exponent.
                    ret(bits(
                        Ty::F32,
                        binary(
                            Add,
                            s(),
                            binary(
                                Add,
                                binary(Shl, binary(Add, uint(150), shift()), uint(23)),
                                binary(Sub, var("keep"), hex(NORMAL_FROM_BITS)),
                            ),
                        ),
                    )),
                ],
            )
        }
        DIV_F => func(name, &[("a", Ty::F32), ("b", Ty::F32)], Ty::F32, divide()),
        SQRT_F => func(name, &[("v", Ty::F32)], Ty::F32, square_root()),
        _ => return None,
    };
    func.strict = true;
    Some(func)
}

// `/` and `sqrt` of strict shaders: integer code on the significands. The
// hardware's result is an estimate of the 25 bits wanted (24 and a round
// bit); a first correction comes from the exact 64-bit residual, converted
// to `F32`, a second from the residual after it, which then fits an `I32`,
// and a last step of one either way leaves the exact quotient or root. The
// remainder is the sticky bit. Exact whenever the estimate is within 2^-11.

fn u(name: &str) -> Expr {
    var(name)
}

fn hex(value: u32) -> Expr {
    Expr::Hex(value)
}

fn as_i32(value: Expr) -> Expr {
    cast(Ty::I32, value)
}

fn as_u32(value: Expr) -> Expr {
    cast(Ty::U32, value)
}

/// `F32` from a `U32` or an `I32`: part of an estimate.
fn as_f32(value: Expr) -> Expr {
    cast(Ty::F32, value)
}

/// `floor(value)` as an `I32`.
fn floor_i32(value: Expr) -> Expr {
    as_i32(math(MathFn::Floor, vec![value]))
}

/// 1 when `cond`, else 0, as a `U32`.
fn carry(cond: Expr) -> Expr {
    select(cond, uint(1), uint(0))
}

/// `{out}_lo` and `{out}_hi` := the 64-bit product of two `U32`s below 2^25.
fn mul64(out: &str, a: Expr, b: Expr) -> Vec<Stmt> {
    use BinOp::*;
    let n = |part: &str| format!("{out}_{part}");
    let low = |value: Expr| binary(BitAnd, value, hex(0xffff));
    let high = |value: Expr| binary(Shr, value, uint(16));
    vec![
        let_(&n("a0"), Ty::U32, low(a.clone())),
        let_(&n("a1"), Ty::U32, high(a)),
        let_(&n("b0"), Ty::U32, low(b.clone())),
        let_(&n("b1"), Ty::U32, high(b)),
        let_(&n("p00"), Ty::U32, binary(Mul, u(&n("a0")), u(&n("b0")))),
        let_(
            &n("p01"),
            Ty::U32,
            binary(
                Add,
                binary(Mul, u(&n("a0")), u(&n("b1"))),
                binary(Mul, u(&n("a1")), u(&n("b0"))),
            ),
        ),
        let_(
            &n("lo"),
            Ty::U32,
            binary(Add, u(&n("p00")), binary(Shl, u(&n("p01")), uint(16))),
        ),
        let_(
            &n("hi"),
            Ty::U32,
            binary(
                Add,
                binary(
                    Add,
                    binary(Mul, u(&n("a1")), u(&n("b1"))),
                    binary(Shr, u(&n("p01")), uint(16)),
                ),
                carry(binary(Lt, u(&n("lo")), u(&n("p00")))),
            ),
        ),
    ]
}

/// `{out}_lo` and `{out}_hi` := `{x}` - `{y}`, 64 bits each.
fn sub64(out: &str, x: &str, y: &str) -> Vec<Stmt> {
    use BinOp::*;
    let (xl, xh) = (u(&format!("{x}_lo")), u(&format!("{x}_hi")));
    let (yl, yh) = (u(&format!("{y}_lo")), u(&format!("{y}_hi")));
    vec![
        let_(
            &format!("{out}_lo"),
            Ty::U32,
            binary(Sub, xl.clone(), yl.clone()),
        ),
        let_(
            &format!("{out}_hi"),
            Ty::U32,
            binary(Sub, binary(Sub, xh, yh), carry(binary(Lt, xl, yl))),
        ),
    ]
}

/// A signed 64-bit value as an `F32`.
fn wide(name: &str) -> Expr {
    binary(
        BinOp::Add,
        binary(
            BinOp::Mul,
            as_f32(as_i32(u(&format!("{name}_hi")))),
            Expr::F32(4_294_967_296.0),
        ),
        as_f32(u(&format!("{name}_lo"))),
    )
}

/// Returns the `F32` with sign bit `sign`, biased exponent `e` before
/// rounding, and significand `q` (24 bits and a round bit) rounded to
/// nearest, ties to even, with `sticky` saying whether anything is left
/// below the round bit. Infinity when the exponent overflows.
fn pack(sign: Expr, sticky: Expr) -> Vec<Stmt> {
    use BinOp::*;
    vec![
        let_("sig", Ty::U32, binary(Shr, u("q"), uint(1))),
        if_(
            binary(
                And,
                binary(Ne, binary(BitAnd, u("q"), uint(1)), uint(0)),
                binary(
                    Or,
                    sticky,
                    binary(Ne, binary(BitAnd, u("sig"), uint(1)), uint(0)),
                ),
            ),
            vec![assign(u("sig"), binary(Add, u("sig"), uint(1)))],
        ),
        if_(
            binary(Eq, u("sig"), hex(0x0100_0000)),
            vec![
                assign(u("sig"), hex(NORMAL_FROM_BITS)),
                assign(u("e"), binary(Add, u("e"), int(1))),
            ],
        ),
        if_(
            binary(Ge, u("e"), int(255)),
            vec![ret(bits(
                Ty::F32,
                binary(Add, sign.clone(), hex(INFINITY_BITS)),
            ))],
        ),
        ret(bits(
            Ty::F32,
            binary(
                Add,
                binary(Add, sign, binary(Shl, as_u32(u("e")), uint(23))),
                binary(BitAnd, u("sig"), hex(0x007f_ffff)),
            ),
        )),
    ]
}

fn divide() -> Vec<Stmt> {
    use BinOp::*;
    let nan = || ret(Expr::F32Bits(NAN_BITS));
    let field = |value: Expr, shift: u32, mask: u32| {
        binary(BitAnd, binary(Shr, value, uint(shift)), hex(mask))
    };
    let is = |name: &str, value: u32| binary(Eq, u(name), uint(value));
    let mut body = vec![
        let_("ua", Ty::U32, bits(Ty::U32, var("a"))),
        let_("ub", Ty::U32, bits(Ty::U32, var("b"))),
        let_(
            "sg",
            Ty::U32,
            binary(BitAnd, binary(BitXor, u("ua"), u("ub")), hex(SIGN_BIT)),
        ),
        let_("ea", Ty::U32, field(u("ua"), 23, 0xff)),
        let_("eb", Ty::U32, field(u("ub"), 23, 0xff)),
        let_("ma", Ty::U32, binary(BitAnd, u("ua"), hex(0x007f_ffff))),
        let_("mb", Ty::U32, binary(BitAnd, u("ub"), hex(0x007f_ffff))),
        // A NaN, 0 / 0 and infinity / infinity. A subnormal is zero.
        if_(
            binary(
                Or,
                binary(
                    Or,
                    binary(And, is("ea", 255), binary(Ne, u("ma"), uint(0))),
                    binary(And, is("eb", 255), binary(Ne, u("mb"), uint(0))),
                ),
                binary(
                    Or,
                    binary(And, is("ea", 0), is("eb", 0)),
                    binary(And, is("ea", 255), is("eb", 255)),
                ),
            ),
            vec![nan()],
        ),
        if_(
            binary(Or, is("ea", 255), is("eb", 0)),
            vec![ret(bits(Ty::F32, binary(Add, u("sg"), hex(INFINITY_BITS))))],
        ),
        if_(
            binary(Or, is("ea", 0), is("eb", 255)),
            vec![ret(bits(Ty::F32, u("sg")))],
        ),
        // Significands A in [B, 2B), so that A / B is in [1, 2).
        let_("sa", Ty::U32, binary(Add, u("ma"), hex(NORMAL_FROM_BITS))),
        let_("sb", Ty::U32, binary(Add, u("mb"), hex(NORMAL_FROM_BITS))),
        let_(
            "e",
            Ty::I32,
            binary(Add, binary(Sub, as_i32(u("ea")), as_i32(u("eb"))), int(127)),
        ),
        if_(
            binary(Lt, u("sa"), u("sb")),
            vec![
                assign(u("sa"), binary(Shl, u("sa"), uint(1))),
                assign(u("e"), binary(Sub, u("e"), int(1))),
            ],
        ),
        // Below 2^-126: no quotient of normal numbers rounds up to it.
        if_(
            binary(Le, u("e"), int(0)),
            vec![ret(bits(Ty::F32, u("sg")))],
        ),
        // q = floor(sa * 2^24 / sb).
        let_(
            "q",
            Ty::U32,
            as_u32(as_i32(math(
                MathFn::Trunc,
                vec![binary(
                    Mul,
                    binary(Div, as_f32(u("sa")), as_f32(u("sb"))),
                    Expr::F32(16_777_216.0),
                )],
            ))),
        ),
        let_("n_lo", Ty::U32, binary(Shl, u("sa"), uint(24))),
        let_("n_hi", Ty::U32, binary(Shr, u("sa"), uint(8))),
    ];
    body.extend(mul64("p1", u("q"), u("sb")));
    body.extend(sub64("d1", "n", "p1"));
    body.push(assign(
        u("q"),
        as_u32(binary(
            Add,
            as_i32(u("q")),
            floor_i32(binary(Div, wide("d1"), as_f32(u("sb")))),
        )),
    ));
    body.extend(mul64("p2", u("q"), u("sb")));
    body.extend(sub64("d2", "n", "p2"));
    body.extend([
        let_("r", Ty::I32, as_i32(u("d2_lo"))),
        let_("bi", Ty::I32, as_i32(u("sb"))),
        let_(
            "dq",
            Ty::I32,
            floor_i32(binary(Div, as_f32(u("r")), as_f32(u("bi")))),
        ),
        assign(u("q"), as_u32(binary(Add, as_i32(u("q")), u("dq")))),
        assign(u("r"), binary(Sub, u("r"), binary(Mul, u("dq"), u("bi")))),
        if_(
            binary(Lt, u("r"), int(0)),
            vec![
                assign(u("q"), binary(Sub, u("q"), uint(1))),
                assign(u("r"), binary(Add, u("r"), u("bi"))),
            ],
        ),
        if_(
            binary(Ge, u("r"), u("bi")),
            vec![
                assign(u("q"), binary(Add, u("q"), uint(1))),
                assign(u("r"), binary(Sub, u("r"), u("bi"))),
            ],
        ),
    ]);
    body.extend(pack(u("sg"), binary(Ne, u("r"), int(0))));
    body
}

fn square_root() -> Vec<Stmt> {
    use BinOp::*;
    let nan = || ret(Expr::F32Bits(NAN_BITS));
    let twice = || binary(Mul, uint(2), u("q"));
    let mut body = vec![
        let_("w", Ty::U32, bits(Ty::U32, var("v"))),
        let_(
            "ex",
            Ty::U32,
            binary(BitAnd, binary(Shr, u("w"), uint(23)), hex(0xff)),
        ),
        let_("m", Ty::U32, binary(BitAnd, u("w"), hex(0x007f_ffff))),
        // Zeros and subnormals: zero with the sign.
        if_(
            binary(Eq, u("ex"), uint(0)),
            vec![ret(bits(Ty::F32, binary(BitAnd, u("w"), hex(SIGN_BIT))))],
        ),
        if_(
            binary(
                Or,
                binary(
                    And,
                    binary(Eq, u("ex"), uint(255)),
                    binary(Ne, u("m"), uint(0)),
                ),
                binary(Ne, binary(BitAnd, u("w"), hex(SIGN_BIT)), uint(0)),
            ),
            vec![nan()],
        ),
        if_(binary(Eq, u("ex"), uint(255)), vec![ret(var("v"))]),
        // M in [2^23, 2^25) and an even exponent.
        let_("sm", Ty::U32, binary(Add, u("m"), hex(NORMAL_FROM_BITS))),
        let_("e", Ty::I32, binary(Sub, as_i32(u("ex")), int(127))),
        if_(
            binary(Ne, binary(BitAnd, u("e"), int(1)), int(0)),
            vec![
                assign(u("sm"), binary(Shl, u("sm"), uint(1))),
                assign(u("e"), binary(Sub, u("e"), int(1))),
            ],
        ),
        // q = floor(sqrt(sm * 2^25)).
        let_(
            "q",
            Ty::U32,
            as_u32(as_i32(math(
                MathFn::Trunc,
                vec![binary(
                    Mul,
                    math(
                        MathFn::Sqrt,
                        vec![binary(Mul, as_f32(u("sm")), Expr::F32(2.0))],
                    ),
                    Expr::F32(4096.0),
                )],
            ))),
        ),
        let_("n_lo", Ty::U32, binary(Shl, u("sm"), uint(25))),
        let_("n_hi", Ty::U32, binary(Shr, u("sm"), uint(7))),
    ];
    body.extend(mul64("p1", u("q"), u("q")));
    body.extend(sub64("d1", "n", "p1"));
    body.push(assign(
        u("q"),
        as_u32(binary(
            Add,
            as_i32(u("q")),
            floor_i32(binary(Div, wide("d1"), as_f32(twice()))),
        )),
    ));
    body.extend(mul64("p2", u("q"), u("q")));
    body.extend(sub64("d2", "n", "p2"));
    body.extend([
        let_("r", Ty::I32, as_i32(u("d2_lo"))),
        let_(
            "dq",
            Ty::I32,
            floor_i32(binary(Div, as_f32(u("r")), as_f32(twice()))),
        ),
        assign(
            u("r"),
            binary(
                Sub,
                u("r"),
                binary(
                    Mul,
                    u("dq"),
                    binary(Add, binary(Mul, int(2), as_i32(u("q"))), u("dq")),
                ),
            ),
        ),
        assign(u("q"), as_u32(binary(Add, as_i32(u("q")), u("dq")))),
        if_(
            binary(Lt, u("r"), int(0)),
            vec![
                assign(u("q"), binary(Sub, u("q"), uint(1))),
                assign(
                    u("r"),
                    binary(Add, u("r"), as_i32(binary(Add, twice(), uint(1)))),
                ),
            ],
        ),
        if_(
            binary(Gt, u("r"), as_i32(twice())),
            vec![
                assign(
                    u("r"),
                    binary(Sub, u("r"), as_i32(binary(Add, twice(), uint(1)))),
                ),
                assign(u("q"), binary(Add, u("q"), uint(1))),
            ],
        ),
        assign(u("e"), binary(Add, binary(Div, u("e"), int(2)), int(127))),
    ]);
    body.extend(pack(uint(0), binary(Ne, u("r"), int(0))));
    body
}
