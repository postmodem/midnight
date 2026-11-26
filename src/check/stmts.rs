//! Statements and blocks: variables, assignment, loops, queries, `on`,
//! `return`, and the rules for what may be written.

use crate::ast;
use crate::diag::Diagnostic;
use crate::source::Span;
use crate::tir::{self, BinOp, ExprKind, LocalId, StmtKind, Type};

use super::{
    BUILTIN_NAMES, BodyKind, Checker, FLOAT_TO_INT, Global, Mode, ReadOnly, Resolved, Ret, RetSite,
    TypeUse, mk, util, walk,
};

/// How a block ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Tail {
    /// With a statement, or with an expression that has no value.
    None,
    /// With an expression that failed to check.
    Poison,
    /// With a value, stored in the block's `value`.
    Value,
}

/// A parameter of a `do` block, before it is declared.
struct ParamSpec<'b> {
    name: &'b ast::Ident,
    ty: Option<Type>,
    kind: tir::LocalKind,
    readonly: Option<ReadOnly>,
}

fn is_loop(expr: &ast::Expr) -> bool {
    matches!(expr.kind, ast::ExprKind::Member { block: Some(_), .. })
}

/// The arguments of `Window.open(…)` written as a statement.
fn window_open(expr: &ast::Expr) -> Option<&[ast::Arg]> {
    match &expr.kind {
        ast::ExprKind::Member {
            recv,
            name,
            args: Some(args),
            block: None,
        } if name.name == "open"
            && matches!(&recv.kind, ast::ExprKind::Const(owner) if owner == "Window") =>
        {
            Some(args)
        }
        _ => None,
    }
}

impl Checker<'_> {
    /// Checks the block of a whole body in a scope of its own.
    pub(super) fn block(
        &mut self,
        block: &ast::Block,
        mode: Mode,
        expected: Option<&Type>,
    ) -> (tir::Block, Tail) {
        self.scoped_block(None, block, mode, expected)
    }

    /// Checks a block in a scope of its own. `construct` names what the
    /// block belongs to: "the `if`".
    pub(super) fn scoped_block(
        &mut self,
        construct: Option<&'static str>,
        block: &ast::Block,
        mode: Mode,
        expected: Option<&Type>,
    ) -> (tir::Block, Tail) {
        self.push_scope(construct);
        let result = self.stmts(block.stmts.iter(), mode, expected);
        self.pop_scope();
        result
    }

    /// Checks statements in the current scope. Unless the mode is `Stmt`,
    /// a last statement that is an expression with a value becomes the
    /// block's value.
    pub(super) fn stmts<'b>(
        &mut self,
        stmts: impl ExactSizeIterator<Item = &'b ast::Stmt>,
        mode: Mode,
        expected: Option<&Type>,
    ) -> (tir::Block, Tail) {
        let count = stmts.len();
        let mut out = Vec::new();
        let mut value = None;
        let mut tail = Tail::None;
        for (i, stmt) in stmts.enumerate() {
            if i + 1 == count
                && mode != Mode::Stmt
                && let ast::StmtKind::Expr(e) = &stmt.kind
                && !is_loop(e)
                && window_open(e).is_none()
            {
                match self.expr(e, mode, expected) {
                    None => tail = Tail::Poison,
                    Some(result) if result.ty == Type::Void => out.push(tir::Stmt {
                        kind: StmtKind::Expr(result),
                        span: stmt.span,
                    }),
                    Some(result) => {
                        value = Some(Box::new(result));
                        tail = Tail::Value;
                    }
                }
            } else if let Some(checked) = self.stmt(stmt) {
                out.push(checked);
            }
        }
        (tir::Block { stmts: out, value }, tail)
    }

    fn stmt(&mut self, stmt: &ast::Stmt) -> Option<tir::Stmt> {
        let span = stmt.span;
        let kind = match &stmt.kind {
            ast::StmtKind::Expr(e) => match (&e.kind, window_open(e)) {
                (_, Some(args)) => self.window_open(args, e.span)?,
                (
                    ast::ExprKind::Member {
                        recv,
                        name,
                        args,
                        block: Some(block),
                    },
                    None,
                ) => self.loop_stmt(recv, name, args.as_deref(), block)?,
                _ => StmtKind::Expr(self.expr(e, Mode::Stmt, None)?),
            },
            ast::StmtKind::VarDecl { name, ty, value } => self.var_decl(name, ty, value)?,
            ast::StmtKind::Assign { target, op, value } => self.assign(target, *op, value, span)?,
            ast::StmtKind::Return(value) => self.return_stmt(value.as_ref(), span)?,
            ast::StmtKind::Break => self.jump("break", span).then_some(StmtKind::Break)?,
            ast::StmtKind::Next => self.jump("next", span).then_some(StmtKind::Next)?,
            ast::StmtKind::Puts(args) => StmtKind::Puts(self.values(args)?),
            ast::StmtKind::Print(args) => StmtKind::Print(self.values(args)?),
            ast::StmtKind::Emit(event) => self.emit(event)?,
            ast::StmtKind::While { cond, negate, body } => {
                let cond = self.condition(cond, *negate);
                let construct = if *negate {
                    "the `until`"
                } else {
                    "the `while`"
                };
                let (_, body, saw_break) = self.loop_body(construct, Vec::new(), body);
                let cond = cond?;
                if matches!(cond.kind, ExprKind::Bool(true)) && !saw_break {
                    self.cx.unreachable = true;
                }
                StmtKind::While { cond, body }
            }
            ast::StmtKind::Loop { body } => {
                let (_, body, saw_break) = self.loop_body("the `loop`", Vec::new(), body);
                if !saw_break {
                    self.cx.unreachable = true;
                }
                StmtKind::While {
                    cond: mk(ExprKind::Bool(true), Type::Bool, span),
                    body,
                }
            }
            ast::StmtKind::Each {
                query,
                params,
                body,
            } => self.each(query, params, body)?,
            ast::StmtKind::On {
                event,
                params,
                body,
            } => self.on(event, params, body)?,
        };
        Some(tir::Stmt { kind, span })
    }

    /// The arguments of `puts` or `print`: any values.
    fn values(&mut self, args: &[ast::Expr]) -> Option<Vec<tir::Expr>> {
        let values: Vec<Option<tir::Expr>> = args.iter().map(|arg| self.value(arg, None)).collect();
        values.into_iter().collect()
    }

    /// `break` or `next`: reports one outside a loop.
    fn jump(&mut self, keyword: &str, span: Span) -> bool {
        let Some(saw_break) = self.cx.loops.last_mut() else {
            self.err_label(
                format!("`{keyword}` outside a loop"),
                span,
                "it belongs in `while`, `until`, `loop`, `each`, `on` or a `do` block",
            );
            return false;
        };
        if keyword == "break" {
            *saw_break = true;
        }
        self.cx.unreachable = true;
        true
    }

    // ----- variables and assignment -----

    /// `name: Type = value`.
    fn var_decl(
        &mut self,
        name: &ast::Ident,
        ty: &ast::TypeExpr,
        value: &ast::Expr,
    ) -> Option<StmtKind> {
        self.plain_name("variable", &name.name, name.span);
        let ty = self.resolve_type(ty, TypeUse::Variable);
        let init = self.store_opt(value, ty.as_ref());
        if let Some(existing) = self.cx.lookup(&name.name) {
            let first = self.cx.meta[existing.0].span;
            self.error(
                Diagnostic::new(
                    format!("variable `{}` already exists", name.name),
                    name.span,
                )
                .label("a type is given only where a variable is created")
                .note(format!("`{}` is created here", name.name), first),
            );
            return None;
        }
        let local = self.declare_local(&name.name, name.span, ty, tir::LocalKind::Var, None);
        Some(StmtKind::Let { local, init: init? })
    }

    fn is_self_field(&self, name: &str) -> bool {
        self.cx
            .self_
            .is_some_and(|(_, record)| self.field_index(record, name).is_some())
    }

    fn assign(
        &mut self,
        target: &ast::Expr,
        op: ast::AssignOp,
        value: &ast::Expr,
        span: Span,
    ) -> Option<StmtKind> {
        // The first assignment to a name creates the variable.
        if let ast::ExprKind::Var(name) = &target.kind
            && op == ast::AssignOp::Set
            && self.cx.lookup(name).is_none()
            && !self.is_self_field(name)
        {
            self.plain_name("variable", name, target.span);
            let init = self.value(value, None);
            let ty = init.as_ref().map(|v| v.ty.clone());
            let local = self.declare_local(name, target.span, ty, tir::LocalKind::Var, None);
            return Some(StmtKind::Let { local, init: init? });
        }
        if let ast::ExprKind::Const(name) = &target.kind
            && self.global(name).is_none()
            && !BUILTIN_NAMES.contains(&name.as_str())
        {
            self.err_label(
                format!("constant `{name}` can't be defined here"),
                target.span,
                "a constant is defined by an assignment at the top level",
            );
            self.probe(value);
            return None;
        }

        let Some(resolved) = self.resolve(target, Mode::Value) else {
            self.probe(value);
            return None;
        };
        let described = util::describe(target);
        let Resolved::Place(mut target_place) = resolved else {
            let message = match described {
                Some(text) => format!("can't assign to `{text}`"),
                None => "can't assign to this expression".to_string(),
            };
            self.err_label(
                message,
                target.span,
                "only variables, query parameters, `entity[Component]`, resource fields and their parts can be assigned",
            );
            self.probe(value);
            return None;
        };
        if let Some(readonly) = &target_place.readonly {
            let text = described.unwrap_or_else(|| "this".to_string());
            self.err_label(
                format!("can't assign to `{text}`"),
                target.span,
                readonly.reason(),
            );
            self.probe(value);
            return None;
        }
        self.mark_write(&mut target_place.place);
        let place = target_place.place;
        let ty = place.ty.value_type();

        if op == ast::AssignOp::Set {
            let value = self.value(value, Some(&place.ty))?;
            if let tir::PlaceKind::Local(local) = place.kind
                && value.ty != ty
                && !(value.ty == Type::Int && ty == Type::Float)
            {
                let name = self.cx.locals[local.0].name.clone();
                let first = self.cx.meta[local.0].span;
                let message = format!(
                    "can't assign `{}` to `{name}`, which is `{}`",
                    self.type_name(&value.ty),
                    self.type_name(&ty)
                );
                const KEEPS: &str = "a variable keeps the type of its first value";
                let diagnostic = Diagnostic::new(message, value.span);
                self.error(if value.ty == Type::Float && ty == Type::Int {
                    diagnostic
                        .label(FLOAT_TO_INT)
                        .note(format!("{KEEPS}: `{name}` gets its type here"), first)
                } else {
                    diagnostic
                        .label(KEEPS)
                        .note(format!("`{name}` gets its type here"), first)
                });
                return None;
            }
            let value = self.convert(value, &place.ty, None)?;
            return Some(StmtKind::Assign { place, value });
        }

        let (symbol, bin) = match op {
            ast::AssignOp::Add => ("+", BinOp::Add),
            ast::AssignOp::Sub => ("-", BinOp::Sub),
            ast::AssignOp::Mul => ("*", BinOp::Mul),
            ast::AssignOp::Div => ("/", BinOp::Div),
            _ => ("%", BinOp::Mod),
        };
        let builtin = tir::CompoundOp::Builtin(bin);
        let (compound, value) = match &ty {
            Type::Int => match self.value(value, Some(&Type::Int)) {
                Some(checked) if checked.ty == Type::Float => {
                    // The result of the operator would be a `Float`.
                    let target = described.unwrap_or_else(|| "this".to_string());
                    let operand = match (&value.kind, self.snippet(value.span)) {
                        (
                            ast::ExprKind::Binary { .. } | ast::ExprKind::Ternary { .. },
                            Some(text),
                        ) => {
                            format!("({text})")
                        }
                        (_, Some(text)) => text.to_string(),
                        (_, None) => "…".to_string(),
                    };
                    let mut diagnostic = Diagnostic::new(
                        format!(
                            "can't assign `Float` to `{target}`, which is `{}`",
                            self.type_name(&place.ty)
                        ),
                        span,
                    )
                    .label(format!("`{target} {symbol} {operand}` is a `Float`"));
                    if let tir::PlaceKind::Local(local) = place.kind {
                        let first = self.cx.meta[local.0].span;
                        diagnostic =
                            diagnostic.note(format!("`{target}` gets its type here"), first);
                    }
                    self.error(diagnostic);
                    return None;
                }
                Some(checked) => (builtin, self.convert(checked, &Type::Int, None)),
                None => (builtin, None),
            },
            Type::Float => (builtin, self.store(value, &ty)),
            Type::String if bin == BinOp::Add => (builtin, self.store(value, &Type::String)),
            Type::String | Type::List(_) if bin == BinOp::Mul => {
                (builtin, self.store(value, &Type::Int))
            }
            Type::List(_) if bin == BinOp::Add => (builtin, self.store(value, &ty)),
            Type::Record(record) => {
                let type_name = self.records[record.0].name.clone();
                let Some(func) = self.method(*record, symbol) else {
                    self.err_label(
                        format!("`{type_name}` has no `{symbol}` operator"),
                        span,
                        format!("define it in `{type_name}` with `def {symbol}(other: …)`"),
                    );
                    self.probe(value);
                    return None;
                };
                let param = self.funcs[func.0].params.first().and_then(|p| p.ty.clone());
                let value = self.store_opt(value, param.as_ref());
                let ret = self.ret_type(func, span, Mode::Value)?;
                if ret != ty {
                    let ret = self.type_name(&ret);
                    self.err_label(
                        format!("`{symbol}=` can't store what `{symbol}` returns"),
                        span,
                        format!("`{type_name}`'s `{symbol}` returns `{ret}`, not `{type_name}`"),
                    );
                    return None;
                }
                (tir::CompoundOp::Method(func), value)
            }
            other => {
                let name = self.type_name(other);
                self.err(format!("`{name}` has no `{symbol}` operator"), span);
                self.probe(value);
                return None;
            }
        };
        Some(StmtKind::CompoundAssign {
            place,
            op: compound,
            value: value?,
        })
    }

    fn return_stmt(&mut self, value: Option<&ast::Expr>, span: Span) -> Option<StmtKind> {
        let result = match (self.cx.ret.clone(), value) {
            (Ret::Nothing, None) => Some(StmtKind::Return(None)),
            (Ret::Nothing, Some(value)) => {
                let place = if self.cx.kind == BodyKind::System {
                    "a system returns nothing"
                } else {
                    "the top level returns nothing"
                };
                self.err_label("`return` can't have a value here", value.span, place);
                self.probe(value);
                None
            }
            (Ret::Declared(Some(ty)), Some(value)) => self
                .store(value, &ty)
                .map(|value| StmtKind::Return(Some(value))),
            (Ret::Declared(Some(ty)), None) => {
                let ty = self.type_name(&ty);
                self.err_label(
                    "`return` needs a value",
                    span,
                    format!("this function returns `{ty}`"),
                );
                None
            }
            (Ret::Declared(None), value) => {
                if let Some(value) = value {
                    self.probe(value);
                }
                None
            }
            (Ret::Inferred, Some(value)) => match self.value(value, None) {
                Some(value) => {
                    self.cx.ret_sites.push(RetSite {
                        ty: Some(value.ty.clone()),
                        span: value.span,
                    });
                    Some(StmtKind::Return(Some(value)))
                }
                None => {
                    self.cx.ret_poison = true;
                    None
                }
            },
            (Ret::Inferred, None) => {
                self.cx.ret_sites.push(RetSite { ty: None, span });
                Some(StmtKind::Return(None))
            }
        };
        self.cx.unreachable = true;
        result
    }

    /// `Window.open(title, width, height)`.
    fn window_open(&mut self, args: &[ast::Arg], span: Span) -> Option<StmtKind> {
        if self.cx.query_eaches > 0 && !self.cx.gpu {
            self.err_label(
                "`Window.open` is not allowed inside an `each`",
                span,
                "the loop holds the world's rows; call it from top-level code",
            );
        }
        self.no_keywords("Window.open", args);
        let types = [Type::String, Type::Int, Type::Int];
        let values: Vec<Option<tir::Expr>> = args
            .iter()
            .zip(&types)
            .map(|(arg, ty)| self.store(&arg.value, ty))
            .collect();
        for arg in args.iter().skip(types.len()) {
            self.probe(&arg.value);
        }
        if !self.arity("Window.open", 3, args.len(), span) {
            return None;
        }
        let mut values = values.into_iter().collect::<Option<Vec<_>>>()?.into_iter();
        Some(StmtKind::WindowOpen {
            title: values.next()?,
            width: values.next()?,
            height: values.next()?,
        })
    }

    fn emit(&mut self, event: &ast::Expr) -> Option<StmtKind> {
        let kind = self.emit_kind(event)?;
        if let StmtKind::Emit { event: id, .. } = &kind
            && self.is_builtin(self.records[self.events[id.0].record.0].decl.span)
        {
            let name = self.events[id.0].name.clone();
            self.err_label(
                format!("`{name}` is a built-in event and can't be emitted"),
                event.span,
                "the runtime emits it when a tick begins",
            );
            return None;
        }
        Some(kind)
    }

    fn emit_kind(&mut self, event: &ast::Expr) -> Option<StmtKind> {
        // A bare event name, when every field has a default.
        if let ast::ExprKind::Const(name) = &event.kind
            && let Some(Global::Record(record)) = self.global(name)
            && let tir::RecordKind::Event(id) = self.records[record.0].kind
        {
            return match self.construct_defaults(record, event.span) {
                Ok(value) => Some(StmtKind::Emit {
                    event: id,
                    value: value?,
                }),
                Err(missing) => {
                    let fields: Vec<String> = missing.iter().map(|f| format!("{f}: …")).collect();
                    self.err_label(
                        format!("event `{name}` has fields without defaults"),
                        event.span,
                        format!("construct it: `{name}({})`", fields.join(", ")),
                    );
                    None
                }
            };
        }
        let value = self.value(event, None)?;
        if let Type::Record(record) = value.ty
            && let tir::RecordKind::Event(id) = self.records[record.0].kind
        {
            return Some(StmtKind::Emit { event: id, value });
        }
        let found = self.type_name(&value.ty);
        self.err_label(
            format!("`emit` needs an event, found `{found}`"),
            value.span,
            "events are declared with `event`",
        );
        None
    }

    // ----- loops -----

    /// Declares a block parameter, which must not reuse a name in scope.
    fn declare_param(&mut self, spec: ParamSpec<'_>) -> LocalId {
        let name = &spec.name.name;
        let message = format!("block parameter `{name}` reuses a name that is already in scope");
        if let Some(existing) = self.cx.lookup(name) {
            let first = self.cx.meta[existing.0].span;
            self.error(
                Diagnostic::new(message, spec.name.span)
                    .note(format!("`{name}` is defined here"), first),
            );
        } else if self.is_self_field(name) {
            self.err_label(
                message,
                spec.name.span,
                format!("`{name}` is a field of `self`"),
            );
        }
        self.plain_name("block parameter", name, spec.name.span);
        let local = self.declare_local(name, spec.name.span, spec.ty, spec.kind, spec.readonly);
        self.cx.meta[local.0].block_param = true;
        local
    }

    /// Checks the body of a loop with its block parameters in scope.
    /// `construct` names the loop: "the `while`". Returns the parameters'
    /// locals, the body, and whether a `break` leaves the loop.
    fn loop_body(
        &mut self,
        construct: &'static str,
        params: Vec<ParamSpec<'_>>,
        body: &ast::Block,
    ) -> (Vec<LocalId>, tir::Block, bool) {
        let before = self.cx.unreachable;
        self.push_scope(Some(construct));
        let locals: Vec<LocalId> = params
            .into_iter()
            .map(|spec| self.declare_param(spec))
            .collect();
        self.cx.loops.push(false);
        let (block, _) = self.stmts(body.stmts.iter(), Mode::Stmt, None);
        let saw_break = self.cx.loops.pop().unwrap_or(false);
        self.pop_scope();
        self.cx.unreachable = before;
        (locals, block, saw_break)
    }

    /// Reports a block with more parameters than its loop provides.
    fn block_params(&mut self, method: &str, most: usize, block: &ast::DoBlock) -> bool {
        if block.params.len() <= most {
            return true;
        }
        self.err(
            format!(
                "the block of `{method}` takes at most {}, but {}",
                util::count(most, "parameter"),
                util::were_given(block.params.len())
            ),
            block.params[most].span,
        );
        false
    }

    /// A method call with a `do` block: a counting loop or a list
    /// iteration.
    fn loop_stmt(
        &mut self,
        recv: &ast::Expr,
        name: &ast::Ident,
        args: Option<&[ast::Arg]>,
        block: &ast::DoBlock,
    ) -> Option<StmtKind> {
        let method = name.name.as_str();
        let args = args.unwrap_or(&[]);
        match method {
            "times" | "upto" | "downto" => {
                self.no_keywords(method, args);
                let wanted = usize::from(method != "times");
                let arity_ok = self.arity(method, wanted, args.len(), name.span);
                let start = self.store(recv, &Type::Int);
                let bound = if arity_ok && wanted == 1 {
                    self.store(&args[0].value, &Type::Int)
                } else {
                    for arg in args {
                        self.probe(&arg.value);
                    }
                    None
                };
                let params_ok = self.block_params(method, 1, block);
                let specs = block
                    .params
                    .iter()
                    .map(|param| ParamSpec {
                        name: param,
                        ty: Some(Type::Int),
                        kind: tir::LocalKind::Var,
                        readonly: Some(ReadOnly::CountParam(param.name.clone())),
                    })
                    .collect();
                let (locals, body, _) = self.loop_body("the `do` block", specs, &block.body);
                if !arity_ok || !params_ok {
                    return None;
                }
                let (from, to, inclusive, down) = match method {
                    "times" => (
                        mk(ExprKind::Int(0), Type::Int, recv.span),
                        start?,
                        false,
                        false,
                    ),
                    "upto" => (start?, bound?, true, false),
                    _ => (start?, bound?, true, true),
                };
                Some(StmtKind::Count {
                    var: locals.first().copied(),
                    from,
                    to,
                    inclusive,
                    down,
                    body,
                })
            }
            "each" | "each_with_index" => {
                self.no_keywords(method, args);
                let arity_ok = self.arity(method, 0, args.len(), name.span);
                let list = self.value(recv, None);
                let elem = match &list {
                    Some(tir::Expr {
                        ty: Type::List(elem),
                        ..
                    }) => Some(elem.value_type()),
                    Some(other) => {
                        let found = self.type_name(&other.ty);
                        self.err_label(
                            format!("`{method}` iterates a list, found `{found}`"),
                            other.span,
                            "only lists are iterated with a `do` block",
                        );
                        None
                    }
                    None => None,
                };
                let most = if method == "each" { 1 } else { 2 };
                let params_ok = self.block_params(method, most, block);
                let specs = block
                    .params
                    .iter()
                    .enumerate()
                    .map(|(i, param)| ParamSpec {
                        name: param,
                        ty: match i {
                            0 => elem.clone(),
                            1 if most == 2 => Some(Type::Int),
                            _ => None,
                        },
                        kind: tir::LocalKind::Var,
                        readonly: Some(if i == 1 {
                            ReadOnly::IndexParam(param.name.clone())
                        } else {
                            ReadOnly::ListParam(param.name.clone())
                        }),
                    })
                    .collect();
                let (locals, body, _) = self.loop_body("the `do` block", specs, &block.body);
                elem.as_ref()?;
                if !arity_ok || !params_ok {
                    return None;
                }
                Some(StmtKind::ListEach {
                    list: list?,
                    elem: locals.first().copied(),
                    index: locals.get(1).copied(),
                    body,
                })
            }
            _ => {
                self.stray_block(name);
                None
            }
        }
    }

    /// A query loop. Each component parameter is a view; whether the body
    /// writes through it is recorded on its term.
    fn each(
        &mut self,
        query: &ast::Query,
        params: &[ast::Ident],
        body: &ast::Block,
    ) -> Option<StmtKind> {
        let checked = self.query(query);
        let mut count_ok = true;
        if !query.terms.is_empty() && params.len() != query.terms.len() {
            let span = match (params.first(), params.last()) {
                (Some(first), Some(last)) => first.span.to(last.span),
                _ => query.span,
            };
            self.err_label(
                format!(
                    "this query has {}, but its block takes {}",
                    util::count(query.terms.len(), "term"),
                    util::count(params.len(), "parameter")
                ),
                span,
                "the block takes one parameter per term, in order",
            );
            count_ok = false;
        }
        let specs = params
            .iter()
            .enumerate()
            .map(|(i, param)| {
                let (ty, kind) = match checked.terms.get(i) {
                    Some(Some(tir::Term::Entity)) => (Some(Type::Entity), tir::LocalKind::Var),
                    Some(Some(tir::Term::Component { component, .. })) => (
                        self.components[component.0].record.map(Type::Record),
                        tir::LocalKind::View,
                    ),
                    _ => (None, tir::LocalKind::Var),
                };
                ParamSpec {
                    name: param,
                    ty,
                    kind,
                    readonly: None,
                }
            })
            .collect();
        self.cx.query_eaches += 1;
        let (locals, mut body, _) = self.loop_body("the `each`", specs, body);
        self.cx.query_eaches -= 1;
        if !count_ok {
            return None;
        }
        let mut query = checked.finish()?;
        let mut named = vec![false; locals.len()];
        walk::block(&mut body, &mut |node| {
            if let walk::Node::Local(id) = node
                && let Some(at) = locals.iter().position(|local| *local == id)
            {
                named[at] = true;
            }
        });
        for ((term, local), named) in query.terms.iter_mut().zip(&locals).zip(named) {
            if let tir::Term::Component { write, touched, .. } = term {
                *write = self.cx.meta[local.0].written;
                *touched = named;
            }
        }
        Some(StmtKind::Each {
            query,
            params: locals,
            body,
            // Decided by the effect analysis.
            run: tir::Run::Inside,
        })
    }

    /// `on Event do |event| … end`.
    fn on(
        &mut self,
        event: &ast::Ident,
        params: &[ast::Ident],
        body: &ast::Block,
    ) -> Option<StmtKind> {
        let name = &event.name;
        let found = match self.global(name) {
            Some(Global::Record(record)) => match self.records[record.0].kind {
                tir::RecordKind::Event(id) => Some((id, record)),
                _ => {
                    let kind = self.global_kind(Global::Record(record));
                    self.err_label(
                        format!("`{name}` is {kind}, not an event"),
                        event.span,
                        "`on` receives events, which are declared with `event`",
                    );
                    None
                }
            },
            Some(global) => {
                let kind = self.global_kind(global);
                self.err_label(
                    format!("`{name}` is {kind}, not an event"),
                    event.span,
                    "`on` receives events, which are declared with `event`",
                );
                None
            }
            None if BUILTIN_NAMES.contains(&name.as_str()) => {
                self.err_label(
                    format!("`{name}` is not an event"),
                    event.span,
                    "`on` receives events, which are declared with `event`",
                );
                None
            }
            None => {
                let candidates: Vec<String> = self.events.iter().map(|e| e.name.clone()).collect();
                self.unknown(
                    format!("unknown event `{name}`"),
                    event.span,
                    name,
                    &candidates,
                );
                None
            }
        };
        let mut params_ok = true;
        if params.len() > 1 {
            self.err(
                format!(
                    "the block of `on` takes at most 1 parameter, but {}",
                    util::were_given(params.len())
                ),
                params[1].span,
            );
            params_ok = false;
        }
        let specs = params
            .iter()
            .enumerate()
            .map(|(i, param)| ParamSpec {
                name: param,
                ty: match (i, found) {
                    (0, Some((_, record))) => Some(Type::Record(record)),
                    _ => None,
                },
                kind: tir::LocalKind::Var,
                readonly: Some(ReadOnly::OnParam(param.name.clone())),
            })
            .collect();
        let (locals, body, _) = self.loop_body("the `on`", specs, body);
        let (event, _) = found?;
        if !params_ok {
            return None;
        }
        Some(StmtKind::On {
            event,
            param: locals.first().copied(),
            body,
        })
    }
}
