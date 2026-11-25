//! End-to-end tests: every program under `tests/` and `examples/` is
//! compiled by the `gremlin` binary and its behaviour compared with the
//! expected files next to it.
//!
//! - `tests/run/NAME.grem` + `NAME.out`: builds in release and debug mode;
//!   each must exit 0, print exactly `NAME.out`, and print nothing to
//!   standard error.
//! - `tests/fail/NAME.grem` + `NAME.err`: `gremlin check` must exit 1 and
//!   print exactly `NAME.err`.
//! - `tests/panic/NAME.grem` + `NAME.err`: builds in both modes; each must
//!   exit 1 and print exactly `NAME.err` to standard error.
//! - `examples/NAME.grem` + `NAME.out`: as `tests/run`. `examples/bench.grem`
//!   and `bench/churn.grem` are only compiled.
//!
//! Set `UPDATE_EXPECT=1` to write the expected files from the actual
//! output instead of comparing.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

const GREMLIN: &str = env!("CARGO_BIN_EXE_gremlin");
const STRICT_CFLAGS: &str = "-Wall -Wextra -Werror";
const MODES: [Mode; 2] = [Mode::Release, Mode::Debug];

#[derive(Clone, Copy, Debug, PartialEq)]
enum Mode {
    Release,
    Debug,
}

fn update_expect() -> bool {
    std::env::var_os("UPDATE_EXPECT").is_some()
}

/// The `.grem` files directly inside `dir`, sorted. Empty if it is missing.
fn programs(dir: &str) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(|entry| Some(entry.ok()?.path()))
        .filter(|path| path.extension().is_some_and(|ext| ext == "grem"))
        .collect();
    files.sort();
    files
}

/// A fresh directory for the executables of one test function.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("gremlin-test-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch directory");
    dir
}

/// Runs `job` on every item across the available cores and returns the
/// failures in input order.
fn for_each_parallel<T: Sync>(
    items: &[T],
    job: impl Fn(&T) -> Result<(), String> + Sync,
) -> Vec<String> {
    let next = AtomicUsize::new(0);
    let failures = Mutex::new(Vec::new());
    let workers = std::thread::available_parallelism().map_or(4, |n| n.get());
    std::thread::scope(|scope| {
        for _ in 0..workers.min(items.len()) {
            scope.spawn(|| {
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some(item) = items.get(index) else { break };
                    if let Err(message) = job(item) {
                        failures.lock().unwrap().push((index, message));
                    }
                }
            });
        }
    });
    let mut failures = failures.into_inner().unwrap();
    failures.sort();
    failures.into_iter().map(|(_, message)| message).collect()
}

fn report(kind: &str, total: usize, failures: Vec<String>) {
    if !failures.is_empty() {
        panic!(
            "{} of {total} {kind} failed:\n\n{}",
            failures.len(),
            failures.join("\n\n")
        );
    }
}

fn build(source: &Path, exe: &Path, mode: Mode) -> Result<(), String> {
    let mut command = Command::new(GREMLIN);
    command.arg("build").arg(source).arg("-o").arg(exe);
    if mode == Mode::Debug {
        command.arg("--debug");
    }
    command.env("GREMLIN_CFLAGS", STRICT_CFLAGS);
    let output = command.output().expect("run gremlin");
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "{} ({mode:?}): build failed\n{}",
            source.display(),
            String::from_utf8_lossy(&output.stderr)
        ))
    }
}

fn exe_path(dir: &Path, source: &Path, mode: Mode) -> PathBuf {
    let stem = source.file_stem().unwrap().to_string_lossy();
    dir.join(format!("{stem}-{mode:?}"))
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// Compares `actual` with the contents of `expected_path`, or writes it
/// there when updating.
fn expect(expected_path: &Path, actual: &str, what: &str) -> Result<(), String> {
    if update_expect() {
        std::fs::write(expected_path, actual).expect("write expected file");
        return Ok(());
    }
    let expected = std::fs::read_to_string(expected_path)
        .map_err(|error| format!("can't read {}: {error}", expected_path.display()))?;
    if expected == actual {
        Ok(())
    } else {
        Err(format!(
            "{what} differs from {}\n--- expected\n{expected}--- actual\n{actual}---",
            expected_path.display()
        ))
    }
}

/// Builds and runs a program that must succeed, in one mode.
fn run_ok(dir: &Path, source: &Path, mode: Mode) -> Result<(), String> {
    let exe = exe_path(dir, source, mode);
    build(source, &exe, mode)?;
    let output: Output = Command::new(&exe).output().expect("run program");
    let label = format!("{} ({mode:?})", source.display());
    if output.status.code() != Some(0) {
        return Err(format!(
            "{label}: exited with {:?}\n--- stdout\n{}--- stderr\n{}",
            output.status,
            text(&output.stdout),
            text(&output.stderr)
        ));
    }
    if !output.stderr.is_empty() {
        return Err(format!(
            "{label}: wrote to standard error\n{}",
            text(&output.stderr)
        ));
    }
    // Only the release run may write the expected file; debug must match it.
    let out_path = source.with_extension("out");
    if update_expect() && mode == Mode::Debug {
        return Ok(());
    }
    expect(&out_path, &text(&output.stdout), "standard output")
        .map_err(|message| format!("{label}: {message}"))
}

fn run_ok_all(name: &str, sources: &[PathBuf]) {
    let dir = scratch(name);
    let jobs: Vec<(&PathBuf, Mode)> = sources
        .iter()
        .flat_map(|source| MODES.map(|mode| (source, mode)))
        .collect();
    let failures = for_each_parallel(&jobs, |(source, mode)| run_ok(&dir, source, *mode));
    let _ = std::fs::remove_dir_all(&dir);
    report("runs", jobs.len(), failures);
}

#[test]
fn run_programs() {
    run_ok_all("run", &programs("tests/run"));
}

#[test]
fn example_programs() {
    let sources: Vec<PathBuf> = programs("examples")
        .into_iter()
        .filter(|path| path.file_name().is_some_and(|name| name != "bench.grem"))
        .collect();
    run_ok_all("examples", &sources);
}

#[test]
fn benchmarks_compile() {
    let sources: Vec<PathBuf> = ["examples/bench.grem", "bench/churn.grem"]
        .iter()
        .map(PathBuf::from)
        .filter(|path| path.exists())
        .collect();
    let dir = scratch("bench");
    let jobs: Vec<(&PathBuf, Mode)> = sources
        .iter()
        .flat_map(|source| MODES.map(|mode| (source, mode)))
        .collect();
    let failures = for_each_parallel(&jobs, |(source, mode)| {
        build(source, &exe_path(&dir, source, *mode), *mode)
    });
    let _ = std::fs::remove_dir_all(&dir);
    report("benchmark builds", jobs.len(), failures);
}

#[test]
fn fail_programs() {
    let sources = programs("tests/fail");
    let failures = for_each_parallel(&sources, |source| {
        let output = Command::new(GREMLIN)
            .arg("check")
            .arg(source)
            .output()
            .expect("run gremlin");
        if output.status.code() != Some(1) {
            return Err(format!(
                "{}: `gremlin check` exited with {:?}, expected 1\n{}",
                source.display(),
                output.status,
                text(&output.stderr)
            ));
        }
        expect(
            &source.with_extension("err"),
            &text(&output.stderr),
            "diagnostics",
        )
        .map_err(|message| format!("{}: {message}", source.display()))
    });
    report("failing programs", sources.len(), failures);
}

#[test]
fn panic_programs() {
    let sources = programs("tests/panic");
    let dir = scratch("panic");
    let jobs: Vec<(&PathBuf, Mode)> = sources
        .iter()
        .flat_map(|source| MODES.map(|mode| (source, mode)))
        .collect();
    let failures = for_each_parallel(&jobs, |(source, mode)| {
        let exe = exe_path(&dir, source, *mode);
        build(source, &exe, *mode)?;
        let output = Command::new(&exe).output().expect("run program");
        let label = format!("{} ({mode:?})", source.display());
        if output.status.code() != Some(1) {
            return Err(format!(
                "{label}: exited with {:?}, expected 1\n--- stderr\n{}",
                output.status,
                text(&output.stderr)
            ));
        }
        if update_expect() && *mode == Mode::Debug {
            return Ok(());
        }
        expect(
            &source.with_extension("err"),
            &text(&output.stderr),
            "standard error",
        )
        .map_err(|message| format!("{label}: {message}"))
    });
    let _ = std::fs::remove_dir_all(&dir);
    report("panicking programs", jobs.len(), failures);
}

#[test]
fn move_example_is_the_first_program_in_the_reference() {
    let reference = std::fs::read_to_string("docs/language.md").expect("read the reference");
    let opening = "```ruby\n";
    let start = reference.find(opening).expect("a ruby block") + opening.len();
    let end = start + reference[start..].find("```").expect("the block's end");
    let example = std::fs::read_to_string("examples/move.grem").expect("read examples/move.grem");
    assert_eq!(example, &reference[start..end]);
}

/// The component in `tests/run/layout.grem` has a `Float`, a `Float32`, an
/// `Int16`, a `UInt8`, a `Bool` and an enum: 17 bytes of fields, which the
/// largest alignment rounds up to 24.
#[test]
fn mixed_component_is_as_small_as_its_fields_allow() {
    let output = Command::new(GREMLIN)
        .args(["emit-c", "tests/run/layout.grem"])
        .output()
        .expect("run gremlin");
    assert!(output.status.success(), "{}", text(&output.stderr));
    let dir = scratch("layout");
    let mut c_source = text(&output.stdout);
    c_source.push_str("\n_Static_assert(sizeof(T_Body) == 24, \"size of Body\");\n");
    std::fs::write(dir.join("layout.c"), c_source).expect("write C");
    std::fs::copy("runtime/gremlin_rt.h", dir.join("gremlin_rt.h")).expect("copy header");
    let cc = std::env::var("GREMLIN_CC").unwrap_or_else(|_| "cc".to_string());
    let compiled = Command::new(cc)
        .args(["-std=c11", "-fsyntax-only"])
        .arg(dir.join("layout.c"))
        .output()
        .expect("run the C compiler");
    let _ = std::fs::remove_dir_all(&dir);
    assert!(compiled.status.success(), "{}", text(&compiled.stderr));
}
