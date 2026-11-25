//! Syntax tree to typed program: name resolution, type inference and
//! checking, constant expressions, place and read-only rules, query
//! validation.
//!
//! The checker makes four passes over the program:
//!
//! 1. the name of every declaration is collected, so that anything can be
//!    used before it is defined;
//! 2. field types and function signatures are resolved;
//! 3. constants and field defaults are typed, each when it is first needed;
//! 4. the top level, functions, methods and systems are typed. A function
//!    with no declared return type is typed when it is first called.
//!
//! An expression that fails to check is `None`: its error has been
//! reported, and whatever it feeds into is skipped without a second report.
//! A variable whose first value failed is still declared, with its type
//! unknown, so later uses of it stay quiet.

mod decls;
mod exprs;
mod gpu;
mod members;
mod stmts;
mod util;
mod walk;

#[cfg(test)]
mod tests;

use std::collections::{HashMap, HashSet};

use crate::ast;
use crate::diag::Diagnostic;
use crate::source::{SourceMap, Span};
use crate::tir::{self, ComponentId, ConstId, EnumId, FuncId, LocalId, RecordId, ResourceId, Type};

/// Checks a whole program. Reports every independent error, in source
/// order.
pub fn check(program: &ast::Program, sources: &SourceMap) -> Result<tir::Program, Vec<Diagnostic>> {
    let mut checker = Checker::new(sources);
    checker.declare(program);
    checker.resolve_signatures();
    checker.check_constants();
    checker.check_bodies();
    checker.finish()
}

/// Names that are built in and can't be redefined.
const BUILTIN_NAMES: &[&str] = &[
    "Int", "Float", "Bool", "String", "Entity", "World", "Time", "Math", "Float32", "Int8",
    "Int16", "Int32", "UInt8", "UInt16", "UInt32",
];

/// The built-in names that are types.
const BUILTIN_TYPES: &[&str] = &[
    "Int", "Float", "Bool", "String", "Entity", "Float32", "Int8", "Int16", "Int32", "UInt8",
    "UInt16", "UInt32",
];

/// Built-in functions, called as `name` or `name(…)`.
const BUILTIN_FUNCTIONS: &[&str] = &["min", "max", "rand", "srand", "clock", "sleep", "exit"];

/// The hint on a `Float` where an `Int` is required.
const FLOAT_TO_INT: &str =
    "a `Float` does not convert to `Int`; use `to_i`, `floor`, `ceil` or `round`";

/// What a capitalized name refers to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Global {
    /// A struct, a component with fields, or an event.
    Record(RecordId),
    Enum(EnumId),
    Tag(ComponentId),
    Resource(ResourceId),
    Const(ConstId),
}

/// What a lowercase name refers to outside any body.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FnSym {
    Func(FuncId),
    System,
}

struct RecordInfo<'a> {
    name: String,
    kind: tir::RecordKind,
    decl: &'a ast::RecordDecl,
    fields: Vec<FieldInfo<'a>>,
    methods: Vec<(String, FuncId)>,
}

struct FieldInfo<'a> {
    name: String,
    span: Span,
    /// The storage type; `None` when it failed to resolve.
    ty: Option<Type>,
    default: DefaultState<'a>,
}

enum DefaultState<'a> {
    Absent,
    Pending(&'a ast::Expr),
    InProgress,
    /// `None` when the default failed to check.
    Done(Option<tir::Expr>),
}

struct EnumInfo {
    name: String,
    variants: Vec<String>,
}

struct ResourceInfo<'a> {
    name: String,
    decl: &'a ast::RecordDecl,
    fields: Vec<FieldInfo<'a>>,
}

struct ConstInfo<'a> {
    name: String,
    span: Span,
    value: &'a ast::Expr,
    state: ConstState,
}

enum ConstState {
    Pending,
    InProgress,
    Done {
        /// `None` when the value failed to check.
        init: Option<tir::Expr>,
        /// The value, when it is an `Int` the checker can compute.
        int: Option<i64>,
    },
}

struct FuncInfo<'a> {
    name: String,
    /// The name in the declaration.
    span: Span,
    decl: &'a ast::FuncDecl,
    owner: Option<RecordId>,
    params: Vec<ParamInfo>,
    ret: RetState,
    state: FuncState,
    body: Option<tir::Body>,
    param_locals: Vec<LocalId>,
    /// The "needs a declared return type" error has been reported.
    recursion_reported: bool,
    /// The function returns nothing because its last expression is a call
    /// back into a function whose return type was still being inferred.
    void_by_recursion: bool,
}

struct ParamInfo {
    name: String,
    span: Span,
    ty: Option<Type>,
}

#[derive(Clone)]
enum RetState {
    /// Not declared and not inferred yet.
    Unknown,
    /// `None` when the type failed to resolve or to infer.
    Known(Option<Type>),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FuncState {
    Pending,
    InProgress,
    Done,
}

struct SystemInfo<'a> {
    decl: &'a ast::SystemDecl,
    body: Option<tir::Body>,
}

/// How an expression's result is used.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    /// The result is discarded.
    Stmt,
    /// A value is required.
    Value,
    /// The last expression of a function body: a value if it has one.
    Maybe,
}

/// Where a type is written, which decides whether it may be compact.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TypeUse {
    Field,
    Element,
    Variable,
    Parameter,
    Return,
}

/// Why something can't be assigned to or modified.
#[derive(Clone, Debug)]
enum ReadOnly {
    Param(String),
    SelfValue,
    SelfField(String),
    Const(String),
    OnParam(String),
    ListParam(String),
    IndexParam(String),
    CountParam(String),
    TimeField(&'static str),
}

impl ReadOnly {
    fn reason(&self) -> String {
        match self {
            ReadOnly::Param(name) => {
                format!("`{name}` is a parameter, and parameters are read-only")
            }
            ReadOnly::SelfValue => {
                "`self` is read-only: a method can't modify its receiver".to_string()
            }
            ReadOnly::SelfField(name) => {
                format!("`{name}` is a field of `self`, and a method can't modify its receiver")
            }
            ReadOnly::Const(name) => format!("`{name}` is a constant"),
            ReadOnly::OnParam(name) => {
                format!("`{name}` is the parameter of an `on` block, a read-only copy of the event")
            }
            ReadOnly::ListParam(name) => format!(
                "`{name}` is a parameter of a list iteration, a read-only copy; loop over indexes to change the list"
            ),
            ReadOnly::IndexParam(name) => {
                format!("`{name}` is the index of a list iteration, which is read-only")
            }
            ReadOnly::CountParam(name) => {
                format!("`{name}` is the counter of a counting loop, which is read-only")
            }
            ReadOnly::TimeField(name) => {
                format!("`Time.{name}` is read-only; only `Time.dt` can be assigned")
            }
        }
    }
}

/// A place together with whether it may be written.
struct PlaceRef {
    place: tir::Place,
    readonly: Option<ReadOnly>,
}

/// The result of checking an expression that may name a place.
enum Resolved {
    Place(PlaceRef),
    Value(tir::Expr),
}

impl Resolved {
    /// The value type of the result.
    fn ty(&self) -> Type {
        match self {
            Resolved::Place(p) => p.place.ty.value_type(),
            Resolved::Value(v) => v.ty.clone(),
        }
    }

    fn into_value(self) -> tir::Expr {
        match self {
            Resolved::Place(p) => read(p.place),
            Resolved::Value(v) => v,
        }
    }
}

/// What a constant expression is the value of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ConstUse {
    Constant(ConstId),
    FieldDefault,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BodyKind {
    Top,
    System,
    Func(FuncId),
    Const(ConstUse),
}

/// How `return` behaves in the body being checked.
#[derive(Clone)]
enum Ret {
    /// The top level, a system, or a constant expression: no value.
    Nothing,
    /// A declared return type; `None` when it failed to resolve.
    Declared(Option<Type>),
    /// No declared type: every `return` is recorded in `ret_sites`.
    Inferred,
}

struct RetSite {
    /// `None` for a bare `return`.
    ty: Option<Type>,
    span: Span,
}

struct LocalMeta {
    /// The type is unknown because the first value failed to check.
    poisoned: bool,
    readonly: Option<ReadOnly>,
    /// Something was assigned or modified through this local.
    written: bool,
    /// Where the local was created.
    span: Span,
    /// The local is a parameter of a `do` block.
    block_param: bool,
}

/// The variables of one block.
struct Scope {
    names: Vec<(String, LocalId)>,
    /// What the block belongs to, as messages name it: "the `if`". `None`
    /// for the scope of a whole body.
    construct: Option<&'static str>,
}

impl Scope {
    fn new(construct: Option<&'static str>) -> Scope {
        Scope {
            names: Vec::new(),
            construct,
        }
    }
}

/// A variable whose block has ended.
struct Ended {
    name: String,
    local: LocalId,
    /// The construct whose block it belonged to.
    construct: &'static str,
}

/// The state of the body being checked: its locals, scopes and loops.
struct FnCtx {
    kind: BodyKind,
    locals: Vec<tir::Local>,
    meta: Vec<LocalMeta>,
    scopes: Vec<Scope>,
    /// The variables of every block that has ended, oldest first.
    ended: Vec<Ended>,
    /// In a constant expression: its error has been reported.
    const_reported: bool,
    /// In a method: the `self` local and its record.
    self_: Option<(LocalId, RecordId)>,
    /// One entry per enclosing loop: whether a `break` targets it.
    loops: Vec<bool>,
    /// Control can't reach the point being checked.
    unreachable: bool,
    ret: Ret,
    ret_sites: Vec<RetSite>,
    /// A `return` whose value failed to check was seen.
    ret_poison: bool,
    /// The body's last expression is a call of this function, whose return
    /// type was still being inferred.
    recursive_tail: Option<(FuncId, Span)>,
}

impl FnCtx {
    fn new(kind: BodyKind) -> FnCtx {
        FnCtx {
            kind,
            locals: Vec::new(),
            meta: Vec::new(),
            scopes: vec![Scope::new(None)],
            ended: Vec::new(),
            const_reported: false,
            self_: None,
            loops: Vec::new(),
            unreachable: false,
            ret: Ret::Nothing,
            ret_sites: Vec::new(),
            ret_poison: false,
            recursive_tail: None,
        }
    }

    fn lookup(&self, name: &str) -> Option<LocalId> {
        self.scopes
            .iter()
            .rev()
            .flat_map(|scope| scope.names.iter().rev())
            .find(|(n, _)| n == name)
            .map(|(_, id)| *id)
    }

    fn const_use(&self) -> Option<ConstUse> {
        match self.kind {
            BodyKind::Const(c) => Some(c),
            _ => None,
        }
    }

    /// Every variable name in scope.
    fn names(&self) -> Vec<String> {
        self.scopes
            .iter()
            .flat_map(|scope| scope.names.iter().map(|(n, _)| n.clone()))
            .collect()
    }
}

struct Checker<'a> {
    sources: &'a SourceMap,
    errors: Vec<Diagnostic>,
    globals: HashMap<String, (Global, Span)>,
    fn_names: HashMap<String, (FnSym, Span)>,
    records: Vec<RecordInfo<'a>>,
    enums: Vec<EnumInfo>,
    components: Vec<tir::Component>,
    resources: Vec<ResourceInfo<'a>>,
    events: Vec<tir::Event>,
    consts: Vec<ConstInfo<'a>>,
    funcs: Vec<FuncInfo<'a>>,
    systems: Vec<SystemInfo<'a>>,
    /// The top-level statements, in order.
    top: Vec<&'a ast::Stmt>,
    /// Names of variables created by top-level statements.
    top_vars: HashSet<String>,
    main: Option<tir::Body>,
    cx: FnCtx,
    /// Some call was typed before its function's return type was known.
    deferred_calls: bool,
}

fn mk(kind: tir::ExprKind, ty: Type, span: Span) -> tir::Expr {
    tir::Expr { kind, ty, span }
}

/// A read of a place, as a copy.
fn read(place: tir::Place) -> tir::Expr {
    let ty = place.ty.value_type();
    let span = place.span;
    mk(tir::ExprKind::Read(place), ty, span)
}

fn int_to_float(expr: tir::Expr) -> tir::Expr {
    let span = expr.span;
    mk(tir::ExprKind::IntToFloat(Box::new(expr)), Type::Float, span)
}

fn is_number(ty: &Type) -> bool {
    matches!(ty, Type::Int | Type::Float)
}

impl<'a> Checker<'a> {
    fn new(sources: &'a SourceMap) -> Checker<'a> {
        Checker {
            sources,
            errors: Vec::new(),
            globals: HashMap::new(),
            fn_names: HashMap::new(),
            records: Vec::new(),
            enums: Vec::new(),
            components: Vec::new(),
            resources: Vec::new(),
            events: Vec::new(),
            consts: Vec::new(),
            funcs: Vec::new(),
            systems: Vec::new(),
            top: Vec::new(),
            top_vars: HashSet::new(),
            main: None,
            cx: FnCtx::new(BodyKind::Top),
            deferred_calls: false,
        }
    }

    fn error(&mut self, diagnostic: Diagnostic) {
        self.errors.push(diagnostic);
    }

    fn err(&mut self, message: impl Into<String>, span: Span) {
        self.error(Diagnostic::new(message, span));
    }

    fn err_label(&mut self, message: impl Into<String>, span: Span, label: impl Into<String>) {
        self.error(Diagnostic::new(message, span).label(label));
    }

    /// Reports an unknown name, suggesting the closest candidate.
    fn unknown(&mut self, message: String, span: Span, name: &str, candidates: &[String]) {
        let mut diagnostic = Diagnostic::new(message, span);
        if let Some(close) = util::closest(name, candidates.iter().map(String::as_str)) {
            diagnostic = diagnostic.label(format!("did you mean `{close}`?"));
        }
        self.error(diagnostic);
    }

    fn mismatch(&mut self, expected: &Type, found: &Type, span: Span, label: Option<String>) {
        let message = format!(
            "expected `{}`, found `{}`",
            self.type_name(expected),
            self.type_name(found)
        );
        let label = label.or_else(|| {
            (*expected == Type::Int && *found == Type::Float).then(|| FLOAT_TO_INT.to_string())
        });
        let mut diagnostic = Diagnostic::new(message, span);
        if let Some(label) = label {
            diagnostic = diagnostic.label(label);
        }
        self.error(diagnostic);
    }

    /// The source text of a span, when it is short and on one line.
    fn snippet(&self, span: Span) -> Option<&str> {
        let text = &self.sources.file(span.file).text;
        let text = text.get(span.start as usize..span.end as usize)?;
        (text.chars().count() <= 40 && !text.contains('\n')).then_some(text)
    }

    /// Reports a name that ends in `?` or `!` where only function and
    /// method names may. `what` is "variable", "parameter", …
    fn plain_name(&mut self, what: &str, name: &str, span: Span) {
        if let Some(mark) = name.chars().last().filter(|c| matches!(c, '?' | '!')) {
            self.err_label(
                format!("{what} name `{name}` can't end in `{mark}`"),
                span,
                "only function and method names can",
            );
        }
    }

    /// Opens the scope of a block. `construct` names what the block
    /// belongs to: "the `if`".
    fn push_scope(&mut self, construct: Option<&'static str>) {
        self.cx.scopes.push(Scope::new(construct));
    }

    /// Closes the innermost scope and remembers its variables, so that a
    /// later use of one can be explained.
    fn pop_scope(&mut self) {
        let Some(scope) = self.cx.scopes.pop() else {
            return;
        };
        if let Some(construct) = scope.construct {
            for (name, local) in scope.names {
                self.cx.ended.push(Ended {
                    name,
                    local,
                    construct,
                });
            }
        }
    }

    /// A type as it is written in source.
    fn type_name(&self, ty: &Type) -> String {
        match ty {
            Type::Void => "nothing".to_string(),
            Type::Int => "Int".to_string(),
            Type::Float => "Float".to_string(),
            Type::Bool => "Bool".to_string(),
            Type::String => "String".to_string(),
            Type::Entity => "Entity".to_string(),
            Type::Compact(c) => c.name().to_string(),
            Type::List(elem) => format!("[{}]", self.type_name(elem)),
            Type::Record(id) => self.records[id.0].name.clone(),
            Type::Enum(id) => self.enums[id.0].name.clone(),
        }
    }

    /// "a struct", "a tag", … for a capitalized name that is defined.
    fn global_kind(&self, global: Global) -> &'static str {
        match global {
            Global::Record(id) => match self.records[id.0].kind {
                tir::RecordKind::Struct => "a struct",
                tir::RecordKind::Component(_) => "a component",
                tir::RecordKind::Event(_) => "an event",
            },
            Global::Enum(_) => "an enum",
            Global::Tag(_) => "a tag",
            Global::Resource(_) => "a resource",
            Global::Const(_) => "a constant",
        }
    }

    fn global(&self, name: &str) -> Option<Global> {
        self.globals.get(name).map(|(global, _)| *global)
    }

    fn field_index(&self, record: RecordId, name: &str) -> Option<usize> {
        self.records[record.0]
            .fields
            .iter()
            .position(|f| f.name == name)
    }

    fn method(&self, record: RecordId, name: &str) -> Option<FuncId> {
        self.records[record.0]
            .methods
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, id)| *id)
    }

    /// Creates a local in the innermost scope. With no type it is poisoned:
    /// uses of it check as failed expressions.
    fn declare_local(
        &mut self,
        name: &str,
        span: Span,
        ty: Option<Type>,
        kind: tir::LocalKind,
        readonly: Option<ReadOnly>,
    ) -> LocalId {
        let id = self.hidden_local(name, ty.clone().unwrap_or(Type::Void), kind);
        let meta = &mut self.cx.meta[id.0];
        meta.poisoned = ty.is_none();
        meta.readonly = readonly;
        meta.span = span;
        self.cx
            .scopes
            .last_mut()
            .expect("a body always has a scope")
            .names
            .push((name.to_string(), id));
        id
    }

    /// Creates a local that no name in scope refers to.
    fn hidden_local(&mut self, name: &str, ty: Type, kind: tir::LocalKind) -> LocalId {
        let id = LocalId(self.cx.locals.len());
        self.cx.locals.push(tir::Local {
            name: name.to_string(),
            ty,
            kind,
        });
        self.cx.meta.push(LocalMeta {
            poisoned: false,
            readonly: None,
            written: false,
            span: Span::new(crate::source::FileId(0), 0, 0),
            block_param: false,
        });
        id
    }

    /// Turns the collected declarations into the typed program, or returns
    /// the errors in source order.
    fn finish(mut self) -> Result<tir::Program, Vec<Diagnostic>> {
        if !self.errors.is_empty() {
            let mut errors = std::mem::take(&mut self.errors);
            errors.sort_by_key(|d| (d.span.file.0, d.span.start, d.span.end));
            errors.dedup_by(|a, b| a.span == b.span && a.message == b.message);
            return Err(errors);
        }

        const CHECKED: &str = "a program without errors is fully typed";
        let rets: Vec<Type> = self
            .funcs
            .iter()
            .map(|f| match &f.ret {
                RetState::Known(Some(ty)) => ty.clone(),
                _ => panic!("{CHECKED}"),
            })
            .collect();

        let mut funcs: Vec<tir::Func> = Vec::new();
        for (f, ret) in self.funcs.into_iter().zip(&rets) {
            funcs.push(tir::Func {
                name: f.name,
                owner: f.owner,
                params: f.param_locals,
                ret: ret.clone(),
                body: f.body.expect(CHECKED),
                span: f.decl.span,
            });
        }
        let system_decls: Vec<&ast::SystemDecl> = self.systems.iter().map(|s| s.decl).collect();
        let mut systems: Vec<tir::System> = self
            .systems
            .into_iter()
            .map(|s| tir::System {
                name: s.decl.name.name.clone(),
                gpu: s.decl.gpu,
                body: s.body.expect(CHECKED),
                span: s.decl.span,
            })
            .collect();
        let mut main = self.main.expect(CHECKED);

        if self.deferred_calls {
            let mut set_call_type = |node: walk::Node<'_>| {
                if let walk::Node::Expr(expr) = node
                    && let tir::ExprKind::Call { func, .. } = &expr.kind
                {
                    expr.ty = rets[func.0].clone();
                }
            };
            for func in &mut funcs {
                walk::block(&mut func.body.block, &mut set_call_type);
            }
            for system in &mut systems {
                walk::block(&mut system.body.block, &mut set_call_type);
            }
            walk::block(&mut main.block, &mut set_call_type);
        }

        let records = self
            .records
            .into_iter()
            .map(|r| tir::Record {
                name: r.name,
                kind: r.kind,
                fields: r
                    .fields
                    .into_iter()
                    .map(|f| tir::Field {
                        name: f.name,
                        ty: f.ty.expect(CHECKED),
                    })
                    .collect(),
            })
            .collect();
        let enums = self
            .enums
            .into_iter()
            .map(|e| tir::Enum {
                name: e.name,
                variants: e.variants,
            })
            .collect();
        let resources = self
            .resources
            .into_iter()
            .map(|r| tir::Resource {
                name: r.name,
                fields: r
                    .fields
                    .into_iter()
                    .map(|f| tir::ResourceField {
                        name: f.name,
                        ty: f.ty.expect(CHECKED),
                        init: match f.default {
                            DefaultState::Done(Some(init)) => init,
                            _ => panic!("{CHECKED}"),
                        },
                    })
                    .collect(),
            })
            .collect();
        let known_ints: Vec<Option<i64>> = self
            .consts
            .iter()
            .map(|c| match c.state {
                ConstState::Done { int, .. } => int,
                _ => None,
            })
            .collect();
        let consts = self
            .consts
            .into_iter()
            .map(|c| match c.state {
                ConstState::Done {
                    init: Some(init), ..
                } => tir::Const {
                    name: c.name,
                    ty: init.ty.clone(),
                    init,
                },
                _ => panic!("{CHECKED}"),
            })
            .collect();

        let mut program = tir::Program {
            records,
            enums,
            components: self.components,
            resources,
            events: self.events,
            consts,
            funcs,
            systems,
            main,
        };
        let errors = gpu::check(&mut program, &system_decls, &known_ints);
        if errors.is_empty() {
            Ok(program)
        } else {
            Err(errors)
        }
    }
}
