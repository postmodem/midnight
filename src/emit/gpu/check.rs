//! The strict check: strict code that the runtime runs on the device and
//! on the CPU when a program with a strict GPU system opens the device,
//! comparing the bits. A device that gives different bits runs strict
//! blocks on the CPU.
//!
//! The check is one library function, [`CHECK`], which gives the bits of
//! one of [`EXPRESSIONS`] expressions over the three inputs `a`, `b` and
//! `c` of a row of [`INPUTS`]. The rows are chosen so that each way a
//! driver could go wrong changes some result: a fused multiply-add, the
//! choice of product in a sum of two, a factored sum, `x - x` and
//! `x * 0.0` folded, `x + 0.0` folded with -0, an inexact `/` or `sqrt`, a
//! subnormal kept, and a comparison that sees through a NaN.

use super::helpers::{self, F};
use super::ir::{
    BinOp, Expr, Func, MathFn, Param, Ty, binary, bits, call, cast, if_, let_, ret, select, var,
};

/// `s32_check(a, b, c, k)`: the bits of expression `k`.
pub const CHECK: &str = "s32_check";

/// Results per row.
pub const EXPRESSIONS: u32 = 14;

/// The bits of a NaN result, whichever NaN it is.
const NAN_BITS: u32 = 0x7fc0_0000;

/// The rows of the check: the bits of `a`, `b` and `c`.
pub const INPUTS: [[u32; 3]; 39] = [
    // `a * b + c` and `a * b - c` where a fused multiply-add keeps what
    // rounding `a * b` drops.
    [0x3f80_0001, 0x3f80_0001, 0xbf80_0002],
    [0x3f80_0001, 0x3f80_0001, 0x3f80_0002],
    [0x3faa_aaab, 0x4040_0000, 0xc080_0000],
    // `a * a + b * b` where fusing either product changes the sum.
    [0x3fcc_39b1, 0x3f93_ed82, 0],
    [0x3f86_ead9, 0x3f80_714c, 0],
    // `a * a + b * b` where the two fused forms differ.
    [0x3fba_f963, 0x3ff5_b451, 0],
    [0x3f97_614c, 0x3f93_9c8e, 0],
    // `a * b + b * c` where `b * (a + c)` differs.
    [0x3fec_6358, 0x3fbe_6d9e, 0x3f14_8740],
    [0x3f93_b530, 0x3fe8_de5a, 0x3f3c_96a3],
    // `a / b` next to a midpoint.
    [0x3fb6_08a2, 0x3f85_0c33, 0],
    [0x3fb7_0497, 0x3f9d_af5e, 0],
    [0x3fef_8e59, 0x3fc1_e705, 0],
    // `sqrt(a)` next to a midpoint.
    [0x3ff7_5852, 0x3f80_0000, 0],
    [0x4010_5f4f, 0x3f80_0000, 0],
    [0x3f81_7162, 0x3f80_0000, 0],
    // Divisors at and above 2^126.
    [0x7f00_0000, 0x7e80_0000, 0],
    [0x7149_f2ca, 0x7f7f_ffff, 0],
    [0x7f7f_ffff, 0x7f00_0000, 0],
    // Products of two normal numbers just below 2^-126, which round to it.
    [0x207a_91b2, 0x1f82_c638, 0],
    [0xa071_4811, 0x1f87_cee4, 0],
    // Subnormal operands and results.
    [0x0000_0001, 0x0080_0000, 0x8000_0001],
    [0x0080_0000, 0x0000_0001, 0x3f80_0000],
    [0x00c0_0000, 0x8080_0000, 0],
    [0x807f_ffff, 0x0000_0000, 0x3f80_0000],
    [0x1e3c_e508, 0x1e3c_e508, 0x60ad_78ec],
    // Infinities, NaNs and zeros.
    [0x7f80_0000, 0x3f80_0000, 0x7f80_0000],
    [0xff80_0000, 0x7f80_0000, 0x0000_0000],
    [0x7fc0_0000, 0x3f80_0000, 0x3f80_0000],
    [0x3f80_0000, 0xffc0_0001, 0x0000_0000],
    [0x8000_0000, 0x8000_0000, 0x0000_0000],
    [0x0000_0000, 0x8000_0000, 0x8000_0000],
    [0x7f7f_ffff, 0x7f7f_ffff, 0xff7f_ffff],
    // `Int` to `Float` past 2^24, ties included: the bits of `a`.
    [0x0100_0001, 0x4b80_0001, 0],
    [0x0100_0003, 0xcb80_0003, 0],
    [0x4b80_0001, 0x0100_0001, 0],
    [0xcf00_0000, 0x4f00_0000, 0],
    // `floor` and plain values.
    [0xbfc0_0000, 0x3f00_0000, 0x4000_0000],
    [0x4b7f_ffff, 0xcb7f_ffff, 0x3eaa_aaab],
    [0x3dcc_cccd, 0x4049_0fdb, 0xbf80_0000],
];

/// The check function. It calls strict helpers, which the library must
/// also hold.
pub fn function() -> Func {
    let f = F { strict: true };
    let a = || var("a");
    let b = || var("b");
    let c = || var("c");
    let arith = |op, x, y| f.arith(op, x, y);
    use BinOp::{Add, Mul, Sub};
    let floats = [
        arith(Add, arith(Mul, a(), b()), c()),
        arith(Sub, arith(Mul, a(), b()), c()),
        arith(Add, arith(Mul, a(), a()), arith(Mul, b(), b())),
        arith(Add, arith(Mul, a(), b()), arith(Mul, b(), c())),
        arith(Sub, a(), a()),
        arith(Mul, a(), Expr::F32(0.0)),
        arith(Add, a(), Expr::F32(0.0)),
        f.div(a(), b()),
        call(helpers::SQRT_F, vec![a()]),
        arith(Mul, a(), b()),
        arith(Add, a(), b()),
        f.whole(MathFn::Floor, a()),
        call(helpers::TO_F, vec![cast(Ty::I32, bits(Ty::U32, a()))]),
    ];
    let mut body = Vec::new();
    for (k, value) in floats.into_iter().enumerate() {
        let r = format!("r{k}");
        body.push(if_(
            binary(BinOp::Eq, var("k"), Expr::I32(k as i32)),
            vec![
                let_(&r, Ty::F32, value),
                ret(select(
                    helpers::is_nan(var(&r)),
                    Expr::Hex(NAN_BITS),
                    bits(Ty::U32, var(&r)),
                )),
            ],
        ));
    }
    // The six comparisons of `a` and `b`, one bit each.
    let comparisons = [
        BinOp::Lt,
        BinOp::Le,
        BinOp::Gt,
        BinOp::Ge,
        BinOp::Eq,
        BinOp::Ne,
    ];
    let mut mask = Expr::U32(0);
    for (bit, op) in comparisons.into_iter().enumerate() {
        mask = binary(
            Add,
            mask,
            select(f.cmp(op, a(), b()), Expr::U32(1 << bit), Expr::U32(0)),
        );
    }
    body.push(ret(mask));
    Func {
        name: CHECK.to_string(),
        params: ["a", "b", "c"]
            .iter()
            .map(|name| Param {
                name: name.to_string(),
                ty: Ty::F32,
                unused: false,
            })
            .chain([Param {
                name: "k".to_string(),
                ty: Ty::I32,
                unused: false,
            }])
            .collect(),
        ret: Ty::U32,
        body,
        strict: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(index: usize) -> [f32; 3] {
        INPUTS[index].map(f32::from_bits)
    }

    #[test]
    fn one_result_per_expression() {
        assert_eq!(function().body.len(), EXPRESSIONS as usize);
    }

    #[test]
    fn fused_multiply_add_changes_the_first_rows() {
        let [a, b, c] = row(0);
        assert_ne!(a * b + c, a.mul_add(b, c));
        let [a, b, c] = row(1);
        assert_ne!(a * b - c, a.mul_add(b, -c));
        let [a, b, c] = row(2);
        assert_ne!(a * b + c, a.mul_add(b, c));
    }

    #[test]
    fn each_choice_of_product_changes_a_sum_of_squares() {
        for index in [3, 4] {
            let [a, b, _] = row(index);
            let plain = a * a + b * b;
            assert_ne!(plain, a.mul_add(a, b * b), "row {index}");
            assert_ne!(plain, b.mul_add(b, a * a), "row {index}");
        }
        for index in [5, 6] {
            let [a, b, _] = row(index);
            assert_ne!(a.mul_add(a, b * b), b.mul_add(b, a * a), "row {index}");
        }
    }

    #[test]
    fn factoring_changes_a_sum_of_products() {
        for index in [7, 8] {
            let [a, b, c] = row(index);
            assert_ne!(a * b + b * c, b * (a + c), "row {index}");
        }
    }

    /// The exact quotient or root lies within 2^-16 of a unit in the last
    /// place of the midpoint between two `F32`s.
    fn near_midpoint(exact: f64) -> bool {
        let rounded = exact as f32;
        let ulp = f64::from(f32::from_bits(rounded.to_bits() + 1)) - f64::from(rounded);
        let midpoint = if exact > f64::from(rounded) {
            f64::from(rounded) + ulp / 2.0
        } else {
            f64::from(rounded) - ulp / 2.0
        };
        ((exact - midpoint) / ulp).abs() < 1.0 / 65_536.0
    }

    #[test]
    fn quotients_and_roots_lie_next_to_a_midpoint() {
        for index in 9..12 {
            let [a, b, _] = row(index);
            assert!(near_midpoint(f64::from(a) / f64::from(b)), "row {index}");
        }
        for index in 12..15 {
            let [a, _, _] = row(index);
            assert!(near_midpoint(f64::from(a).sqrt()), "row {index}");
        }
    }

    #[test]
    fn boundary_products_round_up_to_the_smallest_normal() {
        let smallest = f64::from(f32::MIN_POSITIVE);
        for index in [18, 19] {
            let [a, b, _] = row(index);
            assert!(a.is_normal() && b.is_normal(), "row {index}");
            let exact = (f64::from(a) * f64::from(b)).abs();
            assert!(
                exact < smallest && exact >= smallest * (1.0 - 1.0 / 33_554_432.0),
                "row {index}"
            );
        }
    }
}
