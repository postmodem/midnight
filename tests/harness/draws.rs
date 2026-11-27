//! The check `draws`: reading answers, comparing them by what they draw,
//! what a failed check records, and the section of the report and line of
//! compare that read it.

use crate::CAP;
use crate::draws::{self as d, Difference, Shape};
use crate::grade;
use crate::help::{Temp, fixture_json, script};
use crate::json::Value;
use crate::logs;
use crate::obj;
use crate::report;
use crate::runner;
use crate::task::Task;

const ANSWER: &str = "draws: 3 ticks, 320x200, 3 draws
  Window.background = Color(16, 16, 32, 255)
  Draw.rect(10.0, 20.0, 30.0, 6.0, Color(40, 40, 60, 255))
  Draw.circle(26.0, 34.5, 4.0, Color(255, 200, 0, 255))
  Draw.line(NaN, -0.0, 1e+30, Infinity, 2.0, Color(1, 2, 3, 128))
";

/// ANSWER with its numbers written otherwise: the same draws.
const RESPELT: &str = "draws: 3 ticks, 320x200, 3 draws
  Window.background = Color(16, 16, 32, 255)
  Draw.rect(10, 20.00, 30.0, 6.0, Color(40, 40, 60, 255))
  Draw.circle(26.0, 34.5, 4.0, Color(255, 200, 0, 255))
  Draw.line(NaN, -0.0, 1000000000000000019884624838656.0, Infinity, 2, Color(1, 2, 3, 128))
";

fn diff(got: &str, want: &str, any: bool) -> Option<Difference> {
    d::difference(&d::parse(got), &d::parse(want), any)
}

fn differs(how: &str, got: Option<&str>, want: Option<&str>, answer: usize) -> Option<Difference> {
    Some(Difference {
        answer: Some(answer),
        how: how.to_string(),
        got: got.map(str::to_string),
        want: want.map(str::to_string),
    })
}

fn lines() -> Vec<&'static str> {
    ANSWER.split_inclusive('\n').collect()
}

fn said(text: &str) -> String {
    text.split_inclusive('\n')
        .map(|l| format!("midnight: drive: {l}"))
        .collect()
}

#[test]
fn answers_are_taken_from_standard_error() {
    let err = format!(
        "midnight: drive: tick 3\n{}runtime error\nmidnight: drive:   not an answer's line\n\
         midnight: drive: draws: 4 ticks, 1x1, 0 draws\nmidnight: drive:   Window.background = Color(0, 0, 0, 0)\n",
        said(ANSWER)
    );
    assert_eq!(
        d::answers_text(&err),
        format!("{ANSWER}draws: 4 ticks, 1x1, 0 draws\n  Window.background = Color(0, 0, 0, 0)\n")
    );
}

#[test]
fn the_same_draws_written_otherwise() {
    let a = &d::parse(ANSWER)[0];
    assert_eq!(a.frame, (3, "320x200".to_string(), Some([16, 16, 32, 255])));
    assert_eq!(
        a.shapes[0],
        Shape::Draw {
            kind: "rect".to_string(),
            numbers: [10.0f32, 20.0, 30.0, 6.0]
                .iter()
                .map(|x| x.to_bits())
                .collect(),
            color: [40, 40, 60, 255],
        }
    );
    let Shape::Draw { numbers, .. } = &a.shapes[2] else {
        panic!("a draw")
    };
    assert_eq!(
        numbers,
        &vec![
            0x7fc0_0000,
            0x8000_0000,
            d::bits("1e+30").expect("a number"),
            0x7f80_0000,
            0x4000_0000
        ]
    );
    assert_eq!(diff(RESPELT, ANSWER, false), None);
    assert_eq!(diff(ANSWER, RESPELT, false), None);
    assert!(matches!(
        d::shape("Draw.rect(1.0, 2.0, Color(0, 0, 0, 255))"),
        Shape::Text(_)
    ));
    assert!(matches!(
        d::shape("Draw.oval(1.0, 2.0, 3.0, Color(0, 0, 0, 255))"),
        Shape::Text(_)
    ));
}

#[test]
fn in_order() {
    let l = lines();
    let mut swapped = l.clone();
    swapped.swap(2, 3);
    assert_eq!(
        diff(&swapped.concat(), ANSWER, false),
        differs("order", Some(l[3].trim()), Some(l[2].trim()), 1)
    );
    let missing = [&l[0..3], &l[4..]].concat().concat();
    assert_eq!(
        diff(&missing, ANSWER, false),
        differs("missing", None, Some(l[3].trim()), 1)
    );
    let extra = "  Draw.rect(0.0, 0.0, 0.0, 5.0, Color(1, 1, 1, 255))\n";
    assert_eq!(
        diff(&format!("{ANSWER}{extra}"), ANSWER, false),
        differs("extra", Some(extra.trim()), None, 1)
    );
    let how = |got: &str| diff(got, ANSWER, false).map(|x| x.how);
    assert_eq!(
        how(&ANSWER.replace("26.0, 34.5", "26.0, 34.0")).as_deref(),
        Some("differs")
    );
    assert_eq!(
        how(&ANSWER.replace("3 ticks", "4 ticks")).as_deref(),
        Some("frame")
    );
    assert_eq!(
        how(&ANSWER.replace("Color(16, 16, 32, 255)", "Color(16, 16, 32, 0)")).as_deref(),
        Some("frame")
    );
    assert_eq!(
        how(&ANSWER.replace("3 draws", "5 draws")),
        None,
        "the count is the shapes'"
    );
    let two = format!("{ANSWER}{}", ANSWER.replace("3 ticks", "6 ticks"));
    assert_eq!(
        diff(ANSWER, &two, false),
        differs("answers", None, Some("draws: 6 ticks, 320x200, 3 draws"), 2)
    );
    let second = format!(
        "{ANSWER}{}",
        ANSWER
            .replace("3 ticks", "6 ticks")
            .replace("Color(40, 40, 60, 255)", "Color(40, 60, 40, 255)")
    );
    assert_eq!(
        diff(&second, &two, false),
        differs(
            "differs",
            Some("Draw.rect(10.0, 20.0, 30.0, 6.0, Color(40, 60, 40, 255))"),
            Some("Draw.rect(10.0, 20.0, 30.0, 6.0, Color(40, 40, 60, 255))"),
            2
        )
    );
}

#[test]
fn any_order() {
    let l = lines();
    let reordered = [l[0], l[1], l[4], l[2], l[3]].concat();
    assert_eq!(diff(&reordered, ANSWER, true), None);
    assert!(diff(&reordered, ANSWER, false).is_some());
    let twice = format!("{ANSWER}{}", l[2]);
    assert_eq!(
        diff(&twice, ANSWER, true).map(|x| x.how).as_deref(),
        Some("extra")
    );
    assert_eq!(
        diff(ANSWER, &twice, true).map(|x| x.how).as_deref(),
        Some("missing")
    );
    let near = ANSWER.replace("Draw.circle(26.0, 34.5", "Draw.circle(34.5, 26.0");
    let got = diff(&near, ANSWER, true);
    assert_eq!(
        got,
        differs(
            "differs",
            Some("Draw.circle(34.5, 26.0, 4.0, Color(255, 200, 0, 255))"),
            Some(l[3].trim()),
            1
        )
    );
    let got = got.expect("a difference");
    assert!(d::swapped(got.got.as_deref(), got.want.as_deref()));
    assert_eq!(
        diff(&ANSWER.replace("320x200", "200x320"), ANSWER, true)
            .map(|x| x.how)
            .as_deref(),
        Some("frame")
    );
}

#[test]
fn swapped_and_by_center() {
    let want = Some("Draw.rect(10.0, 20.0, 30.0, 6.0, Color(40, 40, 60, 255))");
    assert!(d::swapped(
        Some("Draw.rect(20.0, 10.0, 30.0, 6.0, Color(40, 40, 60, 255))"),
        want
    ));
    assert!(d::swapped(
        Some("Draw.rect(10.0, 20.0, 6.0, 30.0, Color(40, 40, 60, 255))"),
        want
    ));
    assert!(!d::swapped(
        Some("Draw.rect(20.0, 10.0, 6.0, 30.0, Color(40, 40, 60, 255))"),
        want
    ));
    assert!(!d::swapped(
        Some("Draw.rect(20.0, 10.0, 30.0, 6.0, Color(40, 40, 61, 255))"),
        want
    ));
    assert!(!d::swapped(want, want));
    assert!(!d::swapped(None, want));
    assert!(d::by_center(
        Some("Draw.rect(25.0, 23.0, 30.0, 6.0, Color(0, 0, 0, 255))"),
        want
    ));
    assert!(!d::by_center(
        Some("Draw.rect(-5.0, 17.0, 30.0, 6.0, Color(40, 40, 60, 255))"),
        want
    ));
    assert!(!d::by_center(
        Some("Draw.circle(25.0, 23.0, 30.0, Color(40, 40, 60, 255))"),
        want
    ));
}

#[test]
fn a_draws_answer_is_one_record() {
    let err = format!(
        "midnight: drive: tick 3\n{}midnight: drive: try.drive:4: can't list the draws: no window is open\n",
        said(ANSWER)
    );
    let messages: Vec<String> = logs::parse_stderr(&err)
        .iter()
        .map(|x| x.get("message").to_text())
        .collect();
    assert_eq!(
        messages,
        [
            "drive: tick 3",
            "drive: draws",
            "drive: try.drive:4: can't list the draws: no window is open"
        ]
    );
}

/// A task in a fresh directory, and a stub compiler whose `build` makes a
/// program that prints what solution.12am holds: its standard output
/// before a line `---`, its standard error after.
struct Fixture {
    dir: Temp,
    stub: String,
}

impl Fixture {
    fn new(any: bool) -> Fixture {
        let dir = Temp::new();
        let order = if any { "any" } else { "in" };
        dir.write(
            "task/meta",
            &format!("areas: draw window\ndraws: {order} order\n"),
        );
        dir.write("task/expected.out", "tick 3\n");
        dir.write("task/expected.draws", ANSWER);
        dir.write("task/check.drive", "tick 3\ndraws\n");
        let stub = dir.path("stub");
        let sh = crate::help::sh();
        script(
            &stub,
            &format!(
                "[ \"$1\" = build ] || exit 0
case \"$(cat solution.12am)\" in *FAIL*) exit 1 ;; esac
csplit -s -f part solution.12am '/^---$/'
cat part00 > solution.out
tail -n +2 part01 > solution.err
printf '#!{sh}\\ncat solution.out\\ncat solution.err >&2\\n' > solution
chmod 755 solution
"
            ),
        );
        Fixture { dir, stub }
    }

    fn task(&self) -> Task {
        Task::new(&self.dir.path("task")).expect("a task")
    }

    fn program(&self, err: &str, out: &str) -> String {
        self.dir.write("prog.12am", &format!("{out}---\n{err}"))
    }
}

#[test]
fn the_check_grades_and_keeps_the_answers() {
    let f = Fixture::new(false);
    let task = f.task();
    let keep = f.dir.path("keep");
    let g = grade::grade(
        &task,
        &f.program(&said(RESPELT), "tick 3\n"),
        &f.stub,
        Some(&keep),
    )
    .expect("a grade");
    assert_eq!(g.get("verdict").as_str(), Some("pass"));
    assert_eq!(
        g.get("checks"),
        &Value::Obj(obj! { "build" => true, "output" => true, "draws" => true })
    );
    assert_eq!(
        crate::proc::read(&format!("{keep}/graded.draws")).expect("kept"),
        RESPELT
    );

    let g = grade::grade(
        &task,
        &f.program(
            &said(&ANSWER.replace("26.0, 34.5", "34.5, 26.0")),
            "tick 3\n",
        ),
        &f.stub,
        Some(&keep),
    )
    .expect("a grade");
    assert_eq!(g.get("verdict").as_str(), Some("fail"));
    assert_eq!(
        g.get("checks"),
        &Value::Obj(obj! { "build" => true, "output" => true, "draws" => false })
    );
    assert_eq!(g.get("draws").get("how").as_str(), Some("differs"));
    assert!(
        g.get("notes")
            .get("draws")
            .to_text()
            .starts_with("answer 1: differs, got `Draw.circle(34.5, 26.0")
    );
    let mut rec = crate::json::Map::new();
    runner::apply_grade(&mut rec, &g, false, false);
    assert_eq!(rec.get("failed_checks"), &Value::from(vec!["draws"]));
    assert_eq!(rec.get("draws").get("answer").as_i64(), Some(1));

    let flood = "y".repeat(CAP + 10);
    let g = grade::grade(&task, &f.program(&flood, "tick 3\n"), &f.stub, None).expect("a grade");
    assert_eq!(g.get("draws").get("how").as_str(), Some("cut"));

    let g =
        grade::grade(&task, &f.program(&said(ANSWER), "tick 4\n"), &f.stub, None).expect("a grade");
    assert_eq!(
        g.get("checks"),
        &Value::Obj(obj! { "build" => true, "output" => false, "draws" => true })
    );
}

#[test]
fn self_test_wants_a_wrong_program_that_fails_only_draws() {
    let f = Fixture::new(false);
    let w = |rel: &str, text: &str| {
        f.dir.write(&format!("task/{rel}"), text);
    };
    w("TASK.md", "Draw.\n\n```\ntick 1\n```\n");
    w("expected.out", "tick 1\ntick 3\n");
    let good = format!("tick 1\ntick 3\n---\n{}", said(ANSWER));
    w("solution.12am", &good);
    w(
        "right/respelt.12am",
        &format!("tick 1\ntick 3\n---\n{}", said(RESPELT)),
    );
    w("wrong/build.12am", "FAIL\n");
    let root = f.dir.0.clone();
    let (_, problems) = grade::self_test(&root, &f.stub, &[]).expect("a self-test");
    assert_eq!(problems, ["task: no program in wrong/ fails only `draws`"]);
    w(
        "wrong/swapped.12am",
        &good.replace("26.0, 34.5", "34.5, 26.0"),
    );
    let (_, problems) = grade::self_test(&root, &f.stub, &[]).expect("a self-test");
    assert!(problems.is_empty(), "{problems:?}");
    let meta = f.task().read(&["meta"]).expect("meta");
    w("meta", &meta.replace("draws: in order\n", ""));
    let (_, problems) = grade::self_test(&root, &f.stub, &[]).expect("a self-test");
    assert_eq!(
        problems,
        ["task: expected.draws and `draws: in order` or `draws: any order` in meta go together"]
    );
}

#[test]
fn expected_draws_counts_in_a_tasks_hash() {
    let f = Fixture::new(false);
    let task = f.task();
    let before = task.sha256().expect("a hash");
    f.dir
        .write("task/expected.draws", &ANSWER.replace("3 ticks", "4 ticks"));
    assert_ne!(before, task.sha256().expect("a hash"));
}

#[test]
fn compare_pools_every_row() {
    let text = report::render_compare(
        &fixture_json("results-a.json"),
        &fixture_json("results-b.json"),
    );
    assert!(text.contains("Over all rows: A passed 6 of 6 graded runs (100.0%), B 5 of 6 (83.3%); Fisher's exact test, two-sided: p = 1.000."));
    assert_eq!(
        report::pooled_line(3, 10, 9, 10),
        "Over all rows: A passed 3 of 10 graded runs (30.0%), B 9 of 10 (90.0%); Fisher's exact test, two-sided: p = 0.020."
    );
}

#[test]
fn report_lists_how_draws_failed() {
    let mut a = fixture_json("results-a.json");
    let want = "Draw.rect(10.0, 20.0, 30.0, 6.0, Color(40, 40, 60, 255))";
    let added = [
        (
            Some("Draw.rect(20.0, 10.0, 30.0, 6.0, Color(40, 40, 60, 255))"),
            "differs",
        ),
        (
            Some("Draw.rect(25.0, 23.0, 30.0, 6.0, Color(40, 40, 60, 255))"),
            "differs",
        ),
        (None, "missing"),
    ];
    let m = a.as_obj_mut().expect("an object");
    let mut runs = m.get("runs").items().to_vec();
    for (i, (got, how)) in added.iter().enumerate() {
        runs.push(Value::Obj(obj! {
            "task" => "poster",
            "model" => "haiku",
            "trial" => i + 1,
            "attempt" => 1,
            "counts" => true,
            "outcome" => "fail",
            "checks" => obj! { "build" => true, "output" => true, "draws" => false },
            "failed_checks" => vec!["draws"],
            "draws" => obj! { "answer" => 1, "how" => *how, "got" => got.map(str::to_string), "want" => want },
        }));
    }
    m.set("runs", runs);
    let mut names = m.get("task_names").items().to_vec();
    names.push(Value::from("poster"));
    m.set("task_names", names);
    let text = report::render(&a);
    assert!(text.contains("## Draws"));
    assert!(text.contains("| `poster` | `haiku` | 3 | 3 | 0 | 0 | 1 | 0 | 0 | 2 | 0 |"));
    assert!(text.contains("- Two numbers exchanged: poster/haiku/1.1\n"));
    assert!(text.contains("- A rectangle placed by its center: poster/haiku/2.1\n"));
    assert!(!report::render(&fixture_json("results-a.json")).contains("## Draws"));
}
