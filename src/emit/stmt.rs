//! Statements.

use std::collections::HashMap;
use std::fmt::Write as _;

use crate::source::Span;
use crate::tir::{
    BinOp, Block, CompoundOp, Cores, EventId, Expr, LocalId, Place, PlaceKind, Query, Run, Shape,
    Stmt, StmtKind, Target, Term, Type,
};

use super::analysis::{self, HoistKey, Node};
use super::body::{ADD, Dep, FnCx, Mode, Own, ScopeKind, UNARY, Val};
use super::expr::{Sink, Want, arg_list, push_lit, text_parts};
use super::names::{self, draw_kind};
use super::place::{Prepared, load};
use super::{LoopDesc, RESET_LINE};

/// The terms of an `each` whose component some GPU block names and whose
/// parameter the block uses.
struct GpuTerms {
    /// The `MnUse` of each.
    uses: Vec<String>,
    /// For each one the block can assign through without assigning it
    /// whole: the flag an assignment sets, and the component.
    written: Vec<(String, usize)>,
    /// Some term is assigned whole.
    overwrites: bool,
}

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
                run,
            } => match run {
                Run::Cores(cores) => self.each_cores(query, params, body, cores, stmt.span),
                Run::Inside | Run::Gpu | Run::OneCore(_) => self.each(query, params, body),
            },
            StmtKind::On { event, param, body } => self.on(*event, *param, body),
            StmtKind::Puts(args) => self.output(args, true),
            StmtKind::Print(args) => self.output(args, false),
            StmtKind::Emit { event, value } => self.emit_event(*event, value),
            StmtKind::WindowOpen {
                title,
                width,
                height,
            } => {
                let values = self.operands(&[title, width, height], Want::Peek);
                let loc = self.cx.loc(stmt.span);
                self.line(&format!("mn_window_open({}, {loc});", arg_list(&values)));
            }
            StmtKind::Draw { shape, args, .. } => self.draw(*shape, args),
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
        self.whole_store = true;
        let prepared = self.prepare(
            place,
            true,
            analysis::inert(value),
            analysis::local_safe(value),
        );
        self.whole_store = false;
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
        // A sum of an independent `each` is collected by its function and
        // added to the place when the loop has ended.
        if let CompoundOp::Builtin(sign @ (BinOp::Add | BinOp::Sub)) = op
            && let Some(collected) = self.sum_of(place)
        {
            let right = self.expr(value, Mode::Peek);
            let sign = if sign == BinOp::Add { "+" } else { "-" };
            self.line(&format!("{collected} {sign}= {};", right.c));
            return;
        }
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
        let (written, jump) = match &mut self.scopes[index].kind {
            ScopeKind::Each {
                label,
                used,
                written,
            } => {
                *used = true;
                (written.clone(), format!("goto {label};"))
            }
            _ => (Vec::new(), "break;".to_string()),
        };
        for line in written {
            self.line(&line);
        }
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

    /// Loads the resource fields and `Time` fields a block reads and never
    /// writes into constants, before its loop, and returns their names.
    fn hoist(&mut self, body: &'a Block) -> HashMap<HoistKey, String> {
        let prog = self.cx.prog;
        let mut hoists = HashMap::new();
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
        hoists
    }

    /// The terms of an `each` whose component some GPU block names and
    /// whose parameter the block uses. Declares a flag for each one the
    /// block can assign through without assigning it whole, which an
    /// assignment sets.
    fn gpu_terms(&mut self, query: &Query, params: &[LocalId], sfx: &str) -> GpuTerms {
        let mut terms = GpuTerms {
            uses: Vec::new(),
            written: Vec::new(),
            overwrites: false,
        };
        for (index, (term, &param)) in query.terms.iter().zip(params).enumerate() {
            if let Term::Component {
                component,
                write,
                touched: true,
                overwritten,
            } = term
                && self.cx.gpu.named[component.0]
            {
                let usage = if *overwritten {
                    "MN_USE_OVERWRITE"
                } else if *write {
                    "MN_USE_ASSIGN"
                } else {
                    "MN_USE_READ"
                };
                terms.uses.push(format!("{{{}, {usage}}}", component.0));
                terms.overwrites |= *overwritten;
                if *write && !*overwritten {
                    let flag = format!("w{sfx}_{index}");
                    self.line(&format!("bool {flag} = false;"));
                    self.flags.insert(param.0, flag.clone());
                    terms.written.push((flag, component.0));
                }
            }
        }
        terms
    }

    /// Declares, for the table an `each` is at, the column of each term
    /// the block uses, and the handles when it reads an `Entity`
    /// parameter. Returns those parameters. With `from`, row 0 of what is
    /// declared is that row of the table.
    fn columns(
        &mut self,
        query: &Query,
        params: &[LocalId],
        vector: bool,
        sfx: &str,
        from: Option<&str>,
    ) -> Vec<LocalId> {
        let prog = self.cx.prog;
        let restrict = if vector { "restrict " } else { "" };
        let mut entities = Vec::new();
        for (index, (term, &param)) in query.terms.iter().zip(params).enumerate() {
            match term {
                Term::Entity => {
                    if self.usage.read[param.0] {
                        entities.push(param);
                    }
                }
                Term::Component {
                    component,
                    write,
                    touched,
                    ..
                } => {
                    if !touched {
                        continue;
                    }
                    let record = prog.components[component.0]
                        .record
                        .expect("a tag is not a query term");
                    let cty = self.cx.cty(&Type::Record(record));
                    let konst = if *write { "" } else { "const " };
                    let column = match from {
                        Some(row) => format!(
                            "({konst}{cty} *)mn_column(table{sfx}, {}) + {row}",
                            component.0
                        ),
                        None => format!("mn_column(table{sfx}, {})", component.0),
                    };
                    self.line(&format!(
                        "{konst}{cty} *{restrict}c{sfx}{index} = {column};"
                    ));
                    self.views.insert(param.0, format!("c{sfx}{index}[i{sfx}]"));
                }
            }
        }
        if !entities.is_empty() {
            let from = from.map_or(String::new(), |row| format!(" + {row}"));
            self.line(&format!(
                "const MnEntity *ents{sfx} = table{sfx}->entities{from};"
            ));
        }
        entities
    }

    fn each(&mut self, query: &'a Query, params: &'a [LocalId], body: &'a Block) {
        let q = self.cx.query(query);
        let outer_view = |id: usize| self.views.contains_key(&id) && !params.contains(&LocalId(id));
        let vector = analysis::vector_ok(body) && !analysis::touches_local(body, &outer_view);
        let sfx = names::depth_suffix(self.each_depth);
        self.line("{");
        self.indent += 1;
        self.line(&format!("mn_query_update(&{q});"));
        self.line("mn_defer_enter();");

        let hoists = if vector {
            self.hoist(body)
        } else {
            HashMap::new()
        };
        self.hoists.push(hoists);

        // The columns of components some GPU block names are brought back
        // before the tables are visited, and a table in which the body
        // assigned through a parameter is reported after its rows.
        let GpuTerms {
            uses,
            written,
            overwrites,
        } = self.gpu_terms(query, params, &sfx);
        if !uses.is_empty() {
            let list = self.fresh("uses");
            self.line(&format!(
                "static const MnUse {list}[] = {{{}}};",
                uses.join(", ")
            ));
            let guard = if overwrites {
                "mn_gpu_active"
            } else {
                "mn_gpu_only"
            };
            self.line(&format!(
                "if ({guard}) mn_gpu_use(&{q}, {list}, {});",
                uses.len()
            ));
        }
        let report = |flag: &str, component: usize| {
            format!("if ({flag}) mn_wrote(table{sfx}, {component});")
        };
        let early: Vec<String> = written.iter().map(|(flag, c)| report(flag, *c)).collect();

        self.line(&format!(
            "for (uint32_t t{sfx} = 0; t{sfx} < {q}.table_count; t{sfx}++) {{"
        ));
        self.indent += 1;
        self.line(&format!("MnTable *table{sfx} = {q}.tables[t{sfx}];"));
        self.line(&format!("uint32_t n{sfx} = table{sfx}->count;"));
        self.line(&format!("if (n{sfx} == 0) continue;"));
        let entities = self.columns(query, params, vector, &sfx, None);
        self.line(&format!(
            "for (uint32_t i{sfx} = 0; i{sfx} < n{sfx}; i{sfx}++) {{"
        ));
        let label = format!("brk{}", &q[1..]);
        self.push_scope(ScopeKind::Each {
            label: label.clone(),
            used: false,
            written: early,
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
        for (flag, component) in &written {
            self.line(&format!(
                "if ({flag}) {{ mn_wrote(table{sfx}, {component}); {flag} = false; }}"
            ));
        }
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

    /// The variable that collects the sum a place is, in the function of
    /// an independent `each`.
    fn sum_of(&self, place: &Place) -> Option<String> {
        let target = match place.kind {
            PlaceKind::Local(local) => Target::Local(local),
            PlaceKind::ResourceField(resource, field) => Target::Field(resource, field),
            _ => return None,
        };
        self.sums
            .iter()
            .find(|(sum, _)| *sum == target)
            .map(|(_, collected)| collected.clone())
    }

    /// The locals of the code around an `each` that its block reads:
    /// those it names and neither creates nor adds to as a sum.
    fn captured(&self, params: &[LocalId], body: &Block, cores: &Cores) -> Vec<LocalId> {
        let mut inside = vec![false; self.locals.len()];
        let mut named = vec![false; self.locals.len()];
        for param in params {
            inside[param.0] = true;
        }
        analysis::walk_block(body, &mut |node| match node {
            Node::Stmt(stmt) => match &stmt.kind {
                StmtKind::Let { local, .. } => inside[local.0] = true,
                StmtKind::Count { var: Some(var), .. } => inside[var.0] = true,
                StmtKind::ListEach { elem, index, .. } => {
                    for local in elem.iter().chain(index) {
                        inside[local.0] = true;
                    }
                }
                StmtKind::Each { params, .. } => {
                    for param in params {
                        inside[param.0] = true;
                    }
                }
                StmtKind::On {
                    param: Some(param), ..
                } => inside[param.0] = true,
                _ => {}
            },
            Node::Place(place, _) => {
                if let PlaceKind::Local(local) = analysis::root(place).kind {
                    named[local.0] = true;
                }
            }
            Node::Expr(_) => {}
        });
        (0..self.locals.len())
            .map(LocalId)
            .filter(|local| {
                named[local.0] && !inside[local.0] && !cores.sums.contains(&Target::Local(*local))
            })
            .collect()
    }

    /// An independent `each`. Its rows are run by a function of its own,
    /// which `mn_loop` calls once for the whole loop or once for each
    /// piece of it. What the block reads of the code around it travels in
    /// a struct, and its sums come back as totals.
    fn each_cores(
        &mut self,
        query: &'a Query,
        params: &'a [LocalId],
        body: &'a Block,
        cores: &'a Cores,
        span: Span,
    ) {
        assert!(
            self.each_depth == 0,
            "an independent `each` is not inside another"
        );
        let prog = self.cx.prog;
        let q = self.cx.query(query);
        let number = q[1..].to_string();
        let captured = self.captured(params, body, cores);
        let env_type = format!("Env{number}");

        let outer_indent = std::mem::replace(&mut self.indent, 1);
        let (text, (uses, wrote)) = self.capture(|cx| {
            if !captured.is_empty() {
                cx.line(&format!("const {env_type} *env = env_;"));
                let locals = cx.locals;
                for local in &captured {
                    let cty = cx.cx.cty(&locals[local.0].ty);
                    let name = cx.names[local.0].clone();
                    cx.line(&format!("{cty} {name} = env->{name};"));
                }
            }
            let hoists = if analysis::vector_ok(body) {
                cx.hoist(body)
            } else {
                HashMap::new()
            };
            cx.hoists.push(hoists);
            let mut sums = Vec::new();
            for target in &cores.sums {
                let collected = cx.fresh("sum");
                cx.line(&format!("int64_t {collected} = 0;"));
                sums.push((*target, collected));
            }
            cx.sums = sums;
            let terms = cx.gpu_terms(query, params, "");

            cx.line("for (uint32_t t = 0; t < piece->table_count; t++) {");
            cx.indent += 1;
            // The rows of the piece in this table are counted from 0, as
            // the rows of a table are in a loop that is not split.
            cx.line("MnTable *table = piece->tables[t];");
            cx.line("const uint32_t first = t == 0 ? piece->first : 0;");
            cx.line(
                "const uint32_t end = t + 1 == piece->table_count ? piece->end : table->count;",
            );
            cx.line("if (first >= end) continue;");
            cx.line("uint32_t n = end - first;");
            let entities = cx.columns(query, params, analysis::vector_ok(body), "", Some("first"));
            cx.line("for (uint32_t i = 0; i < n; i++) {");
            cx.push_scope(ScopeKind::Each {
                label: String::new(),
                used: false,
                written: Vec::new(),
            });
            cx.indent += 1;
            cx.each_depth += 1;
            for id in entities {
                let name = cx.names[id.0].clone();
                cx.line(&format!("MnEntity {name} = ents[i];"));
            }
            cx.block_in_scope(body, None);
            cx.pop_scope(analysis::block_leaves(body));
            cx.each_depth -= 1;
            cx.indent -= 1;
            cx.line("}");
            let flags = terms.written.len();
            for (index, (flag, _)) in terms.written.iter().enumerate() {
                cx.line(&format!(
                    "if (piece->wrote) piece->wrote[(size_t)t * {flags} + {index}] |= {flag};"
                ));
                cx.line(&format!("{flag} = false;"));
            }
            cx.indent -= 1;
            cx.line("}");
            cx.hoists.pop();
            for (index, (_, collected)) in std::mem::take(&mut cx.sums).iter().enumerate() {
                cx.line(&format!("piece->sums[{index}] += {collected};"));
            }
            let wrote: Vec<usize> = terms.written.iter().map(|(_, c)| *c).collect();
            (terms.uses, wrote)
        });
        self.indent = outer_indent;

        let mut def = String::new();
        let _ = writeln!(def, "{RESET_LINE}");
        let env_param = if captured.is_empty() {
            "const void *env_ __attribute__((unused))"
        } else {
            let _ = writeln!(def, "typedef struct {env_type} {{");
            for local in &captured {
                let _ = writeln!(
                    def,
                    "    {} {};",
                    self.cx.cty(&self.locals[local.0].ty),
                    self.names[local.0]
                );
            }
            let _ = writeln!(def, "}} {env_type};\n");
            "const void *env_"
        };
        let _ = writeln!(def, "{}", self.cx.line_directive(span));
        let _ = writeln!(
            def,
            "static void L{number}(MnPiece *piece, {env_param}) {{\n{text}}}\n"
        );
        self.cx.loop_defs.push_str(&def);
        self.cx.loops.push(LoopDesc {
            number: number.clone(),
            cores,
            span,
            uses,
            wrote,
        });

        self.line("{");
        self.indent += 1;
        let env = if captured.is_empty() {
            "NULL".to_string()
        } else {
            let values: Vec<String> = captured
                .iter()
                .map(|local| self.names[local.0].clone())
                .collect();
            self.line(&format!(
                "{env_type} env{number} = {{{}}};",
                values.join(", ")
            ));
            format!("&env{number}")
        };
        let sums = if cores.sums.is_empty() {
            "NULL".to_string()
        } else {
            self.line(&format!("int64_t sums{number}[{}];", cores.sums.len()));
            format!("sums{number}")
        };
        self.line(&format!("mn_loop(&loop{number}, {env}, {sums});"));
        for (index, target) in cores.sums.iter().enumerate() {
            let place = match *target {
                Target::Local(local) => self.names[local.0].clone(),
                Target::Field(resource, field) => {
                    let item = &prog.resources[resource.0];
                    format!(
                        "{}.{}",
                        names::resource(&item.name),
                        names::field(&item.fields[field].name)
                    )
                }
            };
            self.line(&format!("{place} = {place} + sums{number}[{index}];"));
        }
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

    /// A draw: its arguments in order, then its record, filled at once; a
    /// draw of text hands its arguments to the runtime, which copies the
    /// text.
    fn draw(&mut self, shape: Shape, args: &'a [Expr]) {
        let exprs: Vec<&'a Expr> = args.iter().collect();
        let values = self.operands(&exprs, Want::Peek);
        let (color_value, nums) = values.split_last().expect("a draw has a color");
        let color = args.last().expect("a draw has a color");
        let color_name = self.declare(&color.ty, &color_value.c);
        if shape == Shape::Text {
            let [x, y, text, size] = nums else {
                unreachable!("a draw of text has five arguments")
            };
            let color = self.tmp();
            self.line(&format!("MnColor {color};"));
            self.line(&format!("memcpy(&{color}, &{color_name}, sizeof {color});"));
            self.line(&format!(
                "mn_draw_text((float)({}), (float)({}), {}, (float)({}), {color});",
                x.c, y.c, text.c, size.c
            ));
            return;
        }
        let record = self.tmp();
        self.line(&format!("MnDraw *const {record} = mn_draw();"));
        self.line(&format!("{record}->kind = {};", draw_kind(shape)));
        for (index, num) in nums.iter().enumerate() {
            self.line(&format!("{record}->n[{index}] = (float)({});", num.c));
        }
        self.line(&format!(
            "memcpy(&{record}->color, &{color_name}, sizeof {record}->color);"
        ));
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
