//! Kits, tasks, the compiler's standard error, call logs and transcripts.

use crate::audit::{self, Places};
use crate::grade;
use crate::guard;
use crate::help::{Temp, fixture};
use crate::json::{self, Value};
use crate::logs::{self, Transcript};
use crate::obj;
use crate::runner;
use crate::task::{self, Task};
use crate::text;

fn pairs(v: &[(&str, &str)]) -> Vec<(String, String)> {
    v.iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect()
}

#[test]
fn kit_lines() {
    let entries = task::kit_parse("# docs\n\ndocs/language.md language.md\nexamples  examples\n")
        .expect("a kit");
    assert_eq!(
        entries,
        pairs(&[
            ("docs/language.md", "language.md"),
            ("examples", "examples")
        ])
    );
    assert!(task::kit_parse("a ../b\n").is_err());
    assert!(task::kit_parse("a /b\n").is_err());
    assert!(task::kit_parse("a\n").is_err());
}

#[test]
fn kit_files_expand_directories() {
    let tree: Vec<String> = [
        "README.md",
        "docs/language.md",
        "examples/a.12am",
        "examples/a.out",
        "examples/sub/b.12am",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let files = task::kit_files(
        &pairs(&[("docs/language.md", "language.md"), ("examples", "ex")]),
        &tree,
    )
    .expect("files");
    assert_eq!(
        files,
        pairs(&[
            ("language.md", "docs/language.md"),
            ("ex/a.12am", "examples/a.12am"),
            ("ex/a.out", "examples/a.out"),
            ("ex/sub/b.12am", "examples/sub/b.12am"),
        ])
    );
    assert!(task::kit_files(&pairs(&[("nothing", "x")]), &tree).is_err());
}

#[test]
fn sections() {
    let text =
        "# Title\n\nintro\n\n## One\n\n```\n## not a heading\n```\n\n### Two\n\n#### Deeper\n";
    let s = |n: usize, t: &str| (n, Some(t.to_string()));
    assert_eq!(
        task::sections(text),
        [s(1, task::BEFORE), s(5, "One"), s(11, "Two")]
    );
    assert_eq!(task::sections("# Title\n\n## One\n"), [s(3, "One")]);
    assert_eq!(task::sections("## One\ntext\n"), [s(1, "One")]);
    assert_eq!(task::sections("no headings\n"), [(1, None)]);
}

#[test]
fn meta() {
    let meta = Task::parse_meta("areas: ecs strings\nminutes:\n").expect("meta");
    assert_eq!(meta, pairs(&[("areas", "ecs strings"), ("minutes", "")]));
    assert!(Task::parse_meta("no colon\n").is_err());
}

#[test]
fn task_loops() {
    let d = Temp::new();
    d.write("meta", "areas: cores\n");
    d.write(
        "loops",
        "system walk cores\nstrict gpu system settle gpu\nsystem count one core\n",
    );
    let task = Task::new(&d.0).expect("a task");
    assert_eq!(
        task.loops().expect("loops"),
        pairs(&[
            ("system walk", "cores"),
            ("strict gpu system settle", "gpu"),
            ("system count", "one core"),
        ])
    );
    assert_eq!(task.minutes(), 15);
}

fn wanted(v: &[(&str, bool)]) -> Vec<(String, bool)> {
    v.iter().map(|(n, e)| (n.to_string(), *e)).collect()
}

#[test]
fn systems_check() {
    let d = Temp::new();
    d.write("meta", "areas: ecs\n");
    d.write("systems", "hunt each\nreport\n");
    assert_eq!(
        Task::new(&d.0).expect("a task").systems().expect("systems"),
        wanted(&[("hunt", true), ("report", false)])
    );
    d.write("systems", "hunt all\n");
    assert!(Task::new(&d.0).expect("a task").systems().is_err());

    let prog = d.path("prog");
    d.write(
        "prog/solution.12am",
        "require \"lib/more\"\nsystem hunt\nend\n# system report\n",
    );
    d.write("prog/lib/more.12am", "  gpu system settle\nend\n");
    let loops = "solution.12am:3  system hunt  each Fox  one core: prints at line 4\n";
    let w = wanted(&[("hunt", true), ("settle", false), ("report", false)]);
    assert_eq!(grade::missing_systems(&prog, Some(loops), &w), ["report"]);
    assert_eq!(
        grade::missing_systems(&prog, None, &w),
        ["hunt each", "report"]
    );

    d.write(
        "prog/solution.12am",
        "system hunt\n  chase(1)\nend\n\nsystem report\n  puts 1\nend\n",
    );
    let helper = "solution.12am:9  def chase  each Fox  cores\n";
    assert!(
        grade::missing_systems(
            &prog,
            Some(helper),
            &wanted(&[("hunt", true), ("report", false)])
        )
        .is_empty()
    );
    assert_eq!(
        grade::missing_systems(&prog, Some(helper), &wanted(&[("report", true)])),
        ["report each"]
    );
}

#[test]
fn the_program_is_its_files_outside_docs() {
    let d = Temp::new();
    let keep = Temp::new();
    d.write("solution.12am", "system tally\nend\n");
    d.write("lib/a.12am", "");
    d.write("docs/examples/report.12am", "system report\nend\n");
    assert_eq!(guard::program_files(&d.0), ["lib/a.12am", "solution.12am"]);
    runner::keep_program(&d.0, &keep.0).expect("kept");
    assert_eq!(
        guard::program_files(&keep.0),
        ["lib/a.12am", "solution.12am"]
    );
    assert_eq!(
        grade::missing_systems(&keep.0, None, &wanted(&[("report", false)])),
        ["report"]
    );
    let sha = guard::program_sha256(&d.0);
    assert!(sha.is_some());
    assert_eq!(sha, guard::program_sha256(&keep.0));
    d.write("docs/examples/report.12am", "changed");
    assert_eq!(sha, guard::program_sha256(&d.0));
    d.write("lib/a.12am", "changed");
    assert_ne!(sha, guard::program_sha256(&d.0));
}

#[test]
fn empty_trees_are_removed_and_others_kept() {
    let d = Temp::new();
    crate::proc::mkdir_p(&d.path("a/session/started")).expect("dirs");
    runner::remove_empty_tree(&d.path("a"));
    assert!(!crate::proc::exists(&d.path("a")));
    d.write("b/session/file", "");
    runner::remove_empty_tree(&d.path("b"));
    assert!(crate::proc::exists(&d.path("b/session/file")));
}

#[test]
fn output_is_compared_as_bytes() {
    let d = Temp::new();
    d.write("meta", "areas: x\n");
    d.write("expected.out", "tick 1: 20.5 °C → on\n");
    let task = Task::new(&d.0).expect("a task");
    assert_eq!(
        task.expected_bytes().expect("bytes"),
        "tick 1: 20.5 °C → on\n".as_bytes()
    );
    assert_eq!(
        grade::first_difference("tick 1: 20.5 °C\n".as_bytes(), b"tick 1: 20.5 C\n"),
        "line 1: got \"tick 1: 20.5 °C\", want \"tick 1: 20.5 C\""
    );
    assert_eq!(
        grade::first_difference(b"a\nb\t\xff\n", b"a\n"),
        "line 2: got \"b\\t\u{fffd}\", want \"\""
    );
}

#[test]
fn loops_check() {
    let out = "s.12am:4  system walk   each Pos  cores\n\
               s.12am:9  system walk   each Vel  one core: assigns `best` at line 10\n\
               s.12am:12  system count  each Pos  one core: prints at line 13\n";
    assert!(grade::loops_ok(
        out,
        &pairs(&[("system count", "one core")])
    ));
    assert!(!grade::loops_ok(out, &pairs(&[("system walk", "cores")])));
    assert!(!grade::loops_ok(
        out,
        &pairs(&[("system missing", "cores")])
    ));
}

#[test]
fn diagnostics() {
    let err = "error: unknown method `insert` for `[Row]`
  --> solution.12am:80:12
   |
80 |     sorted.insert(i, r)
   |            ^^^^^^
error: component `Health` has fields without defaults
  --> lib/a.12am:10:7
solution.12am:9: runtime error: 999 does not fit in UInt8
midnight: MIDNIGHT_GPU_FLAGS is not set; GPU systems will run on the CPU
note: first defined here
";
    let d = logs::parse_stderr(err);
    let kinds: Vec<String> = d.iter().map(|x| x.get("kind").to_text()).collect();
    assert_eq!(kinds, ["error", "error", "runtime", "midnight"]);
    assert_eq!(d[0].get("template").to_text(), "unknown method `…` for `…`");
    assert_eq!(d[0].get("spans"), &Value::from(vec!["insert", "[Row]"]));
    assert_eq!(d[0].get("file").as_str(), Some("solution.12am"));
    assert_eq!(d[0].get("line").as_i64(), Some(80));
    assert_eq!(d[1].get("file").as_str(), Some("lib/a.12am"));
    assert_eq!(d[1].get("line").as_i64(), Some(10));
    assert_eq!(d[2].get("template").to_text(), "999 does not fit in UInt8");
    assert_eq!(d[2].get("file").as_str(), Some("solution.12am"));
    assert_eq!(d[2].get("line").as_i64(), Some(9));
    assert_eq!(
        d[3].get("template").to_text(),
        "MIDNIGHT_GPU_FLAGS is not set; GPU systems will run on the CPU"
    );
    assert!(logs::compiler_bug(
        "midnight: the C compiler rejected the generated code. This is a bug in midnight.\n"
    ));
}

fn log_call(
    args: &[&str],
    status: i64,
    stderr: &str,
    sha: &str,
    refused: Option<&str>,
    timeout: bool,
) -> Value {
    Value::Obj(obj! {
        "args" => args.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
        "status" => status,
        "stderr" => stderr,
        "sha256" => sha,
        "file" => args.get(1).map(|s| s.to_string()),
        "refused" => refused.map(str::to_string),
        "timeout" => timeout,
    })
}

#[test]
fn call_log_summary_and_repeats() {
    let e1 = "error: unknown function `puts_all`\n  --> solution.12am:3:1\n";
    let e2 = "error: unknown function `puts_all`\n  --> solution.12am:4:1\n";
    let calls = vec![
        log_call(&["check", "solution.12am"], 1, e1, "a", None, false),
        log_call(&["run", "solution.12am"], 1, e2, "a", None, false),
        log_call(
            &["run", "solution.12am"],
            1,
            "solution.12am:2: runtime error: division by zero\n",
            "a",
            None,
            false,
        ),
        log_call(
            &["run", "solution.12am"],
            2,
            "",
            "a",
            Some("file `/x` is outside this directory"),
            false,
        ),
        log_call(&["run", "solution.12am"], 124, "", "a", None, true),
        log_call(
            &["build", "solution.12am", "-o", "s"],
            2,
            "midnight: unknown option `-x` for `build`\n",
            "a",
            None,
            false,
        ),
        log_call(&["run", "solution.12am"], 0, "", "a", None, false),
    ];
    let s = logs::summary(&calls);
    assert_eq!(s.get("total").as_i64(), Some(7));
    assert_eq!(
        s.get("by_subcommand"),
        &Value::Obj(obj! { "check" => 1, "run" => 5, "build" => 1 })
    );
    let failed = s.get("failed");
    for (k, n) in [
        ("compile", 2),
        ("runtime", 1),
        ("refused", 1),
        ("timeout", 1),
        ("other", 1),
    ] {
        assert_eq!(failed.get(k).as_i64(), Some(n), "{k}");
    }
    assert_eq!(failed.members().len(), 5);
    let d = logs::diagnostics(&calls);
    let repeats: Vec<bool> = d
        .iter()
        .filter(|x| x.get("template").as_str() == Some("unknown function `…`"))
        .map(|x| x.get("repeated") == &Value::Bool(true))
        .collect();
    assert_eq!(repeats, [true, false]);
}

#[test]
fn first_try_and_verified() {
    let mut calls = vec![
        log_call(&["loops", "solution.12am"], 0, "", "w1", None, false),
        log_call(&["check", "solution.12am"], 1, "", "w1", None, false),
        log_call(&["run", "solution.12am"], 0, "", "w2", None, false),
        log_call(&["run", "solution.12am"], 0, "", "w3", None, false),
    ];
    assert_eq!(logs::first_try(&calls, None), Some(false));
    assert!(logs::verified(&calls, Some("w2"), None));
    assert!(!logs::verified(&calls, Some("w4"), None));
    assert_eq!(logs::first_try(&[], None), None);
    // For a repair, calls on the given content do not count.
    assert_eq!(logs::first_try(&calls[1..], Some("w1")), Some(true));

    // With a hash of the whole program, a helper changed after the last run
    // leaves the graded program unverified.
    calls[2]
        .as_obj_mut()
        .expect("an object")
        .set("program_sha256", "p2");
    assert!(logs::verified(&calls, Some("w2"), Some("p2")));
    assert!(!logs::verified(&calls, Some("w2"), Some("p9")));
}

fn with(init: &Value, key: &str, v: Value) -> Value {
    let mut i = init.clone();
    i.as_obj_mut().expect("an object").set(key, v);
    i
}

fn problem(init: &Value) -> String {
    logs::isolation_problem(Some(init), None).unwrap_or_default()
}

#[test]
fn init_isolation() {
    let init = json::parse(&fixture("init.json")).expect("JSON");
    assert_eq!(logs::isolation_problem(Some(&init), None), None);
    assert!(
        problem(&with(
            &init,
            "tools",
            Value::from(vec!["Bash", "Read", "Agent"])
        ))
        .contains("tools")
    );
    let servers = Value::Arr(vec![Value::Obj(obj! { "name" => "x" })]);
    assert!(problem(&with(&init, "mcp_servers", servers)).contains("MCP"));
    assert!(problem(&with(&init, "skills", Value::from(vec!["dataviz"]))).contains("skills"));
    assert!(problem(&with(&init, "slash_commands", Value::from(vec!["loop"]))).contains("slash"));
    let mut plugins = init.get("plugins").items().to_vec();
    plugins.push(Value::Obj(obj! {
        "name" => "mine",
        "path" => "/home/x/.claude/plugins/mine",
        "source" => "mine@local",
    }));
    assert!(problem(&with(&init, "plugins", Value::Arr(plugins))).contains("plugins [\"mine\"]"));
    assert_eq!(
        logs::isolation_problem(None, None).as_deref(),
        Some("no init line")
    );
    assert_eq!(
        logs::isolation_problem(Some(&init), Some("/tmp/eval.Root0000")),
        None
    );
    assert!(
        logs::isolation_problem(Some(&init), Some("/tmp/eval.Other000"))
            .unwrap_or_default()
            .contains("cwd")
    );
    assert!(
        problem(&with(
            &init,
            "permissionMode",
            Value::from("bypassPermissions")
        ))
        .contains("permission mode")
    );
}

#[test]
fn tool_calls_and_denials() {
    let t = Transcript::new(&fixture("denied.jsonl"));
    let calls = t.tool_calls();
    let tools: Vec<String> = calls.iter().map(|c| c.get("tool").to_text()).collect();
    assert_eq!(tools, ["Bash", "Read", "Bash", "Write"]);
    let denied: Vec<bool> = calls
        .iter()
        .map(|c| c.get("denied") == &Value::Bool(true))
        .collect();
    assert_eq!(denied, [true, true, false, false]);
    assert_eq!(calls[2].get("result").as_str(), Some("hi"));
    assert_eq!(t.models_reported(), ["claude-haiku-5-5"]);
    assert_eq!(t.infrastructure(), None);
    let u = t.usage();
    assert_eq!(u.get("turns").as_i64(), Some(5));
    assert_eq!(u.get("partial"), &Value::Bool(false));
}

/// A real run: Sonnet on thermostat, ids and the run's directory replaced.
#[test]
fn recorded_run() {
    let t = Transcript::new(&fixture("thermostat-sonnet.jsonl"));
    assert_eq!(logs::isolation_problem(t.init(), None), None);
    let calls = t.tool_calls();
    let denied: Vec<_> = calls.iter().filter(|c| c.get("denied").truthy()).collect();
    assert_eq!(denied.len(), 2);
    assert!(
        denied
            .iter()
            .all(|c| c.get("tool").as_str() == Some("Bash"))
    );
    assert_eq!(t.models_reported(), ["claude-sonnet-5-5"]);
    assert_eq!(t.infrastructure(), None);
    let root = t.init().expect("an init line").get("cwd").to_text();
    let tool_results = runner::tool_results_dir(&root);
    let (escapes, _) = audit::check(
        &calls,
        &Places {
            root: &root,
            wrapper: "/tmp/eval.Wrap0000",
            tool_results: &tool_results,
            scratch: None,
        },
    );
    assert!(escapes.is_empty());

    let log: Vec<Value> = text::lines(&fixture("thermostat-sonnet-calls.jsonl"))
        .into_iter()
        .map(|l| json::parse(l).expect("JSON"))
        .collect();
    let s = logs::summary(&log);
    assert_eq!(s.get("by_subcommand"), &Value::Obj(obj! { "run" => 2 }));
    assert_eq!(s.get("failed"), &Value::Obj(obj! { "compile" => 1 }));
    let d = logs::diagnostics(&log);
    let templates: Vec<String> = d.iter().map(|x| x.get("template").to_text()).collect();
    assert_eq!(templates, ["`…` is a keyword and can't be used as a name"]);
    assert_eq!(d[0].get("spans"), &Value::from(vec!["on"]));
    assert_eq!(logs::first_try(&log, None), Some(false));
    assert!(logs::verified(
        &log,
        log.last().expect("a call").get("sha256").as_str(),
        None
    ));
}

#[test]
fn infrastructure() {
    let api = Transcript::new(&fixture("api-error.jsonl"));
    let (reason, limit) = api.infrastructure().expect("an error");
    assert!(reason.starts_with("API error"));
    assert!(!limit);
    let usage = Transcript::new(&fixture("usage-limit.jsonl"));
    let (reason, limit) = usage.infrastructure().expect("an error");
    assert!(reason.starts_with("usage limit"));
    assert!(limit);
    let last = json::parse(
        text::lines(&fixture("api-error.jsonl"))
            .last()
            .expect("a line"),
    )
    .expect("JSON");
    let login = with(
        &with(
            &last,
            "result",
            Value::from("Not logged in · Please run /login"),
        ),
        "api_error_status",
        Value::Null,
    );
    let (reason, limit) = Transcript::new(&json::generate(&login))
        .infrastructure()
        .expect("an error");
    assert!(reason.contains("Not logged in"));
    assert!(!limit);
    let rate = with(
        &with(
            &last,
            "result",
            Value::from("API Error: 429 rate_limit_error"),
        ),
        "api_error_status",
        Value::Int(429),
    );
    let (reason, limit) = Transcript::new(&json::generate(&rate))
        .infrastructure()
        .expect("an error");
    assert!(reason.starts_with("API error"));
    assert!(!limit, "a rate limit is not a usage limit");
    let first4: String = text::lines(&fixture("denied.jsonl"))[..4].concat();
    let cut = Transcript::new(&first4);
    assert_eq!(
        cut.infrastructure(),
        Some(("no result line".to_string(), false))
    );
    let u = cut.usage();
    assert_eq!(u.get("partial"), &Value::Bool(true));
    assert!(u.get("output_tokens").as_i64().unwrap_or(0) > 0);
}
