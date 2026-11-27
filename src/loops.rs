//! `midnight loops`: where each loop of a program runs.
//!
//! One line for each `each` that is not written inside another `each`, in
//! source order: its place, what it stands in, its terms, and where it
//! runs, as the checker decided ([`Run`]). For a loop that stays on one
//! core the line ends with the first thing in its block that keeps it
//! there.

use crate::source::{SourceMap, Span};
use crate::tir::{
    Block, Body, ComponentId, Expr, ExprKind, FuncId, Local, Place, PlaceKind, Program, Query,
    Reason, ReasonKind, Run, Stmt, StmtKind, Target, Term,
};

/// The name the top-level code has in a line.
const TOP_LEVEL: &str = "top level";

struct Line {
    span: Span,
    place: String,
    stands_in: String,
    terms: String,
    runs: String,
}

/// The lines of a program, each ending in a newline.
pub fn render(program: &Program, sources: &SourceMap) -> String {
    let mut printer = Printer {
        program,
        sources,
        lines: Vec::new(),
    };
    for (index, func) in program.funcs.iter().enumerate() {
        let stands_in = format!("def {}", printer.func_name(FuncId(index)));
        printer.body(&func.body, &stands_in);
    }
    for system in &program.systems {
        let kind = match (system.gpu, system.strict) {
            (true, true) => "strict gpu system",
            (true, false) => "gpu system",
            _ => "system",
        };
        printer.body(&system.body, &format!("{kind} {}", system.name));
    }
    printer.body(&program.main, TOP_LEVEL);

    let mut lines = printer.lines;
    lines.sort_by_key(|line| (line.span.file.0, line.span.start));
    let width = |column: fn(&Line) -> &String| {
        lines
            .iter()
            .map(|line| column(line).chars().count())
            .max()
            .unwrap_or(0)
    };
    let widths = (
        width(|line| &line.place),
        width(|line| &line.stands_in),
        width(|line| &line.terms),
    );
    lines
        .iter()
        .map(|line| {
            format!(
                "{:place$}  {:stands_in$}  {:terms$}  {}\n",
                line.place,
                line.stands_in,
                line.terms,
                line.runs,
                place = widths.0,
                stands_in = widths.1,
                terms = widths.2,
            )
        })
        .collect()
}

struct Printer<'p> {
    program: &'p Program,
    sources: &'p SourceMap,
    lines: Vec<Line>,
}

impl<'p> Printer<'p> {
    /// `nearest`, or `Vec2.length` for a method.
    fn func_name(&self, id: FuncId) -> String {
        let func = &self.program.funcs[id.0];
        match func.owner {
            Some(owner) => format!("{}.{}", self.program.records[owner.0].name, func.name),
            None => func.name.clone(),
        }
    }

    fn component(&self, id: ComponentId) -> &'p str {
        &self.program.components[id.0].name
    }

    fn line_of(&self, span: Span) -> u32 {
        self.sources.line(span)
    }

    /// Adds a line for every `each` of a body that has a verdict.
    fn body(&mut self, body: &'p Body, stands_in: &str) {
        let mut found = Vec::new();
        block_eaches(&body.block, &mut found);
        for stmt in found {
            let StmtKind::Each { query, run, .. } = &stmt.kind else {
                unreachable!("an `each` statement");
            };
            let runs = match run {
                Run::Inside => continue,
                Run::Gpu => "gpu".to_string(),
                Run::Cores(_) => "cores".to_string(),
                Run::OneCore(reason) => format!("one core: {}", self.reason(reason, &body.locals)),
            };
            self.lines.push(Line {
                span: stmt.span,
                place: format!(
                    "{}:{}",
                    self.sources.file(stmt.span.file).name,
                    self.line_of(stmt.span)
                ),
                stands_in: stands_in.to_string(),
                terms: self.terms(query),
                runs,
            });
        }
    }

    /// `each Entity, Life`
    fn terms(&self, query: &Query) -> String {
        let names: Vec<&str> = query
            .terms
            .iter()
            .map(|term| match term {
                Term::Entity => "Entity",
                Term::Component { component, .. } => self.component(*component),
            })
            .collect();
        format!("each {}", names.join(", "))
    }

    /// `Stats.expired`, or the name of a variable.
    fn target(&self, target: Target, locals: &[Local]) -> String {
        match target {
            Target::Field(resource, field) => {
                let resource = &self.program.resources[resource.0];
                format!("{}.{}", resource.name, resource.fields[field].name)
            }
            Target::Local(local) => locals[local.0].name.clone(),
        }
    }

    /// An `e[C]` as the source writes it, when that is short and on one
    /// line.
    fn row(&self, span: Span, component: ComponentId) -> String {
        let text = &self.sources.file(span.file).text;
        match text.get(span.start as usize..span.end as usize) {
            Some(text) if text.chars().count() <= 40 && !text.contains('\n') => text.to_string(),
            _ => format!("…[{}]", self.component(component)),
        }
    }

    fn reason(&self, reason: &Reason, locals: &[Local]) -> String {
        let line = self.line_of(reason.span);
        let at = match reason.via {
            None => format!("at line {line}"),
            Some(via) => format!(
                "in `{}` (line {line}), called at line {}",
                self.func_name(via.func),
                self.line_of(via.call)
            ),
        };
        let name = |target: Target| self.target(target, locals);
        let how = |component: ComponentId, nested: bool| {
            if nested {
                format!("in the nested `each` {at}")
            } else {
                format!("by `{}` {at}", self.row(reason.span, component))
            }
        };
        match reason.kind {
            ReasonKind::UsesString => format!("uses a `String` {at}"),
            ReasonKind::UsesList => format!("uses a list {at}"),
            ReasonKind::AssignsRow(component) => {
                format!("assigns `{}` {at}", self.row(reason.span, component))
            }
            ReasonKind::AssignsNested(component) => format!(
                "assigns `{}` through the nested `each` {at}",
                self.component(component)
            ),
            ReasonKind::Assigns {
                target,
                spelled_out,
            } => {
                let name = name(target);
                if spelled_out {
                    format!("assigns `{name}` {at}; write `{name} += …` for a sum")
                } else {
                    format!("assigns `{name}` {at}")
                }
            }
            ReasonKind::AssignsDt => format!("assigns `Time.dt` {at}"),
            ReasonKind::ReadsAssigned { component, nested } => format!(
                "reads `{}`, which this loop assigns, {}",
                self.component(component),
                how(component, nested)
            ),
            ReasonKind::FloatSum(target) => {
                format!("`{} += …` {at} is a `Float` sum", name(target))
            }
            ReasonKind::CompactSum(target) => {
                format!("`{} += …` {at} is a sum into a compact field", name(target))
            }
            ReasonKind::SummedAndRead(target) => {
                format!("`{}` is summed and also read {at}", name(target))
            }
            ReasonKind::SummedAndAssigned(target) => {
                format!("`{}` is summed and also assigned {at}", name(target))
            }
            ReasonKind::SumInFunction(target) => {
                let place = at.strip_prefix("in ").unwrap_or(&at);
                format!("`{} += …` is in a called function: {place}", name(target))
            }
            ReasonKind::Does(deed) => format!("`{}` {at}", deed.word()),
            ReasonKind::ReadsGpu {
                component,
                system,
                nested,
            } => format!(
                "reads `{}`, which the GPU system `{}` uses, {}",
                self.component(component),
                self.program.systems[system].name,
                how(component, nested)
            ),
        }
    }
}

/// Every `each` statement of a block, at any depth, in source order.
fn block_eaches<'p>(block: &'p Block, out: &mut Vec<&'p Stmt>) {
    for stmt in &block.stmts {
        stmt_eaches(stmt, out);
    }
    if let Some(value) = &block.value {
        expr_eaches(value, out);
    }
}

fn stmt_eaches<'p>(stmt: &'p Stmt, out: &mut Vec<&'p Stmt>) {
    match &stmt.kind {
        StmtKind::Expr(expr)
        | StmtKind::Let { init: expr, .. }
        | StmtKind::Emit { value: expr, .. } => {
            expr_eaches(expr, out);
        }
        StmtKind::Assign { place, value } | StmtKind::CompoundAssign { place, value, .. } => {
            place_eaches(place, out);
            expr_eaches(value, out);
        }
        StmtKind::Return(value) => {
            if let Some(value) = value {
                expr_eaches(value, out);
            }
        }
        StmtKind::Break | StmtKind::Next => {}
        StmtKind::While { cond, body } => {
            expr_eaches(cond, out);
            block_eaches(body, out);
        }
        StmtKind::Count { from, to, body, .. } => {
            expr_eaches(from, out);
            expr_eaches(to, out);
            block_eaches(body, out);
        }
        StmtKind::ListEach { list, body, .. } => {
            expr_eaches(list, out);
            block_eaches(body, out);
        }
        StmtKind::Each { body, .. } => {
            out.push(stmt);
            block_eaches(body, out);
        }
        StmtKind::On { body, .. } => block_eaches(body, out),
        StmtKind::Puts(args) | StmtKind::Print(args) => {
            for arg in args {
                expr_eaches(arg, out);
            }
        }
        StmtKind::WindowOpen {
            title,
            width,
            height,
        } => {
            expr_eaches(title, out);
            expr_eaches(width, out);
            expr_eaches(height, out);
        }
        StmtKind::Draw { nums, color, .. } => {
            for num in nums {
                expr_eaches(num, out);
            }
            expr_eaches(color, out);
        }
    }
}

fn expr_eaches<'p>(expr: &'p Expr, out: &mut Vec<&'p Stmt>) {
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
        ExprKind::Read(place)
        | ExprKind::ListPop { list: place }
        | ExprKind::ListClear { list: place } => {
            place_eaches(place, out);
        }
        ExprKind::Field { base, .. } => expr_eaches(base, out),
        ExprKind::Index { base, index } => {
            expr_eaches(base, out);
            expr_eaches(index, out);
        }
        ExprKind::ListLit(items) | ExprKind::Construct(items) | ExprKind::Concat(items) => {
            for item in items {
                expr_eaches(item, out);
            }
        }
        ExprKind::Unary { expr: inner, .. }
        | ExprKind::IntToFloat(inner)
        | ExprKind::Despawn(inner)
        | ExprKind::Alive(inner)
        | ExprKind::Input { arg: inner, .. }
        | ExprKind::Remove { entity: inner, .. }
        | ExprKind::Has { entity: inner, .. } => expr_eaches(inner, out),
        ExprKind::Binary { lhs, rhs, .. }
        | ExprKind::And { lhs, rhs }
        | ExprKind::Or { lhs, rhs } => {
            expr_eaches(lhs, out);
            expr_eaches(rhs, out);
        }
        ExprKind::Call { args, .. } | ExprKind::Builtin { args, .. } => {
            for arg in args {
                expr_eaches(arg, out);
            }
        }
        ExprKind::If { cond, then, else_ } => {
            expr_eaches(cond, out);
            block_eaches(then, out);
            block_eaches(else_, out);
        }
        ExprKind::Block(block) => block_eaches(block, out),
        ExprKind::Spawn(components) => {
            for init in components {
                if let Some(value) = &init.value {
                    expr_eaches(value, out);
                }
            }
        }
        ExprKind::Add { entity, components } => {
            expr_eaches(entity, out);
            for init in components {
                if let Some(value) = &init.value {
                    expr_eaches(value, out);
                }
            }
        }
        ExprKind::ListPush { list, value } => {
            place_eaches(list, out);
            expr_eaches(value, out);
        }
        ExprKind::ListDeleteAt { list, index } => {
            place_eaches(list, out);
            expr_eaches(index, out);
        }
    }
}

fn place_eaches<'p>(place: &'p Place, out: &mut Vec<&'p Stmt>) {
    match &place.kind {
        PlaceKind::Local(_)
        | PlaceKind::Const(_)
        | PlaceKind::ResourceField(..)
        | PlaceKind::Time(_) => {}
        PlaceKind::Component { entity, .. } => expr_eaches(entity, out),
        PlaceKind::Field { base, .. } => place_eaches(base, out),
        PlaceKind::Index { base, index } => {
            place_eaches(base, out);
            expr_eaches(index, out);
        }
    }
}
