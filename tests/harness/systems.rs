//! The order the `systems` check requires: the named systems in the order
//! they run, and with `others after`, before any other.

use crate::grade;
use crate::help::Temp;
use crate::task::Task;

fn task(dir: &Temp, systems: &str) -> Task {
    dir.write("task/meta", "areas: draw\n");
    dir.write("task/systems", systems);
    Task::new(&dir.path("task")).expect("a task")
}

fn order_note(dir: &Temp, wanted: &[&str], others_after: bool) -> Option<String> {
    let wanted: Vec<(String, bool)> = wanted.iter().map(|n| (n.to_string(), false)).collect();
    grade::systems_out_of_order(&dir.0, &wanted, others_after)
}

#[test]
fn the_marker() {
    let d = Temp::new();
    let t = task(&d, "adjust\nreport each\nothers after\n");
    assert_eq!(
        t.systems().expect("systems"),
        [("adjust".to_string(), false), ("report".to_string(), true)]
    );
    assert!(t.others_after().expect("systems"));
    assert!(
        !task(&d, "adjust\nreport\n")
            .others_after()
            .expect("systems")
    );
}

#[test]
fn order_follows_requires() {
    let d = Temp::new();
    d.write(
        "solution.12am",
        "system a\nend\nrequire \"lib/more\"\nsystem d\nend\nrequire 'lib/more'\n",
    );
    d.write("lib/more.12am", "gpu system b\nend\nrequire \"deep\"\n");
    d.write("lib/deep.12am", "strict gpu system c\nend\n");
    d.write("unused.12am", "system z\nend\n");
    assert_eq!(grade::system_order(&d.0), ["a", "b", "c", "d"]);
}

#[test]
fn named_systems_in_their_order() {
    let d = Temp::new();
    d.write(
        "solution.12am",
        "system move\nend\nsystem launch\nend\nsystem report\nend\n",
    );
    assert_eq!(order_note(&d, &["move", "report"], false), None);
    assert_eq!(
        order_note(&d, &["launch", "move"], false).as_deref(),
        Some("runs `move`, `launch` in that order")
    );
}

#[test]
fn others_after_the_named_ones() {
    let d = Temp::new();
    d.write(
        "solution.12am",
        "system adjust\nend\nsystem show\nend\nsystem report\nend\nsystem extra\nend\n",
    );
    assert_eq!(order_note(&d, &["adjust", "report"], false), None);
    assert_eq!(
        order_note(&d, &["adjust", "report"], true).as_deref(),
        Some("`show` runs before `report`")
    );
    assert_eq!(order_note(&d, &["adjust", "show", "report"], true), None);
}

#[test]
fn a_missing_system_is_left_to_the_other_rule() {
    let d = Temp::new();
    d.write("solution.12am", "system report\nend\n");
    assert_eq!(order_note(&d, &["adjust", "report"], true), None);
}
