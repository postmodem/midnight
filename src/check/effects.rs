//! What code does to the world, worked out on the typed program: the
//! number of every query, the summary of every function, and where every
//! `each` runs.
//!
//! One scan reads a block in source order and reports what each statement
//! and expression does, as a list of [`Event`]s. Everything else is built
//! on those lists:
//!
//! - the call graph, which the GPU rules use too;
//! - a function's [`Summary`]: the effects of its own events, with those
//!   of each function it calls where the call stands, found by repeating
//!   until nothing changes;
//! - the verdict of each `each` that is not written inside another one:
//!   the events of its block, and at each call the effects of the
//!   function's summary, are held against the rules of an independent
//!   loop in "Several cores" of the reference. The first that breaks one
//!   is the reason the loop stays on one core; when none does the loop is
//!   independent, and gets its sums, its weight and the queries it can
//!   reach.
//!
//! The weight of a block follows its structure instead: a branch counts
//! as the heavier of its arms, term by term, and a loop as a multiple of
//! its body.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::source::Span;
use crate::tir::{
    BinOp, Block, Builtin, ComponentId, CompoundOp, Cores, Deed, Effect, EffectKind, Expr,
    ExprKind, FuncId, Local, LocalId, LocalKind, Place, PlaceKind, Program, QueryId, Reason,
    ReasonKind, Run, Stmt, StmtKind, Summary, Target, Term, Type, Via, Weight, WeightTerm,
};

use super::walk;

/// The weight of a call of a function that recurses.
const RECURSIVE_WEIGHT: u64 = 64;
/// How many times the body of a loop that is not a query is taken to run.
const LOOP_ROUNDS: u64 = 8;

/// One thing a statement or an expression does, as a scan meets it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Event {
    /// Something a summary records.
    Effect(EffectKind),
    /// A read of a local, or of part of it.
    LocalRead(LocalId),
    /// An assignment to a local that is not a query parameter, or to part
    /// of it, other than a `LocalSum`.
    LocalWrite(LocalId),
    /// `v += x` or `v -= x` with a built-in operator.
    LocalSum(LocalId),
    /// An `Int` resource field or variable is assigned itself plus or
    /// minus something. The event of the assignment follows.
    SpelledOut(Target),
    /// A `break` that leaves the block the scan started from.
    Break,
    Return,
    /// A call of a user function, after its arguments.
    Call(FuncId),
    /// An `each` or a `World.count(…)`.
    Query(QueryId),
}

/// What a scan of one block found.
struct Scanned<'p> {
    events: Vec<(Event, Span)>,
    /// Every `each` statement that is not inside another `each` of the
    /// block.
    outer: Vec<&'p Stmt>,
    /// The locals the block creates: variables, loop variables and the
    /// parameters of `each` and `on` blocks.
    declared: Vec<LocalId>,
}

struct Scan<'a, 'p> {
    /// For each record: `UsesString` or `UsesList` when it has a string or
    /// a list inside, by its first such field.
    unfit: &'a [Option<EffectKind>],
    locals: &'p [Local],
    found: Scanned<'p>,
    /// How many `each` enclose the statement being scanned.
    each_depth: usize,
}

/// Scans a block of a body with these locals.
fn scan<'p>(unfit: &[Option<EffectKind>], locals: &'p [Local], block: &'p Block) -> Scanned<'p> {
    let mut scan = Scan {
        unfit,
        locals,
        found: Scanned {
            events: Vec::new(),
            outer: Vec::new(),
            declared: Vec::new(),
        },
        each_depth: 0,
    };
    scan.block(block, 0);
    scan.found
}

/// For each record: whether a value of it holds a string or a list, by
/// its first field that is or holds one.
fn unfit_records(program: &Program) -> Vec<Option<EffectKind>> {
    fn of(program: &Program, ty: &Type) -> Option<EffectKind> {
        match ty {
            Type::String => Some(EffectKind::UsesString),
            Type::List(_) => Some(EffectKind::UsesList),
            Type::Record(id) => program.records[id.0]
                .fields
                .iter()
                .find_map(|field| of(program, &field.ty)),
            _ => None,
        }
    }
    (0..program.records.len())
        .map(|id| of(program, &Type::Record(crate::tir::RecordId(id))))
        .collect()
}

/// The place a chain of fields and indexes starts from.
fn root(place: &Place) -> &Place {
    match &place.kind {
        PlaceKind::Field { base, .. } | PlaceKind::Index { base, .. } => root(base),
        _ => place,
    }
}

/// Whether two places are the same resource field or the same local.
fn same_target(a: &Place, b: &Place) -> bool {
    match (&a.kind, &b.kind) {
        (PlaceKind::Local(x), PlaceKind::Local(y)) => x == y,
        (PlaceKind::ResourceField(r, f), PlaceKind::ResourceField(s, g)) => r == s && f == g,
        _ => false,
    }
}

impl<'p> Scan<'_, 'p> {
    fn event(&mut self, event: Event, span: Span) {
        self.found.events.push((event, span));
    }

    fn effect(&mut self, kind: EffectKind, span: Span) {
        self.event(Event::Effect(kind), span);
    }

    /// `UsesString` or `UsesList` for a type whose values are not plain.
    fn unfit(&self, ty: &Type) -> Option<EffectKind> {
        match ty {
            Type::String => Some(EffectKind::UsesString),
            Type::List(_) => Some(EffectKind::UsesList),
            Type::Record(id) => self.unfit[id.0],
            _ => None,
        }
    }

    /// `depth` counts the loops around the statements of `block` that lie
    /// inside the block the scan started from.
    fn block(&mut self, block: &'p Block, depth: usize) {
        for stmt in &block.stmts {
            self.stmt(stmt, depth);
        }
        if let Some(value) = &block.value {
            self.expr(value, depth);
        }
    }

    fn stmt(&mut self, stmt: &'p Stmt, depth: usize) {
        let span = stmt.span;
        match &stmt.kind {
            StmtKind::Expr(expr) => self.expr(expr, depth),
            StmtKind::Let { local, init } => {
                self.found.declared.push(*local);
                self.expr(init, depth);
            }
            StmtKind::Assign { place, value } => {
                if let Some(kind) = self.unfit(&place.ty.value_type()) {
                    self.effect(kind, span);
                }
                if let Some(target) = self.spelled_out(place, value) {
                    self.event(Event::SpelledOut(target), span);
                }
                self.write(place, span);
                self.operands(place, depth);
                self.expr(value, depth);
            }
            StmtKind::CompoundAssign { place, op, value } => {
                if let Some(kind) = self.unfit(&place.ty.value_type()) {
                    self.effect(kind, span);
                }
                let sums = matches!(op, CompoundOp::Builtin(BinOp::Add | BinOp::Sub));
                match place.kind {
                    PlaceKind::ResourceField(resource, field) if sums => {
                        self.effect(EffectKind::FieldSum(resource, field), span);
                    }
                    PlaceKind::Local(local)
                        if sums && self.locals[local.0].kind != LocalKind::View =>
                    {
                        self.event(Event::LocalSum(local), span);
                    }
                    _ => {
                        self.write(place, span);
                        self.read(place);
                    }
                }
                self.operands(place, depth);
                self.expr(value, depth);
                if let CompoundOp::Method(func) = op {
                    self.event(Event::Call(*func), span);
                }
            }
            StmtKind::Return(value) => {
                self.event(Event::Return, span);
                if let Some(value) = value {
                    self.expr(value, depth);
                }
            }
            StmtKind::Break => {
                if depth == 0 {
                    self.event(Event::Break, span);
                }
            }
            StmtKind::Next => {}
            StmtKind::While { cond, body } => {
                self.expr(cond, depth);
                self.block(body, depth + 1);
            }
            StmtKind::Count {
                var,
                from,
                to,
                body,
                ..
            } => {
                self.found.declared.extend(var);
                self.expr(from, depth);
                self.expr(to, depth);
                self.block(body, depth + 1);
            }
            StmtKind::ListEach {
                list,
                elem,
                index,
                body,
            } => {
                self.found.declared.extend(elem);
                self.found.declared.extend(index);
                self.expr(list, depth);
                self.block(body, depth + 1);
            }
            StmtKind::Each {
                query,
                params,
                body,
                ..
            } => {
                if self.each_depth == 0 {
                    self.found.outer.push(stmt);
                }
                self.found.declared.extend(params);
                self.event(Event::Query(query.id), span);
                for term in &query.terms {
                    if let Term::Component {
                        component,
                        write,
                        touched,
                        ..
                    } = *term
                    {
                        if touched {
                            self.effect(EffectKind::TermUse(component), span);
                        }
                        if write {
                            self.effect(EffectKind::TermWrite(component), span);
                        }
                    }
                }
                self.each_depth += 1;
                self.block(body, depth + 1);
                self.each_depth -= 1;
            }
            StmtKind::On { param, body, .. } => {
                self.found.declared.extend(param);
                self.effect(EffectKind::Does(Deed::On), span);
                self.block(body, depth + 1);
            }
            StmtKind::Puts(args) | StmtKind::Print(args) => {
                let deed = if matches!(stmt.kind, StmtKind::Puts(_)) {
                    Deed::Puts
                } else {
                    Deed::Print
                };
                self.effect(EffectKind::Does(deed), span);
                for arg in args {
                    self.expr(arg, depth);
                }
            }
            StmtKind::Emit { event, value } => {
                self.effect(EffectKind::Emit(*event), span);
                self.expr(value, depth);
            }
            StmtKind::WindowOpen {
                title,
                width,
                height,
            } => {
                self.effect(EffectKind::Does(Deed::WindowOpen), span);
                self.expr(title, depth);
                self.expr(width, depth);
                self.expr(height, depth);
            }
        }
    }

    /// The target of `place = place + x`, `place = place - x` or
    /// `place = x + place`, when the place is an `Int` resource field or
    /// an `Int` variable.
    fn spelled_out(&self, place: &Place, value: &Expr) -> Option<Target> {
        let target = match place.kind {
            PlaceKind::ResourceField(resource, field) if place.ty == Type::Int => {
                Target::Field(resource, field)
            }
            PlaceKind::Local(local) if self.locals[local.0].ty == Type::Int => Target::Local(local),
            _ => return None,
        };
        let ExprKind::Binary { op, lhs, rhs } = &value.kind else {
            return None;
        };
        let is_place =
            |expr: &Expr| matches!(&expr.kind, ExprKind::Read(read) if same_target(read, place));
        let itself = match op {
            BinOp::Add => is_place(lhs) || is_place(rhs),
            BinOp::Sub => is_place(lhs),
            _ => false,
        };
        itself.then_some(target)
    }

    /// The event of a read of a place, by what the place starts from.
    fn read(&mut self, place: &Place) {
        let start = root(place);
        match start.kind {
            PlaceKind::Local(local) => self.event(Event::LocalRead(local), start.span),
            PlaceKind::ResourceField(resource, field) => {
                self.effect(EffectKind::FieldRead(resource, field), start.span);
            }
            PlaceKind::Component { component, .. } => {
                self.effect(EffectKind::RowRead(component), start.span);
            }
            PlaceKind::Const(_) | PlaceKind::Time(_) => {}
            PlaceKind::Field { .. } | PlaceKind::Index { .. } => unreachable!("a root"),
        }
    }

    /// The event of an assignment to a place or to part of it, by what the
    /// place starts from. An assignment through a query parameter has
    /// none: its term says so.
    fn write(&mut self, place: &Place, span: Span) {
        let start = root(place);
        match start.kind {
            PlaceKind::Local(local) => {
                if self.locals[local.0].kind != LocalKind::View {
                    self.event(Event::LocalWrite(local), span);
                }
            }
            PlaceKind::ResourceField(resource, field) => {
                self.effect(EffectKind::FieldWrite(resource, field), span);
            }
            PlaceKind::Time(_) => self.effect(EffectKind::DtWrite, span),
            PlaceKind::Component { component, .. } => {
                self.effect(EffectKind::RowWrite(component), start.span);
            }
            PlaceKind::Const(_) => {}
            PlaceKind::Field { .. } | PlaceKind::Index { .. } => unreachable!("a root"),
        }
    }

    /// The expressions inside a place: the entity of `e[C]` and the
    /// indexes. An index also means a list is used.
    fn operands(&mut self, place: &'p Place, depth: usize) {
        match &place.kind {
            PlaceKind::Local(_)
            | PlaceKind::Const(_)
            | PlaceKind::ResourceField(..)
            | PlaceKind::Time(_) => {}
            PlaceKind::Component { entity, .. } => self.expr(entity, depth),
            PlaceKind::Field { base, .. } => self.operands(base, depth),
            PlaceKind::Index { base, index } => {
                self.operands(base, depth);
                self.effect(EffectKind::UsesList, place.span);
                self.expr(index, depth);
            }
        }
    }

    fn exprs(&mut self, exprs: &'p [Expr], depth: usize) {
        for expr in exprs {
            self.expr(expr, depth);
        }
    }

    fn expr(&mut self, expr: &'p Expr, depth: usize) {
        let span = expr.span;
        if let Some(kind) = self.unfit(&expr.ty) {
            self.effect(kind, span);
        }
        match &expr.kind {
            ExprKind::Int(_)
            | ExprKind::Float(_)
            | ExprKind::Bool(_)
            | ExprKind::Str(_)
            | ExprKind::EnumVariant(_)
            | ExprKind::EntityNone
            | ExprKind::WorldCount(None) => {}
            ExprKind::Read(place) => {
                self.read(place);
                self.operands(place, depth);
            }
            ExprKind::Field { base, .. } => self.expr(base, depth),
            ExprKind::Index { base, index } => {
                self.expr(base, depth);
                self.expr(index, depth);
            }
            ExprKind::ListLit(items) | ExprKind::Construct(items) | ExprKind::Concat(items) => {
                self.exprs(items, depth);
            }
            ExprKind::Unary { expr: inner, .. }
            | ExprKind::IntToFloat(inner)
            | ExprKind::Has { entity: inner, .. }
            | ExprKind::Alive(inner)
            | ExprKind::Input { arg: inner, .. } => self.expr(inner, depth),
            ExprKind::Binary { lhs, rhs, .. }
            | ExprKind::And { lhs, rhs }
            | ExprKind::Or { lhs, rhs } => {
                self.expr(lhs, depth);
                self.expr(rhs, depth);
            }
            ExprKind::Call { func, args } => {
                self.exprs(args, depth);
                self.event(Event::Call(*func), span);
            }
            ExprKind::Builtin { op, args } => {
                let deed = match op {
                    Builtin::Rand | Builtin::RandInt => Some(Deed::Rand),
                    Builtin::Srand | Builtin::SrandClock => Some(Deed::Srand),
                    Builtin::Clock => Some(Deed::Clock),
                    Builtin::Sleep => Some(Deed::Sleep),
                    Builtin::Exit => Some(Deed::Exit),
                    _ => None,
                };
                if let Some(deed) = deed {
                    self.effect(EffectKind::Does(deed), span);
                }
                self.exprs(args, depth);
            }
            ExprKind::If { cond, then, else_ } => {
                self.expr(cond, depth);
                self.block(then, depth);
                self.block(else_, depth);
            }
            ExprKind::Block(block) => self.block(block, depth),
            ExprKind::Spawn(components) => {
                self.effect(EffectKind::Does(Deed::Spawn), span);
                for init in components {
                    if let Some(value) = &init.value {
                        self.expr(value, depth);
                    }
                }
            }
            ExprKind::Add { entity, components } => {
                self.effect(EffectKind::Add, span);
                self.expr(entity, depth);
                for init in components {
                    if let Some(value) = &init.value {
                        self.expr(value, depth);
                    }
                }
            }
            ExprKind::Remove { entity, .. } => {
                self.effect(EffectKind::Remove, span);
                self.expr(entity, depth);
            }
            ExprKind::Despawn(entity) => {
                self.effect(EffectKind::Despawn, span);
                self.expr(entity, depth);
            }
            ExprKind::ListPush { list, value } => {
                self.list_change(list, span, depth);
                self.expr(value, depth);
            }
            ExprKind::ListPop { list } | ExprKind::ListClear { list } => {
                self.list_change(list, span, depth);
            }
            ExprKind::ListDeleteAt { list, index } => {
                self.list_change(list, span, depth);
                self.expr(index, depth);
            }
            ExprKind::WorldTick => self.effect(EffectKind::Does(Deed::WorldTick), span),
            ExprKind::WorldRun => self.effect(EffectKind::Does(Deed::WorldRun), span),
            ExprKind::WorldStop => self.effect(EffectKind::Does(Deed::WorldStop), span),
            ExprKind::WorldCount(Some(query)) => self.event(Event::Query(query.id), span),
        }
    }

    /// `push`, `pop`, `delete_at` and `clear` on a list place.
    fn list_change(&mut self, list: &'p Place, span: Span, depth: usize) {
        self.effect(EffectKind::UsesList, span);
        self.write(list, span);
        self.operands(list, depth);
    }
}

/// The functions each function calls, in the order of their first call.
pub(super) fn call_graph(program: &Program) -> Vec<Vec<FuncId>> {
    let unfit = unfit_records(program);
    program
        .funcs
        .iter()
        .map(|func| {
            let scanned = scan(&unfit, &func.body.locals, &func.body.block);
            callees(&scanned.events)
        })
        .collect()
}

fn callees(events: &[(Event, Span)]) -> Vec<FuncId> {
    let mut callees = Vec::new();
    for (event, _) in events {
        if let Event::Call(func) = event
            && !callees.contains(func)
        {
            callees.push(*func);
        }
    }
    callees
}

/// A sum of products of row counts, by the queries whose rows are
/// multiplied. No factor is zero.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Poly(BTreeMap<Vec<QueryId>, u64>);

impl Poly {
    fn constant(value: u64) -> Poly {
        let mut poly = Poly::default();
        poly.add_term(Vec::new(), value);
        poly
    }

    fn add_term(&mut self, rows: Vec<QueryId>, factor: u64) {
        if factor == 0 {
            return;
        }
        let entry = self.0.entry(rows).or_insert(0);
        *entry = entry.saturating_add(factor).min(Weight::LIMIT);
    }

    fn add(&mut self, other: Poly) {
        for (rows, factor) in other.0 {
            self.add_term(rows, factor);
        }
    }

    fn plus(mut self, value: u64) -> Poly {
        self.add_term(Vec::new(), value);
        self
    }

    fn scaled(self, by: u64) -> Poly {
        let mut out = Poly::default();
        for (rows, factor) in self.0 {
            out.add_term(rows, factor.saturating_mul(by).min(Weight::LIMIT));
        }
        out
    }

    /// Every term once for each row of a query.
    fn times_rows(self, query: QueryId) -> Poly {
        let mut out = Poly::default();
        for (mut rows, factor) in self.0 {
            rows.push(query);
            rows.sort();
            out.add_term(rows, factor);
        }
        out
    }

    /// The heavier of two, term by term.
    fn heavier(mut self, other: Poly) -> Poly {
        for (rows, factor) in other.0 {
            let entry = self.0.entry(rows).or_insert(0);
            *entry = (*entry).max(factor);
        }
        self
    }

    fn weight(&self) -> Weight {
        let mut terms: Vec<WeightTerm> = self
            .0
            .iter()
            .map(|(rows, factor)| WeightTerm {
                factor: *factor,
                rows: rows.clone(),
            })
            .collect();
        terms.sort_by(|a, b| (a.rows.len(), &a.rows).cmp(&(b.rows.len(), &b.rows)));
        Weight { terms }
    }
}

/// Works out weights. The weight of a function is found when a call of it
/// is first weighed.
struct Weigher<'p> {
    program: &'p Program,
    /// By function: it can call itself, directly or through others.
    recursive: Vec<bool>,
    funcs: Vec<Option<Poly>>,
}

fn binary_weight(op: BinOp) -> u64 {
    match op {
        BinOp::Div | BinOp::Mod | BinOp::Pow => 8,
        _ => 1,
    }
}

fn builtin_weight(op: Builtin) -> u64 {
    match op {
        Builtin::IntAbs
        | Builtin::IntClamp
        | Builtin::IntMin
        | Builtin::IntMax
        | Builtin::FloatAbs
        | Builtin::FloatToI
        | Builtin::FloatFloor
        | Builtin::FloatCeil
        | Builtin::FloatRound
        | Builtin::FloatNan
        | Builtin::FloatClamp
        | Builtin::FloatMin
        | Builtin::FloatMax
        | Builtin::StrBytesize
        | Builtin::StrEmpty
        | Builtin::ListLength
        | Builtin::ListEmpty
        | Builtin::ListFirst
        | Builtin::ListLast => 1,
        _ => 8,
    }
}

/// 1 for a place that is a field or an element of something: a field
/// read. A variable, a constant, a resource field and a field of `Time`
/// weigh nothing.
fn load_weight(place: &Place) -> u64 {
    matches!(
        place.kind,
        PlaceKind::Field { .. } | PlaceKind::Index { .. }
    ) as u64
}

/// 1 for a store anywhere but into a whole variable: a field write.
fn store_weight(place: &Place, locals: &[Local]) -> u64 {
    match place.kind {
        PlaceKind::Local(local) => (locals[local.0].kind == LocalKind::View) as u64,
        _ => 1,
    }
}

impl Weigher<'_> {
    fn func(&mut self, func: FuncId) -> Poly {
        if self.recursive[func.0] {
            return Poly::constant(RECURSIVE_WEIGHT);
        }
        if let Some(known) = &self.funcs[func.0] {
            return known.clone();
        }
        let body = &self.program.funcs[func.0].body;
        let weight = self.block(&body.block, &body.locals);
        self.funcs[func.0] = Some(weight.clone());
        weight
    }

    fn block(&mut self, block: &Block, locals: &[Local]) -> Poly {
        let mut total = Poly::default();
        for stmt in &block.stmts {
            total.add(self.stmt(stmt, locals));
        }
        if let Some(value) = &block.value {
            total.add(self.expr(value, locals));
        }
        total
    }

    fn stmt(&mut self, stmt: &Stmt, locals: &[Local]) -> Poly {
        match &stmt.kind {
            StmtKind::Expr(expr) | StmtKind::Let { init: expr, .. } => self.expr(expr, locals),
            StmtKind::Assign { place, value } => {
                let mut total = self.place(place, locals);
                total.add(self.expr(value, locals));
                total.plus(store_weight(place, locals))
            }
            StmtKind::CompoundAssign { place, op, value } => {
                let mut total = self.place(place, locals);
                total.add(self.expr(value, locals));
                match op {
                    CompoundOp::Builtin(op) => total.add_term(Vec::new(), binary_weight(*op)),
                    CompoundOp::Method(func) => total.add(self.func(*func)),
                }
                total.plus(load_weight(place) + store_weight(place, locals))
            }
            StmtKind::Return(value) => match value {
                Some(value) => self.expr(value, locals),
                None => Poly::default(),
            },
            StmtKind::Break | StmtKind::Next => Poly::default(),
            StmtKind::While { cond, body } => {
                let mut round = self.expr(cond, locals);
                round.add(self.block(body, locals));
                round.scaled(LOOP_ROUNDS)
            }
            StmtKind::Count { from, to, body, .. } => {
                let mut total = self.expr(from, locals);
                total.add(self.expr(to, locals));
                total.add(self.block(body, locals).scaled(LOOP_ROUNDS));
                total
            }
            StmtKind::ListEach { list, body, .. } => {
                let mut total = self.expr(list, locals);
                total.add(self.block(body, locals).scaled(LOOP_ROUNDS));
                total
            }
            StmtKind::Each { query, body, .. } => self.block(body, locals).times_rows(query.id),
            StmtKind::On { body, .. } => self.block(body, locals).scaled(LOOP_ROUNDS),
            StmtKind::Puts(args) | StmtKind::Print(args) => self.exprs(args, locals).plus(8),
            StmtKind::Emit { value, .. } => self.expr(value, locals).plus(8),
            StmtKind::WindowOpen {
                title,
                width,
                height,
            } => {
                let mut total = self.expr(title, locals);
                total.add(self.expr(width, locals));
                total.add(self.expr(height, locals));
                total.plus(8)
            }
        }
    }

    /// The expressions inside a place, and 4 for `e[C]`.
    fn place(&mut self, place: &Place, locals: &[Local]) -> Poly {
        match &place.kind {
            PlaceKind::Local(_)
            | PlaceKind::Const(_)
            | PlaceKind::ResourceField(..)
            | PlaceKind::Time(_) => Poly::default(),
            PlaceKind::Component { entity, .. } => self.expr(entity, locals).plus(4),
            PlaceKind::Field { base, .. } => self.place(base, locals),
            PlaceKind::Index { base, index } => {
                let mut total = self.place(base, locals);
                total.add(self.expr(index, locals));
                total
            }
        }
    }

    fn exprs(&mut self, exprs: &[Expr], locals: &[Local]) -> Poly {
        let mut total = Poly::default();
        for expr in exprs {
            total.add(self.expr(expr, locals));
        }
        total
    }

    fn inits(&mut self, inits: &[crate::tir::ComponentInit], locals: &[Local]) -> Poly {
        let mut total = Poly::default();
        for init in inits {
            if let Some(value) = &init.value {
                total.add(self.expr(value, locals));
            }
        }
        total
    }

    fn expr(&mut self, expr: &Expr, locals: &[Local]) -> Poly {
        match &expr.kind {
            ExprKind::Int(_)
            | ExprKind::Float(_)
            | ExprKind::Bool(_)
            | ExprKind::Str(_)
            | ExprKind::EnumVariant(_)
            | ExprKind::EntityNone => Poly::default(),
            ExprKind::Read(place) => self.place(place, locals).plus(load_weight(place)),
            ExprKind::Field { base, .. } => self.expr(base, locals).plus(1),
            ExprKind::Index { base, index } => {
                let mut total = self.expr(base, locals);
                total.add(self.expr(index, locals));
                total.plus(1)
            }
            ExprKind::Construct(items) => self.exprs(items, locals),
            ExprKind::ListLit(items) | ExprKind::Concat(items) => self.exprs(items, locals).plus(8),
            ExprKind::IntToFloat(inner) => self.expr(inner, locals),
            ExprKind::Unary { expr: inner, .. }
            | ExprKind::Has { entity: inner, .. }
            | ExprKind::Alive(inner)
            | ExprKind::Input { arg: inner, .. } => self.expr(inner, locals).plus(1),
            ExprKind::Binary { op, lhs, rhs } => {
                let mut total = self.expr(lhs, locals);
                total.add(self.expr(rhs, locals));
                total.plus(binary_weight(*op))
            }
            ExprKind::And { lhs, rhs } | ExprKind::Or { lhs, rhs } => {
                let mut total = self.expr(lhs, locals);
                total.add(self.expr(rhs, locals));
                total.plus(1)
            }
            ExprKind::Call { func, args } => {
                let mut total = self.exprs(args, locals);
                total.add(self.func(*func));
                total
            }
            ExprKind::Builtin { op, args } => self.exprs(args, locals).plus(builtin_weight(*op)),
            ExprKind::If { cond, then, else_ } => {
                let mut total = self.expr(cond, locals);
                let then = self.block(then, locals);
                total.add(then.heavier(self.block(else_, locals)));
                total
            }
            ExprKind::Block(block) => self.block(block, locals),
            ExprKind::Spawn(inits) => self.inits(inits, locals).plus(8),
            ExprKind::Add { entity, components } => {
                let mut total = self.expr(entity, locals);
                total.add(self.inits(components, locals));
                total.plus(8)
            }
            ExprKind::Remove { entity, .. } | ExprKind::Despawn(entity) => {
                self.expr(entity, locals).plus(8)
            }
            ExprKind::ListPush { list, value } => {
                let mut total = self.place(list, locals);
                total.add(self.expr(value, locals));
                total.plus(8)
            }
            ExprKind::ListPop { list } | ExprKind::ListClear { list } => {
                self.place(list, locals).plus(8)
            }
            ExprKind::ListDeleteAt { list, index } => {
                let mut total = self.place(list, locals);
                total.add(self.expr(index, locals));
                total.plus(8)
            }
            ExprKind::WorldTick | ExprKind::WorldRun | ExprKind::WorldStop => Poly::constant(8),
            ExprKind::WorldCount(_) => Poly::constant(4),
        }
    }
}

/// For each function, whether it can call itself, directly or through
/// others.
fn recursive(calls: &[Vec<FuncId>]) -> Vec<bool> {
    (0..calls.len())
        .map(|start| {
            let mut seen = vec![false; calls.len()];
            let mut stack: Vec<FuncId> = calls[start].clone();
            while let Some(func) = stack.pop() {
                if func.0 == start {
                    return true;
                }
                if !std::mem::replace(&mut seen[func.0], true) {
                    stack.extend(&calls[func.0]);
                }
            }
            false
        })
        .collect()
}

/// The effects of each function: those of its own events, in order, with
/// the effects of each function it calls where the call stands, each kind
/// once.
fn effects(direct: &[Vec<(Event, Span)>]) -> Vec<Vec<Effect>> {
    let mut found: Vec<Vec<Effect>> = vec![Vec::new(); direct.len()];
    // Once no function gains an effect, only the order of first uses can
    // still move, one call deeper with each round.
    let mut settled = 0;
    while settled <= direct.len() {
        let mut changed = false;
        let mut grew = false;
        for (func, events) in direct.iter().enumerate() {
            let mut list: Vec<Effect> = Vec::new();
            let mut seen: HashSet<EffectKind> = HashSet::new();
            for (event, span) in events {
                match event {
                    Event::Effect(kind) if seen.insert(*kind) => {
                        list.push(Effect {
                            kind: *kind,
                            func: FuncId(func),
                            span: *span,
                        });
                    }
                    Event::Call(callee) => {
                        for effect in &found[callee.0] {
                            if seen.insert(effect.kind) {
                                list.push(*effect);
                            }
                        }
                    }
                    _ => {}
                }
            }
            if list != found[func] {
                changed = true;
                grew |= list.len() > found[func].len();
                found[func] = list;
            }
        }
        if !changed {
            break;
        }
        if !grew {
            settled += 1;
        }
    }
    found
}

/// The queries each function can reach, ascending.
fn reachable_queries(direct: &[Vec<(Event, Span)>], calls: &[Vec<FuncId>]) -> Vec<Vec<QueryId>> {
    let mut found: Vec<Vec<QueryId>> = direct.iter().map(|events| queries_of(events)).collect();
    let mut changed = true;
    while changed {
        changed = false;
        for (func, callees) in calls.iter().enumerate() {
            for callee in callees {
                let gained: Vec<QueryId> = found[callee.0]
                    .iter()
                    .filter(|query| !found[func].contains(query))
                    .copied()
                    .collect();
                changed |= !gained.is_empty();
                found[func].extend(gained);
            }
        }
    }
    for list in &mut found {
        list.sort();
    }
    found
}

fn queries_of(events: &[(Event, Span)]) -> Vec<QueryId> {
    let mut queries = Vec::new();
    for (event, _) in events {
        if let Event::Query(query) = event
            && !queries.contains(query)
        {
            queries.push(*query);
        }
    }
    queries
}

/// What the verdict of one `each` is judged against.
struct Judge<'p> {
    program: &'p Program,
    locals: &'p [Local],
    /// By local: created by the `each` or inside its block.
    inside: Vec<bool>,
    /// By component: the `each` assigns through its own parameter for it.
    assigned: Vec<bool>,
    /// By component: the first GPU system that names it as a term.
    gpu: &'p [Option<usize>],
    /// The totals of the block, in the order of their first sum.
    sums: Vec<Target>,
}

impl Judge<'_> {
    fn field_type(&self, resource: crate::tir::ResourceId, field: usize) -> &Type {
        &self.program.resources[resource.0].fields[field].ty
    }

    /// The rule an event breaks, if any. `called` when the event is an
    /// effect of a function the block calls.
    fn breaks(&self, event: Event, called: bool) -> Option<ReasonKind> {
        let summed = |target: Target| self.sums.contains(&target);
        let outer = |local: LocalId| !self.inside[local.0];
        let reads = |component: ComponentId, nested: bool| {
            if self.assigned[component.0] {
                Some(ReasonKind::ReadsAssigned { component, nested })
            } else {
                self.gpu[component.0].map(|system| ReasonKind::ReadsGpu {
                    component,
                    system,
                    nested,
                })
            }
        };
        let assigns = |target: Target| ReasonKind::Assigns {
            target,
            spelled_out: false,
        };
        match event {
            Event::Effect(kind) => match kind {
                EffectKind::UsesString => Some(ReasonKind::UsesString),
                EffectKind::UsesList => Some(ReasonKind::UsesList),
                EffectKind::RowWrite(component) => Some(ReasonKind::AssignsRow(component)),
                EffectKind::TermWrite(component) => Some(ReasonKind::AssignsNested(component)),
                EffectKind::FieldWrite(resource, field) => {
                    let target = Target::Field(resource, field);
                    Some(if summed(target) {
                        ReasonKind::SummedAndAssigned(target)
                    } else {
                        assigns(target)
                    })
                }
                EffectKind::DtWrite => Some(ReasonKind::AssignsDt),
                EffectKind::RowRead(component) => reads(component, false),
                EffectKind::TermUse(component) => reads(component, true),
                EffectKind::FieldSum(resource, field) => {
                    let target = Target::Field(resource, field);
                    match self.field_type(resource, field) {
                        Type::Int if called => Some(ReasonKind::SumInFunction(target)),
                        Type::Int => None,
                        Type::Float | Type::Compact(crate::tir::Compact::Float32) => {
                            Some(ReasonKind::FloatSum(target))
                        }
                        Type::Compact(_) => Some(ReasonKind::CompactSum(target)),
                        _ => Some(assigns(target)),
                    }
                }
                EffectKind::FieldRead(resource, field) => {
                    let target = Target::Field(resource, field);
                    summed(target).then_some(ReasonKind::SummedAndRead(target))
                }
                EffectKind::Despawn
                | EffectKind::Add
                | EffectKind::Remove
                | EffectKind::Emit(_) => None,
                EffectKind::Does(deed) => Some(ReasonKind::Does(deed)),
            },
            Event::LocalRead(local) => {
                let target = Target::Local(local);
                summed(target).then_some(ReasonKind::SummedAndRead(target))
            }
            Event::LocalWrite(local) if outer(local) => {
                let target = Target::Local(local);
                Some(if summed(target) {
                    ReasonKind::SummedAndAssigned(target)
                } else {
                    assigns(target)
                })
            }
            Event::LocalSum(local) if outer(local) => {
                let target = Target::Local(local);
                match self.locals[local.0].ty {
                    Type::Int => None,
                    Type::Float => Some(ReasonKind::FloatSum(target)),
                    _ => Some(assigns(target)),
                }
            }
            Event::SpelledOut(target) => {
                let applies = match target {
                    Target::Field(..) => true,
                    Target::Local(local) => outer(local),
                };
                applies.then_some(ReasonKind::Assigns {
                    target,
                    spelled_out: true,
                })
            }
            Event::Break => Some(ReasonKind::Does(Deed::Break)),
            Event::Return => Some(ReasonKind::Does(Deed::Return)),
            Event::LocalWrite(_) | Event::LocalSum(_) | Event::Call(_) | Event::Query(_) => None,
        }
    }
}

/// Everything the verdicts of a program are worked out from.
struct Analysis<'p> {
    program: &'p Program,
    unfit: Vec<Option<EffectKind>>,
    summaries: Vec<Summary>,
    gpu: Vec<Option<usize>>,
    weigher: Weigher<'p>,
    /// The verdict of each `each` that has one, by the span of its
    /// statement.
    runs: HashMap<Span, Run>,
}

impl<'p> Analysis<'p> {
    /// Gives a verdict to every `each` of a body that is not inside
    /// another `each`.
    fn body(&mut self, locals: &'p [Local], outer: &[&'p Stmt]) {
        for stmt in outer {
            let StmtKind::Each {
                query,
                params,
                body,
                ..
            } = &stmt.kind
            else {
                unreachable!("an `each` statement");
            };
            let run = self.verdict(locals, query, params, body);
            self.runs.insert(stmt.span, run);
        }
    }

    fn verdict(
        &mut self,
        locals: &'p [Local],
        query: &crate::tir::Query,
        params: &[LocalId],
        body: &'p Block,
    ) -> Run {
        let program = self.program;
        let scanned = scan(&self.unfit, locals, body);
        let mut inside = vec![false; locals.len()];
        for local in params.iter().chain(&scanned.declared) {
            inside[local.0] = true;
        }
        let mut assigned = vec![false; program.components.len()];
        for term in &query.terms {
            if let Term::Component {
                component,
                write: true,
                ..
            } = term
            {
                assigned[component.0] = true;
            }
        }
        let mut judge = Judge {
            program,
            locals,
            inside,
            assigned,
            gpu: &self.gpu,
            sums: Vec::new(),
        };
        for (event, _) in &scanned.events {
            let target = match *event {
                Event::Effect(EffectKind::FieldSum(resource, field))
                    if *judge.field_type(resource, field) == Type::Int =>
                {
                    Target::Field(resource, field)
                }
                Event::LocalSum(local)
                    if !judge.inside[local.0] && locals[local.0].ty == Type::Int =>
                {
                    Target::Local(local)
                }
                _ => continue,
            };
            if !judge.sums.contains(&target) {
                judge.sums.push(target);
            }
        }

        let mut queries = queries_of(&scanned.events);
        for (event, span) in &scanned.events {
            if let Event::Call(func) = event {
                let summary = &self.summaries[func.0];
                for effect in &summary.effects {
                    if let Some(kind) = judge.breaks(Event::Effect(effect.kind), true) {
                        return Run::OneCore(Reason {
                            kind,
                            span: effect.span,
                            via: Some(Via {
                                func: effect.func,
                                call: *span,
                            }),
                        });
                    }
                }
                queries.extend(&summary.queries);
            } else if let Some(kind) = judge.breaks(*event, false) {
                return Run::OneCore(Reason {
                    kind,
                    span: *span,
                    via: None,
                });
            }
        }
        queries.sort();
        queries.dedup();
        let mut weight = self.weigher.block(body, locals);
        if !weight.0.contains_key(&Vec::new()) {
            weight.add_term(Vec::new(), 1);
        }
        Run::Cores(Cores {
            sums: judge.sums,
            weight: weight.weight(),
            queries,
        })
    }
}

/// For each component, the first GPU system that names it as a term of a
/// block or of an `each` inside one.
fn gpu_named(program: &Program, unfit: &[Option<EffectKind>]) -> Vec<Option<usize>> {
    fn terms(stmt: &Stmt, system: usize, named: &mut [Option<usize>]) {
        if let StmtKind::Each { query, .. } = &stmt.kind {
            for term in &query.terms {
                if let Term::Component { component, .. } = term {
                    named[component.0].get_or_insert(system);
                }
            }
        }
    }
    let mut named = vec![None; program.components.len()];
    for (index, system) in program.systems.iter().enumerate() {
        if !system.gpu {
            continue;
        }
        for stmt in &system.body.block.stmts {
            terms(stmt, index, &mut named);
            if let StmtKind::Each { body, .. } = &stmt.kind {
                for inner in scan(unfit, &system.body.locals, body).outer {
                    terms(inner, index, &mut named);
                }
            }
        }
    }
    named
}

/// Numbers every query of a program, gives every function its summary and
/// every `each` where it runs. The program has no errors.
pub(super) fn analyze(program: &mut Program) {
    let mut next = 0;
    let mut number = |node: walk::Node<'_>| {
        let query = match node {
            walk::Node::Stmt(Stmt {
                kind: StmtKind::Each { query, .. },
                ..
            }) => query,
            walk::Node::Expr(Expr {
                kind: ExprKind::WorldCount(Some(query)),
                ..
            }) => query,
            _ => return,
        };
        query.id = QueryId(next);
        next += 1;
    };
    for func in &mut program.funcs {
        walk::block(&mut func.body.block, &mut number);
    }
    for system in &mut program.systems {
        walk::block(&mut system.body.block, &mut number);
    }
    walk::block(&mut program.main.block, &mut number);

    let (summaries, runs) = {
        let program = &*program;
        let unfit = unfit_records(program);
        let scans: Vec<Scanned<'_>> = program
            .funcs
            .iter()
            .map(|func| scan(&unfit, &func.body.locals, &func.body.block))
            .collect();
        let direct: Vec<Vec<(Event, Span)>> =
            scans.iter().map(|scan| scan.events.clone()).collect();
        let calls: Vec<Vec<FuncId>> = direct.iter().map(|events| callees(events)).collect();
        let recursive = recursive(&calls);
        let mut weigher = Weigher {
            program,
            recursive: recursive.clone(),
            funcs: vec![None; program.funcs.len()],
        };
        let effects = effects(&direct);
        let queries = reachable_queries(&direct, &calls);
        let summaries: Vec<Summary> = effects
            .into_iter()
            .zip(queries)
            .enumerate()
            .map(|(index, (effects, queries))| Summary {
                effects,
                recursive: recursive[index],
                weight: weigher.func(FuncId(index)).weight(),
                queries,
            })
            .collect();

        let gpu = gpu_named(program, &unfit);
        let mut analysis = Analysis {
            program,
            unfit,
            summaries,
            gpu,
            weigher,
            runs: HashMap::new(),
        };
        for (func, scanned) in program.funcs.iter().zip(&scans) {
            analysis.body(&func.body.locals, &scanned.outer);
        }
        for system in &program.systems {
            let body = &system.body;
            let outer = scan(&analysis.unfit, &body.locals, &body.block).outer;
            if system.gpu {
                for stmt in outer {
                    analysis.runs.insert(stmt.span, Run::Gpu);
                }
            } else {
                analysis.body(&body.locals, &outer);
            }
        }
        let main = &program.main;
        let outer = scan(&analysis.unfit, &main.locals, &main.block).outer;
        analysis.body(&main.locals, &outer);
        (analysis.summaries, analysis.runs)
    };

    program.summaries = summaries;
    let mut runs = runs;
    let mut place = |node: walk::Node<'_>| {
        if let walk::Node::Stmt(Stmt {
            kind: StmtKind::Each { run, .. },
            span,
        }) = node
            && let Some(found) = runs.remove(span)
        {
            *run = found;
        }
    };
    for func in &mut program.funcs {
        walk::block(&mut func.body.block, &mut place);
    }
    for system in &mut program.systems {
        walk::block(&mut system.body.block, &mut place);
    }
    walk::block(&mut program.main.block, &mut place);
}
