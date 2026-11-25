//! The form GPU code takes between the typed tree and its two printers.
//!
//! Everything here means the same in C and in WGSL. Expressions have no
//! effects apart from calls, and the lowering puts every call whose effect
//! matters (a user function, a random draw) into a statement of its own,
//! so the order of evaluation never depends on the target. Arithmetic is
//! 32-bit: `I32` and `U32` wrap, `F32` is IEEE single precision.

use std::collections::{BTreeSet, HashMap};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Ty {
    Void,
    I32,
    U32,
    F32,
    Bool,
    /// `MnEntity`: two `U32`s, `index` and `generation`.
    Entity,
    /// The 32-bit form of a record, by record id.
    Record(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnOp {
    Neg,
    Not,
}

/// Both operands have the same type, except for the shifts, whose right
/// operand is a `U32`. `Div` on `I32` truncates and is only used where the
/// divisor is neither 0 nor -1; `Rem` is only used on `U32`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    /// Short-circuit.
    And,
    /// Short-circuit.
    Or,
    BitAnd,
    BitXor,
    Shl,
    /// Arithmetic on `I32`, logical on `U32`.
    Shr,
}

/// Functions of `F32` that both targets provide.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MathFn {
    Sqrt,
    Sin,
    Cos,
    Tan,
    Atan2,
    Exp,
    Log,
    /// Only called with a positive base.
    Pow,
    Abs,
    Floor,
    Ceil,
    Trunc,
}

/// How a field is stored in a column.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Storage {
    F32,
    I8,
    I16,
    I32,
    U8,
    U16,
    /// A `UInt32`: a word that GPU code carries as an `I32` with the same
    /// bits and never computes with.
    U32,
    /// One byte, 0 or 1.
    Bool,
    /// An unsigned integer of this many bytes: 1, 2 or 4.
    Enum(u32),
    /// Two words: index, then generation.
    Entity,
}

impl Storage {
    pub fn bytes(self) -> u32 {
        match self {
            Storage::I8 | Storage::U8 | Storage::Bool => 1,
            Storage::I16 | Storage::U16 => 2,
            Storage::F32 | Storage::I32 | Storage::U32 => 4,
            Storage::Enum(bytes) => bytes,
            Storage::Entity => 8,
        }
    }

    /// The type of the value a load gives and a store takes.
    pub fn ty(self) -> Ty {
        match self {
            Storage::F32 => Ty::F32,
            Storage::I8
            | Storage::I16
            | Storage::I32
            | Storage::U8
            | Storage::U16
            | Storage::U32 => Ty::I32,
            Storage::Bool => Ty::Bool,
            Storage::Enum(_) => Ty::U32,
            Storage::Entity => Ty::Entity,
        }
    }
}

/// One field of one row of a column: of the run's own row for an outer
/// column, of the row the nested loop is at for an inner one.
#[derive(Clone, Debug, PartialEq)]
pub struct Cell {
    pub inner: bool,
    /// Index into the block's outer or inner columns.
    pub column: usize,
    /// The member in C, written after the row: `.pos.x`, or empty for the
    /// whole row.
    pub member: String,
    /// Byte offset of the field in the row.
    pub offset: u32,
    /// Bytes per row.
    pub row_size: u32,
    pub storage: Storage,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    I32(i32),
    U32(u32),
    /// A `U32` written in hexadecimal: a mask or a bit pattern.
    Hex(u32),
    /// Finite.
    F32(f32),
    /// An `F32` given by its bits: an infinity or a NaN.
    F32Bits(u32),
    Bool(bool),
    /// A local variable, a parameter or a temporary.
    Var(String),
    /// A member of a record or entity value.
    Field(Box<Expr>, String),
    /// One of the block's uniform words, as an `I32`, `U32` or `F32`.
    Uniform(u32, Ty),
    /// The hash of everything `rand` depends on except the draw count.
    Key,
    /// How many random numbers the run has drawn.
    Draws,
    /// The index of a failure site in the program's table, as a `U32`.
    /// Holds the order of discovery until the table is sorted.
    Site(usize),
    Load(Cell),
    Unary(UnOp, Box<Expr>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
    /// `then` when `cond`, else `else_`. Both are evaluated; scalars only.
    Select {
        cond: Box<Expr>,
        then: Box<Expr>,
        else_: Box<Expr>,
    },
    /// Conversion to `I32`, `U32` or `F32`. Between the integers the bits
    /// are kept; a `Bool` gives 0 or 1; an `F32` is only converted to
    /// `I32` when it is a whole number in range.
    Cast(Ty, Box<Expr>),
    /// The same bits as another type: `F32` from `U32` or the reverse.
    Bits(Ty, Box<Expr>),
    Math(MathFn, Vec<Expr>),
    /// A call of a function in the library.
    Call(String, Vec<Expr>),
    /// A record or entity value from its fields in order.
    Construct(Ty, Vec<Expr>),
}

impl Expr {
    pub fn is_literal(&self) -> bool {
        matches!(
            self,
            Expr::I32(_)
                | Expr::U32(_)
                | Expr::Hex(_)
                | Expr::F32(_)
                | Expr::F32Bits(_)
                | Expr::Bool(_)
        )
    }

    /// The value of an `F32` literal.
    pub fn as_f32(&self) -> Option<f32> {
        match self {
            Expr::F32(value) => Some(*value),
            Expr::F32Bits(bits) => Some(f32::from_bits(*bits)),
            _ => None,
        }
    }

    /// Whether the expression is a name or a member of one: cheap and
    /// safe to repeat.
    pub fn is_path(&self) -> bool {
        match self {
            Expr::Var(_) | Expr::Uniform(..) => true,
            Expr::Field(base, _) => base.is_path(),
            _ => false,
        }
    }

    /// Whether evaluating the expression can record a failure: it names a
    /// site.
    pub fn can_fail(&self) -> bool {
        match self {
            Expr::Site(_) => true,
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
            | Expr::Load(_) => false,
            Expr::Field(inner, _)
            | Expr::Unary(_, inner)
            | Expr::Cast(_, inner)
            | Expr::Bits(_, inner) => inner.can_fail(),
            Expr::Binary(_, lhs, rhs) => lhs.can_fail() || rhs.can_fail(),
            Expr::Select { cond, then, else_ } => {
                cond.can_fail() || then.can_fail() || else_.can_fail()
            }
            Expr::Math(_, args) | Expr::Call(_, args) | Expr::Construct(_, args) => {
                args.iter().any(Expr::can_fail)
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Stmt {
    /// Declares a variable. Without `init` it starts as all zeros.
    /// `unused` says nothing reads it.
    Let {
        name: String,
        ty: Ty,
        init: Option<Expr>,
        unused: bool,
    },
    /// The target is a variable, a member of one, `Key` or `Draws`.
    Assign(Expr, Expr),
    Store(Cell, Expr),
    If(Expr, Vec<Stmt>, Vec<Stmt>),
    While(Expr, Vec<Stmt>),
    /// Repeats until a `Break`.
    Loop(Vec<Stmt>),
    /// Runs once. A `Break` directly inside ends it early.
    Once(Vec<Stmt>),
    /// Counts `var` from `from` by one, up or down, while `var op to`.
    /// `to` is evaluated before every round and must not change.
    For {
        var: String,
        ty: Ty,
        from: Expr,
        op: BinOp,
        to: Expr,
        down: bool,
        body: Vec<Stmt>,
    },
    Break,
    Continue,
    Return(Option<Expr>),
    /// A call whose value, if any, is not used.
    Call(Expr),
    /// Evaluates nothing; marks a value as deliberately unused.
    Discard(Expr),
    /// Lowers the error word to this site.
    Fail(Expr),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Param {
    pub name: String,
    pub ty: Ty,
    pub unused: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Func {
    pub name: String,
    pub params: Vec<Param>,
    pub ret: Ty,
    pub body: Vec<Stmt>,
}

/// The 32-bit form of a record: the same fields in source order, each
/// with the type its value has in GPU code.
#[derive(Clone, Debug, PartialEq)]
pub struct Struct {
    pub name: String,
    pub fields: Vec<(String, Ty)>,
}

/// Every function GPU code can call: the 32-bit forms of the program's
/// functions and the helpers, in the order they were first needed.
#[derive(Default)]
pub struct Library {
    pub funcs: Vec<Func>,
    index: HashMap<String, usize>,
}

impl Library {
    pub fn has(&self, name: &str) -> bool {
        self.index.contains_key(name)
    }

    pub fn add(&mut self, func: Func) {
        assert!(
            !self.has(&func.name),
            "`{}` is defined twice in the GPU library",
            func.name
        );
        self.index.insert(func.name.clone(), self.funcs.len());
        self.funcs.push(func);
    }

    pub fn get(&self, name: &str) -> &Func {
        &self.funcs[self.index[name]]
    }
}

/// What a piece of code uses, directly.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Uses {
    /// Library functions it calls.
    pub calls: BTreeSet<String>,
    /// Record types it names.
    pub records: BTreeSet<usize>,
    pub entity: bool,
    /// Uniform words it reads.
    pub uniforms: BTreeSet<u32>,
    /// It reads or sets `Key` or `Draws`.
    pub rand_state: bool,
    pub fails: bool,
}

impl Uses {
    pub fn merge(&mut self, other: &Uses) {
        self.calls.extend(other.calls.iter().cloned());
        self.records.extend(&other.records);
        self.entity |= other.entity;
        self.uniforms.extend(&other.uniforms);
        self.rand_state |= other.rand_state;
        self.fails |= other.fails;
    }

    /// Whether the code needs the per-run state C passes as `g`.
    pub fn needs_state(&self) -> bool {
        !self.uniforms.is_empty() || self.rand_state || self.fails
    }

    fn ty(&mut self, ty: Ty) {
        match ty {
            Ty::Entity => self.entity = true,
            Ty::Record(id) => {
                self.records.insert(id);
            }
            _ => {}
        }
    }

    fn expr(&mut self, expr: &Expr) {
        match expr {
            Expr::I32(_)
            | Expr::U32(_)
            | Expr::Hex(_)
            | Expr::F32(_)
            | Expr::F32Bits(_)
            | Expr::Bool(_)
            | Expr::Var(_)
            | Expr::Site(_) => {}
            Expr::Field(base, _) => self.expr(base),
            Expr::Uniform(word, _) => {
                self.uniforms.insert(*word);
            }
            Expr::Key | Expr::Draws => self.rand_state = true,
            Expr::Load(cell) => self.ty(cell.storage.ty()),
            Expr::Unary(_, inner) | Expr::Cast(_, inner) | Expr::Bits(_, inner) => self.expr(inner),
            Expr::Binary(_, lhs, rhs) => {
                self.expr(lhs);
                self.expr(rhs);
            }
            Expr::Select { cond, then, else_ } => {
                self.expr(cond);
                self.expr(then);
                self.expr(else_);
            }
            Expr::Math(_, args) => args.iter().for_each(|arg| self.expr(arg)),
            Expr::Call(name, args) => {
                self.calls.insert(name.clone());
                args.iter().for_each(|arg| self.expr(arg));
            }
            Expr::Construct(ty, args) => {
                self.ty(*ty);
                args.iter().for_each(|arg| self.expr(arg));
            }
        }
    }

    fn stmts(&mut self, stmts: &[Stmt]) {
        for stmt in stmts {
            match stmt {
                Stmt::Let { ty, init, .. } => {
                    self.ty(*ty);
                    if let Some(init) = init {
                        self.expr(init);
                    }
                }
                Stmt::Assign(target, value) => {
                    self.expr(target);
                    self.expr(value);
                }
                Stmt::Store(cell, value) => {
                    self.ty(cell.storage.ty());
                    self.expr(value);
                }
                Stmt::If(cond, then, else_) => {
                    self.expr(cond);
                    self.stmts(then);
                    self.stmts(else_);
                }
                Stmt::While(cond, body) => {
                    self.expr(cond);
                    self.stmts(body);
                }
                Stmt::Loop(body) | Stmt::Once(body) => self.stmts(body),
                Stmt::For { from, to, body, .. } => {
                    self.expr(from);
                    self.expr(to);
                    self.stmts(body);
                }
                Stmt::Break | Stmt::Continue | Stmt::Return(None) => {}
                Stmt::Return(Some(value)) | Stmt::Call(value) | Stmt::Discard(value) => {
                    self.expr(value);
                }
                Stmt::Fail(site) => {
                    self.fails = true;
                    self.expr(site);
                }
            }
        }
    }

    pub fn of_stmts(stmts: &[Stmt]) -> Uses {
        let mut uses = Uses::default();
        uses.stmts(stmts);
        uses
    }

    pub fn of_func(func: &Func) -> Uses {
        let mut uses = Uses::of_stmts(&func.body);
        for param in &func.params {
            uses.ty(param.ty);
        }
        uses.ty(func.ret);
        uses
    }
}

/// What a piece of code uses, itself and through every function it can
/// reach. `calls` lists those functions.
pub fn closure(library: &Library, direct: &Uses) -> Uses {
    let mut all = direct.clone();
    let mut pending: Vec<String> = direct.calls.iter().cloned().collect();
    while let Some(name) = pending.pop() {
        let uses = Uses::of_func(library.get(&name));
        for callee in &uses.calls {
            if all.calls.insert(callee.clone()) {
                pending.push(callee.clone());
            }
        }
        all.merge(&uses);
    }
    all
}

// Shorthand for building expressions and statements by hand.

pub fn var(name: &str) -> Expr {
    Expr::Var(name.to_string())
}

pub fn field(base: Expr, name: &str) -> Expr {
    Expr::Field(Box::new(base), name.to_string())
}

pub fn unary(op: UnOp, expr: Expr) -> Expr {
    Expr::Unary(op, Box::new(expr))
}

pub fn binary(op: BinOp, lhs: Expr, rhs: Expr) -> Expr {
    Expr::Binary(op, Box::new(lhs), Box::new(rhs))
}

pub fn cast(ty: Ty, expr: Expr) -> Expr {
    Expr::Cast(ty, Box::new(expr))
}

pub fn bits(ty: Ty, expr: Expr) -> Expr {
    Expr::Bits(ty, Box::new(expr))
}

pub fn call(name: &str, args: Vec<Expr>) -> Expr {
    Expr::Call(name.to_string(), args)
}

pub fn select(cond: Expr, then: Expr, else_: Expr) -> Expr {
    Expr::Select {
        cond: Box::new(cond),
        then: Box::new(then),
        else_: Box::new(else_),
    }
}

pub fn let_(name: &str, ty: Ty, init: Expr) -> Stmt {
    Stmt::Let {
        name: name.to_string(),
        ty,
        init: Some(init),
        unused: false,
    }
}

pub fn assign(target: Expr, value: Expr) -> Stmt {
    Stmt::Assign(target, value)
}

pub fn ret(value: Expr) -> Stmt {
    Stmt::Return(Some(value))
}

pub fn if_(cond: Expr, then: Vec<Stmt>) -> Stmt {
    Stmt::If(cond, then, Vec::new())
}

/// An `F32` literal for any value.
pub fn float(value: f32) -> Expr {
    if value.is_finite() {
        Expr::F32(value)
    } else {
        Expr::F32Bits(value.to_bits())
    }
}

/// `Add`, `Sub` or `Mul` on `I32`, computed here when both are literals.
pub fn int_arith(op: BinOp, lhs: Expr, rhs: Expr) -> Expr {
    if let (Expr::I32(a), Expr::I32(b)) = (&lhs, &rhs) {
        match op {
            BinOp::Add => return Expr::I32(a.wrapping_add(*b)),
            BinOp::Sub => return Expr::I32(a.wrapping_sub(*b)),
            BinOp::Mul => return Expr::I32(a.wrapping_mul(*b)),
            _ => {}
        }
    }
    binary(op, lhs, rhs)
}

/// `Add`, `Sub`, `Mul` or `Div` on `F32`, computed here when both are
/// literals: a shader compiler rejects a constant expression that
/// overflows.
pub fn float_arith(op: BinOp, lhs: Expr, rhs: Expr) -> Expr {
    if let (Some(a), Some(b)) = (lhs.as_f32(), rhs.as_f32()) {
        match op {
            BinOp::Add => return float(a + b),
            BinOp::Sub => return float(a - b),
            BinOp::Mul => return float(a * b),
            BinOp::Div => return float(a / b),
            _ => {}
        }
    }
    binary(op, lhs, rhs)
}

/// A comparison, computed here when both are literals.
pub fn compare(op: BinOp, lhs: Expr, rhs: Expr) -> Expr {
    fn pick<T: PartialOrd>(op: BinOp, a: T, b: T) -> bool {
        match op {
            BinOp::Eq => a == b,
            BinOp::Ne => a != b,
            BinOp::Lt => a < b,
            BinOp::Le => a <= b,
            BinOp::Gt => a > b,
            BinOp::Ge => a >= b,
            _ => unreachable!("not a comparison"),
        }
    }
    let folded = match (&lhs, &rhs) {
        (Expr::I32(a), Expr::I32(b)) => Some(pick(op, a, b)),
        (Expr::U32(a), Expr::U32(b)) => Some(pick(op, a, b)),
        (Expr::Bool(a), Expr::Bool(b)) => Some(pick(op, a, b)),
        _ => match (lhs.as_f32(), rhs.as_f32()) {
            (Some(a), Some(b)) => Some(pick(op, a, b)),
            _ => None,
        },
    };
    match folded {
        Some(value) => Expr::Bool(value),
        None => binary(op, lhs, rhs),
    }
}

pub fn not(expr: Expr) -> Expr {
    match expr {
        Expr::Bool(value) => Expr::Bool(!value),
        other => unary(UnOp::Not, other),
    }
}

/// `And` or `Or`, decided here as far as a literal operand allows.
pub fn logic(op: BinOp, lhs: Expr, rhs: Expr) -> Expr {
    let and = op == BinOp::And;
    match (&lhs, &rhs) {
        (Expr::Bool(value), _) if *value == and => rhs,
        (Expr::Bool(_), _) => lhs,
        (_, Expr::Bool(value)) if *value == and => lhs,
        _ => binary(op, lhs, rhs),
    }
}

/// A `Select`, chosen here when the condition is a literal.
pub fn choose(cond: Expr, then: Expr, else_: Expr) -> Expr {
    match cond {
        Expr::Bool(true) => then,
        Expr::Bool(false) => else_,
        cond => select(cond, then, else_),
    }
}
