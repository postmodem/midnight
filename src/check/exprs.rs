//! Expressions: literals, names, calls, constructors, operators, and the
//! `if`, `case` and conditional forms.

use crate::ast;
use crate::diag::Diagnostic;
use crate::source::Span;
use crate::tir::{self, BinOp, ExprKind, FuncId, RecordId, Type};

use super::decls::DefaultValue;
use super::stmts::Tail;
use super::{
    BUILTIN_FUNCTIONS, BUILTIN_NAMES, BodyKind, Checker, ConstUse, FnSym, FuncState, Global, Mode,
    PlaceRef, ReadOnly, Resolved, RetState, int_to_float, is_number, mk, read, util,
};

/// One branch of an `if`, a `case` or a conditional, checked.
pub(super) struct Branch {
    pub(super) block: tir::Block,
    pub(super) tail: Tail,
    /// Control never reaches the end of the branch.
    pub(super) diverges: bool,
    /// Where to report that the branch has no value.
    pub(super) span: Span,
}

/// Whether an expression has one of the forms a constant expression may
/// have. Its parts are judged when they are checked in turn.
fn constant_form(expr: &ast::Expr) -> bool {
    match &expr.kind {
        ast::ExprKind::Int(_)
        | ast::ExprKind::Float(_)
        | ast::ExprKind::Bool(_)
        | ast::ExprKind::Str(_)
        | ast::ExprKind::List(_)
        | ast::ExprKind::Const(_)
        | ast::ExprKind::Construct { .. }
        | ast::ExprKind::Path { .. }
        | ast::ExprKind::Unary { .. }
        | ast::ExprKind::Binary { .. }
        | ast::ExprKind::Ternary { .. } => true,
        ast::ExprKind::Member {
            recv, block: None, ..
        } => matches!(&recv.kind, ast::ExprKind::Const(name) if name == "Math" || name == "Entity"),
        _ => false,
    }
}

fn is_list_literal(expr: &ast::Expr) -> bool {
    matches!(expr.kind, ast::ExprKind::List(_))
}

fn to_float(expr: tir::Expr) -> tir::Expr {
    if expr.ty == Type::Int {
        int_to_float(expr)
    } else {
        expr
    }
}

fn binary(op: BinOp, lhs: tir::Expr, rhs: tir::Expr, ty: Type, span: Span) -> tir::Expr {
    mk(
        ExprKind::Binary {
            op,
            lhs: Box::new(lhs),
            rhs: Box::new(rhs),
        },
        ty,
        span,
    )
}

pub(super) fn builtin(op: tir::Builtin, args: Vec<tir::Expr>, ty: Type, span: Span) -> tir::Expr {
    mk(ExprKind::Builtin { op, args }, ty, span)
}

/// Whether evaluating an expression can't be observed: it has no effects
/// and can't fail.
fn is_pure(expr: &tir::Expr) -> bool {
    fn pure_place(place: &tir::Place) -> bool {
        match &place.kind {
            tir::PlaceKind::Local(_)
            | tir::PlaceKind::Const(_)
            | tir::PlaceKind::ResourceField(..)
            | tir::PlaceKind::Time(_) => true,
            tir::PlaceKind::Field { base, .. } => pure_place(base),
            tir::PlaceKind::Component { .. } | tir::PlaceKind::Index { .. } => false,
        }
    }
    match &expr.kind {
        ExprKind::Int(_)
        | ExprKind::Float(_)
        | ExprKind::Bool(_)
        | ExprKind::Str(_)
        | ExprKind::EnumVariant(_)
        | ExprKind::EntityNone => true,
        ExprKind::Read(place) => pure_place(place),
        ExprKind::IntToFloat(inner) | ExprKind::Unary { expr: inner, .. } => is_pure(inner),
        ExprKind::Field { base, .. } => is_pure(base),
        ExprKind::ListLit(items) | ExprKind::Construct(items) | ExprKind::Concat(items) => {
            items.iter().all(is_pure)
        }
        _ => false,
    }
}

/// Moves a block's value, if it has one, into its statements.
fn demote(mut block: tir::Block) -> tir::Block {
    if let Some(value) = block.value.take() {
        let span = value.span;
        block.stmts.push(tir::Stmt {
            kind: tir::StmtKind::Expr(*value),
            span,
        });
    }
    block
}

impl Checker<'_> {
    /// Checks an expression. In `Value` mode the result may still be of
    /// type `Void`; use [`Checker::value`] where a value is required.
    /// `expected` is the type the result will be converted to, when the
    /// destination has one: list literals and branches take their type
    /// from it.
    pub(super) fn expr(
        &mut self,
        e: &ast::Expr,
        mode: Mode,
        expected: Option<&Type>,
    ) -> Option<tir::Expr> {
        if self.cx.const_use().is_some() && !constant_form(e) {
            let is_function = |name: &str| {
                matches!(self.fn_names.get(name), Some((FnSym::Func(_), _)))
                    || BUILTIN_FUNCTIONS.contains(&name)
            };
            let offender = match &e.kind {
                ast::ExprKind::Var(name) if self.top_vars.contains(name) => {
                    Some(format!("`{name}` is a variable"))
                }
                ast::ExprKind::Var(name) if is_function(name) => {
                    Some(format!("`{name}` is a function call"))
                }
                ast::ExprKind::Call { name, .. } => {
                    Some(format!("`{}(…)` is a function call", name.name))
                }
                _ => None,
            };
            self.not_constant_because(e.span, offender);
            return None;
        }
        let span = e.span;
        match &e.kind {
            ast::ExprKind::Int(value) => Some(mk(ExprKind::Int(*value), Type::Int, span)),
            ast::ExprKind::Float(value) => Some(mk(ExprKind::Float(*value), Type::Float, span)),
            ast::ExprKind::Bool(value) => Some(mk(ExprKind::Bool(*value), Type::Bool, span)),
            ast::ExprKind::Str(parts) => self.string(parts, span),
            ast::ExprKind::List(items) => self.list(items, expected, span),
            ast::ExprKind::SelfRef
            | ast::ExprKind::Var(_)
            | ast::ExprKind::Const(_)
            | ast::ExprKind::Member { .. }
            | ast::ExprKind::Index { .. } => self.resolve(e, mode).map(Resolved::into_value),
            ast::ExprKind::Call { name, args } => self.call(name, args, span, mode),
            ast::ExprKind::Construct { name, args } => self.construct(name, args, span),
            ast::ExprKind::Path { recv, name } => self.path(recv, name, span),
            ast::ExprKind::Unary { op, expr } => self.unary(*op, expr, span),
            ast::ExprKind::Binary { op, lhs, rhs } => self.binary(*op, lhs, rhs, expected, span),
            ast::ExprKind::Ternary { cond, then, else_ } => {
                self.ternary(cond, then, else_, mode, expected, span)
            }
            ast::ExprKind::Spawn(args) => {
                let components = self.component_inits(args.iter())?;
                Some(mk(ExprKind::Spawn(components), Type::Entity, span))
            }
            ast::ExprKind::If { .. } => self.if_expr(e, mode, expected),
            ast::ExprKind::Case { .. } => self.case_expr(e, mode, expected),
        }
    }

    /// Checks an expression that may name a place, without reading it.
    pub(super) fn resolve(&mut self, e: &ast::Expr, mode: Mode) -> Option<Resolved> {
        let span = e.span;
        match &e.kind {
            ast::ExprKind::SelfRef => match self.cx.self_ {
                Some((local, record)) => Some(Resolved::Place(PlaceRef {
                    place: tir::Place {
                        kind: tir::PlaceKind::Local(local),
                        ty: Type::Record(record),
                        span,
                    },
                    readonly: Some(ReadOnly::SelfValue),
                })),
                None => {
                    self.err_label(
                        "`self` can only be used in a method",
                        span,
                        "there is no receiver here",
                    );
                    None
                }
            },
            ast::ExprKind::Var(name) => self.var(name, span, mode),
            ast::ExprKind::Const(name) => self.const_name(name, span),
            ast::ExprKind::Member {
                recv,
                name,
                args,
                block,
            } => {
                if block.is_some() {
                    self.stray_block(name);
                    return None;
                }
                self.member(recv, name, args.as_deref(), span, mode)
            }
            ast::ExprKind::Index { recv, index } => self.index(recv, index, span),
            _ => self.expr(e, mode, None).map(Resolved::Value),
        }
    }

    /// Checks an expression whose value is needed.
    pub(super) fn value(&mut self, e: &ast::Expr, expected: Option<&Type>) -> Option<tir::Expr> {
        let expr = self.expr(e, Mode::Value, expected)?;
        if expr.ty == Type::Void {
            self.no_value(&expr);
            return None;
        }
        Some(expr)
    }

    /// Checks an expression and converts its value for storing into
    /// something of type `to`, a storage type.
    pub(super) fn store(&mut self, e: &ast::Expr, to: &Type) -> Option<tir::Expr> {
        let expr = self.value(e, Some(to))?;
        self.convert(expr, to, None)
    }

    /// As [`Checker::store`]; when the destination's type is unknown the
    /// expression is only checked for errors of its own.
    pub(super) fn store_opt(&mut self, e: &ast::Expr, to: Option<&Type>) -> Option<tir::Expr> {
        match to {
            Some(to) => self.store(e, to),
            None => {
                self.probe(e);
                None
            }
        }
    }

    /// Checks an expression for errors of its own when what it feeds into
    /// has already failed. An empty list literal is let through, since its
    /// type would have come from there.
    pub(super) fn probe(&mut self, e: &ast::Expr) {
        if !matches!(&e.kind, ast::ExprKind::List(items) if items.is_empty()) {
            self.value(e, None);
        }
    }

    /// Converts a value to the value type of `to`, a storage type: an
    /// `Int` becomes a `Float` where one is wanted, and a constant that
    /// does not fit a compact integer type is rejected.
    pub(super) fn convert(
        &mut self,
        expr: tir::Expr,
        to: &Type,
        label: Option<String>,
    ) -> Option<tir::Expr> {
        let want = to.value_type();
        let expr = if expr.ty == want {
            expr
        } else if expr.ty == Type::Int && want == Type::Float {
            int_to_float(expr)
        } else {
            let label = label.or_else(|| {
                (matches!(want, Type::List(_)) && matches!(expr.ty, Type::List(_))).then(|| {
                    "list types do not convert; a list literal takes its type from where it is used"
                        .to_string()
                })
            });
            self.mismatch(&want, &expr.ty, expr.span, label);
            return None;
        };
        if let Type::Compact(compact) = to
            && let Some((low, high)) = compact.int_range()
            && let Some(value) = self.const_int(&expr)
            && !(low..=high).contains(&value)
        {
            let name = compact.name();
            self.err_label(
                format!("{value} does not fit in `{name}`"),
                expr.span,
                format!("`{name}` holds {low} to {high}"),
            );
            return None;
        }
        Some(expr)
    }

    /// Checks a condition, which must be a `Bool`. With `negate` the
    /// result is its opposite.
    pub(super) fn condition(&mut self, cond: &ast::Expr, negate: bool) -> Option<tir::Expr> {
        let value = self.value(cond, None)?;
        if value.ty != Type::Bool {
            let found = self.type_name(&value.ty);
            let mut diagnostic = Diagnostic::new(
                format!("condition must be `Bool`, found `{found}`"),
                value.span,
            );
            let hint = match &value.ty {
                Type::Int | Type::Float => Some("compare it, like `x != 0`"),
                Type::String | Type::List(_) => Some("test it, like `!x.empty?`"),
                Type::Entity => Some("test it, like `e.alive?`"),
                _ => None,
            };
            if let Some(hint) = hint {
                diagnostic = diagnostic.label(format!("there is no truthiness: {hint}"));
            }
            self.error(diagnostic);
            return None;
        }
        Some(if negate { self.not(value) } else { value })
    }

    fn not(&self, value: tir::Expr) -> tir::Expr {
        let span = value.span;
        mk(
            ExprKind::Unary {
                op: tir::UnOp::Not,
                expr: Box::new(value),
            },
            Type::Bool,
            span,
        )
    }

    fn no_value(&mut self, expr: &tir::Expr) {
        let label = match &expr.kind {
            ExprKind::Call { func, .. } => {
                let func = &self.funcs[func.0];
                Some(if func.void_by_recursion {
                    format!(
                        "`{}` is recursive: it returns a value only with a declared return type",
                        func.name
                    )
                } else {
                    format!("`{}` returns nothing", func.name)
                })
            }
            ExprKind::ListPush { .. } => Some("`push` returns nothing".to_string()),
            ExprKind::ListClear { .. } => Some("`clear` returns nothing".to_string()),
            ExprKind::Add { .. } => Some("`add` returns nothing".to_string()),
            ExprKind::Remove { .. } => Some("`remove` returns nothing".to_string()),
            ExprKind::Despawn(_) => Some("`despawn` returns nothing".to_string()),
            ExprKind::WorldTick | ExprKind::WorldRun | ExprKind::WorldStop => {
                Some("this returns nothing".to_string())
            }
            ExprKind::Builtin { .. } => Some("this built-in returns nothing".to_string()),
            ExprKind::If { .. } | ExprKind::Block(_) => {
                Some("every branch leaves before it ends".to_string())
            }
            _ => None,
        };
        let mut diagnostic = Diagnostic::new("this expression has no value", expr.span);
        if let Some(label) = label {
            diagnostic = diagnostic.label(label);
        }
        self.error(diagnostic);
    }

    pub(super) fn not_constant(&mut self, span: Span) {
        self.not_constant_because(span, None);
    }

    /// Reports the first part of a constant expression that is not
    /// constant. `offender` says what it is: "`size` is a variable".
    fn not_constant_because(&mut self, span: Span, offender: Option<String>) {
        if self.cx.const_reported {
            return;
        }
        self.cx.const_reported = true;
        let what = match self.cx.const_use() {
            Some(ConstUse::Constant(_)) => "a constant's value",
            _ => "a field default",
        };
        let allowed = format!(
            "{what} can use literals, constants, enum variants, operators, constructors, list literals and `Math` functions"
        );
        let label = match offender {
            Some(offender) => format!("{offender}; {allowed}"),
            None => allowed,
        };
        self.err_label("not a constant expression", span, label);
    }

    pub(super) fn stray_block(&mut self, name: &ast::Ident) {
        let method = name.name.as_str();
        if matches!(
            method,
            "times" | "upto" | "downto" | "each" | "each_with_index"
        ) {
            self.err_label(
                format!("a `{method}` loop has no value"),
                name.span,
                "a loop is a statement",
            );
        } else {
            self.err_label(
                format!("`{method}` does not take a `do` block"),
                name.span,
                "only `times`, `upto`, `downto`, `each` and `each_with_index` take one",
            );
        }
    }

    // ----- literals -----

    fn string(&mut self, parts: &[ast::StrPart], span: Span) -> Option<tir::Expr> {
        if parts.iter().all(|p| matches!(p, ast::StrPart::Lit(_))) {
            let text: String = parts
                .iter()
                .map(|p| match p {
                    ast::StrPart::Lit(text) => text.as_str(),
                    ast::StrPart::Expr(_) => "",
                })
                .collect();
            return Some(mk(ExprKind::Str(text), Type::String, span));
        }
        let mut out = Vec::new();
        let mut failed = false;
        for part in parts {
            match part {
                ast::StrPart::Lit(text) => {
                    out.push(mk(ExprKind::Str(text.clone()), Type::String, span));
                }
                ast::StrPart::Expr(inner) => match self.value(inner, None) {
                    Some(value) if value.ty == Type::String => out.push(value),
                    Some(value) => {
                        let part_span = value.span;
                        out.push(builtin(
                            tir::Builtin::ToS,
                            vec![value],
                            Type::String,
                            part_span,
                        ));
                    }
                    None => failed = true,
                },
            }
        }
        (!failed).then(|| mk(ExprKind::Concat(out), Type::String, span))
    }

    fn list(
        &mut self,
        items: &[ast::Expr],
        expected: Option<&Type>,
        span: Span,
    ) -> Option<tir::Expr> {
        if let Some(Type::List(elem)) = expected {
            let values: Vec<Option<tir::Expr>> =
                items.iter().map(|item| self.store(item, elem)).collect();
            let values: Option<Vec<tir::Expr>> = values.into_iter().collect();
            return Some(mk(
                ExprKind::ListLit(values?),
                Type::List(elem.clone()),
                span,
            ));
        }
        if items.is_empty() {
            match expected {
                Some(other) => {
                    let message = format!("expected `{}`, found `[]`", self.type_name(other));
                    self.err(message, span);
                }
                None => self.cant_infer(span),
            }
            return None;
        }

        // Elements that have a type of their own come first; an empty
        // list among them takes the type they settle on.
        let is_empty =
            |item: &ast::Expr| matches!(&item.kind, ast::ExprKind::List(inner) if inner.is_empty());
        let mut values: Vec<Option<tir::Expr>> = Vec::new();
        let mut failed = false;
        for item in items {
            if is_empty(item) {
                values.push(None);
            } else {
                let value = self.value(item, None);
                failed |= value.is_none();
                values.push(value);
            }
        }
        let mut elem: Option<Type> = None;
        for value in values.iter().flatten() {
            match elem.clone() {
                None => elem = Some(value.ty.clone()),
                Some(ty) if ty == value.ty => {}
                Some(ty) if is_number(&ty) && is_number(&value.ty) => elem = Some(Type::Float),
                Some(ty) => {
                    self.mismatch(
                        &ty,
                        &value.ty,
                        value.span,
                        Some("the elements of a list share one type".to_string()),
                    );
                    failed = true;
                }
            }
        }
        let Some(elem) = elem else {
            if !failed {
                self.cant_infer(span);
            }
            return None;
        };
        if failed {
            return None;
        }
        let mut out = Vec::new();
        for (item, value) in items.iter().zip(values) {
            out.push(match value {
                Some(value) if elem == Type::Float => to_float(value),
                Some(value) => value,
                None => self.store(item, &elem)?,
            });
        }
        Some(mk(ExprKind::ListLit(out), Type::List(Box::new(elem)), span))
    }

    fn cant_infer(&mut self, span: Span) {
        self.err_label(
            "can't infer the element type of `[]`",
            span,
            "give it a type, like `xs: [Int] = []`",
        );
    }

    // ----- names -----

    /// A bare lowercase name: a local, then a field or a method of `self`,
    /// then a function called with no arguments.
    fn var(&mut self, name: &str, span: Span, mode: Mode) -> Option<Resolved> {
        if let Some(id) = self.cx.lookup(name) {
            let meta = &self.cx.meta[id.0];
            if meta.poisoned {
                return None;
            }
            return Some(Resolved::Place(PlaceRef {
                place: tir::Place {
                    kind: tir::PlaceKind::Local(id),
                    ty: self.cx.locals[id.0].ty.clone(),
                    span,
                },
                readonly: meta.readonly.clone(),
            }));
        }
        if let Some((self_local, record)) = self.cx.self_ {
            let receiver = tir::Place {
                kind: tir::PlaceKind::Local(self_local),
                ty: Type::Record(record),
                span,
            };
            if let Some(field) = self.field_index(record, name) {
                let ty = self.records[record.0].fields[field].ty.clone()?;
                return Some(Resolved::Place(PlaceRef {
                    place: tir::Place {
                        kind: tir::PlaceKind::Field {
                            base: Box::new(receiver),
                            field,
                        },
                        ty,
                        span,
                    },
                    readonly: Some(ReadOnly::SelfField(name.to_string())),
                }));
            }
            if let Some(func) = self.method(record, name)
                && self.funcs[func.0].params.is_empty()
            {
                return self
                    .call_func(func, Some(read(receiver)), name, &[], span, mode)
                    .map(Resolved::Value);
            }
        }
        match self.fn_names.get(name).map(|(sym, _)| *sym) {
            Some(FnSym::Func(func)) => {
                return self
                    .call_func(func, None, name, &[], span, mode)
                    .map(Resolved::Value);
            }
            Some(FnSym::System) => {
                self.system_call(name, span);
                return None;
            }
            None => {}
        }
        if BUILTIN_FUNCTIONS.contains(&name) {
            return self.builtin_call(name, &[], span).map(Resolved::Value);
        }

        let mut candidates = self.cx.names();
        let mut self_method = false;
        if let Some((_, record)) = self.cx.self_ {
            let info = &self.records[record.0];
            candidates.extend(info.fields.iter().map(|f| f.name.clone()));
            candidates.extend(util::named_methods(&info.methods));
            self_method = self.method(record, name).is_some();
        }
        candidates.extend(self.function_names());
        let mut diagnostic =
            Diagnostic::new(format!("unknown variable or function `{name}`"), span);
        let ended = self
            .cx
            .ended
            .iter()
            .rev()
            .find(|ended| ended.name == name)
            .map(|ended| (ended.construct, &self.cx.meta[ended.local.0]));
        if let Some((construct, meta)) = ended {
            // The name was a variable of a block that has ended.
            diagnostic = if meta.block_param {
                diagnostic.label(format!(
                    "`{name}` is a block parameter and ends with its block"
                ))
            } else {
                diagnostic
                    .label(format!(
                        "`{name}` was created inside {construct} and ends with it; create it before {construct}"
                    ))
                    .note(format!("`{name}` is created here"), meta.span)
            };
        } else if self_method {
            diagnostic = diagnostic.label(format!(
                "the method takes arguments: write `self.{name}(…)`"
            ));
        } else if let Some(close) = util::closest(name, candidates.iter().map(String::as_str)) {
            diagnostic = diagnostic.label(format!("did you mean `{close}`?"));
        } else if self.top_vars.contains(name) && self.cx.kind != BodyKind::Top {
            diagnostic = diagnostic.label(
                "top-level variables can't be seen from functions and systems; use a resource",
            );
        }
        self.error(diagnostic);
        None
    }

    /// Names of user functions and built-in functions.
    fn function_names(&self) -> Vec<String> {
        self.fn_names
            .iter()
            .filter(|(_, (sym, _))| matches!(sym, FnSym::Func(_)))
            .map(|(name, _)| name.clone())
            .chain(BUILTIN_FUNCTIONS.iter().map(|n| n.to_string()))
            .collect()
    }

    fn system_call(&mut self, name: &str, span: Span) {
        self.err_label(
            format!("`{name}` is a system and can't be called"),
            span,
            "systems run when `World.tick` is called",
        );
    }

    /// A bare capitalized name used as a value: only a constant is one.
    fn const_name(&mut self, name: &str, span: Span) -> Option<Resolved> {
        let global = match self.global(name) {
            Some(Global::Const(id)) => return self.constant(id, span).map(Resolved::Place),
            Some(global) => global,
            None => {
                if BUILTIN_NAMES.contains(&name) {
                    let label = match name {
                        "World" | "Time" | "Math" => format!("use its members, like `{name}.…`"),
                        _ => "a type is not a value".to_string(),
                    };
                    self.err_label(format!("`{name}` is not a value"), span, label);
                } else {
                    let candidates: Vec<String> = self.globals.keys().cloned().collect();
                    self.unknown(
                        format!("unknown constant `{name}`"),
                        span,
                        name,
                        &candidates,
                    );
                }
                return None;
            }
        };
        let kind = self.global_kind(global);
        let (message, label) = match global {
            Global::Record(_) => (
                format!("`{name}` is {kind}, not a value"),
                format!("construct one with `{name}(…)`"),
            ),
            Global::Enum(id) => (
                format!("`{name}` is an enum, not a value"),
                match self.enums[id.0].variants.first() {
                    Some(variant) => format!("write a variant, like `{name}::{variant}`"),
                    None => "write one of its variants".to_string(),
                },
            ),
            Global::Tag(_) => (
                format!("`{name}` is a tag and has no value"),
                "a tag can be attached, removed, tested with `has?` and used to filter queries"
                    .to_string(),
            ),
            _ => (
                format!("resource `{name}` is not a value"),
                format!("read and write its fields, like `{name}.field`"),
            ),
        };
        self.err_label(message, span, label);
        None
    }

    // ----- calls -----

    /// Reports keyword arguments, which only constructors and
    /// `World.count` take.
    pub(super) fn no_keywords(&mut self, name: &str, args: &[ast::Arg]) {
        for arg in args {
            if let Some(keyword) = &arg.name {
                self.err_label(
                    format!("`{name}` takes no keyword arguments"),
                    keyword.span,
                    "pass the value by position",
                );
            }
        }
    }

    /// Reports a wrong number of arguments; returns whether it was right.
    pub(super) fn arity(&mut self, name: &str, want: usize, got: usize, span: Span) -> bool {
        if want == got {
            return true;
        }
        self.err(
            format!(
                "`{name}` takes {}, but {}",
                util::count(want, "argument"),
                util::were_given(got)
            ),
            span,
        );
        false
    }

    /// Reports too many arguments to something that takes up to `most`.
    pub(super) fn at_most(&mut self, name: &str, most: usize, got: usize, span: Span) {
        self.err(
            format!(
                "`{name}` takes at most {}, but {}",
                util::count(most, "argument"),
                util::were_given(got)
            ),
            span,
        );
    }

    /// `name(args)`: a user function, then a built-in one.
    fn call(
        &mut self,
        name: &ast::Ident,
        args: &[ast::Arg],
        span: Span,
        mode: Mode,
    ) -> Option<tir::Expr> {
        match self.fn_names.get(&name.name).map(|(sym, _)| *sym) {
            Some(FnSym::Func(func)) => {
                return self.call_func(func, None, &name.name, args, span, mode);
            }
            Some(FnSym::System) => {
                self.system_call(&name.name, name.span);
                return None;
            }
            None => {}
        }
        if BUILTIN_FUNCTIONS.contains(&name.name.as_str()) {
            return self.builtin_call(&name.name, args, span);
        }
        let candidates = self.function_names();
        let mut diagnostic =
            Diagnostic::new(format!("unknown function `{}`", name.name), name.span);
        if self.cx.lookup(&name.name).is_some() {
            diagnostic = diagnostic.label(format!(
                "`{0}` is a variable; `{0}(…)` always calls a function",
                name.name
            ));
        } else if let Some((_, record)) = self.cx.self_
            && self.method(record, &name.name).is_some()
        {
            diagnostic = diagnostic.label(format!(
                "`{0}(…)` is always a function call; the method is `self.{0}(…)`",
                name.name
            ));
        } else if let Some(close) = util::closest(&name.name, candidates.iter().map(String::as_str))
        {
            diagnostic = diagnostic.label(format!("did you mean `{close}`?"));
        }
        self.error(diagnostic);
        for arg in args {
            self.probe(&arg.value);
        }
        None
    }

    /// A call of a user function or method. `recv` is the receiver of a
    /// method, already checked.
    pub(super) fn call_func(
        &mut self,
        func: FuncId,
        recv: Option<tir::Expr>,
        name: &str,
        args: &[ast::Arg],
        span: Span,
        mode: Mode,
    ) -> Option<tir::Expr> {
        if self.cx.const_use().is_some() {
            self.not_constant(span);
            return None;
        }
        let params: Vec<Option<Type>> = self.funcs[func.0]
            .params
            .iter()
            .map(|p| p.ty.clone())
            .collect();
        self.no_keywords(name, args);
        let arity_ok = self.arity(name, params.len(), args.len(), span);
        let mut values: Vec<Option<tir::Expr>> = recv.into_iter().map(Some).collect();
        for (i, arg) in args.iter().enumerate() {
            values.push(match params.get(i) {
                Some(ty) => self.store_opt(&arg.value, ty.as_ref()),
                None => {
                    self.probe(&arg.value);
                    None
                }
            });
        }
        let ret = self.ret_type(func, span, mode);
        if !arity_ok {
            return None;
        }
        let values: Option<Vec<tir::Expr>> = values.into_iter().collect();
        Some(mk(
            ExprKind::Call {
                func,
                args: values?,
            },
            ret?,
            span,
        ))
    }

    /// The return type of a function, inferring it now if it is neither
    /// declared nor known yet. A call back into a function that is being
    /// inferred is allowed where no value is needed.
    pub(super) fn ret_type(&mut self, func: FuncId, span: Span, mode: Mode) -> Option<Type> {
        if let RetState::Known(ty) = &self.funcs[func.0].ret {
            return ty.clone();
        }
        match self.funcs[func.0].state {
            FuncState::Pending => {
                self.check_func(func);
                match &self.funcs[func.0].ret {
                    RetState::Known(ty) => ty.clone(),
                    RetState::Unknown => None,
                }
            }
            FuncState::InProgress => match mode {
                Mode::Value => {
                    self.recursion_error(func, span);
                    None
                }
                Mode::Stmt | Mode::Maybe => {
                    self.deferred_calls = true;
                    if mode == Mode::Maybe {
                        self.cx.recursive_tail = Some((func, span));
                    }
                    Some(Type::Void)
                }
            },
            FuncState::Done => None,
        }
    }

    /// A call of a built-in function. `name` is one of
    /// [`BUILTIN_FUNCTIONS`].
    pub(super) fn builtin_call(
        &mut self,
        name: &str,
        args: &[ast::Arg],
        span: Span,
    ) -> Option<tir::Expr> {
        use tir::Builtin;
        self.no_keywords(name, args);
        match (name, args) {
            ("min" | "max", _) => {
                let arity_ok = self.arity(name, 2, args.len(), span);
                let values: Vec<Option<tir::Expr>> =
                    args.iter().map(|a| self.value(&a.value, None)).collect();
                if !arity_ok {
                    return None;
                }
                let values: Option<Vec<tir::Expr>> = values.into_iter().collect();
                let (values, float) = self.numbers(values?)?;
                let op = match (name, float) {
                    ("min", false) => Builtin::IntMin,
                    ("min", true) => Builtin::FloatMin,
                    (_, false) => Builtin::IntMax,
                    (_, true) => Builtin::FloatMax,
                };
                let ty = if float { Type::Float } else { Type::Int };
                Some(builtin(op, values, ty, span))
            }
            ("rand", []) => Some(builtin(Builtin::Rand, Vec::new(), Type::Float, span)),
            ("rand", [bound]) => {
                let bound = self.store(&bound.value, &Type::Int)?;
                Some(builtin(Builtin::RandInt, vec![bound], Type::Int, span))
            }
            ("srand", []) => Some(builtin(Builtin::SrandClock, Vec::new(), Type::Void, span)),
            ("srand", [seed]) => {
                let seed = self.store(&seed.value, &Type::Int)?;
                Some(builtin(Builtin::Srand, vec![seed], Type::Void, span))
            }
            ("clock", []) => Some(builtin(Builtin::Clock, Vec::new(), Type::Float, span)),
            ("sleep", [seconds]) => {
                let seconds = self.store(&seconds.value, &Type::Float)?;
                Some(builtin(Builtin::Sleep, vec![seconds], Type::Void, span))
            }
            ("exit", [] | [_]) => {
                let status = match args {
                    [status] => self.store(&status.value, &Type::Int),
                    _ => Some(mk(ExprKind::Int(0), Type::Int, span)),
                };
                self.cx.unreachable = true;
                Some(builtin(Builtin::Exit, vec![status?], Type::Void, span))
            }
            _ => {
                match name {
                    "clock" => {
                        self.arity(name, 0, args.len(), span);
                    }
                    "sleep" => {
                        self.arity(name, 1, args.len(), span);
                    }
                    _ => self.at_most(name, 1, args.len(), span),
                }
                for arg in args {
                    self.probe(&arg.value);
                }
                None
            }
        }
    }

    /// Requires every value to be a number; when any is a `Float`, the
    /// `Int`s among them are converted. Returns whether they are `Float`s.
    pub(super) fn numbers(&mut self, values: Vec<tir::Expr>) -> Option<(Vec<tir::Expr>, bool)> {
        let mut ok = true;
        for value in &values {
            if !is_number(&value.ty) {
                self.expected_number(&value.ty, value.span, None);
                ok = false;
            }
        }
        if !ok {
            return None;
        }
        let float = values.iter().any(|v| v.ty == Type::Float);
        let values = if float {
            values.into_iter().map(to_float).collect()
        } else {
            values
        };
        Some((values, float))
    }

    fn expected_number(&mut self, found: &Type, span: Span, label: Option<String>) {
        let found = self.type_name(found);
        let mut diagnostic =
            Diagnostic::new(format!("expected `Int` or `Float`, found `{found}`"), span);
        if let Some(label) = label {
            diagnostic = diagnostic.label(label);
        }
        self.error(diagnostic);
    }

    // ----- constructors -----

    /// Every field of a record from its default, for a bare `spawn Health`
    /// or `emit Tick`. `Err` lists the fields that have no default.
    pub(super) fn construct_defaults(
        &mut self,
        record: RecordId,
        span: Span,
    ) -> Result<Option<tir::Expr>, Vec<String>> {
        let mut fields = Vec::new();
        let mut missing = Vec::new();
        let mut failed = false;
        for i in 0..self.records[record.0].fields.len() {
            match self.field_default(record, i) {
                DefaultValue::Value(value) => fields.push(value),
                DefaultValue::Poison => failed = true,
                DefaultValue::Absent => missing.push(self.records[record.0].fields[i].name.clone()),
            }
        }
        if !missing.is_empty() {
            return Err(missing);
        }
        Ok((!failed).then(|| mk(ExprKind::Construct(fields), Type::Record(record), span)))
    }

    fn construct(&mut self, name: &ast::Ident, args: &[ast::Arg], span: Span) -> Option<tir::Expr> {
        let record = match self.global(&name.name) {
            Some(Global::Record(record)) => record,
            other => {
                self.not_constructible(name, other);
                for arg in args {
                    self.probe(&arg.value);
                }
                return None;
            }
        };
        let type_name = self.records[record.0].name.clone();
        let field_count = self.records[record.0].fields.len();
        let named = args.iter().filter(|a| a.name.is_some()).count();
        if named != 0 && named != args.len() {
            let first_named = args[0].name.is_some();
            let odd = args
                .iter()
                .find(|a| a.name.is_some() != first_named)
                .map_or(span, |a| a.name.as_ref().map_or(a.value.span, |n| n.span));
            self.err_label(
                "positional and named fields can't be mixed",
                odd,
                format!("give every field of `{type_name}` by position, or every one by name"),
            );
            for arg in args {
                self.probe(&arg.value);
            }
            return None;
        }

        // One slot per field: not given, given but failed, or given.
        let mut slots: Vec<Option<Option<tir::Expr>>> = (0..field_count).map(|_| None).collect();
        // The fields that were given, in the order they were written.
        let mut order: Vec<usize> = Vec::new();
        let mut ok = true;
        if named == 0 {
            if args.len() > field_count {
                self.err(
                    format!(
                        "`{type_name}` has {}, but {}",
                        util::count(field_count, "field"),
                        util::were_given(args.len())
                    ),
                    span,
                );
                ok = false;
            }
            for (i, arg) in args.iter().enumerate() {
                if i < field_count {
                    let ty = self.records[record.0].fields[i].ty.clone();
                    slots[i] = Some(self.store_opt(&arg.value, ty.as_ref()));
                    order.push(i);
                } else {
                    self.probe(&arg.value);
                }
            }
        } else {
            for arg in args {
                let Some(keyword) = &arg.name else { continue };
                match self.field_index(record, &keyword.name) {
                    None => {
                        let candidates: Vec<String> = self.records[record.0]
                            .fields
                            .iter()
                            .map(|f| f.name.clone())
                            .collect();
                        self.unknown(
                            format!("unknown field `{}` for `{type_name}`", keyword.name),
                            keyword.span,
                            &keyword.name,
                            &candidates,
                        );
                        self.probe(&arg.value);
                        ok = false;
                    }
                    Some(i) if slots[i].is_some() => {
                        self.err(
                            format!("field `{}` is given twice", keyword.name),
                            keyword.span,
                        );
                        self.probe(&arg.value);
                        ok = false;
                    }
                    Some(i) => {
                        let ty = self.records[record.0].fields[i].ty.clone();
                        slots[i] = Some(self.store_opt(&arg.value, ty.as_ref()));
                        order.push(i);
                    }
                }
            }
        }

        let mut missing: Vec<String> = Vec::new();
        for (i, slot) in slots.iter_mut().enumerate() {
            if slot.is_some() {
                continue;
            }
            match self.field_default(record, i) {
                DefaultValue::Value(value) => *slot = Some(Some(value)),
                DefaultValue::Poison => *slot = Some(None),
                DefaultValue::Absent => missing.push(self.records[record.0].fields[i].name.clone()),
            }
        }
        // After an unknown keyword, the field it was meant for is missing
        // too; one report is enough.
        if !missing.is_empty() && ok {
            let noun = if missing.len() == 1 {
                "field"
            } else {
                "fields"
            };
            let verb = if missing.len() == 1 { "has" } else { "have" };
            self.err_label(
                format!(
                    "missing {noun} {} in `{type_name}(…)`",
                    util::name_list(&missing)
                ),
                span,
                format!("{} {verb} no default", util::name_list(&missing)),
            );
            ok = false;
        }
        if !ok {
            return None;
        }
        let fields: Option<Vec<tir::Expr>> = slots.into_iter().map(Option::flatten).collect();
        let mut fields = fields?;
        let ty = Type::Record(record);

        // Fields named out of order are evaluated as written, through
        // hidden locals, when the order can be observed.
        let reordered = order.windows(2).any(|pair| pair[0] > pair[1]);
        if reordered && self.cx.const_use().is_none() && order.iter().any(|&i| !is_pure(&fields[i]))
        {
            let mut stmts = Vec::new();
            for &i in &order {
                let value_ty = fields[i].ty.clone();
                let value_span = fields[i].span;
                let name = format!("${}", self.records[record.0].fields[i].name);
                let local = self.hidden_local(&name, value_ty.clone(), tir::LocalKind::Var);
                let place = tir::Place {
                    kind: tir::PlaceKind::Local(local),
                    ty: value_ty,
                    span: value_span,
                };
                let init = std::mem::replace(&mut fields[i], read(place));
                stmts.push(tir::Stmt {
                    kind: tir::StmtKind::Let { local, init },
                    span: value_span,
                });
            }
            let value = mk(ExprKind::Construct(fields), ty.clone(), span);
            let block = tir::Block {
                stmts,
                value: Some(Box::new(value)),
            };
            return Some(mk(ExprKind::Block(block), ty, span));
        }
        Some(mk(ExprKind::Construct(fields), ty, span))
    }

    fn not_constructible(&mut self, name: &ast::Ident, global: Option<Global>) {
        let n = &name.name;
        let (message, label) = match global {
            Some(Global::Tag(_)) => (
                format!("`{n}` is a tag and can't be constructed"),
                Some(format!(
                    "a tag has no value; name it bare, as in `spawn {n}`"
                )),
            ),
            Some(Global::Resource(_)) => (
                format!("`{n}` is a resource and can't be constructed"),
                Some(format!(
                    "a resource is one global instance; use its fields, like `{n}.field`"
                )),
            ),
            Some(Global::Enum(id)) => (
                format!("`{n}` is an enum and can't be constructed"),
                self.enums[id.0]
                    .variants
                    .first()
                    .map(|variant| format!("write a variant, like `{n}::{variant}`")),
            ),
            Some(Global::Const(_)) => (format!("`{n}` is a constant, not a type"), None),
            Some(Global::Record(_)) => return,
            None if BUILTIN_NAMES.contains(&n.as_str()) => (
                format!("`{n}` is built in and has no constructor"),
                Some("only structs, components and events are constructed".to_string()),
            ),
            None => {
                let candidates: Vec<String> = self
                    .globals
                    .iter()
                    .filter(|(_, (global, _))| matches!(global, Global::Record(_)))
                    .map(|(name, _)| name.clone())
                    .collect();
                self.unknown(format!("unknown type `{n}`"), name.span, n, &candidates);
                return;
            }
        };
        let mut diagnostic = Diagnostic::new(message, name.span);
        if let Some(label) = label {
            diagnostic = diagnostic.label(label);
        }
        self.error(diagnostic);
    }

    /// `Enum::Variant`, `Math::PI` and `Math::E`.
    fn path(&mut self, recv: &ast::Expr, name: &ast::Ident, span: Span) -> Option<tir::Expr> {
        let ast::ExprKind::Const(owner) = &recv.kind else {
            self.err_label(
                "`::` needs the name of an enum on its left",
                recv.span,
                "variants are written `Enum::Variant`",
            );
            return None;
        };
        if owner == "Math" {
            let value = match name.name.as_str() {
                "PI" => std::f64::consts::PI,
                "E" => std::f64::consts::E,
                other => {
                    self.unknown(
                        format!("unknown constant `Math::{other}`"),
                        name.span,
                        other,
                        &["PI".to_string(), "E".to_string()],
                    );
                    return None;
                }
            };
            return Some(mk(ExprKind::Float(value), Type::Float, span));
        }
        match self.global(owner) {
            Some(Global::Enum(id)) => {
                let variants = &self.enums[id.0].variants;
                match variants.iter().position(|v| *v == name.name) {
                    Some(index) => Some(mk(ExprKind::EnumVariant(index), Type::Enum(id), span)),
                    None => {
                        let candidates = variants.clone();
                        self.unknown(
                            format!("unknown variant `{}` for enum `{owner}`", name.name),
                            name.span,
                            &name.name,
                            &candidates,
                        );
                        None
                    }
                }
            }
            Some(global) => {
                let kind = self.global_kind(global);
                self.err_label(
                    format!("`{owner}` is {kind}, not an enum"),
                    recv.span,
                    "`::` selects a variant of an enum",
                );
                None
            }
            None if BUILTIN_NAMES.contains(&owner.as_str()) => {
                self.err_label(
                    format!("`{owner}` is not an enum"),
                    recv.span,
                    "`::` selects a variant of an enum",
                );
                None
            }
            None => {
                let candidates: Vec<String> = self
                    .globals
                    .iter()
                    .filter(|(_, (global, _))| matches!(global, Global::Enum(_)))
                    .map(|(name, _)| name.clone())
                    .collect();
                self.unknown(
                    format!("unknown type `{owner}`"),
                    recv.span,
                    owner,
                    &candidates,
                );
                None
            }
        }
    }

    // ----- operators -----

    fn unary(&mut self, op: ast::UnaryOp, operand: &ast::Expr, span: Span) -> Option<tir::Expr> {
        let value = self.value(operand, None)?;
        match op {
            ast::UnaryOp::Not => {
                if value.ty != Type::Bool {
                    self.mismatch(
                        &Type::Bool,
                        &value.ty,
                        value.span,
                        Some("`!` needs a `Bool`".to_string()),
                    );
                    return None;
                }
                Some(self.not(value))
            }
            ast::UnaryOp::Neg => match value.ty.clone() {
                ty @ (Type::Int | Type::Float) => Some(mk(
                    ExprKind::Unary {
                        op: tir::UnOp::Neg,
                        expr: Box::new(value),
                    },
                    ty,
                    span,
                )),
                Type::Record(record) => {
                    let Some(func) = self.method(record, "-@") else {
                        let name = self.records[record.0].name.clone();
                        self.err_label(
                            format!("`{name}` has no unary `-`"),
                            span,
                            format!("define it in `{name}` with `def -@`"),
                        );
                        return None;
                    };
                    if self.cx.const_use().is_some() {
                        self.not_constant(span);
                        return None;
                    }
                    let ret = self.ret_type(func, span, Mode::Value)?;
                    Some(mk(
                        ExprKind::Call {
                            func,
                            args: vec![value],
                        },
                        ret,
                        span,
                    ))
                }
                other => {
                    let name = self.type_name(&other);
                    self.err(format!("`{name}` has no unary `-`"), span);
                    None
                }
            },
        }
    }

    fn binary(
        &mut self,
        op: ast::BinaryOp,
        lhs: &ast::Expr,
        rhs: &ast::Expr,
        expected: Option<&Type>,
        span: Span,
    ) -> Option<tir::Expr> {
        use ast::BinaryOp as Op;
        match op {
            Op::And | Op::Or => {
                let symbol = util::binary_symbol(op);
                let lhs = self.bool_operand(lhs, symbol);
                let rhs = self.bool_operand(rhs, symbol);
                let (lhs, rhs) = (Box::new(lhs?), Box::new(rhs?));
                let kind = if op == Op::And {
                    ExprKind::And { lhs, rhs }
                } else {
                    ExprKind::Or { lhs, rhs }
                };
                Some(mk(kind, Type::Bool, span))
            }
            Op::Eq | Op::Ne => self.equality(op, lhs, rhs, span),
            Op::Lt | Op::Le | Op::Gt | Op::Ge => self.comparison(op, lhs, rhs, span),
            _ => self.arithmetic(op, lhs, rhs, expected, span),
        }
    }

    fn bool_operand(&mut self, operand: &ast::Expr, symbol: &str) -> Option<tir::Expr> {
        let value = self.value(operand, None)?;
        if value.ty != Type::Bool {
            self.mismatch(
                &Type::Bool,
                &value.ty,
                value.span,
                Some(format!("`{symbol}` needs a `Bool` on both sides")),
            );
            return None;
        }
        Some(value)
    }

    /// Checks two operands that must have the same type. A list literal
    /// on either side takes its type from the other.
    fn operand_pair(
        &mut self,
        lhs: &ast::Expr,
        rhs: &ast::Expr,
        hint: Option<&Type>,
    ) -> (Option<tir::Expr>, Option<tir::Expr>) {
        let list_type = |value: &Option<tir::Expr>| match value {
            Some(tir::Expr {
                ty: ty @ Type::List(_),
                ..
            }) => Some(ty.clone()),
            _ => None,
        };
        if is_list_literal(lhs) && !is_list_literal(rhs) && hint.is_none() {
            let right = self.value(rhs, None);
            let left = self.value(lhs, list_type(&right).as_ref());
            (left, right)
        } else {
            let left = self.value(lhs, hint);
            let right = self.value(rhs, list_type(&left).as_ref());
            (left, right)
        }
    }

    fn equality(
        &mut self,
        op: ast::BinaryOp,
        lhs: &ast::Expr,
        rhs: &ast::Expr,
        span: Span,
    ) -> Option<tir::Expr> {
        let (left, right) = self.operand_pair(lhs, rhs, None);
        let (left, right) = (left?, right?);
        let (left, right) = if left.ty == right.ty {
            (left, right)
        } else if is_number(&left.ty) && is_number(&right.ty) {
            (to_float(left), to_float(right))
        } else {
            let symbol = util::binary_symbol(op);
            self.mismatch(
                &left.ty,
                &right.ty,
                right.span,
                Some(format!("`{symbol}` compares two values of the same type")),
            );
            return None;
        };
        let op = if op == ast::BinaryOp::Eq {
            BinOp::Eq
        } else {
            BinOp::Ne
        };
        Some(binary(op, left, right, Type::Bool, span))
    }

    fn comparison(
        &mut self,
        op: ast::BinaryOp,
        lhs: &ast::Expr,
        rhs: &ast::Expr,
        span: Span,
    ) -> Option<tir::Expr> {
        use ast::BinaryOp as Op;
        let symbol = util::binary_symbol(op);
        let left = self.value(lhs, None);
        let right = self.value(rhs, None);
        let (left, right) = (left?, right?);
        let (left, right) = match (&left.ty, &right.ty) {
            (Type::String, Type::String) => (left, right),
            (a, b) if is_number(a) && is_number(b) => {
                if a == b {
                    (left, right)
                } else {
                    (to_float(left), to_float(right))
                }
            }
            (a, _) if is_number(a) => {
                self.expected_number(&right.ty, right.span, None);
                return None;
            }
            (Type::String, _) => {
                self.mismatch(&Type::String, &right.ty, right.span, None);
                return None;
            }
            (other, _) => {
                let name = self.type_name(other);
                self.err_label(
                    format!("`{symbol}` can't compare `{name}` values"),
                    span,
                    "`<`, `<=`, `>` and `>=` work on numbers and strings",
                );
                return None;
            }
        };
        let op = match op {
            Op::Lt => BinOp::Lt,
            Op::Le => BinOp::Le,
            Op::Gt => BinOp::Gt,
            _ => BinOp::Ge,
        };
        Some(binary(op, left, right, Type::Bool, span))
    }

    fn arithmetic(
        &mut self,
        op: ast::BinaryOp,
        lhs: &ast::Expr,
        rhs: &ast::Expr,
        expected: Option<&Type>,
        span: Span,
    ) -> Option<tir::Expr> {
        use ast::BinaryOp as Op;
        let symbol = util::binary_symbol(op);
        let list_hint = expected.filter(|ty| matches!(ty, Type::List(_)));

        if op == Op::Add && is_list_literal(lhs) && list_hint.is_none() {
            let (left, right) = self.operand_pair(lhs, rhs, None);
            return self.arithmetic_values(op, left?, right?, span);
        }
        let hint = if matches!(op, Op::Add | Op::Mul) {
            list_hint
        } else {
            None
        };
        let Some(left) = self.value(lhs, hint) else {
            self.probe(rhs);
            return None;
        };

        // A struct on the left: its operator method.
        if let Type::Record(record) = left.ty {
            let type_name = self.records[record.0].name.clone();
            let Some(func) = self.method(record, symbol) else {
                let label = if op == Op::Pow {
                    "`**` works on numbers only".to_string()
                } else {
                    format!("define it in `{type_name}` with `def {symbol}(other: …)`")
                };
                self.err_label(
                    format!("`{type_name}` has no `{symbol}` operator"),
                    span,
                    label,
                );
                self.probe(rhs);
                return None;
            };
            if self.cx.const_use().is_some() {
                self.not_constant(span);
                return None;
            }
            let param = self.funcs[func.0].params.first().and_then(|p| p.ty.clone());
            let right = self.store_opt(rhs, param.as_ref());
            let ret = self.ret_type(func, span, Mode::Value);
            return Some(mk(
                ExprKind::Call {
                    func,
                    args: vec![left, right?],
                },
                ret?,
                span,
            ));
        }

        let rhs_hint = match (&left.ty, op) {
            (Type::List(_), Op::Add) => Some(left.ty.clone()),
            _ => None,
        };
        let right = self.value(rhs, rhs_hint.as_ref())?;
        self.arithmetic_values(op, left, right, span)
    }

    /// A built-in arithmetic operator applied to two checked operands.
    fn arithmetic_values(
        &mut self,
        op: ast::BinaryOp,
        left: tir::Expr,
        right: tir::Expr,
        span: Span,
    ) -> Option<tir::Expr> {
        use ast::BinaryOp as Op;
        let symbol = util::binary_symbol(op);
        let bin = match op {
            Op::Add => BinOp::Add,
            Op::Sub => BinOp::Sub,
            Op::Mul => BinOp::Mul,
            Op::Div => BinOp::Div,
            Op::Mod => BinOp::Mod,
            _ => BinOp::Pow,
        };
        let ty = match (&left.ty, &right.ty) {
            (Type::Int, Type::Int) => Type::Int,
            (a, b) if is_number(a) && is_number(b) => {
                return Some(binary(
                    bin,
                    to_float(left),
                    to_float(right),
                    Type::Float,
                    span,
                ));
            }
            (Type::String, Type::String) if op == Op::Add => Type::String,
            (Type::String, Type::Int) if op == Op::Mul => Type::String,
            (Type::List(a), Type::List(b)) if op == Op::Add && a == b => left.ty.clone(),
            (Type::List(_), Type::Int) if op == Op::Mul => left.ty.clone(),
            (a, _) if is_number(a) => {
                // An operator method is found through the left operand.
                let label = match &right.ty {
                    Type::Record(record) if self.method(*record, symbol).is_some() => {
                        let name = &self.records[record.0].name;
                        Some(format!(
                            "`{name}` defines `{symbol}` for a `{name}` on the left-hand side"
                        ))
                    }
                    _ => None,
                };
                self.expected_number(&right.ty, right.span, label);
                return None;
            }
            (Type::String, _) if op == Op::Add => {
                self.mismatch(
                    &Type::String,
                    &right.ty,
                    right.span,
                    Some("convert it with `to_s`, or interpolate: `\"#{…}\"`".to_string()),
                );
                return None;
            }
            (Type::String | Type::List(_), _) if op == Op::Mul => {
                self.mismatch(&Type::Int, &right.ty, right.span, None);
                return None;
            }
            (Type::List(_), _) if op == Op::Add => {
                self.mismatch(&left.ty, &right.ty, right.span, None);
                return None;
            }
            (other, _) => {
                let name = self.type_name(other);
                self.err(format!("`{name}` has no `{symbol}` operator"), span);
                return None;
            }
        };
        Some(binary(bin, left, right, ty, span))
    }

    // ----- conditionals -----

    /// `cond ? then : else_`.
    fn ternary(
        &mut self,
        cond: &ast::Expr,
        then: &ast::Expr,
        else_: &ast::Expr,
        mode: Mode,
        expected: Option<&Type>,
        span: Span,
    ) -> Option<tir::Expr> {
        let cond = self.condition(cond, false);
        if mode == Mode::Stmt {
            let then = self.expr(then, Mode::Stmt, None);
            let else_ = self.expr(else_, Mode::Stmt, None);
            let wrap = |value: tir::Expr| tir::Block {
                stmts: vec![tir::Stmt {
                    span: value.span,
                    kind: tir::StmtKind::Expr(value),
                }],
                value: None,
            };
            let (then, else_) = (wrap(then?), wrap(else_?));
            return Some(mk(
                ExprKind::If {
                    cond: Box::new(cond?),
                    then,
                    else_,
                },
                Type::Void,
                span,
            ));
        }
        let mut branches = Vec::new();
        for branch in [then, else_] {
            let value = self.value(branch, expected);
            branches.push(Branch {
                tail: if value.is_some() {
                    Tail::Value
                } else {
                    Tail::Poison
                },
                block: tir::Block {
                    stmts: Vec::new(),
                    value: value.map(Box::new),
                },
                diverges: false,
                span: branch.span,
            });
        }
        let (mut blocks, ty) = self.join(branches, Mode::Value, expected, "a conditional")?;
        let else_ = blocks.pop()?;
        let then = blocks.pop()?;
        Some(mk(
            ExprKind::If {
                cond: Box::new(cond?),
                then,
                else_,
            },
            ty,
            span,
        ))
    }

    /// Checks one branch block and records whether control leaves it.
    /// `construct` names what the branch belongs to: "the `if`". `before`
    /// is whether the branch itself was unreachable.
    fn branch(
        &mut self,
        construct: &'static str,
        block: &ast::Block,
        mode: Mode,
        expected: Option<&Type>,
        before: bool,
    ) -> Branch {
        self.cx.unreachable = before;
        let (checked, tail) = self.scoped_block(Some(construct), block, mode, expected);
        let diverges = self.cx.unreachable;
        self.cx.unreachable = before;
        Branch {
            block: checked,
            tail,
            diverges,
            span: block.stmts.last().map_or(block.span, |s| s.span),
        }
    }

    fn if_expr(&mut self, e: &ast::Expr, mode: Mode, expected: Option<&Type>) -> Option<tir::Expr> {
        let ast::ExprKind::If {
            cond,
            negate,
            then,
            else_,
        } = &e.kind
        else {
            return None;
        };
        let span = e.span;
        let keyword = if *negate { "unless" } else { "if" };
        let cond = self.condition(cond, *negate);
        let mut failed = false;
        if mode == Mode::Value && else_.is_none() {
            self.err_label(
                format!("`{keyword}` used as a value needs an `else`"),
                span,
                "without one there is no value when the other branch would run",
            );
            failed = true;
        }
        let valued = mode != Mode::Stmt && else_.is_some();
        let (branch_mode, branch_expected) = if valued {
            (mode, expected)
        } else {
            (Mode::Stmt, None)
        };

        let before = self.cx.unreachable;
        let construct = if *negate { "the `unless`" } else { "the `if`" };
        let mut branches = vec![self.branch(construct, then, branch_mode, branch_expected, before)];
        match else_ {
            Some(else_) => {
                branches.push(self.branch(construct, else_, branch_mode, branch_expected, before));
            }
            None => branches.push(Branch {
                block: tir::Block {
                    stmts: Vec::new(),
                    value: None,
                },
                tail: Tail::None,
                diverges: false,
                span,
            }),
        }
        self.cx.unreachable = before || branches.iter().all(|b| b.diverges);
        if failed {
            return None;
        }
        let (mut blocks, ty) = if valued {
            self.join(branches, mode, expected, &format!("an `{keyword}`"))?
        } else {
            (branches.into_iter().map(|b| b.block).collect(), Type::Void)
        };
        let else_ = blocks.pop()?;
        let then = blocks.pop()?;
        Some(mk(
            ExprKind::If {
                cond: Box::new(cond?),
                then,
                else_,
            },
            ty,
            span,
        ))
    }

    /// `case`: a hidden local holds the subject, and the arms become a
    /// chain of `If`s comparing it with `==`.
    fn case_expr(
        &mut self,
        e: &ast::Expr,
        mode: Mode,
        expected: Option<&Type>,
    ) -> Option<tir::Expr> {
        let ast::ExprKind::Case {
            subject,
            arms,
            else_,
        } = &e.kind
        else {
            return None;
        };
        const CASE: &str = "the `case`";
        let span = e.span;
        let mut failed = false;
        let subject_value = self.value(subject, None);
        let subject_ty = match &subject_value {
            Some(value)
                if matches!(
                    value.ty,
                    Type::Int | Type::String | Type::Bool | Type::Enum(_)
                ) =>
            {
                Some(value.ty.clone())
            }
            Some(value) => {
                let name = self.type_name(&value.ty);
                self.err_label(
                    format!("`case` can't compare `{name}` values"),
                    value.span,
                    "the value of a `case` is an `Int`, a `String`, a `Bool` or an enum",
                );
                None
            }
            None => None,
        };
        let local = subject_ty
            .as_ref()
            .map(|ty| self.hidden_local("$case", ty.clone(), tir::LocalKind::Var));

        // The condition of each arm, and the enum variants they name.
        let mut conds: Vec<Option<tir::Expr>> = Vec::new();
        let mut covered: Vec<usize> = Vec::new();
        for arm in arms {
            let mut cond: Option<tir::Expr> = None;
            let mut ok = !arm.values.is_empty();
            for value in &arm.values {
                let value = match &subject_ty {
                    Some(ty) => {
                        let label = format!(
                            "`when` values are compared with the `case` value, which is `{}`",
                            self.type_name(ty)
                        );
                        self.value(value, Some(ty))
                            .and_then(|value| self.convert(value, ty, Some(label)))
                    }
                    None => {
                        self.probe(value);
                        None
                    }
                };
                let (Some(value), Some(local), Some(ty)) = (value, local, &subject_ty) else {
                    ok = false;
                    continue;
                };
                if let ExprKind::EnumVariant(index) = value.kind {
                    covered.push(index);
                }
                let value_span = value.span;
                let subject_read = read(tir::Place {
                    kind: tir::PlaceKind::Local(local),
                    ty: ty.clone(),
                    span: subject.span,
                });
                let test = binary(BinOp::Eq, subject_read, value, Type::Bool, value_span);
                cond = Some(match cond {
                    None => test,
                    Some(previous) => {
                        let both = previous.span.to(value_span);
                        mk(
                            ExprKind::Or {
                                lhs: Box::new(previous),
                                rhs: Box::new(test),
                            },
                            Type::Bool,
                            both,
                        )
                    }
                });
            }
            conds.push(if ok { cond } else { None });
        }

        // An enum with no `else` must have every variant covered; the
        // last arm then takes the place of the `else`.
        let mut exhaustive = false;
        if else_.is_none()
            && let Some(Type::Enum(id)) = &subject_ty
        {
            let info = &self.enums[id.0];
            let missing: Vec<String> = info
                .variants
                .iter()
                .enumerate()
                .filter(|(index, _)| !covered.contains(index))
                .map(|(_, variant)| format!("{}::{variant}", info.name))
                .collect();
            if missing.is_empty() {
                exhaustive = !arms.is_empty();
            } else {
                self.err_label(
                    format!("`case` does not cover {}", util::name_list(&missing)),
                    subject.span,
                    "add a `when` for each, or an `else`",
                );
                failed = true;
            }
        }
        let has_else = else_.is_some() || exhaustive;
        if mode == Mode::Value && !has_else && !failed {
            self.err_label(
                "`case` used as a value needs an `else`",
                span,
                "without one there is no value when no `when` matches",
            );
            failed = true;
        }
        let valued = mode != Mode::Stmt && has_else;
        let (branch_mode, branch_expected) = if valued {
            (mode, expected)
        } else {
            (Mode::Stmt, None)
        };

        let before = self.cx.unreachable;
        let mut branches: Vec<Branch> = Vec::new();
        for arm in arms {
            branches.push(self.branch(CASE, &arm.body, branch_mode, branch_expected, before));
        }
        if let Some(else_) = else_ {
            branches.push(self.branch(CASE, else_, branch_mode, branch_expected, before));
        }
        self.cx.unreachable =
            before || (has_else && !branches.is_empty() && branches.iter().all(|b| b.diverges));
        if failed {
            return None;
        }
        let conds: Option<Vec<tir::Expr>> = conds.into_iter().collect();
        let mut conds = conds?;
        let (local, subject_value) = (local?, subject_value?);
        let (mut blocks, ty) = if valued {
            self.join(branches, mode, expected, "a `case`")?
        } else {
            (branches.into_iter().map(|b| b.block).collect(), Type::Void)
        };

        // Build the chain from the last arm backwards.
        let mut rest = if has_else {
            blocks.pop()?
        } else {
            tir::Block {
                stmts: Vec::new(),
                value: None,
            }
        };
        if else_.is_none() && exhaustive {
            conds.pop();
        }
        for (cond, then) in conds.into_iter().zip(blocks).rev() {
            let chain = mk(
                ExprKind::If {
                    cond: Box::new(cond),
                    then,
                    else_: rest,
                },
                ty.clone(),
                span,
            );
            rest = if ty == Type::Void {
                tir::Block {
                    stmts: vec![tir::Stmt {
                        kind: tir::StmtKind::Expr(chain),
                        span,
                    }],
                    value: None,
                }
            } else {
                tir::Block {
                    stmts: Vec::new(),
                    value: Some(Box::new(chain)),
                }
            };
        }
        let mut stmts = vec![tir::Stmt {
            kind: tir::StmtKind::Let {
                local,
                init: subject_value,
            },
            span: subject.span,
        }];
        stmts.extend(rest.stmts);
        let block = tir::Block {
            stmts,
            value: rest.value,
        };
        Some(mk(ExprKind::Block(block), ty, span))
    }

    /// Gives the branches of one construct a common type. Returns the
    /// blocks and that type: `Void`, with no block values, when the
    /// construct has no value. In `Value` mode a branch that neither has a
    /// value nor leaves is an error; in `Maybe` mode the construct then
    /// simply has no value. A branch that leaves gets a value that is
    /// never used, so that every block has one.
    fn join(
        &mut self,
        mut branches: Vec<Branch>,
        mode: Mode,
        expected: Option<&Type>,
        what: &str,
    ) -> Option<(Vec<tir::Block>, Type)> {
        let void = |branches: Vec<Branch>| -> (Vec<tir::Block>, Type) {
            (
                branches.into_iter().map(|b| demote(b.block)).collect(),
                Type::Void,
            )
        };
        let missing: Vec<Span> = branches
            .iter()
            .filter(|b| b.tail == Tail::None && !b.diverges)
            .map(|b| b.span)
            .collect();
        let any_poison = branches.iter().any(|b| b.tail == Tail::Poison);
        if !missing.is_empty() {
            if mode == Mode::Maybe {
                // A branch with a value next to one that ends in a call
                // whose type is not known yet: the type is needed.
                if let Some((callee, span)) = self.cx.recursive_tail
                    && branches.iter().any(|b| b.tail == Tail::Value)
                {
                    self.recursion_error(callee, span);
                    return None;
                }
                return Some(void(branches));
            }
            for span in missing {
                self.err_label(
                    "this branch has no value",
                    span,
                    format!("every branch of {what} used as a value ends in one"),
                );
            }
            return None;
        }
        if any_poison {
            return None;
        }
        if !branches.iter().any(|b| b.tail == Tail::Value) {
            return Some(void(branches));
        }

        let ty = match expected {
            Some(expected) => {
                let mut ok = true;
                for branch in &mut branches {
                    if let Some(value) = branch.block.value.take() {
                        match self.convert(*value, expected, None) {
                            Some(value) => branch.block.value = Some(Box::new(value)),
                            None => ok = false,
                        }
                    }
                }
                if !ok {
                    return None;
                }
                expected.value_type()
            }
            None => {
                let types: Vec<(Type, Span)> = branches
                    .iter()
                    .filter_map(|b| b.block.value.as_ref())
                    .map(|v| (v.ty.clone(), v.span))
                    .collect();
                let first = types[0].0.clone();
                if types.iter().all(|(ty, _)| *ty == first) {
                    first
                } else if types.iter().all(|(ty, _)| is_number(ty)) {
                    for branch in &mut branches {
                        if let Some(value) = branch.block.value.take() {
                            branch.block.value = Some(Box::new(to_float(*value)));
                        }
                    }
                    Type::Float
                } else {
                    if mode == Mode::Maybe {
                        return Some(void(branches));
                    }
                    if let Some((found, span)) = types.iter().find(|(ty, _)| *ty != first) {
                        self.mismatch(
                            &first,
                            found,
                            *span,
                            Some(format!("every branch of {what} has the same type")),
                        );
                    }
                    return None;
                }
            }
        };
        let mut blocks = Vec::new();
        for mut branch in branches {
            if branch.block.value.is_none() {
                branch.block.value = Some(Box::new(self.zero_value(&ty, branch.span)));
            }
            blocks.push(branch.block);
        }
        Some((blocks, ty))
    }

    /// The all-zero value of a type.
    fn zero_value(&self, ty: &Type, span: Span) -> tir::Expr {
        let kind = match ty {
            Type::Int | Type::Void | Type::Compact(_) => ExprKind::Int(0),
            Type::Float => ExprKind::Float(0.0),
            Type::Bool => ExprKind::Bool(false),
            Type::String => ExprKind::Str(String::new()),
            Type::Entity => ExprKind::EntityNone,
            Type::List(_) => ExprKind::ListLit(Vec::new()),
            Type::Enum(_) => ExprKind::EnumVariant(0),
            Type::Record(id) => ExprKind::Construct(
                self.records[id.0]
                    .fields
                    .iter()
                    .map(|field| match &field.ty {
                        Some(ty) => self.zero_value(&ty.value_type(), span),
                        None => mk(ExprKind::Int(0), Type::Int, span),
                    })
                    .collect(),
            ),
        };
        mk(kind, ty.clone(), span)
    }
}
