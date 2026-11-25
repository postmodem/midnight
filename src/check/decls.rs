//! Declarations: names, field types, signatures, constants, defaults, and
//! the driver that checks each body.

use crate::ast;
use crate::diag::Diagnostic;
use crate::source::Span;
use crate::tir::{self, Compact, ComponentId, ConstId, EnumId, EventId, FuncId, RecordId, Type};

use super::walk::{self, Node};
use super::{
    BUILTIN_NAMES, BUILTIN_TYPES, BodyKind, Checker, ConstInfo, ConstState, ConstUse, DefaultState,
    EnumInfo, FieldInfo, FnCtx, FnSym, FuncInfo, FuncState, Global, Mode, ParamInfo, ReadOnly,
    RecordInfo, ResourceInfo, Ret, RetState, SystemInfo, TypeUse, int_to_float, util,
};

/// The default of a field, as a constructor that omits the field sees it.
pub(super) enum DefaultValue {
    Absent,
    /// The default exists but failed to check.
    Poison,
    Value(tir::Expr),
}

fn compact(name: &str) -> Option<Compact> {
    Some(match name {
        "Float32" => Compact::Float32,
        "Int8" => Compact::Int8,
        "Int16" => Compact::Int16,
        "Int32" => Compact::Int32,
        "UInt8" => Compact::UInt8,
        "UInt16" => Compact::UInt16,
        "UInt32" => Compact::UInt32,
        _ => return None,
    })
}

impl<'a> Checker<'a> {
    // ----- pass 1: names -----

    pub(super) fn declare(&mut self, program: &'a ast::Program) {
        for item in &program.items {
            match item {
                ast::Item::Require(_) => {}
                ast::Item::Record(decl) => self.declare_record(decl),
                ast::Item::Enum(decl) => self.declare_enum(decl),
                ast::Item::Tag(decl) => {
                    let id = ComponentId(self.components.len());
                    if self.claim_global(&decl.name, Global::Tag(id)) {
                        self.components.push(tir::Component {
                            name: decl.name.name.clone(),
                            record: None,
                        });
                    }
                }
                ast::Item::Def(decl) => {
                    let id = FuncId(self.funcs.len());
                    if self.claim_fn(&decl.name, FnSym::Func(id)) {
                        self.funcs.push(FuncInfo::new(decl, None));
                    }
                }
                ast::Item::System(decl) => {
                    self.plain_name("system", &decl.name.name, decl.name.span);
                    if self.claim_fn(&decl.name, FnSym::System) {
                        self.systems.push(SystemInfo { decl, body: None });
                    }
                }
                ast::Item::Const(decl) => {
                    let id = ConstId(self.consts.len());
                    if self.claim_global(&decl.name, Global::Const(id)) {
                        self.consts.push(ConstInfo {
                            name: decl.name.name.clone(),
                            span: decl.name.span,
                            value: &decl.value,
                            state: ConstState::Pending,
                        });
                    }
                }
                ast::Item::Stmt(stmt) => {
                    match &stmt.kind {
                        ast::StmtKind::VarDecl { name, .. } => {
                            self.top_vars.insert(name.name.clone());
                        }
                        ast::StmtKind::Assign {
                            target:
                                ast::Expr {
                                    kind: ast::ExprKind::Var(name),
                                    ..
                                },
                            ..
                        } => {
                            self.top_vars.insert(name.clone());
                        }
                        _ => {}
                    }
                    self.top.push(stmt);
                }
            }
        }
    }

    /// Registers a capitalized name. Reports a built-in name or a
    /// duplicate, and returns whether the name was free.
    fn claim_global(&mut self, name: &ast::Ident, global: Global) -> bool {
        if BUILTIN_NAMES.contains(&name.name.as_str()) {
            self.err(
                format!("`{}` is a built-in name and can't be redefined", name.name),
                name.span,
            );
            return false;
        }
        if let Some((_, first)) = self.globals.get(&name.name) {
            let first = *first;
            self.duplicate(&name.name, name.span, first);
            return false;
        }
        self.globals.insert(name.name.clone(), (global, name.span));
        true
    }

    /// Registers the name of a function or a system.
    fn claim_fn(&mut self, name: &ast::Ident, sym: FnSym) -> bool {
        if let Some((_, first)) = self.fn_names.get(&name.name) {
            let first = *first;
            self.duplicate(&name.name, name.span, first);
            return false;
        }
        self.fn_names.insert(name.name.clone(), (sym, name.span));
        true
    }

    fn duplicate(&mut self, name: &str, span: Span, first: Span) {
        self.error(
            Diagnostic::new(format!("`{name}` is already defined"), span)
                .note("first defined here", first),
        );
    }

    fn declare_record(&mut self, decl: &'a ast::RecordDecl) {
        let name = decl.name.name.clone();
        if decl.kind == ast::RecordKind::Resource {
            let id = tir::ResourceId(self.resources.len());
            if self.claim_global(&decl.name, Global::Resource(id)) {
                self.resources.push(ResourceInfo {
                    name,
                    decl,
                    fields: Vec::new(),
                });
            }
            return;
        }
        let id = RecordId(self.records.len());
        if !self.claim_global(&decl.name, Global::Record(id)) {
            return;
        }
        let kind = match decl.kind {
            ast::RecordKind::Component => {
                let component = ComponentId(self.components.len());
                self.components.push(tir::Component {
                    name: name.clone(),
                    record: Some(id),
                });
                tir::RecordKind::Component(component)
            }
            ast::RecordKind::Event => {
                let event = EventId(self.events.len());
                self.events.push(tir::Event {
                    name: name.clone(),
                    record: id,
                });
                tir::RecordKind::Event(event)
            }
            _ => tir::RecordKind::Struct,
        };
        self.records.push(RecordInfo {
            name,
            kind,
            decl,
            fields: Vec::new(),
            methods: Vec::new(),
        });
    }

    fn declare_enum(&mut self, decl: &ast::EnumDecl) {
        let id = EnumId(self.enums.len());
        if !self.claim_global(&decl.name, Global::Enum(id)) {
            return;
        }
        let mut variants: Vec<String> = Vec::new();
        for (i, variant) in decl.variants.iter().enumerate() {
            if variants.contains(&variant.name) {
                let first = decl.variants[..i]
                    .iter()
                    .find(|v| v.name == variant.name)
                    .map_or(variant.span, |v| v.span);
                self.duplicate(&variant.name, variant.span, first);
            } else {
                variants.push(variant.name.clone());
            }
        }
        if variants.is_empty() {
            self.err(
                format!("enum `{}` needs at least one variant", decl.name.name),
                decl.name.span,
            );
        }
        self.enums.push(EnumInfo {
            name: decl.name.name.clone(),
            variants,
        });
    }

    // ----- pass 2: field types and signatures -----

    pub(super) fn resolve_signatures(&mut self) {
        let functions = self.funcs.len();
        for i in 0..self.records.len() {
            self.resolve_record(RecordId(i));
        }
        for i in 0..self.resources.len() {
            self.resolve_resource(i);
        }
        for i in 0..functions {
            let decl = self.funcs[i].decl;
            let (params, ret) = self.signature(decl);
            self.funcs[i].params = params;
            self.funcs[i].ret = ret;
        }
        self.check_containment();
    }

    /// Resolves a list of field declarations, reporting repeated names.
    fn fields(&mut self, decls: &'a [ast::FieldDecl]) -> Vec<FieldInfo<'a>> {
        let mut fields: Vec<FieldInfo<'a>> = Vec::new();
        for field in decls {
            if let Some(first) = fields.iter().find(|f| f.name == field.name.name) {
                let first = first.span;
                self.duplicate(&field.name.name, field.name.span, first);
                continue;
            }
            self.plain_name("field", &field.name.name, field.name.span);
            let ty = self.resolve_type(&field.ty, TypeUse::Field);
            fields.push(FieldInfo {
                name: field.name.name.clone(),
                span: field.name.span,
                ty,
                default: match &field.default {
                    Some(default) => DefaultState::Pending(default),
                    None => DefaultState::Absent,
                },
            });
        }
        fields
    }

    fn resolve_record(&mut self, id: RecordId) {
        let decl = self.records[id.0].decl;
        let fields = self.fields(&decl.fields);
        if decl.kind == ast::RecordKind::Component && decl.fields.is_empty() {
            self.err_label(
                format!("component `{}` needs at least one field", decl.name.name),
                decl.name.span,
                format!(
                    "a component with no data is a tag: `tag {}`",
                    decl.name.name
                ),
            );
        }
        self.records[id.0].fields = fields;

        for method in &decl.methods {
            let name = method.name.name.as_str();
            if name == "to_s" || name == "==" {
                self.err(
                    format!("`{name}` is provided for every type and can't be redefined"),
                    method.name.span,
                );
                continue;
            }
            if let Some(field) = self.field_index(id, name) {
                let first = self.records[id.0].fields[field].span;
                self.duplicate(name, method.name.span, first);
                continue;
            }
            if let Some(first) = self.method(id, name) {
                let first = self.funcs[first.0].span;
                self.duplicate(name, method.name.span, first);
                continue;
            }
            let wanted = match name {
                "+" | "-" | "*" | "/" | "%" => Some(1),
                "-@" => Some(0),
                _ => None,
            };
            if let Some(wanted) = wanted
                && method.params.len() != wanted
            {
                let mut diagnostic = Diagnostic::new(
                    format!(
                        "operator method `{name}` takes {}, but {} declared",
                        util::count(wanted, "parameter"),
                        match method.params.len() {
                            0 => "none are".to_string(),
                            1 => "1 is".to_string(),
                            n => format!("{n} are"),
                        }
                    ),
                    method.name.span,
                );
                if name == "-" && method.params.is_empty() {
                    diagnostic = diagnostic.label("unary minus is defined with `def -@`");
                }
                self.error(diagnostic);
                continue;
            }
            let func = FuncId(self.funcs.len());
            let (params, ret) = self.signature(method);
            let mut info = FuncInfo::new(method, Some(id));
            info.params = params;
            info.ret = ret;
            self.funcs.push(info);
            self.records[id.0].methods.push((name.to_string(), func));
        }
    }

    fn resolve_resource(&mut self, index: usize) {
        let decl = self.resources[index].decl;
        let fields = self.fields(&decl.fields);
        for field in &fields {
            if matches!(field.default, DefaultState::Absent) {
                self.err_label(
                    format!(
                        "field `{}` of resource `{}` needs a default",
                        field.name, decl.name.name
                    ),
                    field.span,
                    "a resource exists from the start, so every field has a default",
                );
            }
        }
        self.resources[index].fields = fields;
    }

    fn signature(&mut self, decl: &ast::FuncDecl) -> (Vec<ParamInfo>, RetState) {
        let mut params: Vec<ParamInfo> = Vec::new();
        for param in &decl.params {
            if let Some(first) = params.iter().find(|p| p.name == param.name.name) {
                let first = first.span;
                self.duplicate(&param.name.name, param.name.span, first);
            }
            self.plain_name("parameter", &param.name.name, param.name.span);
            let ty = self.resolve_type(&param.ty, TypeUse::Parameter);
            params.push(ParamInfo {
                name: param.name.name.clone(),
                span: param.name.span,
                ty,
            });
        }
        let ret = match &decl.ret {
            Some(ty) => RetState::Known(self.resolve_type(ty, TypeUse::Return)),
            None => RetState::Unknown,
        };
        (params, ret)
    }

    /// Resolves a written type. Compact types are accepted for fields and
    /// list elements only; tags and resources are never value types.
    pub(super) fn resolve_type(&mut self, ty: &ast::TypeExpr, usage: TypeUse) -> Option<Type> {
        let name = match &ty.kind {
            ast::TypeExprKind::List(elem) => {
                let elem = self.resolve_type(elem, TypeUse::Element)?;
                return Some(Type::List(Box::new(elem)));
            }
            ast::TypeExprKind::Name(name) => name.as_str(),
        };
        match name {
            "Int" => return Some(Type::Int),
            "Float" => return Some(Type::Float),
            "Bool" => return Some(Type::Bool),
            "String" => return Some(Type::String),
            "Entity" => return Some(Type::Entity),
            "World" | "Time" | "Math" => {
                self.err(format!("`{name}` is not a type"), ty.span);
                return None;
            }
            _ => {}
        }
        if let Some(compact) = compact(name) {
            let what = match usage {
                TypeUse::Field | TypeUse::Element => return Some(Type::Compact(compact)),
                TypeUse::Variable => "a variable",
                TypeUse::Parameter => "a parameter",
                TypeUse::Return => "a return value",
            };
            let plain = if compact == Compact::Float32 {
                "Float"
            } else {
                "Int"
            };
            self.err_label(
                format!("`{name}` can't be the type of {what}"),
                ty.span,
                format!(
                    "variables, parameters and return values are `Int` or `Float`; use `{plain}`"
                ),
            );
            return None;
        }
        match self.global(name) {
            Some(Global::Record(id)) => Some(Type::Record(id)),
            Some(Global::Enum(id)) => Some(Type::Enum(id)),
            Some(Global::Tag(_)) => {
                self.err_label(
                    format!("`{name}` is a tag, not a value type"),
                    ty.span,
                    "a tag has no value: it can only be attached to entities and named in queries",
                );
                None
            }
            Some(Global::Resource(_)) => {
                self.err_label(
                    format!("`{name}` is a resource, not a value type"),
                    ty.span,
                    format!(
                        "a resource is one global instance; use its fields, like `{name}.field`"
                    ),
                );
                None
            }
            Some(Global::Const(_)) => {
                self.err(format!("`{name}` is a constant, not a type"), ty.span);
                None
            }
            None => {
                let mut candidates = self.type_names();
                candidates.extend(BUILTIN_TYPES.iter().map(|n| n.to_string()));
                self.unknown(format!("unknown type `{name}`"), ty.span, name, &candidates);
                None
            }
        }
    }

    /// Names of the declared value types: structs, components, events and
    /// enums.
    pub(super) fn type_names(&self) -> Vec<String> {
        self.globals
            .iter()
            .filter(|(_, (global, _))| matches!(global, Global::Record(_) | Global::Enum(_)))
            .map(|(name, _)| name.clone())
            .collect()
    }

    /// Reports each record that holds itself by value, directly or through
    /// other records. The field that starts each cycle loses its type, so
    /// that no cycle is left and none is reported twice.
    fn check_containment(&mut self) {
        let count = self.records.len();
        let mut start = 0;
        while start < count {
            let mut path: Vec<(usize, usize)> = Vec::new();
            let mut visited = vec![false; count];
            if !self.find_cycle(start, start, &mut visited, &mut path) {
                start += 1;
                continue;
            }
            let (record, field) = path[0];
            let name = self.records[record].name.clone();
            let kind = &self.global_kind(Global::Record(RecordId(record)))[2..];
            let kind = kind.trim_start();
            let through: Vec<String> = path[1..]
                .iter()
                .map(|&(r, _)| self.records[r].name.clone())
                .collect();
            let message = if through.is_empty() {
                format!("{kind} `{name}` contains itself")
            } else {
                format!(
                    "{kind} `{name}` contains itself through {}",
                    util::name_list(&through)
                )
            };
            let span = self.records[record].fields[field].span;
            self.err_label(
                message,
                span,
                format!("a value can contain its own type only through a list, like `[{name}]`"),
            );
            self.records[record].fields[field].ty = None;
        }
    }

    fn find_cycle(
        &self,
        target: usize,
        at: usize,
        visited: &mut [bool],
        path: &mut Vec<(usize, usize)>,
    ) -> bool {
        for (i, field) in self.records[at].fields.iter().enumerate() {
            let Some(Type::Record(next)) = &field.ty else {
                continue;
            };
            path.push((at, i));
            if next.0 == target {
                return true;
            }
            if !visited[next.0] {
                visited[next.0] = true;
                if self.find_cycle(target, next.0, visited, path) {
                    return true;
                }
            }
            path.pop();
        }
        false
    }

    // ----- pass 3: constants and defaults -----

    pub(super) fn check_constants(&mut self) {
        for i in 0..self.consts.len() {
            self.force_const(ConstId(i));
        }
        for record in 0..self.records.len() {
            for field in 0..self.records[record].fields.len() {
                self.field_default(RecordId(record), field);
            }
        }
        for resource in 0..self.resources.len() {
            for field in 0..self.resources[resource].fields.len() {
                let info = &self.resources[resource].fields[field];
                let DefaultState::Pending(default) = info.default else {
                    continue;
                };
                let ty = info.ty.clone();
                let init = self.constant_expr(default, ty.as_ref(), ConstUse::FieldDefault);
                self.resources[resource].fields[field].default = DefaultState::Done(init);
            }
        }
    }

    /// Checks a constant expression, converted to `ty` when one is given.
    fn constant_expr(
        &mut self,
        expr: &ast::Expr,
        ty: Option<&Type>,
        usage: ConstUse,
    ) -> Option<tir::Expr> {
        let outer = std::mem::replace(&mut self.cx, FnCtx::new(BodyKind::Const(usage)));
        let result = match ty {
            Some(ty) => self.store(expr, ty),
            None => self.value(expr, None),
        };
        self.cx = outer;
        result
    }

    /// Types a constant if that has not been done yet.
    pub(super) fn force_const(&mut self, id: ConstId) {
        if !matches!(self.consts[id.0].state, ConstState::Pending) {
            return;
        }
        self.consts[id.0].state = ConstState::InProgress;
        let value = self.consts[id.0].value;
        let init = self.constant_expr(value, None, ConstUse::Constant(id));
        let init = init.filter(|init| self.reads_only_earlier(id, init));
        let int = init.as_ref().and_then(|init| self.const_int(init));
        self.consts[id.0].state = ConstState::Done { init, int };
    }

    /// Whether a constant's value reads only constants defined above it.
    /// A field default filled into a constructor can read any constant, so
    /// this looks at the finished expression.
    fn reads_only_earlier(&mut self, id: ConstId, init: &tir::Expr) -> bool {
        let mut later: Option<(ConstId, Span)> = None;
        let mut copy = init.clone();
        walk::expr(&mut copy, &mut |node| {
            if let Node::Expr(expr) = node
                && let tir::ExprKind::Read(place) = &expr.kind
            {
                let mut root = place;
                while let tir::PlaceKind::Field { base, .. } | tir::PlaceKind::Index { base, .. } =
                    &root.kind
                {
                    root = base;
                }
                if let tir::PlaceKind::Const(read) = root.kind
                    && read.0 >= id.0
                    && later.is_none()
                {
                    later = Some((read, root.span));
                }
            }
        });
        let Some((read, span)) = later else {
            return true;
        };
        let name = self.consts[id.0].name.clone();
        let read = self.consts[read.0].name.clone();
        let decl = self.consts[id.0].span;
        self.error(
            Diagnostic::new(
                format!("constant `{name}` uses `{read}` before it is defined"),
                decl,
            )
            .label(format!(
                "a field default in its value reads `{read}`; define `{read}` above `{name}`"
            ))
            .note(format!("`{read}` is read here"), span),
        );
        false
    }

    /// The type of a constant; `None` when its value failed to check.
    pub(super) fn const_type(&self, id: ConstId) -> Option<Type> {
        match &self.consts[id.0].state {
            ConstState::Done {
                init: Some(init), ..
            } => Some(init.ty.clone()),
            _ => None,
        }
    }

    /// A read of a constant by name, as a read-only place.
    pub(super) fn constant(&mut self, id: ConstId, span: Span) -> Option<super::PlaceRef> {
        let name = self.consts[id.0].name.clone();
        if let Some(ConstUse::Constant(current)) = self.cx.const_use()
            && id.0 >= current.0
        {
            if id == current {
                self.err(format!("constant `{name}` is used in its own value"), span);
            } else {
                self.err_label(
                    format!("constant `{name}` is used before it is defined"),
                    span,
                    "a constant can use only the constants defined above it",
                );
            }
            return None;
        }
        match self.consts[id.0].state {
            ConstState::Pending => self.force_const(id),
            ConstState::InProgress => {
                self.err_label(
                    format!("constant `{name}` is used in its own value"),
                    span,
                    "a field default in its value reads it",
                );
                return None;
            }
            ConstState::Done { .. } => {}
        }
        let ty = self.const_type(id)?;
        Some(super::PlaceRef {
            place: tir::Place {
                kind: tir::PlaceKind::Const(id),
                ty,
                span,
            },
            readonly: Some(ReadOnly::Const(name)),
        })
    }

    /// The default of a record field, typed on first use.
    pub(super) fn field_default(&mut self, record: RecordId, index: usize) -> DefaultValue {
        let field = &mut self.records[record.0].fields[index];
        match std::mem::replace(&mut field.default, DefaultState::InProgress) {
            DefaultState::Absent => {
                field.default = DefaultState::Absent;
                DefaultValue::Absent
            }
            DefaultState::Done(value) => {
                let result = value.clone();
                field.default = DefaultState::Done(value);
                result.map_or(DefaultValue::Poison, DefaultValue::Value)
            }
            DefaultState::InProgress => {
                let (name, span) = (field.name.clone(), field.span);
                self.err(format!("the default of `{name}` depends on itself"), span);
                DefaultValue::Poison
            }
            DefaultState::Pending(default) => {
                let ty = field.ty.clone();
                let value = match &ty {
                    Some(ty) => self.constant_expr(default, Some(ty), ConstUse::FieldDefault),
                    None => {
                        self.constant_expr(default, None, ConstUse::FieldDefault);
                        None
                    }
                };
                self.records[record.0].fields[index].default = DefaultState::Done(value.clone());
                value.map_or(DefaultValue::Poison, DefaultValue::Value)
            }
        }
    }

    /// The value of an `Int` expression the checker can compute: literals,
    /// arithmetic on them, and `Int` constants.
    pub(super) fn const_int(&self, expr: &tir::Expr) -> Option<i64> {
        match &expr.kind {
            tir::ExprKind::Int(value) => Some(*value),
            tir::ExprKind::Unary {
                op: tir::UnOp::Neg,
                expr: inner,
            } => Some(self.const_int(inner)?.wrapping_neg()),
            tir::ExprKind::Binary { op, lhs, rhs } if expr.ty == Type::Int => {
                let a = self.const_int(lhs)?;
                let b = self.const_int(rhs)?;
                match op {
                    tir::BinOp::Add => Some(a.wrapping_add(b)),
                    tir::BinOp::Sub => Some(a.wrapping_sub(b)),
                    tir::BinOp::Mul => Some(a.wrapping_mul(b)),
                    tir::BinOp::Div => util::floor_div(a, b),
                    tir::BinOp::Mod => util::floor_mod(a, b),
                    tir::BinOp::Pow => util::wrapping_pow(a, b),
                    _ => None,
                }
            }
            tir::ExprKind::Read(tir::Place {
                kind: tir::PlaceKind::Const(id),
                ..
            }) => match self.consts[id.0].state {
                ConstState::Done { int, .. } => int,
                _ => None,
            },
            _ => None,
        }
    }

    // ----- pass 4: bodies -----

    pub(super) fn check_bodies(&mut self) {
        self.cx = FnCtx::new(BodyKind::Top);
        let top = self.top.clone();
        let (block, _) = self.stmts(top.into_iter(), Mode::Stmt, None);
        let cx = std::mem::replace(&mut self.cx, FnCtx::new(BodyKind::Top));
        self.main = Some(tir::Body {
            locals: cx.locals,
            block,
        });

        for i in 0..self.funcs.len() {
            if self.funcs[i].state == FuncState::Pending {
                self.check_func(FuncId(i));
            }
        }

        for i in 0..self.systems.len() {
            let decl = self.systems[i].decl;
            self.cx = FnCtx::new(BodyKind::System);
            let (block, _) = self.block(&decl.body, Mode::Stmt, None);
            let cx = std::mem::replace(&mut self.cx, FnCtx::new(BodyKind::Top));
            self.systems[i].body = Some(tir::Body {
                locals: cx.locals,
                block,
            });
        }
    }

    /// Types the body of a function or method, inferring its return type
    /// when none is declared.
    pub(super) fn check_func(&mut self, id: FuncId) {
        self.funcs[id.0].state = FuncState::InProgress;
        let decl = self.funcs[id.0].decl;
        let declared = match &self.funcs[id.0].ret {
            RetState::Known(ty) => Some(ty.clone()),
            RetState::Unknown => None,
        };
        let outer = std::mem::replace(&mut self.cx, FnCtx::new(BodyKind::Func(id)));
        self.cx.ret = match &declared {
            Some(ty) => Ret::Declared(ty.clone()),
            None => Ret::Inferred,
        };

        let mut param_locals = Vec::new();
        if let Some(owner) = self.funcs[id.0].owner {
            let local = self.declare_local(
                "self",
                decl.name.span,
                Some(Type::Record(owner)),
                tir::LocalKind::Param,
                Some(ReadOnly::SelfValue),
            );
            self.cx.self_ = Some((local, owner));
            param_locals.push(local);
        }
        for i in 0..self.funcs[id.0].params.len() {
            let param = &self.funcs[id.0].params[i];
            let (name, span, ty) = (param.name.clone(), param.span, param.ty.clone());
            let readonly = Some(ReadOnly::Param(name.clone()));
            let local = self.declare_local(&name, span, ty, tir::LocalKind::Param, readonly);
            param_locals.push(local);
        }

        let (block, ret) = match declared {
            Some(ty) => self.declared_body(decl, ty),
            None => self.inferred_body(id, decl),
        };
        let cx = std::mem::replace(&mut self.cx, outer);
        let func = &mut self.funcs[id.0];
        func.body = Some(tir::Body {
            locals: cx.locals,
            block,
        });
        func.param_locals = param_locals;
        func.ret = RetState::Known(ret);
        func.state = FuncState::Done;
    }

    fn declared_body(
        &mut self,
        decl: &ast::FuncDecl,
        ty: Option<Type>,
    ) -> (tir::Block, Option<Type>) {
        let (mut block, tail) = self.block(&decl.body, Mode::Maybe, ty.as_ref());
        let Some(ty) = ty else {
            return (block, None);
        };
        match tail {
            super::stmts::Tail::Value => {
                let value = *block.value.take().expect("a block with a value");
                block.value = self.convert(value, &ty, None).map(Box::new);
            }
            super::stmts::Tail::Poison => {}
            super::stmts::Tail::None => {
                if !self.cx.unreachable {
                    self.missing_return(decl, &ty);
                }
            }
        }
        (block, Some(ty))
    }

    fn missing_return(&mut self, decl: &ast::FuncDecl, ty: &Type) {
        let ty = self.type_name(ty);
        self.err_label(
            format!("`{}` can end without returning a value", decl.name.name),
            decl.name.span,
            format!("it returns `{ty}`: end every path with a value or a `return`"),
        );
    }

    fn inferred_body(&mut self, id: FuncId, decl: &ast::FuncDecl) -> (tir::Block, Option<Type>) {
        use super::stmts::Tail;
        let name = decl.name.name.clone();
        let (mut block, tail) = self.block(&decl.body, Mode::Maybe, None);
        let sites = std::mem::take(&mut self.cx.ret_sites);
        let recursive_tail = self.cx.recursive_tail.take();

        let mut ty: Option<Type> = None;
        let mut widened = false;
        let mut failed = self.cx.ret_poison || tail == Tail::Poison;
        let tail_site = match (&tail, &block.value) {
            (Tail::Value, Some(value)) => Some((value.ty.clone(), value.span)),
            _ => None,
        };
        let valued = sites
            .iter()
            .filter_map(|site| Some((site.ty.clone()?, site.span)));
        let any_return_value = sites.iter().any(|site| site.ty.is_some());
        let bare: Vec<Span> = sites
            .iter()
            .filter(|site| site.ty.is_none())
            .map(|site| site.span)
            .collect();

        // A function whose only `return`s are bare returns nothing, and its
        // last expression is evaluated for effect.
        let candidates: Vec<(Type, Span)> = if !any_return_value && !bare.is_empty() {
            Vec::new()
        } else {
            valued.chain(tail_site).collect()
        };
        for (found, span) in &candidates {
            match &ty {
                None => ty = Some(found.clone()),
                Some(current) if current == found => {}
                Some(current) if super::is_number(current) && super::is_number(found) => {
                    ty = Some(Type::Float);
                    widened = true;
                }
                Some(current) => {
                    let current = current.clone();
                    self.mismatch(
                        &current,
                        found,
                        *span,
                        Some(format!(
                            "`{name}` returns `{}` earlier",
                            self.type_name(&current)
                        )),
                    );
                    failed = true;
                }
            }
        }

        // The body may end in a call whose type was needed but was still
        // being inferred, here or in the function it calls.
        let ends_in_recursion = recursive_tail.or_else(|| match block.stmts.last() {
            Some(tir::Stmt {
                kind:
                    tir::StmtKind::Expr(tir::Expr {
                        kind: tir::ExprKind::Call { func, .. },
                        span,
                        ..
                    }),
                ..
            }) if tail == Tail::None && self.funcs[func.0].void_by_recursion => {
                Some((*func, *span))
            }
            _ => None,
        });

        let Some(ty) = ty else {
            // Nothing is returned.
            if let Some(value) = block.value.take() {
                let span = value.span;
                block.stmts.push(tir::Stmt {
                    kind: tir::StmtKind::Expr(*value),
                    span,
                });
            }
            if failed {
                return (block, None);
            }
            if ends_in_recursion.is_some() && bare.is_empty() {
                self.funcs[id.0].void_by_recursion = true;
            }
            return (block, Some(Type::Void));
        };

        for span in bare {
            self.err_label(
                "`return` needs a value",
                span,
                format!("`{name}` returns `{}` elsewhere", self.type_name(&ty)),
            );
            failed = true;
        }
        if tail == Tail::None && !self.cx.unreachable {
            match ends_in_recursion {
                Some((callee, span)) => self.recursion_error(callee, span),
                None => self.missing_return(decl, &ty),
            }
            failed = true;
        }
        if widened {
            walk::block(&mut block, &mut |node| {
                if let Node::Stmt(stmt) = node
                    && let tir::StmtKind::Return(Some(value)) = &mut stmt.kind
                    && value.ty == Type::Int
                {
                    let inner = std::mem::replace(
                        value,
                        super::mk(tir::ExprKind::Int(0), Type::Int, stmt.span),
                    );
                    *value = int_to_float(inner);
                }
            });
            if let Some(value) = block.value.take() {
                block.value = Some(if value.ty == Type::Int {
                    Box::new(int_to_float(*value))
                } else {
                    value
                });
            }
        }
        (block, if failed { None } else { Some(ty) })
    }

    /// Reports a call that needs the return type of a function whose own
    /// inference is still under way.
    pub(super) fn recursion_error(&mut self, callee: FuncId, span: Span) {
        let func = &mut self.funcs[callee.0];
        if func.recursion_reported {
            return;
        }
        func.recursion_reported = true;
        let (name, decl) = (func.name.clone(), func.span);
        self.error(
            Diagnostic::new(
                format!("recursive function `{name}` needs a declared return type"),
                span,
            )
            .label(format!(
                "this call needs the type `{name}` returns, which is still being inferred"
            ))
            .note(format!("add `-> Type` to the definition of `{name}`"), decl),
        );
    }
}

impl<'a> FuncInfo<'a> {
    fn new(decl: &'a ast::FuncDecl, owner: Option<RecordId>) -> FuncInfo<'a> {
        FuncInfo {
            name: decl.name.name.clone(),
            span: decl.name.span,
            decl,
            owner,
            params: Vec::new(),
            ret: RetState::Unknown,
            state: FuncState::Pending,
            body: None,
            param_locals: Vec::new(),
            recursion_reported: false,
            void_by_recursion: false,
        }
    }
}
