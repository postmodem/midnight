//! What was read, Fisher's test, report and compare.

use crate::audit;
use crate::help::fixture_json;
use crate::json::{Map, Value};
use crate::obj;
use crate::report;

const ROOT: &str = "/tmp/eval.Root0000";

fn doc() -> String {
    (1..=40)
        .map(|n| match n {
            10 => "## Lists\n".to_string(),
            30 => "### Sorting\n".to_string(),
            _ => format!("line {n} of the reference\n"),
        })
        .collect()
}

const EXAMPLE: &str = "puts \"hello, world\"\nputs \"and goodbye\"\n";

fn docs() -> Vec<(String, String)> {
    vec![
        ("language.md".to_string(), doc()),
        ("examples/hello.12am".to_string(), EXAMPLE.to_string()),
    ]
}

fn call(tool: &str, input: Map, result: &str, denied: bool) -> Map {
    obj! { "tool" => tool, "input" => input, "result" => result, "denied" => denied }
}

fn read(calls: &[Map]) -> Vec<(String, Vec<usize>)> {
    audit::lines_read(calls, ROOT, &docs())
}

fn one(dest: &str, lines: &[usize]) -> Vec<(String, Vec<usize>)> {
    vec![(dest.to_string(), lines.to_vec())]
}

fn bash(command: &str, result: &str) -> Map {
    call("Bash", obj! { "command" => command }, result, false)
}

#[test]
fn read_tool_ranges() {
    let shown: Vec<String> = (11..=13)
        .map(|n| format!("{n:6}\tline {n} of the reference"))
        .collect();
    let c = call(
        "Read",
        obj! { "file_path" => format!("{ROOT}/docs/language.md") },
        &shown.join("\n"),
        false,
    );
    assert_eq!(read(&[c]), one("language.md", &[11, 12, 13]));
}

#[test]
fn shell_output_located_by_text() {
    let out = "line 5 of the reference\nline 6 of the reference\n";
    assert_eq!(
        read(&[bash("sed -n '5,6p' docs/language.md", out)]),
        one("language.md", &[5, 6])
    );
    assert_eq!(
        read(&[bash(
            "grep -n '^##' docs/language.md",
            "10:## Lists\n30:### Sorting"
        )]),
        one("language.md", &[10, 30])
    );
    assert_eq!(
        read(&[bash("cat docs/examples/*.12am", EXAMPLE)]),
        one("examples/hello.12am", &[1, 2])
    );
}

#[test]
fn grep_tool_in_content_mode_only() {
    let input = obj! {
        "pattern" => "Lists",
        "path" => "docs",
        "output_mode" => "content",
        "-n" => true,
    };
    let c = call(
        "Grep",
        input.clone(),
        &format!("{ROOT}/docs/language.md:10:## Lists"),
        false,
    );
    assert_eq!(read(&[c]), one("language.md", &[10]));
    let mut files = input;
    files.set("output_mode", "files_with_matches");
    let c = call("Grep", files, &format!("{ROOT}/docs/language.md"), false);
    assert!(read(&[c]).is_empty());
}

#[test]
fn errors_previews_and_denials_count_only_what_they_show() {
    let preview = "Output too large (144.6KB). Full output saved to: /x/tool-results/a.txt\n\n\
                   Preview (first 2KB):\nline 1 of the reference\nline 2 of the reference\n...";
    assert_eq!(
        read(&[bash("cat docs/language.md", preview)]),
        one("language.md", &[1, 2])
    );
    let too_big = "File content (40000 tokens) exceeds maximum allowed tokens (25000).";
    let c = call(
        "Read",
        obj! { "file_path" => format!("{ROOT}/docs/language.md") },
        too_big,
        false,
    );
    assert!(read(&[c]).is_empty());
    let denied = call(
        "Bash",
        obj! { "command" => "cat docs/language.md" },
        &doc(),
        true,
    );
    assert!(read(&[denied]).is_empty());
    assert!(read(&[bash("midnight run docs/examples/hello.12am", EXAMPLE)]).is_empty());
}

#[test]
fn the_shells_directory_carries_over() {
    let out = "line 5 of the reference\nline 6 of the reference\n";
    assert_eq!(
        read(&[
            bash("cd docs && grep -n '^#' language.md", "10:## Lists"),
            bash("sed -n 5,6p language.md", out),
        ]),
        one("language.md", &[5, 6, 10])
    );
    let reset = format!("10:## Lists\nShell cwd was reset to {ROOT}");
    assert_eq!(
        read(&[
            bash("cd docs && grep -n '^#' language.md", &reset),
            bash("sed -n 5,6p language.md", out),
        ]),
        one("language.md", &[10])
    );
    assert!(read(&[bash("cat ~nobody/language.md ~+/x", out)]).is_empty());
}

fn near(a: f64, b: f64, delta: f64) -> bool {
    (a - b).abs() <= delta
}

#[test]
fn fisher() {
    assert!(near(report::fisher(5, 0, 0, 5), 2.0 / 252.0, 1e-12));
    assert!(near(report::fisher(3, 2, 2, 3), 1.0, 1e-12));
    assert!(near(report::fisher(1, 9, 11, 3), 0.002759, 1e-6));
    assert!(near(report::fisher(5, 0, 5, 0), 1.0, 1e-12));
}

#[test]
fn chance_of_a_mark() {
    assert!(near(report::chance_of_mark(1, 1, 0, 1), 0.0, 1e-12));
    assert!(near(report::chance_of_mark(5, 5, 5, 5), 0.0, 1e-12));
    let half = report::chance_of_mark(3, 5, 2, 5);
    assert!(half > 0.0);
    assert!(half < 0.1);
}

#[test]
fn code_spans() {
    assert_eq!(report::code("plain"), "`plain`");
    assert_eq!(report::code("unknown `…`"), "`` unknown `…` ``");
}

fn results(name: &str) -> Value {
    fixture_json(name)
}

#[test]
fn the_report() {
    let text = report::render(&results("results-a.json"));
    for h in [
        "Pass rates",
        "Tasks",
        "Diagnostics",
        "The reference as read",
        "Exceptions",
    ] {
        assert!(text.contains(&format!("## {h}")), "{h}");
    }
    assert!(text.contains("| `league` | `haiku` | 1/1 | yes |"));
    assert!(text.contains("| `sonnet` | 100.0% ± 0.0 | 3 of 3 |"));
    assert!(
        text.contains(
            "`` `…` is a keyword and can't be used as a name `` | 1 | 1 | 1 | 0 | on ×1 |"
        )
    );
    assert!(text.contains("| `language.md` | Lists | 3 |"));
    assert!(text.contains("Denied tool calls: 6 in 5 runs"));

    let b = report::render(&results("results-b.json"));
    assert!(b.contains("| `league` | `haiku` | 0/1 | no |"));
    assert!(b.contains(
        "| error | `` unknown method `…` for `…` `` | 1 | 1 | 0 | 0 | insert / [Row] ×1 |"
    ));
}

#[test]
fn compare() {
    let text = report::render_compare(&results("results-a.json"), &results("results-b.json"));
    assert!(text.contains("| `haiku` | 100.0% | 66.7% | -33.3% ± 33.3 |"));
    assert!(text.contains("| `league` | `haiku` | 1/1 | 0/1 | 1.000 |  |"));
    assert!(text.contains("Kit: A `evals/kit`, B `evals/test/kit-cut`; changed: language.md"));
    assert!(text.contains("0 marked (p < 0.1). Chance alone, with each row passing at its pooled rate in both, would mark about 0.0 of 6 rows"));
    assert!(text.contains("| error | `` unknown method `…` for `…` `` | 0 | 1 |"));
    assert!(
        text.contains("| error | `` `…` is a keyword and can't be used as a name `` | 1 | 0 |")
    );
    assert!(!text.contains("preamble differs"));
    let mut b = results("results-b.json");
    b.as_obj_mut()
        .expect("an object")
        .set("preamble_sha256", "other");
    assert!(
        report::render_compare(&results("results-a.json"), &b).contains("The preamble differs")
    );
}

#[test]
fn compare_counts_templates_of_the_tasks_in_both() {
    let a = results("results-a.json");
    let mut b = results("results-b.json");
    let m = b.as_obj_mut().expect("an object");
    let runs: Vec<Value> = m
        .get("runs")
        .items()
        .iter()
        .filter(|r| r.get("task").as_str() != Some("thermostat"))
        .cloned()
        .collect();
    m.set("runs", runs);
    if let Some(Value::Obj(tasks)) = m.get_mut("tasks") {
        tasks.remove("thermostat");
    }
    let names: Vec<Value> = m
        .get("task_names")
        .items()
        .iter()
        .filter(|t| t.as_str() != Some("thermostat"))
        .cloned()
        .collect();
    m.set("task_names", names);
    let text = report::render_compare(&a, &b);
    assert!(!text.contains("is a keyword and can't be used as a name"));
    assert!(text.contains("| error | `` unknown method `…` for `…` `` | 0 | 1 |"));
}
