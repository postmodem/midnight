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
//!
//! # Effects, for backends
//!
//! The checker's last pass works out what code does to the world, and
//! leaves three things here:
//!
//! - every [`Query`] has an [`id`](Query::id), unique in the program;
//! - every function has a [`Summary`] in [`Program::summaries`]: what it
//!   does, directly or through the functions it calls;
//! - every [`StmtKind::Each`] has a [`Run`]: where the loop runs, as
//!   "Several cores" in the reference defines it. For a loop that can be
//!   split across the cores it carries what a backend needs to split it
//!   ([`Cores`]); for one that can't, the first thing in its block that
//!   keeps it on one core ([`Reason`]).

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
    /// The number of a [`Query`]: of one `each` or one `World.count(…)`
    /// in the source. Counted from 0 through the functions, then the
    /// systems, then the top level, each in source order.
    QueryId,
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
    /// Resources, the built-in `Window` among them in a program with a
    /// window. The built-in `Time` is not listed; see [`PlaceKind::Time`].
    pub resources: Vec<Resource>,
    pub events: Vec<Event>,
    /// In definition order. A constant's `init` may read earlier constants.
    pub consts: Vec<Const>,
    /// Functions and methods.
    pub funcs: Vec<Func>,
    /// One per function, in the order of `funcs`.
    pub summaries: Vec<Summary>,
    /// In the order `World.tick` runs them.
    pub systems: Vec<System>,
    /// The top-level statements, in order.
    pub main: Body,
    /// Set when the program has a window: when it names `Window`,
    /// `WindowResized`, `WindowClosed`, `Keyboard`, `Mouse`, `Draw` or one
    /// of the events of the keyboard, the mouse and the focus.
    pub window: Option<Window>,
}

/// The built-in declarations of a program with a window. They are
/// ordinary declarations of `enums`, `records`, `resources` and `events`,
/// and a backend handles them as it handles any other; it also hands the
/// two resources and the eight events to the runtime.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Window {
    /// `Window`. Its fields, in order: `width`, `height`, `pixel_width`,
    /// `pixel_height` (`Int`), `background` (`Color`), `open` (`Bool`,
    /// which `Window.open?` reads) and `focused` (`Bool`, which
    /// `Window.focused?` reads). The program assigns only `background`;
    /// the runtime assigns the others when a tick begins and in
    /// [`StmtKind::WindowOpen`].
    pub resource: ResourceId,
    /// The struct `Color`: `r`, `g`, `b`, `a`, each a `UInt8`.
    pub color: RecordId,
    /// `WindowResized`: `width`, `height`, `pixel_width`, `pixel_height`.
    pub resized: EventId,
    /// `WindowClosed`, with no fields.
    pub closed: EventId,
    /// `Mouse`. Its fields, in order: `x`, `y`, `wheel_x`, `wheel_y`
    /// (`Float`) and `inside` (`Bool`, which `Mouse.inside?` reads). The
    /// runtime assigns them when a tick begins; the program assigns none.
    pub mouse: ResourceId,
    /// The enums `Key` and `MouseButton`.
    pub key: EnumId,
    pub button: EnumId,
    /// `KeyPressed` and `KeyReleased`: `key`.
    pub key_pressed: EventId,
    pub key_released: EventId,
    /// `MousePressed` and `MouseReleased`: `button`, `x`, `y`.
    pub mouse_pressed: EventId,
    pub mouse_released: EventId,
    /// `WindowFocused` and `WindowUnfocused`, with no fields.
    pub focused: EventId,
    pub unfocused: EventId,
}

/// `Keyboard.down?(key)` and the other queries of the keyboard and the
/// mouse. They answer from what the runtime set when the tick began.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputQuery {
    KeyDown,
    KeyPressed,
    KeyReleased,
    ButtonDown,
    ButtonPressed,
    ButtonReleased,
}

impl InputQuery {
    /// The query as the source writes it.
    pub fn name(self) -> &'static str {
        match self {
            InputQuery::KeyDown => "Keyboard.down?",
            InputQuery::KeyPressed => "Keyboard.pressed?",
            InputQuery::KeyReleased => "Keyboard.released?",
            InputQuery::ButtonDown => "Mouse.down?",
            InputQuery::ButtonPressed => "Mouse.pressed?",
            InputQuery::ButtonReleased => "Mouse.released?",
        }
    }
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
        /// Where the loop runs.
        run: Run,
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
    /// `Window.open(title, width, height)`: a `String` and two `Int`s,
    /// evaluated in that order. Never inside an `each` or a GPU system.
    WindowOpen {
        title: Expr,
        width: Expr,
        height: Expr,
    },
    /// `Draw.rect(…)` and the other shapes: `nums` holds the shape's
    /// numbers as `Float`s, in the order of [`Shape::params`], and `color`
    /// is a `Color`; they are evaluated in that order, and then the draw is
    /// added to the frame's list. In a GPU block it stands directly in the
    /// block or in a branch of it, never in a loop, the nested `each` or a
    /// function.
    Draw {
        shape: Shape,
        nums: Vec<Expr>,
        color: Expr,
        /// `Draw.rect` as written, for messages.
        head: Span,
    },
}

/// The shapes of `Draw`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    Rect,
    Circle,
    Line,
    Triangle,
}

impl Shape {
    pub const ALL: [Shape; 4] = [Shape::Rect, Shape::Circle, Shape::Line, Shape::Triangle];

    /// The member of `Draw`.
    pub fn name(self) -> &'static str {
        match self {
            Shape::Rect => "rect",
            Shape::Circle => "circle",
            Shape::Line => "line",
            Shape::Triangle => "triangle",
        }
    }

    /// The names of its numbers, in order; the color comes after them.
    pub fn params(self) -> &'static [&'static str] {
        match self {
            Shape::Rect => &["x", "y", "width", "height"],
            Shape::Circle => &["x", "y", "radius"],
            Shape::Line => &["x1", "y1", "x2", "y2", "width"],
            Shape::Triangle => &["x1", "y1", "x2", "y2", "x3", "y3"],
        }
    }

    /// The kind word of its record: `MN_DRAW_RECT` and the others.
    pub fn kind(self) -> u32 {
        match self {
            Shape::Rect => 1,
            Shape::Circle => 2,
            Shape::Line => 3,
            Shape::Triangle => 4,
        }
    }

    /// The statement as the reference writes it:
    /// `Draw.circle(x, y, radius, color)`.
    pub fn form(self) -> String {
        format!("Draw.{}({}, color)", self.name(), self.params().join(", "))
    }
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
    /// Which query of the program this is.
    pub id: QueryId,
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

/// Where an `each` runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Run {
    /// Written inside another `each`: it runs where it is reached, as part
    /// of a run of that loop.
    Inside,
    /// A block of a GPU system: on the device, and in pieces on the cores
    /// when it runs on the CPU.
    Gpu,
    /// Independent: no run of the block can see what another run does, so
    /// the rows can be divided among the cores.
    Cores(Cores),
    /// On the thread that reaches it, one row after another.
    OneCore(Reason),
}

/// What a backend needs to split an independent `each`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cores {
    /// The whole-number totals the block adds to with `+=` and `-=`, in
    /// the order of their first sum. A `CompoundAssign` in the block, loops
    /// and `each`es inside it included, whose place is one of these is a
    /// sum, and nothing else the block reaches reads or assigns one.
    pub sums: Vec<Target>,
    /// The work of one run. Its first term has no rows and is at least 1.
    pub weight: Weight,
    /// Every other query the block can reach: the `each`es and
    /// `World.count(…)` inside the block and in every function it calls,
    /// directly or through other functions. Ascending.
    pub queries: Vec<QueryId>,
}

/// A resource field or a variable that a loop adds to or assigns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// A field of a user resource, by index into [`Resource::fields`].
    Field(ResourceId, usize),
    /// A variable of the code around the loop: a local of the [`Body`] the
    /// `each` stands in.
    Local(LocalId),
}

/// An estimate of work, in operations: the sum of its terms. The terms
/// are ordered by `rows`, fewer first, and no two have the same `rows`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Weight {
    pub terms: Vec<WeightTerm>,
}

/// `factor` times the number of rows of each query in `rows`, as they are
/// when the loop begins. `factor` is at least 1 and at most
/// [`Weight::LIMIT`]. `rows` is ascending and may name a query twice.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WeightTerm {
    pub factor: u64,
    pub rows: Vec<QueryId>,
}

impl Weight {
    /// Where factors stop growing. A backend's sums and products of
    /// weights stop here too.
    pub const LIMIT: u64 = 1 << 62;
}

/// The first thing in an `each`'s block, in source order, that keeps the
/// loop on one core.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reason {
    pub kind: ReasonKind,
    /// Where the thing is. For a nested `each`, the `each`.
    pub span: Span,
    /// Set when the thing is not in the block but in a function it calls.
    pub via: Option<Via>,
}

/// How a block reaches something in a function.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Via {
    /// The function the thing is in.
    pub func: FuncId,
    /// The call in the block that leads there, directly or through other
    /// functions.
    pub call: Span,
}

/// The ways of breaking the rules of an independent `each`. The number is
/// the rule's in "Several cores".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReasonKind {
    /// 1: an expression or a place that is a `String`, or a record with
    /// one inside.
    UsesString,
    /// 1: the same for a list.
    UsesList,
    /// 2: an assignment to, or through, `e[C]`. `span` is the `e[C]`.
    AssignsRow(ComponentId),
    /// 2: a nested `each` assigns through its parameter for this
    /// component.
    AssignsNested(ComponentId),
    /// 2: an assignment to a resource field, or to a variable of the code
    /// around the loop. `spelled_out` when an `Int` target is assigned
    /// itself plus or minus something, which `+=` or `-=` would make a
    /// sum.
    Assigns { target: Target, spelled_out: bool },
    /// 2: an assignment to `Time.dt`.
    AssignsDt,
    /// 3: a read, by `e[C]` or (`nested`) through a parameter of a nested
    /// `each`, of a component the loop assigns through its own parameters.
    ReadsAssigned {
        component: ComponentId,
        nested: bool,
    },
    /// 4: `+=` or `-=` on a `Float` target.
    FloatSum(Target),
    /// 4: `+=` or `-=` on a compact integer field.
    CompactSum(Target),
    /// 4: something reads a total of the loop.
    SummedAndRead(Target),
    /// 4: something assigns a total of the loop otherwise than by a sum
    /// in the block.
    SummedAndAssigned(Target),
    /// 4: `+=` or `-=` on an `Int` resource field in a called function.
    /// `via` is always set.
    SumInFunction(Target),
    /// 6
    Does(Deed),
    /// 7: a read, by `e[C]` or (`nested`) through a parameter of a nested
    /// `each`, of a component that a GPU block names as a term. `system`
    /// indexes [`Program::systems`]: the first GPU system that names it.
    ReadsGpu {
        component: ComponentId,
        system: usize,
        nested: bool,
    },
}

/// What rule 6 of an independent `each` leaves out.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Deed {
    Spawn,
    On,
    /// `rand` and `rand(n)`.
    Rand,
    /// `srand` with and without a seed.
    Srand,
    Puts,
    Print,
    Clock,
    Sleep,
    Exit,
    WorldStop,
    WorldTick,
    WorldRun,
    WindowOpen,
    /// A `break` that leaves the `each`. Never in a [`Summary`].
    Break,
    /// A `return` in the block. Never in a [`Summary`].
    Return,
}

impl Deed {
    /// The word as the source writes it.
    pub fn word(self) -> &'static str {
        match self {
            Deed::Spawn => "spawn",
            Deed::On => "on",
            Deed::Rand => "rand",
            Deed::Srand => "srand",
            Deed::Puts => "puts",
            Deed::Print => "print",
            Deed::Clock => "clock",
            Deed::Sleep => "sleep",
            Deed::Exit => "exit",
            Deed::WorldStop => "World.stop",
            Deed::WorldTick => "World.tick",
            Deed::WorldRun => "World.run",
            Deed::WindowOpen => "Window.open",
            Deed::Break => "break",
            Deed::Return => "return",
        }
    }
}

/// What a function does, directly or through the functions it calls.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Summary {
    /// Each kind of effect once, with the place of its first use: in the
    /// order a reader of the body meets them, a call standing for
    /// everything its function does.
    pub effects: Vec<Effect>,
    /// The function can call itself, directly or through others.
    pub recursive: bool,
    /// The work of one call: 64 for a function that recurses.
    pub weight: Weight,
    /// Every query the function can reach: its own `each`es and
    /// `World.count(…)` and those of the functions it calls. Ascending.
    pub queries: Vec<QueryId>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Effect {
    pub kind: EffectKind,
    /// The function the effect is written in.
    pub func: FuncId,
    /// Where in that function. For `TermUse` and `TermWrite`, the `each`;
    /// for `RowRead` and `RowWrite`, the `e[C]`.
    pub span: Span,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EffectKind {
    /// A read of `e[C]`.
    RowRead(ComponentId),
    /// An assignment to, or through, `e[C]`.
    RowWrite(ComponentId),
    /// An `each` has the component as a term and its block uses the
    /// parameter.
    TermUse(ComponentId),
    /// The block of an `each` assigns through the parameter of this term.
    TermWrite(ComponentId),
    /// A read of a resource field.
    FieldRead(ResourceId, usize),
    /// An assignment to a resource field, or to part of it, that is not a
    /// `FieldSum`.
    FieldWrite(ResourceId, usize),
    /// `R.f += x` or `R.f -= x` with a built-in operator.
    FieldSum(ResourceId, usize),
    /// An assignment to `Time.dt`.
    DtWrite,
    Despawn,
    Add,
    Remove,
    Emit(EventId),
    /// A draw.
    Draw,
    /// `spawn`, `on`, output, random numbers, the clock, `exit` and the
    /// `World` methods other than `count`.
    Does(Deed),
    /// A value that is a `String`, or a record with one inside.
    UsesString,
    /// The same for a list.
    UsesList,
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
    /// A query of the keyboard or the mouse. `Bool`. `arg` is a `Key` for
    /// the keyboard's and a `MouseButton` for the mouse's.
    Input {
        query: InputQuery,
        arg: Box<Expr>,
    },

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
