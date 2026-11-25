//! Questions the emitter asks about typed code before generating it.

use crate::tir::{
    BinOp, Block, Body, CompoundOp, Expr, ExprKind, Place, PlaceKind, Stmt, StmtKind, TimeField,
    Type, UnOp,
};

/// How a place is used where it appears.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Use {
    Read,
    Write,
    /// Read and written: a compound assignment or a list mutator.
    Modify,
}

pub enum Node<'a> {
    Stmt(&'a Stmt),
    Expr(&'a Expr),
    /// A whole place, outermost first; its operands follow as `Expr`s.
    Place(&'a Place, Use),
}

/// Calls `f` for every statement, expression and place in the block,
/// nested blocks included.
pub fn walk_block<'a>(block: &'a Block, f: &mut dyn FnMut(Node<'a>)) {
    for stmt in &block.stmts {
        walk_stmt(stmt, f);
    }
    if let Some(value) = &block.value {
        walk_expr(value, f);
    }
}

pub fn walk_stmt<'a>(stmt: &'a Stmt, f: &mut dyn FnMut(Node<'a>)) {
    f(Node::Stmt(stmt));
    match &stmt.kind {
        StmtKind::Expr(expr) => walk_expr(expr, f),
        StmtKind::Let { init, .. } => walk_expr(init, f),
        StmtKind::Assign { place, value } => {
            walk_place(place, Use::Write, f);
            walk_expr(value, f);
        }
        StmtKind::CompoundAssign { place, value, .. } => {
            walk_place(place, Use::Modify, f);
            walk_expr(value, f);
        }
        StmtKind::Return(value) => {
            if let Some(value) = value {
                walk_expr(value, f);
            }
        }
        StmtKind::Break | StmtKind::Next => {}
        StmtKind::While { cond, body } => {
            walk_expr(cond, f);
            walk_block(body, f);
        }
        StmtKind::Count { from, to, body, .. } => {
            walk_expr(from, f);
            walk_expr(to, f);
            walk_block(body, f);
        }
        StmtKind::ListEach { list, body, .. } => {
            walk_expr(list, f);
            walk_block(body, f);
        }
        StmtKind::Each { body, .. } | StmtKind::On { body, .. } => walk_block(body, f),
        StmtKind::Puts(args) | StmtKind::Print(args) => {
            for arg in args {
                walk_expr(arg, f);
            }
        }
        StmtKind::Emit { value, .. } => walk_expr(value, f),
    }
}

pub fn walk_expr<'a>(expr: &'a Expr, f: &mut dyn FnMut(Node<'a>)) {
    f(Node::Expr(expr));
    match &expr.kind {
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
        ExprKind::Read(place) => walk_place(place, Use::Read, f),
        ExprKind::Field { base, .. } => walk_expr(base, f),
        ExprKind::Index { base, index } => {
            walk_expr(base, f);
            walk_expr(index, f);
        }
        ExprKind::ListLit(items) | ExprKind::Construct(items) | ExprKind::Concat(items) => {
            for item in items {
                walk_expr(item, f);
            }
        }
        ExprKind::Unary { expr, .. } | ExprKind::IntToFloat(expr) => walk_expr(expr, f),
        ExprKind::Binary { lhs, rhs, .. }
        | ExprKind::And { lhs, rhs }
        | ExprKind::Or { lhs, rhs } => {
            walk_expr(lhs, f);
            walk_expr(rhs, f);
        }
        ExprKind::Call { args, .. } | ExprKind::Builtin { args, .. } => {
            for arg in args {
                walk_expr(arg, f);
            }
        }
        ExprKind::If { cond, then, else_ } => {
            walk_expr(cond, f);
            walk_block(then, f);
            walk_block(else_, f);
        }
        ExprKind::Block(block) => walk_block(block, f),
        ExprKind::Spawn(inits) => {
            for init in inits {
                if let Some(value) = &init.value {
                    walk_expr(value, f);
                }
            }
        }
        ExprKind::Add { entity, components } => {
            walk_expr(entity, f);
            for init in components {
                if let Some(value) = &init.value {
                    walk_expr(value, f);
                }
            }
        }
        ExprKind::Remove { entity, .. }
        | ExprKind::Despawn(entity)
        | ExprKind::Has { entity, .. }
        | ExprKind::Alive(entity) => walk_expr(entity, f),
        ExprKind::ListPush { list, value } => {
            walk_place(list, Use::Modify, f);
            walk_expr(value, f);
        }
        ExprKind::ListPop { list } | ExprKind::ListClear { list } => {
            walk_place(list, Use::Modify, f);
        }
        ExprKind::ListDeleteAt { list, index } => {
            walk_place(list, Use::Modify, f);
            walk_expr(index, f);
        }
    }
}

pub fn walk_place<'a>(place: &'a Place, usage: Use, f: &mut dyn FnMut(Node<'a>)) {
    f(Node::Place(place, usage));
    place_operands(place, f);
}

fn place_operands<'a>(place: &'a Place, f: &mut dyn FnMut(Node<'a>)) {
    match &place.kind {
        PlaceKind::Local(_)
        | PlaceKind::Const(_)
        | PlaceKind::ResourceField(..)
        | PlaceKind::Time(_) => {}
        PlaceKind::Component { entity, .. } => walk_expr(entity, f),
        PlaceKind::Field { base, .. } => place_operands(base, f),
        PlaceKind::Index { base, index } => {
            place_operands(base, f);
            walk_expr(index, f);
        }
    }
}

/// The place a chain of fields and indexes starts from.
pub fn root(place: &Place) -> &Place {
    let mut current = place;
    loop {
        match &current.kind {
            PlaceKind::Field { base, .. } | PlaceKind::Index { base, .. } => current = base,
            _ => return current,
        }
    }
}

/// Whether evaluating the expression can neither run user code nor change
/// any place. It may still read anything and stop the program.
pub fn inert(expr: &Expr) -> bool {
    let mut ok = true;
    walk_expr(expr, &mut |node| match node {
        Node::Stmt(_) => ok = false,
        Node::Expr(expr) => {
            if matches!(
                expr.kind,
                ExprKind::Call { .. }
                    | ExprKind::Spawn(_)
                    | ExprKind::Add { .. }
                    | ExprKind::Remove { .. }
                    | ExprKind::Despawn(_)
                    | ExprKind::ListPush { .. }
                    | ExprKind::ListPop { .. }
                    | ExprKind::ListDeleteAt { .. }
                    | ExprKind::ListClear { .. }
                    | ExprKind::WorldTick
                    | ExprKind::WorldRun
            ) {
                ok = false;
            }
        }
        Node::Place(..) => {}
    });
    ok
}

/// Whether evaluating the expression leaves the locals of the enclosing
/// body as they are. Functions it calls can't reach them.
pub fn local_safe(expr: &Expr) -> bool {
    let mut ok = true;
    walk_expr(expr, &mut |node| match node {
        Node::Stmt(_) => ok = false,
        Node::Expr(expr) => {
            if matches!(
                expr.kind,
                ExprKind::ListPush { .. }
                    | ExprKind::ListPop { .. }
                    | ExprKind::ListDeleteAt { .. }
                    | ExprKind::ListClear { .. }
            ) {
                ok = false;
            }
        }
        Node::Place(..) => {}
    });
    ok
}

/// Which locals of a body are used, by local id.
pub struct LocalUse {
    /// The local's value is read somewhere.
    pub read: Vec<bool>,
    /// The local appears in some place, read or written.
    pub touched: Vec<bool>,
}

pub fn local_use(body: &Body) -> LocalUse {
    let mut usage = LocalUse {
        read: vec![false; body.locals.len()],
        touched: vec![false; body.locals.len()],
    };
    walk_block(&body.block, &mut |node| {
        if let Node::Place(place, how) = node
            && let PlaceKind::Local(id) = root(place).kind
        {
            usage.touched[id.0] = true;
            if how == Use::Read {
                usage.read[id.0] = true;
            }
        }
    });
    usage
}

/// Whether nothing in the body of an `each` can reach the columns except
/// its own parameters: no user function runs, no `e[C]` is used, no other
/// query is nested in it and no tick runs.
pub fn vector_ok(body: &Block) -> bool {
    let mut ok = true;
    walk_block(body, &mut |node| match node {
        Node::Stmt(stmt) => {
            if matches!(
                stmt.kind,
                StmtKind::Each { .. }
                    | StmtKind::CompoundAssign {
                        op: CompoundOp::Method(_),
                        ..
                    }
            ) {
                ok = false;
            }
        }
        Node::Expr(expr) => {
            if matches!(
                expr.kind,
                ExprKind::Call { .. } | ExprKind::WorldTick | ExprKind::WorldRun
            ) {
                ok = false;
            }
        }
        Node::Place(place, _) => {
            if matches!(root(place).kind, PlaceKind::Component { .. }) {
                ok = false;
            }
        }
    });
    ok
}

/// Whether the block uses a local for which `pick` is true.
pub fn touches_local(body: &Block, pick: &dyn Fn(usize) -> bool) -> bool {
    let mut found = false;
    walk_block(body, &mut |node| {
        if let Node::Place(place, _) = node
            && let PlaceKind::Local(id) = root(place).kind
            && pick(id.0)
        {
            found = true;
        }
    });
    found
}

/// A resource field or `Time` field that can be loaded once before a loop.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum HoistKey {
    Resource(usize, usize),
    Time(u8),
}

pub fn time_key(field: TimeField) -> HoistKey {
    HoistKey::Time(match field {
        TimeField::Tick => 0,
        TimeField::Dt => 1,
        TimeField::Elapsed => 2,
    })
}

/// The resource fields and `Time` fields the block reads and never writes,
/// in the order they are first read.
pub fn hoistable(body: &Block) -> Vec<HoistKey> {
    let mut read = Vec::new();
    let mut written = Vec::new();
    walk_block(body, &mut |node| {
        let Node::Place(place, how) = node else {
            return;
        };
        let key = match root(place).kind {
            PlaceKind::ResourceField(resource, field) => HoistKey::Resource(resource.0, field),
            PlaceKind::Time(field) => time_key(field),
            _ => return,
        };
        let list = if how == Use::Read {
            &mut read
        } else {
            &mut written
        };
        if !list.contains(&key) {
            list.push(key);
        }
    });
    read.retain(|key| !written.contains(key));
    read
}

/// The value of a constant that is known at compile time.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Folded {
    Int(i64),
    Float(f64),
    Bool(bool),
}

/// Evaluates a constant expression made of number and `Bool` literals,
/// arithmetic that can't fail, and constants already folded.
pub fn fold(expr: &Expr, known: &[Option<Folded>]) -> Option<Folded> {
    match &expr.kind {
        ExprKind::Int(value) => Some(Folded::Int(*value)),
        ExprKind::Float(value) => Some(Folded::Float(*value)),
        ExprKind::Bool(value) => Some(Folded::Bool(*value)),
        ExprKind::Read(Place {
            kind: PlaceKind::Const(id),
            ..
        }) => known.get(id.0).copied().flatten(),
        ExprKind::IntToFloat(inner) => match fold(inner, known)? {
            Folded::Int(value) => Some(Folded::Float(value as f64)),
            _ => None,
        },
        ExprKind::Unary { op, expr } => match (op, fold(expr, known)?) {
            (UnOp::Neg, Folded::Int(value)) => Some(Folded::Int(value.wrapping_neg())),
            (UnOp::Neg, Folded::Float(value)) => Some(Folded::Float(-value)),
            (UnOp::Not, Folded::Bool(value)) => Some(Folded::Bool(!value)),
            _ => None,
        },
        ExprKind::Binary { op, lhs, rhs } => match (fold(lhs, known)?, fold(rhs, known)?) {
            (Folded::Int(a), Folded::Int(b)) => fold_int(*op, a, b).map(Folded::Int),
            (Folded::Float(a), Folded::Float(b)) => match op {
                BinOp::Add => Some(Folded::Float(a + b)),
                BinOp::Sub => Some(Folded::Float(a - b)),
                BinOp::Mul => Some(Folded::Float(a * b)),
                BinOp::Div => Some(Folded::Float(a / b)),
                _ => None,
            },
            _ => None,
        },
        _ => None,
    }
}

/// `Int` arithmetic that wraps and can't fail.
pub fn fold_int(op: BinOp, a: i64, b: i64) -> Option<i64> {
    match op {
        BinOp::Add => Some(a.wrapping_add(b)),
        BinOp::Sub => Some(a.wrapping_sub(b)),
        BinOp::Mul => Some(a.wrapping_mul(b)),
        _ => None,
    }
}

/// Whether a statement always leaves the block it is in.
pub fn leaves(stmt: &Stmt) -> bool {
    matches!(
        stmt.kind,
        StmtKind::Return(_) | StmtKind::Break | StmtKind::Next
    )
}

/// Whether the block ends by leaving: nothing after it in the same scope
/// can run.
pub fn block_leaves(block: &Block) -> bool {
    block.value.is_none() && block.stmts.last().is_some_and(leaves)
}

/// Whether a constant of this type can be folded.
pub fn foldable_type(ty: &Type) -> bool {
    matches!(ty, Type::Int | Type::Float | Type::Bool)
}
