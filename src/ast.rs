//! The syntax tree the parser produces and the checker consumes.
//!
//! The tree follows the grammar in `docs/language.md` closely and resolves
//! nothing: a name is a string, and whether `Health` is a constant, a type or
//! a resource is for the checker to decide. The parser does three small
//! rewrites so that later stages see fewer forms:
//!
//! - `stmt if cond` and `stmt unless cond` become an [`ExprKind::If`] whose
//!   `then` block holds the one statement;
//! - `elsif` becomes an `else` block holding one `If`;
//! - a `-` directly before a number literal is folded into the literal
//!   (except before `**`, as the reference describes).

use crate::source::Span;

#[derive(Clone, Debug, PartialEq)]
pub struct Ident {
    pub name: String,
    pub span: Span,
}

/// A whole program: the root file's items, with each required file's items
/// in place of its first `require`. The parser returns the items of one file;
/// `source::load` splices files together and removes [`Item::Require`].
#[derive(Clone, Debug, PartialEq)]
pub struct Program {
    pub items: Vec<Item>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    Require(Require),
    Record(RecordDecl),
    Enum(EnumDecl),
    Tag(TagDecl),
    Def(FuncDecl),
    System(SystemDecl),
    /// `NAME = expr` at the top level, with a capitalized name.
    Const(ConstDecl),
    /// A top-level statement.
    Stmt(Stmt),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Require {
    /// The string as written, without `.12am`.
    pub path: String,
    /// The whole `require "…"`.
    pub span: Span,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecordKind {
    Struct,
    Component,
    Resource,
    Event,
}

/// `struct`, `component`, `resource` or `event`. The grammar gives methods
/// to structs and components only; the parser rejects them elsewhere.
#[derive(Clone, Debug, PartialEq)]
pub struct RecordDecl {
    pub kind: RecordKind,
    pub name: Ident,
    pub fields: Vec<FieldDecl>,
    pub methods: Vec<FuncDecl>,
    /// From the keyword to the end of the name.
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FieldDecl {
    pub name: Ident,
    pub ty: TypeExpr,
    pub default: Option<Expr>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EnumDecl {
    pub name: Ident,
    pub variants: Vec<Ident>,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TagDecl {
    pub name: Ident,
    pub span: Span,
}

/// A function, or a method when it appears inside a record.
#[derive(Clone, Debug, PartialEq)]
pub struct FuncDecl {
    /// For operator methods the name is the operator: `+`, `-`, `*`, `/`,
    /// `%` or `-@`.
    pub name: Ident,
    pub params: Vec<Param>,
    pub ret: Option<TypeExpr>,
    pub body: Block,
    /// From `def` to the end of the signature.
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Param {
    pub name: Ident,
    pub ty: TypeExpr,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SystemDecl {
    /// Declared `gpu system`.
    pub gpu: bool,
    pub name: Ident,
    pub body: Block,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ConstDecl {
    pub name: Ident,
    pub value: Expr,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TypeExpr {
    pub kind: TypeExprKind,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TypeExprKind {
    /// `Int`, `Vec2`, `Float32`, …
    Name(String),
    /// `[T]`
    List(Box<TypeExpr>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Block {
    pub stmts: Vec<Stmt>,
    /// From the first statement to the last; empty at the block's position
    /// when there are none.
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Stmt {
    pub kind: StmtKind,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub enum StmtKind {
    /// An expression used as a statement. `if`, `unless`, `case`, and the
    /// loop forms written as method calls with a `do` block (`3.times do`,
    /// `xs.each do |x|`) arrive this way.
    Expr(Expr),
    /// `name: Type = value`
    VarDecl {
        name: Ident,
        ty: TypeExpr,
        value: Expr,
    },
    /// `target = value`, `target += value`, … A target that is a bare
    /// [`ExprKind::Var`] not yet in scope creates the variable.
    Assign {
        target: Expr,
        op: AssignOp,
        value: Expr,
    },
    Return(Option<Expr>),
    Break,
    Next,
    Puts(Vec<Expr>),
    Print(Vec<Expr>),
    Emit(Expr),
    /// `while cond` or, with `negate`, `until cond`.
    While {
        cond: Expr,
        negate: bool,
        body: Block,
    },
    /// `loop do … end`
    Loop {
        body: Block,
    },
    /// `each Terms, with: …, without: … do |params| … end`
    Each {
        query: Query,
        params: Vec<Ident>,
        body: Block,
    },
    /// `on Event do |param| … end`
    On {
        event: Ident,
        params: Vec<Ident>,
        body: Block,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssignOp {
    Set,
    Add,
    Sub,
    Mul,
    Div,
    Mod,
}

/// The part of `each` between the keyword and `do`.
#[derive(Clone, Debug, PartialEq)]
pub struct Query {
    /// Component names, or `Entity`.
    pub terms: Vec<Ident>,
    pub with: Vec<Ident>,
    pub without: Vec<Ident>,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ExprKind {
    Int(i64),
    Float(f64),
    Bool(bool),
    /// A string literal. A single-quoted string, or a double-quoted one with
    /// no interpolation, is one `Lit` part (none when empty). Escapes are
    /// already decoded.
    Str(Vec<StrPart>),
    /// `[a, b, c]`
    List(Vec<Expr>),
    /// `self`
    SelfRef,
    /// A lowercase name with no argument list: a local, a field or method of
    /// `self`, or a function called without parentheses.
    Var(String),
    /// A capitalized name with no argument list: a constant, a type (as in
    /// `e[Health]` or `spawn Player`), a resource or a built-in namespace.
    Const(String),
    /// `name(args)` with a lowercase name: always a function call.
    Call {
        name: Ident,
        args: Vec<Arg>,
    },
    /// `Type(args)`: a constructor.
    Construct {
        name: Ident,
        args: Vec<Arg>,
    },
    /// `recv.name`, `recv.name(args)`, and either with a `do` block.
    /// `args` is `None` when no parentheses were written.
    Member {
        recv: Box<Expr>,
        name: Ident,
        args: Option<Vec<Arg>>,
        block: Option<DoBlock>,
    },
    /// `recv[index]`
    Index {
        recv: Box<Expr>,
        index: Box<Expr>,
    },
    /// `recv::Name`
    Path {
        recv: Box<Expr>,
        name: Ident,
    },
    Unary {
        op: UnaryOp,
        expr: Box<Expr>,
    },
    Binary {
        op: BinaryOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
    /// `cond ? then : else_`
    Ternary {
        cond: Box<Expr>,
        then: Box<Expr>,
        else_: Box<Expr>,
    },
    /// `spawn a, b` or `spawn(a, b)`.
    Spawn(Vec<Expr>),
    /// `if` or, with `negate`, `unless`.
    If {
        cond: Box<Expr>,
        negate: bool,
        then: Block,
        else_: Option<Block>,
    },
    Case {
        subject: Box<Expr>,
        arms: Vec<CaseArm>,
        else_: Option<Block>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum StrPart {
    Lit(String),
    /// `#{expr}`
    Expr(Expr),
}

/// One argument of a call or constructor, with its keyword if it has one.
#[derive(Clone, Debug, PartialEq)]
pub struct Arg {
    pub name: Option<Ident>,
    pub value: Expr,
}

/// `do |params| … end`
#[derive(Clone, Debug, PartialEq)]
pub struct DoBlock {
    pub params: Vec<Ident>,
    pub body: Block,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CaseArm {
    pub values: Vec<Expr>,
    pub body: Block,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,
    Not,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
}
