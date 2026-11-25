//! Expressions.
//!
//! An expression is lowered to C statements that compute whatever has
//! effects, in source order, and a [`Val`]: a C expression without effects
//! for the result. A value that reads something later code could change is
//! copied into a temporary before that code is generated.

use crate::source::Span;
use crate::tir::{
    BinOp, Block, Builtin, ComponentId, ComponentInit, Expr, ExprKind, StmtKind, Type, UnOp,
};

use super::Cx;
use super::analysis;
use super::body::{
    ADD, AND, ATOM, Dep, EQ, FnCx, MUL, Mode, OR, Own, REL, Scope, ScopeKind, UNARY, Val,
};
use super::names;
use super::place::{borrow_dep, load};

/// What the consumer of a group of operands does with them.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Want {
    /// Reads them at once, running no user code.
    Peek,
    /// Passes them to a user function, which borrows them.
    Call,
    /// Keeps them.
    Own,
}

/// A piece of text to write: literal bytes, or the `to_s` of a value.
pub(super) enum Part<'a> {
    Lit(String),
    Val(&'a Expr),
}

/// Where text goes: standard output, or the named `GrBuilder`.
pub(super) enum Sink {
    Out,
    Builder(String),
}

/// The value of a branch generated into a buffer of its own.
struct Captured {
    text: String,
    scope: Scope,
    val: Val,
}

impl Captured {
    /// Whether the branch is nothing but an expression without effects.
    fn inline(&self) -> bool {
        self.text.is_empty() && self.scope.owned.is_empty() && !self.val.pending
    }
}

/// Splits a string expression into the pieces that are joined to make
/// it, appending them to `out`.
pub(super) fn text_parts<'a>(expr: &'a Expr, out: &mut Vec<Part<'a>>) {
    match &expr.kind {
        ExprKind::Str(text) => push_lit(out, text),
        ExprKind::Concat(parts) => {
            for part in parts {
                text_parts(part, out);
            }
        }
        ExprKind::Binary {
            op: BinOp::Add,
            lhs,
            rhs,
        } if expr.ty == Type::String => {
            text_parts(lhs, out);
            text_parts(rhs, out);
        }
        ExprKind::Builtin {
            op: Builtin::ToS,
            args,
        } if args.len() == 1 => {
            if args[0].ty == Type::String {
                text_parts(&args[0], out);
            } else {
                out.push(Part::Val(&args[0]));
            }
        }
        _ => out.push(Part::Val(expr)),
    }
}

/// Whether a string expression is itself made by joining pieces or by
/// formatting a value, so that a `+` with it is better built in one go.
fn joins(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::Concat(_) => true,
        ExprKind::Binary { op: BinOp::Add, .. } => expr.ty == Type::String,
        ExprKind::Builtin {
            op: Builtin::ToS,
            args,
        } => args.len() == 1 && args[0].ty != Type::String,
        _ => false,
    }
}

/// Appends literal text, joining it to a literal that is already last.
pub(super) fn push_lit(out: &mut Vec<Part<'_>>, text: &str) {
    if text.is_empty() {
        return;
    }
    match out.last_mut() {
        Some(Part::Lit(last)) => last.push_str(text),
        _ => out.push(Part::Lit(text.to_string())),
    }
}

impl<'a> FnCx<'a, '_> {
    /// Evaluates an expression for a consumer that handles its value as
    /// `mode` says.
    pub(super) fn expr(&mut self, expr: &'a Expr, mode: Mode) -> Val {
        let value = self.rvalue(expr);
        let value = self.materialize(value, &expr.ty);
        self.ensure(value, &expr.ty, mode)
    }

    /// Emits a pending value into a temporary.
    pub(super) fn materialize(&mut self, value: Val, ty: &Type) -> Val {
        if !value.pending {
            return value;
        }
        if *ty == Type::Void {
            self.line(&format!("{};", value.c));
            return Val::void();
        }
        let name = self.declare(ty, &value.c);
        if value.own == Own::Owned {
            self.register(&name, ty, true);
        }
        Val {
            own: value.own,
            dep: borrow_dep(value.own).min(value.dep),
            ..Val::atom(name)
        }
    }

    /// Makes a borrowed value owned when `mode` needs more than it gives.
    pub(super) fn ensure(&mut self, value: Val, ty: &Type, mode: Mode) -> Val {
        if !self.cx.managed(ty) {
            return value;
        }
        match (mode, value.own) {
            (_, Own::Owned | Own::Free)
            | (Mode::Peek, _)
            | (Mode::Stable, Own::Borrowed { stable: true }) => value,
            _ => self.own_copy(value, ty),
        }
    }

    /// Retains a borrowed value, giving a temporary that owns it.
    fn own_copy(&mut self, value: Val, ty: &Type) -> Val {
        let name = if value.slot {
            value.c
        } else {
            self.declare(ty, &value.c)
        };
        let retain = self.cx.retain_stmt(&name, ty);
        self.line(&retain);
        self.register(&name, ty, true);
        Val {
            own: Own::Owned,
            ..Val::atom(name)
        }
    }

    /// The C expression for a value that is being moved somewhere that
    /// will own it.
    pub(super) fn take(&mut self, value: Val, ty: &Type) -> String {
        debug_assert!(!value.pending);
        if !self.cx.managed(ty) {
            return value.c;
        }
        match value.own {
            Own::Owned => {
                self.disown(&value.c);
                value.c
            }
            Own::Free => value.c,
            Own::Borrowed { .. } => {
                let owned = self.own_copy(value, ty);
                self.disown(&owned.c);
                owned.c
            }
        }
    }

    /// Writes a value into storage that holds nothing yet, which then
    /// owns it. `decl` is empty or the type that declares `target` here.
    pub(super) fn store_new(&mut self, target: &str, decl: &str, value: Val, ty: &Type) {
        self.line(&format!("{decl}{target} = {};", value.c));
        self.adopt(target, &value, ty);
    }

    /// Settles ownership after a value's bits were written into `target`,
    /// which held nothing before and owns the value from now on.
    pub(super) fn adopt(&mut self, target: &str, value: &Val, ty: &Type) {
        if !self.cx.managed(ty) {
            return;
        }
        match value.own {
            Own::Owned if !value.pending => self.disown(&value.c),
            Own::Borrowed { .. } => {
                let retain = self.cx.retain_stmt(target, ty);
                self.line(&retain);
            }
            _ => {}
        }
    }

    /// Copies a value that later code could change into a temporary.
    pub(super) fn stabilize(&mut self, value: Val, ty: &Type) -> Val {
        if value.dep == Dep::Fixed && !value.pending {
            return value;
        }
        self.copy(value, ty)
    }

    /// Makes a value cheap to use more than once.
    pub(super) fn once(&mut self, value: Val, ty: &Type) -> Val {
        if value.simple {
            return value;
        }
        self.copy(value, ty)
    }

    /// Makes a record value something `&` can be applied to.
    fn addressable(&mut self, value: Val, ty: &Type) -> Val {
        if value.simple && value.lit.is_none() {
            return value;
        }
        self.copy(value, ty)
    }

    fn copy(&mut self, value: Val, ty: &Type) -> Val {
        let name = self.declare(ty, &value.c);
        let borrowed = matches!(value.own, Own::Borrowed { .. });
        if value.pending && value.own == Own::Owned {
            self.register(&name, ty, true);
        }
        Val {
            own: value.own,
            dep: if borrowed { value.dep } else { Dep::Fixed },
            ..Val::atom(name)
        }
    }

    /// Evaluates operands left to right.
    pub(super) fn operands(&mut self, exprs: &[&'a Expr], want: Want) -> Vec<Val> {
        self.operands_then(exprs, want, true, true)
    }

    /// Evaluates operands left to right for a consumer that runs other
    /// code first. `tail_inert` says that code neither runs user code nor
    /// changes any place; `tail_safe` that it leaves this function's
    /// locals alone. An operand that later code could change is copied.
    pub(super) fn operands_then(
        &mut self,
        exprs: &[&'a Expr],
        want: Want,
        tail_inert: bool,
        tail_safe: bool,
    ) -> Vec<Val> {
        let count = exprs.len();
        let mut inert_after = vec![tail_inert; count];
        let mut safe_after = vec![tail_safe; count];
        for index in (0..count.saturating_sub(1)).rev() {
            inert_after[index] = inert_after[index + 1] && analysis::inert(exprs[index + 1]);
            safe_after[index] = safe_after[index + 1] && analysis::local_safe(exprs[index + 1]);
        }
        let mut values = Vec::with_capacity(count);
        for (index, &expr) in exprs.iter().enumerate() {
            let mode = match want {
                Want::Own => Mode::Own,
                Want::Peek if inert_after[index] => Mode::Peek,
                _ if safe_after[index] => Mode::Stable,
                _ => Mode::Own,
            };
            let mut value = self.expr(expr, mode);
            let outlives = match value.dep {
                Dep::Fixed => true,
                Dep::Local => safe_after[index],
                Dep::World => inert_after[index],
            };
            if !outlives {
                value = self.stabilize(value, &expr.ty);
            }
            values.push(value);
        }
        values
    }

    /// Evaluates an expression. The result may be pending.
    pub(super) fn rvalue(&mut self, expr: &'a Expr) -> Val {
        match &expr.kind {
            ExprKind::Int(value) => Val::int(*value),
            ExprKind::Float(value) => Val::float(*value),
            ExprKind::Bool(value) => Val::atom(if *value { "true" } else { "false" }),
            ExprKind::Str(text) => Val::atom(self.cx.str_lit(text)),
            ExprKind::EnumVariant(index) => Val::atom(index.to_string()),
            ExprKind::EntityNone => Val::atom("(GrEntity){0, 0}"),
            ExprKind::Read(place) => self.read_place(place),
            ExprKind::Field { base, field } => {
                let Type::Record(record) = &base.ty else {
                    unreachable!("a field of something that is not a record");
                };
                let prog = self.cx.prog;
                let field = &prog.records[record.0].fields[*field];
                let value = self.expr(base, Mode::Peek);
                let member = format!("{}.{}", value.p(ATOM), names::field(&field.name));
                let (c, prec) = load(&member, ATOM, &field.ty);
                let own = match value.own {
                    _ if !self.cx.managed(&expr.ty) => Own::Free,
                    Own::Borrowed { stable } => Own::Borrowed { stable },
                    Own::Owned | Own::Free => Own::Borrowed { stable: true },
                };
                Val {
                    simple: value.simple && c == member,
                    slot: value.slot && c == member,
                    c,
                    prec,
                    lit: None,
                    own,
                    dep: value.dep,
                    pending: false,
                }
            }
            ExprKind::Index { base, index } => {
                let Type::List(elem) = &base.ty else {
                    unreachable!("an index into something that is not a list");
                };
                let mut values = self.operands(&[&**base, &**index], Want::Peek);
                let index = values.pop().expect("two operands");
                let list = values.pop().expect("two operands");
                let list = self.once(list, &base.ty);
                let loc = self.cx.loc(expr.span);
                let position = self.tmp();
                self.line(&format!(
                    "int64_t {position} = gr_list_index({}, {}, {loc});",
                    list.c, index.c
                ));
                self.load_elem(&list, &position, elem)
            }
            ExprKind::ListLit(items) => self.list_lit(expr, items),
            ExprKind::Construct(fields) => self.construct(expr, fields),
            ExprKind::Concat(_) => self.join(expr),
            ExprKind::Binary {
                op: BinOp::Add,
                lhs,
                rhs,
            } if expr.ty == Type::String && (joins(lhs) || joins(rhs)) => self.join(expr),
            ExprKind::Unary { op, expr: inner } => {
                let value = self.expr(inner, Mode::Peek);
                match op {
                    UnOp::Neg => {
                        if let Some(literal) = value.lit {
                            return Val::int(literal.wrapping_neg());
                        }
                        let operand = if value.prec > UNARY || value.c.starts_with('-') {
                            format!("({})", value.c)
                        } else {
                            value.c
                        };
                        Val::lazy(format!("-{operand}"), UNARY, value.dep)
                    }
                    UnOp::Not => Val::lazy(format!("!{}", value.p(UNARY)), UNARY, value.dep),
                }
            }
            ExprKind::Binary { op, lhs, rhs } => {
                let mut values = self.operands(&[&**lhs, &**rhs], Want::Peek);
                let right = values.pop().expect("two operands");
                let left = values.pop().expect("two operands");
                self.binary(*op, &lhs.ty, left, right, expr.span)
            }
            ExprKind::And { lhs, rhs } => self.short_circuit(lhs, rhs, true),
            ExprKind::Or { lhs, rhs } => self.short_circuit(lhs, rhs, false),
            ExprKind::IntToFloat(inner) => {
                let value = self.expr(inner, Mode::Peek);
                match value.lit {
                    Some(literal) => Val::float(literal as f64),
                    None => Val::lazy(format!("(double){}", value.p(UNARY)), UNARY, value.dep),
                }
            }
            ExprKind::Call { func, args } => {
                let exprs: Vec<&'a Expr> = args.iter().collect();
                let values = self.operands(&exprs, Want::Call);
                self.cx.func_used[func.0] = true;
                let own = if self.cx.managed(&expr.ty) {
                    Own::Owned
                } else {
                    Own::Free
                };
                let name = names::func(self.cx.prog, func.0);
                Val::pending(format!("{name}({})", arg_list(&values)), own)
            }
            ExprKind::Builtin { op, args } => self.builtin(expr, *op, args),
            ExprKind::If { cond, then, else_ } => self.if_expr(expr, cond, then, else_),
            ExprKind::Block(block) => {
                if block.stmts.is_empty() {
                    return match &block.value {
                        Some(value) => self.rvalue(value),
                        None => Val::void(),
                    };
                }
                if expr.ty == Type::Void {
                    self.line("{");
                    self.branch(block, None);
                    self.line("}");
                    return Val::void();
                }
                let name = self.result_slot(&expr.ty);
                self.line("{");
                self.branch(block, Some((&name, &expr.ty)));
                self.line("}");
                self.result(name, &expr.ty)
            }
            ExprKind::Spawn(inits) => {
                let exprs: Vec<&'a Expr> = inits
                    .iter()
                    .filter_map(|init| init.value.as_ref())
                    .collect();
                let values = self.operands(&exprs, Want::Own);
                let (bundle, array) = self.bundle_values(inits, values);
                Val::pending(format!("gr_spawn(&{bundle}, {array})"), Own::Free)
            }
            ExprKind::Add { entity, components } => {
                let mut exprs: Vec<&'a Expr> = vec![&**entity];
                exprs.extend(components.iter().filter_map(|init| init.value.as_ref()));
                let mut values = self.operands(&exprs, Want::Own);
                let entity = values.remove(0);
                let (bundle, array) = self.bundle_values(components, values);
                Val::pending(
                    format!("gr_add({}, &{bundle}, {array})", entity.c),
                    Own::Free,
                )
            }
            ExprKind::Remove { entity, components } => {
                let entity = self.expr(entity, Mode::Peek);
                let (ids, count) = self.cx.remove_list(components);
                Val::pending(
                    format!("gr_remove({}, {ids}, {count})", entity.c),
                    Own::Free,
                )
            }
            ExprKind::Despawn(entity) => {
                let entity = self.expr(entity, Mode::Peek);
                Val::pending(format!("gr_despawn({})", entity.c), Own::Free)
            }
            ExprKind::Has { entity, component } => {
                let entity = self.expr(entity, Mode::Peek);
                Val::lazy(
                    format!("gr_has({}, {})", entity.c, component.0),
                    ATOM,
                    Dep::World,
                )
            }
            ExprKind::Alive(entity) => {
                let entity = self.expr(entity, Mode::Peek);
                Val::lazy(format!("gr_alive({})", entity.c), ATOM, Dep::World)
            }
            ExprKind::ListPush { list, value } => {
                let Type::List(elem) = &list.ty else {
                    unreachable!("push on something that is not a list");
                };
                let prepared = self.prepare(
                    list,
                    true,
                    analysis::inert(value),
                    analysis::local_safe(value),
                );
                let pushed = self.expr(value, Mode::Own);
                let pushed = self.stabilize(pushed, &value.ty);
                let c = if self.cx.managed(&value.ty) {
                    self.take(pushed, &value.ty)
                } else {
                    self.store_checked(&pushed, elem, value.span)
                };
                let lv = self.addr(&prepared, true);
                let elem_name = self.cx.elem(elem);
                let cty = self.cx.cty(elem);
                self.line(&format!(
                    "*({cty} *)gr_list_push({}, &{elem_name}) = {c};",
                    lv.addr()
                ));
                Val::void()
            }
            ExprKind::ListPop { list } => {
                let Type::List(elem) = &list.ty else {
                    unreachable!("pop on something that is not a list");
                };
                let prepared = self.prepare(list, true, true, true);
                let lv = self.addr(&prepared, true);
                let out = self.removed_slot(elem);
                let elem_name = self.cx.elem(elem);
                let loc = self.cx.loc(expr.span);
                self.line(&format!(
                    "gr_list_pop({}, &{elem_name}, &{out}, {loc});",
                    lv.addr()
                ));
                self.removed_value(out, elem, &expr.ty)
            }
            ExprKind::ListDeleteAt { list, index } => {
                let Type::List(elem) = &list.ty else {
                    unreachable!("delete_at on something that is not a list");
                };
                let prepared = self.prepare(
                    list,
                    true,
                    analysis::inert(index),
                    analysis::local_safe(index),
                );
                let index = self.expr(index, Mode::Peek);
                let lv = self.addr(&prepared, true);
                let out = self.removed_slot(elem);
                let elem_name = self.cx.elem(elem);
                let loc = self.cx.loc(expr.span);
                self.line(&format!(
                    "gr_list_delete_at({}, &{elem_name}, {}, &{out}, {loc});",
                    lv.addr(),
                    index.c
                ));
                self.removed_value(out, elem, &expr.ty)
            }
            ExprKind::ListClear { list } => {
                let Type::List(elem) = &list.ty else {
                    unreachable!("clear on something that is not a list");
                };
                let prepared = self.prepare(list, true, true, true);
                let lv = self.addr(&prepared, true);
                let elem_name = self.cx.elem(elem);
                self.line(&format!("gr_list_clear({}, &{elem_name});", lv.addr()));
                Val::void()
            }
            ExprKind::WorldTick => {
                let loc = self.cx.loc(expr.span);
                Val::pending(format!("gr_tick({loc})"), Own::Free)
            }
            ExprKind::WorldRun => {
                let loc = self.cx.loc(expr.span);
                Val::pending(format!("gr_run({loc})"), Own::Free)
            }
            ExprKind::WorldStop => Val::pending("gr_stop()".to_string(), Own::Free),
            ExprKind::WorldCount(Some(query)) => {
                let query = self.cx.query(query);
                Val::lazy(format!("gr_query_count(&{query})"), ATOM, Dep::World)
            }
            ExprKind::WorldCount(None) => {
                Val::lazy("gr_entity_count()".to_string(), ATOM, Dep::World)
            }
        }
    }

    /// A string made by joining pieces: all are evaluated, then appended
    /// to one builder.
    fn join(&mut self, expr: &'a Expr) -> Val {
        let mut parts = Vec::new();
        text_parts(expr, &mut parts);
        match parts.as_slice() {
            [] => Val::atom("NULL"),
            [Part::Lit(text)] => Val::atom(self.cx.str_lit(text)),
            [Part::Val(only)] if only.ty == Type::String => self.rvalue(only),
            [Part::Val(only)] => self.text_of(only),
            _ => {
                let values = self.eval_parts(&parts);
                let builder = self.fresh("sb");
                self.line(&format!("GrBuilder {builder} = {{0}};"));
                self.write_parts(&parts, values, &Sink::Builder(builder.clone()));
                Val::pending(format!("gr_sb_finish(&{builder})"), Own::Owned)
            }
        }
    }

    /// Declares the zeroed variable that a block or `if` leaves its value
    /// in.
    fn result_slot(&mut self, ty: &Type) -> String {
        let name = self.tmp();
        let cty = self.cx.cty(ty);
        let zero = self.cx.zero(ty);
        self.line(&format!("{cty} {name} = {zero};"));
        name
    }

    /// The value in a result variable once its branches have run.
    fn result(&mut self, name: String, ty: &Type) -> Val {
        if self.cx.managed(ty) {
            self.register(&name, ty, true);
            Val {
                own: Own::Owned,
                ..Val::atom(name)
            }
        } else {
            Val::atom(name)
        }
    }

    /// Declares the zeroed variable that `pop` or `delete_at` moves an
    /// element into.
    fn removed_slot(&mut self, elem: &Type) -> String {
        let name = self.tmp();
        let cty = self.cx.cty(elem);
        let zero = self.cx.zero(elem);
        self.line(&format!("{cty} {name} = {zero};"));
        name
    }

    fn removed_value(&mut self, out: String, elem: &Type, ty: &Type) -> Val {
        if self.cx.managed(ty) {
            self.register(&out, ty, true);
            return Val {
                own: Own::Owned,
                ..Val::atom(out)
            };
        }
        let (c, prec) = load(&out, ATOM, elem);
        Val {
            simple: c == out,
            prec,
            ..Val::atom(c)
        }
    }

    /// The element at `position` of a list value, as a read that must be
    /// emitted at once.
    fn load_elem(&mut self, list: &Val, position: &str, elem: &Type) -> Val {
        let cty = self.cx.cty(elem);
        let slot = format!("((const {cty} *)gr_list_data({}))[{position}]", list.c);
        let (c, prec) = load(&slot, ATOM, elem);
        let (own, dep) = match list.own {
            _ if !self.cx.managed(&elem.value_type()) => (Own::Free, Dep::Fixed),
            Own::Borrowed { stable } => (Own::Borrowed { stable }, list.dep),
            Own::Owned | Own::Free => (Own::Borrowed { stable: true }, Dep::Fixed),
        };
        Val {
            prec,
            dep,
            ..Val::pending(c, own)
        }
    }

    fn list_lit(&mut self, expr: &'a Expr, items: &'a [Expr]) -> Val {
        let Type::List(elem) = &expr.ty else {
            unreachable!("a list literal that is not a list");
        };
        if items.is_empty() {
            return Val::atom("NULL");
        }
        let exprs: Vec<&'a Expr> = items.iter().collect();
        let values = self.operands(&exprs, Want::Own);
        let mut stored = Vec::with_capacity(items.len());
        for (value, item) in values.into_iter().zip(items) {
            stored.push(if self.cx.managed(&item.ty) {
                self.take(value, &item.ty)
            } else {
                self.store_checked(&value, elem, item.span)
            });
        }
        let elem_name = self.cx.elem(elem);
        let cty = self.cx.cty(elem);
        let list = self.tmp();
        self.line(&format!(
            "GrList {list} = gr_list_alloc(&{elem_name}, {});",
            items.len()
        ));
        let data = self.fresh("p");
        self.line(&format!("{cty} *{data} = gr_list_data({list});"));
        for (index, c) in stored.iter().enumerate() {
            self.line(&format!("{data}[{index}] = {c};"));
        }
        self.register(&list, &expr.ty, true);
        Val {
            own: Own::Owned,
            ..Val::atom(list)
        }
    }

    fn construct(&mut self, expr: &'a Expr, fields: &'a [Expr]) -> Val {
        let Type::Record(record) = &expr.ty else {
            unreachable!("a constructor of something that is not a record");
        };
        let prog = self.cx.prog;
        let record = &prog.records[record.0];
        let cty = self.cx.cty(&expr.ty);
        if fields.is_empty() {
            return Val::lazy(format!("({cty}){{0}}"), ATOM, Dep::Fixed);
        }
        let exprs: Vec<&'a Expr> = fields.iter().collect();
        let values = self.operands(&exprs, Want::Own);
        let mut dep = Dep::Fixed;
        let mut inits = Vec::with_capacity(fields.len());
        for ((value, init), field) in values.into_iter().zip(fields).zip(&record.fields) {
            dep = dep.max(value.dep);
            let c = if self.cx.managed(&init.ty) {
                self.take(value, &init.ty)
            } else {
                self.store_checked(&value, &field.ty, init.span)
            };
            inits.push(format!(".{} = {c}", names::field(&field.name)));
        }
        let inits = inits.join(", ");
        if self.cx.managed(&expr.ty) {
            let name = self.tmp();
            self.line(&format!("{cty} {name} = {{{inits}}};"));
            self.register(&name, &expr.ty, true);
            Val {
                own: Own::Owned,
                ..Val::atom(name)
            }
        } else {
            Val::lazy(format!("({cty}){{{inits}}}"), ATOM, dep)
        }
    }

    /// A built-in binary operator on two evaluated operands. `ty` is the
    /// type of the left one.
    pub(super) fn binary(
        &mut self,
        op: BinOp,
        ty: &Type,
        left: Val,
        right: Val,
        span: Span,
    ) -> Val {
        let dep = left.dep.max(right.dep);
        match op {
            BinOp::Eq | BinOp::Ne => {
                if !Cx::eq_is_call(ty) {
                    let symbol = if op == BinOp::Eq { "==" } else { "!=" };
                    let c = format!("{} {symbol} {}", left.p(ADD), right.p(ADD));
                    return Val::lazy(c, EQ, dep);
                }
                let (left, right) = if matches!(ty, Type::Record(_)) {
                    (self.addressable(left, ty), self.addressable(right, ty))
                } else {
                    (left, right)
                };
                let equal = self.cx.eq_c(&left.c, &right.c, ty);
                if op == BinOp::Eq {
                    Val::lazy(equal, ATOM, dep)
                } else {
                    Val::lazy(format!("!{equal}"), UNARY, dep)
                }
            }
            BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                let symbol = match op {
                    BinOp::Lt => "<",
                    BinOp::Le => "<=",
                    BinOp::Gt => ">",
                    _ => ">=",
                };
                let c = if *ty == Type::String {
                    format!("gr_str_cmp({}, {}) {symbol} 0", left.c, right.c)
                } else {
                    format!("{} {symbol} {}", left.p(ADD), right.p(ADD))
                };
                Val::lazy(c, REL, dep)
            }
            _ => match ty {
                Type::Int => {
                    if let (Some(a), Some(b)) = (left.lit, right.lit)
                        && let Some(folded) = analysis::fold_int(op, a, b)
                    {
                        return Val::int(folded);
                    }
                    let checked = |name: &str, cx: &mut Self| {
                        let loc = cx.cx.loc(span);
                        Val::pending(format!("{name}({}, {}, {loc})", left.c, right.c), Own::Free)
                    };
                    match op {
                        BinOp::Add => {
                            Val::lazy(format!("{} + {}", left.p(ADD), right.p(MUL)), ADD, dep)
                        }
                        BinOp::Sub => {
                            Val::lazy(format!("{} - {}", left.p(ADD), right.p(MUL)), ADD, dep)
                        }
                        BinOp::Mul => {
                            Val::lazy(format!("{} * {}", left.p(MUL), right.p(UNARY)), MUL, dep)
                        }
                        BinOp::Div => checked("gr_int_div", self),
                        BinOp::Mod => checked("gr_int_mod", self),
                        _ => checked("gr_int_pow", self),
                    }
                }
                Type::Float => match op {
                    BinOp::Add => {
                        Val::lazy(format!("{} + {}", left.p(ADD), right.p(MUL)), ADD, dep)
                    }
                    BinOp::Sub => {
                        Val::lazy(format!("{} - {}", left.p(ADD), right.p(MUL)), ADD, dep)
                    }
                    BinOp::Mul => {
                        Val::lazy(format!("{} * {}", left.p(MUL), right.p(UNARY)), MUL, dep)
                    }
                    BinOp::Div => {
                        Val::lazy(format!("{} / {}", left.p(MUL), right.p(UNARY)), MUL, dep)
                    }
                    BinOp::Mod => {
                        Val::lazy(format!("gr_float_mod({}, {})", left.c, right.c), ATOM, dep)
                    }
                    _ => Val::lazy(format!("pow({}, {})", left.c, right.c), ATOM, dep),
                },
                Type::String => {
                    let name = if op == BinOp::Add {
                        "gr_str_concat"
                    } else {
                        "gr_str_repeat"
                    };
                    Val::pending(format!("{name}({}, {})", left.c, right.c), Own::Owned)
                }
                Type::List(elem) => {
                    let name = if op == BinOp::Add {
                        "gr_list_concat"
                    } else {
                        "gr_list_repeat"
                    };
                    let elem = self.cx.elem(elem);
                    Val::pending(
                        format!("{name}({}, {}, &{elem})", left.c, right.c),
                        Own::Owned,
                    )
                }
                _ => unreachable!("arithmetic on a type that has none"),
            },
        }
    }

    /// `&&` and `||`. The right operand is generated into a buffer: when
    /// it turns out to be an expression without effects the C operator is
    /// used, and otherwise an `if` runs its statements only when needed.
    fn short_circuit(&mut self, lhs: &'a Expr, rhs: &'a Expr, and: bool) -> Val {
        let left = self.rvalue(lhs);
        let fresh = left.pending;
        let left = self.materialize(left, &Type::Bool);
        self.push_scope(ScopeKind::Plain);
        self.indent += 1;
        let (text, right) = self.capture(|cx| cx.rvalue(rhs));
        self.indent -= 1;
        let scope_empty = self
            .scopes
            .last()
            .is_some_and(|scope| scope.owned.is_empty());
        if text.is_empty() && scope_empty && !right.pending {
            self.scopes.pop();
            let (symbol, prec) = if and { ("&&", AND) } else { ("||", OR) };
            return Val::lazy(
                format!("{} {symbol} {}", left.p(EQ), right.p(EQ)),
                prec,
                left.dep.max(right.dep),
            );
        }
        let name = if fresh {
            left.c
        } else {
            self.declare(&Type::Bool, &left.c)
        };
        let not = if and { "" } else { "!" };
        self.line(&format!("if ({not}{name}) {{"));
        self.out.push_str(&text);
        self.indent += 1;
        self.line(&format!("{name} = {};", right.c));
        self.pop_scope(false);
        self.indent -= 1;
        self.line("}");
        Val::atom(name)
    }

    /// Generates the value of a branch into a buffer, in a scope of its
    /// own that is left closed.
    fn capture_value(&mut self, value: &'a Expr) -> Captured {
        self.push_scope(ScopeKind::Plain);
        self.indent += 1;
        let (text, val) = self.capture(|cx| cx.rvalue(value));
        self.indent -= 1;
        let scope = self.scopes.pop().expect("the scope pushed above");
        Captured { text, scope, val }
    }

    /// Writes a captured branch out, ending with the store of its value
    /// into `target`.
    fn finish_capture(&mut self, captured: Captured, target: &str, ty: &Type) {
        self.out.push_str(&captured.text);
        self.scopes.push(captured.scope);
        self.indent += 1;
        self.store_new(target, "", captured.val, ty);
        self.pop_scope(false);
        self.indent -= 1;
    }

    fn if_expr(
        &mut self,
        expr: &'a Expr,
        cond: &'a Expr,
        then: &'a Block,
        else_: &'a Block,
    ) -> Val {
        let cond = self.expr(cond, Mode::Peek);
        if expr.ty == Type::Void {
            self.if_chain(cond, then, else_, None);
            return Val::void();
        }
        if !self.cx.managed(&expr.ty)
            && then.stmts.is_empty()
            && else_.stmts.is_empty()
            && let (Some(first), Some(second)) = (&then.value, &else_.value)
        {
            let first = self.capture_value(first);
            let second = self.capture_value(second);
            if first.inline() && second.inline() {
                let dep = cond.dep.max(first.val.dep).max(second.val.dep);
                let chosen = if expr.ty == Type::Int {
                    first.val.typed_int()
                } else {
                    first.val.c
                };
                return Val::lazy(
                    format!("({} ? {chosen} : {})", cond.c, second.val.c),
                    ATOM,
                    dep,
                );
            }
            let name = self.result_slot(&expr.ty);
            self.line(&format!("if ({}) {{", cond.c));
            self.finish_capture(first, &name, &expr.ty);
            self.line("} else {");
            self.finish_capture(second, &name, &expr.ty);
            self.line("}");
            return Val::atom(name);
        }
        let name = self.result_slot(&expr.ty);
        self.if_chain(cond, then, else_, Some((&name, &expr.ty)));
        self.result(name, &expr.ty)
    }

    /// Generates an `if` whose condition has been evaluated, following
    /// `elsif` chains. `result` is where each branch stores its value.
    fn if_chain(
        &mut self,
        cond: Val,
        then: &'a Block,
        else_: &'a Block,
        result: Option<(&str, &Type)>,
    ) {
        let mut cond = cond;
        let mut then = then;
        let mut else_ = else_;
        let mut keyword = "if";
        let mut open = 0;
        loop {
            self.line(&format!("{keyword} ({}) {{", cond.c));
            self.branch(then, result);
            if else_.stmts.is_empty() && else_.value.is_none() {
                self.line("}");
                break;
            }
            if else_.value.is_none()
                && let [only] = else_.stmts.as_slice()
                && let StmtKind::Expr(Expr {
                    kind:
                        ExprKind::If {
                            cond: next_cond,
                            then: next_then,
                            else_: next_else,
                        },
                    ty: Type::Void,
                    ..
                }) = &only.kind
            {
                self.push_scope(ScopeKind::Plain);
                self.indent += 1;
                let (text, value) = self.capture(|cx| cx.expr(next_cond, Mode::Peek));
                self.indent -= 1;
                let scope_empty = self
                    .scopes
                    .last()
                    .is_some_and(|scope| scope.owned.is_empty());
                if text.is_empty() && scope_empty {
                    self.scopes.pop();
                    keyword = "} else if";
                } else {
                    self.line("} else {");
                    self.indent += 1;
                    self.directive(only.span);
                    self.out.push_str(&text);
                    open += 1;
                    keyword = "if";
                }
                cond = value;
                then = next_then;
                else_ = next_else;
                continue;
            }
            self.line("} else {");
            self.branch(else_, result);
            self.line("}");
            break;
        }
        for _ in 0..open {
            self.pop_scope(false);
            self.indent -= 1;
            self.line("}");
        }
    }

    /// Generates a block in a C block and scope of its own. The braces
    /// are the caller's.
    pub(super) fn branch(&mut self, block: &'a Block, result: Option<(&str, &Type)>) {
        self.push_scope(ScopeKind::Plain);
        self.indent += 1;
        self.block_in_scope(block, result);
        self.pop_scope(analysis::block_leaves(block));
        self.indent -= 1;
    }

    /// Generates a block's statements in the innermost scope, then stores
    /// its value into `result` or discards it.
    pub(super) fn block_in_scope(&mut self, block: &'a Block, result: Option<(&str, &Type)>) {
        self.stmts(&block.stmts);
        let Some(value) = &block.value else {
            return;
        };
        self.directive(value.span);
        let evaluated = self.rvalue(value);
        match result {
            Some((target, ty)) => self.store_new(target, "", evaluated, ty),
            None => self.discard(evaluated, &value.ty),
        }
    }

    /// Ends the evaluation of an expression whose value is not used.
    pub(super) fn discard(&mut self, value: Val, ty: &Type) {
        if *ty == Type::Void {
            if value.pending {
                self.line(&format!("{};", value.c));
            }
        } else if value.pending {
            if value.own == Own::Owned {
                self.materialize(value, ty);
            } else {
                self.line(&format!("(void){};", value.c));
            }
        } else if value.own != Own::Owned && value.lit.is_none() && !value.c.is_empty() {
            self.line(&format!("(void){};", value.p(UNARY)));
        }
    }

    /// Moves the values of a `spawn` or `add` into variables, and returns
    /// the static bundle and the array of their addresses (or `NULL`).
    fn bundle_values(&mut self, inits: &'a [ComponentInit], values: Vec<Val>) -> (String, String) {
        let ids: Vec<ComponentId> = inits.iter().map(|init| init.component).collect();
        let bundle = self.cx.bundle(&ids);
        let mut values = values.into_iter();
        let mut pointers = Vec::with_capacity(inits.len());
        let mut any = false;
        for init in inits {
            let Some(expr) = &init.value else {
                pointers.push("NULL".to_string());
                continue;
            };
            let value = values.next().expect("one value per component with data");
            let owned = value.own == Own::Owned;
            let c = self.take(value, &expr.ty);
            let name = if owned { c } else { self.declare(&expr.ty, &c) };
            pointers.push(format!("&{name}"));
            any = true;
        }
        if !any {
            return (bundle, "NULL".to_string());
        }
        let array = self.fresh("v");
        self.line(&format!(
            "void *const {array}[] = {{{}}};",
            pointers.join(", ")
        ));
        (bundle, array)
    }

    /// Evaluates the values among the parts of a text.
    pub(super) fn eval_parts(&mut self, parts: &[Part<'a>]) -> Vec<Val> {
        let exprs: Vec<&'a Expr> = parts
            .iter()
            .filter_map(|part| match part {
                Part::Val(expr) => Some(*expr),
                Part::Lit(_) => None,
            })
            .collect();
        self.operands(&exprs, Want::Peek)
    }

    /// Writes the parts of a text, in order, to a sink.
    pub(super) fn write_parts(&mut self, parts: &[Part<'a>], values: Vec<Val>, sink: &Sink) {
        let mut values = values.into_iter();
        for part in parts {
            match part {
                Part::Lit(text) => {
                    let literal = names::c_string(text.as_bytes());
                    let line = match sink {
                        Sink::Out if text == "\n" => "gr_out_newline();".to_string(),
                        Sink::Out => format!("gr_out_bytes({literal}, {});", text.len()),
                        Sink::Builder(builder) => {
                            format!("gr_sb_bytes(&{builder}, {literal}, {});", text.len())
                        }
                    };
                    self.line(&line);
                }
                Part::Val(expr) => {
                    let value = values.next().expect("one value per part");
                    self.write_value(sink, value, &expr.ty);
                }
            }
        }
    }

    /// Writes the `to_s` of one value to a sink.
    fn write_value(&mut self, sink: &Sink, value: Val, ty: &Type) {
        match sink {
            Sink::Builder(builder) => {
                let value = if matches!(ty, Type::Record(_)) {
                    self.addressable(value, ty)
                } else {
                    value
                };
                let line = self
                    .cx
                    .fmt_stmt(&format!("&{builder}"), &value.c, ty, false);
                self.line(&line);
            }
            Sink::Out => {
                let line = match ty {
                    Type::Int => format!("gr_out_int({});", value.c),
                    Type::Float => format!("gr_out_float({});", value.c),
                    Type::Bool => format!("gr_out_bool({});", value.c),
                    Type::String => format!("gr_out_str({});", value.c),
                    Type::Entity => format!("gr_out_entity({});", value.c),
                    Type::Enum(id) => {
                        format!("gr_out_str({}[{}]);", self.cx.enum_names(*id), value.c)
                    }
                    Type::Record(_) | Type::List(_) => {
                        let builder = self.fresh("sb");
                        self.line(&format!("GrBuilder {builder} = {{0}};"));
                        self.write_value(&Sink::Builder(builder.clone()), value, ty);
                        format!("gr_sb_write(&{builder});")
                    }
                    Type::Void | Type::Compact(_) => unreachable!("not a value type"),
                };
                self.line(&line);
            }
        }
    }

    /// `to_s` of an expression.
    fn text_of(&mut self, arg: &'a Expr) -> Val {
        let function = match &arg.ty {
            Type::String => return self.rvalue(arg),
            Type::Int => "gr_int_to_s",
            Type::Float => "gr_float_to_s",
            Type::Bool => "gr_bool_to_s",
            Type::Entity => "gr_entity_to_s",
            Type::Enum(id) => {
                let value = self.expr(arg, Mode::Peek);
                let table = self.cx.enum_names(*id);
                return Val::lazy(format!("{table}[{}]", value.c), ATOM, value.dep);
            }
            Type::Record(_) | Type::List(_) => {
                let value = self.expr(arg, Mode::Peek);
                let builder = self.fresh("sb");
                self.line(&format!("GrBuilder {builder} = {{0}};"));
                self.write_value(&Sink::Builder(builder.clone()), value, &arg.ty);
                return Val::pending(format!("gr_sb_finish(&{builder})"), Own::Owned);
            }
            Type::Void | Type::Compact(_) => unreachable!("not a value type"),
        };
        let value = self.expr(arg, Mode::Peek);
        Val::pending(format!("{function}({})", value.c), Own::Owned)
    }

    fn builtin(&mut self, expr: &'a Expr, op: Builtin, args: &'a [Expr]) -> Val {
        if op == Builtin::ToS {
            return self.text_of(&args[0]);
        }
        let exprs: Vec<&'a Expr> = args.iter().collect();
        let values = self.operands(&exprs, Want::Peek);
        let dep = values
            .iter()
            .map(|value| value.dep)
            .max()
            .unwrap_or(Dep::Fixed);
        let call = |name: &str| format!("{name}({})", arg_list(&values));
        let pure = |name: &str| Val::lazy(call(name), ATOM, dep);
        let effect = |name: &str, own: Own| Val::pending(call(name), own);
        match op {
            Builtin::ToS => unreachable!("handled above"),
            Builtin::IntAbs => {
                if let Some(literal) = values[0].lit {
                    return Val::int(literal.wrapping_abs());
                }
                let value = self.once(values[0].clone(), &Type::Int);
                Val::lazy(
                    format!("({0} < 0 ? (int64_t)(0 - (uint64_t){0}) : {0})", value.c),
                    ATOM,
                    dep,
                )
            }
            Builtin::IntClamp | Builtin::FloatClamp => {
                let ty = &args[0].ty;
                let value = self.once(values[0].clone(), ty);
                let low = self.once(values[1].clone(), ty);
                let high = self.once(values[2].clone(), ty);
                Val::lazy(
                    format!(
                        "({0} < {1} ? {3} : ({0} > {2} ? {4} : {5}))",
                        value.p(ADD),
                        low.p(ADD),
                        high.p(ADD),
                        low.typed_int(),
                        high.typed_int(),
                        value.typed_int(),
                    ),
                    ATOM,
                    dep,
                )
            }
            Builtin::IntMin | Builtin::FloatMin | Builtin::IntMax | Builtin::FloatMax => {
                let ty = &args[0].ty;
                let symbol = if matches!(op, Builtin::IntMin | Builtin::FloatMin) {
                    "<"
                } else {
                    ">"
                };
                let left = self.once(values[0].clone(), ty);
                let right = self.once(values[1].clone(), ty);
                Val::lazy(
                    format!(
                        "({} {symbol} {} ? {} : {})",
                        left.p(ADD),
                        right.p(ADD),
                        left.typed_int(),
                        right.typed_int()
                    ),
                    ATOM,
                    dep,
                )
            }
            Builtin::FloatAbs => pure("fabs"),
            Builtin::FloatToI => pure("gr_float_to_i"),
            Builtin::FloatFloor => pure("gr_float_floor"),
            Builtin::FloatCeil => pure("gr_float_ceil"),
            Builtin::FloatRound => pure("gr_float_round"),
            Builtin::FloatRoundDigits => pure("gr_float_round_digits"),
            Builtin::FloatNan => {
                let value = self.once(values[0].clone(), &Type::Float);
                Val::lazy(format!("{0} != {0}", value.p(ADD)), EQ, dep)
            }
            Builtin::Sqrt => pure("sqrt"),
            Builtin::Sin => pure("sin"),
            Builtin::Cos => pure("cos"),
            Builtin::Tan => pure("tan"),
            Builtin::Atan2 => pure("atan2"),
            Builtin::Exp => pure("exp"),
            Builtin::Log => pure("log"),
            Builtin::StrLength => pure("gr_str_length"),
            Builtin::StrBytesize => pure("gr_str_bytesize"),
            Builtin::StrEmpty => Val::lazy(format!("{} == 0", call("gr_str_bytesize")), EQ, dep),
            Builtin::StrUpcase => effect("gr_str_upcase", Own::Owned),
            Builtin::StrDowncase => effect("gr_str_downcase", Own::Owned),
            Builtin::StrInclude => pure("gr_str_include"),
            Builtin::StrStartWith => pure("gr_str_start_with"),
            Builtin::StrEndWith => pure("gr_str_end_with"),
            Builtin::StrLjust => effect("gr_str_ljust", Own::Owned),
            Builtin::StrRjust => effect("gr_str_rjust", Own::Owned),
            Builtin::StrToI => pure("gr_str_to_i"),
            Builtin::StrToF => pure("gr_str_to_f"),
            Builtin::ListLength => pure("gr_list_len"),
            Builtin::ListEmpty => Val::lazy(format!("{} == 0", call("gr_list_len")), EQ, dep),
            Builtin::ListFirst | Builtin::ListLast => {
                let Type::List(elem) = &args[0].ty else {
                    unreachable!("first and last are list methods");
                };
                let list = self.once(values[0].clone(), &args[0].ty);
                let loc = self.cx.loc(expr.span);
                if op == Builtin::ListFirst {
                    self.line(&format!("gr_list_need({}, \"first\", {loc});", list.c));
                    self.load_elem(&list, "0", elem)
                } else {
                    let length = self.tmp();
                    self.line(&format!(
                        "int64_t {length} = gr_list_need({}, \"last\", {loc});",
                        list.c
                    ));
                    self.load_elem(&list, &format!("{length} - 1"), elem)
                }
            }
            Builtin::ListInclude => {
                let function = self.cx.include_fn(&args[0].ty);
                pure(&function)
            }
            Builtin::Rand => effect("gr_rand", Own::Free),
            Builtin::RandInt => {
                let loc = self.cx.loc(expr.span);
                Val::pending(format!("gr_rand_int({}, {loc})", values[0].c), Own::Free)
            }
            Builtin::Srand => effect("gr_srand", Own::Free),
            Builtin::SrandClock => effect("gr_srand_clock", Own::Free),
            Builtin::Clock => effect("gr_clock", Own::Free),
            Builtin::Sleep => effect("gr_sleep", Own::Free),
            Builtin::Exit => effect("gr_exit", Own::Free),
        }
    }
}

/// The values as the arguments of a C call.
fn arg_list(values: &[Val]) -> String {
    let list: Vec<&str> = values.iter().map(|value| value.c.as_str()).collect();
    list.join(", ")
}
