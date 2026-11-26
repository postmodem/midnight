//! The weight of a GPU block: an estimate of the work of one run, in
//! operations. The runtime uses it to divide the block's rows among
//! threads when the block runs on the CPU.
//!
//! - 1 for an arithmetic operation, a comparison, a field read from a
//!   column and a field written to one;
//! - 8 for `/`, `%` and a function of `Math`;
//! - for a branch, its condition and the heaviest of its arms;
//! - for a loop that is not the nested `each`, 8 times its body;
//! - for a call, the weight of the function, and 64 for one that recurses;
//! - for the nested `each`, the weight of its body for every row of the
//!   snapshot.
//!
//! Sums and products stop at 2^62.

use std::collections::HashMap;

use super::helpers;
use super::ir::{BinOp, Expr, Library, MathFn, Stmt, Ty};
use super::layout::WORD_SNAPSHOT_ROWS;
use super::lower::{Block, INNER_ROW};

/// Where sums and products stop.
const MOST: u64 = 1 << 62;
/// How often a loop that is not a query is taken to run.
const ROUNDS: u64 = 8;
const HEAVY: u64 = 8;
const RECURSION: u64 = 64;

/// `base`, plus `inner` for every row of the snapshot.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Weight {
    pub base: u64,
    pub inner: u64,
}

fn add(a: u64, b: u64) -> u64 {
    a.saturating_add(b).min(MOST)
}

fn mul(a: u64, b: u64) -> u64 {
    a.saturating_mul(b).min(MOST)
}

impl Weight {
    fn of(base: u64) -> Weight {
        Weight { base, inner: 0 }
    }

    fn plus(self, other: Weight) -> Weight {
        Weight {
            base: add(self.base, other.base),
            inner: add(self.inner, other.inner),
        }
    }

    fn times(self, count: u64) -> Weight {
        Weight {
            base: mul(self.base, count),
            inner: mul(self.inner, count),
        }
    }

    fn heavier(self, other: Weight) -> Weight {
        Weight {
            base: self.base.max(other.base),
            inner: self.inner.max(other.inner),
        }
    }
}

/// The weight of one run of a block. `base` is at least 1.
pub fn of_block(library: &Library, block: &Block) -> Weight {
    let mut weigher = Weigher {
        library,
        funcs: HashMap::new(),
    };
    let weight = weigher.stmts(&block.body);
    Weight {
        base: weight.base.max(1),
        inner: weight.inner,
    }
}

struct Weigher<'a> {
    library: &'a Library,
    /// The weight of each function weighed so far; `None` while it is
    /// being weighed.
    funcs: HashMap<String, Option<u64>>,
}

impl Weigher<'_> {
    fn func(&mut self, name: &str) -> u64 {
        match self.funcs.get(name) {
            Some(Some(weight)) => return *weight,
            Some(None) => return RECURSION,
            None => {}
        }
        self.funcs.insert(name.to_string(), None);
        let library = self.library;
        let weight = self.stmts(&library.get(name).body);
        let weight = add(weight.base, weight.inner);
        self.funcs.insert(name.to_string(), Some(weight));
        weight
    }

    fn exprs(&mut self, exprs: &[Expr]) -> u64 {
        exprs
            .iter()
            .fold(0, |total, expr| add(total, self.expr(expr)))
    }

    fn expr(&mut self, expr: &Expr) -> u64 {
        match expr {
            Expr::I32(_)
            | Expr::U32(_)
            | Expr::Hex(_)
            | Expr::F32(_)
            | Expr::F32Bits(_)
            | Expr::Bool(_)
            | Expr::Var(_)
            | Expr::Uniform(..)
            | Expr::Key
            | Expr::Draws
            | Expr::Site(_) => 0,
            Expr::Load(_) => 1,
            Expr::Field(inner, _)
            | Expr::Cast(_, inner)
            | Expr::Bits(_, inner)
            | Expr::Opaque(_, inner) => self.expr(inner),
            Expr::Unary(_, inner) => add(1, self.expr(inner)),
            Expr::Binary(op, lhs, rhs) => {
                let own = match op {
                    BinOp::Div | BinOp::Rem => HEAVY,
                    _ => 1,
                };
                add(own, add(self.expr(lhs), self.expr(rhs)))
            }
            Expr::Select { cond, then, else_ } => {
                add(self.expr(cond), add(self.expr(then), self.expr(else_)))
            }
            Expr::Math(function, args) => {
                let own = match function {
                    MathFn::Abs | MathFn::Floor | MathFn::Ceil | MathFn::Trunc => 1,
                    MathFn::Sqrt
                    | MathFn::Sin
                    | MathFn::Cos
                    | MathFn::Tan
                    | MathFn::Atan2
                    | MathFn::Exp
                    | MathFn::Log
                    | MathFn::Pow => HEAVY,
                };
                add(own, self.exprs(args))
            }
            // The strict `/` and `sqrt`, which C leaves to the hardware.
            Expr::Call(name, args) if helpers::SHADER_ONLY.contains(&name.as_str()) => {
                add(HEAVY, self.exprs(args))
            }
            Expr::Call(name, args) => add(self.func(name), self.exprs(args)),
            Expr::Construct(_, args) => self.exprs(args),
        }
    }

    fn stmts(&mut self, stmts: &[Stmt]) -> Weight {
        stmts
            .iter()
            .fold(Weight::default(), |total, stmt| total.plus(self.stmt(stmt)))
    }

    fn stmt(&mut self, stmt: &Stmt) -> Weight {
        match stmt {
            Stmt::Let { init, .. } => Weight::of(init.as_ref().map_or(0, |init| self.expr(init))),
            Stmt::Assign(_, value) => Weight::of(self.expr(value)),
            Stmt::Store(_, value) => Weight::of(add(1, self.expr(value))),
            Stmt::If(cond, then, else_) => {
                let arms = self.stmts(then).heavier(self.stmts(else_));
                Weight::of(self.expr(cond)).plus(arms)
            }
            Stmt::While(cond, body) => Weight::of(self.expr(cond))
                .plus(self.stmts(body))
                .times(ROUNDS),
            Stmt::Loop(body) => self.stmts(body).times(ROUNDS),
            Stmt::Once(body) => self.stmts(body),
            Stmt::For { var, to, body, .. }
                if var == INNER_ROW && *to == Expr::Uniform(WORD_SNAPSHOT_ROWS, Ty::U32) =>
            {
                let body = self.stmts(body);
                Weight {
                    base: 0,
                    inner: add(body.base, body.inner).max(1),
                }
            }
            Stmt::For { from, to, body, .. } => Weight::of(self.expr(from)).plus(
                Weight::of(self.expr(to))
                    .plus(self.stmts(body))
                    .times(ROUNDS),
            ),
            Stmt::Break | Stmt::Continue | Stmt::Return(None) | Stmt::Discard(_) => {
                Weight::default()
            }
            Stmt::Return(Some(value)) | Stmt::Call(value) => Weight::of(self.expr(value)),
            Stmt::Fail(_) => Weight::of(1),
        }
    }
}
