//! Shorthand for writing typed programs by hand.

use crate::source::{FileId, Span};
use crate::tir::*;

/// A span on the first line of the test file.
pub const S: Span = Span {
    file: FileId(0),
    start: 0,
    end: 0,
};

pub fn empty_program() -> Program {
    Program {
        records: Vec::new(),
        enums: Vec::new(),
        components: Vec::new(),
        resources: Vec::new(),
        events: Vec::new(),
        consts: Vec::new(),
        funcs: Vec::new(),
        systems: Vec::new(),
        main: Body {
            locals: Vec::new(),
            block: blk(vec![]),
        },
    }
}

fn fields(list: &[(&str, Type)]) -> Vec<Field> {
    list.iter()
        .map(|(name, ty)| Field {
            name: name.to_string(),
            ty: ty.clone(),
        })
        .collect()
}

pub fn structure(p: &mut Program, name: &str, list: &[(&str, Type)]) -> Type {
    p.records.push(Record {
        name: name.to_string(),
        kind: RecordKind::Struct,
        fields: fields(list),
    });
    Type::Record(RecordId(p.records.len() - 1))
}

pub fn component(p: &mut Program, name: &str, list: &[(&str, Type)]) -> (ComponentId, Type) {
    let id = ComponentId(p.components.len());
    p.records.push(Record {
        name: name.to_string(),
        kind: RecordKind::Component(id),
        fields: fields(list),
    });
    let record = RecordId(p.records.len() - 1);
    p.components.push(Component {
        name: name.to_string(),
        record: Some(record),
    });
    (id, Type::Record(record))
}

pub fn tag(p: &mut Program, name: &str) -> ComponentId {
    p.components.push(Component {
        name: name.to_string(),
        record: None,
    });
    ComponentId(p.components.len() - 1)
}

pub fn event(p: &mut Program, name: &str, list: &[(&str, Type)]) -> (EventId, Type) {
    let id = EventId(p.events.len());
    p.records.push(Record {
        name: name.to_string(),
        kind: RecordKind::Event(id),
        fields: fields(list),
    });
    let record = RecordId(p.records.len() - 1);
    p.events.push(Event {
        name: name.to_string(),
        record,
    });
    (id, Type::Record(record))
}

pub fn enumeration(p: &mut Program, name: &str, variants: &[&str]) -> Type {
    p.enums.push(Enum {
        name: name.to_string(),
        variants: variants.iter().map(|v| v.to_string()).collect(),
    });
    Type::Enum(EnumId(p.enums.len() - 1))
}

pub fn resource(p: &mut Program, name: &str, list: Vec<(&str, Type, Expr)>) -> ResourceId {
    p.resources.push(Resource {
        name: name.to_string(),
        fields: list
            .into_iter()
            .map(|(name, ty, init)| ResourceField {
                name: name.to_string(),
                ty,
                init,
            })
            .collect(),
    });
    ResourceId(p.resources.len() - 1)
}

pub fn constant(p: &mut Program, name: &str, init: Expr) -> Place {
    let ty = init.ty.clone();
    p.consts.push(Const {
        name: name.to_string(),
        ty: ty.clone(),
        init,
    });
    Place {
        kind: PlaceKind::Const(ConstId(p.consts.len() - 1)),
        ty,
        span: S,
    }
}

/// The locals of one body.
#[derive(Default)]
pub struct Locals(pub Vec<Local>);

impl Locals {
    fn add(&mut self, name: &str, ty: &Type, kind: LocalKind) -> Place {
        self.0.push(Local {
            name: name.to_string(),
            ty: ty.clone(),
            kind,
        });
        Place {
            kind: PlaceKind::Local(LocalId(self.0.len() - 1)),
            ty: ty.clone(),
            span: S,
        }
    }

    pub fn var(&mut self, name: &str, ty: &Type) -> Place {
        self.add(name, ty, LocalKind::Var)
    }

    pub fn param(&mut self, name: &str, ty: &Type) -> Place {
        self.add(name, ty, LocalKind::Param)
    }

    pub fn view(&mut self, name: &str, ty: &Type) -> Place {
        self.add(name, ty, LocalKind::View)
    }

    pub fn body(self, block: Block) -> Body {
        Body {
            locals: self.0,
            block,
        }
    }
}

/// The local a place made by `Locals` names.
pub fn id(place: &Place) -> LocalId {
    match place.kind {
        PlaceKind::Local(id) => id,
        _ => panic!("not a local"),
    }
}

/// Adds a function whose parameters are the first `params` locals.
pub fn function(
    p: &mut Program,
    name: &str,
    owner: Option<&Type>,
    params: usize,
    ret: Type,
    body: Body,
) -> FuncId {
    p.funcs.push(Func {
        name: name.to_string(),
        owner: owner.map(|ty| match ty {
            Type::Record(id) => *id,
            _ => panic!("not a record"),
        }),
        params: (0..params).map(LocalId).collect(),
        ret,
        body,
        span: S,
    });
    FuncId(p.funcs.len() - 1)
}

pub fn system(p: &mut Program, name: &str, body: Body) {
    p.systems.push(System {
        name: name.to_string(),
        gpu: false,
        body,
        span: S,
    });
}

pub fn list_of(elem: &Type) -> Type {
    Type::List(Box::new(elem.clone()))
}

pub fn f32_type() -> Type {
    Type::Compact(Compact::Float32)
}

// ---- Places ----

pub fn fld(base: &Place, field: usize, ty: &Type) -> Place {
    Place {
        kind: PlaceKind::Field {
            base: Box::new(base.clone()),
            field,
        },
        ty: ty.clone(),
        span: S,
    }
}

pub fn at(base: &Place, index: Expr) -> Place {
    let Type::List(elem) = &base.ty else {
        panic!("not a list");
    };
    Place {
        kind: PlaceKind::Index {
            base: Box::new(base.clone()),
            index: Box::new(index),
        },
        ty: (**elem).clone(),
        span: S,
    }
}

pub fn of(entity: Expr, component: ComponentId, ty: &Type, write: bool) -> Place {
    Place {
        kind: PlaceKind::Component {
            entity: Box::new(entity),
            component,
            write,
        },
        ty: ty.clone(),
        span: S,
    }
}

pub fn res(resource: ResourceId, field: usize, ty: &Type) -> Place {
    Place {
        kind: PlaceKind::ResourceField(resource, field),
        ty: ty.clone(),
        span: S,
    }
}

pub fn time(field: TimeField) -> Place {
    Place {
        kind: PlaceKind::Time(field),
        ty: if field == TimeField::Tick {
            Type::Int
        } else {
            Type::Float
        },
        span: S,
    }
}

// ---- Expressions ----

pub fn ex(kind: ExprKind, ty: Type) -> Expr {
    Expr { kind, ty, span: S }
}

pub fn int(value: i64) -> Expr {
    ex(ExprKind::Int(value), Type::Int)
}

pub fn flt(value: f64) -> Expr {
    ex(ExprKind::Float(value), Type::Float)
}

pub fn boolean(value: bool) -> Expr {
    ex(ExprKind::Bool(value), Type::Bool)
}

pub fn text(value: &str) -> Expr {
    ex(ExprKind::Str(value.to_string()), Type::String)
}

pub fn rd(place: &Place) -> Expr {
    ex(ExprKind::Read(place.clone()), place.ty.value_type())
}

pub fn bin(op: BinOp, lhs: Expr, rhs: Expr) -> Expr {
    let ty = match op {
        BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => Type::Bool,
        _ => lhs.ty.clone(),
    };
    ex(
        ExprKind::Binary {
            op,
            lhs: Box::new(lhs),
            rhs: Box::new(rhs),
        },
        ty,
    )
}

pub fn neg(expr: Expr) -> Expr {
    let ty = expr.ty.clone();
    ex(
        ExprKind::Unary {
            op: UnOp::Neg,
            expr: Box::new(expr),
        },
        ty,
    )
}

pub fn not(expr: Expr) -> Expr {
    ex(
        ExprKind::Unary {
            op: UnOp::Not,
            expr: Box::new(expr),
        },
        Type::Bool,
    )
}

pub fn and(lhs: Expr, rhs: Expr) -> Expr {
    ex(
        ExprKind::And {
            lhs: Box::new(lhs),
            rhs: Box::new(rhs),
        },
        Type::Bool,
    )
}

pub fn or(lhs: Expr, rhs: Expr) -> Expr {
    ex(
        ExprKind::Or {
            lhs: Box::new(lhs),
            rhs: Box::new(rhs),
        },
        Type::Bool,
    )
}

pub fn to_f(expr: Expr) -> Expr {
    ex(ExprKind::IntToFloat(Box::new(expr)), Type::Float)
}

pub fn call(func: FuncId, args: Vec<Expr>, ret: &Type) -> Expr {
    ex(ExprKind::Call { func, args }, ret.clone())
}

pub fn bi(op: Builtin, args: Vec<Expr>, ty: Type) -> Expr {
    ex(ExprKind::Builtin { op, args }, ty)
}

pub fn to_s(expr: Expr) -> Expr {
    bi(Builtin::ToS, vec![expr], Type::String)
}

pub fn concat(parts: Vec<Expr>) -> Expr {
    ex(ExprKind::Concat(parts), Type::String)
}

pub fn list(elem: &Type, items: Vec<Expr>) -> Expr {
    ex(ExprKind::ListLit(items), list_of(elem))
}

pub fn make(ty: &Type, values: Vec<Expr>) -> Expr {
    ex(ExprKind::Construct(values), ty.clone())
}

pub fn iff(cond: Expr, then: Block, else_: Block, ty: Type) -> Expr {
    ex(
        ExprKind::If {
            cond: Box::new(cond),
            then,
            else_,
        },
        ty,
    )
}

pub fn push(list: &Place, value: Expr) -> Expr {
    ex(
        ExprKind::ListPush {
            list: list.clone(),
            value: Box::new(value),
        },
        Type::Void,
    )
}

pub fn pop(list: &Place) -> Expr {
    let Type::List(elem) = &list.ty else {
        panic!("not a list");
    };
    ex(ExprKind::ListPop { list: list.clone() }, elem.value_type())
}

pub fn spawn(inits: Vec<(ComponentId, Option<Expr>)>) -> Expr {
    ex(ExprKind::Spawn(inits_of(inits)), Type::Entity)
}

pub fn inits_of(inits: Vec<(ComponentId, Option<Expr>)>) -> Vec<ComponentInit> {
    inits
        .into_iter()
        .map(|(component, value)| ComponentInit { component, value })
        .collect()
}

// ---- Statements ----

pub fn st(kind: StmtKind) -> Stmt {
    Stmt { kind, span: S }
}

pub fn es(expr: Expr) -> Stmt {
    st(StmtKind::Expr(expr))
}

pub fn let_(local: &Place, init: Expr) -> Stmt {
    st(StmtKind::Let {
        local: id(local),
        init,
    })
}

pub fn set(place: &Place, value: Expr) -> Stmt {
    st(StmtKind::Assign {
        place: place.clone(),
        value,
    })
}

pub fn cset(place: &Place, op: BinOp, value: Expr) -> Stmt {
    st(StmtKind::CompoundAssign {
        place: place.clone(),
        op: CompoundOp::Builtin(op),
        value,
    })
}

pub fn ret(value: Option<Expr>) -> Stmt {
    st(StmtKind::Return(value))
}

pub fn puts(args: Vec<Expr>) -> Stmt {
    st(StmtKind::Puts(args))
}

pub fn when(cond: Expr, then: Vec<Stmt>) -> Stmt {
    es(iff(cond, blk(then), blk(vec![]), Type::Void))
}

pub fn when_else(cond: Expr, then: Vec<Stmt>, else_: Vec<Stmt>) -> Stmt {
    es(iff(cond, blk(then), blk(else_), Type::Void))
}

pub fn while_(cond: Expr, body: Vec<Stmt>) -> Stmt {
    st(StmtKind::While {
        cond,
        body: blk(body),
    })
}

pub fn times(var: Option<&Place>, count: Expr, body: Vec<Stmt>) -> Stmt {
    st(StmtKind::Count {
        var: var.map(id),
        from: int(0),
        to: count,
        inclusive: false,
        down: false,
        body: blk(body),
    })
}

pub fn list_each(list: Expr, elem: Option<&Place>, index: Option<&Place>, body: Vec<Stmt>) -> Stmt {
    st(StmtKind::ListEach {
        list,
        elem: elem.map(id),
        index: index.map(id),
        body: blk(body),
    })
}

/// A query `each`. A term is `None` for `Entity`, or a component and
/// whether it is written.
pub fn each(
    terms: Vec<Option<(ComponentId, bool)>>,
    with: Vec<ComponentId>,
    without: Vec<ComponentId>,
    params: Vec<&Place>,
    body: Vec<Stmt>,
) -> Stmt {
    st(StmtKind::Each {
        query: query(terms, with, without),
        params: params.into_iter().map(id).collect(),
        body: blk(body),
    })
}

pub fn query(
    terms: Vec<Option<(ComponentId, bool)>>,
    with: Vec<ComponentId>,
    without: Vec<ComponentId>,
) -> Query {
    Query {
        terms: terms
            .into_iter()
            .map(|term| match term {
                None => Term::Entity,
                Some((component, write)) => Term::Component {
                    component,
                    write,
                    touched: true,
                    overwritten: false,
                },
            })
            .collect(),
        with,
        without,
    }
}

pub fn blk(stmts: Vec<Stmt>) -> Block {
    Block { stmts, value: None }
}

pub fn blkv(stmts: Vec<Stmt>, value: Expr) -> Block {
    Block {
        stmts,
        value: Some(Box::new(value)),
    }
}
