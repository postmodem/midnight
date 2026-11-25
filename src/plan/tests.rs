//! Plans for programs parsed and checked from source text. Each test adds
//! its systems to [`WORLD`]: two components and the GPU system `move`,
//! which writes `Position` and reads `Velocity`.

use std::path::PathBuf;

use super::*;
use crate::source::SourceMap;
use crate::{ast, check, diag, parser};

const WORLD: &str = "
component Position
  x: Float32
  y: Float32
end

component Velocity
  dx: Float32
  dy: Float32
end

gpu system move
  each Position, Velocity do |pos, vel|
    pos.x += vel.dx * Time.dt
    pos.y += vel.dy * Time.dt
  end
end
";

/// The rendered plan of `text`, which must have no errors.
#[track_caller]
fn plan_of(text: &str) -> String {
    let mut sources = SourceMap::new();
    let file = sources.add(
        "test.12am".to_string(),
        PathBuf::from("test.12am"),
        text.to_string(),
    );
    let items = parser::parse(file, text).unwrap_or_else(|error| {
        panic!(
            "syntax error in a plan test: {} at {}..{}\n{text}",
            error.message, error.span.start, error.span.end
        )
    });
    match check::check(&ast::Program { items }, &sources) {
        Ok(program) => render(&plan(&program)),
        Err(errors) => panic!(
            "unexpected errors:\n{}",
            diag::render(&sources, &errors, false)
        ),
    }
}

/// Checks the plan of [`WORLD`] followed by `rest`.
#[track_caller]
fn assert_plan(rest: &str, expected: &[&str]) {
    let actual = plan_of(&format!("{WORLD}{rest}"));
    let actual: Vec<&str> = actual.lines().collect();
    assert_eq!(actual, expected, "\n{rest}");
}

#[test]
fn renders_the_columns_lined_up() {
    let line = |direction, from: &str, to: &str, cause| Transfer {
        direction,
        component: "Position".to_string(),
        from: from.to_string(),
        to: to.to_string(),
        cause,
    };
    let transfers = [
        line(Direction::Download, "move", "report", None),
        line(
            Direction::Download,
            "move",
            "reap",
            Some(Structural::Despawn),
        ),
        line(Direction::Upload, "reap", "move", Some(Structural::Despawn)),
    ];
    assert_eq!(
        render(&transfers),
        "\
download  Position  move -> report
download  Position  move -> reap     on despawn
upload    Position  reap -> move     on despawn
"
    );
    assert_eq!(render(&[]), "");
}

#[test]
fn pads_to_the_widest_component_and_route() {
    let transfers = [
        Transfer {
            direction: Direction::Upload,
            component: "Hp".to_string(),
            from: TOP_LEVEL.to_string(),
            to: "regenerate".to_string(),
            cause: Some(Structural::Spawn),
        },
        Transfer {
            direction: Direction::Download,
            component: "Position".to_string(),
            from: "a".to_string(),
            to: "b".to_string(),
            cause: Some(Structural::Add),
        },
    ];
    assert_eq!(
        render(&transfers),
        "\
upload    Hp        top level -> regenerate   on spawn
download  Position  a -> b                    on add
"
    );
}

/// Every order of up to five steps on one component, each step one of the
/// seven things a step can do with it. A tick must come to end as it began,
/// with and without the structural changes, and a transfer of the tick with
/// them must be unmarked exactly when the tick without them has it too.
#[test]
fn every_short_tick_settles_and_its_marked_transfers_are_the_added_ones() {
    let uses = |read, write, change| Uses {
        read,
        write,
        change,
    };
    let despawn = Some(Structural::Despawn);
    let kinds = [
        (true, uses(true, false, None)),
        (true, uses(true, true, None)),
        (false, uses(true, false, None)),
        (false, uses(true, true, None)),
        (false, uses(false, false, despawn)),
        (false, uses(true, false, despawn)),
        (false, uses(true, true, despawn)),
    ];
    for length in 1..=5u32 {
        for code in 0..kinds.len().pow(length) {
            let order: Vec<usize> = (0..length)
                .map(|place| code / kinds.len().pow(place) % kinds.len())
                .collect();
            let steps: Vec<Step> = order
                .iter()
                .enumerate()
                .map(|(who, &kind)| Step {
                    who,
                    gpu: kinds[kind].0,
                    uses: vec![(0, kinds[kind].1)],
                })
                .collect();
            for changes in [false, true] {
                let mut columns = vec![Current::Cpu {
                    by: steps.len(),
                    cause: None,
                }];
                let settled = (0..TICK_LIMIT).any(|_| {
                    let before = columns.clone();
                    tick(&steps, &mut columns, changes);
                    columns == before
                });
                assert!(settled, "steps {order:?}, changes {changes}");
            }
            let quiet = steady_tick(&steps, 1, steps.len(), false);
            let changed = steady_tick(&steps, 1, steps.len(), true);
            assert!(quiet.iter().all(|t| t.cause.is_none()), "steps {order:?}");
            for t in &changed {
                let also_quiet = quiet
                    .iter()
                    .any(|q| q.at == t.at && q.direction == t.direction);
                assert_eq!(
                    also_quiet,
                    t.cause.is_none(),
                    "steps {order:?}, step {}",
                    t.at.0
                );
            }
        }
    }
}

#[test]
fn data_that_stays_on_the_gpu_has_an_empty_plan() {
    assert_plan("", &[]);
    assert_plan(
        "
spawn Position(0.0, 0.0), Velocity(1.0, 2.0)
100.times do
  World.tick
end
",
        &[],
    );
}

#[test]
fn a_cpu_reader_downloads_once_and_never_uploads() {
    assert_plan(
        "
system report
  each Position do |pos|
    puts pos.x
  end
end
",
        &["download  Position  move -> report"],
    );
}

#[test]
fn a_cpu_reader_before_the_gpu_writer_downloads_last_ticks_result() {
    let text = "
component Position
  x: Float32
end

system report
  each Position do |pos|
    puts pos.x
  end
end

gpu system move
  each Position do |pos|
    pos.x += 1.0
  end
end
";
    assert_eq!(plan_of(text), "download  Position  move -> report\n");
}

#[test]
fn a_cpu_writer_before_a_gpu_reader_uploads() {
    let text = "
component Position
  x: Float32
end

component Velocity
  dx: Float32
end

system steer
  each Velocity do |vel|
    vel.dx *= 0.5
  end
end

gpu system move
  each Position, Velocity do |pos, vel|
    pos.x += vel.dx
  end
end
";
    assert_eq!(plan_of(text), "upload    Velocity  steer -> move\n");
}

#[test]
fn a_component_the_gpu_only_reads_is_never_downloaded() {
    assert_plan(
        "
system steer
  each Velocity do |vel|
    vel.dx *= 0.5
  end
end

system log
  each Velocity do |vel|
    puts vel.dx
  end
end
",
        &["upload    Velocity  steer -> move"],
    );
}

#[test]
fn a_cpu_writer_of_a_gpu_written_component_moves_it_both_ways() {
    assert_plan(
        "
system wrap
  each Position do |pos|
    pos.x = 0.0 if pos.x > 100.0
  end
end
",
        &[
            "upload    Position  wrap -> move",
            "download  Position  move -> wrap",
        ],
    );
}

#[test]
fn a_cpu_reader_between_two_gpu_writers_downloads_once() {
    assert_plan(
        "
system report
  each Position do |pos|
    puts pos.x
  end
end

gpu system settle
  each Position do |pos|
    pos.y *= 0.5
  end
end
",
        &["download  Position  move -> report"],
    );
}

#[test]
fn a_cpu_writer_between_two_gpu_writers_downloads_and_uploads() {
    assert_plan(
        "
system wrap
  each Position do |pos|
    pos.x = 0.0 if pos.x > 100.0
  end
end

gpu system settle
  each Position do |pos|
    pos.y *= 0.5
  end
end
",
        &[
            "download  Position  move -> wrap",
            "upload    Position  wrap -> settle",
        ],
    );
}

#[test]
fn two_readers_share_one_download() {
    assert_plan(
        "
system report
  each Position do |pos|
    puts pos.x
  end
end

system audit
  each Position do |pos|
    puts pos.y
  end
end
",
        &["download  Position  move -> report"],
    );
}

#[test]
fn gpu_systems_pass_data_without_transfers() {
    assert_plan(
        "
gpu system settle
  each Position do |pos|
    pos.y *= 0.5
  end
end

gpu system steer
  each Velocity, Position do |vel, pos|
    vel.dx -= pos.x * 0.01
  end
end
",
        &[],
    );
}

#[test]
fn the_blocks_of_a_gpu_system_are_steps_in_order() {
    let text = "
component Position
  x: Float32
end

component Heat
  value: Float32
end

gpu system simulate
  each Heat, Position do |heat, pos|
    heat.value += pos.x
  end
  each Position do |pos|
    pos.x += 1.0
  end
end

system cool
  each Heat do |heat|
    heat.value = 0.0
  end
end

system nudge
  each Position do |pos|
    pos.x -= 0.5
  end
end
";
    assert_eq!(
        plan_of(text),
        "\
upload    Heat      cool -> simulate
upload    Position  nudge -> simulate
download  Heat      simulate -> cool
download  Position  simulate -> nudge
"
    );
}

#[test]
fn calls_count_as_the_calling_systems_accesses() {
    assert_plan(
        "
def total_x -> Float
  sum = 0.0
  each Position do |pos|
    sum += pos.x
  end
  sum
end

def describe -> String
  \"total #{total_x}\"
end

system report
  puts describe
end
",
        &["download  Position  move -> report"],
    );
}

#[test]
fn methods_and_operator_methods_count_too() {
    assert_plan(
        "
struct Meter
  total: Float

  def +(other: Meter) -> Meter
    each Velocity do |vel|
      vel.dx = 0.0
    end
    Meter(total + other.total)
  end

  def reading -> Float
    sum = total
    each Position do |pos|
      sum += pos.x
    end
    sum
  end
end

system report
  meter = Meter(0.0)
  meter += Meter(1.0)
  puts meter.reading
end
",
        &[
            "upload    Velocity  report -> move",
            "download  Position  move -> report",
        ],
    );
}

#[test]
fn recursive_functions_are_followed_once() {
    assert_plan(
        "
def even?(n: Int) -> Bool
  each Position do |pos|
    pos.x = 0.0
  end
  n == 0 || odd?(n - 1)
end

def odd?(n: Int) -> Bool
  each Velocity do |vel|
    puts vel.dx
  end
  n != 0 && even?(n - 1)
end

system parity
  puts odd?(3)
end
",
        &[
            "upload    Position  parity -> move",
            "download  Position  move -> parity",
        ],
    );
}

#[test]
fn an_indexed_read_downloads() {
    assert_plan(
        "
tag Tracked

system watch
  each Entity, with: Tracked do |e|
    puts e[Position].x
  end
end
",
        &["download  Position  move -> watch"],
    );
}

#[test]
fn an_indexed_write_downloads_and_uploads() {
    assert_plan(
        "
tag Tracked

system pin
  each Entity, with: Tracked do |e|
    e[Position].x = 0.0
  end
end

system swap
  each Entity, with: Tracked do |e|
    e[Velocity] = Velocity(0.0, 0.0)
  end
end
",
        &[
            "upload    Position  pin -> move",
            "upload    Velocity  swap -> move",
            "download  Position  move -> pin",
        ],
    );
}

#[test]
fn counting_and_membership_tests_need_no_column() {
    assert_plan(
        "
system census
  puts World.count(Position)
  each Entity, with: Position do |e|
    puts e.has?(Velocity)
    puts e.alive?
  end
end
",
        &[],
    );
}

#[test]
fn spawn_changes_the_components_it_lists() {
    assert_plan(
        "
component Label
  id: Int
end

system emitter
  spawn Position(0.0, 0.0), Label(1)
end
",
        &[
            "upload    Position  emitter -> move   on spawn",
            "download  Position  move -> emitter   on spawn",
        ],
    );
    assert_plan(
        "
component Label
  id: Int
end

system emitter
  spawn Label(1)
end
",
        &[],
    );
}

#[test]
fn spawn_of_a_component_the_gpu_only_reads_uploads_it() {
    assert_plan(
        "
system emitter
  spawn Velocity(1.0, 0.0)
end
",
        &["upload    Velocity  emitter -> move   on spawn"],
    );
}

#[test]
fn despawn_changes_every_component_with_a_gpu_copy() {
    assert_plan(
        "
component Lifetime
  left: Int
end

system reap
  each Entity, Lifetime do |e, life|
    life.left -= 1
    e.despawn if life.left <= 0
  end
end
",
        &[
            "upload    Position  reap -> move   on despawn",
            "upload    Velocity  reap -> move   on despawn",
            "download  Position  move -> reap   on despawn",
        ],
    );
}

#[test]
fn add_and_remove_change_every_component_with_a_gpu_copy() {
    assert_plan(
        "
tag Frozen

system freeze
  each Entity, without: Frozen do |e|
    e.add(Frozen)
  end
end
",
        &[
            "upload    Position  freeze -> move   on add",
            "upload    Velocity  freeze -> move   on add",
            "download  Position  move -> freeze   on add",
        ],
    );
    assert_plan(
        "
tag Frozen

system thaw
  each Entity, with: Frozen do |e|
    e.remove(Frozen)
  end
end
",
        &[
            "upload    Position  thaw -> move   on remove",
            "upload    Velocity  thaw -> move   on remove",
            "download  Position  move -> thaw   on remove",
        ],
    );
}

#[test]
fn the_first_structural_change_of_a_system_is_named() {
    assert_plan(
        "
tag Frozen

def thaw(e: Entity)
  e.remove(Frozen)
end

system churn
  each Entity, with: Frozen do |e|
    thaw(e)
    e.despawn
  end
  spawn Position(0.0, 0.0)
end
",
        &[
            "upload    Position  churn -> move   on remove",
            "upload    Velocity  churn -> move   on remove",
            "download  Position  move -> churn   on remove",
        ],
    );
}

#[test]
fn a_read_beside_a_structural_change_marks_only_the_upload() {
    assert_plan(
        "
system reap
  each Entity, Position do |e, pos|
    e.despawn if pos.x > 100.0
  end
end
",
        &[
            "upload    Position  reap -> move   on despawn",
            "upload    Velocity  reap -> move   on despawn",
            "download  Position  move -> reap",
        ],
    );
}

#[test]
fn a_write_beside_a_structural_change_marks_nothing() {
    assert_plan(
        "
system reap
  each Entity, Position do |e, pos|
    pos.x += 1.0
    e.despawn if pos.x > 100.0
  end
end
",
        &[
            "upload    Position  reap -> move",
            "upload    Velocity  reap -> move   on despawn",
            "download  Position  move -> reap",
        ],
    );
}

#[test]
fn a_structural_change_after_a_writer_leaves_its_upload_unmarked() {
    assert_plan(
        "
system wrap
  each Position do |pos|
    pos.x = 0.0 if pos.x > 100.0
  end
end

system reap
  each Entity do |e|
    e.despawn
  end
end
",
        &[
            "upload    Position  wrap -> move",
            "upload    Velocity  reap -> move   on despawn",
            "download  Position  move -> wrap",
        ],
    );
}

#[test]
fn a_structural_change_before_a_writer_adds_its_own_download() {
    assert_plan(
        "
system reap
  each Entity do |e|
    e.despawn
  end
end

system wrap
  each Position do |pos|
    pos.x = 0.0 if pos.x > 100.0
  end
end
",
        &[
            "upload    Position  wrap -> move",
            "upload    Velocity  reap -> move   on despawn",
            "download  Position  move -> reap   on despawn",
            "download  Position  move -> wrap",
        ],
    );
}

#[test]
fn a_structural_change_after_a_reader_downloads_nothing_more() {
    assert_plan(
        "
system report
  each Position do |pos|
    puts pos.x
  end
end

system reap
  each Entity do |e|
    e.despawn
  end
end
",
        &[
            "upload    Position  reap -> move     on despawn",
            "upload    Velocity  reap -> move     on despawn",
            "download  Position  move -> report",
        ],
    );
}

#[test]
fn a_structural_change_before_a_reader_does_not_hide_its_download() {
    let text = "
component Position
  x: Float32
end

gpu system move
  each Position do |pos|
    pos.x += 1.0
  end
end

system reap
  each Entity do |e|
    e.despawn
  end
end

system report
  each Position do |pos|
    puts pos.x
  end
end
";
    assert_eq!(
        plan_of(text),
        "\
upload    Position  reap -> move     on despawn
download  Position  move -> reap     on despawn
download  Position  move -> report
"
    );
}

#[test]
fn a_component_of_the_nested_each_only_is_uploaded_like_any_read() {
    assert_plan(
        "
component Attractor
  x: Float32
  pull: Float32
end

gpu system attract
  each Velocity, Position do |vel, pos|
    each Attractor do |a|
      vel.dx += (a.x - pos.x) * a.pull
    end
  end
end

system drift
  each Attractor do |a|
    a.x += 1.0
  end
end
",
        &["upload    Attractor  drift -> attract"],
    );
}

#[test]
fn a_nested_each_over_a_component_the_block_writes_is_no_transfer() {
    assert_plan(
        "
gpu system separate
  each Position do |a|
    each Position do |b|
      a.x += (a.x - b.x) * 0.01
    end
  end
end
",
        &[],
    );
}

#[test]
fn top_level_reads_in_the_tick_loop_download() {
    assert_plan(
        "
e = spawn Position(0.0, 0.0), Velocity(1.0, 2.0)
3.times do
  World.tick
  puts e[Position].x
end
",
        &["download  Position  move -> top level"],
    );
}

#[test]
fn top_level_code_before_the_tick_in_its_loop_counts() {
    assert_plan(
        "
e = spawn Position(0.0, 0.0), Velocity(1.0, 2.0)
frame = 0
while frame < 3
  e[Velocity].dx = 0.0
  World.tick
  frame += 1
end
",
        &["upload    Velocity  top level -> move"],
    );
}

#[test]
fn top_level_setup_before_the_loop_that_ticks_does_not_count() {
    assert_plan(
        "
def populate(n: Int)
  n.times do
    spawn Position(0.0, 0.0), Velocity(1.0, 2.0)
  end
end

populate(10)
each Position do |pos|
  pos.x = 5.0
end
first = spawn Position(1.0, 1.0)
puts first[Position].x
2.times do
  World.tick
end
",
        &[],
    );
}

#[test]
fn top_level_code_after_the_loop_that_ticks_does_not_count() {
    assert_plan(
        "
e = spawn Position(0.0, 0.0), Velocity(1.0, 2.0)
100.times do
  World.tick
end
each Position do |pos|
  puts pos.x
end
e[Position].x = 0.0
e.despawn
",
        &[],
    );
}

#[test]
fn top_level_code_between_ticks_outside_a_loop_does_not_count() {
    assert_plan(
        "
e = spawn Position(0.0, 0.0), Velocity(1.0, 2.0)
World.tick
puts e[Position].x
World.tick
puts e[Position].x
",
        &[],
    );
    assert_plan(
        "
spawn Position(0.0, 0.0), Velocity(1.0, 2.0)
World.run
each Position do |pos|
  puts pos.x
end
",
        &[],
    );
}

#[test]
fn every_loop_around_the_tick_counts_and_the_code_after_each_does_not() {
    assert_plan(
        "
e = spawn Position(0.0, 0.0), Velocity(1.0, 2.0)
3.times do
  10.times do
    World.tick
  end
  e[Velocity].dx = 0.0
end
puts e[Position].x
",
        &["upload    Velocity  top level -> move"],
    );
    assert_plan(
        "
e = spawn Position(0.0, 0.0), Velocity(1.0, 2.0)
3.times do
  World.run
  puts e[Position].x
end
",
        &["download  Position  move -> top level"],
    );
}

#[test]
fn top_level_structural_changes_between_ticks_are_marked() {
    assert_plan(
        "
5.times do
  World.tick
  spawn Position(0.0, 0.0), Velocity(1.0, 2.0)
end
",
        &[
            "upload    Position  top level -> move   on spawn",
            "upload    Velocity  top level -> move   on spawn",
            "download  Position  move -> top level   on spawn",
        ],
    );
}

#[test]
fn a_loop_in_a_called_function_counts_like_one_at_the_top_level() {
    assert_plan(
        "
def total_x -> Float
  sum = 0.0
  each Position do |pos|
    sum += pos.x
  end
  sum
end

def simulate(ticks: Int)
  puts total_x
  ticks.times do
    World.tick
  end
  puts total_x
end

simulate(10)
",
        &[],
    );
    assert_plan(
        "
def total_x -> Float
  sum = 0.0
  each Position do |pos|
    sum += pos.x
  end
  sum
end

def simulate(ticks: Int)
  ticks.times do
    World.tick
    puts total_x
  end
end

puts total_x
simulate(10)
puts total_x
",
        &["download  Position  move -> top level"],
    );
}

#[test]
fn a_loop_that_ticks_through_a_called_function_counts() {
    assert_plan(
        "
def total_x -> Float
  sum = 0.0
  each Position do |pos|
    sum += pos.x
  end
  sum
end

def advance
  World.tick
  puts total_x
end

puts total_x
4.times do
  advance
end
",
        &["download  Position  move -> top level"],
    );
    assert_plan(
        "
def total_x -> Float
  sum = 0.0
  each Position do |pos|
    sum += pos.x
  end
  sum
end

def warm_up
  puts total_x
  World.tick
end

warm_up
",
        &[],
    );
}

#[test]
fn a_top_level_reader_after_a_system_reader_adds_nothing() {
    assert_plan(
        "
system report
  each Position do |pos|
    puts pos.x
  end
end

e = spawn Position(0.0, 0.0), Velocity(1.0, 2.0)
3.times do
  World.tick
  puts e[Position].x
end
",
        &["download  Position  move -> report"],
    );
}

#[test]
fn components_without_a_gpu_copy_never_appear() {
    assert_plan(
        "
component Health
  hp: Int
end

system regenerate
  each Health do |h|
    h.hp += 1
  end
end

system reap
  each Entity, Health do |e, h|
    puts h.hp
  end
end
",
        &[],
    );
    let text = "
component Health
  hp: Int
end

system regenerate
  each Entity, Health do |e, h|
    h.hp += 1
    e.despawn if h.hp > 10
  end
end
";
    assert_eq!(plan_of(text), "");
}
