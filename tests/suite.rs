//! End-to-end tests: every program under `tests/` and `examples/` is
//! compiled by the `midnight` binary and its behaviour compared with the
//! expected files next to it.
//!
//! - `tests/run/NAME.12am` + `NAME.out`: builds in release and debug mode;
//!   each must exit 0, print exactly `NAME.out`, and print nothing to
//!   standard error.
//! - `tests/fail/NAME.12am` + `NAME.err`: `midnight check` must exit 1 and
//!   print exactly `NAME.err`.
//! - `tests/panic/NAME.12am` + `NAME.err`: builds in both modes; each must
//!   exit 1 and print exactly `NAME.err` to standard error.
//! - `tests/plan/NAME.12am` + `NAME.plan`: `midnight plan` must exit 0,
//!   print exactly `NAME.plan`, and print nothing to standard error.
//! - `tests/loops/NAME.12am` + `NAME.loops`: `midnight loops` must exit 0,
//!   print exactly `NAME.loops`, and print nothing to standard error.
//! - `examples/NAME.12am` + `NAME.out`: as `tests/run`. `examples/bench.12am`
//!   and `bench/churn.12am` are only compiled.
//!
//! Every program above runs with `MIDNIGHT_GPU=off`, so GPU systems run
//! their CPU implementation.
//!
//! Set `UPDATE_EXPECT=1` to write the expected files from the actual
//! output instead of comparing.
//!
//! # GPU mode
//!
//! The GPU tests (`gpu_programs`, `gpu_stats_match_plan`,
//! `gpu_stats_match_expected`, `gpu_driver_finds_its_parts` and
//! `gpu_strict_div_sqrt`) do nothing unless `MIDNIGHT_TEST_GPU` names where
//! GPU systems are to run:
//!
//! - `software`: on Mesa's `llvmpipe`. `MIDNIGHT_TEST_SOFTWARE_ICD` is the
//!   path of its Vulkan driver file.
//! - `hardware`: on the device the Vulkan loader finds, which a
//!   `VK_DRIVER_FILES` the caller sets chooses.
//!
//! Either way every program runs as a release and as a debug build.
//!
//! `gpu_programs` runs every program of `tests/run`, `tests/panic` and
//! `examples` that declares a GPU system, as it is and again with
//! `MIDNIGHT_GPU=rows`. Standard output must match
//! `NAME.out`: exactly, or, when the program has a `# tolerance: T` line,
//! with each `Float` within `T * max(1, |expected|)` of the expected one and
//! everything else exact. A program whose GPU systems are all strict may not
//! have the line. A `tests/panic` program must exit 1 with exactly
//! `NAME.err`. A run in which there is no device, or in which a GPU block
//! runs on the CPU, fails.
//! A program with a strict GPU system must say that the device passed the
//! strict check, and runs once more with `MIDNIGHT_GPU=strict-cpu`, which
//! must give the same output with every strict block on the CPU.
//!
//! `gpu_strict_div_sqrt` runs the `/` and `sqrt` of strict shaders on the
//! device through `tests/device/strict_div_sqrt.c`, built with
//! `MIDNIGHT_GPU_FLAGS`, and compares every result with the CPU's.
//!
//! `gpu_stats_match_plan` runs `tests/run/gpu_transfers.12am` with
//! `MIDNIGHT_GPU=stats` and compares the uploads and downloads of every tick
//! after the first with what `midnight plan` lists, and requires no row to
//! move alone.
//!
//! `gpu_stats_match_expected` runs every program of `tests/run` that has a
//! `NAME.stats` file with `MIDNIGHT_GPU=stats`, and its `stats` lines must
//! be exactly that file. When there is a `NAME.rows.stats` too, it runs the
//! program again with `MIDNIGHT_GPU=stats,rows` against that file. With
//! `UPDATE_EXPECT=1` the release run writes the files instead: `.stats`
//! files are the only expected files GPU mode writes.
//!
//! In GPU mode `gpu_programs`, `gpu_stats_match_plan` and
//! `gpu_stats_match_expected` also run each program with
//! `MIDNIGHT_THREADS=4,small`, which must change neither the output nor a
//! count.
//!
//! # Several threads
//!
//! A program that must stop with an error fails when it is still running
//! after ten seconds.
//!
//! `thread_programs` runs every program of `tests/run`, `examples` and
//! `tests/panic` that has a loop that can be split, as a release and as a
//! debug build, with `MIDNIGHT_THREADS` set to `1`, `4`, `3,small`,
//! `7,small,reverse`, `64,small,jitter` and `2,small,awake`: each run must
//! give the program's expected output, and a program that stops with an
//! error must also print the same every time.
//!
//! `splits_match_expected` runs every program of `tests/run` that has a
//! `NAME.split` file with `MIDNIGHT_THREADS=4,small,info`: after the line
//! with the thread count, standard error must be exactly the lines of that
//! file with `midnight: threads: ` before each, one for each loop the
//! first time it is split.
//!
//! `pool_stress` builds `tests/pool/stress.c` with
//! `runtime/midnight_pool.c` and runs each of its modes.
//!
//! With `MIDNIGHT_TEST_TSAN=1`, `tsan_pool_stress` does the same with
//! ThreadSanitizer, and `tsan_programs` builds every program
//! `thread_programs` runs in release mode with `-fsanitize=thread` and runs
//! it with `4,small` and with `7,small,reverse`; both fail on any report.
//! Without the variable they do nothing.
//!
//! The compiler is the one cargo built, or `MIDNIGHT_BIN` when that is set.
//! Programs are found relative to the working directory, which must be the
//! repository root.

use std::collections::BTreeSet;
use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

const STRICT_CFLAGS: &str = "-Wall -Wextra -Werror";
const MODES: [Mode; 2] = [Mode::Release, Mode::Debug];

/// The program whose `stats` lines are compared with its plan.
const STATS_PROGRAM: &str = "tests/run/gpu_transfers.12am";

#[derive(Clone, Copy, Debug, PartialEq)]
enum Mode {
    Release,
    Debug,
}

/// Where GPU systems run in GPU mode.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Device {
    Software,
    Hardware,
}

impl Device {
    /// The device `MIDNIGHT_TEST_GPU` names, or `None` outside GPU mode.
    fn from_env() -> Option<Device> {
        match std::env::var("MIDNIGHT_TEST_GPU").ok().as_deref() {
            None | Some("") => None,
            Some("software") => {
                software_driver();
                Some(Device::Software)
            }
            Some("hardware") => Some(Device::Hardware),
            Some(other) => {
                panic!("MIDNIGHT_TEST_GPU is `{other}`: expected `software` or `hardware`")
            }
        }
    }
}

/// The path of the Vulkan driver file that offers `llvmpipe`.
fn software_driver() -> std::ffi::OsString {
    std::env::var_os("MIDNIGHT_TEST_SOFTWARE_ICD").expect(
        "MIDNIGHT_TEST_GPU=software needs MIDNIGHT_TEST_SOFTWARE_ICD, the path of \
         llvmpipe's Vulkan driver file; the dev shell sets it",
    )
}

fn update_expect() -> bool {
    std::env::var_os("UPDATE_EXPECT").is_some()
}

/// A command that runs the compiler.
fn midnight() -> Command {
    match std::env::var_os("MIDNIGHT_BIN") {
        Some(path) => Command::new(path),
        None => Command::new(env!("CARGO_BIN_EXE_midnight")),
    }
}

/// A command that runs a compiled program with no GPU device.
fn on_cpu(exe: &Path) -> Command {
    let mut command = Command::new(exe);
    command.env("MIDNIGHT_GPU", "off");
    command
}

/// The `.12am` files directly inside `dir`, sorted. Empty if it is missing.
fn programs(dir: &str) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(|entry| Some(entry.ok()?.path()))
        .filter(|path| path.extension().is_some_and(|ext| ext == "12am"))
        .collect();
    files.sort();
    files
}

/// A fresh directory for the executables of one test function.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("midnight-test-{}-{name}", std::process::id()));
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
    let mut command = midnight();
    command.arg("build").arg(source).arg("-o").arg(exe);
    if mode == Mode::Debug {
        command.arg("--debug");
    }
    command.env("MIDNIGHT_CFLAGS", STRICT_CFLAGS);
    let output = command.output().expect("run midnight");
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
    let output: Output = on_cpu(&exe).output().expect("run program");
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

/// The examples that have an expected output.
fn examples() -> Vec<PathBuf> {
    programs("examples")
        .into_iter()
        .filter(|path| path.file_name().is_some_and(|name| name != "bench.12am"))
        .collect()
}

#[test]
fn example_programs() {
    run_ok_all("examples", &examples());
}

#[test]
fn benchmarks_compile() {
    let sources: Vec<PathBuf> = [
        "examples/bench.12am",
        "bench/churn.12am",
        "bench/gpu_gravity.12am",
        "bench/gpu_move.12am",
        "bench/gpu_move_read.12am",
        "bench/cores_age.12am",
        "bench/cores_flee.12am",
        "bench/cores_small.12am",
    ]
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
        let output = midnight()
            .arg("check")
            .arg(source)
            .output()
            .expect("run midnight");
        if output.status.code() != Some(1) {
            return Err(format!(
                "{}: `midnight check` exited with {:?}, expected 1\n{}",
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
fn plan_programs() {
    let sources = programs("tests/plan");
    let failures = for_each_parallel(&sources, |source| {
        let output = midnight()
            .arg("plan")
            .arg(source)
            .output()
            .expect("run midnight");
        if output.status.code() != Some(0) || !output.stderr.is_empty() {
            return Err(format!(
                "{}: `midnight plan` exited with {:?}, expected 0 and no diagnostics\n{}",
                source.display(),
                output.status,
                text(&output.stderr)
            ));
        }
        expect(
            &source.with_extension("plan"),
            &text(&output.stdout),
            "plan",
        )
        .map_err(|message| format!("{}: {message}", source.display()))
    });
    report("plans", sources.len(), failures);
}

#[test]
fn loops_programs() {
    let sources = programs("tests/loops");
    let failures = for_each_parallel(&sources, |source| {
        let output = midnight()
            .arg("loops")
            .arg(source)
            .output()
            .expect("run midnight");
        if output.status.code() != Some(0) || !output.stderr.is_empty() {
            return Err(format!(
                "{}: `midnight loops` exited with {:?}, expected 0 and no diagnostics\n{}",
                source.display(),
                output.status,
                text(&output.stderr)
            ));
        }
        expect(
            &source.with_extension("loops"),
            &text(&output.stdout),
            "loops",
        )
        .map_err(|message| format!("{}: {message}", source.display()))
    });
    report("loops", sources.len(), failures);
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
        let run = run_within(&mut on_cpu(&exe), PANIC_LIMIT);
        let label = format!("{} ({mode:?})", source.display());
        if run.timed_out {
            return Err(format!(
                "{label}: still running after {} s\n--- stderr\n{}",
                PANIC_LIMIT.as_secs(),
                run.stderr
            ));
        }
        if run.status != Some(1) {
            return Err(format!(
                "{label}: exited with {:?}, expected 1\n--- stderr\n{}",
                run.status, run.stderr
            ));
        }
        if update_expect() && *mode == Mode::Debug {
            return Ok(());
        }
        expect(&source.with_extension("err"), &run.stderr, "standard error")
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
    let example = std::fs::read_to_string("examples/move.12am").expect("read examples/move.12am");
    assert_eq!(example, &reference[start..end]);
}

/// The program under "The plan" in the reference prints the plan shown
/// right after it.
#[test]
fn the_reference_shows_the_plan_of_its_example() {
    let reference = std::fs::read_to_string("docs/language.md").expect("read the reference");
    let section = &reference[reference.find("**The plan.**").expect("the plan's part")..];
    let block = |text: &str, opening: &str| {
        let start = text.find(opening).expect("a code block") + opening.len();
        let end = start + text[start..].find("```").expect("the block's end");
        (text[start..end].to_string(), end + 3)
    };
    let (program, after) = block(section, "```ruby\n");
    let (shown, _) = block(&section[after..], "```\n");
    let dir = scratch("reference-plan");
    let source = dir.join("example.12am");
    std::fs::write(&source, program).expect("write the example");
    let output = midnight()
        .arg("plan")
        .arg(&source)
        .output()
        .expect("run midnight");
    let _ = std::fs::remove_dir_all(&dir);
    assert!(output.status.success(), "{}", text(&output.stderr));
    assert_eq!(text(&output.stdout), shown);
}

/// The component in `tests/run/layout.12am` has a `Float`, a `Float32`, an
/// `Int16`, a `UInt8`, a `Bool` and an enum: 17 bytes of fields, which the
/// largest alignment rounds up to 24.
#[test]
fn mixed_component_is_as_small_as_its_fields_allow() {
    let output = midnight()
        .args(["emit-c", "tests/run/layout.12am"])
        .output()
        .expect("run midnight");
    assert!(output.status.success(), "{}", text(&output.stderr));
    let dir = scratch("layout");
    let mut c_source = text(&output.stdout);
    c_source.push_str("\n_Static_assert(sizeof(T_Body) == 24, \"size of Body\");\n");
    std::fs::write(dir.join("layout.c"), c_source).expect("write C");
    std::fs::copy("runtime/midnight_rt.h", dir.join("midnight_rt.h")).expect("copy header");
    let cc = std::env::var("MIDNIGHT_CC").unwrap_or_else(|_| "cc".to_string());
    let compiled = Command::new(cc)
        .args(["-std=c11", "-fsyntax-only"])
        .arg(dir.join("layout.c"))
        .output()
        .expect("run the C compiler");
    let _ = std::fs::remove_dir_all(&dir);
    assert!(compiled.status.success(), "{}", text(&compiled.stderr));
}

/// Writes a line to standard error past the test harness's capture, so it
/// shows when the test passes.
fn note(line: &str) {
    let _ = writeln!(std::io::stderr(), "{line}");
}

/// The lines of a program that declare GPU systems.
fn gpu_system_lines(program: &str) -> impl Iterator<Item = &str> {
    program
        .lines()
        .filter(|line| line.starts_with("gpu system ") || line.starts_with("strict gpu system "))
}

/// Whether the program at `source` declares a GPU system.
fn declares_gpu_system(source: &Path) -> bool {
    std::fs::read_to_string(source).is_ok_and(|program| gpu_system_lines(&program).next().is_some())
}

/// Whether every GPU system of a program that has some is strict.
fn all_strict(program: &str) -> bool {
    gpu_system_lines(program).all(|line| line.starts_with("strict "))
}

/// The tolerance a program states in a `# tolerance: T` line, if it has
/// one.
fn stated_tolerance(program: &str) -> Result<Option<f64>, String> {
    let Some(stated) = program
        .lines()
        .find_map(|line| line.strip_prefix("# tolerance:"))
    else {
        return Ok(None);
    };
    match stated.trim().parse::<f64>() {
        Ok(tolerance) if tolerance.is_finite() && tolerance > 0.0 => Ok(Some(tolerance)),
        _ => Err(format!("`# tolerance:{stated}` is not a positive number")),
    }
}

/// Splits a line of output at its `Float`s: the text around them, one piece
/// more than there are `Float`s, and the `Float`s. A `Float` is digits with
/// a fraction, an exponent or both, as the language prints one, with a `-`
/// in front when it is negative. Digits that continue a name or have
/// neither part are text.
fn split_floats(line: &str) -> (Vec<&str>, Vec<&str>) {
    let bytes = line.as_bytes();
    let digits = |mut at: usize| {
        while bytes.get(at).is_some_and(u8::is_ascii_digit) {
            at += 1;
        }
        at
    };
    let (mut texts, mut floats) = (Vec::new(), Vec::new());
    let (mut text_start, mut at) = (0, 0);
    while at < bytes.len() {
        let start = at;
        let whole = if bytes[start] == b'-' {
            start + 1
        } else {
            start
        };
        let mut end = digits(whole);
        let in_name =
            start > 0 && (bytes[start - 1].is_ascii_alphanumeric() || bytes[start - 1] == b'_');
        if end == whole || in_name {
            at += 1;
            continue;
        }
        let mut float = false;
        if bytes.get(end) == Some(&b'.') && digits(end + 1) > end + 1 {
            end = digits(end + 1);
            float = true;
        }
        if bytes.get(end) == Some(&b'e') {
            let sign = matches!(bytes.get(end + 1), Some(b'+' | b'-'));
            let exponent = end + 1 + usize::from(sign);
            if digits(exponent) > exponent {
                end = digits(exponent);
                float = true;
            }
        }
        if float {
            texts.push(&line[text_start..start]);
            floats.push(&line[start..end]);
            text_start = end;
        }
        at = end;
    }
    texts.push(&line[text_start..]);
    (texts, floats)
}

/// Compares what a program printed with its expected output. With no
/// tolerance the two must be equal. With one they must have the same
/// lines, every `Float` within `tolerance * max(1, |expected|)` of the
/// expected one, and everything else equal. Returns the largest difference
/// between two `Float`s, as a multiple of `max(1, |expected|)`.
fn compare_output(expected: &str, actual: &str, tolerance: Option<f64>) -> Result<f64, String> {
    if expected == actual {
        return Ok(0.0);
    }
    let differs = |number: usize, expected: &str, actual: &str| {
        Err(format!(
            "line {number}: expected `{expected}`, got `{actual}`"
        ))
    };
    if expected.split('\n').count() != actual.split('\n').count() {
        let (expected_count, actual_count) = (expected.lines().count(), actual.lines().count());
        return Err(if expected_count == actual_count {
            "the end of the last line differs".to_string()
        } else {
            format!("{actual_count} lines, expected {expected_count}")
        });
    }
    let mut largest: f64 = 0.0;
    let lines = expected.split('\n').zip(actual.split('\n'));
    for (index, (expected, actual)) in lines.enumerate() {
        if expected == actual {
            continue;
        }
        let Some(tolerance) = tolerance else {
            return differs(index + 1, expected, actual);
        };
        let (expected_texts, expected_floats) = split_floats(expected);
        let (actual_texts, actual_floats) = split_floats(actual);
        if expected_texts != actual_texts {
            return differs(index + 1, expected, actual);
        }
        for (expected, actual) in expected_floats.iter().zip(&actual_floats) {
            let (e, a): (f64, f64) = (expected.parse().unwrap(), actual.parse().unwrap());
            let difference = (a - e).abs() / e.abs().max(1.0);
            if difference.is_nan() || difference > tolerance {
                return Err(format!(
                    "line {}: {actual} is not within {tolerance} of {expected}",
                    index + 1
                ));
            }
            largest = largest.max(difference);
        }
    }
    Ok(largest)
}

/// What a program did when it ran on a device.
struct DeviceRun {
    status: Option<i32>,
    stdout: String,
    /// Standard error without the lines `MIDNIGHT_GPU` asks for.
    stderr: String,
    /// The device the `info` line names.
    device: String,
    /// What each `stats` line counts, tick 0 first.
    ticks: Vec<Counts>,
    /// The `stats` lines, each without `midnight: gpu: `.
    stats: String,
    /// The `info` line that says where strict blocks run, without
    /// `midnight: gpu: `.
    strict: Option<String>,
}

/// What one `stats` line counts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Counts {
    uploads: u64,
    downloads: u64,
    rows_up: u64,
    rows_down: u64,
    submissions: u64,
    waits: u64,
}

/// Reads what a `stats` line says after `tick`, `N: U uploads, D
/// downloads, R rows up, S rows down, B submissions, W waits`: the tick and
/// the counts.
fn parse_stats(line: &str) -> Option<(usize, Counts)> {
    let (tick, counts) = line.split_once(": ")?;
    let units = [
        ("upload", ""),
        ("download", ""),
        ("row", " up"),
        ("row", " down"),
        ("submission", ""),
        ("wait", ""),
    ];
    let mut numbers = [0u64; 6];
    let mut parts = counts.split(", ");
    for ((noun, after), number) in units.iter().zip(numbers.iter_mut()) {
        let (count, words) = parts.next()?.split_once(' ')?;
        let words = words.strip_suffix(after)?;
        if words.strip_suffix('s').unwrap_or(words) != *noun {
            return None;
        }
        *number = count.parse().ok()?;
    }
    if parts.next().is_some() {
        return None;
    }
    let [uploads, downloads, rows_up, rows_down, submissions, waits] = numbers;
    let counts = Counts {
        uploads,
        downloads,
        rows_up,
        rows_down,
        submissions,
        waits,
    };
    Some((tick.parse().ok()?, counts))
}

/// The `info` line of a device that passed the strict check.
const STRICT_PASSED: &str = "strict check passed";

/// How the `info` lines end that say a strict block runs on the CPU because
/// of `strict-cpu`.
const STRICT_REFUSED: &str = ": MIDNIGHT_GPU has strict-cpu";

/// The setting of `MIDNIGHT_THREADS` that the runs on a device are
/// repeated with.
const DEVICE_THREADS: &str = "4,small";

/// Runs a compiled program with its GPU systems on `device`, asking for
/// the `info` lines and giving the further words of `MIDNIGHT_GPU` in
/// `more`, and with `MIDNIGHT_THREADS` set to `threads` when that is
/// given. Fails when the program finds no such device or runs a GPU block
/// on the CPU, unless `more` has `strict-cpu` and the block is strict.
fn run_on_device(
    exe: &Path,
    device: Device,
    more: &[&str],
    threads: Option<&str>,
) -> Result<DeviceRun, String> {
    let mut command = Command::new(exe);
    if let Some(threads) = threads {
        command.env("MIDNIGHT_THREADS", threads);
    }
    let mut words = vec!["info"];
    words.extend_from_slice(more);
    if device == Device::Software {
        words.push("software");
        command
            .env("VK_DRIVER_FILES", software_driver())
            .env_remove("VK_ADD_DRIVER_FILES");
    }
    command.env("MIDNIGHT_GPU", words.join(","));
    let output = command.output().expect("run program");

    let mut run = DeviceRun {
        status: output.status.code(),
        stdout: text(&output.stdout),
        stderr: String::new(),
        device: String::new(),
        ticks: Vec::new(),
        stats: String::new(),
        strict: None,
    };
    for line in text(&output.stderr).split_inclusive('\n') {
        let Some(said) = line.strip_prefix("midnight: gpu: ") else {
            run.stderr.push_str(line);
            continue;
        };
        let said = said.trim_end();
        if let Some(counts) = said.strip_prefix("tick ") {
            match parse_stats(counts) {
                Some((tick, counts)) if tick == run.ticks.len() => {
                    run.ticks.push(counts);
                    run.stats.push_str(said);
                    run.stats.push('\n');
                }
                _ => return Err(format!("unexpected `stats` line: {said}")),
            }
        } else if said == "no device" {
            return Err(format!("no {device:?} device").to_lowercase());
        } else if said == STRICT_PASSED || said.starts_with("strict blocks run on the CPU: ") {
            run.strict = Some(said.to_string());
        } else if said.contains(" runs on the CPU: ") {
            if !(more.contains(&"strict-cpu") && said.ends_with(STRICT_REFUSED)) {
                return Err(said.to_string());
            }
        } else {
            run.device = said.to_string();
        }
    }
    if run.device.is_empty() {
        return Err(format!(
            "no `info` line\n--- stderr\n{}---",
            text(&output.stderr)
        ));
    }
    if device == Device::Software && !run.device.contains("llvmpipe") {
        return Err(format!("the device is {}, not llvmpipe", run.device));
    }
    Ok(run)
}

/// Builds a program in one mode and runs it on `device` twice: as it is,
/// and with `MIDNIGHT_GPU=rows`, which makes every row travel alone. A
/// program with a strict GPU system must pass the strict check, and runs a
/// third time with `strict-cpu`, with its strict blocks on the CPU. One
/// that `stops` must exit 1 with exactly `NAME.err`; any other must exit 0,
/// print `NAME.out` within the tolerance it states, and print nothing else
/// to standard error. Returns the device and the largest difference from
/// `NAME.out`.
fn run_gpu_program(
    dir: &Path,
    source: &Path,
    stops: bool,
    mode: Mode,
    device: Device,
) -> Result<(String, f64), String> {
    let exe = exe_path(dir, source, mode);
    build(source, &exe, mode)?;
    let read = |path: &Path| {
        std::fs::read_to_string(path).map_err(|error| {
            format!(
                "{}: can't read {}: {error}",
                source.display(),
                path.display()
            )
        })
    };
    let (status, expected_stderr) = if stops {
        (1, read(&source.with_extension("err"))?)
    } else {
        (0, String::new())
    };
    let program = read(source)?;
    let tolerance = if stops {
        None
    } else {
        stated_tolerance(&program).map_err(|message| format!("{}: {message}", source.display()))?
    };
    if tolerance.is_some() && all_strict(&program) {
        return Err(format!(
            "{}: every GPU system is strict, so the output must be exact: remove `# tolerance:`",
            source.display()
        ));
    }
    let out_path = source.with_extension("out");
    let expected = if stops {
        String::new()
    } else {
        read(&out_path)?
    };
    let strict = gpu_system_lines(&program).any(|line| line.starts_with("strict "));
    let mut runs: Vec<(&[&str], Option<&str>)> =
        vec![(&[], None), (&["rows"], None), (&[], Some(DEVICE_THREADS))];
    if strict {
        runs.push((&["strict-cpu"], None));
        runs.push((&["strict-cpu"], Some(DEVICE_THREADS)));
    }
    let mut found = (String::new(), 0.0f64);
    for (more, threads) in runs {
        let mut how = format!("{mode:?}");
        for word in more {
            how.push_str(&format!(", {word}"));
        }
        if let Some(threads) = threads {
            how.push_str(&format!(", MIDNIGHT_THREADS={threads}"));
        }
        let label = format!("{} ({how})", source.display());
        let run = run_on_device(&exe, device, more, threads)
            .map_err(|message| format!("{label}: {message}"))?;
        let strict_line = match more {
            _ if !strict => None,
            ["strict-cpu"] => Some(format!("strict blocks run on the CPU{STRICT_REFUSED}")),
            _ => Some(STRICT_PASSED.to_string()),
        };
        if run.strict != strict_line {
            return Err(format!(
                "{label}: the `info` line about strict blocks is {:?}, expected {strict_line:?}",
                run.strict
            ));
        }
        if run.status != Some(status) {
            return Err(format!(
                "{label}: exited with {:?}, expected {status}\n--- stdout\n{}--- stderr\n{}---",
                run.status, run.stdout, run.stderr
            ));
        }
        if run.stderr != expected_stderr {
            return Err(format!(
                "{label}: standard error differs\n--- expected\n{expected_stderr}--- actual\n{}---",
                run.stderr
            ));
        }
        found.0 = run.device;
        if stops {
            continue;
        }
        match compare_output(&expected, &run.stdout, tolerance) {
            Ok(difference) => found.1 = found.1.max(difference),
            Err(message) => {
                return Err(format!(
                    "{label}: standard output differs from {}: {message}\n\
                     --- expected\n{expected}--- actual\n{}---",
                    out_path.display(),
                    run.stdout
                ));
            }
        }
    }
    Ok(found)
}

#[test]
fn gpu_programs() {
    let Some(device) = Device::from_env() else {
        return;
    };
    let groups = [
        ("gpu-run", programs("tests/run"), false),
        ("gpu-examples", examples(), false),
        ("gpu-panic", programs("tests/panic"), true),
    ];
    let mut dirs = Vec::new();
    let mut jobs = Vec::new();
    for (name, sources, stops) in &groups {
        dirs.push(scratch(name));
        for source in sources.iter().filter(|source| declares_gpu_system(source)) {
            for mode in &MODES {
                jobs.push((dirs.len() - 1, source, *stops, *mode));
            }
        }
    }
    assert!(
        !jobs.is_empty(),
        "no program with a GPU system under tests/ and examples/ of the working directory"
    );
    let devices = Mutex::new(BTreeSet::new());
    let inexact = Mutex::new(BTreeSet::new());
    let failures = for_each_parallel(&jobs, |(dir, source, stops, mode)| {
        let (name, difference) = run_gpu_program(&dirs[*dir], source, *stops, *mode, device)?;
        devices.lock().unwrap().insert(name);
        if difference > 0.0 {
            inexact.lock().unwrap().insert(format!(
                "gpu_programs: {} ({mode:?}) is {difference:.6} from the expected output, within its tolerance",
                source.display()
            ));
        }
        Ok(())
    });
    for dir in &dirs {
        let _ = std::fs::remove_dir_all(dir);
    }
    report("runs on a device", jobs.len(), failures);
    let devices: Vec<String> = devices.into_inner().unwrap().into_iter().collect();
    note(&format!(
        "gpu_programs: {} builds of {} programs, each run as it is, with rows and with MIDNIGHT_THREADS={DEVICE_THREADS}, and strict ones with strict-cpu, on {}",
        jobs.len(),
        jobs.len() / MODES.len(),
        devices.join(" and ")
    ));
    for line in inexact.into_inner().unwrap() {
        note(&line);
    }
}

/// A strict block whose shader has `s32_div` and `s32_sqrt`.
const STRICT_DIVISION: &str = "\
component Body
  x: Float32
end

strict gpu system exact
  each Body do |b|
    b.x = Math.sqrt(b.x) / b.x
  end
end
";

/// The functions `s32_div` and `s32_sqrt` of a listing of shaders, each
/// once.
fn strict_helpers(wgsl: &str) -> String {
    let mut out = String::new();
    let mut inside = false;
    let mut seen = BTreeSet::new();
    for line in wgsl.lines() {
        if (line.starts_with("fn s32_div(") || line.starts_with("fn s32_sqrt("))
            && seen.insert(line)
        {
            inside = true;
        }
        if inside {
            out.push_str(line);
            out.push('\n');
            inside = line != "}";
        }
    }
    out
}

/// The `/` and `sqrt` of strict shaders give on the device exactly what the
/// CPU's give with subnormals flushed: `tests/device/strict_div_sqrt.c` runs
/// the functions `midnight emit-wgsl` prints. On hardware, `sqrt` of every
/// bit pattern and `/` of 2^28 pairs; on llvmpipe 2^24 of each. Edge values
/// and quotients next to a midpoint are among them either way.
#[test]
fn gpu_strict_div_sqrt() {
    let Some(device) = Device::from_env() else {
        return;
    };
    let dir = scratch("gpu-strict-div-sqrt");
    let program = dir.join("division.12am");
    std::fs::write(&program, STRICT_DIVISION).expect("write a program");
    let output = midnight()
        .arg("emit-wgsl")
        .arg(&program)
        .output()
        .expect("run midnight");
    assert!(output.status.success(), "{}", text(&output.stderr));
    let helpers = strict_helpers(&text(&output.stdout));
    assert!(
        helpers.contains("fn s32_div(") && helpers.contains("fn s32_sqrt("),
        "the shader has no `s32_div` or `s32_sqrt`:\n{}",
        text(&output.stdout)
    );
    let helpers_path = dir.join("helpers.wgsl");
    std::fs::write(&helpers_path, helpers).expect("write the helpers");

    let exe = dir.join("strict_div_sqrt");
    let flags = std::env::var("MIDNIGHT_GPU_FLAGS")
        .expect("GPU mode needs MIDNIGHT_GPU_FLAGS, which the dev shell and the flake set");
    let mut args: Vec<OsString> = vec!["-std=c11".into(), "-O2".into()];
    args.extend(STRICT_CFLAGS.split_whitespace().map(OsString::from));
    args.extend(flags.split_whitespace().map(OsString::from));
    args.extend([
        "tests/device/strict_div_sqrt.c".into(),
        "-o".into(),
        exe.clone().into_os_string(),
        "-lm".into(),
    ]);
    let args: Vec<&OsStr> = args.iter().map(OsString::as_os_str).collect();
    cc(&args);

    let (stride, pairs) = match device {
        Device::Hardware => ("1", 1u64 << 28),
        Device::Software => ("256", 1u64 << 24),
    };
    let mut command = Command::new(&exe);
    command
        .arg(&helpers_path)
        .arg(stride)
        .arg(pairs.to_string());
    if device == Device::Software {
        command
            .env("VK_DRIVER_FILES", software_driver())
            .env_remove("VK_ADD_DRIVER_FILES");
    }
    let output = command.output().expect("run the check");
    let _ = std::fs::remove_dir_all(&dir);
    let stdout = text(&output.stdout);
    assert!(
        output.status.success(),
        "strict `/` and `sqrt` on the device:\n{stdout}{}",
        text(&output.stderr)
    );
    note(&format!(
        "gpu_strict_div_sqrt: {}",
        stdout.lines().collect::<Vec<_>>().join("; ")
    ));
}

#[test]
fn gpu_stats_match_plan() {
    let Some(device) = Device::from_env() else {
        return;
    };
    let source = Path::new(STATS_PROGRAM);
    let output = midnight()
        .arg("plan")
        .arg(source)
        .output()
        .expect("run midnight");
    assert!(output.status.success(), "{}", text(&output.stderr));
    let plan = text(&output.stdout);
    let mut planned = (0, 0);
    for line in plan.lines() {
        assert!(
            !line.contains("   on ") && !line.ends_with(" rows"),
            "{STATS_PROGRAM}: the plan has a transfer that is not a whole array in every tick\n{plan}"
        );
        match line.split_whitespace().next() {
            Some("upload") => planned.0 += 1,
            Some("download") => planned.1 += 1,
            _ => panic!("{STATS_PROGRAM}: unexpected line in the plan\n{plan}"),
        }
    }
    assert!(planned != (0, 0), "{STATS_PROGRAM}: the plan is empty");

    let dir = scratch("gpu-stats");
    let devices = Mutex::new(BTreeSet::new());
    let failures = for_each_parallel(&MODES, |mode| {
        let exe = exe_path(&dir, source, *mode);
        build(source, &exe, *mode)?;
        for threads in [None, Some(DEVICE_THREADS)] {
            let label = match threads {
                None => format!("{STATS_PROGRAM} ({mode:?})"),
                Some(threads) => {
                    format!("{STATS_PROGRAM} ({mode:?}, MIDNIGHT_THREADS={threads})")
                }
            };
            let run = run_on_device(&exe, device, &["stats"], threads)
                .map_err(|message| format!("{label}: {message}"))?;
            if run.status != Some(0) || run.ticks.len() < 3 {
                return Err(format!(
                    "{label}: exited with {:?} after {} ticks\n--- stderr\n{}---",
                    run.status,
                    run.ticks.len(),
                    run.stderr
                ));
            }
            // The last line also counts the top-level code after the loop.
            let last = run.ticks.len() - 1;
            for (tick, counts) in run.ticks.iter().enumerate().take(last).skip(1) {
                if (counts.uploads, counts.downloads) != planned {
                    return Err(format!(
                        "{label}: tick {tick} made {} uploads and {} downloads; \
                         the plan lists {} and {}\n{plan}",
                        counts.uploads, counts.downloads, planned.0, planned.1
                    ));
                }
                if (counts.rows_up, counts.rows_down) != (0, 0) {
                    return Err(format!(
                        "{label}: tick {tick} moved {} rows up and {} rows down; \
                         the plan has no `rows` line\n{plan}",
                        counts.rows_up, counts.rows_down
                    ));
                }
            }
            devices.lock().unwrap().insert(run.device);
        }
        Ok(())
    });
    let _ = std::fs::remove_dir_all(&dir);
    report("runs with stats", MODES.len(), failures);
    let devices: Vec<String> = devices.into_inner().unwrap().into_iter().collect();
    note(&format!(
        "gpu_stats_match_plan: uploads {} and downloads {} in every tick but the first and the last on {}",
        planned.0,
        planned.1,
        devices.join(" and ")
    ));
}

/// The programs of `tests/run` that have a `NAME.stats` file.
fn stats_programs() -> Vec<PathBuf> {
    programs("tests/run")
        .into_iter()
        .filter(|source| source.with_extension("stats").exists())
        .collect()
}

#[test]
fn gpu_stats_match_expected() {
    let Some(device) = Device::from_env() else {
        return;
    };
    let sources = stats_programs();
    assert!(
        !sources.is_empty(),
        "no program with a .stats file under tests/run of the working directory"
    );
    let dir = scratch("gpu-stats-expected");
    let jobs: Vec<(&PathBuf, Mode)> = sources
        .iter()
        .flat_map(|source| MODES.map(|mode| (source, mode)))
        .collect();
    let devices: Mutex<BTreeSet<String>> = Mutex::new(BTreeSet::new());
    let failures = for_each_parallel(&jobs, |(source, mode)| {
        let exe = exe_path(&dir, source, *mode);
        build(source, &exe, *mode)?;
        let threads = format!(", MIDNIGHT_THREADS={DEVICE_THREADS}");
        let runs = [
            (vec!["stats"], "stats", "", None),
            (vec!["stats", "rows"], "rows.stats", ", rows", None),
            (
                vec!["stats"],
                "stats",
                threads.as_str(),
                Some(DEVICE_THREADS),
            ),
        ];
        for (words, extension, how, threads) in runs {
            let expected = source.with_extension(extension);
            if !expected.exists() {
                continue;
            }
            let label = format!("{} ({mode:?}{how})", source.display());
            let run = run_on_device(&exe, device, &words, threads)
                .map_err(|message| format!("{label}: {message}"))?;
            if run.status != Some(0) || !run.stderr.is_empty() {
                return Err(format!(
                    "{label}: exited with {:?}\n--- stderr\n{}---",
                    run.status, run.stderr
                ));
            }
            devices.lock().unwrap().insert(run.device);
            // Only the release run may write the expected file; debug must
            // match it.
            if update_expect() && *mode == Mode::Debug {
                continue;
            }
            expect(&expected, &run.stats, "the `stats` lines")
                .map_err(|message| format!("{label}: {message}"))?;
        }
        Ok(())
    });
    let _ = std::fs::remove_dir_all(&dir);
    report("runs with stats", jobs.len(), failures);
    let devices: Vec<String> = devices.into_inner().unwrap().into_iter().collect();
    note(&format!(
        "gpu_stats_match_expected: {} runs of {} programs on {}",
        jobs.len(),
        sources.len(),
        devices.join(" and ")
    ));
}

#[test]
fn floats_are_split_from_the_text_around_them() {
    assert_eq!(
        split_floats("Vec2(x: 7.5, y: -7.75)"),
        (vec!["Vec2(x: ", ", y: ", ")"], vec!["7.5", "-7.75"])
    );
    assert_eq!(
        split_floats("1e+20 1.5e-07 2.5e3"),
        (vec!["", " ", " ", ""], vec!["1e+20", "1.5e-07", "2.5e3"])
    );
    assert_eq!(
        split_floats("3-1.5 took 0.25s"),
        (vec!["3-", " took ", "s"], vec!["1.5", "0.25"])
    );
    // Whole numbers, digits in a name and words are text.
    for text in ["7 -12 2236", "Float32 x1.5 v_2.5", "NaN -Infinity", "1. e5"] {
        assert_eq!(split_floats(text), (vec![text], vec![]));
    }
}

#[test]
fn equal_output_matches_with_and_without_a_tolerance() {
    let text = "Ints(power: 2236)\n2.5 -7.0\n";
    assert_eq!(compare_output(text, text, None), Ok(0.0));
    assert_eq!(compare_output(text, text, Some(0.002)), Ok(0.0));
    assert_eq!(compare_output("", "", None), Ok(0.0));
}

#[test]
fn without_a_tolerance_floats_must_be_equal() {
    assert!(compare_output("1.25\n", "1.251\n", None).is_err());
    assert!(compare_output("0.0\n", "-0.0\n", None).is_err());
}

#[test]
fn floats_within_the_tolerance_match() {
    let within = |expected, actual| compare_output(expected, actual, Some(0.002));
    assert!(within("0.02 0.011\n", "0.021 0.009\n").is_ok());
    assert!(within("x: 0.745, y: -0.359\n", "x: 0.746, y: -0.36\n").is_ok());
    assert!(within("0.0\n", "-0.0\n").is_ok());
    assert!(within("0.001\n", "-0.001\n").is_ok());
    assert!(within("1.5e-07\n", "0.0\n").is_ok());
    assert!(within("0.02\n", "0.023\n").is_err());
    assert!(within("-1.279\n", "1.279\n").is_err());
    assert!(within("0.5 0.5\n", "0.5 0.503\n").is_err());
}

#[test]
fn the_tolerance_grows_with_the_expected_number() {
    let within = |expected, actual| compare_output(expected, actual, Some(0.002));
    assert!(within("250.0\n", "250.4\n").is_ok());
    assert!(within("250.0\n", "250.6\n").is_err());
    assert!(within("-250.0\n", "-250.4\n").is_ok());
    assert!(within("1e+20\n", "1.001e+20\n").is_ok());
    assert!(within("1e+20\n", "1.01e+20\n").is_err());
}

#[test]
fn the_largest_difference_is_returned() {
    let difference = compare_output("1.0 4.0\n0.5\n", "1.0 4.004\n0.5005\n", Some(0.002)).unwrap();
    assert!((difference - 0.001).abs() < 1e-9, "{difference}");
}

#[test]
fn everything_but_floats_must_be_equal_within_a_tolerance() {
    let within = |expected, actual| compare_output(expected, actual, Some(0.02));
    // Whole numbers.
    assert!(within("power: 531585\n", "power: 531586\n").is_err());
    assert!(within("-3 1\n", "3 1\n").is_err());
    // A whole number against a `Float`.
    assert!(within("2.0\n", "2\n").is_err());
    // Words, names and spacing.
    assert!(within("true 1.5\n", "false 1.5\n").is_err());
    assert!(within("Vec2(x: 1.0)\n", "Vec3(x: 1.0)\n").is_err());
    assert!(within("1.0 2.0\n", "1.0  2.0\n").is_err());
    assert!(within("NaN\n", "0.0\n").is_err());
    assert!(within("Infinity\n", "-Infinity\n").is_err());
}

#[test]
fn the_count_of_floats_and_of_lines_must_be_equal() {
    let within = |expected, actual| compare_output(expected, actual, Some(0.02));
    assert!(within("1.0 2.0\n", "1.0\n").is_err());
    assert!(within("1.0\n", "1.0 2.0\n").is_err());
    assert_eq!(
        within("1.0\n2.0\n", "1.0\n"),
        Err("1 lines, expected 2".to_string())
    );
    assert_eq!(
        within("1.0\n", "1.0\n2.0\n"),
        Err("2 lines, expected 1".to_string())
    );
    assert!(within("1.0\n", "1.0").is_err());
    assert!(within("1.0\n", "1.0\n\n").is_err());
    assert!(compare_output("a\n", "a\nb\n", None).is_err());
}

#[test]
fn a_difference_names_its_line() {
    assert_eq!(
        compare_output("a\n1.0\n", "a\n1.5\n", Some(0.002)),
        Err("line 2: 1.5 is not within 0.002 of 1.0".to_string())
    );
    assert_eq!(
        compare_output("a\nb\n", "a\nc\n", None),
        Err("line 2: expected `b`, got `c`".to_string())
    );
}

#[test]
fn a_program_states_its_tolerance_in_a_comment() {
    let program = "# tolerance: 0.002\n# Gravity.\ncomponent Body\nend\n";
    assert_eq!(stated_tolerance(program), Ok(Some(0.002)));
    assert_eq!(stated_tolerance("# Gravity.\nputs 1\n"), Ok(None));
    assert!(stated_tolerance("# tolerance: small\n").is_err());
    assert!(stated_tolerance("# tolerance: -1\n").is_err());
    assert!(stated_tolerance("# tolerance: 0\n").is_err());
}

/// A library the stand-in driver opens by name.
const PROBE_PART: &str = "int mn_probe_part(void) { return 1; }\n";

/// A Vulkan driver that offers nothing. When it is loaded it opens another
/// library by name, as real drivers open their other parts, and says on
/// standard error whether the dynamic loader found it.
const PROBE_DRIVER: &str = r#"#include <dlfcn.h>
#include <stdio.h>

__attribute__((constructor)) static void probe(void) {
    void *part = dlopen("libmn_probe_part.so", RTLD_NOW);
    fprintf(stderr, "probe driver: part %s\n", part ? "found" : "missing");
}

void *vk_icdGetInstanceProcAddr(void *instance, const char *name);
void *vk_icdGetInstanceProcAddr(void *instance, const char *name) {
    (void)instance;
    (void)name;
    return 0;
}
"#;

/// Runs the C compiler that programs are built with.
fn cc(args: &[&OsStr]) {
    let cc = std::env::var_os("MIDNIGHT_CC").unwrap_or_else(|| "cc".into());
    let output = Command::new(&cc)
        .args(args)
        .output()
        .expect("run the C compiler");
    assert!(output.status.success(), "{}", text(&output.stderr));
}

/// A GPU driver finds the libraries it opens by name, in a release and in
/// a debug build. The dynamic loader looks for such a library in the
/// runpath of the library that asked, and a debug build must not put
/// AddressSanitizer's `dlopen` between the two. The driver is a stand-in
/// that the Vulkan loader loads beside the real one, so the test needs no
/// particular device.
#[test]
fn gpu_driver_finds_its_parts() {
    let Some(device) = Device::from_env() else {
        return;
    };
    if !cfg!(target_os = "linux") {
        return;
    }
    let dir = scratch("gpu-driver");
    let parts = dir.join("parts");
    std::fs::create_dir_all(&parts).expect("create a directory");
    let part_source = dir.join("part.c");
    let driver_source = dir.join("driver.c");
    let driver = dir.join("libmn_probe_driver.so");
    std::fs::write(&part_source, PROBE_PART).expect("write a source file");
    std::fs::write(&driver_source, PROBE_DRIVER).expect("write a source file");
    cc(&[
        "-shared".as_ref(),
        "-fPIC".as_ref(),
        part_source.as_os_str(),
        "-o".as_ref(),
        parts.join("libmn_probe_part.so").as_os_str(),
    ]);
    // The part is found through the driver's runpath and no other way.
    let mut runpath = OsString::from("-Wl,--enable-new-dtags,-rpath,");
    runpath.push(&parts);
    cc(&[
        "-shared".as_ref(),
        "-fPIC".as_ref(),
        driver_source.as_os_str(),
        "-o".as_ref(),
        driver.as_os_str(),
        &runpath,
    ]);
    let manifest = dir.join("probe.json");
    std::fs::write(
        &manifest,
        format!(
            "{{\"file_format_version\": \"1.0.0\", \
             \"ICD\": {{\"library_path\": {:?}, \"api_version\": \"1.3.0\"}}}}\n",
            driver.to_string_lossy()
        ),
    )
    .expect("write the driver's manifest");

    let source = Path::new(STATS_PROGRAM);
    let failures = for_each_parallel(&MODES, |mode| {
        let exe = exe_path(&dir, source, *mode);
        build(source, &exe, *mode)?;
        let mut command = Command::new(&exe);
        match device {
            Device::Software => {
                let mut drivers = OsString::from(&manifest);
                drivers.push(":");
                drivers.push(software_driver());
                command
                    .env("VK_DRIVER_FILES", drivers)
                    .env_remove("VK_ADD_DRIVER_FILES")
                    .env("MIDNIGHT_GPU", "software");
            }
            Device::Hardware => {
                // The loader ignores `VK_ADD_DRIVER_FILES` while
                // `VK_DRIVER_FILES` is set.
                match std::env::var_os("VK_DRIVER_FILES") {
                    Some(chosen) => {
                        let mut drivers = OsString::from(&manifest);
                        drivers.push(":");
                        drivers.push(chosen);
                        command.env("VK_DRIVER_FILES", drivers);
                    }
                    None => {
                        command.env("VK_ADD_DRIVER_FILES", &manifest);
                    }
                }
                command.env_remove("MIDNIGHT_GPU");
            }
        }
        let output = command.output().expect("run program");
        let stderr = text(&output.stderr);
        let label = format!("{STATS_PROGRAM} ({mode:?})");
        if !output.status.success() {
            return Err(format!(
                "{label}: exited with {:?}\n--- stderr\n{stderr}---",
                output.status.code()
            ));
        }
        if stderr.contains("probe driver: part missing") {
            return Err(format!(
                "{label}: a driver could not open its own library by name\n--- stderr\n{stderr}---"
            ));
        }
        if !stderr.contains("probe driver: part found") {
            return Err(format!(
                "{label}: the stand-in driver was not loaded\n--- stderr\n{stderr}---"
            ));
        }
        Ok(())
    });
    report("runs with a stand-in driver", MODES.len(), failures);
}

#[test]
fn stats_lines_are_read() {
    let counts = |numbers: [u64; 6]| Counts {
        uploads: numbers[0],
        downloads: numbers[1],
        rows_up: numbers[2],
        rows_down: numbers[3],
        submissions: numbers[4],
        waits: numbers[5],
    };
    assert_eq!(
        parse_stats("0: 3 uploads, 1 download, 2 rows up, 1 row down, 2 submissions, 1 wait"),
        Some((0, counts([3, 1, 2, 1, 2, 1])))
    );
    assert_eq!(
        parse_stats("12: 1 upload, 0 downloads, 0 rows up, 0 rows down, 1 submission, 0 waits"),
        Some((12, counts([1, 0, 0, 0, 1, 0])))
    );
    assert_eq!(parse_stats("0: 3 uploads, 1 download"), None);
    assert_eq!(
        parse_stats("0: 1 download, 3 uploads, 0 rows up, 0 rows down, 1 submission, 1 wait"),
        None
    );
    assert_eq!(
        parse_stats("0: 3 uploads, 1 download, 2 rows down, 1 row up, 2 submissions, 1 wait"),
        None
    );
    assert_eq!(
        parse_stats("0: 3 uploads, 1 download, 2 rows up, 1 row down, 2 submissions, 1 wait, 4"),
        None
    );
}

/// What a program did within its time limit.
struct TimedRun {
    /// `None` when the program was stopped by a signal or by the limit.
    status: Option<i32>,
    stdout: String,
    stderr: String,
    /// The program was still running when the limit passed, and was killed.
    timed_out: bool,
}

/// Runs a command, and kills it when it has not ended after `limit`.
fn run_within(command: &mut Command, limit: Duration) -> TimedRun {
    use std::io::Read;
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("run program");
    let mut stdout = child.stdout.take().expect("piped");
    let mut stderr = child.stderr.take().expect("piped");
    std::thread::scope(|scope| {
        let read_out = scope.spawn(move || {
            let mut bytes = Vec::new();
            let _ = stdout.read_to_end(&mut bytes);
            bytes
        });
        let read_err = scope.spawn(move || {
            let mut bytes = Vec::new();
            let _ = stderr.read_to_end(&mut bytes);
            bytes
        });
        let started = Instant::now();
        let mut timed_out = false;
        let status = loop {
            if let Some(status) = child.try_wait().expect("wait for program") {
                break status;
            }
            if started.elapsed() >= limit {
                timed_out = true;
                let _ = child.kill();
                break child.wait().expect("wait for program");
            }
            std::thread::sleep(Duration::from_millis(2));
        };
        TimedRun {
            status: if timed_out { None } else { status.code() },
            stdout: text(&read_out.join().expect("read standard output")),
            stderr: text(&read_err.join().expect("read standard error")),
            timed_out,
        }
    })
}

/// Whether `MIDNIGHT_TEST_TSAN` asks for the ThreadSanitizer tests.
fn tsan_mode() -> bool {
    std::env::var_os("MIDNIGHT_TEST_TSAN").is_some_and(|value| !value.is_empty())
}

/// How long a run of the pool's stress test may take.
const STRESS_LIMIT: Duration = Duration::from_secs(300);

/// Builds `tests/pool/stress.c` with `runtime/midnight_pool.c`, with
/// ThreadSanitizer when `sanitize`, and runs each of its modes, one after
/// another, plain and with further words. A mode that ends well prints
/// `ok`; `fail` and `hang` must name piece 400 and `swarm` piece 5, the
/// earliest failing one, and exit with status 3. Nothing may be written to
/// standard error, where ThreadSanitizer reports.
fn pool_stress_runs(name: &str, sanitize: bool) {
    let dir = scratch(name);
    let exe = dir.join("stress");
    let mut args: Vec<OsString> = ["-std=c11", "-O2", "-pthread", "-Iruntime"]
        .iter()
        .map(OsString::from)
        .collect();
    args.extend(STRICT_CFLAGS.split_whitespace().map(OsString::from));
    if sanitize {
        args.extend(["-g".into(), "-fsanitize=thread".into()]);
    }
    args.extend([
        "tests/pool/stress.c".into(),
        "runtime/midnight_pool.c".into(),
        "-o".into(),
        exe.clone().into_os_string(),
    ]);
    let args: Vec<&OsStr> = args.iter().map(OsString::as_os_str).collect();
    cc(&args);

    let passing = [
        ("back", ""),
        ("back", "8,reverse"),
        ("idle", ""),
        ("idle", "4,awake"),
        ("crowd", ""),
        ("crowd", "jitter"),
        ("one", ""),
        ("restart", ""),
        ("restart", "5,reverse,jitter"),
    ];
    let failing = [
        "4",
        "8,reverse",
        "3,jitter",
        "2,awake",
        "1",
        "16,reverse,jitter",
        "64",
    ];
    let mut runs: Vec<(&str, &str, &str, i32)> = passing
        .iter()
        .map(|(mode, words)| (*mode, *words, "ok\n", 0))
        .collect();
    for mode in ["fail", "hang"] {
        runs.extend(
            failing
                .iter()
                .map(|words| (mode, *words, "failed 400\n", 3)),
        );
    }
    for words in [
        "64,jitter",
        "64,jitter",
        "48,reverse,jitter",
        "32",
        "7,awake",
    ] {
        runs.push(("swarm", words, "failed 5\n", 3));
    }
    let mut failures = Vec::new();
    for (mode, words, stdout, status) in &runs {
        let mut command = Command::new(&exe);
        command.arg(mode);
        if !words.is_empty() {
            command.arg(words);
        }
        let run = run_within(&mut command, STRESS_LIMIT);
        if run.timed_out
            || run.status != Some(*status)
            || run.stdout != *stdout
            || !run.stderr.is_empty()
        {
            failures.push(format!(
                "stress {mode} {words}: {}, expected status {status} and `{}`\n\
                 --- stdout\n{}--- stderr\n{}---",
                if run.timed_out {
                    format!("still running after {} s", STRESS_LIMIT.as_secs())
                } else {
                    format!("exited with {:?}", run.status)
                },
                stdout.trim_end(),
                run.stdout,
                run.stderr
            ));
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    report("runs of the pool's stress test", runs.len(), failures);
}

/// The thread pool on its own.
#[test]
fn pool_stress() {
    pool_stress_runs("pool-stress", false);
}

/// The thread pool on its own, under ThreadSanitizer.
#[test]
fn tsan_pool_stress() {
    if !tsan_mode() {
        return;
    }
    pool_stress_runs("tsan-pool-stress", true);
    note("tsan_pool_stress: every mode of tests/pool/stress.c, with no report");
}

/// The settings of `MIDNIGHT_THREADS` that every program with a loop that
/// can be split runs under.
const THREAD_SETTINGS: [&str; 6] = [
    "1",
    "4",
    "3,small",
    "7,small,reverse",
    "64,small,jitter",
    "2,small,awake",
];

/// The settings the ThreadSanitizer builds run under.
const TSAN_SETTINGS: [&str; 2] = ["4,small", "7,small,reverse"];

/// How long a run of a program that must stop with an error may take.
const PANIC_LIMIT: Duration = Duration::from_secs(10);
/// How long any other run of a test program may take.
const RUN_LIMIT: Duration = Duration::from_secs(300);

/// Whether a program has a loop that can be split across threads:
/// `midnight loops` says `gpu` or `cores` of one.
fn can_split(source: &Path) -> bool {
    let output = midnight()
        .arg("loops")
        .arg(source)
        .output()
        .expect("run midnight");
    output.status.success()
        && text(&output.stdout)
            .lines()
            .any(|line| line.ends_with("  gpu") || line.ends_with("  cores"))
}

/// The programs of `tests/run`, `examples` and `tests/panic` that have a
/// loop that can be split, each with whether it must stop with an error.
fn split_programs() -> Vec<(PathBuf, bool)> {
    let mut found = Vec::new();
    for (sources, stops) in [
        (programs("tests/run"), false),
        (examples(), false),
        (programs("tests/panic"), true),
    ] {
        found.extend(
            sources
                .into_iter()
                .filter(|source| can_split(source))
                .map(|source| (source, stops)),
        );
    }
    found
}

/// Runs a built program once for each setting of `MIDNIGHT_THREADS`, with
/// no GPU device. A program that `stops` must exit 1 within ten seconds
/// with exactly `NAME.err`, and print what it prints under the first
/// setting; any other must exit 0, print exactly `NAME.out` and write
/// nothing to standard error.
fn run_with_threads(
    exe: &Path,
    source: &Path,
    stops: bool,
    how: &str,
    settings: &[&str],
) -> Result<(), String> {
    let read = |extension: &str| {
        let path = source.with_extension(extension);
        std::fs::read_to_string(&path).map_err(|error| {
            format!(
                "{}: can't read {}: {error}",
                source.display(),
                path.display()
            )
        })
    };
    let (status, expected_stderr, limit) = if stops {
        (1, read("err")?, PANIC_LIMIT)
    } else {
        (0, String::new(), RUN_LIMIT)
    };
    let mut expected_stdout = if stops { None } else { Some(read("out")?) };
    for setting in settings {
        let label = format!("{} ({how}, MIDNIGHT_THREADS={setting})", source.display());
        let mut command = on_cpu(exe);
        command.env("MIDNIGHT_THREADS", setting);
        let run = run_within(&mut command, limit);
        if run.timed_out {
            return Err(format!(
                "{label}: still running after {} s\n--- stdout\n{}--- stderr\n{}---",
                limit.as_secs(),
                run.stdout,
                run.stderr
            ));
        }
        if run.status != Some(status) {
            return Err(format!(
                "{label}: exited with {:?}, expected {status}\n--- stdout\n{}--- stderr\n{}---",
                run.status, run.stdout, run.stderr
            ));
        }
        if run.stderr != expected_stderr {
            return Err(format!(
                "{label}: standard error differs\n--- expected\n{expected_stderr}--- actual\n{}---",
                run.stderr
            ));
        }
        match &expected_stdout {
            Some(expected) if *expected != run.stdout => {
                return Err(format!(
                    "{label}: standard output differs\n--- expected\n{expected}--- actual\n{}---",
                    run.stdout
                ));
            }
            Some(_) => {}
            None => expected_stdout = Some(run.stdout),
        }
    }
    Ok(())
}

/// What a program says on standard error with `info` in `MIDNIGHT_THREADS`
/// starts with.
const THREADS_SAID: &str = "midnight: threads: ";

/// Every program of `tests/run` with a `NAME.split` file, run with
/// `MIDNIGHT_THREADS=4,small,info`, says it has four threads and then
/// exactly the lines of that file, one for each loop the first time it is
/// split: so a loop that is meant to run in pieces and does not fails
/// here. With `UPDATE_EXPECT=1` the release run writes the file.
#[test]
fn splits_match_expected() {
    let sources: Vec<PathBuf> = programs("tests/run")
        .into_iter()
        .filter(|source| source.with_extension("split").exists())
        .collect();
    assert!(
        !sources.is_empty(),
        "no program with a .split file under tests/run of the working directory"
    );
    let dir = scratch("splits");
    let jobs: Vec<(&PathBuf, Mode)> = sources
        .iter()
        .flat_map(|source| MODES.map(|mode| (source, mode)))
        .collect();
    let failures = for_each_parallel(&jobs, |(source, mode)| {
        let exe = exe_path(&dir, source, *mode);
        build(source, &exe, *mode)?;
        let label = format!("{} ({mode:?})", source.display());
        let mut command = on_cpu(&exe);
        command.env("MIDNIGHT_THREADS", "4,small,info");
        let run = run_within(&mut command, RUN_LIMIT);
        let said = run.stderr.strip_prefix(&format!("{THREADS_SAID}4\n"));
        let lines: Option<String> = said.and_then(|said| {
            said.split_inclusive('\n')
                .map(|line| line.strip_prefix(THREADS_SAID))
                .collect()
        });
        let (Some(0), Some(lines)) = (run.status, lines) else {
            return Err(format!(
                "{label}: exited with {:?}\n--- stderr\n{}---",
                run.status, run.stderr
            ));
        };
        let expected = std::fs::read_to_string(source.with_extension("out"))
            .map_err(|error| format!("{label}: can't read the expected output: {error}"))?;
        if run.stdout != expected {
            return Err(format!(
                "{label}: standard output differs\n--- expected\n{expected}--- actual\n{}---",
                run.stdout
            ));
        }
        if update_expect() && *mode == Mode::Debug {
            return Ok(());
        }
        expect(
            &source.with_extension("split"),
            &lines,
            "the lines about split loops",
        )
        .map_err(|message| format!("{label}: {message}"))
    });
    let _ = std::fs::remove_dir_all(&dir);
    report("runs that list their split loops", jobs.len(), failures);
}

/// Every program with a loop that can be split gives its expected output
/// for every setting of `MIDNIGHT_THREADS`, as a release and as a debug
/// build.
#[test]
fn thread_programs() {
    let sources = split_programs();
    assert!(
        !sources.is_empty(),
        "no program with a loop that can be split under tests/ and examples/ of the working directory"
    );
    let dir = scratch("threads");
    let jobs: Vec<(usize, &(PathBuf, bool), Mode)> = sources
        .iter()
        .enumerate()
        .flat_map(|(index, source)| MODES.map(|mode| (index, source, mode)))
        .collect();
    let failures = for_each_parallel(&jobs, |(index, (source, stops), mode)| {
        let stem = source.file_stem().unwrap().to_string_lossy();
        let exe = dir.join(format!("{index}-{stem}-{mode:?}"));
        build(source, &exe, *mode)?;
        run_with_threads(&exe, source, *stops, &format!("{mode:?}"), &THREAD_SETTINGS)
    });
    let _ = std::fs::remove_dir_all(&dir);
    report("programs under several thread counts", jobs.len(), failures);
}

/// Every program with a loop that can be split, built in release mode with
/// ThreadSanitizer, gives its expected output with no report.
#[test]
fn tsan_programs() {
    if !tsan_mode() {
        return;
    }
    let sources = split_programs();
    assert!(
        !sources.is_empty(),
        "no program with a loop that can be split under tests/ and examples/ of the working directory"
    );
    let dir = scratch("tsan");
    let jobs: Vec<(usize, &(PathBuf, bool))> = sources.iter().enumerate().collect();
    let failures = for_each_parallel(&jobs, |(index, (source, stops))| {
        let stem = source.file_stem().unwrap().to_string_lossy();
        let exe = dir.join(format!("{index}-{stem}"));
        let output = midnight()
            .arg("build")
            .arg(source)
            .arg("-o")
            .arg(&exe)
            .env(
                "MIDNIGHT_CFLAGS",
                format!("{STRICT_CFLAGS} -g -fsanitize=thread"),
            )
            .output()
            .expect("run midnight");
        if !output.status.success() {
            return Err(format!(
                "{} (ThreadSanitizer): build failed\n{}",
                source.display(),
                text(&output.stderr)
            ));
        }
        run_with_threads(&exe, source, *stops, "ThreadSanitizer", &TSAN_SETTINGS)
    });
    let _ = std::fs::remove_dir_all(&dir);
    report("programs under ThreadSanitizer", jobs.len(), failures);
    note(&format!(
        "tsan_programs: {} programs, each with MIDNIGHT_THREADS={}, with no report",
        jobs.len(),
        TSAN_SETTINGS.join(" and ")
    ));
}
