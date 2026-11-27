//! JSON, SHA-256, numbers as Ruby writes them, shell words and glob
//! patterns, results files read and written again, and grading with the
//! real compiler where MIDNIGHT_BIN names one.

use std::process::{Command, Stdio};

use crate::grade;
use crate::help::{Temp, repo};
use crate::json::{self, Value};
use crate::proc;
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
fn json_writes_as_ruby_does() {
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
}

#[test]
fn values_as_ruby_inspects_them() {
    let v = json::parse("{\"output\":\"status 1\",\"x\":null,\"a\":[1,\"b\"]}").expect("JSON");
    assert_eq!(
        json::inspect(&v),
        "{\"output\" => \"status 1\", \"x\" => nil, \"a\" => [1, \"b\"]}"
    );
    assert_eq!(
        json::inspect_str("\u{1}\u{1b}\u{7f}#{x}é"),
        "\"\\u0001\\e\\u007F\\#{x}é\""
    );
    assert_eq!(
        json::inspect_bytes("tick 1: 20.5 °C".as_bytes()),
        "\"tick 1: 20.5 \\xC2\\xB0C\""
    );
}

#[test]
fn results_files_read_and_write_back_the_same() {
    let mut files: Vec<String> = proc::children(&format!("{}/evals/results", repo()))
        .into_iter()
        .filter(|f| f.ends_with(".json"))
        .map(|f| format!("{}/evals/results/{f}", repo()))
        .collect();
    files.push(format!("{}/evals/test/fixtures/results-a.json", repo()));
    files.push(format!("{}/evals/test/fixtures/results-b.json", repo()));
    for f in files {
        let text = proc::read(&f).expect("a results file");
        let v = json::parse(&text).expect("JSON");
        assert_eq!(format!("{}\n", json::pretty(&v)), text, "{f}");
    }
}

#[test]
fn floats_as_ruby_writes_them() {
    for (x, text) in [
        (1.0, "1.0"),
        (0.1 + 0.2, "0.30000000000000004"),
        (1e-5, "1.0e-05"),
        (0.0001, "0.0001"),
        (1e14, "100000000000000.0"),
        (1e15, "1.0e+15"),
        (123456789012345.0, "123456789012345.0"),
        (1234567890123456.8, "1234567890123456.8"),
        (12345678901234567.0, "1.2345678901234568e+16"),
        (0.00012, "0.00012"),
        (5048710871122148.0, "5.048710871122148e+15"),
        (1221336002775921.2, "1221336002775921.2"),
        (1e16, "1.0e+16"),
        (3.805629266812557e17, "3.805629266812557e+17"),
        (2.75303133667458e-06, "2.75303133667458e-06"),
        (-0.0, "-0.0"),
        (245906243263069.2, "245906243263069.2"),
    ] {
        assert_eq!(json::float_text(x), text);
    }
    // x, then %.0f, %.1f, %.2f and %.3f as Ruby 3.4 writes them.
    let table = [
        (0.35, "0", "0.4", "0.35", "0.350"),
        (0.3499999999999999, "0", "0.4", "0.35", "0.350"),
        (0.045, "0", "0.0", "0.04", "0.045"),
        (2.675, "3", "2.7", "2.68", "2.675"),
        (1.005, "1", "1.0", "1.00", "1.005"),
        (0.15, "0", "0.2", "0.15", "0.150"),
        (99.95, "100", "100.0", "99.95", "99.950"),
        (0.05, "0", "0.1", "0.05", "0.050"),
        (2.5, "2", "2.5", "2.50", "2.500"),
        (0.25, "0", "0.2", "0.25", "0.250"),
        (0.125, "0", "0.1", "0.12", "0.125"),
        (0.375, "0", "0.4", "0.38", "0.375"),
        (0.0005, "0", "0.0", "0.00", "0.001"),
        (0.00049, "0", "0.0", "0.00", "0.000"),
        (0.0, "0", "0.0", "0.00", "0.000"),
        (66.66666666666667, "67", "66.7", "66.67", "66.667"),
        (83.125, "83", "83.1", "83.12", "83.125"),
        (50.35, "50", "50.4", "50.35", "50.350"),
        (80.349, "80", "80.3", "80.35", "80.349"),
        (2.6666666666666665, "3", "2.7", "2.67", "2.667"),
        (
            245906243263069.2,
            "245906243263069",
            "245906243263069.2",
            "245906243263069.19",
            "245906243263069.188",
        ),
        (
            9.759759907609144e16,
            "97597599076091440",
            "97597599076091440.0",
            "97597599076091440.00",
            "97597599076091440.000",
        ),
        (7.191893270562275e-05, "0", "0.0", "0.00", "0.000"),
    ];
    for (x, f0, f1, f2, f3) in table {
        assert_eq!(
            [
                json::fixed(x, 0),
                json::fixed(x, 1),
                json::fixed(x, 2),
                json::fixed(x, 3)
            ],
            [f0, f1, f2, f3],
            "{x}"
        );
    }
    assert_eq!(json::fixed(-0.0004, 1), "-0.0");
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
