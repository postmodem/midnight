//! Checker tests. Programs are parsed from source text. Diagnostics are
//! compared by message, the text they mark and their label; typed trees
//! are compared in a compact printed form (see [`Dump`]).

use std::path::PathBuf;

use super::*;
use crate::parser;
use crate::tir::{Expr, ExprKind, Place, PlaceKind, Stmt, StmtKind};

mod diagnostics;
mod effects;
mod gpu;
mod overwrite;
mod typed;

fn parse(text: &str) -> (SourceMap, ast::Program) {
    let mut sources = SourceMap::new();
    let file = sources.add(
        "test.12am".to_string(),
        PathBuf::from("test.12am"),
        text.to_string(),
    );
    let items = parser::parse(file, text).unwrap_or_else(|error| {
        panic!(
            "syntax error in a checker test: {} at {}..{}\n{text}",
            error.message, error.span.start, error.span.end
        )
    });
    (sources, ast::Program { items })
}

/// The typed program for `text`, which must have no errors.
#[track_caller]
fn ok(text: &str) -> tir::Program {
    let (sources, program) = parse(text);
    match check(&program, &sources) {
        Ok(program) => program,
        Err(errors) => panic!(
            "unexpected errors:\n{}",
            crate::diag::render(&sources, &errors, false)
        ),
    }
}

/// The diagnostics for `text`, which must have some.
#[track_caller]
fn diagnostics(text: &str) -> Vec<Diagnostic> {
    let (sources, program) = parse(text);
    match check(&program, &sources) {
        Ok(_) => panic!("expected errors, but the program checks:\n{text}"),
        Err(errors) => errors,
    }
}

/// Each diagnostic for `text` as its message, the text it marks and its
/// label (empty when it has none).
#[track_caller]
fn errors(text: &str) -> Vec<(String, String, String)> {
    diagnostics(text)
        .into_iter()
        .map(|d| {
            let marked = text[d.span.start as usize..d.span.end as usize].to_string();
            (d.message, marked, d.label.unwrap_or_default())
        })
        .collect()
}

/// Checks that `text` has exactly these errors, in order.
#[track_caller]
fn assert_errors(text: &str, expected: &[(&str, &str, &str)]) {
    let actual = errors(text);
    let actual: Vec<(&str, &str, &str)> = actual
        .iter()
        .map(|(m, a, l)| (m.as_str(), a.as_str(), l.as_str()))
        .collect();
    assert_eq!(actual, expected, "\n{text}");
}

/// Checks that `text` has exactly one error.
#[track_caller]
fn assert_error(text: &str, message: &str, at: &str, label: &str) {
    assert_errors(text, &[(message, at, label)]);
}

/// Prints typed trees compactly:
///
/// - a place is written as in source, with `!` after a component that is
///   written: `pos.x`, `e[Health]!.hp`, `Score.points`;
/// - an operator is prefix and parenthesized: `(+ a b)`, `(float n)` for
///   `IntToFloat`;
/// - a list literal carries its type: `[1, 2]:[Int]`;
/// - calls are `name(args)`, methods `Owner.name(self, args)`, built-ins
///   by their variant name: `IntAbs(n)`;
/// - a block is `{ stmt; stmt; => value }`.
struct Dump<'p> {
    program: &'p tir::Program,
    locals: &'p [tir::Local],
}

impl Dump<'_> {
    fn ty(&self, ty: &Type) -> String {
        match ty {
            Type::Void => "Void".to_string(),
            Type::Int => "Int".to_string(),
            Type::Float => "Float".to_string(),
            Type::Bool => "Bool".to_string(),
            Type::String => "String".to_string(),
            Type::Entity => "Entity".to_string(),
            Type::Compact(c) => c.name().to_string(),
            Type::List(elem) => format!("[{}]", self.ty(elem)),
            Type::Record(id) => self.program.records[id.0].name.clone(),
            Type::Enum(id) => self.program.enums[id.0].name.clone(),
        }
    }

    fn local(&self, id: LocalId) -> &str {
        &self.locals[id.0].name
    }

    fn opt_local(&self, id: Option<LocalId>) -> &str {
        id.map_or("_", |id| self.local(id))
    }

    fn component(&self, id: ComponentId) -> &str {
        &self.program.components[id.0].name
    }

    fn list(&self, exprs: &[Expr]) -> String {
        let parts: Vec<String> = exprs.iter().map(|e| self.expr(e)).collect();
        parts.join(", ")
    }

    fn block(&self, block: &tir::Block) -> String {
        let mut parts: Vec<String> = block.stmts.iter().map(|s| self.stmt(s)).collect();
        if let Some(value) = &block.value {
            parts.push(format!("=> {}", self.expr(value)));
        }
        if parts.is_empty() {
            "{ }".to_string()
        } else {
            format!("{{ {} }}", parts.join("; "))
        }
    }

    fn query(&self, query: &tir::Query) -> String {
        let terms: Vec<String> = query
            .terms
            .iter()
            .map(|term| match term {
                tir::Term::Entity => "Entity".to_string(),
                tir::Term::Component {
                    component, write, ..
                } => {
                    format!(
                        "{}{}",
                        self.component(*component),
                        if *write { "!" } else { "" }
                    )
                }
            })
            .collect();
        let names = |ids: &[ComponentId]| {
            let names: Vec<&str> = ids.iter().map(|id| self.component(*id)).collect();
            names.join(", ")
        };
        let mut out = terms.join(", ");
        if !query.with.is_empty() {
            out.push_str(&format!(" with {}", names(&query.with)));
        }
        if !query.without.is_empty() {
            out.push_str(&format!(" without {}", names(&query.without)));
        }
        out
    }

    fn inits(&self, inits: &[tir::ComponentInit]) -> String {
        let parts: Vec<String> = inits
            .iter()
            .map(|init| match &init.value {
                Some(value) => self.expr(value),
                None => self.component(init.component).to_string(),
            })
            .collect();
        parts.join(", ")
    }

    fn place(&self, place: &Place) -> String {
        match &place.kind {
            PlaceKind::Local(id) => self.local(*id).to_string(),
            PlaceKind::Const(id) => self.program.consts[id.0].name.clone(),
            PlaceKind::ResourceField(id, field) => {
                let resource = &self.program.resources[id.0];
                format!("{}.{}", resource.name, resource.fields[*field].name)
            }
            PlaceKind::Time(field) => format!("Time.{}", format!("{field:?}").to_lowercase()),
            PlaceKind::Component {
                entity,
                component,
                write,
            } => format!(
                "{}[{}]{}",
                self.expr(entity),
                self.component(*component),
                if *write { "!" } else { "" }
            ),
            PlaceKind::Field { base, field } => {
                let Type::Record(record) = base.ty else {
                    panic!("field of a non-record place");
                };
                let name = &self.program.records[record.0].fields[*field].name;
                format!("{}.{name}", self.place(base))
            }
            PlaceKind::Index { base, index } => {
                format!("{}[{}]", self.place(base), self.expr(index))
            }
        }
    }

    fn bin(op: tir::BinOp) -> &'static str {
        match op {
            tir::BinOp::Add => "+",
            tir::BinOp::Sub => "-",
            tir::BinOp::Mul => "*",
            tir::BinOp::Div => "/",
            tir::BinOp::Mod => "%",
            tir::BinOp::Pow => "**",
            tir::BinOp::Eq => "==",
            tir::BinOp::Ne => "!=",
            tir::BinOp::Lt => "<",
            tir::BinOp::Le => "<=",
            tir::BinOp::Gt => ">",
            tir::BinOp::Ge => ">=",
        }
    }

    fn func(&self, id: FuncId) -> String {
        let func = &self.program.funcs[id.0];
        match func.owner {
            Some(owner) => format!("{}.{}", self.program.records[owner.0].name, func.name),
            None => func.name.clone(),
        }
    }

    fn expr(&self, expr: &Expr) -> String {
        match &expr.kind {
            ExprKind::Int(value) => value.to_string(),
            ExprKind::Float(value) => format!("{value:?}"),
            ExprKind::Bool(value) => value.to_string(),
            ExprKind::Str(value) => format!("{value:?}"),
            ExprKind::EnumVariant(index) => {
                let Type::Enum(id) = expr.ty else {
                    panic!("variant of a non-enum type");
                };
                let info = &self.program.enums[id.0];
                format!("{}::{}", info.name, info.variants[*index])
            }
            ExprKind::EntityNone => "Entity.none".to_string(),
            ExprKind::Read(place) => self.place(place),
            ExprKind::Field { base, field } => {
                let Type::Record(record) = base.ty else {
                    panic!("field of a non-record value");
                };
                let name = &self.program.records[record.0].fields[*field].name;
                format!("(field {} {name})", self.expr(base))
            }
            ExprKind::Index { base, index } => {
                format!("(index {} {})", self.expr(base), self.expr(index))
            }
            ExprKind::ListLit(items) => format!("[{}]:{}", self.list(items), self.ty(&expr.ty)),
            ExprKind::Construct(fields) => {
                format!("{}({})", self.ty(&expr.ty), self.list(fields))
            }
            ExprKind::Concat(parts) => {
                let parts: Vec<String> = parts.iter().map(|part| self.expr(part)).collect();
                format!("(concat {})", parts.join(" "))
            }
            ExprKind::Unary { op, expr: inner } => {
                let symbol = match op {
                    tir::UnOp::Neg => "-",
                    tir::UnOp::Not => "!",
                };
                format!("({symbol} {})", self.expr(inner))
            }
            ExprKind::Binary { op, lhs, rhs } => {
                format!("({} {} {})", Self::bin(*op), self.expr(lhs), self.expr(rhs))
            }
            ExprKind::And { lhs, rhs } => format!("(&& {} {})", self.expr(lhs), self.expr(rhs)),
            ExprKind::Or { lhs, rhs } => format!("(|| {} {})", self.expr(lhs), self.expr(rhs)),
            ExprKind::IntToFloat(inner) => format!("(float {})", self.expr(inner)),
            ExprKind::Call { func, args } => format!("{}({})", self.func(*func), self.list(args)),
            ExprKind::Builtin { op, args } => format!("{op:?}({})", self.list(args)),
            ExprKind::If { cond, then, else_ } => format!(
                "(if {} {} {})",
                self.expr(cond),
                self.block(then),
                self.block(else_)
            ),
            ExprKind::Block(block) => format!("(block {})", self.block(block)),
            ExprKind::Spawn(inits) => format!("(spawn {})", self.inits(inits)),
            ExprKind::Add { entity, components } => {
                format!("(add {} {})", self.expr(entity), self.inits(components))
            }
            ExprKind::Remove { entity, components } => {
                let names: Vec<&str> = components.iter().map(|id| self.component(*id)).collect();
                format!("(remove {} {})", self.expr(entity), names.join(", "))
            }
            ExprKind::Despawn(entity) => format!("(despawn {})", self.expr(entity)),
            ExprKind::Has { entity, component } => {
                format!("(has {} {})", self.expr(entity), self.component(*component))
            }
            ExprKind::Alive(entity) => format!("(alive {})", self.expr(entity)),
            ExprKind::ListPush { list, value } => {
                format!("(push {} {})", self.place(list), self.expr(value))
            }
            ExprKind::ListPop { list } => format!("(pop {})", self.place(list)),
            ExprKind::ListDeleteAt { list, index } => {
                format!("(delete_at {} {})", self.place(list), self.expr(index))
            }
            ExprKind::ListClear { list } => format!("(clear {})", self.place(list)),
            ExprKind::WorldTick => "World.tick".to_string(),
            ExprKind::WorldRun => "World.run".to_string(),
            ExprKind::WorldStop => "World.stop".to_string(),
            ExprKind::WorldCount(None) => "World.count".to_string(),
            ExprKind::WorldCount(Some(query)) => format!("(count {})", self.query(query)),
        }
    }

    fn stmt(&self, stmt: &Stmt) -> String {
        match &stmt.kind {
            StmtKind::Expr(expr) => self.expr(expr),
            StmtKind::Let { local, init } => {
                format!("let {} = {}", self.local(*local), self.expr(init))
            }
            StmtKind::Assign { place, value } => {
                format!("{} = {}", self.place(place), self.expr(value))
            }
            StmtKind::CompoundAssign { place, op, value } => {
                let op = match op {
                    tir::CompoundOp::Builtin(op) => format!("{}=", Self::bin(*op)),
                    tir::CompoundOp::Method(func) => format!("<{}>=", self.func(*func)),
                };
                format!("{} {op} {}", self.place(place), self.expr(value))
            }
            StmtKind::Return(None) => "return".to_string(),
            StmtKind::Return(Some(value)) => format!("return {}", self.expr(value)),
            StmtKind::Break => "break".to_string(),
            StmtKind::Next => "next".to_string(),
            StmtKind::While { cond, body } => {
                format!("while {} {}", self.expr(cond), self.block(body))
            }
            StmtKind::Count {
                var,
                from,
                to,
                inclusive,
                down,
                body,
            } => format!(
                "count {} {}{}{}{} {}",
                self.opt_local(*var),
                self.expr(from),
                if *inclusive { "..=" } else { ".." },
                self.expr(to),
                if *down { " down" } else { "" },
                self.block(body)
            ),
            StmtKind::ListEach {
                list,
                elem,
                index,
                body,
            } => format!(
                "for {}, {} in {} {}",
                self.opt_local(*elem),
                self.opt_local(*index),
                self.expr(list),
                self.block(body)
            ),
            StmtKind::Each {
                query,
                params,
                body,
                ..
            } => {
                let params: Vec<&str> = params.iter().map(|id| self.local(*id)).collect();
                format!(
                    "each ({}) |{}| {}",
                    self.query(query),
                    params.join(", "),
                    self.block(body)
                )
            }
            StmtKind::On { event, param, body } => format!(
                "on {} |{}| {}",
                self.program.events[event.0].name,
                self.opt_local(*param),
                self.block(body)
            ),
            StmtKind::Puts(args) => format!("puts {}", self.list(args)).trim_end().to_string(),
            StmtKind::Print(args) => format!("print {}", self.list(args)),
            StmtKind::Emit { event, value } => format!(
                "emit {} {}",
                self.program.events[event.0].name,
                self.expr(value)
            ),
        }
    }
}

fn dump_body(program: &tir::Program, body: &tir::Body) -> Vec<String> {
    let dump = Dump {
        program,
        locals: &body.locals,
    };
    let mut parts: Vec<String> = body.block.stmts.iter().map(|s| dump.stmt(s)).collect();
    if let Some(value) = &body.block.value {
        parts.push(format!("=> {}", dump.expr(value)));
    }
    parts
}

/// The top-level statements of `text`, printed.
#[track_caller]
fn main_stmts(text: &str) -> Vec<String> {
    let program = ok(text);
    dump_body(&program, &program.main)
}

/// The last top-level statement of `text`, printed.
#[track_caller]
fn last(text: &str) -> String {
    main_stmts(text).pop().expect("a top-level statement")
}

/// A function or method of `text` as `name(param: Type, …) -> Type { … }`.
/// Methods are named `Owner.name`.
#[track_caller]
fn func(text: &str, name: &str) -> String {
    let program = ok(text);
    let dump = Dump {
        program: &program,
        locals: &[],
    };
    let (id, found) = program
        .funcs
        .iter()
        .enumerate()
        .find(|(id, _)| dump.func(FuncId(*id)) == name)
        .unwrap_or_else(|| panic!("no function `{name}`"));
    let params: Vec<String> = found
        .params
        .iter()
        .map(|local| {
            let local = &found.body.locals[local.0];
            format!("{}: {}", local.name, dump.ty(&local.ty))
        })
        .collect();
    format!(
        "{}({}) -> {} {{ {} }}",
        dump.func(FuncId(id)),
        params.join(", "),
        dump.ty(&found.ret),
        dump_body(&program, &found.body).join("; ")
    )
}

/// The statements of a system of `text`, printed.
#[track_caller]
fn system(text: &str, name: &str) -> Vec<String> {
    let program = ok(text);
    let found = program
        .systems
        .iter()
        .find(|s| s.name == name)
        .unwrap_or_else(|| panic!("no system `{name}`"));
    dump_body(&program, &found.body)
}
