//! The effect analysis: query numbers, the summaries of functions, and
//! where each `each` runs.

use super::*;
use crate::tir::{Cores, Deed, EffectKind, QueryId, ResourceId, Run, Summary, Target, Weight};

const DATA: &str = "\
component P
  x: Float = 0.0
  n: Int = 0
  who: Entity
end

component Q
  y: Float = 0.0
  n: Int = 0
end

component Label
  text: String = \"none\"
  size: Int = 0
end

tag Mark

event Hit
  damage: Int
end

resource Stats
  total: Int = 0
  other: Int = 0
  drift: Float = 0.0
  small: Int16 = 0
end
";

/// A checked program and its source text.
struct Checked {
    program: tir::Program,
    text: String,
}

fn checked(body: &str) -> Checked {
    let text = format!("{DATA}\n{body}");
    Checked {
        program: ok(&text),
        text,
    }
}

impl Checked {
    /// The 1-based line of a span, counted in the body: the line after
    /// the declarations is line 1.
    fn line(&self, span: Span) -> usize {
        let line = self.text[..span.start as usize].matches('\n').count() + 1;
        line - DATA.matches('\n').count() - 1
    }

    fn func_name(&self, id: FuncId) -> &str {
        &self.program.funcs[id.0].name
    }

    /// Where every `each` of the program runs, in the order functions,
    /// systems and the top level are visited.
    fn runs(&mut self) -> Vec<Run> {
        let mut found = Vec::new();
        let mut visit = |block: &mut tir::Block| {
            walk::block(block, &mut |node| {
                if let walk::Node::Stmt(Stmt {
                    kind: StmtKind::Each { run, .. },
                    ..
                }) = node
                {
                    found.push(run.clone());
                }
            });
        };
        for func in &mut self.program.funcs {
            visit(&mut func.body.block);
        }
        for system in &mut self.program.systems {
            visit(&mut system.body.block);
        }
        visit(&mut self.program.main.block);
        found
    }

    /// Every run in a short form: `inside`, `gpu`, `cores`, or the reason
    /// as `Kind@line`, and `Kind@function:line<line of the call` when the
    /// thing is in a function.
    fn verdicts(&mut self) -> Vec<String> {
        let runs = self.runs();
        runs.iter()
            .map(|run| match run {
                Run::Inside => "inside".to_string(),
                Run::Gpu => "gpu".to_string(),
                Run::Cores(_) => "cores".to_string(),
                Run::OneCore(reason) => {
                    let line = self.line(reason.span);
                    match reason.via {
                        None => format!("{:?}@{line}", reason.kind),
                        Some(via) => format!(
                            "{:?}@{}:{line}<{}",
                            reason.kind,
                            self.func_name(via.func),
                            self.line(via.call)
                        ),
                    }
                }
            })
            .collect()
    }

    /// What the only independent `each` of the program carries.
    fn cores(&mut self) -> Cores {
        let mut found: Vec<Cores> = self
            .runs()
            .into_iter()
            .filter_map(|run| match run {
                Run::Cores(cores) => Some(cores),
                _ => None,
            })
            .collect();
        assert_eq!(found.len(), 1, "one independent `each`");
        found.remove(0)
    }

    fn summary(&self, func: &str) -> &Summary {
        let index = self
            .program
            .funcs
            .iter()
            .position(|f| f.name == func)
            .expect("the function");
        &self.program.summaries[index]
    }

    /// A summary's effects as `Kind@function:line`.
    fn effects(&self, func: &str) -> Vec<String> {
        self.summary(func)
            .effects
            .iter()
            .map(|effect| {
                format!(
                    "{:?}@{}:{}",
                    effect.kind,
                    self.func_name(effect.func),
                    self.line(effect.span)
                )
            })
            .collect()
    }
}

/// The verdict of the one `each` written in `block`, the body of a
/// system.
fn verdict(block: &str) -> String {
    let mut checked = checked(&format!("system s\n{block}\nend\n"));
    let mut verdicts = checked.verdicts();
    verdicts.retain(|verdict| verdict != "inside");
    assert_eq!(verdicts.len(), 1, "one `each` that is not nested");
    verdicts.remove(0)
}

/// A weight as its terms: the factor, then the queries whose rows it is
/// multiplied by.
fn terms(weight: &Weight) -> Vec<(u64, Vec<usize>)> {
    weight
        .terms
        .iter()
        .map(|term| (term.factor, term.rows.iter().map(|query| query.0).collect()))
        .collect()
}

/// The weight of the one independent `each` of a system with this body.
fn weight(block: &str) -> Vec<(u64, Vec<usize>)> {
    terms(&checked(&format!("system s\n{block}\nend\n")).cores().weight)
}

const STATS_TOTAL: Target = Target::Field(ResourceId(0), 0);
const STATS_OTHER: Target = Target::Field(ResourceId(0), 1);

// ----- query numbers -----

#[test]
fn queries_are_numbered_through_functions_systems_and_the_top_level() {
    let mut program = checked(
        "\
def count_marks -> Int
  World.count(Entity, with: Mark)
end

def sweep
  each P do |p|
    each Q do |q|
      p.n = World.count(Q)
    end
  end
end

system s
  each Q do |q|
    q.n = World.count
  end
end

each P do |p|
  p.n = 0
end
puts World.count(P)
",
    )
    .program;
    let mut ids = Vec::new();
    let mut visit = |block: &mut tir::Block| {
        walk::block(block, &mut |node| match node {
            walk::Node::Stmt(Stmt {
                kind: StmtKind::Each { query, .. },
                ..
            }) => ids.push(("each", query.id.0)),
            walk::Node::Expr(Expr {
                kind: ExprKind::WorldCount(Some(query)),
                ..
            }) => ids.push(("count", query.id.0)),
            _ => {}
        });
    };
    for func in &mut program.funcs {
        visit(&mut func.body.block);
    }
    for system in &mut program.systems {
        visit(&mut system.body.block);
    }
    visit(&mut program.main.block);
    assert_eq!(
        ids,
        [
            ("count", 0),
            ("each", 1),
            ("each", 2),
            ("count", 3),
            ("each", 4),
            ("each", 5),
            ("count", 6),
        ]
    );
}

// ----- summaries -----

#[test]
fn a_summary_lists_each_effect_once_at_its_first_place() {
    let program = checked(
        "\
def busy(e: Entity)
  Stats.total = Stats.other
  Stats.other = 2
  Stats.total = 3
  e[P].n = e[Q].n
  Time.dt = 0.5
  e.despawn
  e.add(Mark)
  e.remove(Mark)
  emit Hit(1)
  Stats.total += 1
  Stats.drift -= 1.0
end
",
    );
    assert_eq!(
        program.effects("busy"),
        [
            "FieldWrite(ResourceId(0), 0)@busy:2",
            "FieldRead(ResourceId(0), 1)@busy:2",
            "FieldWrite(ResourceId(0), 1)@busy:3",
            "RowWrite(ComponentId(0))@busy:5",
            "RowRead(ComponentId(1))@busy:5",
            "DtWrite@busy:6",
            "Despawn@busy:7",
            "Add@busy:8",
            "Remove@busy:9",
            "Emit(EventId(0))@busy:10",
            "FieldSum(ResourceId(0), 0)@busy:11",
            "FieldSum(ResourceId(0), 2)@busy:12",
        ]
    );
}

#[test]
fn a_summary_lists_the_deeds_of_rule_6() {
    let program = checked(
        "\
def deeds
  spawn Mark
  on Hit do |hit|
    print hit.damage
  end
  x = rand
  n = rand(3)
  srand(n)
  srand
  puts x
  y = clock
  sleep(y)
  World.stop
  World.tick
  World.run
  exit(0)
end
",
    );
    let deeds: Vec<Deed> = program
        .summary("deeds")
        .effects
        .iter()
        .filter_map(|effect| match effect.kind {
            EffectKind::Does(deed) => Some(deed),
            _ => None,
        })
        .collect();
    assert_eq!(
        deeds,
        [
            Deed::Spawn,
            Deed::On,
            Deed::Print,
            Deed::Rand,
            Deed::Srand,
            Deed::Puts,
            Deed::Clock,
            Deed::Sleep,
            Deed::WorldStop,
            Deed::WorldTick,
            Deed::WorldRun,
            Deed::Exit,
        ]
    );
}

#[test]
fn a_summary_says_how_query_terms_are_used() {
    let program = checked(
        "\
def terms
  each P, Q, Label do |p, q, label|
    p.n = q.n
  end
end
",
    );
    assert_eq!(
        program.effects("terms"),
        [
            "TermUse(ComponentId(0))@terms:2",
            "TermWrite(ComponentId(0))@terms:2",
            "TermUse(ComponentId(1))@terms:2",
        ]
    );
}

#[test]
fn a_summary_notes_strings_and_lists() {
    let program = checked(
        "\
def text(n: Int) -> Int
  n.to_s.length
end

def list(n: Int) -> Int
  [n, n][0]
end

def plain(n: Int) -> Int
  n + 1
end
",
    );
    assert_eq!(program.effects("text"), ["UsesString@text:2"]);
    assert_eq!(program.effects("list"), ["UsesList@list:6"]);
    assert_eq!(program.effects("plain"), Vec::<String>::new());
}

#[test]
fn a_call_stands_for_everything_its_function_does() {
    let program = checked(
        "\
def leaf
  puts 1
  Stats.total = 1
end

def middle
  Stats.other = 1
  leaf
  puts 2
end

def top
  middle
  Stats.total = 2
end
",
    );
    assert_eq!(
        program.effects("top"),
        [
            "FieldWrite(ResourceId(0), 1)@middle:7",
            "Does(Puts)@leaf:2",
            "FieldWrite(ResourceId(0), 0)@leaf:3",
        ]
    );
}

#[test]
fn an_operator_method_of_a_compound_assignment_is_a_call() {
    let program = checked(
        "\
struct V
  x: Int = 0

  def +(n: Int) -> V
    puts n
    V(x + n)
  end
end

def bump -> Int
  v = V(1)
  v += 2
  v.x
end
",
    );
    assert_eq!(program.effects("bump"), ["Does(Puts)@+:5"]);
}

#[test]
fn functions_that_call_each_other_share_their_effects() {
    let program = checked(
        "\
def ping(n: Int)
  puts n
  pong(n - 1) if n > 0
end

def pong(n: Int)
  Stats.total = n
  ping(n - 1) if n > 0
end

def alone(n: Int) -> Int
  n
end

def caller
  ping(3)
end
",
    );
    assert_eq!(
        program.effects("ping"),
        ["Does(Puts)@ping:2", "FieldWrite(ResourceId(0), 0)@pong:7"]
    );
    assert_eq!(
        program.effects("pong"),
        ["FieldWrite(ResourceId(0), 0)@pong:7", "Does(Puts)@ping:2"]
    );
    assert!(program.summary("ping").recursive);
    assert!(program.summary("pong").recursive);
    assert!(!program.summary("alone").recursive);
    assert!(!program.summary("caller").recursive);
}

#[test]
fn a_summary_lists_the_queries_a_function_can_reach() {
    let program = checked(
        "\
def inner -> Int
  World.count(Entity, with: Mark)
end

def outer -> Int
  n = 0
  each P do |p|
    n += inner
  end
  n + World.count
end

def none -> Int
  1
end
",
    );
    assert_eq!(program.summary("inner").queries, [QueryId(0)]);
    assert_eq!(program.summary("outer").queries, [QueryId(0), QueryId(1)]);
    assert_eq!(program.summary("none").queries, []);
}

#[test]
fn a_summary_has_the_weight_of_a_call() {
    let program = checked(
        "\
def light(n: Int) -> Int
  n + 1
end

def heavy(n: Int) -> Int
  light(n) / 2
end

def loops(n: Int) -> Int
  return 1 if n <= 0
  loops(n - 1)
end

def sweeps -> Int
  n = 0
  each P do |p|
    n += p.n
  end
  n
end
",
    );
    assert_eq!(terms(&program.summary("light").weight), [(1, vec![])]);
    assert_eq!(terms(&program.summary("heavy").weight), [(9, vec![])]);
    assert_eq!(terms(&program.summary("loops").weight), [(64, vec![])]);
    assert_eq!(terms(&program.summary("sweeps").weight), [(2, vec![0])]);
}

// ----- where an `each` runs -----

#[test]
fn a_gpu_block_is_gpu_and_an_each_inside_another_has_no_verdict() {
    let mut program = checked(
        "\
component B
  x: Float32 = 0.0
end

gpu system g
  each B do |a|
    each B do |b|
      a.x += b.x
    end
  end
end

system s
  each P do |p|
    each Q do |q|
      p.n += q.n
    end
  end
end
",
    );
    assert_eq!(program.verdicts(), ["gpu", "inside", "cores", "inside"]);
}

#[test]
fn an_each_in_a_function_and_at_the_top_level_has_a_verdict() {
    let mut program = checked(
        "\
def f
  each P do |p|
    p.n = 1
  end
end

each P do |p|
  puts p.n
end
3.times do
  each Q do |q|
    q.n += 1
  end
end
",
    );
    assert_eq!(program.verdicts(), ["cores", "Does(Puts)@8", "cores"]);
}

// ----- rule 1 -----

#[test]
fn rule_1_a_string_or_a_list_keeps_a_loop_on_one_core() {
    assert_eq!(
        verdict("  each P do |p|\n    p.n = \"ab\".length\n  end"),
        "UsesString@3"
    );
    assert_eq!(
        verdict("  each Label do |label|\n    label.size = label.text.length\n  end"),
        "UsesString@3"
    );
    assert_eq!(
        verdict("  each Label do |label|\n    copy = label\n  end"),
        "UsesString@3"
    );
    assert_eq!(
        verdict("  each Entity, P do |e, p|\n    e.add(Label)\n  end"),
        "UsesString@3"
    );
    assert_eq!(
        verdict("  xs = [1, 2]\n  each P do |p|\n    p.n = xs[0]\n  end"),
        "UsesList@4"
    );
    assert_eq!(
        verdict("  xs: [Int] = []\n  each P do |p|\n    xs.push(p.n)\n  end"),
        "UsesList@4"
    );
}

#[test]
fn rule_1_a_term_with_a_string_field_is_fine_when_the_block_leaves_it_alone() {
    assert_eq!(
        verdict("  each Label do |label|\n    label.size += 1\n  end"),
        "cores"
    );
}

// ----- rule 2 -----

#[test]
fn rule_2_assignments_outside_the_loops_own_rows() {
    assert_eq!(
        verdict("  each P do |p|\n    p.who[Q].n = 1\n  end"),
        "AssignsRow(ComponentId(1))@3"
    );
    assert_eq!(
        verdict("  each P do |p|\n    p.who[Q] = Q(1.0, 1)\n  end"),
        "AssignsRow(ComponentId(1))@3"
    );
    assert_eq!(
        verdict("  each P do |p|\n    each Q do |q|\n      q.n = p.n\n    end\n  end"),
        "AssignsNested(ComponentId(1))@3"
    );
    assert_eq!(
        verdict("  each P do |p|\n    Stats.other = p.n\n  end"),
        "Assigns { target: Field(ResourceId(0), 1), spelled_out: false }@3"
    );
    assert_eq!(
        verdict("  each P do |p|\n    Stats.total *= 2\n  end"),
        "Assigns { target: Field(ResourceId(0), 0), spelled_out: false }@3"
    );
    assert_eq!(
        verdict("  each P do |p|\n    Time.dt = 0.5\n  end"),
        "AssignsDt@3"
    );
    assert_eq!(
        verdict("  best = 0\n  each P do |p|\n    best = p.n\n  end"),
        "Assigns { target: Local(LocalId(0)), spelled_out: false }@4"
    );
}

#[test]
fn rule_2_variables_created_in_the_block_can_be_assigned() {
    assert_eq!(
        verdict(
            "  each Entity, P do |e, p|\n    a = p.n\n    a = a + 1\n    a += 1\n    \
             3.times do |i|\n      a += i\n    end\n    each Entity, Q do |other, q|\n      \
             other = e\n      a += q.n\n    end\n    e = p.who\n    p.n = a\n  end"
        ),
        "cores"
    );
}

#[test]
fn rule_2_an_int_assigned_itself_plus_something_gets_the_hint() {
    assert_eq!(
        verdict("  each P do |p|\n    Stats.total = Stats.total + p.n\n  end"),
        "Assigns { target: Field(ResourceId(0), 0), spelled_out: true }@3"
    );
    assert_eq!(
        verdict("  each P do |p|\n    Stats.total = p.n + Stats.total\n  end"),
        "Assigns { target: Field(ResourceId(0), 0), spelled_out: true }@3"
    );
    assert_eq!(
        verdict("  each P do |p|\n    Stats.total = Stats.total - 1\n  end"),
        "Assigns { target: Field(ResourceId(0), 0), spelled_out: true }@3"
    );
    assert_eq!(
        verdict("  n = 0\n  each P do |p|\n    n = n + 1\n  end"),
        "Assigns { target: Local(LocalId(0)), spelled_out: true }@4"
    );
    // `1 - total` is not the total minus something, and a `Float` is no
    // whole-number total.
    assert_eq!(
        verdict("  each P do |p|\n    Stats.total = 1 - Stats.total\n  end"),
        "Assigns { target: Field(ResourceId(0), 0), spelled_out: false }@3"
    );
    assert_eq!(
        verdict("  each P do |p|\n    Stats.drift = Stats.drift + 1.0\n  end"),
        "Assigns { target: Field(ResourceId(0), 2), spelled_out: false }@3"
    );
}

// ----- rule 3 -----

#[test]
fn rule_3_a_component_the_loop_assigns_is_not_read_another_way() {
    assert_eq!(
        verdict("  each P do |p|\n    p.n = p.who[P].n\n  end"),
        "ReadsAssigned { component: ComponentId(0), nested: false }@3"
    );
    assert_eq!(
        verdict("  each P do |p|\n    each P do |other|\n      p.n += other.n\n    end\n  end"),
        "ReadsAssigned { component: ComponentId(0), nested: true }@3"
    );
    // A nested `each` that never uses its parameter reads nothing.
    assert_eq!(
        verdict("  each P do |p|\n    each P do |other|\n      p.n += 1\n    end\n  end"),
        "cores"
    );
    // Reading what the loop does not assign is fine, by either way.
    assert_eq!(
        verdict(
            "  each P, Q do |p, q|\n    q.n = p.who[P].n\n    each P do |other|\n      \
             q.y += other.x\n    end\n  end"
        ),
        "cores"
    );
}

// ----- rule 4 -----

#[test]
fn rule_4_whole_number_sums_are_collected_in_order() {
    let mut program = checked(
        "\
system s
  count = 0
  each P do |p|
    Stats.other += 1
    count += p.n
    if p.n > 0
      Stats.total -= p.n
    end
    each Q do |q|
      Stats.other += q.n
    end
  end
end
",
    );
    let cores = program.cores();
    assert_eq!(
        cores.sums,
        [STATS_OTHER, Target::Local(LocalId(0)), STATS_TOTAL]
    );
}

#[test]
fn rule_4_a_float_or_compact_sum_keeps_a_loop_on_one_core() {
    assert_eq!(
        verdict("  each P do |p|\n    Stats.drift += p.x\n  end"),
        "FloatSum(Field(ResourceId(0), 2))@3"
    );
    assert_eq!(
        verdict("  sum = 0.0\n  each P do |p|\n    sum -= p.x\n  end"),
        "FloatSum(Local(LocalId(0)))@4"
    );
    assert_eq!(
        verdict("  each P do |p|\n    Stats.small += 1\n  end"),
        "CompactSum(Field(ResourceId(0), 3))@3"
    );
}

#[test]
fn rule_4_a_total_is_not_read_or_assigned_otherwise() {
    assert_eq!(
        verdict("  each P do |p|\n    Stats.total += 1\n    p.n = Stats.total\n  end"),
        "SummedAndRead(Field(ResourceId(0), 0))@4"
    );
    assert_eq!(
        verdict("  each P do |p|\n    p.n = Stats.total\n    Stats.total += 1\n  end"),
        "SummedAndRead(Field(ResourceId(0), 0))@3"
    );
    assert_eq!(
        verdict("  each P do |p|\n    Stats.total += Stats.total\n  end"),
        "SummedAndRead(Field(ResourceId(0), 0))@3"
    );
    assert_eq!(
        verdict("  n = 0\n  each P do |p|\n    n += 1\n    p.n = n\n  end"),
        "SummedAndRead(Local(LocalId(0)))@5"
    );
    assert_eq!(
        verdict("  each P do |p|\n    Stats.total += 1\n    Stats.total = 0\n  end"),
        "SummedAndAssigned(Field(ResourceId(0), 0))@4"
    );
    assert_eq!(
        verdict("  n = 0\n  each P do |p|\n    n = 5\n    n += 1\n  end"),
        "SummedAndAssigned(Local(LocalId(0)))@4"
    );
    // Reading another field of the resource is fine.
    assert_eq!(
        verdict("  each P do |p|\n    Stats.total += Stats.other\n  end"),
        "cores"
    );
}

#[test]
fn rule_4_a_sum_stands_in_the_block_itself() {
    let mut program = checked(
        "\
def add(n: Int)
  Stats.total += n
end

def look -> Int
  Stats.total
end

def set
  Stats.total = 0
end

system in_function
  each P do |p|
    add(p.n)
  end
end

system also_summed
  each P do |p|
    Stats.total += 1
    add(p.n)
  end
end

system read_in_function
  each P do |p|
    Stats.total += 1
    p.n = look
  end
end

system assigned_in_function
  each P do |p|
    Stats.total += 1
    set
  end
end
",
    );
    assert_eq!(
        program.verdicts(),
        [
            "SumInFunction(Field(ResourceId(0), 0))@add:2<15",
            "SumInFunction(Field(ResourceId(0), 0))@add:2<22",
            "SummedAndRead(Field(ResourceId(0), 0))@look:6<29",
            "SummedAndAssigned(Field(ResourceId(0), 0))@set:10<36",
        ]
    );
}

// ----- rule 5 -----

#[test]
fn rule_5_queued_changes_and_events_are_fine() {
    assert_eq!(
        verdict(
            "  each Entity, P do |e, p|\n    e.despawn if p.n < 0\n    p.who.despawn\n    \
             e.add(Mark)\n    e.add(Q(1.0, 2))\n    p.who.remove(Mark, Q)\n    \
             emit Hit(p.n)\n  end"
        ),
        "cores"
    );
}

// ----- rule 6 -----

#[test]
fn rule_6_each_deed_keeps_a_loop_on_one_core() {
    let deed = |line: &str| verdict(&format!("  each P do |p|\n    {line}\n  end"));
    assert_eq!(deed("spawn Mark"), "Does(Spawn)@3");
    assert_eq!(deed("on Hit do |hit|\n    end"), "Does(On)@3");
    assert_eq!(deed("p.x = rand"), "Does(Rand)@3");
    assert_eq!(deed("p.n = rand(4)"), "Does(Rand)@3");
    assert_eq!(deed("srand(1)"), "Does(Srand)@3");
    assert_eq!(deed("srand"), "Does(Srand)@3");
    assert_eq!(deed("puts p.n"), "Does(Puts)@3");
    assert_eq!(deed("print p.n"), "Does(Print)@3");
    assert_eq!(deed("p.x = clock"), "Does(Clock)@3");
    assert_eq!(deed("sleep(0.0)"), "Does(Sleep)@3");
    assert_eq!(deed("exit(1)"), "Does(Exit)@3");
    assert_eq!(deed("World.stop"), "Does(WorldStop)@3");
    assert_eq!(deed("break if p.n > 3"), "Does(Break)@3");
}

#[test]
fn rule_6_a_break_counts_only_when_it_leaves_the_each() {
    assert_eq!(
        verdict(
            "  each P do |p|\n    3.times do\n      break\n    end\n    while true\n      \
             break\n    end\n    each Q do |q|\n      break\n    end\n  end"
        ),
        "cores"
    );
    assert_eq!(
        verdict("  each P do |p|\n    if p.n > 0\n      break\n    end\n  end"),
        "Does(Break)@4"
    );
}

#[test]
fn rule_6_a_return_in_the_block_counts_and_one_in_a_called_function_does_not() {
    let mut program = checked(
        "\
def cap(n: Int) -> Int
  return 9 if n > 9
  n
end

def first -> Int
  each P do |p|
    3.times do
      return p.n
    end
  end
  0
end

system s
  each P do |p|
    p.n = cap(p.n)
  end
end
",
    );
    assert_eq!(program.verdicts(), ["Does(Return)@9", "cores"]);
}

#[test]
fn rule_6_tick_and_run_are_found_through_functions() {
    let mut program = checked(
        "\
def step
  World.tick
end

def go
  World.run
end

def far
  go
end

each P do |p|
  step
end
each P do |p|
  far
end
",
    );
    assert_eq!(
        program.verdicts(),
        ["Does(WorldTick)@step:2<14", "Does(WorldRun)@go:6<17"]
    );
}

// ----- rule 7 -----

#[test]
fn rule_7_a_component_of_a_gpu_block_is_reached_only_through_own_parameters() {
    let mut program = checked(
        "\
component A
  x: Float32 = 0.0
end

component B
  x: Float32 = 0.0
end

gpu system first
  each A do |a|
    a.x += 1.0
  end
end

gpu system second
  each A do |a|
    each B do |b|
      a.x += b.x
    end
  end
end

def read(e: Entity) -> Float
  e[A].x
end

system row
  each P do |p|
    p.x = p.who[B].x
  end
end

system nested
  each P do |p|
    each A do |a|
      p.x += a.x
    end
  end
end

system called
  each P do |p|
    p.x = read(p.who)
  end
end

system own
  each A, B do |a, b|
    a.x += b.x
  end
end

system unused
  each P do |p|
    each A do |a|
      p.n += 1
    end
  end
end

system assigned_first
  each A do |a|
    a.x = a.x + 1.0
    each A do |other|
      a.x += other.x
    end
  end
end
",
    );
    let mut verdicts = program.verdicts();
    verdicts.retain(|verdict| verdict != "inside");
    // A is component 4 and B component 5; `first` is system 0 and `second`
    // system 1.
    assert_eq!(
        verdicts,
        [
            "gpu",
            "gpu",
            "ReadsGpu { component: ComponentId(5), system: 1, nested: false }@29",
            "ReadsGpu { component: ComponentId(4), system: 0, nested: true }@35",
            "ReadsGpu { component: ComponentId(4), system: 0, nested: false }@read:24<43",
            "cores",
            "cores",
            "ReadsAssigned { component: ComponentId(4), nested: true }@64",
        ]
    );
}

// ----- the first thing in source order -----

#[test]
fn the_first_thing_in_source_order_is_the_reason() {
    assert_eq!(
        verdict("  each P do |p|\n    p.x = rand\n    puts p.x\n  end"),
        "Does(Rand)@3"
    );
    assert_eq!(
        verdict("  each P do |p|\n    puts p.x\n    p.x = rand\n  end"),
        "Does(Puts)@3"
    );
    // The statement comes before what it holds.
    assert_eq!(
        verdict("  each P do |p|\n    puts rand\n  end"),
        "Does(Puts)@3"
    );
    assert_eq!(
        verdict("  each P do |p|\n    Stats.other = rand(3)\n  end"),
        "Assigns { target: Field(ResourceId(0), 1), spelled_out: false }@3"
    );
}

#[test]
fn at_a_call_the_first_effect_of_the_function_is_the_reason() {
    let mut program = checked(
        "\
def quiet(n: Int) -> Int
  n + 1
end

def loud(n: Int) -> Int
  puts n
  rand(n)
end

system s
  each P do |p|
    p.n = quiet(p.n)
    p.n = loud(p.n)
    p.x = rand
  end
end
",
    );
    assert_eq!(program.verdicts(), ["Does(Puts)@loud:6<13"]);
}

// ----- what an independent loop carries -----

#[test]
fn an_independent_loop_lists_the_queries_it_can_reach() {
    let mut program = checked(
        "\
def marks -> Int
  World.count(Entity, with: Mark)
end

def near(x: Float) -> Int
  n = 0
  each Q do |q|
    n += 1 if q.y < x
  end
  n + marks
end

system other
  each Q do |q|
    q.n = 0
  end
end

system s
  each P do |p|
    each Q do |q|
      p.n += World.count(Label)
    end
    p.n += near(p.x)
  end
end
",
    );
    let runs = program.runs();
    let Run::Cores(cores) = &runs[2] else {
        panic!("`s` is independent: {:?}", runs[2]);
    };
    // 0 is the count in `marks`, 1 the `each` of `near`, 2 the loop of
    // `other`, 3 the loop itself, 4 its nested `each` and 5 the count in
    // it.
    assert_eq!(
        cores.queries,
        [QueryId(0), QueryId(1), QueryId(4), QueryId(5)]
    );
}

#[test]
fn the_weight_of_a_run_counts_its_operations() {
    // Two field reads, a multiplication, an addition and a field write,
    // twice: reading `Time.dt` weighs nothing.
    assert_eq!(
        weight("  each P, Q do |p, q|\n    p.x += q.y * Time.dt\n    q.y += p.x * Time.dt\n  end"),
        [(10, vec![])]
    );
    // A subtraction with its read and write, a comparison with its read,
    // and the arm: a sum and a despawn.
    assert_eq!(
        weight(
            "  each Entity, P do |e, p|\n    p.x -= Time.dt\n    if p.x <= 0.0\n      \
             Stats.total += 1\n      e.despawn\n    end\n  end"
        ),
        [(15, vec![])]
    );
    assert_eq!(
        weight("  each P do |p|\n    p.n = p.n / 3 % 2\n  end"),
        [(18, vec![])]
    );
    assert_eq!(
        weight("  each P do |p|\n    p.x = Math.sqrt(p.x)\n  end"),
        [(10, vec![])]
    );
    // `e[C]` is 4 and a field read, `has?` and `alive?` 1 each,
    // `World.count` 4.
    assert_eq!(
        weight(
            "  each P do |p|\n    if p.who.alive? && p.who.has?(Q)\n      \
             p.n = p.who[Q].n + World.count\n    end\n  end"
        ),
        [(17, vec![])]
    );
}

#[test]
fn the_weight_of_a_run_is_never_less_than_one() {
    assert_eq!(weight("  each P do |p|\n  end"), [(1, vec![])]);
    assert_eq!(
        weight("  each P do |p|\n    each Q do |q|\n    end\n  end"),
        [(1, vec![])]
    );
    assert_eq!(
        weight("  each P do |p|\n    each Q do |q|\n      p.n += 1\n    end\n  end"),
        [(1, vec![]), (3, vec![1])]
    );
}

#[test]
fn a_branch_weighs_its_condition_and_its_heaviest_arm() {
    assert_eq!(
        weight(
            "  each P do |p|\n    if p.n > 0\n      p.n = 1\n    else\n      \
             p.n = p.n * p.n * p.n\n    end\n  end"
        ),
        [(8, vec![])]
    );
}

#[test]
fn a_loop_that_is_not_a_query_weighs_eight_rounds() {
    assert_eq!(
        weight("  each P do |p|\n    3.times do\n      p.n += 1\n    end\n  end"),
        [(24, vec![])]
    );
    assert_eq!(
        weight("  each P do |p|\n    while p.n < 10\n      p.n += 1\n    end\n  end"),
        [(40, vec![])]
    );
}

#[test]
fn a_nested_each_weighs_its_block_times_its_rows_wherever_it_stands() {
    // In a branch, with a constant beside it.
    assert_eq!(
        weight(
            "  each P do |p|\n    if p.n > 0\n      each Q do |q|\n        \
             p.n += q.n\n      end\n    end\n  end"
        ),
        [(2, vec![]), (4, vec![1])]
    );
    // In a counting loop.
    assert_eq!(
        weight(
            "  each P do |p|\n    2.times do\n      each Q do |q|\n        \
             p.n += q.n\n      end\n    end\n  end"
        ),
        [(1, vec![]), (32, vec![1])]
    );
    // Inside another nested `each`: the rows of both.
    assert_eq!(
        weight(
            "  each P do |p|\n    each Q do |q|\n      p.n += q.n\n      \
             each Label do |label|\n        p.n += label.size\n      end\n    end\n  end"
        ),
        [(1, vec![]), (4, vec![1]), (4, vec![1, 2])]
    );
}

#[test]
fn a_call_weighs_what_its_function_weighs() {
    let mut program = checked(
        "\
def near(x: Float) -> Int
  n = 0
  each Q do |q|
    n += 1 if q.y < x
  end
  n
end

def deep(n: Int) -> Int
  return 0 if n <= 0
  deep(n - 1)
end

system s
  each P do |p|
    p.n = near(p.x) + deep(3)
  end
end
",
    );
    let runs = program.runs();
    let Run::Cores(cores) = &runs[1] else {
        panic!("`s` is independent: {:?}", runs[1]);
    };
    // A field read, an addition, a field write and 64 for the function
    // that recurses; three for each row of the query in `near`.
    assert_eq!(terms(&cores.weight), [(67, vec![]), (3, vec![0])]);
}

#[test]
fn factors_stop_at_the_limit() {
    // Twenty-two loops inside one another: 3 * 8 ** 22 is past the limit.
    // The checker needs a deeper stack for them than a test thread has.
    let deep = std::thread::Builder::new()
        .stack_size(256 << 20)
        .spawn(|| {
            let mut nested = String::from("p.n += 1\n");
            for _ in 0..22 {
                nested = format!("8.times do\n{nested}end\n");
            }
            weight(&format!("  each P do |p|\n{nested}  end"))
        })
        .expect("start a thread");
    let weight = deep.join().expect("the nested loops check");
    assert_eq!(weight, [(Weight::LIMIT, vec![])]);
}
