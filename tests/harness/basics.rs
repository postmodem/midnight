//! JSON, SHA-256, numbers in results and reports, shell words and glob
//! patterns, results files read and written again, and grading with the
//! real compiler where MIDNIGHT_BIN names one.

use std::process::{Command, Stdio};

use crate::grade;
use crate::help::{Temp, repo};
use crate::json::{self, Value};
use crate::proc;
use crate::report;
use crate::sha256;
use crate::task::Task;
use crate::text;
use crate::tools;

#[test]
fn json_reads_any_json() {
    let v = json::parse(" {\"a\" : [1, -2.5e3, true, false, null],\n\t\"b\": \"x\\u00e9\\ud83d\\ude00\\/\\b\\f\\n\\\"\"} ")
        .expect("JSON");
    assert_eq!(
        v.get("a").items(),
        &[
            Value::Int(1),
            Value::Float(-2500.0),
            Value::Bool(true),
            Value::Bool(false),
            Value::Null
        ]
    );
    assert_eq!(v.get("b").as_str(), Some("xé😀/\u{8}\u{c}\n\""));
    assert_eq!(
        json::parse("\"\\ud800x\"").expect("JSON"),
        Value::from("\u{fffd}x")
    );
    for bad in ["", "{", "[1,]", "01", "1.", "\"a", "nul", "{} x", "NaN"] {
        assert!(json::parse(bad).is_err(), "{bad}");
    }
}

#[test]
fn json_writes_one_line_and_indented() {
    let v =
        json::parse("{\"a\":[1,2.0,\"\\u0001\\u007f/é\"],\"b\":{},\"c\":[],\"d\":{\"e\":null}}")
            .expect("JSON");
    assert_eq!(
        json::generate(&v),
        "{\"a\":[1,2.0,\"\\u0001\u{7f}/é\"],\"b\":{},\"c\":[],\"d\":{\"e\":null}}"
    );
    assert_eq!(
        json::pretty(&v),
        "{\n  \"a\": [\n    1,\n    2.0,\n    \"\\u0001\u{7f}/é\"\n  ],\n  \"b\": {},\n  \"c\": [],\n  \"d\": {\n    \"e\": null\n  }\n}"
    );
    assert_eq!(
        json::generate(&Value::from("\u{8}\u{c}\n\r\t\"\\")),
        "\"\\b\\f\\n\\r\\t\\\"\\\\\""
    );
    assert_eq!(
        v.get("d").to_text(),
        "{\"e\":null}",
        "an object as text is JSON"
    );
}

#[test]
fn floats_are_json_and_read_back_the_same() {
    for (x, text) in [
        (1.0, "1.0"),
        (0.1 + 0.2, "0.30000000000000004"),
        (-0.0, "-0.0"),
        (1e-5, "1e-5"),
        (1e16, "1e16"),
        (f64::NAN, "null"),
        (f64::INFINITY, "null"),
    ] {
        assert_eq!(json::number(x), text, "{x}");
    }
    let xs = [
        0.0,
        -0.0,
        1.0,
        0.1,
        2.0 / 3.0,
        1e-7,
        123456789012345.0,
        1234567890123456.8,
        9.759759907609144e16,
        7.191893270562275e-05,
        f64::MAX,
        f64::MIN_POSITIVE,
        5e-324,
        -1.5e300,
    ];
    for x in xs {
        let text = json::generate(&Value::Arr(vec![Value::Float(x), Value::Int(3)]));
        let back = json::parse(&text).expect("valid JSON");
        let Value::Float(y) = back.items()[0] else {
            panic!("{text}: not a float")
        };
        assert_eq!(y.to_bits(), x.to_bits(), "{text}");
        assert_eq!(back.items()[1], Value::Int(3), "{text}");
    }
}

#[test]
fn results_files_read_report_and_write_back() {
    let mut files: Vec<String> = proc::children(&format!("{}/evals/results", repo()))
        .into_iter()
        .filter(|f| f.ends_with(".json"))
        .map(|f| format!("{}/evals/results/{f}", repo()))
        .collect();
    files.push(format!("{}/evals/test/fixtures/results-a.json", repo()));
    files.push(format!("{}/evals/test/fixtures/results-b.json", repo()));
    let first = json::parse(&proc::read(&files[0]).expect("a results file")).expect("JSON");
    for f in files {
        let text = proc::read(&f).expect("a results file");
        let v = json::parse(&text).expect("JSON");
        assert!(report::render(&v).contains("## Pass rates"), "{f}");
        assert!(report::render_compare(&first, &v).starts_with('#'), "{f}");
        let again = json::parse(&json::pretty(&v)).expect("JSON written back");
        assert_eq!(again, v, "{f}");
    }
}

#[test]
fn numbers_in_reports() {
    assert_eq!(report::fmt(None, 1), "–");
    assert_eq!(report::fmt(Some(200.0 / 3.0), 1), "66.7");
    assert_eq!(report::fmt(Some(2.0), 1), "2.0");
    assert_eq!(report::fmt(Some(0.0123), 3), "0.012");
}

#[test]
fn sha256_known_values() {
    assert_eq!(
        sha256::hex(b""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        sha256::hex(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(
        sha256::hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
        "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
    );
    let mut h = sha256::Sha256::new();
    for _ in 0..1000 {
        h.update(&[b'a'; 1000]);
    }
    assert_eq!(
        h.hex(),
        "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
    );
}

#[test]
fn sha256_agrees_with_sha256sum() {
    let sum = format!("{}/sha256sum", tools::coreutils().expect("coreutils"));
    let d = Temp::new();
    for n in [0, 1, 55, 56, 57, 63, 64, 65, 119, 120, 127, 128, 129, 1000] {
        let data: Vec<u8> = (0..n).map(|i| (i * 7 + 3) as u8).collect();
        let f = d.path("data");
        proc::write(&f, &data).expect("a file");
        let o = Command::new(&sum)
            .arg(&f)
            .stdin(Stdio::null())
            .output()
            .expect("sha256sum runs");
        let want = String::from_utf8_lossy(&o.stdout)
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_string();
        assert_eq!(sha256::hex(&data), want, "{n} bytes");
    }
}

#[test]
fn shell_words() {
    assert_eq!(
        text::shell_split("a 'b c' \"d\\\"e\" f\\ g  \"x\\y\"z").expect("words"),
        ["a", "b c", "d\"e", "f g", "x\\yz"]
    );
    assert_eq!(
        text::shell_split("  ").expect("words"),
        Vec::<String>::new()
    );
    assert_eq!(text::shell_split("a''b \"\"").expect("words"), ["ab", ""]);
    assert!(text::shell_split("a 'b").is_err());
    assert!(text::shell_split("a \"b").is_err());
}

#[test]
fn glob_patterns() {
    assert!(!text::fnmatch("*.md", ".x.md", false));
    assert!(text::fnmatch("{a,b}*", "bcd", false));
    assert!(text::fnmatch("**/*.md", "x/y/z.md", true));
    assert!(text::fnmatch("**/*.md", "z.md", true));
    assert!(text::fnmatch("*", "a/b", false));
    assert!(!text::fnmatch("*", "a/b", true));
    assert!(text::fnmatch(
        "examples/*.12am",
        "examples/hello.12am",
        true
    ));
    assert!(!text::fnmatch(
        "examples/*.12am",
        "examples/sub/b.12am",
        true
    ));
    assert!(text::fnmatch("[a-c]?.md", "bx.md", false));
    assert!(!text::fnmatch("[!a-c]?.md", "bx.md", false));
}

#[test]
fn paths() {
    assert_eq!(
        text::expand_path("../b/./c", "/r/a").as_deref(),
        Some("/r/b/c")
    );
    assert_eq!(text::expand_path("/x/", "/r").as_deref(), Some("/x"));
    assert_eq!(text::expand_path("..", "/").as_deref(), Some("/"));
    assert_eq!(text::expand_path("~nobody/x", "/r"), None);
    assert_eq!(text::dirname("/a/b"), "/a");
    assert_eq!(text::dirname("/a"), "/");
    assert_eq!(text::dirname("a"), ".");
    assert_eq!(text::join(&["/", "a", "/b"]), "/a/b");
    assert!(text::inside("/a/b", "/a"));
    assert!(!text::inside("/ab", "/a"));
    assert_eq!(
        text::iso8601_ms(
            std::time::UNIX_EPOCH + std::time::Duration::from_millis(1_795_000_000_123)
        ),
        "2026-11-18T11:06:40.123Z"
    );
}

/// The real compiler on a task of its own: what passes, what fails and
/// why, and the systems check through `midnight loops`.
#[test]
fn grades_with_the_real_compiler() {
    let Ok(compiler) = std::env::var("MIDNIGHT_BIN") else {
        return;
    };
    if compiler.is_empty() {
        return;
    }
    let _turn = crate::help::grading();
    let d = Temp::new();
    d.write("tasks/greet/TASK.md", "Print this.\n\n```\nhello\n```\n");
    d.write("tasks/greet/meta", "areas: ecs\n");
    d.write("tasks/greet/expected.out", "hello\n1.0\n");
    d.write("tasks/greet/systems", "greet each\n");
    let solution = "puts \"hello\"\n\ncomponent Position\n  x: Float\nend\n\nsystem greet\n  each Position do |p|\n    puts p.x\n  end\nend\n\nspawn Position(1.0)\nWorld.tick\n";
    d.write("tasks/greet/solution.12am", solution);
    d.write("tasks/greet/right/same.12am", solution);
    d.write(
        "tasks/greet/wrong/output.12am",
        &solution.replace("hello", "hi"),
    );
    let no_loop = solution.replace(
        "  each Position do |p|\n    puts p.x\n  end\n",
        "  puts 1.0\n",
    );
    d.write("tasks/greet/wrong/no_loop.12am", &no_loop);
    let (count, problems) =
        grade::self_test(&d.path("tasks"), &compiler, &[]).expect("a self-test");
    assert_eq!(count, 1);
    assert!(problems.is_empty(), "{problems:?}");

    let task = Task::new(&d.path("tasks/greet")).expect("a task");
    let broken = d.write("broken.12am", "puts(\n");
    let g = grade::grade(&task, &broken, &compiler, None).expect("a grade");
    assert_eq!(g.get("verdict").as_str(), Some("fail"));
    assert_eq!(g.get("checks").get("build"), &Value::Bool(false));
    assert!(g.get("notes").get("build").to_text().contains("error"));
    let g = grade::grade(
        &task,
        &d.path("tasks/greet/wrong/no_loop.12am"),
        &compiler,
        None,
    )
    .expect("a grade");
    assert_eq!(
        g.get("notes").get("systems").as_str(),
        Some("missing: greet each")
    );
}
