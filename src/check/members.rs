//! Members and indexing: fields, methods, the built-in methods and
//! namespaces, entity operations, and queries.

use crate::ast;
use crate::diag::Diagnostic;
use crate::source::Span;
use crate::tir::{self, Builtin, ComponentId, ExprKind, RecordId, ResourceId, Type};

use super::exprs::builtin;
use super::{BUILTIN_NAMES, Checker, Global, Mode, PlaceRef, ReadOnly, Resolved, mk, util};

/// The types built-in methods take and return, apart from their receiver.
#[derive(Clone, Copy)]
enum Prim {
    Int,
    Float,
    Bool,
    Str,
}

impl Prim {
    fn ty(self) -> Type {
        match self {
            Prim::Int => Type::Int,
            Prim::Float => Type::Float,
            Prim::Bool => Type::Bool,
            Prim::Str => Type::String,
        }
    }
}

/// Name, parameters after the receiver, result, operation.
type Method = (&'static str, &'static [Prim], Prim, Builtin);

const INT_METHODS: &[Method] = &[("abs", &[], Prim::Int, Builtin::IntAbs)];

const FLOAT_METHODS: &[Method] = &[
    ("abs", &[], Prim::Float, Builtin::FloatAbs),
    ("to_i", &[], Prim::Int, Builtin::FloatToI),
    ("floor", &[], Prim::Int, Builtin::FloatFloor),
    ("ceil", &[], Prim::Int, Builtin::FloatCeil),
    ("round", &[], Prim::Int, Builtin::FloatRound),
    ("nan?", &[], Prim::Bool, Builtin::FloatNan),
];

const STRING_METHODS: &[Method] = &[
    ("length", &[], Prim::Int, Builtin::StrLength),
    ("bytesize", &[], Prim::Int, Builtin::StrBytesize),
    ("empty?", &[], Prim::Bool, Builtin::StrEmpty),
    ("upcase", &[], Prim::Str, Builtin::StrUpcase),
    ("downcase", &[], Prim::Str, Builtin::StrDowncase),
    ("include?", &[Prim::Str], Prim::Bool, Builtin::StrInclude),
    (
        "start_with?",
        &[Prim::Str],
        Prim::Bool,
        Builtin::StrStartWith,
    ),
    ("end_with?", &[Prim::Str], Prim::Bool, Builtin::StrEndWith),
    ("ljust", &[Prim::Int], Prim::Str, Builtin::StrLjust),
    ("rjust", &[Prim::Int], Prim::Str, Builtin::StrRjust),
    ("to_i", &[], Prim::Int, Builtin::StrToI),
    ("to_f", &[], Prim::Float, Builtin::StrToF),
];

const MATH_FUNCTIONS: &[(&str, usize, Builtin)] = &[
    ("sqrt", 1, Builtin::Sqrt),
    ("sin", 1, Builtin::Sin),
    ("cos", 1, Builtin::Cos),
    ("tan", 1, Builtin::Tan),
    ("atan2", 2, Builtin::Atan2),
    ("exp", 1, Builtin::Exp),
    ("log", 1, Builtin::Log),
];

/// Every built-in method of a type, for suggestions.
fn builtin_method_names(ty: &Type) -> &'static [&'static str] {
    match ty {
        Type::Int => &["abs", "to_f", "clamp", "to_s", "times", "upto", "downto"],
        Type::Float => &[
            "abs", "to_i", "floor", "ceil", "round", "nan?", "clamp", "to_s",
        ],
        Type::String => &[
            "length",
            "bytesize",
            "empty?",
            "upcase",
            "downcase",
            "include?",
            "start_with?",
            "end_with?",
            "ljust",
            "rjust",
            "to_i",
            "to_f",
            "to_s",
        ],
        Type::List(_) => &[
            "length",
            "empty?",
            "first",
            "last",
            "push",
            "pop",
            "delete_at",
            "clear",
            "include?",
            "each",
            "each_with_index",
            "to_s",
        ],
        Type::Entity => &["has?", "add", "remove", "despawn", "alive?", "to_s"],
        _ => &["to_s"],
    }
}

/// A component or a tag named in an entity operation or a query.
pub(super) enum ComponentRef {
    Component(ComponentId, RecordId),
    Tag(ComponentId),
}

impl ComponentRef {
    fn id(&self) -> ComponentId {
        match self {
            ComponentRef::Component(id, _) | ComponentRef::Tag(id) => *id,
        }
    }
}

/// A query after checking. A term that failed is `None`, so that the
/// block parameters of the other terms keep their types.
pub(super) struct CheckedQuery {
    pub(super) terms: Vec<Option<tir::Term>>,
    pub(super) with: Vec<ComponentId>,
    pub(super) without: Vec<ComponentId>,
    pub(super) ok: bool,
}

impl CheckedQuery {
    pub(super) fn finish(self) -> Option<tir::Query> {
        if !self.ok {
            return None;
        }
        let terms: Option<Vec<tir::Term>> = self.terms.into_iter().collect();
        Some(tir::Query {
            // Numbered by the effect analysis.
            id: tir::QueryId(0),
            terms: terms?,
            with: self.with,
            without: self.without,
        })
    }
}

impl Checker<'_> {
    /// `recv.name` or `recv.name(args)`.
    pub(super) fn member(
        &mut self,
        recv: &ast::Expr,
        name: &ast::Ident,
        args: Option<&[ast::Arg]>,
        span: Span,
        mode: Mode,
    ) -> Option<Resolved> {
        if let ast::ExprKind::Const(owner) = &recv.kind {
            match owner.as_str() {
                "World" => return self.world_member(name, args, span).map(Resolved::Value),
                "Time" => return self.time_member(name, args, span),
                "Math" => return self.math_member(name, args, span).map(Resolved::Value),
                "Entity" => return self.entity_namespace(name, args, span),
                _ => {}
            }
            match self.global(owner) {
                Some(Global::Resource(id)) if owner == "Window" => {
                    return self.window_member(id, name, args, span);
                }
                Some(Global::Resource(id)) => return self.resource_field(id, name, args, span),
                Some(Global::Const(_)) | None => {}
                Some(Global::Enum(id)) => {
                    let variants = &self.enums[id.0].variants;
                    let label = if variants.contains(&name.name) {
                        format!("did you mean `{owner}::{}`?", name.name)
                    } else {
                        "variants are written with `::`".to_string()
                    };
                    self.err_label(
                        format!("`{owner}` is an enum, not a value"),
                        recv.span,
                        label,
                    );
                    return None;
                }
                Some(global) => {
                    let kind = self.global_kind(global);
                    self.err_label(
                        format!("`{owner}` is {kind}, not a value"),
                        recv.span,
                        format!("`.{}` needs a value on its left", name.name),
                    );
                    return None;
                }
            }
        }

        let base = self.resolve(recv, Mode::Value)?;
        match base.ty() {
            Type::Void => {
                self.err("this expression has no value", recv.span);
                None
            }
            Type::Record(record) => self.record_member(base, record, name, args, span, mode),
            Type::Entity => self
                .entity_method(base.into_value(), name, args, span)
                .map(Resolved::Value),
            Type::List(elem) => self
                .list_method(base, recv, &elem, name, args, span)
                .map(Resolved::Value),
            _ => self
                .builtin_method(base.into_value(), name, args, span)
                .map(Resolved::Value),
        }
    }

    fn unknown_method(&mut self, ty: &Type, name: &ast::Ident, candidates: &[String]) {
        let type_name = self.type_name(ty);
        let mut diagnostic = Diagnostic::new(
            format!("unknown method `{}` for `{type_name}`", name.name),
            name.span,
        );
        let already = matches!(
            (ty, name.name.as_str()),
            (Type::Int, "to_i") | (Type::Float, "to_f")
        );
        if already {
            diagnostic = diagnostic.label(format!("it is already {type_name}: drop the call"));
        } else if let Some(close) = util::closest(&name.name, candidates.iter().map(String::as_str))
        {
            diagnostic = diagnostic.label(format!("did you mean `{close}`?"));
        }
        self.error(diagnostic);
    }

    /// A field or a method of a struct, component or event value.
    fn record_member(
        &mut self,
        base: Resolved,
        record: RecordId,
        name: &ast::Ident,
        args: Option<&[ast::Arg]>,
        span: Span,
        mode: Mode,
    ) -> Option<Resolved> {
        let type_name = self.records[record.0].name.clone();
        if let Some(field) = self.field_index(record, &name.name) {
            if args.is_some() {
                self.err_label(
                    format!("`{}` is a field of `{type_name}`, not a method", name.name),
                    name.span,
                    "read it without parentheses",
                );
                return None;
            }
            let ty = self.records[record.0].fields[field].ty.clone()?;
            return Some(match base {
                Resolved::Place(base) => Resolved::Place(PlaceRef {
                    place: tir::Place {
                        kind: tir::PlaceKind::Field {
                            base: Box::new(base.place),
                            field,
                        },
                        ty,
                        span,
                    },
                    readonly: base.readonly,
                }),
                Resolved::Value(base) => Resolved::Value(mk(
                    ExprKind::Field {
                        base: Box::new(base),
                        field,
                    },
                    ty.value_type(),
                    span,
                )),
            });
        }
        if let Some(func) = self.method(record, &name.name) {
            let recv = base.into_value();
            return self
                .call_func(
                    func,
                    Some(recv),
                    &name.name,
                    args.unwrap_or(&[]),
                    span,
                    mode,
                )
                .map(Resolved::Value);
        }
        if name.name == "to_s" {
            return self
                .builtin_method(base.into_value(), name, args, span)
                .map(Resolved::Value);
        }
        let info = &self.records[record.0];
        let mut candidates: Vec<String> = info.fields.iter().map(|f| f.name.clone()).collect();
        candidates.extend(util::named_methods(&info.methods));
        candidates.push("to_s".to_string());
        let what = if args.is_some() {
            "method"
        } else {
            "field or method"
        };
        self.unknown(
            format!("unknown {what} `{}` for `{type_name}`", name.name),
            name.span,
            &name.name,
            &candidates,
        );
        for arg in args.unwrap_or(&[]) {
            self.probe(&arg.value);
        }
        None
    }

    /// A method of an `Int`, `Float`, `String`, `Bool` or enum value, and
    /// `to_s` on anything.
    fn builtin_method(
        &mut self,
        recv: tir::Expr,
        name: &ast::Ident,
        args: Option<&[ast::Arg]>,
        span: Span,
    ) -> Option<tir::Expr> {
        let args = args.unwrap_or(&[]);
        let method = name.name.as_str();
        self.no_keywords(method, args);
        match (&recv.ty, method) {
            (_, "to_s") => {
                if !self.arity(method, 0, args.len(), span) {
                    return None;
                }
                return Some(if recv.ty == Type::String {
                    recv
                } else {
                    builtin(Builtin::ToS, vec![recv], Type::String, span)
                });
            }
            (Type::Int, "to_f") => {
                if !self.arity(method, 0, args.len(), span) {
                    return None;
                }
                return Some(mk(ExprKind::IntToFloat(Box::new(recv)), Type::Float, span));
            }
            (Type::Int | Type::Float, "clamp") => {
                let arity_ok = self.arity(method, 2, args.len(), span);
                let bounds: Vec<Option<tir::Expr>> =
                    args.iter().map(|a| self.value(&a.value, None)).collect();
                if !arity_ok {
                    return None;
                }
                let bounds: Option<Vec<tir::Expr>> = bounds.into_iter().collect();
                let mut values = vec![recv];
                values.extend(bounds?);
                let (values, float) = self.numbers(values)?;
                return Some(if float {
                    builtin(Builtin::FloatClamp, values, Type::Float, span)
                } else {
                    builtin(Builtin::IntClamp, values, Type::Int, span)
                });
            }
            (Type::Float, "round") if !args.is_empty() => {
                if args.len() > 1 {
                    self.at_most(method, 1, args.len(), span);
                    return None;
                }
                let digits = self.store(&args[0].value, &Type::Int)?;
                return Some(builtin(
                    Builtin::FloatRoundDigits,
                    vec![recv, digits],
                    Type::Float,
                    span,
                ));
            }
            (Type::Int, "times" | "upto" | "downto") => {
                self.needs_block(method, name.span);
                return None;
            }
            _ => {}
        }
        let table = match recv.ty {
            Type::Int => INT_METHODS,
            Type::Float => FLOAT_METHODS,
            Type::String => STRING_METHODS,
            _ => &[],
        };
        let Some((_, params, ret, op)) = table.iter().find(|m| m.0 == method) else {
            let candidates: Vec<String> = builtin_method_names(&recv.ty)
                .iter()
                .map(|n| n.to_string())
                .collect();
            self.unknown_method(&recv.ty, name, &candidates);
            for arg in args {
                self.probe(&arg.value);
            }
            return None;
        };
        let arity_ok = self.arity(method, params.len(), args.len(), span);
        let mut values = vec![Some(recv)];
        for (i, arg) in args.iter().enumerate() {
            values.push(match params.get(i) {
                Some(param) => self.store(&arg.value, &param.ty()),
                None => {
                    self.probe(&arg.value);
                    None
                }
            });
        }
        if !arity_ok {
            return None;
        }
        let values: Option<Vec<tir::Expr>> = values.into_iter().collect();
        Some(builtin(*op, values?, ret.ty(), span))
    }

    fn needs_block(&mut self, method: &str, span: Span) {
        self.err_label(
            format!("`{method}` needs a `do` block"),
            span,
            format!("write `{method} do … end`"),
        );
    }

    /// A method of a list. `push`, `pop`, `delete_at` and `clear` need the
    /// list to be a place that can be written.
    fn list_method(
        &mut self,
        base: Resolved,
        recv: &ast::Expr,
        elem: &Type,
        name: &ast::Ident,
        args: Option<&[ast::Arg]>,
        span: Span,
    ) -> Option<tir::Expr> {
        let args = args.unwrap_or(&[]);
        let method = name.name.as_str();
        let list_ty = Type::List(Box::new(elem.clone()));
        let elem_value = elem.value_type();
        self.no_keywords(method, args);
        let wanted = match method {
            "push" | "delete_at" | "include?" => 1,
            "pop" | "clear" | "length" | "empty?" | "first" | "last" | "to_s" => 0,
            "each" | "each_with_index" => {
                self.needs_block(method, name.span);
                return None;
            }
            _ => {
                let candidates: Vec<String> = builtin_method_names(&list_ty)
                    .iter()
                    .map(|n| n.to_string())
                    .collect();
                self.unknown_method(&list_ty, name, &candidates);
                for arg in args {
                    self.probe(&arg.value);
                }
                return None;
            }
        };
        if !self.arity(method, wanted, args.len(), span) {
            for arg in args {
                self.probe(&arg.value);
            }
            return None;
        }
        match method {
            "push" => {
                let list = self.list_place(base, recv, method);
                let value = self.store(&args[0].value, elem);
                Some(mk(
                    ExprKind::ListPush {
                        list: list?,
                        value: Box::new(value?),
                    },
                    Type::Void,
                    span,
                ))
            }
            "delete_at" => {
                let list = self.list_place(base, recv, method);
                let index = self.store(&args[0].value, &Type::Int);
                Some(mk(
                    ExprKind::ListDeleteAt {
                        list: list?,
                        index: Box::new(index?),
                    },
                    elem_value,
                    span,
                ))
            }
            "pop" => {
                let list = self.list_place(base, recv, method)?;
                Some(mk(ExprKind::ListPop { list }, elem_value, span))
            }
            "clear" => {
                let list = self.list_place(base, recv, method)?;
                Some(mk(ExprKind::ListClear { list }, Type::Void, span))
            }
            "include?" => {
                let value = self.store(&args[0].value, &elem_value)?;
                Some(builtin(
                    Builtin::ListInclude,
                    vec![base.into_value(), value],
                    Type::Bool,
                    span,
                ))
            }
            _ => {
                let (op, ty) = match method {
                    "length" => (Builtin::ListLength, Type::Int),
                    "empty?" => (Builtin::ListEmpty, Type::Bool),
                    "first" => (Builtin::ListFirst, elem_value),
                    "last" => (Builtin::ListLast, elem_value),
                    _ => (Builtin::ToS, Type::String),
                };
                Some(builtin(op, vec![base.into_value()], ty, span))
            }
        }
    }

    /// The list a modifying method works on, which must be a place that
    /// may be written.
    fn list_place(&mut self, base: Resolved, recv: &ast::Expr, method: &str) -> Option<tir::Place> {
        match base {
            Resolved::Place(mut list) => {
                if let Some(readonly) = &list.readonly {
                    let target = util::describe(recv).unwrap_or_else(|| "this list".to_string());
                    self.err_label(
                        format!("can't modify `{target}`"),
                        recv.span,
                        readonly.reason(),
                    );
                    return None;
                }
                self.mark_write(&mut list.place);
                Some(list.place)
            }
            Resolved::Value(_) => {
                self.err_label(
                    format!("`{method}` needs a list that can be modified"),
                    recv.span,
                    "this list is a temporary value; put it in a variable first",
                );
                None
            }
        }
    }

    /// Records that something is written through a place: on the query
    /// parameter or the `entity[Component]` it starts from.
    pub(super) fn mark_write(&mut self, place: &mut tir::Place) {
        match &mut place.kind {
            tir::PlaceKind::Local(id) => self.cx.meta[id.0].written = true,
            tir::PlaceKind::Component { write, .. } => *write = true,
            tir::PlaceKind::Field { base, .. } | tir::PlaceKind::Index { base, .. } => {
                self.mark_write(base);
            }
            tir::PlaceKind::Const(_)
            | tir::PlaceKind::ResourceField(..)
            | tir::PlaceKind::Time(_) => {}
        }
    }

    // ----- namespaces -----

    fn world_member(
        &mut self,
        name: &ast::Ident,
        args: Option<&[ast::Arg]>,
        span: Span,
    ) -> Option<tir::Expr> {
        let args = args.unwrap_or(&[]);
        let kind = match name.name.as_str() {
            "count" => {
                if args.is_empty() {
                    return Some(mk(ExprKind::WorldCount(None), Type::Int, span));
                }
                let query = self.count_query(args, span)?;
                return Some(mk(ExprKind::WorldCount(Some(query)), Type::Int, span));
            }
            "tick" => ExprKind::WorldTick,
            "run" => ExprKind::WorldRun,
            "stop" => ExprKind::WorldStop,
            other => {
                let candidates = ["tick", "run", "stop", "count"].map(String::from);
                self.unknown(
                    format!("unknown method `{other}` for `World`"),
                    name.span,
                    other,
                    &candidates,
                );
                return None;
            }
        };
        let method = format!("World.{}", name.name);
        if matches!(kind, ExprKind::WorldTick | ExprKind::WorldRun)
            && self.cx.query_eaches > 0
            && !self.cx.gpu
        {
            self.err_label(
                format!("`{method}` is not allowed inside an `each`"),
                span,
                "the loop holds the world's rows; call it after the `end`",
            );
        }
        self.no_keywords(&method, args);
        if !self.arity(&method, 0, args.len(), span) {
            return None;
        }
        Some(mk(kind, Type::Void, span))
    }

    fn time_member(
        &mut self,
        name: &ast::Ident,
        args: Option<&[ast::Arg]>,
        span: Span,
    ) -> Option<Resolved> {
        let (field, ty, readonly) = match name.name.as_str() {
            "tick" => (
                tir::TimeField::Tick,
                Type::Int,
                Some(ReadOnly::TimeField("tick")),
            ),
            "dt" => (tir::TimeField::Dt, Type::Float, None),
            "elapsed" => (
                tir::TimeField::Elapsed,
                Type::Float,
                Some(ReadOnly::TimeField("elapsed")),
            ),
            other => {
                let candidates = ["tick", "dt", "elapsed"].map(String::from);
                self.unknown(
                    format!("unknown field `{other}` for `Time`"),
                    name.span,
                    other,
                    &candidates,
                );
                return None;
            }
        };
        if args.is_some() {
            self.err_label(
                format!("`Time.{}` is a field, not a method", name.name),
                name.span,
                "read it without parentheses",
            );
            return None;
        }
        Some(Resolved::Place(PlaceRef {
            place: tir::Place {
                kind: tir::PlaceKind::Time(field),
                ty,
                span,
            },
            readonly,
        }))
    }

    fn math_member(
        &mut self,
        name: &ast::Ident,
        args: Option<&[ast::Arg]>,
        span: Span,
    ) -> Option<tir::Expr> {
        let args = args.unwrap_or(&[]);
        let function = name.name.as_str();
        if matches!(function, "PI" | "E") {
            self.err_label(
                format!("`Math.{function}` is not a function"),
                name.span,
                format!("the constant is written `Math::{function}`"),
            );
            return None;
        }
        let Some((_, wanted, op)) = MATH_FUNCTIONS.iter().find(|f| f.0 == function) else {
            let candidates: Vec<String> = MATH_FUNCTIONS.iter().map(|f| f.0.to_string()).collect();
            self.unknown(
                format!("unknown function `Math.{function}`"),
                name.span,
                function,
                &candidates,
            );
            for arg in args {
                self.probe(&arg.value);
            }
            return None;
        };
        let full = format!("Math.{function}");
        self.no_keywords(&full, args);
        let arity_ok = self.arity(&full, *wanted, args.len(), span);
        let values: Vec<Option<tir::Expr>> = args
            .iter()
            .map(|a| self.store(&a.value, &Type::Float))
            .collect();
        if !arity_ok {
            return None;
        }
        let values: Option<Vec<tir::Expr>> = values.into_iter().collect();
        Some(builtin(*op, values?, Type::Float, span))
    }

    /// `Entity.none`.
    fn entity_namespace(
        &mut self,
        name: &ast::Ident,
        args: Option<&[ast::Arg]>,
        span: Span,
    ) -> Option<Resolved> {
        if name.name != "none" {
            self.err_label(
                format!("unknown method `{}` for the type `Entity`", name.name),
                name.span,
                "the type has `Entity.none`; other operations need an entity value",
            );
            return None;
        }
        if !self.arity("Entity.none", 0, args.map_or(0, <[ast::Arg]>::len), span) {
            return None;
        }
        Some(Resolved::Value(mk(
            ExprKind::EntityNone,
            Type::Entity,
            span,
        )))
    }

    /// A member of the built-in `Window`: one of its fields, or `open?`.
    /// `Window.open(…)` is a statement and is checked as one.
    fn window_member(
        &mut self,
        id: ResourceId,
        name: &ast::Ident,
        args: Option<&[ast::Arg]>,
        span: Span,
    ) -> Option<Resolved> {
        const FIELDS: [&str; 5] = [
            "width",
            "height",
            "pixel_width",
            "pixel_height",
            "background",
        ];
        let field = match name.name.as_str() {
            "open" => {
                self.err_label(
                    "`Window.open` has no value",
                    span,
                    "it opens the window: write it as a statement of its own",
                );
                if let Some(args) = args {
                    for arg in args {
                        self.probe(&arg.value);
                    }
                }
                return None;
            }
            "open?" => ast::Ident {
                name: "open".to_string(),
                span: name.span,
            },
            other if FIELDS.contains(&other) => name.clone(),
            other => {
                let candidates: Vec<String> = FIELDS
                    .iter()
                    .chain(&["open?", "open"])
                    .map(|f| f.to_string())
                    .collect();
                self.unknown(
                    format!("unknown member `{other}` for `Window`"),
                    name.span,
                    other,
                    &candidates,
                );
                return None;
            }
        };
        if name.name == "open?" && args.is_some() {
            self.err_label(
                "`Window.open?` takes no arguments",
                name.span,
                "read it without parentheses",
            );
            return None;
        }
        let mut resolved = self.resource_field(id, &field, args, span)?;
        if let Resolved::Place(place) = &mut resolved
            && name.name != "background"
        {
            place.readonly = Some(ReadOnly::WindowField(name.name.clone()));
        }
        Some(resolved)
    }

    fn resource_field(
        &mut self,
        id: ResourceId,
        name: &ast::Ident,
        args: Option<&[ast::Arg]>,
        span: Span,
    ) -> Option<Resolved> {
        let info = &self.resources[id.0];
        let resource = info.name.clone();
        let Some(field) = info.fields.iter().position(|f| f.name == name.name) else {
            let candidates: Vec<String> = info.fields.iter().map(|f| f.name.clone()).collect();
            self.unknown(
                format!("unknown field `{}` for resource `{resource}`", name.name),
                name.span,
                &name.name,
                &candidates,
            );
            return None;
        };
        if args.is_some() {
            self.err_label(
                format!("`{resource}.{}` is a field, not a method", name.name),
                name.span,
                "read it without parentheses",
            );
            return None;
        }
        let ty = info.fields[field].ty.clone()?;
        Some(Resolved::Place(PlaceRef {
            place: tir::Place {
                kind: tir::PlaceKind::ResourceField(id, field),
                ty,
                span,
            },
            readonly: None,
        }))
    }

    // ----- entities -----

    fn entity_method(
        &mut self,
        entity: tir::Expr,
        name: &ast::Ident,
        args: Option<&[ast::Arg]>,
        span: Span,
    ) -> Option<tir::Expr> {
        let args = args.unwrap_or(&[]);
        let method = name.name.as_str();
        if !matches!(method, "has?" | "add" | "remove" | "despawn" | "alive?") {
            return self.builtin_method(entity, name, Some(args), span);
        }
        if self.cx.const_use().is_some() {
            self.not_constant(span);
            return None;
        }
        self.no_keywords(method, args);
        let entity = Box::new(entity);
        match method {
            "has?" => {
                if !self.arity(method, 1, args.len(), span) {
                    return None;
                }
                let component = self.component_arg(&args[0].value)?.id();
                Some(mk(ExprKind::Has { entity, component }, Type::Bool, span))
            }
            "add" => {
                if args.is_empty() {
                    self.err("`add` takes at least 1 argument, but none were given", span);
                    return None;
                }
                let components = self.component_inits(args.iter().map(|a| &a.value))?;
                Some(mk(ExprKind::Add { entity, components }, Type::Void, span))
            }
            "remove" => {
                if args.is_empty() {
                    self.err(
                        "`remove` takes at least 1 argument, but none were given",
                        span,
                    );
                    return None;
                }
                let components: Vec<Option<ComponentId>> = args
                    .iter()
                    .map(|a| self.component_arg(&a.value).map(|c| c.id()))
                    .collect();
                let components: Option<Vec<ComponentId>> = components.into_iter().collect();
                Some(mk(
                    ExprKind::Remove {
                        entity,
                        components: components?,
                    },
                    Type::Void,
                    span,
                ))
            }
            _ => {
                if !self.arity(method, 0, args.len(), span) {
                    return None;
                }
                Some(if method == "despawn" {
                    mk(ExprKind::Despawn(entity), Type::Void, span)
                } else {
                    mk(ExprKind::Alive(entity), Type::Bool, span)
                })
            }
        }
    }

    /// An argument that must be the bare name of a component or a tag.
    fn component_arg(&mut self, arg: &ast::Expr) -> Option<ComponentRef> {
        let ast::ExprKind::Const(name) = &arg.kind else {
            self.err_label(
                "expected the name of a component or tag",
                arg.span,
                "write the type's name, like `Health`",
            );
            return None;
        };
        self.component_ref(name, arg.span)
    }

    /// Looks up a name that must be a component or a tag.
    pub(super) fn component_ref(&mut self, name: &str, span: Span) -> Option<ComponentRef> {
        match self.global(name) {
            Some(Global::Tag(id)) => Some(ComponentRef::Tag(id)),
            Some(Global::Record(record)) => match self.records[record.0].kind {
                tir::RecordKind::Component(id) => Some(ComponentRef::Component(id, record)),
                _ => {
                    self.not_a_component(name, span, Some(Global::Record(record)));
                    None
                }
            },
            other => {
                self.not_a_component(name, span, other);
                None
            }
        }
    }

    fn not_a_component(&mut self, name: &str, span: Span, global: Option<Global>) {
        match global {
            Some(global) => {
                let kind = self.global_kind(global);
                self.err_label(
                    format!("`{name}` is {kind}, not a component"),
                    span,
                    "only components and tags are attached to entities",
                );
            }
            None if BUILTIN_NAMES.contains(&name) => {
                self.err_label(
                    format!("`{name}` is not a component"),
                    span,
                    "only components and tags are attached to entities",
                );
            }
            None => {
                let candidates = self.component_names();
                self.unknown(
                    format!("unknown component `{name}`"),
                    span,
                    name,
                    &candidates,
                );
            }
        }
    }

    fn component_names(&self) -> Vec<String> {
        self.components.iter().map(|c| c.name.clone()).collect()
    }

    /// The arguments of `spawn` or `add`: component values, and bare names
    /// of tags and of components whose fields all have defaults.
    pub(super) fn component_inits<'b>(
        &mut self,
        args: impl Iterator<Item = &'b ast::Expr>,
    ) -> Option<Vec<tir::ComponentInit>> {
        let mut inits: Vec<tir::ComponentInit> = Vec::new();
        let mut ok = true;
        for arg in args {
            let Some(init) = self.component_init(arg) else {
                ok = false;
                continue;
            };
            if inits.iter().any(|i| i.component == init.component) {
                let name = self.components[init.component.0].name.clone();
                self.err_label(
                    format!("`{name}` is listed twice"),
                    arg.span,
                    "an entity has at most one component of each type",
                );
                ok = false;
                continue;
            }
            inits.push(init);
        }
        ok.then_some(inits)
    }

    fn component_init(&mut self, arg: &ast::Expr) -> Option<tir::ComponentInit> {
        if let ast::ExprKind::Const(name) = &arg.kind
            && !matches!(self.global(name), Some(Global::Const(_)))
        {
            return match self.component_ref(name, arg.span)? {
                ComponentRef::Tag(component) => Some(tir::ComponentInit {
                    component,
                    value: None,
                }),
                ComponentRef::Component(component, record) => {
                    match self.construct_defaults(record, arg.span) {
                        Ok(value) => Some(tir::ComponentInit {
                            component,
                            value: Some(value?),
                        }),
                        Err(missing) => {
                            let fields: Vec<String> =
                                missing.iter().map(|f| format!("{f}: …")).collect();
                            self.err_label(
                                format!("component `{name}` has fields without defaults"),
                                arg.span,
                                format!("construct it: `{name}({})`", fields.join(", ")),
                            );
                            None
                        }
                    }
                }
            };
        }
        let value = self.value(arg, None)?;
        if let Type::Record(record) = value.ty
            && let tir::RecordKind::Component(component) = self.records[record.0].kind
        {
            return Some(tir::ComponentInit {
                component,
                value: Some(value),
            });
        }
        let found = self.type_name(&value.ty);
        self.err_label(
            format!("expected a component, found `{found}`"),
            value.span,
            "an entity is made of component values and tags",
        );
        None
    }

    // ----- indexing -----

    /// `recv[index]`: an element of a list, or a component of an entity.
    pub(super) fn index(
        &mut self,
        recv: &ast::Expr,
        index: &ast::Expr,
        span: Span,
    ) -> Option<Resolved> {
        let Some(base) = self.resolve(recv, Mode::Value) else {
            if !matches!(index.kind, ast::ExprKind::Const(_)) {
                self.probe(index);
            }
            return None;
        };
        match base.ty() {
            Type::Entity => {
                let ast::ExprKind::Const(name) = &index.kind else {
                    self.err_label(
                        "expected the name of a component",
                        index.span,
                        "an entity is indexed by component: `entity[Health]`",
                    );
                    return None;
                };
                if self.cx.const_use().is_some() {
                    self.not_constant(span);
                    return None;
                }
                match self.component_ref(name, index.span)? {
                    ComponentRef::Component(component, record) => Some(Resolved::Place(PlaceRef {
                        place: tir::Place {
                            kind: tir::PlaceKind::Component {
                                entity: Box::new(base.into_value()),
                                component,
                                write: false,
                            },
                            ty: Type::Record(record),
                            span,
                        },
                        readonly: None,
                    })),
                    ComponentRef::Tag(_) => {
                        self.err_label(
                            format!("`{name}` is a tag and has no value"),
                            index.span,
                            format!("test for it with `has?({name})`"),
                        );
                        None
                    }
                }
            }
            Type::List(elem) => {
                let index = self.store(index, &Type::Int)?;
                Some(match base {
                    Resolved::Place(list) => Resolved::Place(PlaceRef {
                        place: tir::Place {
                            kind: tir::PlaceKind::Index {
                                base: Box::new(list.place),
                                index: Box::new(index),
                            },
                            ty: *elem,
                            span,
                        },
                        readonly: list.readonly,
                    }),
                    Resolved::Value(list) => Resolved::Value(mk(
                        ExprKind::Index {
                            base: Box::new(list),
                            index: Box::new(index),
                        },
                        elem.value_type(),
                        span,
                    )),
                })
            }
            Type::Void => {
                self.err("this expression has no value", recv.span);
                None
            }
            other => {
                let name = self.type_name(&other);
                self.err_label(
                    format!("`{name}` can't be indexed"),
                    recv.span,
                    "indexing works on lists, and on entities as `entity[Component]`",
                );
                None
            }
        }
    }

    // ----- queries -----

    /// Checks the terms and filters of an `each` or a `World.count`.
    pub(super) fn query(&mut self, query: &ast::Query) -> CheckedQuery {
        let mut checked = CheckedQuery {
            terms: Vec::new(),
            with: Vec::new(),
            without: Vec::new(),
            ok: true,
        };
        if query.terms.is_empty() {
            self.err_label(
                "a query needs at least one term",
                query.span,
                "to match on filters alone, start with `Entity`",
            );
            checked.ok = false;
        }
        let mut seen: Vec<&str> = Vec::new();
        for term in &query.terms {
            let repeated = self.repeated(&mut seen, term);
            let resolved = if term.name == "Entity" {
                Some(tir::Term::Entity)
            } else {
                self.query_type(term, true)
                    .map(|component| tir::Term::Component {
                        component,
                        write: false,
                        touched: false,
                        overwritten: false,
                    })
            };
            checked.ok &= resolved.is_some() && !repeated;
            checked.terms.push(resolved);
        }
        for (filters, is_with) in [(&query.with, true), (&query.without, false)] {
            for filter in filters {
                if self.repeated(&mut seen, filter) {
                    checked.ok = false;
                    continue;
                }
                if filter.name == "Entity" {
                    self.err_label(
                        "`Entity` can't be a filter",
                        filter.span,
                        "every entity is an `Entity`; filters name components and tags",
                    );
                    checked.ok = false;
                    continue;
                }
                match self.query_type(filter, false) {
                    Some(component) if is_with => checked.with.push(component),
                    Some(component) => checked.without.push(component),
                    None => checked.ok = false,
                }
            }
        }
        checked
    }

    /// Reports a type that already appears in the query.
    fn repeated<'q>(&mut self, seen: &mut Vec<&'q str>, name: &'q ast::Ident) -> bool {
        if seen.contains(&name.name.as_str()) {
            self.err_label(
                format!("`{}` appears twice in this query", name.name),
                name.span,
                "each type may be named once across terms, `with:` and `without:`",
            );
            return true;
        }
        seen.push(&name.name);
        false
    }

    /// A component in a query. A tag is accepted as a filter only.
    fn query_type(&mut self, name: &ast::Ident, term: bool) -> Option<ComponentId> {
        match self.global(&name.name) {
            Some(Global::Tag(_)) if term => {
                self.err_label(
                    format!("tag `{}` can't be a query term", name.name),
                    name.span,
                    format!(
                        "a tag has no data to bind; filter with `with: {}`",
                        name.name
                    ),
                );
                None
            }
            Some(Global::Tag(id)) => Some(id),
            Some(Global::Record(record)) => match self.records[record.0].kind {
                tir::RecordKind::Component(id) => Some(id),
                _ => {
                    self.not_in_query(&name.name, name.span, Some(Global::Record(record)));
                    None
                }
            },
            other => {
                self.not_in_query(&name.name, name.span, other);
                None
            }
        }
    }

    fn not_in_query(&mut self, name: &str, span: Span, global: Option<Global>) {
        const LABEL: &str = "a query names components, tags and `Entity`";
        match global {
            Some(global) => {
                let kind = self.global_kind(global);
                self.err_label(format!("`{name}` is {kind}, not a component"), span, LABEL);
            }
            None if BUILTIN_NAMES.contains(&name) => {
                self.err_label(format!("`{name}` is not a component"), span, LABEL);
            }
            None => {
                let candidates = self.component_names();
                self.unknown(
                    format!("unknown component `{name}`"),
                    span,
                    name,
                    &candidates,
                );
            }
        }
    }

    /// The arguments of `World.count(…)` as a query: positional names are
    /// terms, and `with:` and `without:` take a name or a list of names.
    fn count_query(&mut self, args: &[ast::Arg], span: Span) -> Option<tir::Query> {
        const LABEL: &str = "`World.count` takes the same terms and filters as `each`";
        let mut query = ast::Query {
            terms: Vec::new(),
            with: Vec::new(),
            without: Vec::new(),
            span,
        };
        let mut ok = true;
        let name_of = |checker: &mut Self, expr: &ast::Expr| match &expr.kind {
            ast::ExprKind::Const(name) => Some(ast::Ident {
                name: name.clone(),
                span: expr.span,
            }),
            _ => {
                checker.err_label("expected the name of a component or tag", expr.span, LABEL);
                None
            }
        };
        for arg in args {
            let Some(keyword) = &arg.name else {
                match name_of(self, &arg.value) {
                    Some(name) => query.terms.push(name),
                    None => ok = false,
                }
                continue;
            };
            let filters = match keyword.name.as_str() {
                "with" => &mut query.with,
                "without" => &mut query.without,
                other => {
                    self.err_label(
                        format!("unknown keyword `{other}`"),
                        keyword.span,
                        "`World.count` takes `with:` and `without:`",
                    );
                    ok = false;
                    continue;
                }
            };
            let values: Vec<&ast::Expr> = match &arg.value.kind {
                ast::ExprKind::List(items) => items.iter().collect(),
                _ => vec![&arg.value],
            };
            for value in values {
                match name_of(self, value) {
                    Some(name) => filters.push(name),
                    None => ok = false,
                }
            }
        }
        let checked = self.query(&query);
        if !ok {
            return None;
        }
        checked.finish()
    }
}
