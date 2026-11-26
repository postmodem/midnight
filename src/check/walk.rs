//! Visits every statement and expression of a typed tree, parents first.

use crate::tir::{Block, Expr, ExprKind, LocalId, Place, PlaceKind, Stmt, StmtKind};

pub(super) enum Node<'t> {
    Stmt(&'t mut Stmt),
    Expr(&'t mut Expr),
    /// A local that a place starts from.
    Local(LocalId),
}

pub(super) type Visit<'f> = &'f mut dyn FnMut(Node<'_>);

pub(super) fn block(b: &mut Block, f: Visit<'_>) {
    for s in &mut b.stmts {
        stmt(s, f);
    }
    if let Some(value) = &mut b.value {
        expr(value, f);
    }
}

pub(super) fn stmt(s: &mut Stmt, f: Visit<'_>) {
    f(Node::Stmt(s));
    match &mut s.kind {
        StmtKind::Expr(e) | StmtKind::Let { init: e, .. } | StmtKind::Emit { value: e, .. } => {
            expr(e, f)
        }
        StmtKind::Assign { place: p, value }
        | StmtKind::CompoundAssign {
            place: p, value, ..
        } => {
            place(p, f);
            expr(value, f);
        }
        StmtKind::Return(value) => {
            if let Some(value) = value {
                expr(value, f);
            }
        }
        StmtKind::Break | StmtKind::Next => {}
        StmtKind::While { cond, body } => {
            expr(cond, f);
            block(body, f);
        }
        StmtKind::Count { from, to, body, .. } => {
            expr(from, f);
            expr(to, f);
            block(body, f);
        }
        StmtKind::ListEach { list, body, .. } => {
            expr(list, f);
            block(body, f);
        }
        StmtKind::Each { body, .. } | StmtKind::On { body, .. } => block(body, f),
        StmtKind::Puts(args) | StmtKind::Print(args) => {
            for arg in args {
                expr(arg, f);
            }
        }
        StmtKind::WindowOpen {
            title,
            width,
            height,
        } => {
            expr(title, f);
            expr(width, f);
            expr(height, f);
        }
    }
}

pub(super) fn expr(e: &mut Expr, f: Visit<'_>) {
    f(Node::Expr(e));
    match &mut e.kind {
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
            place(p, f)
        }
        ExprKind::Field { base, .. } => expr(base, f),
        ExprKind::Index { base, index } => {
            expr(base, f);
            expr(index, f);
        }
        ExprKind::ListLit(items) | ExprKind::Construct(items) | ExprKind::Concat(items) => {
            for item in items {
                expr(item, f);
            }
        }
        ExprKind::Unary { expr: inner, .. }
        | ExprKind::IntToFloat(inner)
        | ExprKind::Despawn(inner)
        | ExprKind::Alive(inner)
        | ExprKind::Input { arg: inner, .. }
        | ExprKind::Remove { entity: inner, .. }
        | ExprKind::Has { entity: inner, .. } => expr(inner, f),
        ExprKind::Binary { lhs, rhs, .. }
        | ExprKind::And { lhs, rhs }
        | ExprKind::Or { lhs, rhs } => {
            expr(lhs, f);
            expr(rhs, f);
        }
        ExprKind::Call { args, .. } | ExprKind::Builtin { args, .. } => {
            for arg in args {
                expr(arg, f);
            }
        }
        ExprKind::If { cond, then, else_ } => {
            expr(cond, f);
            block(then, f);
            block(else_, f);
        }
        ExprKind::Block(b) => block(b, f),
        ExprKind::Spawn(components) => {
            for init in components {
                if let Some(value) = &mut init.value {
                    expr(value, f);
                }
            }
        }
        ExprKind::Add { entity, components } => {
            expr(entity, f);
            for init in components {
                if let Some(value) = &mut init.value {
                    expr(value, f);
                }
            }
        }
        ExprKind::ListPush { list, value } => {
            place(list, f);
            expr(value, f);
        }
        ExprKind::ListDeleteAt { list, index } => {
            place(list, f);
            expr(index, f);
        }
    }
}

fn place(p: &mut Place, f: Visit<'_>) {
    match &mut p.kind {
        PlaceKind::Local(id) => f(Node::Local(*id)),
        PlaceKind::Const(_) | PlaceKind::ResourceField(..) | PlaceKind::Time(_) => {}
        PlaceKind::Component { entity, .. } => expr(entity, f),
        PlaceKind::Field { base, .. } => place(base, f),
        PlaceKind::Index { base, index } => {
            place(base, f);
            expr(index, f);
        }
    }
}
