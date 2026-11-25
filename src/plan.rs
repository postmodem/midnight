//! The transfers one tick performs between CPU and GPU memory, worked out
//! from the typed program.
//!
//! Only components that a GPU block names as a query term have a GPU copy.
//! For each of them the plan follows which copy of the column is current
//! (the CPU's, the GPU's or both) through one tick:
//!
//! - A GPU block needs the GPU copy of every component in its outer and
//!   nested queries. Where only the CPU copy is current that is an upload,
//!   after which both are. A component the block writes is then current on
//!   the GPU only.
//! - Ordinary code needs the CPU copy of every component it uses, as a
//!   query term or through `e[C]`. Where only the GPU copy is current that
//!   is a download, after which both are. A component it writes is then
//!   current on the CPU only.
//! - A structural change needs the CPU copy and leaves it the only current
//!   one: `spawn` for the components it lists, `add`, `remove` and
//!   `despawn` for every component.
//!
//! Systems run in source order. An ordinary system is one step, with the
//! functions it calls: what it needs is made current as it starts and what
//! it writes or changes takes effect as it ends. A GPU system is one step
//! per block. Top-level code inside a loop that reaches `World.tick` or
//! `World.run` runs between ticks and is a last step named `top level`;
//! the code before and after such a loop runs once and is no part of a
//! tick. Every statement is taken to run.
//!
//! The plan is the list of one tick in steady state: ticks are followed
//! from a world that is current on the CPU until one ends as it began.
//! That is done twice. The transfers of a tick in which no structural
//! change runs are the lines with no marker. The transfers that a tick has
//! besides those when every structural change runs in every tick are
//! marked with the operation: the download a change forces, and the upload
//! that follows in the next tick. In such a tick a marked download takes
//! the place of an unmarked download of the same component later in the
//! tick.

use crate::tir::{
    Block, ComponentId, CompoundOp, Expr, ExprKind, FuncId, Place, PlaceKind, Program, Query, Stmt,
    StmtKind, Term,
};

/// The name the top-level code has in a plan.
pub const TOP_LEVEL: &str = "top level";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Upload,
    Download,
}

impl Direction {
    fn name(self) -> &'static str {
        match self {
            Direction::Upload => "upload",
            Direction::Download => "download",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Structural {
    Spawn,
    Add,
    Remove,
    Despawn,
}

impl Structural {
    fn name(self) -> &'static str {
        match self {
            Structural::Spawn => "spawn",
            Structural::Add => "add",
            Structural::Remove => "remove",
            Structural::Despawn => "despawn",
        }
    }
}

/// One line of a plan.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Transfer {
    pub direction: Direction,
    /// The component's name.
    pub component: String,
    /// The system that left the other copy as the only current one.
    pub from: String,
    /// The system that needs the copy.
    pub to: String,
    /// Set when the transfer happens only because of a structural change.
    pub cause: Option<Structural>,
}

/// The transfers of one tick in steady state, in the order they happen:
/// those of a tick in which no structural change runs, and, marked with
/// their cause, those that a tick has besides when every one runs.
pub fn plan(program: &Program) -> Vec<Transfer> {
    let steps = steps(program);
    let columns = program.components.len();
    let top_level = program.systems.len();
    let mut transfers = steady_tick(&steps, columns, top_level, false);
    let changed = steady_tick(&steps, columns, top_level, true);
    transfers.extend(changed.into_iter().filter(|t| t.cause.is_some()));
    transfers.sort_by_key(|t| t.at);
    let name = |step: usize| match program.systems.get(step) {
        Some(system) => system.name.clone(),
        None => TOP_LEVEL.to_string(),
    };
    transfers
        .into_iter()
        .map(|t| Transfer {
            direction: t.direction,
            component: program.components[t.component].name.clone(),
            from: name(t.from),
            to: name(t.to),
            cause: t.cause,
        })
        .collect()
}

/// The plan as text: one line per transfer, the columns lined up.
pub fn render(transfers: &[Transfer]) -> String {
    let route = |t: &Transfer| format!("{} -> {}", t.from, t.to);
    let component_width = transfers.iter().map(|t| t.component.len()).max();
    let route_width = transfers.iter().map(|t| route(t).len()).max();
    let (component_width, route_width) = (component_width.unwrap_or(0), route_width.unwrap_or(0));
    let mut out = String::new();
    for t in transfers {
        let mut line = format!(
            "{:<8}  {:<component_width$}  {:<route_width$}",
            t.direction.name(),
            t.component,
            route(t),
        );
        if let Some(cause) = t.cause {
            line.push_str("   on ");
            line.push_str(cause.name());
        }
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out
}

/// More ticks than any program needs to reach its steady state.
const TICK_LIMIT: usize = 16;

/// What one step does with one component.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Uses {
    read: bool,
    write: bool,
    /// The first structural change that touches the component.
    change: Option<Structural>,
}

/// An ordinary system, one block of a GPU system, or the top level.
struct Step {
    /// The system's index, or the number of systems for the top level.
    who: usize,
    gpu: bool,
    /// The components the step uses, by id, in the order it first uses
    /// them.
    uses: Vec<(usize, Uses)>,
}

/// Which copy of a component's column is current.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Current {
    /// The CPU's, since step `by` wrote or changed it. `cause` is set while
    /// only structural changes have made it so.
    Cpu {
        by: usize,
        cause: Option<Structural>,
    },
    /// The GPU's, since step `by` wrote it.
    Gpu {
        by: usize,
    },
    Both,
}

struct RawTransfer {
    /// Where in the tick it happens: the step's position and the
    /// component's position in the step.
    at: (usize, usize),
    direction: Direction,
    component: usize,
    from: usize,
    to: usize,
    cause: Option<Structural>,
}

/// The transfers of a tick that ends as it began, with or without the
/// structural changes. Ticks are followed from a world of `columns`
/// components that the top level has left current on the CPU.
fn steady_tick(
    steps: &[Step],
    columns: usize,
    top_level: usize,
    changes: bool,
) -> Vec<RawTransfer> {
    let start = Current::Cpu {
        by: top_level,
        cause: None,
    };
    let mut columns = vec![start; columns];
    let mut transfers = Vec::new();
    for _ in 0..TICK_LIMIT {
        let before = columns.clone();
        transfers = tick(steps, &mut columns, changes);
        if columns == before {
            break;
        }
    }
    transfers
}

/// Runs every step once, with or without its structural changes, and
/// returns the transfers that needed.
fn tick(steps: &[Step], columns: &mut [Current], changes: bool) -> Vec<RawTransfer> {
    let mut transfers = Vec::new();
    for (step_at, step) in steps.iter().enumerate() {
        for (use_at, &(component, uses)) in step.uses.iter().enumerate() {
            let at = (step_at, use_at);
            let column = &mut columns[component];
            let plain = uses.read || uses.write;
            let change = if changes { uses.change } else { None };
            if !plain && change.is_none() {
                continue;
            }
            if step.gpu {
                if let Current::Cpu { by, cause } = *column {
                    transfers.push(RawTransfer {
                        at,
                        direction: Direction::Upload,
                        component,
                        from: by,
                        to: step.who,
                        cause,
                    });
                    *column = Current::Both;
                }
                if uses.write {
                    *column = Current::Gpu { by: step.who };
                }
                continue;
            }
            if let Current::Gpu { by } = *column {
                transfers.push(RawTransfer {
                    at,
                    direction: Direction::Download,
                    component,
                    from: by,
                    to: step.who,
                    cause: if plain { None } else { change },
                });
                *column = Current::Both;
            }
            if uses.write {
                *column = Current::Cpu {
                    by: step.who,
                    cause: None,
                };
            } else if change.is_some() {
                let written = matches!(*column, Current::Cpu { cause: None, .. });
                *column = Current::Cpu {
                    by: step.who,
                    cause: if written { None } else { change },
                };
            }
        }
    }
    transfers
}

/// The steps of one tick, in order. They use only components that have a
/// GPU copy.
fn steps(program: &Program) -> Vec<Step> {
    let mut on_gpu = vec![false; program.components.len()];
    for system in program.systems.iter().filter(|system| system.gpu) {
        for block in gpu_blocks(&system.body.block) {
            for (component, _) in block {
                on_gpu[component.0] = true;
            }
        }
    }

    let mut steps = Vec::new();
    for (who, system) in program.systems.iter().enumerate() {
        if system.gpu {
            for block in gpu_blocks(&system.body.block) {
                let mut uses: Vec<(usize, Uses)> = Vec::new();
                for (component, write) in block {
                    let at = uses.iter().position(|(used, _)| *used == component.0);
                    let at = at.unwrap_or_else(|| {
                        uses.push((component.0, Uses::default()));
                        uses.len() - 1
                    });
                    uses[at].1.read = true;
                    uses[at].1.write |= write;
                }
                steps.push(Step {
                    who,
                    gpu: true,
                    uses,
                });
            }
        } else {
            let mut walker = Walker::new(program, &on_gpu, true);
            walker.block(&system.body.block);
            steps.push(Step {
                who,
                gpu: false,
                uses: walker.finish(),
            });
        }
    }
    let mut walker = Walker::new(program, &on_gpu, false);
    walker.block(&program.main.block);
    steps.push(Step {
        who: program.systems.len(),
        gpu: false,
        uses: walker.finish(),
    });
    steps
}

/// For each GPU block of a GPU system's body, the components of its outer
/// query with whether the block writes them, then those of the nested
/// queries directly inside it, which it only reads.
fn gpu_blocks(body: &Block) -> impl Iterator<Item = Vec<(ComponentId, bool)>> + '_ {
    body.stmts.iter().filter_map(|stmt| {
        let StmtKind::Each { query, body, .. } = &stmt.kind else {
            return None;
        };
        let mut terms: Vec<(ComponentId, bool)> = components(query).collect();
        for inner in &body.stmts {
            if let StmtKind::Each { query, .. } = &inner.kind {
                terms.extend(components(query).map(|(component, _)| (component, false)));
            }
        }
        Some(terms)
    })
}

/// The component terms of a query, with whether each is written.
fn components(query: &Query) -> impl Iterator<Item = (ComponentId, bool)> + '_ {
    query.terms.iter().filter_map(|term| match *term {
        Term::Entity => None,
        Term::Component { component, write } => Some((component, write)),
    })
}

/// Collects what ordinary code does with the components that have a GPU
/// copy, following calls. Statements are visited in the order they run.
struct Walker<'p> {
    program: &'p Program,
    /// Which components have a GPU copy, by component id.
    on_gpu: &'p [bool],
    /// By component id.
    uses: Vec<Uses>,
    /// The components used so far, by id, in the order of their first use.
    order: Vec<usize>,
    /// Whether the code being visited is recorded. A system's always is.
    /// The top level's is inside a loop that reaches a `World.tick` or
    /// `World.run`: the code before and after such a loop runs once, not
    /// between ticks.
    counting: bool,
    /// Functions whose bodies have been visited while counting.
    counted: Vec<bool>,
    /// Functions whose bodies have been visited while not counting.
    entered: Vec<bool>,
    /// Set while only looking for a `World.tick`.
    probing: bool,
    /// Whether a `World.tick` or `World.run` has been visited.
    ticked: bool,
}

impl<'p> Walker<'p> {
    fn new(program: &'p Program, on_gpu: &'p [bool], counting: bool) -> Walker<'p> {
        Walker {
            program,
            on_gpu,
            uses: vec![Uses::default(); program.components.len()],
            order: Vec::new(),
            counting,
            counted: vec![false; program.funcs.len()],
            entered: vec![false; program.funcs.len()],
            probing: false,
            ticked: false,
        }
    }

    /// What the visited code uses, in the order of first use.
    fn finish(self) -> Vec<(usize, Uses)> {
        let uses = self.uses;
        self.order.into_iter().map(|c| (c, uses[c])).collect()
    }

    fn record(&mut self, component: ComponentId, apply: impl FnOnce(&mut Uses)) {
        if self.counting && self.on_gpu[component.0] {
            let uses = &mut self.uses[component.0];
            if *uses == Uses::default() {
                self.order.push(component.0);
            }
            apply(uses);
        }
    }

    fn read(&mut self, component: ComponentId, write: bool) {
        self.record(component, |uses| {
            uses.read = true;
            uses.write |= write;
        });
    }

    fn change(&mut self, component: ComponentId, change: Structural) {
        self.record(component, |uses| {
            uses.change.get_or_insert(change);
        });
    }

    /// A change that moves or removes a whole row.
    fn change_row(&mut self, change: Structural) {
        for component in 0..self.on_gpu.len() {
            self.change(ComponentId(component), change);
        }
    }

    /// Whether `stmt` reaches a `World.tick` or `World.run`, directly or
    /// through the functions it calls.
    fn ticks(&self, stmt: &Stmt) -> bool {
        let mut probe = Walker::new(self.program, self.on_gpu, false);
        probe.probing = true;
        probe.stmt(stmt);
        probe.ticked
    }

    /// Starts counting at a loop that reaches a tick: all of its body runs
    /// between ticks. Returns whether the code around the loop counts, for
    /// the caller to restore when the loop has been visited.
    fn enter_loop(&mut self, stmt: &Stmt) -> bool {
        let outside = self.counting;
        if !self.counting && !self.probing && self.ticks(stmt) {
            self.counting = true;
        }
        outside
    }

    fn call(&mut self, func: FuncId) {
        let seen = if self.counting {
            &mut self.counted
        } else {
            &mut self.entered
        };
        if !std::mem::replace(&mut seen[func.0], true) {
            let program = self.program;
            self.block(&program.funcs[func.0].body.block);
        }
    }

    fn block(&mut self, block: &Block) {
        for stmt in &block.stmts {
            self.stmt(stmt);
        }
        if let Some(value) = &block.value {
            self.expr(value);
        }
    }

    fn stmt(&mut self, stmt: &Stmt) {
        match &stmt.kind {
            StmtKind::Expr(e) | StmtKind::Let { init: e, .. } | StmtKind::Emit { value: e, .. } => {
                self.expr(e)
            }
            StmtKind::Assign { place, value } => {
                self.expr(value);
                self.place(place);
            }
            StmtKind::CompoundAssign { place, op, value } => {
                self.place(place);
                self.expr(value);
                if let CompoundOp::Method(func) = op {
                    self.call(*func);
                }
            }
            StmtKind::Return(value) => {
                if let Some(value) = value {
                    self.expr(value);
                }
            }
            StmtKind::Break | StmtKind::Next => {}
            StmtKind::While { cond, body } => {
                let outside = self.enter_loop(stmt);
                self.expr(cond);
                self.block(body);
                self.counting = outside;
            }
            StmtKind::Count { from, to, body, .. } => {
                self.expr(from);
                self.expr(to);
                let outside = self.enter_loop(stmt);
                self.block(body);
                self.counting = outside;
            }
            StmtKind::ListEach { list, body, .. } => {
                self.expr(list);
                let outside = self.enter_loop(stmt);
                self.block(body);
                self.counting = outside;
            }
            StmtKind::Each { query, body, .. } => {
                let outside = self.enter_loop(stmt);
                for (component, write) in components(query) {
                    self.read(component, write);
                }
                self.block(body);
                self.counting = outside;
            }
            StmtKind::On { body, .. } => {
                let outside = self.enter_loop(stmt);
                self.block(body);
                self.counting = outside;
            }
            StmtKind::Puts(args) | StmtKind::Print(args) => {
                for arg in args {
                    self.expr(arg);
                }
            }
        }
    }

    fn expr(&mut self, expr: &Expr) {
        match &expr.kind {
            ExprKind::Int(_)
            | ExprKind::Float(_)
            | ExprKind::Bool(_)
            | ExprKind::Str(_)
            | ExprKind::EnumVariant(_)
            | ExprKind::EntityNone
            | ExprKind::WorldStop
            | ExprKind::WorldCount(_) => {}
            ExprKind::WorldTick | ExprKind::WorldRun => self.ticked = true,
            ExprKind::Read(place)
            | ExprKind::ListPop { list: place }
            | ExprKind::ListClear { list: place } => self.place(place),
            ExprKind::Field { base, .. } => self.expr(base),
            ExprKind::Index { base, index } => {
                self.expr(base);
                self.expr(index);
            }
            ExprKind::ListLit(items) | ExprKind::Construct(items) | ExprKind::Concat(items) => {
                for item in items {
                    self.expr(item);
                }
            }
            ExprKind::Unary { expr: inner, .. }
            | ExprKind::IntToFloat(inner)
            | ExprKind::Alive(inner)
            | ExprKind::Has { entity: inner, .. } => self.expr(inner),
            ExprKind::Binary { lhs, rhs, .. }
            | ExprKind::And { lhs, rhs }
            | ExprKind::Or { lhs, rhs } => {
                self.expr(lhs);
                self.expr(rhs);
            }
            ExprKind::Call { func, args } => {
                for arg in args {
                    self.expr(arg);
                }
                self.call(*func);
            }
            ExprKind::Builtin { args, .. } => {
                for arg in args {
                    self.expr(arg);
                }
            }
            ExprKind::If { cond, then, else_ } => {
                self.expr(cond);
                self.block(then);
                self.block(else_);
            }
            ExprKind::Block(block) => self.block(block),
            ExprKind::Spawn(components) => {
                for init in components {
                    if let Some(value) = &init.value {
                        self.expr(value);
                    }
                }
                for init in components {
                    self.change(init.component, Structural::Spawn);
                }
            }
            ExprKind::Add { entity, components } => {
                self.expr(entity);
                for init in components {
                    if let Some(value) = &init.value {
                        self.expr(value);
                    }
                }
                self.change_row(Structural::Add);
            }
            ExprKind::Remove { entity, .. } => {
                self.expr(entity);
                self.change_row(Structural::Remove);
            }
            ExprKind::Despawn(entity) => {
                self.expr(entity);
                self.change_row(Structural::Despawn);
            }
            ExprKind::ListPush { list, value } => {
                self.place(list);
                self.expr(value);
            }
            ExprKind::ListDeleteAt { list, index } => {
                self.place(list);
                self.expr(index);
            }
        }
    }

    fn place(&mut self, place: &Place) {
        match &place.kind {
            PlaceKind::Local(_)
            | PlaceKind::Const(_)
            | PlaceKind::ResourceField(..)
            | PlaceKind::Time(_) => {}
            PlaceKind::Component {
                entity,
                component,
                write,
            } => {
                self.expr(entity);
                self.read(*component, *write);
            }
            PlaceKind::Field { base, .. } => self.place(base),
            PlaceKind::Index { base, index } => {
                self.place(base);
                self.expr(index);
            }
        }
    }
}

#[cfg(test)]
mod tests;
