//! The rules of a `gpu system`, checked on the typed program.
//!
//! The body of a GPU system is one or more `each` statements, each of them
//! a *GPU block*. In a program that passes, a GPU block and every function
//! one calls, directly or through other functions, hold only:
//!
//! - values that are numbers, `Bool`s, enums, entities and records of
//!   those: no expression, local, parameter or return value is a string or
//!   a list, or a record with one inside;
//! - no use of a `UInt32` field itself: no read of a place or of a field
//!   whose type is `UInt32`, no assignment to such a place, and no
//!   `Construct` of a record that has such a field of its own. A record
//!   with one inside is otherwise a value like any other;
//! - the statements `Expr`, `Let`, `Assign`, `CompoundAssign`, `While`,
//!   `Count`, `Next`, and `Break` out of a loop inside the block; in a
//!   function also `Return`; in a GPU block also at most one `Each`, among
//!   the statements of the block itself;
//! - places that start at a local, a constant, `Time` or a resource field,
//!   written only when they start at a local that is neither a parameter
//!   of the nested `each` nor an `Entity` parameter;
//! - calls of functions that do not recurse, and of the built-ins for
//!   numbers, `Math`, `min`, `max`, `rand` and `rand(n)`;
//! - `Int` literals that fit in 32 bits, and no read of an `Int` constant
//!   whose value the checker knows and that does not fit.
//!
//! The terms of a GPU block's query and of its nested `each` are `Entity`
//! or components made of `Float32`, `Int32`, `Int16`, `Int8`, `UInt32`,
//! `UInt16`, `UInt8`, `Bool`, enum and `Entity` fields and records of
//! those.
//!
//! A function is checked once, the first time a GPU system reaches it, and
//! a diagnostic inside it carries a note naming that system and the call.
//! A construct that breaks a rule is reported once. Its operands are still
//! checked unless they are part of the same report: the arguments of
//! `puts`, the parts of a string, the arguments of a call that can't be
//! made.

use std::collections::{HashMap, VecDeque};

use crate::ast;
use crate::diag::Diagnostic;
use crate::source::Span;
use crate::tir::{
    BinOp, Block, Builtin, Compact, CompoundOp, Expr, ExprKind, FuncId, Local, LocalId, LocalKind,
    Place, PlaceKind, Program, Query, RecordId, Stmt, StmtKind, System, Term, TimeField, Type,
};

use super::{util, walk};

/// The types of the values a GPU system works on.
const GPU_VALUES: &str = "numbers, `Bool`s, enums, entities and structs of those";

/// Why GPU code does not use a `UInt32` field.
const UNSIGNED: &str = "which the signed 32-bit `Int` of a GPU system can't hold";

fn is_unsigned(ty: &Type) -> bool {
    matches!(ty, Type::Compact(Compact::UInt32))
}

/// Why `break` and `return` have no meaning in a GPU block.
const NO_ORDER: &str = "runs have no order, so there is nothing to stop";

/// Checks every GPU system of a program that has no other errors. `decls`
/// holds the declaration of each system, in the order of
/// [`Program::systems`], and `known_ints` the value of each constant that
/// is an `Int` the checker can compute. Returns the diagnostics in source
/// order.
pub(super) fn check(
    program: &mut Program,
    decls: &[&ast::SystemDecl],
    known_ints: &[Option<i64>],
) -> Vec<Diagnostic> {
    if !program.systems.iter().any(|system| system.gpu) {
        return Vec::new();
    }
    let calls = call_graph(program);
    let program = &*program;
    let mut pass = Pass {
        program,
        known_ints,
        reached: vec![[false; 2]; program.funcs.len()],
        calls,
        cycles: HashMap::new(),
        queue: VecDeque::new(),
        errors: Vec::new(),
    };
    for (system, decl) in program.systems.iter().zip(decls) {
        if system.gpu {
            pass.system(system, decl);
        }
    }
    let mut errors = pass.errors;
    errors.sort_by_key(|d| (d.span.file.0, d.span.start, d.span.end));
    errors.dedup_by(|a, b| a.span == b.span && a.message == b.message);
    errors
}

/// The functions each function calls, in the order of their first call.
fn call_graph(program: &mut Program) -> Vec<Vec<FuncId>> {
    program
        .funcs
        .iter_mut()
        .map(|func| {
            let mut callees: Vec<FuncId> = Vec::new();
            walk::block(&mut func.body.block, &mut |node| {
                let callee = match node {
                    walk::Node::Expr(Expr {
                        kind: ExprKind::Call { func, .. },
                        ..
                    }) => *func,
                    walk::Node::Stmt(Stmt {
                        kind:
                            StmtKind::CompoundAssign {
                                op: CompoundOp::Method(func),
                                ..
                            },
                        ..
                    }) => *func,
                    _ => return,
                };
                if !callees.contains(&callee) {
                    callees.push(callee);
                }
            });
            callees
        })
        .collect()
}

/// The `each` statement of `block` that covers exactly `span`: its query
/// and its body.
fn source_each(block: &ast::Block, span: Span) -> Option<(&ast::Query, &ast::Block)> {
    block.stmts.iter().find_map(|stmt| match &stmt.kind {
        ast::StmtKind::Each { query, body, .. } if stmt.span == span => Some((query, body)),
        _ => None,
    })
}

/// A type name with its article: "a `Float`", "an `Int`".
fn a(type_name: &str) -> String {
    let article = if type_name.starts_with(['A', 'E', 'I', 'O']) {
        "an"
    } else {
        "a"
    };
    format!("{article} `{type_name}`")
}

/// Whether an exponent is written `2.0` or `2`, which makes `**` a square.
fn is_two(exponent: &Expr) -> bool {
    match &exponent.kind {
        ExprKind::Float(value) => *value == 2.0,
        ExprKind::IntToFloat(inner) => matches!(inner.kind, ExprKind::Int(2)),
        _ => false,
    }
}

fn inside(outer: Span, inner: Span) -> bool {
    outer.file == inner.file && outer.start <= inner.start && inner.end <= outer.end
}

/// How a GPU system reaches a function.
#[derive(Clone)]
struct Reach<'p> {
    system: &'p str,
    /// The system is a `strict gpu system`.
    strict: bool,
    /// The call in the GPU block.
    call: Span,
    /// The functions called on the way, ending with the one reached.
    path: Vec<FuncId>,
}

/// Why values of a type can't exist on the GPU: it is a string or a list,
/// or a record with one in it.
struct Unfit {
    /// For a record: the path of the first such field and the name of
    /// its type.
    field: Option<(String, String)>,
}

/// Where a statement sits in a GPU block.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Pos {
    /// Directly in the block.
    Direct,
    /// In the body of a loop.
    Loop,
    /// In a branch of `if`, `unless`, `case` or the conditional operator.
    Branch,
}

struct Pass<'p> {
    program: &'p Program,
    /// By constant: its value, when it is an `Int` the checker can compute.
    known_ints: &'p [Option<i64>],
    /// The functions some GPU system has reached, as an ordinary and as a
    /// strict GPU system. Their bodies are checked or queued.
    reached: Vec<[bool; 2]>,
    calls: Vec<Vec<FuncId>>,
    /// For a function that has been asked about: the chain of calls from
    /// it that ends by calling a function already in the chain.
    cycles: HashMap<FuncId, Option<Vec<FuncId>>>,
    queue: VecDeque<(FuncId, Reach<'p>)>,
    errors: Vec<Diagnostic>,
}

impl<'p> Pass<'p> {
    fn system(&mut self, system: &'p System, decl: &'p ast::SystemDecl) {
        let name = &system.name;
        if system.body.block.stmts.is_empty() {
            self.errors.push(
                Diagnostic::new(format!("GPU system `{name}` has no `each`"), decl.name.span)
                    .label("the body of a GPU system is one or more `each` blocks"),
            );
        }
        for stmt in &system.body.block.stmts {
            let StmtKind::Each {
                query,
                params,
                body,
            } = &stmt.kind
            else {
                self.errors.push(
                    Diagnostic::new("statement outside `each` in a GPU system", stmt.span).label(
                        "the body of a GPU system is `each` blocks and nothing else; move this into one, or into an ordinary system",
                    ),
                );
                continue;
            };
            let source = source_each(&decl.body, stmt.span);
            let mut walk = Walk::new(self, &system.body.locals, name, system.strict, None);
            walk.gpu_block(query, params, body, source);
        }
        let program = self.program;
        while let Some((id, reach)) = self.queue.pop_front() {
            let func = &program.funcs[id.0];
            let mut walk = Walk::new(
                self,
                &func.body.locals,
                reach.system,
                reach.strict,
                Some(reach),
            );
            walk.block(&func.body.block, Pos::Direct);
        }
    }

    /// `label`, or `Vec2.length` for a method.
    fn func_name(&self, id: FuncId) -> String {
        let func = &self.program.funcs[id.0];
        match func.owner {
            Some(owner) => format!("{}.{}", self.program.records[owner.0].name, func.name),
            None => func.name.clone(),
        }
    }

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
            Type::Record(id) => self.program.records[id.0].name.clone(),
            Type::Enum(id) => self.program.enums[id.0].name.clone(),
        }
    }

    /// The fields of a record, at any depth, whose type `bad` accepts, as
    /// paths from the record: `x`, `pos.x`.
    fn fields(&self, record: RecordId, bad: &dyn Fn(&Type) -> bool) -> Vec<(String, &'p Type)> {
        fn collect<'p>(
            program: &'p Program,
            record: RecordId,
            prefix: &str,
            bad: &dyn Fn(&Type) -> bool,
            out: &mut Vec<(String, &'p Type)>,
        ) {
            for field in &program.records[record.0].fields {
                let path = format!("{prefix}{}", field.name);
                match &field.ty {
                    ty if bad(ty) => out.push((path, ty)),
                    Type::Record(inner) => {
                        collect(program, *inner, &format!("{path}."), bad, out);
                    }
                    _ => {}
                }
            }
        }
        let mut out = Vec::new();
        collect(self.program, record, "", bad, &mut out);
        out
    }

    /// Why values of a type can't exist on the GPU. `None` for numbers,
    /// `Bool`s, enums, entities and records of those.
    fn unfit(&self, ty: &Type) -> Option<Unfit> {
        match ty {
            Type::String | Type::List(_) => Some(Unfit { field: None }),
            Type::Record(id) => {
                let fields = self.fields(*id, &|ty| matches!(ty, Type::String | Type::List(_)));
                let (path, field) = fields.into_iter().next()?;
                Some(Unfit {
                    field: Some((path, self.type_name(field))),
                })
            }
            _ => None,
        }
    }

    /// For a type whose values can't exist on the GPU, what a value of it
    /// is and why it can't: "a `String`; " and `rule`, "a `Label`, whose
    /// field `text` is a `String`; " and `rule`.
    fn not_a_gpu_value(&self, ty: &Type, rule: &str) -> Option<String> {
        let name = self.type_name(ty);
        let unfit = self.unfit(ty)?;
        Some(match &unfit.field {
            None => format!("{}; {rule}", a(&name)),
            Some((path, field)) => {
                format!("{}, whose field `{path}` is {}; {rule}", a(&name), a(field))
            }
        })
    }

    /// The chain of calls from `root` that ends by calling a function
    /// already in the chain, if there is one: `[a, b, a]`, `[f, g, g]`.
    fn cycle(&mut self, root: FuncId) -> Option<Vec<FuncId>> {
        if let Some(known) = self.cycles.get(&root) {
            return known.clone();
        }
        let mut done = vec![false; self.calls.len()];
        let found = self.cycle_from(root, &mut Vec::new(), &mut done);
        self.cycles.insert(root, found.clone());
        found
    }

    fn cycle_from(
        &self,
        func: FuncId,
        path: &mut Vec<FuncId>,
        done: &mut [bool],
    ) -> Option<Vec<FuncId>> {
        if path.contains(&func) {
            let mut chain = path.clone();
            chain.push(func);
            return Some(chain);
        }
        if done[func.0] {
            return None;
        }
        path.push(func);
        for callee in &self.calls[func.0] {
            if let Some(chain) = self.cycle_from(*callee, path, done) {
                return Some(chain);
            }
        }
        path.pop();
        done[func.0] = true;
        None
    }

    /// "`a` calls `b`, which calls `a`", "`f` calls itself".
    fn chain_text(&self, chain: &[FuncId]) -> String {
        let mut text = format!("`{}` calls ", self.func_name(chain[0]));
        for (i, pair) in chain.windows(2).enumerate() {
            if i > 0 {
                text.push_str(", which calls ");
            }
            if pair[0] == pair[1] {
                text.push_str("itself");
            } else {
                text.push_str(&format!("`{}`", self.func_name(pair[1])));
            }
        }
        text
    }
}

/// The check of one body: a GPU block, or a function a GPU system
/// reaches.
struct Walk<'a, 'p> {
    pass: &'a mut Pass<'p>,
    locals: &'p [Local],
    system: &'p str,
    /// The system is a `strict gpu system`.
    strict: bool,
    /// In a function: how the system reaches it.
    reach: Option<Reach<'p>>,
    /// The body of the GPU block in the source.
    source: Option<&'p ast::Block>,
    /// The component parameters of every `each` inside the GPU block.
    nested_params: Vec<LocalId>,
    /// The `Entity` parameters of the GPU block's `each` and of every
    /// `each` inside it.
    entity_params: Vec<LocalId>,
    /// How many `each` enclose the statement being checked.
    each_depth: usize,
    /// How many loops inside the GPU block enclose it.
    loops: usize,
    /// The GPU block already has its nested `each`.
    nested_seen: bool,
    /// While a field default is checked: the constructor that uses it.
    default_use: Option<Span>,
}

impl<'a, 'p> Walk<'a, 'p> {
    fn new(
        pass: &'a mut Pass<'p>,
        locals: &'p [Local],
        system: &'p str,
        strict: bool,
        reach: Option<Reach<'p>>,
    ) -> Walk<'a, 'p> {
        Walk {
            pass,
            locals,
            system,
            strict,
            reach,
            source: None,
            nested_params: Vec::new(),
            entity_params: Vec::new(),
            each_depth: 0,
            loops: 0,
            nested_seen: false,
            default_use: None,
        }
    }

    /// "in a GPU system", or "in a function called from a GPU system".
    fn here(&self) -> &'static str {
        if self.reach.is_some() {
            "in a function called from a GPU system"
        } else {
            "in a GPU system"
        }
    }

    /// Reports a broken rule. In a function, a note says which GPU system
    /// reaches it and where.
    fn report(&mut self, message: impl Into<String>, span: Span, label: impl Into<String>) {
        let mut diagnostic = Diagnostic::new(message, span).label(label);
        if let Some(site) = self.default_use {
            diagnostic = diagnostic.note("the default is used here", site);
        }
        if let Some(reach) = &self.reach {
            let (last, through) = reach.path.split_last().expect("a path has a function");
            let name = self.pass.func_name(*last);
            let system = reach.system;
            let note = if through.is_empty() {
                format!("GPU system `{system}` calls `{name}` here")
            } else {
                let through: Vec<String> =
                    through.iter().map(|f| self.pass.func_name(*f)).collect();
                format!(
                    "GPU system `{system}` reaches `{name}` from here, through {}",
                    util::name_list(&through)
                )
            };
            diagnostic = diagnostic.note(note, reach.call);
        }
        self.pass.errors.push(diagnostic);
    }

    /// `name` is not allowed here, for the reason in `label`.
    fn not_allowed(&mut self, name: &str, span: Span, label: &str) {
        self.report(
            format!("`{name}` is not allowed {}", self.here()),
            span,
            label,
        );
    }

    // ----- queries -----

    /// One `each` of a GPU system: its terms, then its block.
    fn gpu_block(
        &mut self,
        query: &'p Query,
        params: &'p [LocalId],
        body: &'p Block,
        source: Option<(&'p ast::Query, &'p ast::Block)>,
    ) {
        self.source = source.map(|(_, body)| body);
        if let Some((source, _)) = source {
            self.terms(query, source);
        }
        for param in params {
            if self.locals[param.0].kind != LocalKind::View {
                self.entity_params.push(*param);
            }
        }
        self.each_depth = 1;
        self.block(body, Pos::Direct);
    }

    /// Reports each component term that a GPU system can't read or write:
    /// one with a string or a list at any depth, or with a number field
    /// of 64 bits.
    fn terms(&mut self, query: &Query, source: &ast::Query) {
        for (term, name) in query.terms.iter().zip(&source.terms) {
            let Term::Component { component, .. } = term else {
                continue;
            };
            let Some(record) = self.pass.program.components[component.0].record else {
                continue;
            };
            let unfit = self.pass.fields(record, &|ty| {
                matches!(ty, Type::String | Type::List(_) | Type::Float | Type::Int)
            });
            let Some((_, first)) = unfit.first() else {
                continue;
            };
            // Every field with the first one's problem is named.
            let paths: Vec<String> = unfit
                .iter()
                .filter(|(_, ty)| ty == first)
                .map(|(path, _)| path.clone())
                .collect();
            let names = util::name_list(&paths);
            let ty = self.pass.type_name(first);
            let (fields, them) = match paths.as_slice() {
                [_] => (format!("its field {names} is {}", a(&ty)), "it"),
                _ => (format!("its fields {names} are `{ty}`s"), "them"),
            };
            let label = match first {
                Type::Float | Type::Int => format!(
                    "{fields}, 64 bits wide; the GPU holds numbers of at most 32 bits, so declare {them} `{ty}32`"
                ),
                _ => format!(
                    "{fields}; a GPU system queries plain components, with no string or list at any depth"
                ),
            };
            self.report(
                format!(
                    "`{}` can't be a term of an `each` in a GPU system",
                    name.name
                ),
                name.span,
                label,
            );
        }
    }

    /// An `each` inside a GPU block or inside a function. In a GPU block
    /// its parameters are read-only, whether or not it may be there.
    fn each(
        &mut self,
        stmt: &'p Stmt,
        query: &'p Query,
        params: &'p [LocalId],
        body: &'p Block,
        pos: Pos,
    ) {
        const ONE: &str = "a GPU block holds one nested `each`, directly inside it";
        if self.reach.is_some() {
            self.not_allowed(
                "each",
                stmt.span,
                "the only queries of a GPU system are its own `each` blocks",
            );
            self.block(body, Pos::Loop);
            return;
        }
        if self.each_depth >= 2 {
            self.report(
                "`each` nested two deep in a GPU system",
                stmt.span,
                format!("{ONE}, and that one holds none"),
            );
        } else if pos == Pos::Loop {
            self.report(
                "nested `each` inside a loop in a GPU system",
                stmt.span,
                format!("{ONE}; put the loop inside the `each`"),
            );
        } else if pos == Pos::Branch {
            self.report(
                "nested `each` inside a branch in a GPU system",
                stmt.span,
                format!("{ONE}; put the test inside the `each`"),
            );
        } else if self.nested_seen {
            self.report(
                "second nested `each` in a GPU block",
                stmt.span,
                format!("{ONE}; give this one a GPU block of its own"),
            );
        } else {
            self.nested_seen = true;
            let source = self.source.and_then(|block| source_each(block, stmt.span));
            if let Some((source, _)) = source {
                self.terms(query, source);
            }
        }
        for param in params {
            if self.locals[param.0].kind == LocalKind::View {
                self.nested_params.push(*param);
            } else {
                self.entity_params.push(*param);
            }
        }
        self.each_depth += 1;
        self.loops += 1;
        self.block(body, Pos::Loop);
        self.loops -= 1;
        self.each_depth -= 1;
    }

    // ----- statements -----

    fn block(&mut self, block: &'p Block, pos: Pos) {
        for stmt in &block.stmts {
            self.stmt(stmt, pos);
        }
        if let Some(value) = &block.value {
            self.expr(value);
        }
    }

    fn loop_body(&mut self, body: &'p Block) {
        self.loops += 1;
        self.block(body, Pos::Loop);
        self.loops -= 1;
    }

    fn stmt(&mut self, stmt: &'p Stmt, pos: Pos) {
        let span = stmt.span;
        match &stmt.kind {
            StmtKind::Expr(e) | StmtKind::Let { init: e, .. } => self.expr(e),
            StmtKind::Assign { place, value } => {
                if !self.write(place) {
                    self.write_unsigned(place);
                    self.place(place);
                }
                self.expr(value);
            }
            StmtKind::CompoundAssign { place, op, value } => {
                if !self.write(place) {
                    self.write_unsigned(place);
                    self.place(place);
                }
                match op {
                    CompoundOp::Builtin(_) => self.expr(value),
                    CompoundOp::Method(func) => self.call(*func, span, std::slice::from_ref(value)),
                }
            }
            StmtKind::Return(value) => {
                if self.reach.is_none() {
                    self.not_allowed(
                        "return",
                        span,
                        &format!("{NO_ORDER}; `next` ends the run for one entity"),
                    );
                }
                if let Some(value) = value {
                    self.expr(value);
                }
            }
            StmtKind::Break => {
                if self.reach.is_none() && self.loops == 0 {
                    self.report(
                        "`break` can't leave the `each` of a GPU system",
                        span,
                        format!("{NO_ORDER}; `next` ends the run for this entity"),
                    );
                }
            }
            StmtKind::Next => {}
            StmtKind::While { cond, body } => {
                self.expr(cond);
                self.loop_body(body);
            }
            StmtKind::Count { from, to, body, .. } => {
                self.expr(from);
                self.expr(to);
                self.loop_body(body);
            }
            StmtKind::ListEach { list, body, .. } => {
                self.expr(list);
                self.loop_body(body);
            }
            StmtKind::Each {
                query,
                params,
                body,
            } => self.each(stmt, query, params, body, pos),
            StmtKind::On { body, .. } => {
                self.not_allowed(
                    "on",
                    span,
                    "events are received on the CPU; handle them in an ordinary system",
                );
                self.loop_body(body);
            }
            StmtKind::Puts(_) | StmtKind::Print(_) => {
                let name = if matches!(stmt.kind, StmtKind::Puts(_)) {
                    "puts"
                } else {
                    "print"
                };
                self.not_allowed(
                    name,
                    span,
                    "the GPU can't write output; print from an ordinary system",
                );
            }
            StmtKind::Emit { .. } => self.not_allowed(
                "emit",
                span,
                "events are sent on the CPU; emit from an ordinary system",
            ),
        }
    }

    /// Reports an assignment through a place that is read-only in GPU
    /// code. Returns whether it did.
    fn write(&mut self, place: &'p Place) -> bool {
        const WRITES: &str =
            "a GPU system writes only to the component parameters of its outer `each`";
        let mut root = place;
        while let PlaceKind::Field { base, .. } | PlaceKind::Index { base, .. } = &root.kind {
            root = base;
        }
        let label = match &root.kind {
            PlaceKind::ResourceField(..) => format!("resources are read-only there: {WRITES}"),
            PlaceKind::Time(_) => format!("`Time` is read-only there: {WRITES}"),
            PlaceKind::Local(id) if self.nested_params.contains(id) => format!(
                "`{}` is a parameter of the nested `each`, which is read-only: {WRITES}",
                self.locals[id.0].name
            ),
            PlaceKind::Local(id) if self.entity_params.contains(id) => format!(
                "`{}` is an `Entity` parameter, which is read-only in a GPU system",
                self.locals[id.0].name
            ),
            _ => return false,
        };
        let target = self.describe(place);
        self.report(
            format!("can't assign to `{target}` {}", self.here()),
            place.span,
            label,
        );
        true
    }

    /// Reports an assignment to a `UInt32` field.
    fn write_unsigned(&mut self, place: &Place) {
        if !is_unsigned(&place.ty) {
            return;
        }
        let target = self.describe(place);
        self.report(
            format!("can't assign to `{target}` {}", self.here()),
            place.span,
            format!("it is a `UInt32`, {UNSIGNED}; declare it `Int32`"),
        );
    }

    /// Reports a read of field `field` of a `record` value when the field
    /// is a `UInt32`.
    fn read_unsigned(&mut self, record: &Type, field: usize, span: Span) {
        let Type::Record(id) = record else {
            return;
        };
        let item = &self.pass.program.records[id.0];
        let field = &item.fields[field];
        if !is_unsigned(&field.ty) {
            return;
        }
        self.report(
            format!(
                "field `{}` of `{}` can't be read {}",
                field.name,
                item.name,
                self.here()
            ),
            span,
            format!("it is a `UInt32`, {UNSIGNED}; declare it `Int32`"),
        );
    }

    /// A place as it is written in source: `b.x`, `Score.points`.
    fn describe(&self, place: &Place) -> String {
        let program = self.pass.program;
        match &place.kind {
            PlaceKind::Local(id) => self.locals[id.0].name.clone(),
            PlaceKind::Const(id) => program.consts[id.0].name.clone(),
            PlaceKind::ResourceField(id, field) => {
                let resource = &program.resources[id.0];
                format!("{}.{}", resource.name, resource.fields[*field].name)
            }
            PlaceKind::Time(field) => match field {
                TimeField::Tick => "Time.tick".to_string(),
                TimeField::Dt => "Time.dt".to_string(),
                TimeField::Elapsed => "Time.elapsed".to_string(),
            },
            PlaceKind::Component {
                entity, component, ..
            } => {
                let entity = match &entity.kind {
                    ExprKind::Read(place) => self.describe(place),
                    _ => "…".to_string(),
                };
                format!("{entity}[{}]", program.components[component.0].name)
            }
            PlaceKind::Field { base, field } => match base.ty {
                Type::Record(record) => format!(
                    "{}.{}",
                    self.describe(base),
                    program.records[record.0].fields[*field].name
                ),
                _ => self.describe(base),
            },
            PlaceKind::Index { base, .. } => format!("{}[…]", self.describe(base)),
        }
    }

    // ----- expressions -----

    /// Checks what a place reads on the way to its value. A place that
    /// starts at a local is not looked at: the local's first value, its
    /// parameter type or its query term has been.
    fn place(&mut self, place: &'p Place) {
        let reads = format!("a GPU system reads {GPU_VALUES}");
        let program = self.pass.program;
        match &place.kind {
            PlaceKind::Local(_) | PlaceKind::Time(_) => {}
            PlaceKind::Const(id) => {
                let constant = &program.consts[id.0];
                if let Some(what) = self.pass.not_a_gpu_value(&constant.ty, &reads) {
                    self.report(
                        format!("constant `{}` can't be read {}", constant.name, self.here()),
                        place.span,
                        format!("it is {what}"),
                    );
                }
                if let Some(value) = self.pass.known_ints[id.0]
                    && i32::try_from(value).is_err()
                {
                    self.report(
                        format!(
                            "constant `{}` is {value}, which does not fit in 32 bits",
                            constant.name
                        ),
                        place.span,
                        format!(
                            "{} an `Int` holds {} to {}",
                            self.here(),
                            i32::MIN,
                            i32::MAX
                        ),
                    );
                }
            }
            PlaceKind::ResourceField(id, field) => {
                let resource = &program.resources[id.0];
                let field = &resource.fields[*field];
                let what = if is_unsigned(&field.ty) {
                    Some(format!("a `UInt32`, {UNSIGNED}; declare it `Int32`"))
                } else {
                    self.pass.not_a_gpu_value(&field.ty, &reads)
                };
                if let Some(what) = what {
                    self.report(
                        format!(
                            "resource field `{}.{}` can't be read {}",
                            resource.name,
                            field.name,
                            self.here()
                        ),
                        place.span,
                        format!("it is {what}"),
                    );
                }
            }
            PlaceKind::Component {
                entity, component, ..
            } => {
                let name = &program.components[component.0].name;
                self.report(
                    format!("`entity[{name}]` is not allowed {}", self.here()),
                    place.span,
                    format!(
                        "a GPU system reaches components through its queries; make `{name}` a term of an `each`"
                    ),
                );
                self.expr(entity);
            }
            PlaceKind::Field { base, .. } => self.place(base),
            PlaceKind::Index { base, index } => {
                self.place(base);
                self.expr(index);
            }
        }
    }

    fn exprs(&mut self, exprs: &'p [Expr]) {
        for e in exprs {
            self.expr(e);
        }
    }

    fn expr(&mut self, e: &'p Expr) {
        let span = e.span;
        // A value that can't exist on the GPU is reported as a whole. A
        // read is judged by its place and a call by its function.
        match &e.kind {
            ExprKind::Read(_) | ExprKind::Call { .. } => {}
            ExprKind::Builtin {
                op: Builtin::ToS, ..
            } => {
                self.not_allowed(
                    "to_s",
                    span,
                    "it makes a `String`, and the GPU has no strings",
                );
                return;
            }
            _ => {
                if let Some(unfit) = self.pass.unfit(&e.ty) {
                    let ty = self.pass.type_name(&e.ty);
                    let works = format!("the GPU works on {GPU_VALUES}");
                    let label = match &unfit.field {
                        None => works,
                        Some((path, field)) => {
                            format!("its field `{path}` is {}; {works}", a(field))
                        }
                    };
                    self.report(
                        format!("`{ty}` values are not allowed {}", self.here()),
                        span,
                        label,
                    );
                    return;
                }
            }
        }
        match &e.kind {
            ExprKind::Int(value) => {
                if i32::try_from(*value).is_err() {
                    self.report(
                        format!("{value} does not fit in 32 bits"),
                        span,
                        format!(
                            "{} an `Int` holds {} to {}",
                            self.here(),
                            i32::MIN,
                            i32::MAX
                        ),
                    );
                }
            }
            ExprKind::Float(_)
            | ExprKind::Bool(_)
            | ExprKind::Str(_)
            | ExprKind::EnumVariant(_)
            | ExprKind::EntityNone => {}
            ExprKind::Read(place) => {
                if let PlaceKind::Field { base, field } = &place.kind {
                    self.read_unsigned(&base.ty, *field, place.span);
                }
                self.place(place);
            }
            ExprKind::Field { base, field } => {
                self.read_unsigned(&base.ty, *field, span);
                self.expr(base);
            }
            ExprKind::Index { base, index } => {
                self.expr(base);
                self.expr(index);
            }
            ExprKind::ListLit(items) | ExprKind::Concat(items) => self.exprs(items),
            ExprKind::Construct(fields) => {
                self.construct_unsigned(&e.ty, span);
                for field in fields {
                    // A field that was left out is its default, which sits
                    // in the declaration of the record.
                    let outer = self.default_use;
                    if outer.is_none() && !inside(span, field.span) {
                        self.default_use = Some(span);
                    }
                    self.expr(field);
                    self.default_use = outer;
                }
            }
            // An `Int` literal made `Float` need not fit in 32 bits.
            ExprKind::IntToFloat(inner) if matches!(inner.kind, ExprKind::Int(_)) => {}
            ExprKind::Unary { expr: inner, .. } | ExprKind::IntToFloat(inner) => self.expr(inner),
            ExprKind::Binary {
                op: BinOp::Pow,
                lhs,
                rhs,
            } if self.strict && e.ty == Type::Float && !is_two(rhs) => {
                self.not_strict(
                    "`**` on `Float`",
                    span,
                    "its result differs between devices; a square, `x ** 2.0`, is allowed",
                );
                self.expr(lhs);
                self.expr(rhs);
            }
            ExprKind::Binary { lhs, rhs, .. }
            | ExprKind::And { lhs, rhs }
            | ExprKind::Or { lhs, rhs } => {
                self.expr(lhs);
                self.expr(rhs);
            }
            ExprKind::Call { func, args } => self.call(*func, span, args),
            ExprKind::Builtin { op, args } => {
                self.builtin(*op, span);
                self.exprs(args);
            }
            ExprKind::If { cond, then, else_ } => {
                self.expr(cond);
                self.block(then, Pos::Branch);
                self.block(else_, Pos::Branch);
            }
            ExprKind::Block(block) => self.block(block, Pos::Branch),
            ExprKind::Spawn(components) => {
                self.structural("spawn", span);
                for init in components {
                    if let Some(value) = &init.value {
                        self.expr(value);
                    }
                }
            }
            ExprKind::Add { entity, components } => {
                self.structural("add", span);
                self.expr(entity);
                for init in components {
                    if let Some(value) = &init.value {
                        self.expr(value);
                    }
                }
            }
            ExprKind::Remove { entity, .. } => {
                self.structural("remove", span);
                self.expr(entity);
            }
            ExprKind::Despawn(entity) => {
                self.structural("despawn", span);
                self.expr(entity);
            }
            ExprKind::Has { entity, .. } => {
                self.not_allowed(
                    "has?",
                    span,
                    "a GPU system can't look an entity up; filter the query with `with:` or `without:`",
                );
                self.expr(entity);
            }
            ExprKind::Alive(entity) => {
                self.not_allowed(
                    "alive?",
                    span,
                    "a GPU system can't look an entity up; check in an ordinary system",
                );
                self.expr(entity);
            }
            ExprKind::ListPush { list, value } => {
                self.place(list);
                self.expr(value);
            }
            ExprKind::ListDeleteAt { list, index } => {
                self.place(list);
                self.expr(index);
            }
            ExprKind::ListPop { list } | ExprKind::ListClear { list } => self.place(list),
            ExprKind::WorldTick => self.world("World.tick", span),
            ExprKind::WorldRun => self.world("World.run", span),
            ExprKind::WorldStop => self.world("World.stop", span),
            ExprKind::WorldCount(_) => self.not_allowed(
                "World.count",
                span,
                "the only queries of a GPU system are its `each` blocks; count in an ordinary system and keep the result in a resource",
            ),
        }
    }

    /// Reports the construction of a record that has a `UInt32` field of
    /// its own.
    fn construct_unsigned(&mut self, record: &Type, span: Span) {
        let Type::Record(id) = record else {
            return;
        };
        let item = &self.pass.program.records[id.0];
        let names: Vec<String> = item
            .fields
            .iter()
            .filter(|field| is_unsigned(&field.ty))
            .map(|field| field.name.clone())
            .collect();
        let (fields, them) = match names.as_slice() {
            [] => return,
            [name] => (format!("its field `{name}` is a `UInt32`"), "it"),
            _ => (
                format!("its fields {} are `UInt32`s", util::name_list(&names)),
                "them",
            ),
        };
        self.report(
            format!("`{}` can't be built {}", item.name, self.here()),
            span,
            format!("{fields}, {UNSIGNED}; declare {them} `Int32`"),
        );
    }

    fn structural(&mut self, name: &str, span: Span) {
        self.not_allowed(
            name,
            span,
            "structural changes are made on the CPU; make this one in an ordinary system",
        );
    }

    fn world(&mut self, name: &str, span: Span) {
        self.not_allowed(
            name,
            span,
            "the world is driven from the CPU; call it from ordinary code",
        );
    }

    /// In a strict system: `what` gives different results on different
    /// devices.
    fn not_strict(&mut self, what: &str, span: Span, label: &str) {
        let here = if self.reach.is_some() {
            "in a function called from a strict GPU system"
        } else {
            "in a strict GPU system"
        };
        self.report(format!("{what} cannot be used {here}"), span, label);
    }

    /// Reports a built-in function that needs the CPU, or, in a strict
    /// system, one whose result differs between devices.
    fn builtin(&mut self, op: Builtin, span: Span) {
        if self.strict {
            let name = match op {
                Builtin::Sin => Some("Math.sin"),
                Builtin::Cos => Some("Math.cos"),
                Builtin::Tan => Some("Math.tan"),
                Builtin::Atan2 => Some("Math.atan2"),
                Builtin::Exp => Some("Math.exp"),
                Builtin::Log => Some("Math.log"),
                _ => None,
            };
            if let Some(name) = name {
                self.not_strict(
                    &format!("`{name}`"),
                    span,
                    "its result differs between devices",
                );
                return;
            }
        }
        let (name, label) = match op {
            Builtin::Srand | Builtin::SrandClock => (
                "srand",
                "seed from ordinary code; `rand` in a GPU system follows that seed",
            ),
            Builtin::Clock => (
                "clock",
                "the GPU can't read the real clock; `Time` has the simulated one",
            ),
            Builtin::Sleep => ("sleep", "the GPU can't pause the program"),
            Builtin::Exit => (
                "exit",
                "the GPU can't end the program; do it from an ordinary system",
            ),
            _ => return,
        };
        self.not_allowed(name, span, label);
    }

    /// A call of a function or method from GPU code: what it takes and
    /// returns, whether it recurses, and, the first time, its body.
    fn call(&mut self, func: FuncId, span: Span, args: &'p [Expr]) {
        let program = self.pass.program;
        let callee = &program.funcs[func.0];
        let name = self.pass.func_name(func);

        let takes = format!("a function called from a GPU system takes and returns {GPU_VALUES}");
        let mut unfit = None;
        for (i, param) in callee.params.iter().enumerate() {
            let local = &callee.body.locals[param.0];
            if let Some(what) = self.pass.not_a_gpu_value(&local.ty, &takes) {
                unfit = Some(if i == 0 && callee.owner.is_some() {
                    format!("its receiver is {what}")
                } else {
                    format!("its parameter `{}` is {what}", local.name)
                });
                break;
            }
        }
        if unfit.is_none()
            && let Some(what) = self.pass.not_a_gpu_value(&callee.ret, &takes)
        {
            unfit = Some(format!("it returns {what}"));
        }
        if let Some(unfit) = unfit {
            self.report(format!("can't call `{name}` {}", self.here()), span, unfit);
            return;
        }

        self.exprs(args);

        if self.reach.is_none()
            && let Some(chain) = self.pass.cycle(func)
        {
            let recursive = *chain.last().expect("a chain has a function");
            let message = if recursive == func {
                format!("call to recursive function `{name}` in a GPU system")
            } else {
                format!(
                    "call to `{name}` in a GPU system reaches recursive function `{}`",
                    self.pass.func_name(recursive)
                )
            };
            let label = format!(
                "{}; a function called from a GPU system must not recurse",
                self.pass.chain_text(&chain)
            );
            self.report(message, span, label);
        }

        let mode = usize::from(self.strict);
        if !self.pass.reached[func.0][mode] {
            self.pass.reached[func.0][mode] = true;
            let reach = match &self.reach {
                Some(reach) => {
                    let mut reach = reach.clone();
                    reach.path.push(func);
                    reach
                }
                None => Reach {
                    system: self.system,
                    strict: self.strict,
                    call: span,
                    path: vec![func],
                },
            };
            self.pass.queue.push_back((func, reach));
        }
    }
}
