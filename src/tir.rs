//! The typed program: what the checker produces and the only thing a
//! backend reads.
//!
//! Everything here is resolved. Names are ids, every expression carries its
//! type, every implicit conversion is an explicit node, omitted constructor
//! fields are filled in with their defaults, and syntax that is shorthand
//! for something else has been rewritten:
//!
//! | Source                                 | Here                                   |
//! | -------------------------------------- | -------------------------------------- |
//! | `unless`, `until`, modifiers, `c ? a : b` | [`ExprKind::If`], [`StmtKind::While`] with [`UnOp::Not`] |
//! | `loop do`                              | `While` with a `true` condition        |
//! | `case`                                 | a hidden local holding the subject, then a chain of `If` |
//! | `"a#{b}c"`                             | [`ExprKind::Concat`] of strings, non-strings wrapped in [`Builtin::ToS`] |
//! | `a + b` on a struct with `def +`       | [`ExprKind::Call`]                     |
//! | a field or method of `self` used bare  | a [`PlaceKind::Field`] of the `self` local, or a `Call` |
//! | `Math::PI`, `Math::E`                  | [`ExprKind::Float`]                    |
//! | `spawn Health` with all defaults       | a [`ComponentInit`] with a `Construct` |
//!
//! # Types of expressions and of storage
//!
//! The type of an [`Expr`] is a *value type*: never [`Type::Compact`].
//! Compact types appear only as the declared type of a field
//! ([`Field::ty`]) or as the element of a list type, and in [`Place::ty`],
//! which is the type of the storage the place names. Reading a compact
//! place gives `Int` or `Float`; a backend converts on load and on store.
//! A store of an `Int` into a compact integer place is range-checked at run
//! time (the checker has already rejected constants that can't fit).
//!
//! Wherever a value is stored into something with a declared type (a field
//! in `Construct`, an element in `ListLit`, the value of `Assign`, an
//! argument, a returned value) its expression has the value type of the
//! destination: an `Int` going into a `Float` is wrapped in
//! [`ExprKind::IntToFloat`] by the checker.
//!
//! # Ownership, for backends
//!
//! Strings and lists are reference counted. The checker says nothing about
//! counts; the tree only fixes evaluation order (left to right, the
//! right-hand side of an assignment before the place is written) and which
//! locals belong to which block: a local lives from its `Let` (or from the
//! loop that binds it) to the end of the enclosing [`Block`].

use crate::source::Span;

macro_rules! id {
    ($($(#[$doc:meta])* $name:ident),* $(,)?) => {
        $(
            $(#[$doc])*
            #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
            pub struct $name(pub usize);
        )*
    };
}

id! {
    /// Index into [`Program::records`].
    RecordId,
    /// Index into [`Program::enums`].
    EnumId,
    /// Index into [`Program::components`]. Also the component's id at run time.
    ComponentId,
    /// Index into [`Program::resources`].
    ResourceId,
    /// Index into [`Program::events`]. Also the event's id at run time.
    EventId,
    /// Index into [`Program::consts`].
    ConstId,
    /// Index into [`Program::funcs`].
    FuncId,
    /// Index into the `locals` of the enclosing [`Body`].
    LocalId,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Type {
    /// No value: a statement used as an expression, or a function that
    /// returns nothing.
    Void,
    Int,
    Float,
    Bool,
    String,
    Entity,
    /// Storage only. See the module documentation.
    Compact(Compact),
    /// The element is a storage type and may be compact.
    List(Box<Type>),
    /// A struct, a component with fields, or an event.
    Record(RecordId),
    Enum(EnumId),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Compact {
    Float32,
    Int8,
    Int16,
    Int32,
    UInt8,
    UInt16,
    UInt32,
}

impl Compact {
    pub fn name(self) -> &'static str {
        match self {
            Compact::Float32 => "Float32",
            Compact::Int8 => "Int8",
            Compact::Int16 => "Int16",
            Compact::Int32 => "Int32",
            Compact::UInt8 => "UInt8",
            Compact::UInt16 => "UInt16",
            Compact::UInt32 => "UInt32",
        }
    }

    /// Smallest and largest `Int` a compact integer type holds.
    /// `None` for `Float32`.
    pub fn int_range(self) -> Option<(i64, i64)> {
        match self {
            Compact::Float32 => None,
            Compact::Int8 => Some((i8::MIN as i64, i8::MAX as i64)),
            Compact::Int16 => Some((i16::MIN as i64, i16::MAX as i64)),
            Compact::Int32 => Some((i32::MIN as i64, i32::MAX as i64)),
            Compact::UInt8 => Some((0, u8::MAX as i64)),
            Compact::UInt16 => Some((0, u16::MAX as i64)),
            Compact::UInt32 => Some((0, u32::MAX as i64)),
        }
    }
}

impl Type {
    /// The type a read of this storage type gives: `Float` for `Float32`,
    /// `Int` for the compact integers, the type itself otherwise.
    pub fn value_type(&self) -> Type {
        match self {
            Type::Compact(Compact::Float32) => Type::Float,
            Type::Compact(_) => Type::Int,
            other => other.clone(),
        }
    }
}

pub struct Program {
    pub records: Vec<Record>,
    pub enums: Vec<Enum>,
    /// Components and tags, in source order.
    pub components: Vec<Component>,
    /// User resources. The built-in `Time` is not listed; see
    /// [`PlaceKind::Time`].
    pub resources: Vec<Resource>,
    pub events: Vec<Event>,
    /// In definition order. A constant's `init` may read earlier constants.
    pub consts: Vec<Const>,
    /// Functions and methods.
    pub funcs: Vec<Func>,
    /// In the order `World.tick` runs them.
    pub systems: Vec<System>,
    /// The top-level statements, in order.
    pub main: Body,
}

/// A struct, a component with fields, or an event: a value with named
/// fields. Records never contain themselves by value (only through a list).
#[derive(Clone, Debug)]
pub struct Record {
    pub name: String,
    /// What the record was declared as. No backend reads it yet.
    #[allow(dead_code)]
    pub kind: RecordKind,
    /// In source order, which is the order of construction and of `to_s`.
    /// An event may have none.
    pub fields: Vec<Field>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecordKind {
    Struct,
    Component(ComponentId),
    Event(EventId),
}

#[derive(Clone, Debug)]
pub struct Field {
    pub name: String,
    /// A storage type.
    pub ty: Type,
}

#[derive(Clone, Debug)]
pub struct Enum {
    pub name: String,
    /// At least one. The first is the all-zero value.
    pub variants: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct Component {
    pub name: String,
    /// `None` for a tag.
    pub record: Option<RecordId>,
}

#[derive(Clone, Debug)]
pub struct Resource {
    pub name: String,
    pub fields: Vec<ResourceField>,
}

#[derive(Clone, Debug)]
pub struct ResourceField {
    pub name: String,
    /// A storage type.
    pub ty: Type,
    /// The default, a constant expression. Evaluated once at program start,
    /// after every constant.
    pub init: Expr,
}

#[derive(Clone, Debug)]
pub struct Event {
    pub name: String,
    pub record: RecordId,
}

#[derive(Clone, Debug)]
pub struct Const {
    pub name: String,
    pub ty: Type,
    /// A constant expression: no locals, no calls to user functions, no
    /// access to the world. Evaluated once at program start, in order.
    pub init: Expr,
}

/// Code with its local variables: a function, a system or the top level.
#[derive(Clone, Debug)]
pub struct Body {
    pub locals: Vec<Local>,
    pub block: Block,
}

#[derive(Clone, Debug)]
pub struct Local {
    /// The source name. Hidden locals made by the checker have names that
    /// start with `$`. Names are not unique within a body.
    pub name: String,
    /// A value type. For a `View`, the component's record.
    pub ty: Type,
    pub kind: LocalKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LocalKind {
    /// An ordinary variable, created by `Let` or bound by a loop.
    Var,
    /// A function or method parameter, `self` included. Never assigned.
    Param,
    /// A component parameter of a query `each`: not a copy but the
    /// entity's component itself, for the current row.
    View,
}

#[derive(Clone, Debug)]
pub struct Func {
    /// The source name: `clamp`, `alive?`, `+`, `-@`.
    pub name: String,
    /// For a method, the record it belongs to. `params[0]` is then `self`.
    pub owner: Option<RecordId>,
    /// Locals of `body` of kind `Param`, in order.
    pub params: Vec<LocalId>,
    /// `Void` when the function returns nothing.
    pub ret: Type,
    /// When `ret` is not `Void`, every path through the block ends in a
    /// `Return` with a value or in the block's `value`.
    pub body: Body,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct System {
    pub name: String,
    /// Declared `gpu system`. The statements of the body are then all
    /// `Each`, and the checker has applied the rules for GPU systems.
    pub gpu: bool,
    /// Declared `strict gpu system`: its `Float` arithmetic gives the same
    /// bits on every device and on the CPU. Never set without `gpu`.
    pub strict: bool,
    pub body: Body,
    pub span: Span,
}

/// Statements, then optionally a value.
#[derive(Clone, Debug)]
pub struct Block {
    pub stmts: Vec<Stmt>,
    /// The block's value: present exactly when the block is used for one
    /// (a function body that returns its last expression, a branch of an
    /// `If` whose type is not `Void`).
    pub value: Option<Box<Expr>>,
}

#[derive(Clone, Debug)]
pub struct Stmt {
    pub kind: StmtKind,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum StmtKind {
    /// Evaluate and discard.
    Expr(Expr),
    /// Create a local of kind `Var` with its first value.
    Let {
        local: LocalId,
        init: Expr,
    },
    /// `place = value`.
    Assign {
        place: Place,
        value: Expr,
    },
    /// `place op= value`, which means `place = place op value`. The
    /// operands of the place (an entity, indexes) are evaluated once, then
    /// the place is read, then `value` is evaluated, and the result is
    /// written to the place.
    CompoundAssign {
        place: Place,
        op: CompoundOp,
        value: Expr,
    },
    /// The value is present exactly when the function returns one.
    /// In a system or at the top level it is absent; at the top level
    /// `return` ends the program normally.
    Return(Option<Expr>),
    /// Leave the nearest enclosing `While`, `Count`, `ListEach`, `Each` or
    /// `On`.
    Break,
    /// Skip to the next iteration of that loop.
    Next,
    While {
        cond: Expr,
        body: Block,
    },
    /// `n.times`, `a.upto(b)`, `a.downto(b)`. `from` and `to` are `Int`s
    /// evaluated once, `from` first. Counts up from `from` while below `to`
    /// (or up to and including it when `inclusive`); with `down`, counts
    /// down from `from` to `to`.
    Count {
        var: Option<LocalId>,
        from: Expr,
        to: Expr,
        inclusive: bool,
        down: bool,
        body: Block,
    },
    /// `list.each` and `list.each_with_index`: walks the list as it was
    /// when the loop started. `elem` gets a copy of each element (its type
    /// is the element's value type) and `index` its position.
    ListEach {
        list: Expr,
        elem: Option<LocalId>,
        index: Option<LocalId>,
        body: Block,
    },
    /// A query loop. `params` has one local per term, in order: a `Var` of
    /// type `Entity` for [`Term::Entity`], a `View` for a component.
    /// Structural changes made while the loop runs are deferred until the
    /// outermost `Each` is left.
    Each {
        query: Query,
        params: Vec<LocalId>,
        body: Block,
    },
    /// Runs the body for each event this site has not seen yet. `param`
    /// gets a copy of the event. Each `On` in the tree is one site with its
    /// own position in the event stream.
    On {
        event: EventId,
        param: Option<LocalId>,
        body: Block,
    },
    /// Write each argument's `to_s`, each followed by a newline; a single
    /// newline when there are none. Arguments may have any value type.
    Puts(Vec<Expr>),
    /// Write each argument's `to_s`.
    Print(Vec<Expr>),
    /// Send an event. `value` has the type of the event's record.
    Emit {
        event: EventId,
        value: Expr,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompoundOp {
    /// A built-in operator. `value` has the type that [`ExprKind::Binary`]
    /// would require of its right operand when the left is a read of the
    /// place.
    Builtin(BinOp),
    /// An operator method of the place's record: `place = place.op(value)`.
    Method(FuncId),
}

#[derive(Clone, Debug)]
pub struct Query {
    pub terms: Vec<Term>,
    /// Further components the entity must have. Tags go here.
    pub with: Vec<ComponentId>,
    /// Components the entity must not have.
    pub without: Vec<ComponentId>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Term {
    Entity,
    Component {
        component: ComponentId,
        /// Whether the body assigns through this term's parameter. A term
        /// that is only read can be a `const` column.
        write: bool,
        /// Whether the body names this term's parameter at all. The column
        /// of a term that is not touched is neither made current nor
        /// transferred: the term only picks the tables.
        touched: bool,
        /// Whether the body assigns the whole component, in every run,
        /// before anything can read it, as "The transfer plan" in the
        /// reference defines it: the column's earlier values are never
        /// needed. Only an outer term of a GPU block or a term of an
        /// ordinary `each` can be.
        overwritten: bool,
    },
}

/// Something that can be read and, unless the checker ruled it out,
/// assigned or modified in place.
#[derive(Clone, Debug)]
pub struct Place {
    pub kind: PlaceKind,
    /// The storage type: may be compact for a field or a list element.
    pub ty: Type,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum PlaceKind {
    Local(LocalId),
    /// Read-only.
    Const(ConstId),
    /// A field of a user resource, by index into [`Resource::fields`].
    ResourceField(ResourceId, usize),
    /// A field of the built-in `Time`. Only `Dt` is ever written.
    Time(TimeField),
    /// `entity[Component]`. A run-time error if the entity is dead or lacks
    /// the component. Never a tag.
    Component {
        entity: Box<Expr>,
        component: ComponentId,
        /// Whether this use writes the component.
        write: bool,
    },
    /// A field of a record place, by index into [`Record::fields`].
    Field {
        base: Box<Place>,
        field: usize,
    },
    /// An element of a list place. `index` is an `Int`; negative counts
    /// from the end; out of range is a run-time error.
    Index {
        base: Box<Place>,
        index: Box<Expr>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimeField {
    /// `Int`
    Tick,
    /// `Float`
    Dt,
    /// `Float`
    Elapsed,
}

#[derive(Clone, Debug)]
pub struct Expr {
    pub kind: ExprKind,
    /// A value type.
    pub ty: Type,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum ExprKind {
    Int(i64),
    Float(f64),
    Bool(bool),
    /// A string literal: its bytes, escapes decoded.
    Str(String),
    /// The type is the `Enum`; the number indexes [`Enum::variants`].
    EnumVariant(usize),
    /// `Entity.none`
    EntityNone,

    /// The current value of a place, as a copy. Any read whose base is a
    /// place is expressed this way, so `pos.x` on a query parameter is a
    /// `Read` of a `Field` of a `Local`.
    Read(Place),
    /// A field of a record that is not a place, such as a call's result.
    Field {
        base: Box<Expr>,
        field: usize,
    },
    /// An element of a list that is not a place. Checked like
    /// [`PlaceKind::Index`].
    Index {
        base: Box<Expr>,
        index: Box<Expr>,
    },

    /// A list literal. The type is the list type; each element has the
    /// value type of the list's element.
    ListLit(Vec<Expr>),
    /// A record value. One expression per field, in source order, each with
    /// the field's value type. The type is the `Record`.
    Construct(Vec<Expr>),
    /// Strings joined in order. Every part is a `String`.
    Concat(Vec<Expr>),

    Unary {
        op: UnOp,
        expr: Box<Expr>,
    },
    /// A built-in operator. See [`BinOp`] for the operand types.
    Binary {
        op: BinOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
    /// `&&`: `rhs` is evaluated only when `lhs` is true.
    And {
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
    /// `||`: `rhs` is evaluated only when `lhs` is false.
    Or {
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
    /// An `Int` expression converted to `Float`.
    IntToFloat(Box<Expr>),

    /// A call of a user function or method. For a method the receiver is
    /// the first argument.
    Call {
        func: FuncId,
        args: Vec<Expr>,
    },
    /// A built-in function or method that does not modify its receiver.
    Builtin {
        op: Builtin,
        args: Vec<Expr>,
    },

    /// When the type is not `Void`, both blocks have a value of that type.
    /// When it is `Void`, neither has a value.
    If {
        cond: Box<Expr>,
        then: Block,
        else_: Block,
    },
    /// Statements followed by a value, with locals of its own. The checker
    /// uses it for `case`. The block's value is present unless the type is
    /// `Void`.
    Block(Block),

    /// `spawn`. The type is `Entity`.
    Spawn(Vec<ComponentInit>),
    /// `e.add(…)`. `Void`.
    Add {
        entity: Box<Expr>,
        components: Vec<ComponentInit>,
    },
    /// `e.remove(…)`. `Void`.
    Remove {
        entity: Box<Expr>,
        components: Vec<ComponentId>,
    },
    /// `e.despawn`. `Void`.
    Despawn(Box<Expr>),
    /// `e.has?(C)`. `Bool`.
    Has {
        entity: Box<Expr>,
        component: ComponentId,
    },
    /// `e.alive?`. `Bool`.
    Alive(Box<Expr>),

    /// `xs.push(v)`. `Void`. `value` has the element's value type.
    ListPush {
        list: Place,
        value: Box<Expr>,
    },
    /// `xs.pop`. The element's value type.
    ListPop {
        list: Place,
    },
    /// `xs.delete_at(i)`. The element's value type.
    ListDeleteAt {
        list: Place,
        index: Box<Expr>,
    },
    /// `xs.clear`. `Void`.
    ListClear {
        list: Place,
    },

    /// `World.tick`. `Void`.
    WorldTick,
    /// `World.run`. `Void`.
    WorldRun,
    /// `World.stop`. `Void`.
    WorldStop,
    /// `World.count(…)`, or with `None` the number of living entities.
    /// `Int`. Terms are never marked as written.
    WorldCount(Option<Query>),
}

/// One component in a `spawn` or `add`.
#[derive(Clone, Debug)]
pub struct ComponentInit {
    pub component: ComponentId,
    /// `None` for a tag; otherwise a value of the component's record.
    pub value: Option<Expr>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnOp {
    /// On `Int` or `Float`.
    Neg,
    /// On `Bool`.
    Not,
}

/// Built-in binary operators. The operand types are one of:
///
/// | Operators              | `lhs`, `rhs`                    | Result      |
/// | ---------------------- | ------------------------------- | ----------- |
/// | `Add Sub Mul Div Mod Pow` | `Int, Int` or `Float, Float` | same        |
/// | `Add`                  | `String, String`; `[T], [T]`    | same        |
/// | `Mul`                  | `String, Int`; `[T], Int`       | `lhs`'s     |
/// | `Eq Ne`                | any value type, twice           | `Bool`      |
/// | `Lt Le Gt Ge`          | `Int`, `Float` or `String`, twice | `Bool`    |
///
/// `Int` `Div` and `Mod` floor, and are run-time errors when `rhs` is zero.
/// `Float` `Mod` takes the sign of the divisor too. `Int` `Pow` with a
/// negative `rhs` is a run-time error. `Int` arithmetic wraps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinOp {
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
}

/// Built-in functions and methods. The comment gives the argument types
/// (the receiver first) and the result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Builtin {
    /// (any value type) -> String
    ToS,

    /// (Int) -> Int
    IntAbs,
    /// (Int v, Int lo, Int hi) -> Int
    IntClamp,
    /// (Int, Int) -> Int
    IntMin,
    /// (Int, Int) -> Int
    IntMax,

    /// (Float) -> Float
    FloatAbs,
    /// (Float) -> Int, toward zero
    FloatToI,
    /// (Float) -> Int
    FloatFloor,
    /// (Float) -> Int
    FloatCeil,
    /// (Float) -> Int, halves away from zero
    FloatRound,
    /// (Float, Int digits) -> Float
    FloatRoundDigits,
    /// (Float) -> Bool
    FloatNan,
    /// (Float v, Float lo, Float hi) -> Float
    FloatClamp,
    /// (Float, Float) -> Float
    FloatMin,
    /// (Float, Float) -> Float
    FloatMax,

    /// (Float) -> Float
    Sqrt,
    /// (Float) -> Float
    Sin,
    /// (Float) -> Float
    Cos,
    /// (Float) -> Float
    Tan,
    /// (Float y, Float x) -> Float
    Atan2,
    /// (Float) -> Float
    Exp,
    /// (Float) -> Float
    Log,

    /// (String) -> Int, in code points
    StrLength,
    /// (String) -> Int
    StrBytesize,
    /// (String) -> Bool
    StrEmpty,
    /// (String) -> String
    StrUpcase,
    /// (String) -> String
    StrDowncase,
    /// (String, String) -> Bool
    StrInclude,
    /// (String, String) -> Bool
    StrStartWith,
    /// (String, String) -> Bool
    StrEndWith,
    /// (String, Int) -> String
    StrLjust,
    /// (String, Int) -> String
    StrRjust,
    /// (String) -> Int
    StrToI,
    /// (String) -> Float
    StrToF,

    /// ([T]) -> Int
    ListLength,
    /// ([T]) -> Bool
    ListEmpty,
    /// ([T]) -> T's value type; a run-time error on an empty list
    ListFirst,
    /// ([T]) -> T's value type; a run-time error on an empty list
    ListLast,
    /// ([T], T's value type) -> Bool
    ListInclude,

    /// () -> Float in [0, 1)
    Rand,
    /// (Int n) -> Int in [0, n); a run-time error unless n > 0
    RandInt,
    /// (Int seed) -> Void
    Srand,
    /// () -> Void: seed from the system clock
    SrandClock,
    /// () -> Float
    Clock,
    /// (Float seconds) -> Void
    Sleep,
    /// (Int status) -> Void; does not return
    Exit,
}
