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
//!   the GPU only. A component it overwrites needs no upload.
//! - Ordinary code needs the CPU copy of every component it uses, as a
//!   query term or through `e[C]`. Where only the GPU copy is current that
//!   is a download, after which both are. A component it writes is then
//!   current on the CPU only. A component its first loop over it
//!   overwrites needs no download.
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
    /// Set when only the rows concerned move, not the whole array.
    pub rows: bool,
    /// Set when the transfer happens only because of a structural change.
    pub cause: Option<Structural>,
}

/// The transfers of one tick in steady state, in the order they happen:
/// those of a tick in which no structural change runs, and, marked with
/// their cause, those that a tick has besides when every one runs.
pub fn plan(program: &Program) -> Vec<Transfer> {
    let (steps, shapes) = steps(program);
    let columns = program.components.len();
    let top_level = program.systems.len();
    let kinds = Kinds::new(&steps, &shapes);
    let mut transfers = steady_tick(&steps, &kinds, columns, top_level, false);
    let changed = steady_tick(&steps, &kinds, columns, top_level, true);
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
            rows: t.rows,
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
        let mark = match (t.rows, t.cause) {
            (false, None) => String::new(),
            (false, Some(cause)) => format!("on {}", cause.name()),
            (true, None) => "rows".to_string(),
            (true, Some(cause)) => format!("rows on {}", cause.name()),
        };
        if !mark.is_empty() {
            line.push_str("   ");
            line.push_str(&mark);
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
    /// As a query term: read, and assigned.
    read: bool,
    write: bool,
    /// Through `e[C]`: the row read, and the row written.
    row_read: bool,
    row_write: bool,
    /// Whether the first of those uses overwrites: a query term whose body
    /// assigns the whole component, in every row it visits, before reading
    /// it.
    overwrite: bool,
    /// The first structural change that touches the component.
    change: Option<Structural>,
}

/// The tables a use reaches: those that have every component in
/// `require` and none in `exclude`. With `exact`, only the table whose
/// components are `require`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Filter {
    require: Vec<usize>,
    exclude: Vec<usize>,
    exact: bool,
}

impl Filter {
    /// Every table that holds `component`.
    fn any(component: usize) -> Filter {
        Filter {
            require: vec![component],
            ..Filter::default()
        }
    }

    /// The tables of a query: its component terms, touched or not, and its
    /// filters.
    fn query(query: &Query) -> Filter {
        let mut require: Vec<usize> = query
            .terms
            .iter()
            .filter_map(|term| match *term {
                Term::Entity => None,
                Term::Component { component, .. } => Some(component.0),
            })
            .collect();
        require.extend(query.with.iter().map(|c| c.0));
        Filter {
            require,
            exclude: query.without.iter().map(|c| c.0).collect(),
            exact: false,
        }
    }

    /// Whether it reaches a kind of table, given as the set of `kinds`
    /// components it holds.
    fn reaches(&self, kind: u32, kinds: &Kinds) -> bool {
        let require = kinds.mask(&self.require);
        let exclude = kinds.mask(&self.exclude);
        if self.exact {
            kind == require
        } else {
            kind & require == require && kind & exclude == 0
        }
    }
}

/// The components that tell tables apart in a plan: those that some use
/// requires or excludes, at most `KINDS_LIMIT` of them. Leaving one out
/// only widens the uses that name it. A kind of table is the set of them
/// it holds, as a mask.
struct Kinds {
    ids: Vec<usize>,
    /// The kinds of table the program can make: those it spawns, and those
    /// its `add`s and `remove`s make of them.
    possible: Vec<u32>,
}

/// The sets of components the program spawns, adds and removes, by id.
#[derive(Default)]
struct Shapes {
    spawned: Vec<Vec<usize>>,
    added: Vec<Vec<usize>>,
    removed: Vec<Vec<usize>>,
}

const KINDS_LIMIT: usize = 12;

impl Kinds {
    fn new(steps: &[Step], shapes: &Shapes) -> Kinds {
        let mut ids = Vec::new();
        for step in steps {
            for used in &step.uses {
                for &id in used.filter.require.iter().chain(&used.filter.exclude) {
                    if ids.len() < KINDS_LIMIT && !ids.contains(&id) {
                        ids.push(id);
                    }
                }
            }
        }
        let mut kinds = Kinds {
            ids,
            possible: Vec::new(),
        };
        let mut possible: Vec<u32> = shapes.spawned.iter().map(|s| kinds.mask(s)).collect();
        possible.sort_unstable();
        possible.dedup();
        let added: Vec<u32> = shapes.added.iter().map(|s| kinds.mask(s)).collect();
        let removed: Vec<u32> = shapes.removed.iter().map(|s| kinds.mask(s)).collect();
        let mut at = 0;
        while at < possible.len() {
            let kind = possible[at];
            let made = added.iter().map(|a| kind | a);
            for next in made
                .chain(removed.iter().map(|r| kind & !r))
                .collect::<Vec<_>>()
            {
                if !possible.contains(&next) {
                    possible.push(next);
                }
            }
            at += 1;
        }
        kinds.possible = possible;
        kinds
    }

    fn mask(&self, components: &[usize]) -> u32 {
        components
            .iter()
            .filter_map(|c| self.ids.iter().position(|id| id == c))
            .fold(0, |mask, bit| mask | 1 << bit)
    }

    /// The kinds of table the program can make that hold `component`.
    fn holding(&self, component: usize) -> impl Iterator<Item = u32> + '_ {
        let own = self.mask(&[component]);
        self.possible
            .iter()
            .copied()
            .filter(move |kind| kind & own == own)
    }
}

/// What a step does with one component in the tables of `filter`.
struct Used {
    component: usize,
    filter: Filter,
    uses: Uses,
}

/// An ordinary system, one block of a GPU system, or the top level.
struct Step {
    /// The system's index, or the number of systems for the top level.
    who: usize,
    gpu: bool,
    /// In the order the step first uses each component and filter.
    uses: Vec<Used>,
}

impl Step {
    /// The components the step uses, in the order of first use.
    fn components(&self) -> Vec<usize> {
        let mut order = Vec::new();
        for used in &self.uses {
            if !order.contains(&used.component) {
                order.push(used.component);
            }
        }
        order
    }

    /// What the step does with `component` in tables of one kind.
    fn uses_in(&self, component: usize, kind: u32, kinds: &Kinds) -> Uses {
        let mut uses = Uses::default();
        for used in &self.uses {
            if used.component == component && used.filter.reaches(kind, kinds) {
                if !uses.any() && used.uses.any() {
                    uses.overwrite = used.uses.overwrite;
                }
                uses.read |= used.uses.read;
                uses.write |= used.uses.write;
                uses.row_read |= used.uses.row_read;
                uses.row_write |= used.uses.row_write;
                if uses.change.is_none() {
                    uses.change = used.uses.change;
                }
            }
        }
        uses
    }
}

impl Uses {
    /// Whether the component's values are used at all.
    fn any(&self) -> bool {
        self.read || self.write || self.row_read || self.row_write
    }

    /// Records a use as a query term, which overwrites the component when
    /// `overwrite` is set, unless it comes after another use.
    fn first(&mut self, overwrite: bool) {
        if !self.any() {
            self.overwrite = overwrite;
        }
        self.read = true;
    }
}

/// Adds what a use does to a step's list.
fn add_use(list: &mut Vec<Used>, component: usize, filter: Filter, apply: impl FnOnce(&mut Uses)) {
    let at = list
        .iter()
        .position(|used| used.component == component && used.filter == filter);
    let at = at.unwrap_or_else(|| {
        list.push(Used {
            component,
            filter,
            uses: Uses::default(),
        });
        list.len() - 1
    });
    apply(&mut list[at].uses);
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

/// Rows that CPU memory has newer values of than the GPU copy, left by step
/// `by`. `cause` is set while only structural changes have left them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Rows {
    by: usize,
    cause: Option<Structural>,
}

/// What the plan knows of a column: which copy is current and, while the
/// GPU copy is, any rows waiting to be sent to it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct State {
    current: Current,
    rows: Option<Rows>,
}

#[derive(PartialEq, Eq)]
struct RawTransfer {
    /// Where in the tick it happens: the step's position and the
    /// component's position in the step.
    at: (usize, usize),
    direction: Direction,
    component: usize,
    from: usize,
    to: usize,
    /// Set when only the rows concerned move, not the whole array.
    rows: bool,
    cause: Option<Structural>,
}

/// The state of every column: by component, then by kind of table.
type Columns = Vec<Vec<(u32, State)>>;

/// The transfers of a tick that ends as it began, with or without the
/// structural changes. Ticks are followed from a world of `columns`
/// components that the top level has left current on the CPU.
fn steady_tick(
    steps: &[Step],
    kinds: &Kinds,
    columns: usize,
    top_level: usize,
    changes: bool,
) -> Vec<RawTransfer> {
    let start = State {
        current: Current::Cpu {
            by: top_level,
            cause: None,
        },
        rows: None,
    };
    let mut columns: Columns = (0..columns)
        .map(|component| kinds.holding(component).map(|kind| (kind, start)).collect())
        .collect();
    let mut transfers = Vec::new();
    for _ in 0..TICK_LIMIT {
        let before = columns.clone();
        transfers = tick(steps, kinds, &mut columns, changes);
        if columns == before {
            break;
        }
    }
    transfers
}

/// Runs every step once, with or without its structural changes, and
/// returns the transfers that needed. Transfers that tables of several
/// kinds need alike are listed once.
fn tick(steps: &[Step], kinds: &Kinds, columns: &mut Columns, changes: bool) -> Vec<RawTransfer> {
    let mut transfers = Vec::new();
    for (step_at, step) in steps.iter().enumerate() {
        for (use_at, component) in step.components().into_iter().enumerate() {
            let at = (step_at, use_at);
            for (kind, column) in columns[component].iter_mut() {
                let uses = step.uses_in(component, *kind, kinds);
                if let Some(transfer) = visit(step, at, component, uses, column, changes)
                    && !transfers.contains(&transfer)
                {
                    transfers.push(transfer);
                }
            }
        }
    }
    transfers
}

/// What one step does to the column of one component in tables of one
/// kind, and the transfer that needs.
fn visit(
    step: &Step,
    at: (usize, usize),
    component: usize,
    uses: Uses,
    state: &mut State,
    changes: bool,
) -> Option<RawTransfer> {
    let plain = uses.read || uses.write;
    let change = if changes { uses.change } else { None };
    if !uses.any() && change.is_none() {
        return None;
    }
    let transfer = |direction, from, rows, cause| {
        Some(RawTransfer {
            at,
            direction,
            component,
            from,
            to: step.who,
            rows,
            cause,
        })
    };
    let mut moved = None;
    if step.gpu {
        if !uses.overwrite {
            if let Current::Cpu { by, cause } = state.current {
                moved = transfer(Direction::Upload, by, false, cause);
                state.current = Current::Both;
            } else if let Some(rows) = state.rows {
                moved = transfer(Direction::Upload, rows.by, true, rows.cause);
            }
        }
        state.rows = None;
        if uses.write {
            state.current = Current::Gpu { by: step.who };
        }
        return moved;
    }
    if let Current::Gpu { by } = state.current {
        if plain && !uses.overwrite {
            moved = transfer(Direction::Download, by, false, None);
            state.current = Current::Both;
        } else if !plain && change.is_some() {
            moved = transfer(Direction::Download, by, false, change);
            state.current = Current::Both;
        } else if !plain && uses.row_read {
            moved = transfer(Direction::Download, by, true, None);
        }
    }
    if uses.write {
        state.current = Current::Cpu {
            by: step.who,
            cause: None,
        };
    } else if change.is_some() {
        let written = matches!(state.current, Current::Cpu { cause: None, .. });
        state.current = Current::Cpu {
            by: step.who,
            cause: if written { None } else { change },
        };
    }
    if matches!(state.current, Current::Cpu { .. }) {
        state.rows = None;
    } else if uses.row_write {
        state.rows = Some(Rows {
            by: step.who,
            cause: None,
        });
    }
    moved
}

/// The steps of one tick, in order, and what the program spawns, adds and
/// removes anywhere. The steps use only components that have a GPU copy.
fn steps(program: &Program) -> (Vec<Step>, Shapes) {
    let mut shapes = Shapes::default();
    let mut on_gpu = vec![false; program.components.len()];
    for system in program.systems.iter().filter(|system| system.gpu) {
        for block in gpu_blocks(&system.body.block) {
            for term in block {
                on_gpu[term.component.0] = true;
            }
        }
    }

    let mut steps = Vec::new();
    for (who, system) in program.systems.iter().enumerate() {
        if system.gpu {
            for block in gpu_blocks(&system.body.block) {
                let mut uses = Vec::new();
                for term in block {
                    add_use(&mut uses, term.component.0, term.filter, |uses| {
                        uses.first(term.overwrite);
                        uses.write |= term.write;
                    });
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
            walker.shapes_into(&mut shapes);
            steps.push(Step {
                who,
                gpu: false,
                uses: walker.uses,
            });
        }
    }
    let mut walker = Walker::new(program, &on_gpu, false);
    walker.block(&program.main.block);
    walker.shapes_into(&mut shapes);
    steps.push(Step {
        who: program.systems.len(),
        gpu: false,
        uses: walker.uses,
    });
    (steps, shapes)
}

/// A touched component term of a query, and the tables the query reaches.
struct TermUse {
    component: ComponentId,
    write: bool,
    overwrite: bool,
    filter: Filter,
}

/// For each GPU block of a GPU system's body, the terms of its outer query,
/// then those of the nested queries directly inside it, which it only
/// reads. A term whose parameter the block never names is left out: the
/// block does not need its column.
fn gpu_blocks(body: &Block) -> impl Iterator<Item = Vec<TermUse>> + '_ {
    body.stmts.iter().filter_map(|stmt| {
        let StmtKind::Each { query, body, .. } = &stmt.kind else {
            return None;
        };
        let mut terms: Vec<TermUse> = components(query).collect();
        for inner in &body.stmts {
            if let StmtKind::Each { query, .. } = &inner.kind {
                terms.extend(components(query).map(|term| TermUse {
                    write: false,
                    overwrite: false,
                    ..term
                }));
            }
        }
        Some(terms)
    })
}

/// The touched component terms of a query.
fn components(query: &Query) -> impl Iterator<Item = TermUse> + '_ {
    let filter = Filter::query(query);
    query.terms.iter().filter_map(move |term| match *term {
        Term::Component {
            component,
            write,
            touched: true,
            overwritten,
        } => Some(TermUse {
            component,
            write,
            overwrite: overwritten,
            filter: filter.clone(),
        }),
        _ => None,
    })
}

/// Collects what ordinary code does with the components that have a GPU
/// copy, following calls. Statements are visited in the order they run.
struct Walker<'p> {
    program: &'p Program,
    /// Which components have a GPU copy, by component id.
    on_gpu: &'p [bool],
    /// What the visited code does, in the order of first use.
    uses: Vec<Used>,
    /// What the visited code spawns, adds and removes, counted or not.
    shapes: Shapes,
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
            uses: Vec::new(),
            shapes: Shapes::default(),
            counting,
            counted: vec![false; program.funcs.len()],
            entered: vec![false; program.funcs.len()],
            probing: false,
            ticked: false,
        }
    }

    fn shapes_into(&self, shapes: &mut Shapes) {
        shapes.spawned.extend(self.shapes.spawned.iter().cloned());
        shapes.added.extend(self.shapes.added.iter().cloned());
        shapes.removed.extend(self.shapes.removed.iter().cloned());
    }

    fn record(&mut self, component: ComponentId, filter: Filter, apply: impl FnOnce(&mut Uses)) {
        if self.counting && self.on_gpu[component.0] {
            add_use(&mut self.uses, component.0, filter, apply);
        }
    }

    fn read(&mut self, component: ComponentId, write: bool, overwrite: bool, filter: Filter) {
        self.record(component, filter, |uses| {
            uses.first(overwrite);
            uses.write |= write;
        });
    }

    fn change(&mut self, component: ComponentId, change: Structural, filter: Filter) {
        self.record(component, filter, |uses| {
            uses.change.get_or_insert(change);
        });
    }

    /// A change that moves or removes a whole row.
    fn change_row(&mut self, change: Structural) {
        for component in 0..self.on_gpu.len() {
            self.change(ComponentId(component), change, Filter::any(component));
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
                match &place.kind {
                    PlaceKind::Component {
                        entity, component, ..
                    } => {
                        self.expr(entity);
                        self.record(*component, Filter::any(component.0), |uses| {
                            uses.row_write = true;
                        });
                    }
                    _ => self.place(place),
                }
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
                for term in components(query) {
                    self.read(term.component, term.write, term.overwrite, term.filter);
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
                let table = Filter {
                    require: components.iter().map(|init| init.component.0).collect(),
                    exclude: Vec::new(),
                    exact: true,
                };
                self.shapes.spawned.push(table.require.clone());
                for init in components {
                    self.change(init.component, Structural::Spawn, table.clone());
                }
            }
            ExprKind::Add { entity, components } => {
                self.expr(entity);
                for init in components {
                    if let Some(value) = &init.value {
                        self.expr(value);
                    }
                }
                let added = components.iter().map(|init| init.component.0).collect();
                self.shapes.added.push(added);
                self.change_row(Structural::Add);
            }
            ExprKind::Remove { entity, components } => {
                self.expr(entity);
                let removed = components.iter().map(|c| c.0).collect();
                self.shapes.removed.push(removed);
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
                self.record(*component, Filter::any(component.0), |uses| {
                    uses.row_read = true;
                    uses.row_write |= *write;
                });
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
