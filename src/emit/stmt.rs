//! Statements.

use std::collections::HashMap;

use crate::source::Span;
use crate::tir::{
    Block, CompoundOp, EventId, Expr, LocalId, Place, Query, Stmt, StmtKind, Term, Type,
};

use super::analysis::{self, HoistKey};
use super::body::{ADD, Dep, FnCx, Mode, Own, ScopeKind, UNARY, Val};
use super::expr::{Sink, Want, push_lit, text_parts};
use super::names;
use super::place::{Prepared, load};

impl<'a> FnCx<'a, '_> {
    pub(super) fn stmts(&mut self, stmts: &'a [Stmt]) {
        for stmt in stmts {
            self.stmt(stmt);
        }
    }

    fn stmt(&mut self, stmt: &'a Stmt) {
        self.directive(stmt.span);
        let mark = self.mark();
        match &stmt.kind {
            StmtKind::Expr(expr) => {
                let value = self.rvalue(expr);
                self.discard(value, &expr.ty);
            }
            StmtKind::Let { local, init } => self.let_(*local, init),
            StmtKind::Assign { place, value } => self.assign(place, value),
            StmtKind::CompoundAssign { place, op, value } => {
                self.compound(place, *op, value, stmt.span);
            }
            StmtKind::Return(value) => self.ret_value(value.as_ref()),
            StmtKind::Break => self.jump(true),
            StmtKind::Next => self.jump(false),
            StmtKind::While { cond, body } => self.while_(cond, body),
            StmtKind::Count {
                var,
                from,
                to,
                inclusive,
                down,
                body,
            } => self.count(*var, from, to, *inclusive, *down, body),
            StmtKind::ListEach {
                list,
                elem,
                index,
                body,
            } => self.list_each(list, *elem, *index, body),
            StmtKind::Each {
                query,
                params,
                body,
            } => self.each(query, params, body),
            StmtKind::On { event, param, body } => self.on(*event, *param, body),
            StmtKind::Puts(args) => self.output(args, true),
            StmtKind::Print(args) => self.output(args, false),
            StmtKind::Emit { event, value } => self.emit_event(*event, value),
        }
        if analysis::leaves(stmt) {
            self.forget(mark);
        } else {
            self.sweep(mark);
        }
    }

    fn let_(&mut self, local: LocalId, init: &'a Expr) {
        let locals = self.locals;
        let ty = &locals[local.0].ty;
        let value = self.rvalue(init);
        let name = self.names[local.0].clone();
        let cty = self.cx.cty(ty);
        let unused = self.unused_mark(local);
        self.line(&format!("{cty} {name}{unused} = {};", value.c));
        if self.cx.managed(ty) {
            self.adopt(&name, &value, ty);
            self.register(&name, ty, false);
        }
    }

    fn assign(&mut self, place: &'a Place, value: &'a Expr) {
        let prepared = self.prepare(
            place,
            true,
            analysis::inert(value),
            analysis::local_safe(value),
        );
        let evaluated = self.rvalue(value);
        let managed = self.cx.managed(&value.ty);
        if !managed && prepared.direct() {
            let lv = self.addr(&prepared, true);
            let c = self.store_c(&evaluated, &place.ty, value.span);
            self.line(&format!("{} = {c};", lv.c));
            return;
        }
        let evaluated = self.materialize(evaluated, &value.ty);
        let evaluated = self.ensure(evaluated, &value.ty, Mode::Own);
        let lv = self.addr(&prepared, true);
        if managed {
            let c = self.take(evaluated, &value.ty);
            let drop = self.cx.drop_stmt(&lv.c, &value.ty);
            self.line(&drop);
            self.line(&format!("{} = {c};", lv.c));
        } else {
            let c = self.store_c(&evaluated, &place.ty, value.span);
            self.line(&format!("{} = {c};", lv.c));
        }
    }

    /// `place op= value`. The place is read before `value` is evaluated
    /// whenever evaluating `value` could change it; it is then looked up
    /// again for the write.
    fn compound(&mut self, place: &'a Place, op: CompoundOp, value: &'a Expr, span: Span) {
        let value_type = place.ty.value_type();
        let managed = self.cx.managed(&value_type);
        let value_inert = analysis::inert(value);
        let value_safe = analysis::local_safe(value);
        let calls = matches!(op, CompoundOp::Method(_));
        let prepared = self.prepare(place, true, value_inert && !calls, value_safe);
        let in_local = prepared.direct() && prepared.stable;
        let unchanged = value_inert || (in_local && value_safe);
        let saved = if unchanged {
            None
        } else {
            Some(self.save_current(&prepared, place, &value_type))
        };
        match op {
            CompoundOp::Builtin(op) => {
                let (left, right, lv) = match saved {
                    Some(left) => {
                        let right = self.expr(value, Mode::Own);
                        (left, right, self.addr(&prepared, true))
                    }
                    None => {
                        let mode = if prepared.direct() {
                            Mode::Peek
                        } else {
                            Mode::Own
                        };
                        let right = self.expr(value, mode);
                        let lv = self.addr(&prepared, true);
                        let (c, prec) = load(&lv.c, lv.prec, &place.ty);
                        let own = if managed {
                            Own::Borrowed { stable: false }
                        } else {
                            Own::Free
                        };
                        let left = Val {
                            own,
                            ..Val::lazy(c, prec, Dep::World)
                        };
                        (left, right, lv)
                    }
                };
                let result = self.binary(op, &value_type, left, right, span);
                if managed {
                    let result = self.materialize(result, &value_type);
                    let c = self.take(result, &value_type);
                    let drop = self.cx.drop_stmt(&lv.c, &value_type);
                    self.line(&drop);
                    self.line(&format!("{} = {c};", lv.c));
                } else {
                    let c = self.store_c(&result, &place.ty, span);
                    self.line(&format!("{} = {c};", lv.c));
                }
            }
            CompoundOp::Method(func) => {
                let arg = self.expr(value, Mode::Stable);
                self.cx.func_used[func.0] = true;
                let name = names::func(self.cx.prog, func.0);
                let current = match saved {
                    Some(current) => current,
                    None => {
                        if prepared.direct() && !managed {
                            let lv = self.addr(&prepared, false);
                            self.line(&format!("{0} = {name}({0}, {1});", lv.c, arg.c));
                            return;
                        }
                        if in_local {
                            let lv = self.addr(&prepared, false);
                            let call = format!("{name}({}, {})", lv.c, arg.c);
                            let result = self.declare(&value_type, &call);
                            let drop = self.cx.drop_stmt(&lv.c, &value_type);
                            self.line(&drop);
                            self.line(&format!("{} = {result};", lv.c));
                            return;
                        }
                        self.save_current(&prepared, place, &value_type)
                    }
                };
                let call = format!("{name}({}, {})", current.c, arg.c);
                let result = self.declare(&value_type, &call);
                let lv = self.addr(&prepared, true);
                if managed {
                    let drop = self.cx.drop_stmt(&lv.c, &value_type);
                    self.line(&drop);
                }
                self.line(&format!("{} = {result};", lv.c));
            }
        }
    }

    /// Copies the current value of a prepared place into a temporary that
    /// owns it.
    fn save_current(&mut self, prepared: &Prepared, place: &Place, value_type: &Type) -> Val {
        let lv = self.addr(prepared, false);
        let (c, _) = load(&lv.c, lv.prec, &place.ty);
        let name = self.declare(value_type, &c);
        if !self.cx.managed(value_type) {
            return Val::atom(name);
        }
        let retain = self.cx.retain_stmt(&name, value_type);
        self.line(&retain);
        self.register(&name, value_type, true);
        Val {
            own: Own::Owned,
            ..Val::atom(name)
        }
    }

    /// `return`: evaluates the value, releases everything every open scope
    /// owns, ends the deferral of every enclosing `each`, and returns.
    pub(super) fn ret_value(&mut self, value: Option<&'a Expr>) {
        let Some(expr) = value else {
            self.unwind(0, true);
            self.line("return;");
            return;
        };
        let ty = &expr.ty;
        let evaluated = self.rvalue(expr);
        let c = if self.cx.managed(ty) {
            let evaluated = self.materialize(evaluated, ty);
            let evaluated = self.ensure(evaluated, ty, Mode::Own);
            self.take(evaluated, ty)
        } else if self.has_cleanup()
            && (evaluated.pending || !evaluated.simple || evaluated.dep != Dep::Fixed)
        {
            self.declare(ty, &evaluated.c)
        } else {
            evaluated.c
        };
        self.unwind(0, true);
        self.line(&format!("return {c};"));
    }

    /// `break` (with `leave`) or `next`: releases what the scopes inside
    /// the nearest loop own, then jumps.
    fn jump(&mut self, leave: bool) {
        let index = self
            .scopes
            .iter()
            .rposition(|scope| !matches!(scope.kind, ScopeKind::Plain))
            .expect("break and next are inside a loop");
        self.unwind(index, false);
        if !leave {
            self.line("continue;");
            return;
        }
        let jump = match &mut self.scopes[index].kind {
            ScopeKind::Each { label, used } => {
                *used = true;
                format!("goto {label};")
            }
            _ => "break;".to_string(),
        };
        self.line(&jump);
    }

    /// Generates a loop body in a new loop scope and closes the C block.
    fn loop_body(&mut self, body: &'a Block) {
        self.block_in_scope(body, None);
        self.pop_scope(analysis::block_leaves(body));
        self.indent -= 1;
        self.line("}");
    }

    fn while_(&mut self, cond: &'a Expr, body: &'a Block) {
        self.push_scope(ScopeKind::Loop);
        self.indent += 1;
        let (text, value) = self.capture(|cx| cx.expr(cond, Mode::Peek));
        self.indent -= 1;
        let scope_empty = self.mark() == 0;
        if text.is_empty() && scope_empty {
            self.line(&format!("while ({}) {{", value.c));
            self.indent += 1;
        } else {
            self.line("while (1) {");
            self.out.push_str(&text);
            self.indent += 1;
            let value = if scope_empty {
                value
            } else {
                self.stabilize(
                    Val {
                        dep: Dep::World,
                        ..value
                    },
                    &Type::Bool,
                )
            };
            self.sweep(0);
            self.line(&format!("if (!{}) break;", value.p(UNARY)));
        }
        self.loop_body(body);
    }

    fn count(
        &mut self,
        var: Option<LocalId>,
        from: &'a Expr,
        to: &'a Expr,
        inclusive: bool,
        down: bool,
        body: &'a Block,
    ) {
        let mut values = self.operands(&[from, to], Want::Peek);
        let to = values.pop().expect("two operands");
        let from = values.pop().expect("two operands");
        // An inclusive bound that may be the end of `int64_t` is counted in
        // rounds, so the counter never steps past it.
        let end = if down { i64::MIN } else { i64::MAX };
        let guarded = inclusive && !matches!(to.lit, Some(n) if n != end);
        if guarded {
            let from = self.stabilize(from, &Type::Int);
            let to = self.stabilize(to, &Type::Int);
            let read = matches!(var, Some(id) if self.usage.read[id.0]);
            let name = self.loop_var(var, "k");
            let round = self.fresh("round");
            let rounds = self.fresh("rounds");
            let (low, high, step) = if down {
                (to.p(ADD), from.p(ADD), "--")
            } else {
                (from.p(ADD), to.p(ADD), "++")
            };
            // The counter steps once past the last round, wrapping
            // (`-fwrapv`) when that round is at the end of `int64_t`.
            let counter = if read {
                self.line(&format!("int64_t {name} = {};", from.c));
                format!(", {name}{step}")
            } else {
                String::new()
            };
            self.line(&format!(
                "for (uint64_t {round} = 0, {rounds} = mn_rounds({low}, {high}); {round} < {rounds}; {round}++{counter}) {{"
            ));
        } else {
            let to = self.stabilize(to, &Type::Int);
            let name = self.loop_var(var, "k");
            let (compare, step) = match (down, inclusive) {
                (false, false) => ("<", "++"),
                (false, true) => ("<=", "++"),
                (true, false) => (">", "--"),
                (true, true) => (">=", "--"),
            };
            self.line(&format!(
                "for (int64_t {name} = {}; {name} {compare} {}; {name}{step}) {{",
                from.c,
                to.p(ADD)
            ));
        }
        self.push_scope(ScopeKind::Loop);
        self.indent += 1;
        self.loop_body(body);
    }

    /// The C name of a loop's counter: the program's variable when it has
    /// one and reads it, a new name otherwise.
    fn loop_var(&mut self, var: Option<LocalId>, prefix: &str) -> String {
        match var {
            Some(id) if self.usage.read[id.0] => {
                self.fixed[id.0] = true;
                self.names[id.0].clone()
            }
            _ => self.fresh(prefix),
        }
    }

    fn list_each(
        &mut self,
        list: &'a Expr,
        elem: Option<LocalId>,
        index: Option<LocalId>,
        body: &'a Block,
    ) {
        let Type::List(elem_type) = &list.ty else {
            unreachable!("each on something that is not a list");
        };
        let snapshot = self.expr(list, Mode::Own);
        let snapshot = self.once(snapshot, &list.ty);
        let length = self.fresh("n");
        self.line(&format!("int64_t {length} = mn_list_len({});", snapshot.c));
        let position = self.loop_var(index, "k");
        self.line(&format!(
            "for (int64_t {position} = 0; {position} < {length}; {position}++) {{"
        ));
        self.push_scope(ScopeKind::Loop);
        self.indent += 1;
        if let Some(id) = elem
            && self.usage.read[id.0]
        {
            self.fixed[id.0] = true;
            let value_type = elem_type.value_type();
            let slot = format!(
                "((const {} *)mn_list_data({}))[{position}]",
                self.cx.cty(elem_type),
                snapshot.c
            );
            let (c, _) = load(&slot, 0, elem_type);
            let name = self.names[id.0].clone();
            self.line(&format!("{} {name} = {c};", self.cx.cty(&value_type)));
            if self.cx.managed(&value_type) {
                let retain = self.cx.retain_stmt(&name, &value_type);
                self.line(&retain);
                self.register(&name, &value_type, false);
            }
        }
        self.loop_body(body);
    }

    fn each(&mut self, query: &'a Query, params: &'a [LocalId], body: &'a Block) {
        let prog = self.cx.prog;
        let q = self.cx.query(query);
        let outer_view = |id: usize| self.views.contains_key(&id) && !params.contains(&LocalId(id));
        let vector = analysis::vector_ok(body) && !analysis::touches_local(body, &outer_view);
        let sfx = names::depth_suffix(self.each_depth);
        self.line("{");
        self.indent += 1;
        self.line(&format!("mn_query_update(&{q});"));
        self.line("mn_defer_enter();");

        let mut hoists = HashMap::new();
        if vector {
            for key in analysis::hoistable(body) {
                let fixed = |name: &str, cty: &str, source: &str| {
                    (name.to_string(), cty.to_string(), source.to_string())
                };
                let (name, cty, source) = match key {
                    HoistKey::Time(0) => fixed("time_tick", "int64_t", "mn_time.tick"),
                    HoistKey::Time(1) => fixed("time_dt", "double", "mn_time.dt"),
                    HoistKey::Time(_) => fixed("time_elapsed", "double", "mn_time.elapsed"),
                    HoistKey::Resource(resource, field) => {
                        let item = &prog.resources[resource];
                        let member = names::field(&item.fields[field].name);
                        (
                            format!("R{resource}_{member}"),
                            self.cx.cty(&item.fields[field].ty),
                            format!("{}.{member}", names::resource(&item.name)),
                        )
                    }
                };
                self.line(&format!("const {cty} {name} = {source};"));
                hoists.insert(key, name);
            }
        }
        self.hoists.push(hoists);

        self.line(&format!(
            "for (uint32_t t{sfx} = 0; t{sfx} < {q}.table_count; t{sfx}++) {{"
        ));
        self.indent += 1;
        self.line(&format!("MnTable *table{sfx} = {q}.tables[t{sfx}];"));
        self.line(&format!("uint32_t n{sfx} = table{sfx}->count;"));
        self.line(&format!("if (n{sfx} == 0) continue;"));
        let restrict = if vector { "restrict " } else { "" };
        let mut entities = Vec::new();
        for (index, (term, &param)) in query.terms.iter().zip(params).enumerate() {
            match term {
                Term::Entity => {
                    if self.usage.read[param.0] {
                        entities.push(param);
                    }
                }
                Term::Component { component, write } => {
                    if !self.usage.touched[param.0] {
                        continue;
                    }
                    let record = prog.components[component.0]
                        .record
                        .expect("a tag is not a query term");
                    let cty = self.cx.cty(&Type::Record(record));
                    let konst = if *write { "" } else { "const " };
                    if self.cx.gpu.named[component.0] {
                        self.line(&format!("mn_sync(table{sfx}, {}, {write});", component.0));
                    }
                    self.line(&format!(
                        "{konst}{cty} *{restrict}c{sfx}{index} = mn_column(table{sfx}, {});",
                        component.0
                    ));
                    self.views.insert(param.0, format!("c{sfx}{index}[i{sfx}]"));
                }
            }
        }
        if !entities.is_empty() {
            self.line(&format!(
                "const MnEntity *ents{sfx} = table{sfx}->entities;"
            ));
        }
        self.line(&format!(
            "for (uint32_t i{sfx} = 0; i{sfx} < n{sfx}; i{sfx}++) {{"
        ));
        let label = format!("brk{}", &q[1..]);
        self.push_scope(ScopeKind::Each {
            label: label.clone(),
            used: false,
        });
        self.indent += 1;
        self.each_depth += 1;
        for id in entities {
            let name = self.names[id.0].clone();
            self.line(&format!("MnEntity {name} = ents{sfx}[i{sfx}];"));
        }
        self.block_in_scope(body, None);
        let jumped = matches!(
            self.scopes.last().map(|scope| &scope.kind),
            Some(ScopeKind::Each { used: true, .. })
        );
        self.pop_scope(analysis::block_leaves(body));
        self.each_depth -= 1;
        self.indent -= 1;
        self.line("}");
        self.indent -= 1;
        self.line("}");
        self.hoists.pop();
        if jumped {
            self.out.push_str(&format!("{label}:\n"));
        }
        self.line("mn_defer_leave();");
        self.indent -= 1;
        self.line("}");
    }

    fn on(&mut self, event: EventId, param: Option<LocalId>, body: &'a Block) {
        let cursor = self.cx.cursor();
        let number = self.fresh("");
        let end = format!("end{number}");
        let seq = format!("seq{number}");
        self.line(&format!("uint64_t {end} = mn_event_end({});", event.0));
        self.line(&format!("uint64_t {seq} = {cursor};"));
        self.line(&format!("{cursor} = {end};"));
        self.line(&format!("for (; {seq} < {end}; {seq}++) {{"));
        self.push_scope(ScopeKind::Loop);
        self.indent += 1;
        match param {
            Some(id) if self.usage.read[id.0] => {
                let locals = self.locals;
                let ty = &locals[id.0].ty;
                let cty = self.cx.cty(ty);
                let name = self.names[id.0].clone();
                self.fixed[id.0] = true;
                self.line(&format!(
                    "const {cty} *ev{number} = mn_event_at({}, {seq});",
                    event.0
                ));
                self.line(&format!("if (!ev{number}) continue;"));
                self.line(&format!("{cty} {name} = *ev{number};"));
                if self.cx.managed(ty) {
                    let retain = self.cx.retain_stmt(&name, ty);
                    self.line(&retain);
                    self.register(&name, ty, false);
                }
            }
            _ => self.line(&format!("if (!mn_event_at({}, {seq})) continue;", event.0)),
        }
        self.loop_body(body);
    }

    /// `puts` (with `newline`) and `print`: evaluates every argument, then
    /// writes their text.
    fn output(&mut self, args: &'a [Expr], newline: bool) {
        let mut parts = Vec::new();
        for arg in args {
            text_parts(arg, &mut parts);
            if newline {
                push_lit(&mut parts, "\n");
            }
        }
        if args.is_empty() && newline {
            push_lit(&mut parts, "\n");
        }
        let values = self.eval_parts(&parts);
        self.write_parts(&parts, values, &Sink::Out);
    }

    fn emit_event(&mut self, event: EventId, value: &'a Expr) {
        let ty = &value.ty;
        let evaluated = self.expr(value, Mode::Own);
        let owned = evaluated.own == Own::Owned;
        let c = self.take(evaluated, ty);
        let name = if owned { c } else { self.declare(ty, &c) };
        let cty = self.cx.cty(ty);
        self.line(&format!("*({cty} *)mn_emit({}) = {name};", event.0));
    }
}
