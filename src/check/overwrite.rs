//! Which query terms are overwritten: the body of the `each` assigns the
//! whole component, in every run, before anything can read it, so the
//! values the column held before are never needed. "The transfer plan" in
//! the reference defines it; [`mark`] sets
//! [`Term::Component::overwritten`](crate::tir::Term) by that definition.
//!
//! A term qualifies when:
//!
//! 1. The body starts with a run of plain statements (declarations of
//!    locals, assignments, calls, output) that assigns the parameter as a
//!    whole, or each field of the component as a whole.
//! 2. No expression in that run, up to the last of those assignments,
//!    reads the parameter. A compound assignment reads, and so does a
//!    store into a list the component holds.
//! 3. In an ordinary `each`, nothing in the whole body can reach the
//!    component another way: no `e[C]`, no query with it as a term whose
//!    parameter is used, no call of a function that has either, directly or
//!    through other functions; and no `break` that leaves this `each` and
//!    no `return`.
//! 4. In a GPU block, the nested `each` does not use the component.

use std::collections::HashMap;

use crate::source::Span;
use crate::tir::{
    Block, CompoundOp, Expr, ExprKind, LocalId, Place, PlaceKind, Program, Stmt, StmtKind, Term,
};

/// A statement or an expression, and how many loops around it lie inside
/// the block the visit started from.
enum Node<'t> {
    Stmt(&'t Stmt),
    Expr(&'t Expr),
}

type Visit<'f> = &'f mut dyn FnMut(Node<'_>, usize);

fn visit_block(b: &Block, depth: usize, f: Visit<'_>) {
    for s in &b.stmts {
        visit_stmt(s, depth, f);
    }
    if let Some(value) = &b.value {
        visit_expr(value, depth, f);
    }
}

fn visit_stmt(s: &Stmt, depth: usize, f: Visit<'_>) {
    f(Node::Stmt(s), depth);
    match &s.kind {
        StmtKind::Expr(e) | StmtKind::Let { init: e, .. } | StmtKind::Emit { value: e, .. } => {
            visit_expr(e, depth, f)
        }
        StmtKind::Assign { place, value } | StmtKind::CompoundAssign { place, value, .. } => {
            visit_place(place, depth, f);
            visit_expr(value, depth, f);
        }
        StmtKind::Return(value) => {
            if let Some(value) = value {
                visit_expr(value, depth, f);
            }
        }
        StmtKind::Break | StmtKind::Next => {}
        StmtKind::While { cond, body } => {
            visit_expr(cond, depth, f);
            visit_block(body, depth + 1, f);
        }
        StmtKind::Count { from, to, body, .. } => {
            visit_expr(from, depth, f);
            visit_expr(to, depth, f);
            visit_block(body, depth + 1, f);
        }
        StmtKind::ListEach { list, body, .. } => {
            visit_expr(list, depth, f);
            visit_block(body, depth + 1, f);
        }
        StmtKind::Each { body, .. } | StmtKind::On { body, .. } => visit_block(body, depth + 1, f),
        StmtKind::Puts(args) | StmtKind::Print(args) => {
            for arg in args {
                visit_expr(arg, depth, f);
            }
        }
    }
}

fn visit_expr(e: &Expr, depth: usize, f: Visit<'_>) {
    f(Node::Expr(e), depth);
    match &e.kind {
        ExprKind::Int(_)
        | ExprKind::Float(_)
        | ExprKind::Bool(_)
        | ExprKind::Str(_)
        | ExprKind::EnumVariant(_)
        | ExprKind::EntityNone
        | ExprKind::WorldTick
        | ExprKind::WorldRun
        | ExprKind::WorldStop
        | ExprKind::WorldCount(_) => {}
        ExprKind::Read(p) | ExprKind::ListPop { list: p } | ExprKind::ListClear { list: p } => {
            visit_place(p, depth, f)
        }
        ExprKind::Field { base, .. } => visit_expr(base, depth, f),
        ExprKind::Index { base, index } => {
            visit_expr(base, depth, f);
            visit_expr(index, depth, f);
        }
        ExprKind::ListLit(items) | ExprKind::Construct(items) | ExprKind::Concat(items) => {
            for item in items {
                visit_expr(item, depth, f);
            }
        }
        ExprKind::Unary { expr: inner, .. }
        | ExprKind::IntToFloat(inner)
        | ExprKind::Despawn(inner)
        | ExprKind::Alive(inner)
        | ExprKind::Remove { entity: inner, .. }
        | ExprKind::Has { entity: inner, .. } => visit_expr(inner, depth, f),
        ExprKind::Binary { lhs, rhs, .. }
        | ExprKind::And { lhs, rhs }
        | ExprKind::Or { lhs, rhs } => {
            visit_expr(lhs, depth, f);
            visit_expr(rhs, depth, f);
        }
        ExprKind::Call { args, .. } | ExprKind::Builtin { args, .. } => {
            for arg in args {
                visit_expr(arg, depth, f);
            }
        }
        ExprKind::If { cond, then, else_ } => {
            visit_expr(cond, depth, f);
            visit_block(then, depth, f);
            visit_block(else_, depth, f);
        }
        ExprKind::Block(b) => visit_block(b, depth, f),
        ExprKind::Spawn(components) => {
            for init in components {
                if let Some(value) = &init.value {
                    visit_expr(value, depth, f);
                }
            }
        }
        ExprKind::Add { entity, components } => {
            visit_expr(entity, depth, f);
            for init in components {
                if let Some(value) = &init.value {
                    visit_expr(value, depth, f);
                }
            }
        }
        ExprKind::ListPush { list, value } => {
            visit_place(list, depth, f);
            visit_expr(value, depth, f);
        }
        ExprKind::ListDeleteAt { list, index } => {
            visit_place(list, depth, f);
            visit_expr(index, depth, f);
        }
    }
}

/// Visits the expressions inside a place: the entity of `e[C]` and the
/// indexes.
fn visit_place(p: &Place, depth: usize, f: Visit<'_>) {
    match &p.kind {
        PlaceKind::Local(_)
        | PlaceKind::Const(_)
        | PlaceKind::ResourceField(..)
        | PlaceKind::Time(_) => {}
        PlaceKind::Component { entity, .. } => visit_expr(entity, depth, f),
        PlaceKind::Field { base, .. } => visit_place(base, depth, f),
        PlaceKind::Index { base, index } => {
            visit_place(base, depth, f);
            visit_expr(index, depth, f);
        }
    }
}

/// The place a chain of fields and indexes starts from.
fn root(place: &Place) -> &Place {
    match &place.kind {
        PlaceKind::Field { base, .. } | PlaceKind::Index { base, .. } => root(base),
        _ => place,
    }
}

fn is_local(place: &Place, local: LocalId) -> bool {
    matches!(root(place).kind, PlaceKind::Local(id) if id == local)
}

/// The place a statement or expression uses in place of a value: read,
/// written or changed in place.
fn place_of<'t>(node: &Node<'t>) -> Option<&'t Place> {
    match node {
        Node::Expr(e) => match &e.kind {
            ExprKind::Read(p)
            | ExprKind::ListPop { list: p }
            | ExprKind::ListClear { list: p }
            | ExprKind::ListPush { list: p, .. }
            | ExprKind::ListDeleteAt { list: p, .. } => Some(p),
            _ => None,
        },
        Node::Stmt(s) => match &s.kind {
            StmtKind::Assign { place, .. } | StmtKind::CompoundAssign { place, .. } => Some(place),
            _ => None,
        },
    }
}

/// The component of an `e[C]` that a node uses, and every component of a
/// query whose parameter the body uses.
fn components_used(node: &Node<'_>, out: &mut Vec<usize>) {
    if let Some(place) = place_of(node)
        && let PlaceKind::Component { component, .. } = root(place).kind
    {
        out.push(component.0);
    }
    if let Node::Stmt(Stmt {
        kind: StmtKind::Each { query, .. },
        ..
    }) = node
    {
        for term in &query.terms {
            if let Term::Component {
                component,
                touched: true,
                ..
            } = term
            {
                out.push(component.0);
            }
        }
    }
}

/// The function a node calls, if any.
fn callee(node: &Node<'_>) -> Option<usize> {
    match node {
        Node::Expr(Expr {
            kind: ExprKind::Call { func, .. },
            ..
        })
        | Node::Stmt(Stmt {
            kind:
                StmtKind::CompoundAssign {
                    op: CompoundOp::Method(func),
                    ..
                },
            ..
        }) => Some(func.0),
        _ => None,
    }
}

/// For each function, by component: whether it has an `e[C]` or a query
/// term whose parameter is used, directly or through the functions it
/// calls.
fn reach(program: &Program) -> Vec<Vec<bool>> {
    let components = program.components.len();
    let mut reaches = vec![vec![false; components]; program.funcs.len()];
    let mut calls = vec![Vec::new(); program.funcs.len()];
    for (index, func) in program.funcs.iter().enumerate() {
        let mut used = Vec::new();
        visit_block(&func.body.block, 0, &mut |node, _| {
            components_used(&node, &mut used);
            if let Some(callee) = callee(&node) {
                calls[index].push(callee);
            }
        });
        for component in used {
            reaches[index][component] = true;
        }
    }
    let mut changed = true;
    while changed {
        changed = false;
        for (caller, called) in calls.iter().enumerate() {
            for &called in called {
                let gained: Vec<usize> = (0..components)
                    .filter(|&c| reaches[called][c] && !reaches[caller][c])
                    .collect();
                changed |= !gained.is_empty();
                for component in gained {
                    reaches[caller][component] = true;
                }
            }
        }
    }
    reaches
}

/// Whether a statement can be part of the run that starts a body.
fn plain(stmt: &Stmt) -> bool {
    match &stmt.kind {
        StmtKind::Let { .. }
        | StmtKind::Assign { .. }
        | StmtKind::CompoundAssign { .. }
        | StmtKind::Puts(_)
        | StmtKind::Print(_)
        | StmtKind::Emit { .. } => true,
        StmtKind::Expr(e) => !matches!(e.kind, ExprKind::If { .. } | ExprKind::Block(_)),
        _ => false,
    }
}

/// Whether a statement of the run reads `param`. The place an assignment
/// stores to is not read, unless the store goes into a list.
fn reads(stmt: &Stmt, param: LocalId) -> bool {
    let mut read = false;
    let mut check = |node: Node<'_>, _: usize| {
        if let Some(place) = place_of(&node)
            && is_local(place, param)
        {
            read = true;
        }
    };
    match &stmt.kind {
        StmtKind::Assign { place, value } => {
            let mut chain = place;
            loop {
                match &chain.kind {
                    PlaceKind::Field { base, .. } => chain = base,
                    PlaceKind::Index { base, index } => {
                        if is_local(base, param) {
                            return true;
                        }
                        visit_expr(index, 0, &mut check);
                        chain = base;
                    }
                    _ => break,
                }
            }
            visit_expr(value, 0, &mut check);
        }
        _ => visit_stmt(stmt, 0, &mut check),
    }
    read
}

/// Rules 1 and 2: the run of plain statements that starts `body` assigns
/// the whole of `param`, of a component with `fields` fields, before it
/// reads it.
fn run_overwrites(body: &Block, param: LocalId, fields: usize) -> bool {
    if fields == 0 {
        return false;
    }
    let mut assigned = vec![false; fields];
    for stmt in &body.stmts {
        if !plain(stmt) || reads(stmt, param) {
            return false;
        }
        if let StmtKind::Assign { place, .. } = &stmt.kind {
            match &place.kind {
                PlaceKind::Local(id) if *id == param => return true,
                PlaceKind::Field { base, field } if matches!(base.kind, PlaceKind::Local(id) if id == param) =>
                {
                    assigned[*field] = true;
                    if assigned.iter().all(|&done| done) {
                        return true;
                    }
                }
                _ => {}
            }
        }
    }
    false
}

/// Rule 3: whether anything in an ordinary `each`'s body other than its
/// parameter can reach `component`, or the body can be left before every
/// entity has been visited.
fn reached_otherwise(body: &Block, component: usize, reaches: &[Vec<bool>]) -> bool {
    let mut found = false;
    visit_block(body, 0, &mut |node, depth| {
        let mut used = Vec::new();
        components_used(&node, &mut used);
        let leaves = match &node {
            Node::Stmt(s) => match s.kind {
                StmtKind::Return(_) => true,
                StmtKind::Break => depth == 0,
                _ => false,
            },
            Node::Expr(_) => false,
        };
        if leaves
            || used.contains(&component)
            || callee(&node).is_some_and(|func| reaches[func][component])
        {
            found = true;
        }
    });
    found
}

/// Rule 4: whether a nested `each` of a GPU block uses `component`.
fn nested_uses(body: &Block, component: usize) -> bool {
    let mut found = false;
    visit_block(body, 0, &mut |node, _| {
        if let Node::Stmt(Stmt {
            kind: StmtKind::Each { .. },
            ..
        }) = &node
        {
            let mut used = Vec::new();
            components_used(&node, &mut used);
            found |= used.contains(&component);
        }
    });
    found
}

/// The overwritten flag of each term of an `each`, by the rules above.
fn flags(
    program: &Program,
    reaches: &[Vec<bool>],
    query_terms: &[Term],
    params: &[LocalId],
    body: &Block,
    gpu: bool,
) -> Vec<bool> {
    query_terms
        .iter()
        .zip(params)
        .map(|(term, &param)| {
            let Term::Component {
                component,
                touched: true,
                ..
            } = *term
            else {
                return false;
            };
            let Some(record) = program.components[component.0].record else {
                return false;
            };
            let fields = program.records[record.0].fields.len();
            run_overwrites(body, param, fields)
                && if gpu {
                    !nested_uses(body, component.0)
                } else {
                    !reached_otherwise(body, component.0, reaches)
                }
        })
        .collect()
}

/// Sets the overwritten flag of every outer term of a GPU block and every
/// term of an ordinary `each`, in a program without errors.
pub(super) fn mark(program: &mut Program) {
    let reaches = reach(program);
    // The flags of each `each`, by the span of its statement.
    let mut found: HashMap<Span, Vec<bool>> = HashMap::new();
    {
        let program = &*program;
        let ordinary = |block: &Block, found: &mut HashMap<Span, Vec<bool>>| {
            visit_block(block, 0, &mut |node, _| {
                if let Node::Stmt(Stmt {
                    kind:
                        StmtKind::Each {
                            query,
                            params,
                            body,
                            ..
                        },
                    span,
                }) = node
                {
                    let flags = flags(program, &reaches, &query.terms, params, body, false);
                    found.insert(*span, flags);
                }
            });
        };
        for func in &program.funcs {
            ordinary(&func.body.block, &mut found);
        }
        for system in &program.systems {
            if !system.gpu {
                ordinary(&system.body.block, &mut found);
                continue;
            }
            for stmt in &system.body.block.stmts {
                if let StmtKind::Each {
                    query,
                    params,
                    body,
                    ..
                } = &stmt.kind
                {
                    let flags = flags(program, &reaches, &query.terms, params, body, true);
                    found.insert(stmt.span, flags);
                }
            }
        }
        ordinary(&program.main.block, &mut found);
    }

    let mut apply = |block: &mut Block| {
        super::walk::block(block, &mut |node| {
            if let super::walk::Node::Stmt(Stmt {
                kind: StmtKind::Each { query, .. },
                span,
            }) = node
                && let Some(flags) = found.remove(span)
            {
                for (term, flag) in query.terms.iter_mut().zip(flags) {
                    if let Term::Component { overwritten, .. } = term {
                        *overwritten = flag;
                    }
                }
            }
        });
    };
    for func in &mut program.funcs {
        apply(&mut func.body.block);
    }
    for system in &mut program.systems {
        apply(&mut system.body.block);
    }
    apply(&mut program.main.block);
}
