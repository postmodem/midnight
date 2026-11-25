//! Typed tree to the intermediate form: one walk, whose result both
//! printers read.
//!
//! A run keeps each outer component it uses in a local variable of the
//! component's 32-bit form: the fields the block reads or assigns are loaded
//! when the run starts and the fields it assigns are stored when it ends. The
//! parameters of the nested `each` are loaded the same way from the
//! snapshot, once per round. Everything else is ordinary code on locals.

use std::collections::HashMap;

use crate::source::Span;
use crate::tir::{
    BinOp as TBinOp, Block as TBlock, Body, Builtin, Compact, CompoundOp, Expr as TExpr, ExprKind,
    FuncId, Local, LocalId, LocalKind, Place, PlaceKind, Program, Query, Stmt as TStmt, StmtKind,
    System, Term, TimeField, Type, UnOp as TUnOp,
};

use super::super::analysis::{self, Folded, LocalUse, Node, Use};
use super::super::names;
use super::helpers;
use super::ir::{
    BinOp, Cell, Expr, Func, Library, MathFn, Param, Stmt, Storage, Ty, UnOp, Uses, assign, call,
    choose, closure, compare, field, float, float_arith, if_, int_arith, let_, logic, not, unary,
    var,
};
use super::layout::{
    HEADER, Layouts, Leaf, Source, Uniforms, WORD_DT, WORD_ELAPSED, WORD_SNAPSHOT_ROWS, WORD_TICK,
    field_name, value_ty, zero,
};

/// An operation that can fail, where the source has it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Site {
    pub span: Span,
    pub message: String,
}

/// A column a block is given: a component's, or with `None` the handles
/// of the rows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Column {
    pub component: Option<usize>,
    pub write: bool,
}

pub struct Block<'a> {
    /// The system's name.
    pub system: &'a str,
    /// The `each` statement.
    pub span: Span,
    /// Which `each` of the system, from 1.
    pub each: u32,
    /// Unique in the program, from 0.
    pub id: u32,
    pub outer_query: &'a Query,
    pub inner_query: Option<&'a Query>,
    pub outer: Vec<Column>,
    pub inner: Vec<Column>,
    /// One run: the code for the row `row`. Inside the nested loop the
    /// snapshot row is `j`.
    pub body: Vec<Stmt>,
    /// What the run uses, itself and through the functions it reaches.
    pub uses: Uses,
    /// The uniform entries it reads, ascending.
    pub entries: Vec<usize>,
    pub uniform_words: u32,
}

/// The GPU code of a program.
pub struct Lowered<'a> {
    pub prog: &'a Program,
    /// By record: its alignment is raised to 4.
    pub wide: Vec<bool>,
    pub library: Library,
    pub blocks: Vec<Block<'a>>,
    /// In source order: by file in the order the files were loaded, then
    /// by position.
    pub sites: Vec<Site>,
    /// The index in `sites` of each `Expr::Site`.
    pub site_index: Vec<usize>,
    pub uniforms: Uniforms,
}

impl Lowered<'_> {
    pub fn layouts(&self) -> Layouts<'_> {
        Layouts {
            prog: self.prog,
            wide: &self.wide,
        }
    }
}

/// The name of the 32-bit form of a function.
pub fn func_name(prog: &Program, id: usize) -> String {
    format!("G{}", names::func(prog, id))
}

/// The name of the function that compares two values of a record.
fn eq_name(prog: &Program, record: usize) -> String {
    format!("Geq_{}", names::sanitize(&prog.records[record].name))
}

/// The outer loop's row variable and the nested loop's.
pub const ROW: &str = "row";
pub const INNER_ROW: &str = "j";

pub fn lower<'a>(prog: &'a Program, folded: &[Option<Folded>], wide: Vec<bool>) -> Lowered<'a> {
    let mut lower = Lower {
        prog,
        folded: folded.to_vec(),
        wide,
        uniforms: Uniforms::new(),
        sites: Vec::new(),
        library: Library::default(),
        wanted: vec![false; prog.funcs.len()],
        waiting: Vec::new(),
    };
    let mut drafts = Vec::new();
    for system in prog.systems.iter().filter(|system| system.gpu) {
        for (index, stmt) in system.body.block.stmts.iter().enumerate() {
            drafts.push(lower.draft(system, index as u32 + 1, stmt));
        }
    }
    while let Some(id) = lower.waiting.pop() {
        let func = lower.function(id);
        lower.library.add(func);
    }
    let blocks = drafts
        .into_iter()
        .enumerate()
        .map(|(id, draft)| lower.finish(id as u32, draft))
        .collect();

    let mut order: Vec<usize> = (0..lower.sites.len()).collect();
    order.sort_by_key(|&site| (lower.sites[site].span.file.0, lower.sites[site].span.start));
    let mut site_index = vec![0; order.len()];
    for (index, &site) in order.iter().enumerate() {
        site_index[site] = index;
    }
    let sites = order
        .iter()
        .map(|&site| lower.sites[site].clone())
        .collect();
    Lowered {
        prog,
        wide: lower.wide,
        library: lower.library,
        blocks,
        sites,
        site_index,
        uniforms: lower.uniforms,
    }
}

struct Lower<'a> {
    prog: &'a Program,
    folded: Vec<Option<Folded>>,
    wide: Vec<bool>,
    uniforms: Uniforms,
    /// In the order they were found.
    sites: Vec<Site>,
    library: Library,
    /// By function: its 32-bit form is in the library or waiting.
    wanted: Vec<bool>,
    waiting: Vec<usize>,
}

/// A block whose body is lowered but whose use of `rand`, of the uniform
/// words and of the error word is not known yet: that takes the functions
/// it calls.
struct Draft<'a> {
    system: &'a str,
    span: Span,
    each: u32,
    outer_query: &'a Query,
    inner_query: Option<&'a Query>,
    outer: Vec<Column>,
    inner: Vec<Column>,
    /// Loads of the run's own row.
    prologue: Vec<Stmt>,
    body: Vec<Stmt>,
    /// Stores to the run's own row.
    epilogue: Vec<Stmt>,
}

/// The field paths through which code reads and writes one local.
#[derive(Default)]
struct Paths {
    read: Vec<Vec<usize>>,
    write: Vec<Vec<usize>>,
}

impl Paths {
    fn covers(paths: &[Vec<usize>], leaf: &[usize]) -> bool {
        paths.iter().any(|path| leaf.starts_with(path))
    }
}

/// The local a place starts from, or the place it starts from when that
/// is not a local, and the fields that lead from there: record and index.
fn chain(place: &Place) -> (&Place, Vec<(usize, usize)>) {
    let mut fields = Vec::new();
    let mut current = place;
    loop {
        match &current.kind {
            PlaceKind::Field { base, field } => {
                let Type::Record(record) = &base.ty else {
                    unreachable!("a field of something that is not a record");
                };
                fields.push((record.0, *field));
                current = base;
            }
            PlaceKind::Index { .. } => unreachable!("the checker keeps lists out of GPU code"),
            _ => break,
        }
    }
    fields.reverse();
    (current, fields)
}

/// How the code of a block uses each local, by local id.
fn local_paths(body: &TBlock) -> HashMap<usize, Paths> {
    let mut paths: HashMap<usize, Paths> = HashMap::new();
    analysis::walk_block(body, &mut |node| {
        let Node::Place(place, how) = node else {
            return;
        };
        let (root, fields) = chain(place);
        let PlaceKind::Local(id) = root.kind else {
            return;
        };
        let path: Vec<usize> = fields.iter().map(|(_, index)| *index).collect();
        let entry = paths.entry(id.0).or_default();
        if how != Use::Write {
            entry.read.push(path.clone());
        }
        if how != Use::Read {
            entry.write.push(path);
        }
    });
    paths
}

/// What encloses a statement, for `break` and `next`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Looping {
    /// The run of a block: `next` ends it.
    Run,
    /// A loop of the source.
    Loop,
}

/// What a block's body needs to know about the block.
struct InBlock<'a> {
    paths: HashMap<usize, Paths>,
    inner_query: Option<&'a Query>,
    inner: Vec<Column>,
}

/// A value: an expression without effects, and whether an assignment to
/// a local can change what it gives.
struct Val {
    e: Expr,
    ty: Ty,
    fixed: bool,
}

impl Val {
    fn literal(e: Expr, ty: Ty) -> Val {
        Val { e, ty, fixed: true }
    }

    fn void() -> Val {
        Val {
            e: Expr::Bool(false),
            ty: Ty::Void,
            fixed: true,
        }
    }
}

struct BodyCx<'l, 'a> {
    lo: &'l mut Lower<'a>,
    locals: &'a [Local],
    /// The name of each local.
    names: Vec<String>,
    usage: LocalUse,
    out: Vec<Stmt>,
    counter: usize,
    loops: Vec<Looping>,
    /// Some `next` ends the run.
    run_left: bool,
    block: Option<InBlock<'a>>,
}

impl<'a> Lower<'a> {
    fn layouts(&self) -> Layouts<'_> {
        Layouts {
            prog: self.prog,
            wide: &self.wide,
        }
    }

    fn site(&mut self, span: Span, message: &str) -> Expr {
        self.sites.push(Site {
            span,
            message: message.to_string(),
        });
        Expr::Site(self.sites.len() - 1)
    }

    /// The name of a function's 32-bit form, which is lowered later if it
    /// has not been asked for before.
    fn user_fn(&mut self, id: FuncId) -> String {
        if !self.wanted[id.0] {
            self.wanted[id.0] = true;
            self.waiting.push(id.0);
        }
        func_name(self.prog, id.0)
    }

    /// The function that compares two values of a record, field by field.
    fn record_eq(&mut self, record: usize) -> String {
        let name = eq_name(self.prog, record);
        if self.library.has(&name) {
            return name;
        }
        let prog = self.prog;
        let mut result = Expr::Bool(true);
        for item in &prog.records[record].fields {
            let member = field_name(&item.name);
            let (a, b) = (field(var("a"), &member), field(var("b"), &member));
            let equal = self.equal(&item.ty.value_type(), a, b);
            result = logic(BinOp::And, result, equal);
        }
        let param = |name: &str| Param {
            name: name.to_string(),
            ty: Ty::Record(record),
            unused: prog.records[record].fields.is_empty(),
        };
        self.library.add(Func {
            name: name.clone(),
            params: vec![param("a"), param("b")],
            ret: Ty::Bool,
            body: vec![Stmt::Return(Some(result))],
        });
        name
    }

    /// `a == b` for two values of a type.
    fn equal(&mut self, ty: &Type, a: Expr, b: Expr) -> Expr {
        match ty {
            Type::Entity => {
                helpers::require(&mut self.library, helpers::EQ_ENTITY);
                call(helpers::EQ_ENTITY, vec![a, b])
            }
            Type::Record(record) => {
                let name = self.record_eq(record.0);
                call(&name, vec![a, b])
            }
            _ => compare(BinOp::Eq, a, b),
        }
    }

    /// The 32-bit form of a function.
    fn function(&mut self, id: usize) -> Func {
        let prog = self.prog;
        let func = &prog.funcs[id];
        let ret = value_ty(&func.ret);
        let mut cx = BodyCx::new(self, &func.body);
        cx.stmts(&func.body.block.stmts);
        if let Some(value) = &func.body.block.value {
            let value = cx.expr(value);
            if ret == Ty::Void {
                cx.discard(value);
            } else {
                cx.out.push(Stmt::Return(Some(value.e)));
            }
        }
        // A shader compiler does not see that a loop without an exit
        // never falls through.
        if ret != Ty::Void && !matches!(cx.out.last(), Some(Stmt::Return(_))) {
            cx.out.push(Stmt::Return(Some(zero(prog, ret))));
        }
        let params = func
            .params
            .iter()
            .map(|param| Param {
                name: cx.names[param.0].clone(),
                ty: value_ty(&func.body.locals[param.0].ty),
                unused: !cx.usage.read[param.0],
            })
            .collect();
        Func {
            name: func_name(prog, id),
            params,
            ret,
            body: cx.out,
        }
    }

    /// Lowers one `each` of a GPU system.
    fn draft(&mut self, system: &'a System, each: u32, stmt: &'a TStmt) -> Draft<'a> {
        let StmtKind::Each {
            query,
            params,
            body,
        } = &stmt.kind
        else {
            unreachable!("the checker lets the body of a GPU system hold only `each` blocks");
        };
        let mut cx = BodyCx::new(self, &system.body);
        cx.block = Some(InBlock {
            paths: local_paths(body),
            inner_query: None,
            inner: Vec::new(),
        });
        let mut outer = Vec::new();
        let (prologue, epilogue) = cx.views(query, params, false, &mut outer);
        cx.loops.push(Looping::Run);
        cx.block_stmts(body);
        let mut body = std::mem::take(&mut cx.out);
        if cx.run_left {
            body = vec![Stmt::Once(body)];
        }
        let block = cx.block.take().expect("set above");
        Draft {
            system: &system.name,
            span: stmt.span,
            each,
            outer_query: query,
            inner_query: block.inner_query,
            outer,
            inner: block.inner,
            prologue,
            body,
            epilogue,
        }
    }

    /// Completes a block once every function it can reach is in the
    /// library.
    fn finish(&mut self, id: u32, draft: Draft<'a>) -> Block<'a> {
        let mut outer = draft.outer;
        let mut body = draft.prologue;
        let reached = closure(&self.library, &Uses::of_stmts(&draft.body));
        if reached.rand_state {
            // The run draws random numbers, which depend on its entity.
            let column = match outer.iter().position(|column| column.component.is_none()) {
                Some(column) => column,
                None => {
                    outer.push(Column {
                        component: None,
                        write: false,
                    });
                    outer.len() - 1
                }
            };
            helpers::require(&mut self.library, helpers::KEY);
            let me = "me";
            body.push(let_(me, Ty::Entity, Expr::Load(entity_cell(false, column))));
            body.push(assign(
                Expr::Key,
                call(
                    helpers::KEY,
                    vec![field(var(me), "index"), field(var(me), "generation")],
                ),
            ));
            body.push(assign(Expr::Draws, Expr::U32(0)));
        }
        body.extend(draft.body);
        body.extend(draft.epilogue);

        let uses = closure(&self.library, &Uses::of_stmts(&body));
        let mut entries: Vec<usize> = uses
            .uniforms
            .iter()
            .filter_map(|&word| self.uniforms.entry_of(word))
            .collect();
        entries.dedup();
        let uniform_words = entries
            .last()
            .map(|&entry| {
                let entry = &self.uniforms.entries[entry];
                entry.base + entry.words
            })
            .unwrap_or(HEADER.len() as u32);
        Block {
            system: draft.system,
            span: draft.span,
            each: draft.each,
            id,
            outer_query: draft.outer_query,
            inner_query: draft.inner_query,
            outer,
            inner: draft.inner,
            body,
            uses,
            entries,
            uniform_words,
        }
    }
}

/// The handle of a row.
fn entity_cell(inner: bool, column: usize) -> Cell {
    Cell {
        inner,
        column,
        member: String::new(),
        offset: 0,
        row_size: 8,
        storage: Storage::Entity,
    }
}

impl<'l, 'a> BodyCx<'l, 'a> {
    fn new(lo: &'l mut Lower<'a>, body: &'a Body) -> BodyCx<'l, 'a> {
        let names = body
            .locals
            .iter()
            .enumerate()
            .map(|(id, local)| names::local(&local.name, id))
            .collect();
        BodyCx {
            lo,
            locals: &body.locals,
            names,
            usage: analysis::local_use(body),
            out: Vec::new(),
            counter: 0,
            loops: Vec::new(),
            run_left: false,
            block: None,
        }
    }

    fn fresh(&mut self, prefix: &str) -> String {
        self.counter += 1;
        format!("{prefix}{}", self.counter - 1)
    }

    /// Runs `f` with an empty statement list and returns what it wrote.
    fn capture<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> (Vec<Stmt>, R) {
        let saved = std::mem::take(&mut self.out);
        let result = f(self);
        (std::mem::replace(&mut self.out, saved), result)
    }

    /// Copies a value into a new variable.
    fn temp(&mut self, value: Val) -> Val {
        let name = self.fresh("t");
        self.out.push(let_(&name, value.ty, value.e));
        Val {
            e: var(&name),
            ty: value.ty,
            fixed: true,
        }
    }

    /// Makes a value cheap and safe to use more than once.
    fn once(&mut self, value: Val) -> Val {
        if value.e.is_literal() || value.e.is_path() {
            value
        } else {
            self.temp(value)
        }
    }

    /// A call of a helper, which has no effect that depends on order.
    fn helper(&mut self, name: &str, args: Vec<Val>, ty: Ty) -> Val {
        helpers::require(&mut self.lo.library, name);
        Val {
            fixed: args.iter().all(|arg| arg.fixed),
            e: call(name, args.into_iter().map(|arg| arg.e).collect()),
            ty,
        }
    }

    /// A call of one of the targets' own functions. A shader compiler
    /// works out a call whose arguments are all literals and rejects a
    /// result that is not finite, so one of them is put in a variable.
    fn math(&mut self, function: MathFn, mut args: Vec<Val>) -> Val {
        if args.iter().all(|arg| arg.e.is_literal()) {
            let first = args.remove(0);
            let first = self.temp(first);
            args.insert(0, first);
        }
        Val {
            fixed: args.iter().all(|arg| arg.fixed),
            e: Expr::Math(function, args.into_iter().map(|arg| arg.e).collect()),
            ty: Ty::F32,
        }
    }

    /// Declares the variables that stand for the parameters of an `each`
    /// and returns the statements that load them and those that store
    /// what the block assigns. Adds the columns they come from.
    fn views(
        &mut self,
        query: &'a Query,
        params: &'a [LocalId],
        inner: bool,
        columns: &mut Vec<Column>,
    ) -> (Vec<Stmt>, Vec<Stmt>) {
        let prog = self.lo.prog;
        let mut loads = Vec::new();
        let mut stores = Vec::new();
        for (term, &param) in query.terms.iter().zip(params) {
            let name = self.names[param.0].clone();
            match term {
                Term::Entity => {
                    if !self.usage.touched[param.0] {
                        continue;
                    }
                    let column = match columns.iter().position(|column| column.component.is_none())
                    {
                        Some(column) => column,
                        None => {
                            columns.push(Column {
                                component: None,
                                write: false,
                            });
                            columns.len() - 1
                        }
                    };
                    loads.push(Stmt::Let {
                        name,
                        ty: Ty::Entity,
                        init: Some(Expr::Load(entity_cell(inner, column))),
                        unused: !self.usage.read[param.0],
                    });
                }
                Term::Component {
                    component,
                    write,
                    touched,
                } => {
                    if !touched {
                        continue;
                    }
                    let block = self.block.as_ref().expect("an `each` is in a block");
                    let paths = &block.paths[&param.0];
                    assert!(
                        !inner || !*write,
                        "the checker rejects a write to a parameter of a nested `each`"
                    );
                    let record = prog.components[component.0]
                        .record
                        .expect("a tag is not a query term")
                        .0;
                    let leaves: Vec<Leaf> = self.lo.layouts().leaves(record);
                    let row_size = self.lo.layouts().record(record).size;
                    let column = columns.len();
                    columns.push(Column {
                        component: Some(component.0),
                        write: *write,
                    });
                    loads.push(Stmt::Let {
                        name: name.clone(),
                        ty: Ty::Record(record),
                        init: None,
                        unused: false,
                    });
                    for leaf in leaves {
                        let place = leaf
                            .fields
                            .iter()
                            .fold(var(&name), |base, member| field(base, member));
                        let cell = Cell {
                            inner,
                            column,
                            member: leaf.member,
                            offset: leaf.offset,
                            row_size,
                            storage: leaf.storage,
                        };
                        let read = Paths::covers(&paths.read, &leaf.path);
                        let written = Paths::covers(&paths.write, &leaf.path);
                        // A field the block assigns is loaded too: a run that
                        // does not reach the assignment stores what was there.
                        if read || written {
                            loads.push(assign(place.clone(), Expr::Load(cell.clone())));
                        }
                        if written {
                            assert!(*write, "a write through a term that is not marked written");
                            stores.push(Stmt::Store(cell, place));
                        }
                    }
                }
            }
        }
        (loads, stores)
    }

    fn stmts(&mut self, stmts: &'a [TStmt]) {
        for stmt in stmts {
            self.stmt(stmt);
        }
    }

    /// The statements of a block, and its value if it has one, unused.
    fn block_stmts(&mut self, block: &'a TBlock) {
        self.stmts(&block.stmts);
        if let Some(value) = &block.value {
            let value = self.expr(value);
            self.discard(value);
        }
    }

    /// The statements of a block, then its value.
    fn block_value(&mut self, block: &'a TBlock) -> Val {
        self.stmts(&block.stmts);
        let value = block
            .value
            .as_ref()
            .expect("a block used for its value has one");
        self.expr(value)
    }

    fn loop_body(&mut self, body: &'a TBlock) -> Vec<Stmt> {
        self.loops.push(Looping::Loop);
        let (stmts, ()) = self.capture(|cx| cx.block_stmts(body));
        self.loops.pop();
        stmts
    }

    /// Ends the evaluation of an expression whose value is not used. A
    /// temporary of the lowering's own, set from a call by the statement
    /// before, becomes the bare call.
    fn discard(&mut self, value: Val) {
        if value.ty == Ty::Void || value.e.is_literal() {
            return;
        }
        if let Expr::Var(name) = &value.e
            && !self.names.contains(name)
            && let Some(Stmt::Let {
                name: last,
                init: Some(Expr::Call(..)),
                ..
            }) = self.out.last()
            && last == name
        {
            let Some(Stmt::Let {
                init: Some(call), ..
            }) = self.out.pop()
            else {
                unreachable!("matched above");
            };
            self.out.push(Stmt::Call(call));
            return;
        }
        self.out.push(Stmt::Discard(value.e));
    }

    /// An `if` statement. With nothing in either branch, only the
    /// condition is evaluated.
    fn push_if(&mut self, cond: Expr, then: Vec<Stmt>, else_: Vec<Stmt>) {
        if then.is_empty() && else_.is_empty() {
            if !cond.is_literal() {
                self.out.push(Stmt::Discard(cond));
            }
            return;
        }
        if then.is_empty() {
            self.out.push(Stmt::If(not(cond), else_, Vec::new()));
        } else {
            self.out.push(Stmt::If(cond, then, else_));
        }
    }

    fn stmt(&mut self, stmt: &'a TStmt) {
        match &stmt.kind {
            StmtKind::Expr(expr) => {
                let value = self.expr(expr);
                self.discard(value);
            }
            StmtKind::Let { local, init } => {
                let value = self.expr(init);
                self.out.push(Stmt::Let {
                    name: self.names[local.0].clone(),
                    ty: value_ty(&self.locals[local.0].ty),
                    init: Some(value.e),
                    unused: !self.usage.read[local.0],
                });
            }
            StmtKind::Assign { place, value } => {
                let evaluated = self.expr(value);
                let stored = self.narrow(evaluated, &place.ty, value.span, Some(value));
                let target = self.lvalue(place);
                self.out.push(assign(target, stored));
            }
            StmtKind::CompoundAssign { place, op, value } => {
                let target = self.lvalue(place);
                let value_type = place.ty.value_type();
                let mut current = Val {
                    e: target.clone(),
                    ty: value_ty(&value_type),
                    fixed: false,
                };
                // The place is read before the value is evaluated.
                if !analysis::local_safe(value) {
                    current = self.temp(current);
                }
                let operand = self.expr(value);
                match op {
                    CompoundOp::Builtin(op) => {
                        let result = self.binary(*op, &value_type, current, operand, stmt.span);
                        let stored = self.narrow(result, &place.ty, stmt.span, None);
                        self.out.push(assign(target, stored));
                    }
                    CompoundOp::Method(func) => {
                        let name = self.lo.user_fn(*func);
                        self.out
                            .push(assign(target, call(&name, vec![current.e, operand.e])));
                    }
                }
            }
            StmtKind::Return(value) => {
                assert!(
                    self.block.is_none(),
                    "the checker rejects `return` in a GPU block"
                );
                let value = value.as_ref().map(|value| self.expr(value).e);
                self.out.push(Stmt::Return(value));
            }
            StmtKind::Break => match self.loops.last() {
                Some(Looping::Loop) => self.out.push(Stmt::Break),
                _ => unreachable!("the checker rejects `break` directly inside a GPU block"),
            },
            StmtKind::Next => match self.loops.last() {
                Some(Looping::Loop) => self.out.push(Stmt::Continue),
                Some(Looping::Run) => {
                    self.run_left = true;
                    self.out.push(Stmt::Break);
                }
                None => unreachable!("`next` outside a loop"),
            },
            StmtKind::While { cond, body } => {
                let (mut before, cond) = self.capture(|cx| cx.expr(cond));
                let body = self.loop_body(body);
                if !before.is_empty() {
                    before.push(if_(not(cond.e), vec![Stmt::Break]));
                    before.extend(body);
                    self.out.push(Stmt::Loop(before));
                } else if cond.e == Expr::Bool(true) {
                    self.out.push(Stmt::Loop(body));
                } else {
                    self.out.push(Stmt::While(cond.e, body));
                }
            }
            StmtKind::Count {
                var: counter,
                from,
                to,
                inclusive,
                down,
                body,
            } => {
                let mut values = self.operands(&[from, to]);
                let to = values.pop().expect("two operands");
                let from = values.pop().expect("two operands");
                // The bound is evaluated once.
                let to = if to.e.is_literal() { to } else { self.temp(to) };
                let name = match counter {
                    Some(id) if self.usage.read[id.0] => self.names[id.0].clone(),
                    _ => self.fresh("k"),
                };
                let op = match (down, inclusive) {
                    (false, false) => BinOp::Lt,
                    (false, true) => BinOp::Le,
                    (true, false) => BinOp::Gt,
                    (true, true) => BinOp::Ge,
                };
                // A bound at the end of `i32` needs the counter to stop on it.
                let end = if *down { i32::MIN } else { i32::MAX };
                let guard = (*inclusive && !matches!(to.e, Expr::I32(n) if n != end))
                    .then(|| self.fresh("w"));
                let body = self.loop_body(body);
                self.out.push(Stmt::For {
                    var: name,
                    ty: Ty::I32,
                    from: from.e,
                    op,
                    to: to.e,
                    down: *down,
                    guard,
                    body,
                });
            }
            StmtKind::Each {
                query,
                params,
                body,
            } => self.nested_each(query, params, body),
            StmtKind::ListEach { .. }
            | StmtKind::On { .. }
            | StmtKind::Puts(_)
            | StmtKind::Print(_)
            | StmtKind::Emit { .. } => {
                unreachable!("the checker keeps lists, events and output out of GPU code")
            }
        }
    }

    /// The `each` nested in a block: a loop over the rows of the
    /// snapshot.
    fn nested_each(&mut self, query: &'a Query, params: &'a [LocalId], body: &'a TBlock) {
        let directly = self.loops.as_slice() == [Looping::Run];
        let first = self
            .block
            .as_ref()
            .is_some_and(|block| block.inner_query.is_none());
        assert!(
            directly && first,
            "the checker allows one nested `each`, directly inside a GPU block"
        );
        let mut columns = Vec::new();
        let (mut stmts, stores) = self.views(query, params, true, &mut columns);
        assert!(stores.is_empty(), "the nested `each` reads a snapshot");
        let block = self.block.as_mut().expect("checked above");
        block.inner_query = Some(query);
        block.inner = columns;
        stmts.extend(self.loop_body(body));
        self.out.push(Stmt::For {
            var: INNER_ROW.to_string(),
            ty: Ty::U32,
            from: Expr::U32(0),
            op: BinOp::Lt,
            to: Expr::Uniform(WORD_SNAPSHOT_ROWS, Ty::U32),
            down: false,
            guard: None,
            body: stmts,
        });
    }

    /// A variable or a member of one, to assign to.
    fn lvalue(&mut self, place: &'a Place) -> Expr {
        let (root, fields) = chain(place);
        let PlaceKind::Local(id) = root.kind else {
            unreachable!(
                "the checker lets GPU code assign only to locals and to the parameters of the \
                 outer `each`"
            );
        };
        self.member(var(&self.names[id.0]), &fields)
    }

    fn member(&self, base: Expr, fields: &[(usize, usize)]) -> Expr {
        fields.iter().fold(base, |base, &(record, index)| {
            let name = &self.lo.prog.records[record].fields[index].name;
            field(base, &field_name(name))
        })
    }

    /// A record or entity value as a variable, so that its members can
    /// be named.
    fn settle(&mut self, value: Val) -> Val {
        match value.ty {
            Ty::Entity | Ty::Record(_) if !value.e.is_path() => self.temp(value),
            _ => value,
        }
    }

    fn read(&mut self, place: &'a Place) -> Val {
        let prog = self.lo.prog;
        let ty = value_ty(&place.ty);
        let (root, fields) = chain(place);
        let path: Vec<usize> = fields.iter().map(|(_, index)| *index).collect();
        match &root.kind {
            PlaceKind::Local(id) => Val {
                e: self.member(var(&self.names[id.0]), &fields),
                ty,
                fixed: self.locals[id.0].kind == LocalKind::Param,
            },
            PlaceKind::Const(id) => {
                if let Some(value) = self.lo.folded[id.0] {
                    let e = match value {
                        Folded::Int(value) => Expr::I32(i32::try_from(value).expect(
                            "the checker rejects a known constant that does not fit in 32 bits",
                        )),
                        Folded::Float(value) => float(value as f32),
                        Folded::Bool(value) => Expr::Bool(value),
                    };
                    return Val::literal(e, ty);
                }
                let item = &prog.consts[id.0];
                let e = self
                    .lo
                    .uniforms
                    .read(prog, Source::Const(id.0), &item.ty, &path);
                self.settle(Val { e, ty, fixed: true })
            }
            PlaceKind::ResourceField(resource, index) => {
                let item = &prog.resources[resource.0].fields[*index];
                let source = Source::Resource(resource.0, *index);
                let e = self.lo.uniforms.read(prog, source, &item.ty, &path);
                self.settle(Val { e, ty, fixed: true })
            }
            PlaceKind::Time(field) => {
                let word = match field {
                    TimeField::Tick => WORD_TICK,
                    TimeField::Dt => WORD_DT,
                    TimeField::Elapsed => WORD_ELAPSED,
                };
                Val::literal(Expr::Uniform(word, ty), ty)
            }
            PlaceKind::Component { .. } => {
                unreachable!("the checker rejects `e[C]` in GPU code")
            }
            PlaceKind::Field { .. } | PlaceKind::Index { .. } => {
                unreachable!("the chain starts at its root")
            }
        }
    }

    /// Converts a value for a field of type `storage`. A store into a
    /// narrow integer field is checked unless the value is known to fit.
    fn narrow(&mut self, value: Val, storage: &Type, span: Span, source: Option<&TExpr>) -> Expr {
        let Type::Compact(compact) = storage else {
            return value.e;
        };
        let helper = match compact {
            Compact::Int8 => helpers::TO_INT8,
            Compact::Int16 => helpers::TO_INT16,
            Compact::UInt8 => helpers::TO_UINT8,
            Compact::UInt16 => helpers::TO_UINT16,
            // Every 32-bit `Int` fits in 32 bits.
            Compact::Float32 | Compact::Int32 => return value.e,
            Compact::UInt32 => unreachable!("the checker rejects a store into a `UInt32` field"),
        };
        let (low, high) = compact.int_range().expect("an integer type");
        let fits = match (&value.e, source) {
            (Expr::I32(literal), _) => (low..=high).contains(&i64::from(*literal)),
            (_, Some(source)) => match &source.kind {
                ExprKind::Read(Place {
                    ty: Type::Compact(from),
                    ..
                }) if !matches!(from, Compact::Int32 | Compact::UInt32) => from
                    .int_range()
                    .is_some_and(|(from_low, from_high)| low <= from_low && from_high <= high),
                _ => false,
            },
            _ => false,
        };
        if fits {
            return value.e;
        }
        let message = format!("value does not fit in {}", compact.name());
        let site = self.lo.site(span, &message);
        let site = Val::literal(site, Ty::U32);
        self.helper(helper, vec![value, site], Ty::I32).e
    }

    /// Evaluates operands left to right. One that a later operand could
    /// change, by assigning to a local in a statement of its own, is
    /// copied first.
    fn operands(&mut self, exprs: &[&'a TExpr]) -> Vec<Val> {
        let count = exprs.len();
        let mut safe_after = vec![true; count];
        for index in (0..count.saturating_sub(1)).rev() {
            safe_after[index] = safe_after[index + 1] && analysis::local_safe(exprs[index + 1]);
        }
        let mut values = Vec::with_capacity(count);
        for (index, &expr) in exprs.iter().enumerate() {
            let mut value = self.expr(expr);
            if !safe_after[index] && !value.fixed {
                value = self.temp(value);
            }
            values.push(value);
        }
        values
    }

    fn expr(&mut self, expr: &'a TExpr) -> Val {
        let prog = self.lo.prog;
        let ty = value_ty(&expr.ty);
        match &expr.kind {
            ExprKind::Int(value) => Val::literal(Expr::I32(*value as i32), ty),
            ExprKind::Float(value) => Val::literal(float(*value as f32), ty),
            ExprKind::Bool(value) => Val::literal(Expr::Bool(*value), ty),
            ExprKind::EnumVariant(index) => Val::literal(Expr::U32(*index as u32), ty),
            ExprKind::EntityNone => self.temp(Val::literal(zero(prog, Ty::Entity), ty)),
            ExprKind::Read(place) => self.read(place),
            ExprKind::Field { base, field: index } => {
                let Type::Record(record) = &base.ty else {
                    unreachable!("a field of something that is not a record");
                };
                let value = self.expr(base);
                let value = self.settle(value);
                Val {
                    e: self.member(value.e, &[(record.0, *index)]),
                    ty,
                    fixed: value.fixed,
                }
            }
            ExprKind::Construct(fields) => {
                let Type::Record(record) = &expr.ty else {
                    unreachable!("a constructor of something that is not a record");
                };
                let exprs: Vec<&'a TExpr> = fields.iter().collect();
                let values = self.operands(&exprs);
                let mut inits = Vec::with_capacity(fields.len());
                for ((value, init), item) in values
                    .into_iter()
                    .zip(fields)
                    .zip(&prog.records[record.0].fields)
                {
                    inits.push(self.narrow(value, &item.ty, init.span, Some(init)));
                }
                if inits.is_empty() {
                    inits.push(Expr::U32(0));
                }
                self.temp(Val::literal(Expr::Construct(ty, inits), ty))
            }
            ExprKind::Unary { op, expr: inner } => {
                let value = self.expr(inner);
                let e = match (op, value.e) {
                    (TUnOp::Neg, Expr::I32(literal)) => Expr::I32(literal.wrapping_neg()),
                    (TUnOp::Neg, e) => match e.as_f32() {
                        Some(literal) => float(-literal),
                        None => unary(UnOp::Neg, e),
                    },
                    (TUnOp::Not, e) => not(e),
                };
                Val {
                    e,
                    ty,
                    fixed: value.fixed,
                }
            }
            ExprKind::Binary { op, lhs, rhs } => {
                let mut values = self.operands(&[&**lhs, &**rhs]);
                let right = values.pop().expect("two operands");
                let left = values.pop().expect("two operands");
                self.binary(*op, &lhs.ty, left, right, expr.span)
            }
            ExprKind::And { lhs, rhs } => self.short_circuit(lhs, rhs, true),
            ExprKind::Or { lhs, rhs } => self.short_circuit(lhs, rhs, false),
            // An `Int` literal made `Float` need not fit in 32 bits.
            ExprKind::IntToFloat(inner) if let ExprKind::Int(literal) = inner.kind => {
                Val::literal(float(literal as f64 as f32), ty)
            }
            ExprKind::IntToFloat(inner) => {
                let value = self.expr(inner);
                let e = match value.e {
                    Expr::I32(literal) => float(literal as f32),
                    e => Expr::Cast(Ty::F32, Box::new(e)),
                };
                Val {
                    e,
                    ty,
                    fixed: value.fixed,
                }
            }
            ExprKind::Call { func, args } => {
                let exprs: Vec<&'a TExpr> = args.iter().collect();
                let values = self.operands(&exprs);
                let name = self.lo.user_fn(*func);
                let call = call(&name, values.into_iter().map(|value| value.e).collect());
                if ty == Ty::Void {
                    self.out.push(Stmt::Call(call));
                    Val::void()
                } else {
                    // A call may draw random numbers: it runs here, in
                    // the order of the source.
                    self.temp(Val::literal(call, ty))
                }
            }
            ExprKind::Builtin { op, args } => self.builtin(expr, *op, args),
            ExprKind::If { cond, then, else_ } => {
                let cond = self.expr(cond);
                if ty == Ty::Void {
                    let (then, ()) = self.capture(|cx| cx.block_stmts(then));
                    let (else_, ()) = self.capture(|cx| cx.block_stmts(else_));
                    self.push_if(cond.e, then, else_);
                    return Val::void();
                }
                let (mut then, first) = self.capture(|cx| cx.block_value(then));
                let (mut else_, second) = self.capture(|cx| cx.block_value(else_));
                // A select evaluates both sides, so it only stands for an
                // `if` whose sides need no statements and can't fail.
                let scalar = !matches!(ty, Ty::Entity | Ty::Record(_));
                let eager = !first.e.can_fail() && !second.e.can_fail();
                if scalar && eager && then.is_empty() && else_.is_empty() {
                    return Val {
                        fixed: cond.fixed && first.fixed && second.fixed,
                        e: choose(cond.e, first.e, second.e),
                        ty,
                    };
                }
                let name = self.fresh("t");
                self.out.push(Stmt::Let {
                    name: name.clone(),
                    ty,
                    init: None,
                    unused: false,
                });
                then.push(assign(var(&name), first.e));
                else_.push(assign(var(&name), second.e));
                self.out.push(Stmt::If(cond.e, then, else_));
                Val::literal(var(&name), ty)
            }
            ExprKind::Block(block) => {
                self.stmts(&block.stmts);
                match &block.value {
                    Some(value) => self.expr(value),
                    None => Val::void(),
                }
            }
            ExprKind::Str(_)
            | ExprKind::Index { .. }
            | ExprKind::ListLit(_)
            | ExprKind::Concat(_)
            | ExprKind::Spawn(_)
            | ExprKind::Add { .. }
            | ExprKind::Remove { .. }
            | ExprKind::Despawn(_)
            | ExprKind::Has { .. }
            | ExprKind::Alive(_)
            | ExprKind::ListPush { .. }
            | ExprKind::ListPop { .. }
            | ExprKind::ListDeleteAt { .. }
            | ExprKind::ListClear { .. }
            | ExprKind::WorldTick
            | ExprKind::WorldRun
            | ExprKind::WorldStop
            | ExprKind::WorldCount(_) => unreachable!(
                "the checker keeps strings, lists, structural changes and `World` out of GPU code"
            ),
        }
    }

    /// `&&` (with `and`) and `||`. The right operand is lowered apart:
    /// when it needs statements of its own or can fail, it is evaluated
    /// under an `if`, only when the left operand does not decide the
    /// result. Otherwise both targets may evaluate it either way.
    fn short_circuit(&mut self, lhs: &'a TExpr, rhs: &'a TExpr, and: bool) -> Val {
        let left = self.expr(lhs);
        let (stmts, right) = self.capture(|cx| cx.expr(rhs));
        let op = if and { BinOp::And } else { BinOp::Or };
        if stmts.is_empty() && !right.e.can_fail() {
            return Val {
                fixed: left.fixed && right.fixed,
                e: logic(op, left.e, right.e),
                ty: Ty::Bool,
            };
        }
        let result = self.temp(left);
        let mut then = stmts;
        then.push(assign(result.e.clone(), right.e));
        let cond = if and {
            result.e.clone()
        } else {
            not(result.e.clone())
        };
        self.out.push(if_(cond, then));
        result
    }

    /// A built-in binary operator on two evaluated operands. `ty` is the
    /// type of the left one.
    fn binary(&mut self, op: TBinOp, ty: &Type, left: Val, right: Val, span: Span) -> Val {
        let fixed = left.fixed && right.fixed;
        let simple = |op: BinOp, ty: Ty, left: Val, right: Val| {
            let e = match ty {
                Ty::I32 => int_arith(op, left.e, right.e),
                _ => float_arith(op, left.e, right.e),
            };
            Val { e, ty, fixed }
        };
        match op {
            TBinOp::Eq | TBinOp::Ne => {
                let (left, right) = (self.settle(left), self.settle(right));
                let e = match (op, ty) {
                    (TBinOp::Eq, _) => self.lo.equal(ty, left.e, right.e),
                    (_, Type::Entity | Type::Record(_)) => not(self.lo.equal(ty, left.e, right.e)),
                    _ => compare(BinOp::Ne, left.e, right.e),
                };
                Val {
                    e,
                    ty: Ty::Bool,
                    fixed,
                }
            }
            TBinOp::Lt | TBinOp::Le | TBinOp::Gt | TBinOp::Ge => {
                let op = match op {
                    TBinOp::Lt => BinOp::Lt,
                    TBinOp::Le => BinOp::Le,
                    TBinOp::Gt => BinOp::Gt,
                    _ => BinOp::Ge,
                };
                Val {
                    e: compare(op, left.e, right.e),
                    ty: Ty::Bool,
                    fixed,
                }
            }
            TBinOp::Add => simple(BinOp::Add, value_ty(ty), left, right),
            TBinOp::Sub => simple(BinOp::Sub, value_ty(ty), left, right),
            TBinOp::Mul => simple(BinOp::Mul, value_ty(ty), left, right),
            TBinOp::Div | TBinOp::Mod | TBinOp::Pow => match ty {
                Type::Int => {
                    let (plain, checked, message) = match op {
                        TBinOp::Div => (helpers::DIV, helpers::DIV_CHECKED, "division by zero"),
                        TBinOp::Mod => (helpers::MOD, helpers::MOD_CHECKED, "modulo by zero"),
                        _ => (helpers::POW, helpers::POW_CHECKED, "negative exponent"),
                    };
                    // A constant on the right that can't fail needs no
                    // check.
                    let safe = match right.e {
                        Expr::I32(literal) if op == TBinOp::Pow => literal >= 0,
                        Expr::I32(literal) => literal != 0,
                        _ => false,
                    };
                    if safe
                        && let (Expr::I32(a), Expr::I32(b)) = (&left.e, &right.e)
                        && let Some(value) = analysis::fold_int(op, i64::from(*a), i64::from(*b))
                    {
                        // The low 32 bits of the 64-bit result are the
                        // 32-bit result.
                        Val::literal(Expr::I32(value as i32), Ty::I32)
                    } else if safe {
                        self.helper(plain, vec![left, right], Ty::I32)
                    } else {
                        let site = self.lo.site(span, message);
                        let site = Val::literal(site, Ty::U32);
                        self.helper(checked, vec![left, right, site], Ty::I32)
                    }
                }
                Type::Float => match op {
                    TBinOp::Div => simple(BinOp::Div, Ty::F32, left, right),
                    TBinOp::Mod => self.helper(helpers::FMOD, vec![left, right], Ty::F32),
                    _ => self.helper(helpers::FPOW, vec![left, right], Ty::F32),
                },
                _ => unreachable!("arithmetic on a type that has none in GPU code"),
            },
        }
    }

    fn builtin(&mut self, expr: &'a TExpr, op: Builtin, args: &'a [TExpr]) -> Val {
        let exprs: Vec<&'a TExpr> = args.iter().collect();
        let mut values = self.operands(&exprs);
        let fixed = values.iter().all(|value| value.fixed);
        let ty = value_ty(&expr.ty);
        match op {
            Builtin::IntAbs => {
                let value = self.once(values.remove(0));
                let e = match value.e {
                    Expr::I32(literal) => Expr::I32(literal.wrapping_abs()),
                    e => choose(
                        compare(BinOp::Lt, e.clone(), Expr::I32(0)),
                        unary(UnOp::Neg, e.clone()),
                        e,
                    ),
                };
                Val { e, ty, fixed }
            }
            Builtin::IntClamp | Builtin::FloatClamp => {
                let high = self.once(values.remove(2)).e;
                let low = self.once(values.remove(1)).e;
                let value = self.once(values.remove(0)).e;
                let above = choose(
                    compare(BinOp::Gt, value.clone(), high.clone()),
                    high,
                    value.clone(),
                );
                let e = choose(compare(BinOp::Lt, value, low.clone()), low, above);
                Val { e, ty, fixed }
            }
            Builtin::IntMin | Builtin::FloatMin | Builtin::IntMax | Builtin::FloatMax => {
                let op = if matches!(op, Builtin::IntMin | Builtin::FloatMin) {
                    BinOp::Lt
                } else {
                    BinOp::Gt
                };
                let right = self.once(values.remove(1)).e;
                let left = self.once(values.remove(0)).e;
                let e = choose(compare(op, left.clone(), right.clone()), left, right);
                Val { e, ty, fixed }
            }
            Builtin::FloatAbs => self.math(MathFn::Abs, values),
            Builtin::FloatToI => self.helper(helpers::TO_I, values, ty),
            Builtin::FloatFloor => self.helper(helpers::FLOOR, values, ty),
            Builtin::FloatCeil => self.helper(helpers::CEIL, values, ty),
            Builtin::FloatRound => self.helper(helpers::ROUND, values, ty),
            Builtin::FloatRoundDigits => self.helper(helpers::ROUND_DIGITS, values, ty),
            Builtin::FloatNan => Val {
                e: helpers::is_nan(values.remove(0).e),
                ty,
                fixed,
            },
            Builtin::Sqrt => self.math(MathFn::Sqrt, values),
            Builtin::Sin => self.math(MathFn::Sin, values),
            Builtin::Cos => self.math(MathFn::Cos, values),
            Builtin::Tan => self.math(MathFn::Tan, values),
            Builtin::Atan2 => self.math(MathFn::Atan2, values),
            Builtin::Exp => self.math(MathFn::Exp, values),
            Builtin::Log => self.math(MathFn::Log, values),
            // A draw counts: it runs here, in the order of the source.
            Builtin::Rand => {
                let value = self.helper(helpers::RAND, values, ty);
                self.temp(value)
            }
            Builtin::RandInt => {
                let value = match values[0].e {
                    Expr::I32(literal) if literal > 0 => {
                        self.helper(helpers::RAND_BELOW, values, ty)
                    }
                    _ => {
                        let site = self
                            .lo
                            .site(expr.span, "rand: the argument must be positive");
                        values.push(Val::literal(site, Ty::U32));
                        self.helper(helpers::RAND_INT, values, ty)
                    }
                };
                self.temp(value)
            }
            Builtin::ToS
            | Builtin::StrLength
            | Builtin::StrBytesize
            | Builtin::StrEmpty
            | Builtin::StrUpcase
            | Builtin::StrDowncase
            | Builtin::StrInclude
            | Builtin::StrStartWith
            | Builtin::StrEndWith
            | Builtin::StrLjust
            | Builtin::StrRjust
            | Builtin::StrToI
            | Builtin::StrToF
            | Builtin::ListLength
            | Builtin::ListEmpty
            | Builtin::ListFirst
            | Builtin::ListLast
            | Builtin::ListInclude
            | Builtin::Srand
            | Builtin::SrandClock
            | Builtin::Clock
            | Builtin::Sleep
            | Builtin::Exit => unreachable!(
                "the checker keeps strings, lists, the clock and the seed out of GPU code"
            ),
        }
    }
}
