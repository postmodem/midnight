//! The state for generating one C function, and the bookkeeping of what
//! each scope must release.
//!
//! Every value of a type that owns a string or list buffer is in one of
//! three states: owned (a named C variable that holds a reference of its
//! own and is listed in a scope until it is moved or dropped), borrowed
//! (a copy of the bits that holds no reference and must be used before its
//! source can change), or free (a static literal or the empty value).

use std::collections::HashMap;

use crate::source::Span;
use crate::tir::{Body, Expr, Local, LocalId, LocalKind, Type};

use super::Cx;
use super::analysis::{self, HoistKey, LocalUse};
use super::names;

pub(super) const ATOM: u8 = 0;
pub(super) const UNARY: u8 = 1;
pub(super) const MUL: u8 = 2;
pub(super) const ADD: u8 = 3;
pub(super) const REL: u8 = 4;
pub(super) const EQ: u8 = 5;
pub(super) const AND: u8 = 6;
pub(super) const OR: u8 = 7;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Own {
    /// A variable listed in a scope: it must be moved out or dropped.
    Owned,
    /// Nothing to retain or drop.
    Free,
    /// Bits copied from somewhere that keeps the reference. `stable` when
    /// that somewhere is a local, a constant or a temporary of this
    /// function, which other functions can't change.
    Borrowed { stable: bool },
}

/// What can change the value a C expression gives.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub(super) enum Dep {
    /// Nothing: a literal, a temporary, a parameter.
    Fixed,
    /// Assignments to locals of this function.
    Local,
    /// Anything that changes the world or a global.
    World,
}

/// A value as a C expression that has no side effects, unless `pending`.
#[derive(Clone, Debug)]
pub(super) struct Val {
    pub c: String,
    /// How loosely the expression binds; `ATOM` needs no parentheses.
    pub prec: u8,
    /// The value, for an `Int` literal.
    pub lit: Option<i64>,
    pub own: Own,
    pub dep: Dep,
    /// A name, a field of a name, or a literal: cheap and safe to repeat.
    pub simple: bool,
    /// The expression names a variable that is assigned once and can be
    /// retained where it is.
    pub slot: bool,
    /// The expression has effects and has not been emitted: it must be
    /// written into a statement before anything else is generated.
    pub pending: bool,
}

impl Val {
    /// A literal or the name of a temporary.
    pub fn atom(c: impl Into<String>) -> Val {
        Val {
            c: c.into(),
            prec: ATOM,
            lit: None,
            own: Own::Free,
            dep: Dep::Fixed,
            simple: true,
            slot: true,
            pending: false,
        }
    }

    pub fn int(value: i64) -> Val {
        Val {
            prec: if value < 0 { UNARY } else { ATOM },
            lit: Some(value),
            ..Val::atom(names::int_lit(value))
        }
    }

    pub fn float(value: f64) -> Val {
        let c = names::float_lit(value);
        Val {
            prec: if c.starts_with('-') || c.starts_with('(') {
                UNARY
            } else {
                ATOM
            },
            ..Val::atom(c)
        }
    }

    /// A side-effect-free expression that is evaluated where it is used.
    pub fn lazy(c: String, prec: u8, dep: Dep) -> Val {
        Val {
            c,
            prec,
            dep,
            simple: false,
            slot: false,
            ..Val::atom("")
        }
    }

    /// A call that must be emitted at once.
    pub fn pending(c: String, own: Own) -> Val {
        Val {
            c,
            own,
            dep: Dep::World,
            simple: false,
            slot: false,
            pending: true,
            ..Val::atom("")
        }
    }

    pub fn void() -> Val {
        Val::atom("")
    }

    /// The expression, in parentheses if it binds more loosely than `max`.
    pub fn p(&self, max: u8) -> String {
        if self.prec > max {
            format!("({})", self.c)
        } else {
            self.c.clone()
        }
    }

    /// The expression with type `int64_t` even when it is a literal.
    pub fn typed_int(&self) -> String {
        match self.lit {
            Some(_) => format!("(int64_t){}", self.c),
            None => self.c.clone(),
        }
    }
}

/// What the consumer of a value will do with it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Mode {
    /// Keep it, or use it after other code has run: it must be owned or
    /// free.
    Own,
    /// Use it while only other functions run: a stable borrow will do.
    Stable,
    /// Use it at once: any borrow will do.
    Peek,
}

pub(super) enum ScopeKind {
    Plain,
    /// The body of a `while`, counting loop, list loop or `on`.
    Loop,
    /// The row loop of an `each`. `label` is where `break` jumps to.
    Each {
        label: String,
        used: bool,
    },
}

pub(super) struct Owned {
    /// The lvalue that holds the reference.
    pub c: String,
    pub ty: Type,
    /// A temporary of the statement being generated, as opposed to a
    /// local variable.
    pub temp: bool,
}

/// A C block and the owned values declared in it.
pub(super) struct Scope {
    pub kind: ScopeKind,
    pub owned: Vec<Owned>,
}

pub(super) struct FnCx<'a, 'c> {
    pub cx: &'c mut Cx<'a>,
    pub locals: &'a [Local],
    /// The C name of each local.
    pub names: Vec<String>,
    pub usage: LocalUse,
    /// For each local, whether it never changes while it is in scope.
    pub fixed: Vec<bool>,
    pub out: String,
    pub indent: usize,
    counter: usize,
    pub scopes: Vec<Scope>,
    /// The column element each `View` local stands for.
    pub views: HashMap<usize, String>,
    /// Values loaded before the loops of the enclosing `each`es.
    pub hoists: Vec<HashMap<HoistKey, String>>,
    pub each_depth: usize,
    pub ret: Type,
}

impl<'a, 'c> FnCx<'a, 'c> {
    pub fn new(cx: &'c mut Cx<'a>, body: &'a Body, ret: Type) -> FnCx<'a, 'c> {
        let names = body
            .locals
            .iter()
            .enumerate()
            .map(|(id, local)| names::local(&local.name, id))
            .collect();
        let fixed = body
            .locals
            .iter()
            .map(|local| local.kind == LocalKind::Param)
            .collect();
        FnCx {
            cx,
            locals: &body.locals,
            names,
            usage: analysis::local_use(body),
            fixed,
            out: String::new(),
            indent: 1,
            counter: 0,
            scopes: vec![Scope {
                kind: ScopeKind::Plain,
                owned: Vec::new(),
            }],
            views: HashMap::new(),
            hoists: Vec::new(),
            each_depth: 0,
            ret,
        }
    }

    /// For code that has no locals: the initializers of globals.
    pub fn new_bare(cx: &'c mut Cx<'a>) -> FnCx<'a, 'c> {
        FnCx {
            cx,
            locals: &[],
            names: Vec::new(),
            usage: LocalUse {
                read: Vec::new(),
                touched: Vec::new(),
            },
            fixed: Vec::new(),
            out: String::new(),
            indent: 1,
            counter: 0,
            scopes: vec![Scope {
                kind: ScopeKind::Plain,
                owned: Vec::new(),
            }],
            views: HashMap::new(),
            hoists: Vec::new(),
            each_depth: 0,
            ret: Type::Void,
        }
    }

    /// The parameter list of the C function for these parameters.
    pub fn param_list(&self, params: &[LocalId]) -> String {
        if params.is_empty() {
            return "void".to_string();
        }
        let list: Vec<String> = params
            .iter()
            .map(|id| {
                format!(
                    "{} {}{}",
                    self.cx.cty(&self.locals[id.0].ty),
                    self.names[id.0],
                    self.unused_mark(*id)
                )
            })
            .collect();
        list.join(", ")
    }

    /// The attribute that keeps the C compiler quiet about a local the
    /// program never reads.
    pub fn unused_mark(&self, id: LocalId) -> &'static str {
        if self.usage.read[id.0] {
            ""
        } else {
            " __attribute__((unused))"
        }
    }

    /// Generates the body of the function: its statements, then the
    /// return of its value or the release of its locals.
    pub fn function(mut self, body: &'a Body) -> String {
        self.stmts(&body.block.stmts);
        match &body.block.value {
            Some(value) => {
                self.directive(value.span);
                self.ret_value(Some(value));
            }
            None => {
                if !analysis::block_leaves(&body.block) {
                    self.unwind(0, false);
                    if self.ret != Type::Void {
                        let zero = self.cx.zero_expr(&self.ret);
                        self.line(&format!("return {zero};"));
                    }
                }
            }
        }
        self.out
    }

    /// Generates the statements that give one constant or resource field
    /// its first value.
    pub fn init_global(&mut self, lv: &str, storage: &Type, init: &'a Expr) {
        self.directive(init.span);
        let mark = self.mark();
        let value = self.rvalue(init);
        if self.cx.managed(storage) {
            self.store_new(lv, "", value, storage);
        } else {
            let value = self.materialize(value, &init.ty);
            let c = self.store_c(&value, storage, init.span);
            self.line(&format!("{lv} = {c};"));
        }
        self.sweep(mark);
    }

    pub fn line(&mut self, text: &str) {
        for _ in 0..self.indent {
            self.out.push_str("    ");
        }
        self.out.push_str(text);
        self.out.push('\n');
    }

    /// Writes a `#line` directive for the line a span starts on.
    pub fn directive(&mut self, span: Span) {
        let directive = self.cx.line_directive(span);
        self.out.push_str(&directive);
        self.out.push('\n');
    }

    /// A new name: the prefix and a number no other name in this function
    /// has.
    pub fn fresh(&mut self, prefix: &str) -> String {
        self.counter += 1;
        format!("{prefix}{}", self.counter - 1)
    }

    pub fn tmp(&mut self) -> String {
        self.fresh("t")
    }

    /// Runs `f` with an empty output buffer and returns what it wrote.
    pub fn capture<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> (String, R) {
        let saved = std::mem::take(&mut self.out);
        let result = f(self);
        let text = std::mem::replace(&mut self.out, saved);
        (text, result)
    }

    pub fn push_scope(&mut self, kind: ScopeKind) {
        self.scopes.push(Scope {
            kind,
            owned: Vec::new(),
        });
    }

    /// Lists an owned value in the innermost scope.
    pub fn register(&mut self, c: &str, ty: &Type, temp: bool) {
        self.scopes
            .last_mut()
            .expect("a scope is open")
            .owned
            .push(Owned {
                c: c.to_string(),
                ty: ty.clone(),
                temp,
            });
    }

    /// Takes an owned value off the books: it has been moved.
    pub fn disown(&mut self, c: &str) {
        for scope in self.scopes.iter_mut().rev() {
            if let Some(index) = scope.owned.iter().rposition(|owned| owned.c == c) {
                scope.owned.remove(index);
                return;
            }
        }
        unreachable!("moved a value that is not owned: {c}");
    }

    /// The position in the innermost scope that later temporaries follow.
    pub fn mark(&self) -> usize {
        self.scopes.last().expect("a scope is open").owned.len()
    }

    /// Drops the temporaries listed in the innermost scope since `mark`.
    pub fn sweep(&mut self, mark: usize) {
        for (c, ty) in self.take_temps(mark).into_iter().rev() {
            let line = self.cx.drop_stmt(&c, &ty);
            self.line(&line);
        }
    }

    /// Unlists the temporaries since `mark` without dropping them: the
    /// code that would run next is unreachable.
    pub fn forget(&mut self, mark: usize) {
        self.take_temps(mark);
    }

    fn take_temps(&mut self, mark: usize) -> Vec<(String, Type)> {
        let scope = self.scopes.last_mut().expect("a scope is open");
        let mut temps = Vec::new();
        let mut index = mark;
        while index < scope.owned.len() {
            if scope.owned[index].temp {
                let owned = scope.owned.remove(index);
                temps.push((owned.c, owned.ty));
            } else {
                index += 1;
            }
        }
        temps
    }

    /// Closes the innermost scope, dropping what it owns unless its end
    /// can't be reached.
    pub fn pop_scope(&mut self, dead: bool) {
        if !dead {
            self.unwind(self.scopes.len() - 1, false);
        }
        self.scopes.pop();
    }

    /// Generates the drops for leaving every scope from the innermost out
    /// to the one at index `down_to`. With `leave_each`, also ends the
    /// deferral of each `each` among them. The scopes stay open.
    pub fn unwind(&mut self, down_to: usize, leave_each: bool) {
        let mut steps: Vec<Option<(String, Type)>> = Vec::new();
        for scope in self.scopes[down_to..].iter().rev() {
            for owned in scope.owned.iter().rev() {
                steps.push(Some((owned.c.clone(), owned.ty.clone())));
            }
            if leave_each && matches!(scope.kind, ScopeKind::Each { .. }) {
                steps.push(None);
            }
        }
        for step in steps {
            let line = match step {
                Some((c, ty)) => self.cx.drop_stmt(&c, &ty),
                None => "gr_defer_leave();".to_string(),
            };
            self.line(&line);
        }
    }

    /// Whether leaving the function from here has anything to release.
    pub fn has_cleanup(&self) -> bool {
        self.scopes
            .iter()
            .any(|scope| !scope.owned.is_empty() || matches!(scope.kind, ScopeKind::Each { .. }))
    }

    /// Declares a temporary holding `c` and returns its name.
    pub fn declare(&mut self, ty: &Type, c: &str) -> String {
        let name = self.tmp();
        let cty = self.cx.cty(ty);
        self.line(&format!("{cty} {name} = {c};"));
        name
    }
}
