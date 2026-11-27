//! End-to-end tests: every program under `tests/` and `examples/` is
//! compiled by the `midnight` binary and its behavior compared with the
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
//! `gpu_stats_match_expected`, `gpu_driver_finds_its_parts`,
//! `gpu_strict_div_sqrt` and `window_frames`) do nothing unless
//! `MIDNIGHT_TEST_GPU` names where GPU systems and frames are to run:
//!
//! - `software`: on Mesa's `llvmpipe`. `MIDNIGHT_TEST_SOFTWARE_ICD` is the
//!   path of its Vulkan driver file.
//! - `hardware`: on the device the Vulkan loader finds, which a
//!   `VK_DRIVER_FILES` the caller sets chooses.
//!
//! Either way every program runs as a release and as a debug build.
//!
//! `gpu_programs` runs every program of `tests/run`, `tests/panic` and
//! `examples`, and every eval task's `solution.12am`, that declares a GPU
//! system, as it is and again with
//! `MIDNIGHT_GPU=rows`. Standard output must match
//! `NAME.out`: exactly, or, when the program has a `# tolerance: T` line,
//! with each `Float` within `T * max(1, |expected|)` of the expected one and
//! everything else exact. A program whose GPU systems are all strict may not
//! have the line. The answers to its `draws` commands must be `NAME.draws`
//! when it has one: exactly, or, with the line, each number of a shape
//! within the same bound and everything else exact. A `tests/panic`
//! program must exit 1 with exactly `NAME.err`. A run in which there is no
//! device, or in which a GPU block runs on the CPU, fails, unless the
//! program has a line `# on the CPU: REASON`: then a block must say it
//! runs on the CPU for exactly that reason, and none for another.
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
//! program again with `MIDNIGHT_GPU=stats,rows` against that file.
//!
//! `window_frames` runs every program of `tests/run` that has a
//! `NAME.frames` file headless on the device, with all the commands of its
//! `NAME.drive`, `capture` included, in a directory of its own. It must
//! print `NAME.out`, the answers to its commands must be exactly
//! `NAME.frames`, and each file it captures must be a PNG of the size and
//! the pixels its answer gives.
//!
//! With `UPDATE_EXPECT=1` the release runs write `.stats` and `.frames`
//! files instead: those are the only expected files GPU mode writes.
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
//! `tests/panic`, and every eval task's `solution.12am` (against its
//! `expected.out`), that has a loop that can be split, as a release and as a
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
//! `pieces_and_threads_follow_the_work` runs `tests/run/cores_emit.12am`
//! with `16,info`, `4,info` and `1,info`, and requires the pieces and the
//! threads its two loops get by their weight and their rows.
//! `only_programs_that_split_read_the_threads_variable` requires a word
//! the runtime does not know to be reported by a program with a loop that
//! can be split, and by no other.
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
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

mod json_lines;
mod render;

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

/// A command that runs a compiled program with no GPU device and, when it
/// has a window, headless, with the commands of its `.drive` file.
fn on_cpu(exe: &Path, source: &Path) -> Command {
    let mut command = Command::new(exe);
    command.env("MIDNIGHT_GPU", "off");
    headless(&mut command, source, false);
    command
}

/// Sets the variables of a headless run of `source`, read only by a
/// program with a window: `MIDNIGHT_WINDOW=headless`, and `MIDNIGHT_DRIVE`
/// set to the program's `.drive` file, or to an empty one. Unless
/// `captures`, the `capture` commands are left out, since a run with no
/// device has no frames. The display variables name no display.
fn headless(command: &mut Command, source: &Path, captures: bool) {
    command
        .env("MIDNIGHT_WINDOW", "headless")
        .env("MIDNIGHT_DRIVE", drive_file(source, captures));
    no_display(command);
}

/// Sets the display variables to addresses that cannot be connected to,
/// and the runtime directory, where Wayland's sockets are, to an empty
/// one, so that no window of a test reaches the desktop the tests run
/// from.
fn no_display(command: &mut Command) {
    static EMPTY: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    let empty = EMPTY.get_or_init(|| scratch("no-display"));
    command
        .env("XDG_RUNTIME_DIR", empty)
        .env("WAYLAND_DISPLAY", "midnight-no-display")
        .env("DISPLAY", ":no-display")
        .env_remove("WAYLAND_SOCKET");
}

/// The commands a test run of `source` is given. `capture` lines are kept
/// as empty lines when they are left out, so line numbers stay the same.
fn drive_file(source: &Path, captures: bool) -> PathBuf {
    static DIR: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    let dir = DIR.get_or_init(|| scratch("drives"));
    let commands = std::fs::read(source.with_extension("drive")).unwrap_or_default();
    let kept: Vec<u8> = commands
        .split_inclusive(|&byte| byte == b'\n')
        .flat_map(|line| {
            let command = line.trim_ascii_start();
            if !captures && (command.starts_with(b"capture ") || command.starts_with(b"capture\t"))
            {
                b"\n"
            } else {
                line
            }
        })
        .copied()
        .collect();
    let name = format!(
        "{}{}.drive",
        source.to_string_lossy().replace(['/', '.'], "_"),
        if captures { "-captures" } else { "" }
    );
    let path = dir.join(name);
    let partial = dir.join(format!(
        "{}.{:?}",
        path.file_name().unwrap().to_string_lossy(),
        std::thread::current().id()
    ));
    std::fs::write(&partial, kept).expect("write a drive file");
    std::fs::rename(&partial, &path).expect("move a drive file");
    path
}

/// The answers of `MIDNIGHT_DRIVE`'s commands in standard error, and the
/// rest of it.
fn split_drive(stderr: &str) -> (String, String) {
    let mut drive = String::new();
    let mut rest = String::new();
    for line in stderr.split_inclusive('\n') {
        match line.strip_prefix(DRIVE_SAID) {
            Some(answer) => drive.push_str(answer),
            None => rest.push_str(line),
        }
    }
    (drive, rest)
}

const DRIVE_SAID: &str = "midnight: drive: ";

/// The lines of the `draws` answers among a run's answers: each answer's
/// first line, `draws: …`, and the lines after it, which begin with two
/// spaces.
fn draws_answers(answers: &str) -> String {
    answers
        .split_inclusive('\n')
        .filter(|line| line.starts_with("draws: ") || line.starts_with("  "))
        .collect()
}

/// A run's answers without those of `draws`.
fn without_draws(answers: &str) -> String {
    answers
        .split_inclusive('\n')
        .filter(|line| !line.starts_with("draws: ") && !line.starts_with("  "))
        .collect()
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
    let run = run_within(&mut on_cpu(&exe, source), RUN_LIMIT);
    let label = format!("{} ({mode:?})", source.display());
    if run.status != Some(0) {
        return Err(format!(
            "{label}: exited with {:?}{}\n--- stdout\n{}--- stderr\n{}",
            run.status,
            if run.timed_out {
                " (still running)"
            } else {
                ""
            },
            run.stdout,
            run.stderr
        ));
    }
    if !run.stderr.is_empty() {
        return Err(format!("{label}: wrote to standard error\n{}", run.stderr));
    }
    // Only the release run may write the expected file; debug must match it.
    let out_path = source.with_extension("out");
    if update_expect() && mode == Mode::Debug {
        return Ok(());
    }
    expect(&out_path, &run.stdout, "standard output")
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

/// Every program of `tests/run` and `examples` with a `NAME.draws` file,
/// run headless with no device in both build modes, answers its `draws`
/// commands with exactly the lines of that file. With `UPDATE_EXPECT=1`
/// the release run writes the file; no other test does.
#[test]
fn draws_match_expected() {
    let sources: Vec<PathBuf> = programs("tests/run")
        .into_iter()
        .chain(examples())
        .filter(|source| source.with_extension("draws").exists())
        .collect();
    assert!(
        !sources.is_empty(),
        "no program with a .draws file under tests/run and examples of the working directory"
    );
    let dir = scratch("draws");
    let jobs: Vec<(&PathBuf, Mode)> = sources
        .iter()
        .flat_map(|source| MODES.map(|mode| (source, mode)))
        .collect();
    let failures = for_each_parallel(&jobs, |(source, mode)| {
        let label = format!("{} ({mode:?})", source.display());
        let exe = exe_path(&dir, source, *mode);
        build(source, &exe, *mode)?;
        let run = run_within(&mut on_cpu(&exe, source), RUN_LIMIT);
        if run.status != Some(0) || !run.stderr.is_empty() {
            return Err(format!(
                "{label}: exited with {:?}\n--- answers\n{}--- stderr\n{}---",
                run.status, run.drive, run.stderr
            ));
        }
        if update_expect() && *mode == Mode::Debug {
            return Ok(());
        }
        expect(
            &source.with_extension("draws"),
            &draws_answers(&run.drive),
            "the answers of `draws`",
        )
        .map_err(|message| format!("{label}: {message}"))
    });
    let _ = std::fs::remove_dir_all(&dir);
    report("runs with draws", jobs.len(), failures);
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

/// The lines of a diagnostic's text that its JSON form must give: each
/// `error:` and `note:` line, each `-->` line, and the width and label of
/// each primary marker.
fn diagnostic_skeleton_of_text(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim_start();
        if line.starts_with("error: ") || line.starts_with("note: ") {
            out.push(line.to_string());
        } else if let Some(place) = trimmed.strip_prefix("--> ") {
            out.push(format!("--> {place}"));
        } else if let Some(rest) = trimmed.strip_prefix("| ") {
            let marked = rest.trim_start();
            let width = marked.chars().take_while(|&c| c == '^').count();
            if width > 0 {
                out.push(format!("mark: {width}"));
                if let Some(label) = marked[width..].strip_prefix(' ') {
                    out.push(format!("label: {label}"));
                }
            }
        }
    }
    out
}

/// The 1-based line and column, in characters, of a byte offset.
fn line_column(text: &str, byte: usize) -> (usize, usize) {
    let before = &text[..byte];
    let line = before.matches('\n').count() + 1;
    let start = before.rfind('\n').map_or(0, |i| i + 1);
    (line, before[start..].chars().count() + 1)
}

/// Checks a `start` or `end` against the file it names, and returns its
/// line and column.
fn json_position(position: &json_lines::Value, text: &str) -> Result<(usize, usize), String> {
    let (line, column, byte) = (
        position.int("line")? as usize,
        position.int("column")? as usize,
        position.int("byte")? as usize,
    );
    if byte > text.len() || line_column(text, byte) != (line, column) {
        return Err(format!(
            "{} does not agree with its byte in the file",
            position.text()
        ));
    }
    Ok((line, column))
}

/// The skeleton of [`diagnostic_skeleton_of_text`] made from JSON lines.
fn diagnostic_skeleton_of_json(lines: &[json_lines::Value]) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    let read = |name: &str| std::fs::read_to_string(name).map_err(|e| format!("{name}: {e}"));
    let mut errors = 0;
    let mut totals = None;
    for line in lines {
        match line.kind() {
            "error" if totals.is_none() => {
                errors += 1;
                let file = line.str("file")?;
                let text = read(file)?;
                let (start_line, start_column) = json_position(line.get("start").unwrap(), &text)?;
                let (end_line, end_column) = json_position(line.get("end").unwrap(), &text)?;
                out.push(format!("error: {}", line.str("message")?));
                out.push(format!("--> {file}:{start_line}:{start_column}"));
                let width = if end_line == start_line {
                    end_column - start_column
                } else {
                    let line_text = text.lines().nth(start_line - 1).unwrap_or("");
                    (line_text.chars().count() + 1).saturating_sub(start_column)
                };
                out.push(format!("mark: {}", width.max(1)));
                if let Some(label) = line.get("label") {
                    out.push(format!("label: {}", label.as_str().ok_or("a label")?));
                }
                for note in line.array("notes")? {
                    let file = note.str("file")?;
                    let text = read(file)?;
                    let (line, column) = json_position(note.get("start").unwrap(), &text)?;
                    json_position(note.get("end").unwrap(), &text)?;
                    out.push(format!("note: {}", note.str("message")?));
                    out.push(format!("--> {file}:{line}:{column}"));
                }
            }
            "errors" if totals.is_none() => totals = Some((line.int("count")?, line.int("shown")?)),
            _ => return Err(format!("unexpected line {}", line.text())),
        }
    }
    let (count, shown) = totals.ok_or("no `errors` line")?;
    if shown != errors || shown != count.min(20) {
        return Err(format!(
            "{errors} errors, but count {count} and shown {shown}"
        ));
    }
    if count > shown {
        let hidden = count - shown;
        let noun = if hidden == 1 { "error" } else { "errors" };
        out.push(format!("note: {hidden} more {noun} not shown"));
    }
    Ok(out)
}

/// Runs `midnight ARGS --format json` and returns its standard output and
/// standard error.
fn midnight_json(args: &[&OsStr], status: i32) -> Result<(String, String), String> {
    let output = midnight()
        .args(args)
        .args(["--format", "json"])
        .output()
        .expect("run midnight");
    if output.status.code() != Some(status) {
        return Err(format!(
            "`midnight {}` exited with {:?}, expected {status}\n{}",
            args.join(OsStr::new(" ")).to_string_lossy(),
            output.status,
            text(&output.stderr)
        ));
    }
    Ok((text(&output.stdout), text(&output.stderr)))
}

#[test]
fn fail_programs_in_json() {
    let sources = programs("tests/fail");
    let failures = for_each_parallel(&sources, |source| {
        let label = |message: String| format!("{}: {message}", source.display());
        let (stdout, stderr) =
            midnight_json(&[OsStr::new("check"), source.as_os_str()], 1).map_err(label)?;
        if !stdout.is_empty() {
            return Err(label(format!("wrote to standard output\n{stdout}")));
        }
        let lines = json_lines::parse_form(&stderr).map_err(label)?;
        let actual = diagnostic_skeleton_of_json(&lines).map_err(label)?;
        let expected_text = std::fs::read_to_string(source.with_extension("err")).unwrap();
        let expected = diagnostic_skeleton_of_text(&expected_text);
        if actual != expected {
            return Err(label(format!(
                "the JSON says\n{}\nbut the text says\n{}",
                actual.join("\n"),
                expected.join("\n")
            )));
        }
        Ok(())
    });
    report("failing programs in JSON", sources.len(), failures);
}

/// A line of text with its runs of spaces made one space.
fn collapse_spaces(line: &str) -> String {
    line.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A `transfer` line as the text of `midnight plan` gives it, its spaces
/// collapsed.
fn transfer_text(line: &json_lines::Value) -> Result<String, String> {
    if line.kind() != "transfer" {
        return Err(format!("unexpected line {}", line.text()));
    }
    let stage = |key: &str| -> Result<String, String> {
        let stage = line.get(key).ok_or(format!("no `{key}`"))?;
        if stage.get("top_level").and_then(json_lines::Value::as_bool) == Some(true) {
            return Ok("top level".to_string());
        }
        let name = stage.str("system")?;
        Ok(if stage.bool("strict")? {
            format!("strict {name}")
        } else {
            name.to_string()
        })
    };
    let mut text = format!(
        "{} {} {} -> {}",
        line.str("direction")?,
        line.str("component")?,
        stage("from")?,
        stage("to")?
    );
    if line.bool("rows")? {
        text.push_str(" rows");
    }
    if let Some(on) = line.get("on") {
        text.push_str(&format!(" on {}", on.as_str().ok_or("`on`")?));
    }
    Ok(text)
}

/// A `loop` line as the text of `midnight loops` gives it, its spaces
/// collapsed.
fn loop_text(line: &json_lines::Value) -> Result<String, String> {
    if line.kind() != "loop" {
        return Err(format!("unexpected line {}", line.text()));
    }
    let stands_in = line.get("in").ok_or("no `in`")?;
    let stands_in = if let Some(name) = stands_in.get("system") {
        let kind = match (stands_in.bool("gpu")?, stands_in.bool("strict")?) {
            (true, true) => "strict gpu system",
            (true, false) => "gpu system",
            _ => "system",
        };
        format!("{kind} {}", name.as_str().ok_or("`system`")?)
    } else if let Some(name) = stands_in.get("def") {
        format!("def {}", name.as_str().ok_or("`def`")?)
    } else if stands_in.bool("top_level")? {
        "top level".to_string()
    } else {
        return Err(format!("`in` is {}", stands_in.text()));
    };
    let names = |key: &str| -> Result<Vec<String>, String> {
        line.array(key)?
            .iter()
            .map(|name| name.as_str().map(str::to_string).ok_or(format!("`{key}`")))
            .collect()
    };
    names("with")?;
    names("without")?;
    let runs = match line.str("runs")? {
        "one_core" => {
            let rule = line.int("rule")?;
            if !(1..=7).contains(&rule) {
                return Err(format!("rule {rule}"));
            }
            format!("one core: {}", line.str("reason")?)
        }
        runs @ ("gpu" | "cores") => runs.to_string(),
        other => return Err(format!("`runs` is `{other}`")),
    };
    Ok(collapse_spaces(&format!(
        "{}:{} {stands_in} each {} {runs}",
        line.str("file")?,
        line.int("line")?,
        names("terms")?.join(", ")
    )))
}

#[test]
fn plan_and_loops_in_json() {
    let mut sources: Vec<(PathBuf, &str)> = programs("tests/plan")
        .into_iter()
        .map(|source| (source, "plan"))
        .collect();
    sources.extend(
        programs("tests/loops")
            .into_iter()
            .map(|source| (source, "loops")),
    );
    let failures = for_each_parallel(&sources, |(source, command)| {
        let label = |message: String| format!("{}: {message}", source.display());
        let (stdout, stderr) =
            midnight_json(&[OsStr::new(command), source.as_os_str()], 0).map_err(label)?;
        if !stderr.is_empty() {
            return Err(label(format!("wrote to standard error\n{stderr}")));
        }
        let lines = json_lines::parse_form(&stdout).map_err(label)?;
        let line_text = if *command == "plan" {
            transfer_text
        } else {
            loop_text
        };
        let actual: Vec<String> = lines
            .iter()
            .map(line_text)
            .collect::<Result<_, _>>()
            .map_err(label)?;
        let expected_text = std::fs::read_to_string(source.with_extension(command)).unwrap();
        let expected: Vec<String> = expected_text.lines().map(collapse_spaces).collect();
        if actual != expected {
            return Err(label(format!(
                "the JSON says\n{}\nbut the text says\n{}",
                actual.join("\n"),
                expected.join("\n")
            )));
        }
        Ok(())
    });
    report("plans and loops in JSON", sources.len(), failures);
}

/// What a `tests/json` file writes in place of the compiler's version.
const VERSION_MARK: &str = "\"version\":\"VERSION\"";

/// `tests/json/NAME.12am`: its first line, `# json: COMMAND`, names the
/// command whose JSON `NAME.json` holds exactly: `check`, `plan` or
/// `loops`, or `run` for the standard error of a headless run with its
/// `NAME.drive` and `MIDNIGHT_FORMAT=json`.
#[test]
fn json_programs() {
    let sources = programs("tests/json");
    let dir = scratch("json");
    let version = format!("\"version\":\"{}\"", env!("CARGO_PKG_VERSION"));
    let failures = for_each_parallel(&sources, |source| {
        let label = |message: String| format!("{}: {message}", source.display());
        let program = std::fs::read_to_string(source).unwrap();
        let command = program
            .lines()
            .next()
            .and_then(|line| line.strip_prefix("# json: "))
            .ok_or_else(|| label("no `# json: COMMAND` line".to_string()))?;
        let actual = match command {
            "check" | "plan" | "loops" => {
                let output = midnight()
                    .arg(command)
                    .arg(source)
                    .args(["--format", "json"])
                    .output()
                    .expect("run midnight");
                let (stdout, stderr) = (text(&output.stdout), text(&output.stderr));
                match (command, stderr.is_empty()) {
                    ("check", _) if stdout.is_empty() => stderr,
                    (_, true) => stdout,
                    _ => {
                        return Err(label(format!(
                            "wrote to both streams\n--- stdout\n{stdout}--- stderr\n{stderr}"
                        )));
                    }
                }
            }
            "run" => {
                let exe = exe_path(&dir, source, Mode::Release);
                build(source, &exe, Mode::Release)?;
                let mut command = on_cpu(&exe, source);
                command
                    .env("MIDNIGHT_FORMAT", "json")
                    .env("MIDNIGHT_DRIVE", source.with_extension("drive"));
                run_within(&mut command, RUN_LIMIT).stderr
            }
            other => return Err(label(format!("unknown command `{other}`"))),
        };
        for (i, line) in actual.lines().enumerate() {
            json_lines::parse_line(line).map_err(|e| label(format!("line {}: {e}", i + 1)))?;
        }
        expect(
            &source.with_extension("json"),
            &actual.replacen(&version, VERSION_MARK, 1),
            "JSON",
        )
        .map_err(label)
    });
    let _ = std::fs::remove_dir_all(&dir);
    report("JSON programs", sources.len(), failures);
}

/// The first line of every JSON form, with this compiler's version.
fn json_first_line() -> String {
    format!(
        "{{\"kind\":\"midnight\",\"format\":1,\"version\":\"{}\"}}\n",
        env!("CARGO_PKG_VERSION")
    )
}

#[test]
fn format_takes_one_word_on_five_commands() {
    let file = "examples/move.12am";
    for args in [
        vec!["check", file, "--format", "xml"],
        vec!["check", file, "--format"],
        vec!["check", file, "--format", "json", "--format", "json"],
        vec!["check", file, "--format=json"],
        vec!["emit-c", file, "--format", "json"],
        vec!["emit-wgsl", file, "--format", "text"],
    ] {
        let output = midnight().args(&args).output().expect("run midnight");
        let stderr = text(&output.stderr);
        assert_eq!(output.status.code(), Some(2), "{args:?}: {stderr}");
        assert!(
            stderr.starts_with("midnight: ") && stderr.contains("Usage:"),
            "{args:?}: {stderr}"
        );
    }
    let usage = text(&midnight().arg("--help").output().unwrap().stdout);
    assert!(usage.contains("--format text|json"), "{usage}");
    for command in ["check", "plan", "loops"] {
        let output = midnight()
            .args([command, file, "--format", "text"])
            .output()
            .unwrap();
        let plain = midnight().args([command, file]).output().unwrap();
        assert_eq!(output, plain, "{command}");
    }
}

#[test]
fn compiler_messages_in_json() {
    let dir = scratch("compiler-messages");
    let first = json_first_line();
    let lines = |stderr: &str| json_lines::parse_form(stderr).unwrap();

    // A file that can't be read, from `check` and from `plan`, which writes
    // its first line to both streams.
    let (_, stderr) = midnight_json(&[OsStr::new("check"), OsStr::new("no/such.12am")], 1).unwrap();
    let line = &lines(&stderr)[0];
    assert_eq!(line.kind(), "cant_read");
    assert_eq!(line.str("file"), Ok("no/such.12am"));
    assert!(!line.str("message").unwrap().is_empty());
    let (stdout, stderr) =
        midnight_json(&[OsStr::new("plan"), OsStr::new("no/such.12am")], 1).unwrap();
    assert_eq!(stdout, first);
    assert_eq!(lines(&stderr)[0].kind(), "cant_read");
    let (stdout, stderr) = midnight_json(
        &[
            OsStr::new("loops"),
            OsStr::new("tests/fail/args_builtin_count.12am"),
        ],
        1,
    )
    .unwrap();
    assert_eq!(stdout, first);
    assert_eq!(lines(&stderr)[0].kind(), "error");

    // A check with no error writes the first line alone.
    let (stdout, stderr) =
        midnight_json(&[OsStr::new("check"), OsStr::new("examples/move.12am")], 0).unwrap();
    assert_eq!((stdout.as_str(), stderr.as_str()), ("", first.as_str()));

    // A C compiler that fails, and one that does not start.
    let out = dir.join("move");
    let build = |cc: &str| {
        midnight()
            .args(["build", "examples/move.12am", "--format", "json", "-o"])
            .arg(&out)
            .env("MIDNIGHT_CC", cc)
            .output()
            .unwrap()
    };
    let output = build("false");
    assert_eq!(output.status.code(), Some(1));
    let line = &lines(&text(&output.stderr))[0];
    assert_eq!(line.kind(), "c_compiler");
    assert_eq!(
        (line.str("compiler"), line.int("status")),
        (Ok("false"), Ok(1))
    );
    assert_eq!(line.str("output"), Ok(""));
    let kept = line.str("kept").unwrap();
    assert!(
        kept.ends_with("program.c") && Path::new(kept).exists(),
        "{kept}"
    );
    let _ = std::fs::remove_dir_all(Path::new(kept).parent().unwrap());
    let output = build("/no/such/cc");
    let line = &lines(&text(&output.stderr))[0];
    assert_eq!(line.kind(), "c_compiler");
    assert!(line.get("status").is_none() && line.get("kept").is_none());
    assert!(!line.str("message").unwrap().is_empty());

    // A build directory that can't be made.
    let output = midnight()
        .args(["build", "examples/move.12am", "--format", "json", "-o"])
        .arg(&out)
        .env("TMPDIR", "/no/such/dir")
        .output()
        .unwrap();
    let line = &lines(&text(&output.stderr))[0];
    assert_eq!(line.kind(), "build_directory");
    assert_eq!(line.str("directory"), Ok("/no/such/dir"));
    assert!(line.str("message").is_ok());

    // Flags that are not set: a program with a GPU system, and one with a
    // window, built with `false` as the C compiler.
    for (source, affects, variables) in [
        (
            "tests/run/gpu_transfers.12am",
            "gpu_systems",
            vec!["MIDNIGHT_GPU_FLAGS"],
        ),
        (
            "examples/flock.12am",
            "window",
            vec!["MIDNIGHT_GPU_FLAGS", "MIDNIGHT_WINDOW_FLAGS"],
        ),
    ] {
        let output = midnight()
            .args(["build", source, "--format", "json", "-o"])
            .arg(&out)
            .env("MIDNIGHT_CC", "false")
            .env_remove("MIDNIGHT_GPU_FLAGS")
            .env_remove("MIDNIGHT_WINDOW_FLAGS")
            .output()
            .unwrap();
        let found = lines(&text(&output.stderr));
        let line = found
            .iter()
            .find(|line| line.kind() == "no_flags" && line.str("affects") == Ok(affects))
            .unwrap_or_else(|| panic!("{source}: no `no_flags` line for {affects}"));
        assert_eq!(line.str("affects"), Ok(affects), "{source}");
        let named: Vec<&str> = line
            .array("variables")
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(named, variables, "{source}");
        assert!(line.str("message").unwrap().contains("not set"), "{source}");
        let kept = found.iter().find_map(|line| line.get("kept")?.as_str());
        if let Some(kept) = kept {
            let _ = std::fs::remove_dir_all(Path::new(kept).parent().unwrap());
        }
    }

    let _ = std::fs::remove_dir_all(&dir);
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
        let run = run_within(&mut on_cpu(&exe, source), PANIC_LIMIT);
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

/// An enum variant spelled as a built-in name, declared or written after
/// `::`, gives a program no built-in declarations: its generated C is that
/// of the same program with other names for the variants.
#[test]
fn a_variant_named_like_a_built_in_gives_no_window() {
    let dir = scratch("variant-names");
    let program = "enum Shape\n  Mouse, Keyboard, Window, Key, Draw\nend\n\nputs Shape::Mouse, Shape::Key, Shape::Draw\n";
    let emit = |name: &str, body: &str| {
        let source = dir.join(format!("{name}.12am"));
        std::fs::write(&source, body).expect("write a test program");
        let output = midnight()
            .arg("emit-c")
            .arg(&source)
            .output()
            .expect("run midnight");
        assert!(output.status.success(), "{}", text(&output.stderr));
        text(&output.stdout).replace(&format!("{name}.12am"), "PROGRAM")
    };
    let named = emit("named", program);
    let renamed = emit(
        "renamed",
        &program
            .replace("Keyboard", "Qwertyui")
            .replace("Key", "Kay")
            .replace("Mouse", "Moose")
            .replace("Window", "Widget")
            .replace("Draw", "Dram"),
    );
    let _ = std::fs::remove_dir_all(&dir);
    assert!(!named.contains("mn_window_program"), "{named}");
    let back = renamed
        .replace("Qwertyui", "Keyboard")
        .replace("Kay", "Key")
        .replace("Moose", "Mouse")
        .replace("Widget", "Window")
        .replace("Dram", "Draw");
    assert_eq!(named, back);
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

/// The reason a program states in a `# on the CPU: REASON` line, if it has
/// one: why one of its GPU blocks runs on the CPU on every device.
fn stated_cpu_reason(program: &str) -> Option<String> {
    program
        .lines()
        .find_map(|line| line.strip_prefix("# on the CPU: "))
        .map(|reason| reason.trim_end().to_string())
}

/// The `stated_cpu_reason` of the program at `source`.
fn cpu_reason_of(source: &Path) -> Option<String> {
    stated_cpu_reason(&std::fs::read_to_string(source).unwrap_or_default())
}

/// Standard error without the `info` lines that say a GPU block runs on
/// the CPU. Each must give `reason`, or, with `strict_cpu`, say that
/// `MIDNIGHT_GPU` has `strict-cpu`; with a `reason`, at least one must
/// give it.
fn cpu_runs_as_stated(
    stderr: &str,
    reason: Option<&str>,
    strict_cpu: bool,
) -> Result<String, String> {
    let mut rest = String::new();
    let mut stated = 0;
    for line in stderr.split_inclusive('\n') {
        let said = line.strip_prefix("midnight: gpu: ").map(str::trim_end);
        let Some((_, why)) = said.and_then(|said| said.split_once(" runs on the CPU: ")) else {
            rest.push_str(line);
            continue;
        };
        if strict_cpu && line.trim_end().ends_with(STRICT_REFUSED) {
            continue;
        }
        if Some(why) != reason {
            return Err(said.unwrap().to_string());
        }
        stated += 1;
    }
    match reason {
        Some(reason) if stated == 0 => {
            Err(format!("no GPU block said it runs on the CPU: {reason}"))
        }
        _ => Ok(rest),
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

/// Compares the answers to `draws` commands with the expected ones. With
/// no tolerance the two must be equal. With one they must have the same
/// lines, and a line that draws must have the same shape and color and
/// each of the shape's numbers within `tolerance * max(1, |expected|)` of
/// the expected one; every other line must be equal. Returns the largest
/// difference between two numbers, as a multiple of `max(1, |expected|)`.
fn compare_draws(expected: &str, actual: &str, tolerance: Option<f64>) -> Result<f64, String> {
    if expected == actual {
        return Ok(0.0);
    }
    let Some(tolerance) = tolerance else {
        return Err(first_difference(expected, actual));
    };
    if expected.lines().count() != actual.lines().count() {
        return Err(format!(
            "{} lines, expected {}",
            actual.lines().count(),
            expected.lines().count()
        ));
    }
    // A line that draws: `  Draw.NAME(` and the shape's numbers, then
    // `Color(...)`.
    fn parts(line: &str) -> Option<(&str, Vec<&str>, &str)> {
        let rest = line.strip_prefix("  Draw.")?;
        let (name, rest) = rest.split_once('(')?;
        let at = rest.find("Color(")?;
        let numbers = rest[..at].strip_suffix(", ")?.split(", ").collect();
        Some((name, numbers, &rest[at..]))
    }
    let mut largest: f64 = 0.0;
    for (index, (expected, actual)) in expected.lines().zip(actual.lines()).enumerate() {
        if expected == actual {
            continue;
        }
        let differs = || format!("line {}: expected `{expected}`, got `{actual}`", index + 1);
        let (Some((name, numbers, color)), Some((actual_name, actual_numbers, actual_color))) =
            (parts(expected), parts(actual))
        else {
            return Err(differs());
        };
        if name != actual_name || color != actual_color || numbers.len() != actual_numbers.len() {
            return Err(differs());
        }
        for (expected_number, actual_number) in numbers.iter().zip(&actual_numbers) {
            if expected_number == actual_number {
                continue;
            }
            let (Ok(e), Ok(a)) = (expected_number.parse::<f64>(), actual_number.parse::<f64>())
            else {
                return Err(differs());
            };
            let difference = (a - e).abs() / e.abs().max(1.0);
            if difference.is_nan() || difference > tolerance {
                return Err(format!(
                    "line {}: {actual_number} is not within {tolerance} of {expected_number}",
                    index + 1
                ));
            }
            largest = largest.max(difference);
        }
    }
    Ok(largest)
}

/// The first line in which two texts differ, as a message.
fn first_difference(expected: &str, actual: &str) -> String {
    let mut expected_lines = expected.lines();
    let mut actual_lines = actual.lines();
    for number in 1.. {
        match (expected_lines.next(), actual_lines.next()) {
            (Some(e), Some(a)) if e == a => {}
            (None, None) => return "the end of the last line differs".to_string(),
            (e, a) => {
                return format!(
                    "line {number}: expected `{}`, got `{}`",
                    e.unwrap_or("(nothing)"),
                    a.unwrap_or("(nothing)")
                );
            }
        }
    }
    unreachable!()
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
    /// The answers to the `draws` commands.
    draws: String,
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
/// on the CPU, unless `more` has `strict-cpu` and the block is strict, or
/// the program states the reason in `# on the CPU: REASON`; with that line,
/// a block must say it. A program with a window runs headless with its
/// commands, `capture` left out; the answers to them are not in `stderr`,
/// and those of `draws` are in `draws`.
/// The command of [`run_on_device`]: headless, `MIDNIGHT_GPU` with `info`
/// and `more`, on `device`.
fn device_command(
    exe: &Path,
    source: &Path,
    device: Device,
    more: &[&str],
    threads: Option<&str>,
) -> Command {
    let mut command = Command::new(exe);
    headless(&mut command, source, false);
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
    command
}

fn run_on_device(
    exe: &Path,
    source: &Path,
    device: Device,
    more: &[&str],
    threads: Option<&str>,
) -> Result<DeviceRun, String> {
    let output = device_command(exe, source, device, more, threads)
        .output()
        .expect("run program");

    let mut run = DeviceRun {
        status: output.status.code(),
        stdout: text(&output.stdout),
        stderr: String::new(),
        device: String::new(),
        ticks: Vec::new(),
        stats: String::new(),
        strict: None,
        draws: String::new(),
    };
    let (answers, stderr) = split_drive(&text(&output.stderr));
    run.draws = draws_answers(&answers);
    let reason = cpu_reason_of(source);
    let stderr = cpu_runs_as_stated(&stderr, reason.as_deref(), more.contains(&"strict-cpu"))?;
    for line in stderr.split_inclusive('\n') {
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
/// to standard error. The answers to `draws` must be `NAME.draws`, when it
/// exists, within the same tolerance. Returns the device and the largest
/// difference from `NAME.out` and `NAME.draws`.
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
    let draws_path = source.with_extension("draws");
    let expected_draws = if draws_path.exists() {
        Some(read(&draws_path)?)
    } else {
        None
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
        let run = run_on_device(&exe, source, device, more, threads)
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
        if let Some(expected_draws) = &expected_draws {
            match compare_draws(expected_draws, &run.draws, tolerance) {
                Ok(difference) => found.1 = found.1.max(difference),
                Err(message) => {
                    return Err(format!(
                        "{label}: the `draws` answers differ from {}: {message}\n\
                         --- expected\n{expected_draws}--- actual\n{}---",
                        draws_path.display(),
                        run.draws
                    ));
                }
            }
        }
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
            let run = run_on_device(&exe, source, device, &["stats"], threads)
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

/// The programs of `tests/run` and `examples` that have a `NAME.stats` file.
fn stats_programs() -> Vec<PathBuf> {
    let mut sources = programs("tests/run");
    sources.extend(programs("examples"));
    sources
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
        "no program with a .stats file under tests/run or examples of the working directory"
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
            let run = run_on_device(&exe, source, device, &words, threads)
                .map_err(|message| format!("{label}: {message}"))?;
            if run.status != Some(0) || !run.stderr.is_empty() {
                return Err(format!(
                    "{label}: exited with {:?}\n--- stderr\n{}---",
                    run.status, run.stderr
                ));
            }
            devices.lock().unwrap().insert(run.device);
            // The device, strict and `stats` lines say the same in JSON.
            if words.len() == 1 && threads.is_none() {
                let mut command = device_command(&exe, source, device, &words, None);
                run_both_forms(&mut command, RUN_LIMIT)
                    .map_err(|message| format!("{label}: {message}"))?;
            }
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

/// The programs of `tests/run` that have a `NAME.frames` file.
fn frame_programs() -> Vec<PathBuf> {
    programs("tests/run")
        .into_iter()
        .filter(|source| source.with_extension("frames").exists())
        .collect()
}

/// The words of `MIDNIGHT_GPU` and the variables that run a program on
/// `device`, with its `info` line.
fn on_device(command: &mut Command, device: Device) {
    if device == Device::Software {
        command
            .env("MIDNIGHT_GPU", "info,software")
            .env("VK_DRIVER_FILES", software_driver())
            .env_remove("VK_ADD_DRIVER_FILES");
    } else {
        command.env("MIDNIGHT_GPU", "info");
    }
}

/// Takes the device's `info` line out of a run's standard error, and
/// fails when there is no device or it is not the one asked for.
fn device_said(run: &mut TimedRun, device: Device) -> Result<String, String> {
    let mut said = None;
    let mut rest = String::new();
    for line in run.stderr.split_inclusive('\n') {
        match line.strip_prefix("midnight: gpu: ") {
            Some(name) if said.is_none() => said = Some(name.trim_end().to_string()),
            _ => rest.push_str(line),
        }
    }
    run.stderr = rest;
    match said {
        None => Err("no `info` line".to_string()),
        Some(name) if name == "no device" => Err(format!("no {device:?} device").to_lowercase()),
        Some(name) if device == Device::Software && !name.contains("llvmpipe") => {
            Err(format!("the device is {name}, not llvmpipe"))
        }
        Some(name) => Ok(name),
    }
}

/// The file, the size and the CRC-32 that the answer to a `capture`
/// gives: `capture FILE: N ticks, WxH, crc XXXXXXXX`.
fn capture_answer(line: &str) -> Option<(&str, u32, u32, u32)> {
    let (head, crc) = line.rsplit_once(", crc ")?;
    let (head, size) = head.rsplit_once(", ")?;
    let (head, _ticks) = head.rsplit_once(": ")?;
    let file = head.strip_prefix("capture")?.trim_start();
    let (width, height) = size.split_once('x')?;
    Some((
        file,
        width.parse().ok()?,
        height.parse().ok()?,
        u32::from_str_radix(crc, 16).ok()?,
    ))
}

/// Requires the file of each capture answered in `answers`, in `dir`, to
/// be a PNG of the size the answer gives whose pixels have its CRC-32.
/// Returns how many there were.
fn check_captures(answers: &str, dir: &Path) -> Result<usize, String> {
    let mut count = 0;
    for line in answers.lines().filter(|line| line.starts_with("capture")) {
        let (file, width, height, crc) =
            capture_answer(line).ok_or_else(|| format!("not the answer to a capture: {line}"))?;
        let bytes = std::fs::read(dir.join(file)).map_err(|error| format!("{file}: {error}"))?;
        let (png_width, png_height, pixels) =
            png_pixels(&bytes).map_err(|message| format!("{file}: {message}"))?;
        if (png_width, png_height) != (width, height) {
            return Err(format!(
                "{file}: {png_width}x{png_height} pixels, answered {width}x{height}"
            ));
        }
        if crc32(&pixels) != crc {
            return Err(format!(
                "{file}: the pixels have the CRC-32 {:08x}, answered {crc:08x}",
                crc32(&pixels)
            ));
        }
        count += 1;
    }
    Ok(count)
}

/// GPU mode: every program of `tests/run` with a `NAME.frames` file runs
/// headless on the device with all its commands, in release and debug
/// builds. It must print `NAME.out`, its answers must be exactly
/// `NAME.frames`, and each file it captures must be a PNG of the size and
/// pixels its answer gives. A capture to a file that can't be written
/// stops the program.
#[test]
fn window_frames() {
    let Some(device) = Device::from_env() else {
        return;
    };
    let sources = frame_programs();
    assert!(
        !sources.is_empty(),
        "no program with a .frames file under tests/run of the working directory"
    );
    let dir = scratch("window-frames");
    let jobs: Vec<(&PathBuf, Mode)> = sources
        .iter()
        .flat_map(|source| MODES.map(|mode| (source, mode)))
        .collect();
    let devices: Mutex<BTreeSet<String>> = Mutex::new(BTreeSet::new());
    let captures = AtomicUsize::new(0);
    let failures = for_each_parallel(&jobs, |(source, mode)| {
        let label = format!("{} ({mode:?})", source.display());
        let exe = std::path::absolute(exe_path(&dir, source, *mode)).expect("an absolute path");
        build(source, &exe, *mode)?;
        let files = dir.join(format!(
            "{}-{mode:?}-files",
            source.file_stem().unwrap().to_string_lossy()
        ));
        std::fs::create_dir_all(&files).expect("create a directory for captures");
        let mut command = Command::new(&exe);
        headless(&mut command, source, true);
        on_device(&mut command, device);
        command.current_dir(&files);
        let mut run =
            run_both_forms(&mut command, RUN_LIMIT).map_err(|e| format!("{label}: {e}"))?;
        let said = device_said(&mut run, device).map_err(|message| format!("{label}: {message}"));
        if run.status != Some(0) || !run.stderr.is_empty() {
            return Err(format!(
                "{label}: exited with {:?}\n--- answers\n{}--- stderr\n{}---",
                run.status, run.drive, run.stderr
            ));
        }
        devices.lock().unwrap().insert(said?);
        let out = std::fs::read_to_string(source.with_extension("out")).unwrap_or_default();
        if run.stdout != out {
            return Err(format!(
                "{label}: standard output differs from the .out file\n--- actual\n{}---",
                run.stdout
            ));
        }
        // Only the release run may write the expected file; debug must
        // match it.
        if !(update_expect() && *mode == Mode::Debug) {
            expect(
                &source.with_extension("frames"),
                &without_draws(&run.drive),
                "the answers",
            )
            .map_err(|message| format!("{label}: {message}"))?;
        }
        let count =
            check_captures(&run.drive, &files).map_err(|message| format!("{label}: {message}"))?;
        captures.fetch_add(count, Ordering::Relaxed);
        Ok(())
    });

    // A file that can't be written.
    let mut failures = failures;
    let exe = build_text(&dir, "driven", DRIVEN);
    let commands = dir.join("unwritable.drive");
    std::fs::write(&commands, "tick\ncapture missing/frame.png\n").expect("write commands");
    let mut command = Command::new(&exe);
    command
        .env("MIDNIGHT_WINDOW", "headless")
        .env("MIDNIGHT_DRIVE", &commands)
        .current_dir(&dir);
    on_device(&mut command, device);
    let mut run = run_within(&mut command, PANIC_LIMIT);
    let said = device_said(&mut run, device);
    let expected = format!(
        "tick\n{}:2: can't capture: can't write missing/frame.png: No such file or directory\n",
        commands.display()
    );
    if said.is_err() || run.status != Some(1) || run.drive != expected || !run.stderr.is_empty() {
        failures.push(format!(
            "a capture to a file that can't be written: exited with {:?}, {said:?}\n\
             --- expected\n{expected}--- answers\n{}--- stderr\n{}---",
            run.status, run.drive, run.stderr
        ));
    }
    let _ = std::fs::remove_dir_all(&dir);
    report("runs with frames", jobs.len() + 1, failures);
    let devices: Vec<String> = devices.into_inner().unwrap().into_iter().collect();
    note(&format!(
        "window_frames: {} runs of {} programs, {} captures, on {}",
        jobs.len(),
        sources.len(),
        captures.into_inner(),
        devices.join(" and ")
    ));
}

/// The programs of `tests/run` and `examples` whose commands have a
/// `draws` on the line before a `capture`.
fn drawn_programs() -> Vec<PathBuf> {
    let mut sources = programs("tests/run");
    sources.extend(programs("examples"));
    sources
        .into_iter()
        .filter(|source| {
            let commands =
                std::fs::read_to_string(source.with_extension("drive")).unwrap_or_default();
            let lines: Vec<&str> = commands.lines().map(str::trim).collect();
            lines
                .windows(2)
                .any(|pair| pair[0] == "draws" && pair[1].starts_with("capture"))
        })
        .collect()
}

/// Requires each capture answered in `answers` whose answer comes right
/// after a `draws` answer to agree with the reference renderer's picture
/// of that answer, at the size the capture gives; the file is in `dir`.
/// Returns how many there were.
fn check_drawn(answers: &str, dir: &Path) -> Result<usize, String> {
    let lines: Vec<&str> = answers.lines().collect();
    let mut count = 0;
    for (at, line) in lines.iter().enumerate() {
        let Some((file, width, height, _)) = capture_answer(line) else {
            continue;
        };
        let mut first = at;
        while first > 0 && lines[first - 1].starts_with("  ") {
            first -= 1;
        }
        if first == 0 || first == at || !lines[first - 1].starts_with("draws: ") {
            continue;
        }
        let frame = render::parse(&lines[first - 1..at].join("\n"))?;
        let bytes = std::fs::read(dir.join(file)).map_err(|error| format!("{file}: {error}"))?;
        let (_, _, pixels) = png_pixels(&bytes).map_err(|message| format!("{file}: {message}"))?;
        let picture = render::render(&frame, width, height);
        render::agrees(&picture, &pixels).map_err(|message| format!("{file}: {message}"))?;
        count += 1;
    }
    Ok(count)
}

/// The scales `window_drawn` runs each program at besides its own.
const DRAWN_SCALES: [&str; 3] = ["1.25", "1.5", "2"];

/// GPU mode: every program of `tests/run` and `examples` whose commands
/// have a `draws` on the line before a `capture` runs headless on the
/// device, in release and debug builds, as it is and with a `scale`
/// command of 1.25, 1.5 and 2 before its commands. It must print
/// `NAME.out`, and each capture after a `draws` must agree with the
/// reference renderer's picture of that `draws` answer. No GPU block may
/// run on the CPU but as the program's `# on the CPU:` line says.
#[test]
fn window_drawn() {
    let Some(device) = Device::from_env() else {
        return;
    };
    let sources = drawn_programs();
    assert!(
        !sources.is_empty(),
        "no program with `draws` before `capture` in the working directory"
    );
    let dir = scratch("window-drawn");
    let scales: Vec<Option<&str>> = std::iter::once(None)
        .chain(DRAWN_SCALES.iter().copied().map(Some))
        .collect();
    let jobs: Vec<(&PathBuf, Mode)> = sources
        .iter()
        .flat_map(|source| MODES.map(|mode| (source, mode)))
        .collect();
    let devices: Mutex<BTreeSet<String>> = Mutex::new(BTreeSet::new());
    let captures = AtomicUsize::new(0);
    let failures = for_each_parallel(&jobs, |(source, mode)| {
        let exe = std::path::absolute(exe_path(&dir, source, *mode)).expect("an absolute path");
        build(source, &exe, *mode)?;
        let commands =
            std::fs::read_to_string(source.with_extension("drive")).expect("read the commands");
        let reason = cpu_reason_of(source);
        for scale in &scales {
            let label = format!(
                "{} ({mode:?}, scale {})",
                source.display(),
                scale.unwrap_or("as given")
            );
            let files = dir.join(format!(
                "{}-{mode:?}-{}",
                source.file_stem().unwrap().to_string_lossy(),
                scale.unwrap_or("1")
            ));
            std::fs::create_dir_all(&files).expect("create a directory for captures");
            let drive = files.join("commands.drive");
            let given = match scale {
                Some(scale) => format!("scale {scale}\n{commands}"),
                None => commands.clone(),
            };
            std::fs::write(&drive, given).expect("write the commands");
            let mut command = Command::new(&exe);
            command
                .env("MIDNIGHT_WINDOW", "headless")
                .env("MIDNIGHT_DRIVE", &drive);
            no_display(&mut command);
            on_device(&mut command, device);
            command.current_dir(&files);
            let mut run = run_within(&mut command, RUN_LIMIT);
            let said =
                device_said(&mut run, device).map_err(|message| format!("{label}: {message}"))?;
            run.stderr = cpu_runs_as_stated(&run.stderr, reason.as_deref(), false)
                .map_err(|message| format!("{label}: {message}"))?;
            if run.status != Some(0) || !run.stderr.is_empty() {
                return Err(format!(
                    "{label}: exited with {:?}\n--- answers\n{}--- stderr\n{}---",
                    run.status, run.drive, run.stderr
                ));
            }
            devices.lock().unwrap().insert(said);
            let out = std::fs::read_to_string(source.with_extension("out")).unwrap_or_default();
            if run.stdout != out {
                return Err(format!(
                    "{label}: standard output differs from the .out file\n--- actual\n{}---",
                    run.stdout
                ));
            }
            let count =
                check_drawn(&run.drive, &files).map_err(|message| format!("{label}: {message}"))?;
            if count == 0 {
                return Err(format!("{label}: no capture after a `draws`"));
            }
            captures.fetch_add(count, Ordering::Relaxed);
        }
        Ok(())
    });
    let _ = std::fs::remove_dir_all(&dir);
    report("programs that draw", jobs.len(), failures);
    let devices: Vec<String> = devices.into_inner().unwrap().into_iter().collect();
    note(&format!(
        "window_drawn: {} runs of {} programs, {} captures, on {}",
        jobs.len() * scales.len(),
        sources.len(),
        captures.into_inner(),
        devices.join(" and ")
    ));
}

/// The CRC-32 of PNG and of zlib's gzip.
fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &byte in bytes {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                0xedb8_8320 ^ (crc >> 1)
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

/// The width, height and pixels of a PNG file of 8-bit RGBA, not
/// interlaced, every row unfiltered: what `capture` writes. Every chunk's
/// CRC, the zlib header and the Adler-32 are checked. The deflate stream
/// may have stored blocks and blocks of fixed codes.
fn png_pixels(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>), String> {
    let rest = bytes
        .strip_prefix(b"\x89PNG\r\n\x1a\n")
        .ok_or("no PNG signature")?;
    let word = |at: usize| -> Result<u32, String> {
        let four = rest.get(at..at + 4).ok_or("the file is cut short")?;
        Ok(u32::from_be_bytes(four.try_into().unwrap()))
    };
    let mut at = 0;
    let mut size = None;
    let mut stream = Vec::new();
    loop {
        let length = word(at)? as usize;
        let typed = rest
            .get(at + 4..at + 8 + length)
            .ok_or("the file is cut short")?;
        if crc32(typed) != word(at + 8 + length)? {
            return Err(format!(
                "the CRC of a chunk `{}` is wrong",
                String::from_utf8_lossy(&typed[..4])
            ));
        }
        let (kind, body) = typed.split_at(4);
        at += 12 + length;
        match kind {
            b"IHDR" if size.is_none() && body.len() == 13 => {
                if body[8..] != [8, 6, 0, 0, 0] {
                    return Err(format!("not 8-bit RGBA unfiltered: {:?}", &body[8..]));
                }
                let width = u32::from_be_bytes(body[0..4].try_into().unwrap());
                let height = u32::from_be_bytes(body[4..8].try_into().unwrap());
                size = Some((width, height));
            }
            b"IDAT" if size.is_some() => stream.extend_from_slice(body),
            b"IEND" if size.is_some() && body.is_empty() => break,
            _ => {
                return Err(format!(
                    "an unexpected chunk `{}`",
                    String::from_utf8_lossy(kind)
                ));
            }
        }
    }
    if at != rest.len() {
        return Err("bytes after IEND".to_string());
    }
    let (width, height) = size.ok_or("no IHDR")?;
    let raw = zlib_inflate(&stream)?;
    let stride = 1 + width as usize * 4;
    if raw.len() != stride * height as usize {
        return Err(format!(
            "{} bytes of rows, {width}x{height} pixels need {}",
            raw.len(),
            stride * height as usize
        ));
    }
    let mut pixels = Vec::with_capacity(raw.len());
    for row in raw.chunks(stride) {
        if row[0] != 0 {
            return Err(format!("a row has the filter {}", row[0]));
        }
        pixels.extend_from_slice(&row[1..]);
    }
    Ok((width, height, pixels))
}

/// Reads bits of a deflate stream, the lowest bit of each byte first.
struct Bits<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Bits<'_> {
    fn bit(&mut self) -> Result<u32, String> {
        let byte = self
            .bytes
            .get(self.at / 8)
            .ok_or("the deflate stream is cut short")?;
        let bit = (byte >> (self.at % 8)) & 1;
        self.at += 1;
        Ok(u32::from(bit))
    }

    /// `n` bits, the first the lowest.
    fn number(&mut self, n: u32) -> Result<u32, String> {
        let mut value = 0;
        for i in 0..n {
            value |= self.bit()? << i;
        }
        Ok(value)
    }

    /// A Huffman code of `n` bits, its highest bit first.
    fn code(&mut self, n: u32) -> Result<u32, String> {
        let mut value = 0;
        for _ in 0..n {
            value = value << 1 | self.bit()?;
        }
        Ok(value)
    }

    /// A symbol of the fixed literal and length code.
    fn symbol(&mut self) -> Result<u32, String> {
        let code = self.code(7)?;
        if code <= 0x17 {
            return Ok(256 + code);
        }
        let code = code << 1 | self.bit()?;
        match code {
            0x30..=0xbf => Ok(code - 0x30),
            0xc0..=0xc7 => Ok(280 + code - 0xc0),
            _ => Ok(144 + (code << 1 | self.bit()?) - 0x190),
        }
    }
}

/// The bytes of a zlib stream of stored blocks and blocks of fixed codes.
fn zlib_inflate(stream: &[u8]) -> Result<Vec<u8>, String> {
    const LENGTH_BASE: [u32; 29] = [
        3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115,
        131, 163, 195, 227, 258,
    ];
    const LENGTH_EXTRA: [u32; 29] = [
        0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
    ];
    const DISTANCE_BASE: [u32; 30] = [
        1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
        2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
    ];
    const DISTANCE_EXTRA: [u32; 30] = [
        0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12,
        13, 13,
    ];
    if stream.len() < 6 || stream[0] & 0x0f != 8 || stream[1] & 0x20 != 0 {
        return Err("not a zlib stream of deflate without a dictionary".to_string());
    }
    if (u32::from(stream[0]) << 8 | u32::from(stream[1])) % 31 != 0 {
        return Err("the zlib header's check is wrong".to_string());
    }
    let mut bits = Bits {
        bytes: &stream[..stream.len() - 4],
        at: 16,
    };
    let mut out: Vec<u8> = Vec::new();
    loop {
        let last = bits.number(1)? == 1;
        match bits.number(2)? {
            0 => {
                bits.at = bits.at.div_ceil(8) * 8;
                let length = bits.number(16)?;
                if bits.number(16)? != !length & 0xffff {
                    return Err("a stored block's length is wrong".to_string());
                }
                for _ in 0..length {
                    out.push(bits.number(8)? as u8);
                }
            }
            1 => loop {
                let symbol = bits.symbol()?;
                match symbol {
                    0..=255 => out.push(symbol as u8),
                    256 => break,
                    257..=285 => {
                        let l = (symbol - 257) as usize;
                        let length = LENGTH_BASE[l] + bits.number(LENGTH_EXTRA[l])?;
                        let d = bits.code(5)? as usize;
                        if d >= 30 {
                            return Err(format!("the distance code {d}"));
                        }
                        let distance =
                            (DISTANCE_BASE[d] + bits.number(DISTANCE_EXTRA[d])?) as usize;
                        if distance > out.len() {
                            return Err("a copy from before the start".to_string());
                        }
                        for _ in 0..length {
                            out.push(out[out.len() - distance]);
                        }
                    }
                    _ => return Err(format!("the length symbol {symbol}")),
                }
            },
            kind => return Err(format!("a deflate block of type {kind}")),
        }
        if last {
            break;
        }
    }
    if bits.at.div_ceil(8) != bits.bytes.len() {
        return Err("bytes after the last deflate block".to_string());
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &byte in &out {
        a = (a + u32::from(byte)) % 65521;
        b = (b + a) % 65521;
    }
    let adler = u32::from_be_bytes(stream[stream.len() - 4..].try_into().unwrap());
    if adler != b << 16 | a {
        return Err("the Adler-32 is wrong".to_string());
    }
    Ok(out)
}

/// The CRC-32 of the reference renderer's frame of `width` by `height`
/// pixels with the background `rgb` and no draws.
fn frame_crc(width: u32, height: u32, rgb: [u8; 3]) -> u32 {
    let frame = render::Frame {
        width,
        height,
        background: [rgb[0], rgb[1], rgb[2], 255],
        draws: Vec::new(),
    };
    crc32(&render::render(&frame, width, height).rgba)
}

/// The CRCs in the `.frames` files of the frame programs are those of the
/// colors the programs paint: in `frame_bytes`, every value of a byte in
/// each of red, green and blue.
#[test]
fn frames_are_the_colors_painted() {
    let answers = std::fs::read_to_string("tests/run/frame_bytes.frames").expect("read answers");
    let crcs: Vec<u32> = answers
        .lines()
        .filter_map(capture_answer)
        .map(|(_, _, _, crc)| crc)
        .collect();
    let painted: Vec<u32> = (0..256u32)
        .map(|k| frame_crc(16, 8, [k as u8, 255 - k as u8, (k * 7 % 256) as u8]))
        .collect();
    assert_eq!(crcs, painted);

    let answers = std::fs::read_to_string("tests/run/frame_sizes.frames").expect("read answers");
    let captures: Vec<(&str, u32, u32, u32)> = answers.lines().filter_map(capture_answer).collect();
    let painted = [
        ("open.png", 64, 48, [255, 128, 0]),
        ("first.png", 64, 48, [0, 0, 255]),
        ("still.png", 64, 48, [0, 0, 255]),
        ("scaled.png", 45, 32, [0, 0, 255]),
        ("again.png", 20, 10, [10, 20, 30]),
        ("again-ticked.png", 20, 10, [10, 20, 30]),
    ]
    .map(|(file, width, height, rgb)| (file, width, height, frame_crc(width, height, rgb)));
    assert_eq!(captures, painted);
}

/// A PNG file of a stored block and one of a fixed-code block with a copy
/// both decode, and a wrong CRC is found.
#[test]
fn png_files_are_read() {
    fn chunk(kind: &[u8], body: &[u8]) -> Vec<u8> {
        let mut typed = kind.to_vec();
        typed.extend_from_slice(body);
        let mut bytes = (body.len() as u32).to_be_bytes().to_vec();
        bytes.extend_from_slice(&typed);
        bytes.extend_from_slice(&crc32(&typed).to_be_bytes());
        bytes
    }
    fn png(stream: &[u8]) -> Vec<u8> {
        let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
        bytes.extend(chunk(b"IHDR", &[0, 0, 0, 2, 0, 0, 0, 1, 8, 6, 0, 0, 0]));
        bytes.extend(chunk(b"IDAT", stream));
        bytes.extend(chunk(b"IEND", &[]));
        bytes
    }
    let raw = [0u8, 1, 2, 3, 4, 1, 2, 3, 4];
    let mut adler = [0u8; 4];
    let (mut a, mut b) = (1u32, 0u32);
    for &byte in &raw {
        a = (a + u32::from(byte)) % 65521;
        b = (b + a) % 65521;
    }
    adler.copy_from_slice(&(b << 16 | a).to_be_bytes());

    let mut stored = vec![0x78, 0x01, 0x01, 9, 0, !9, 0xff];
    stored.extend_from_slice(&raw);
    stored.extend_from_slice(&adler);
    assert_eq!(
        png_pixels(&png(&stored)),
        Ok((2, 1, vec![1, 2, 3, 4, 1, 2, 3, 4]))
    );

    // The literals 0, 1, 2, 3, 4, a copy of 4 bytes from 4 back, the end:
    // the bits of a fixed-code block, written lowest first.
    let mut fixed = vec![0x78, 0x01];
    let mut bits: Vec<u32> = vec![1, 1, 0];
    let mut code = |value: u32, n: u32| {
        for i in (0..n).rev() {
            bits.push(value >> i & 1);
        }
    };
    for literal in 0..5 {
        code(0x30 + literal, 8);
    }
    code(258 - 256, 7);
    code(3, 5);
    code(0, 7);
    for byte in bits.chunks(8) {
        fixed.push(
            byte.iter()
                .enumerate()
                .fold(0u8, |acc, (i, bit)| acc | (*bit as u8) << i),
        );
    }
    fixed.extend_from_slice(&adler);
    assert_eq!(
        png_pixels(&png(&fixed)),
        Ok((2, 1, vec![1, 2, 3, 4, 1, 2, 3, 4]))
    );

    let mut broken = png(&stored);
    broken[30] ^= 1;
    assert!(png_pixels(&broken).is_err());
    assert_eq!(
        capture_answer("capture a: b.png: 3 ticks, 320x200, crc 5f1d02c9"),
        Some(("a: b.png", 320, 200, 0x5f1d02c9))
    );
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

const DRAWS_ANSWER: &str = "draws: 1 tick, 64x48, 2 draws\n  \
                            Window.background = Color(0, 0, 0, 255)\n  \
                            Draw.circle(12.5, 1e+30, 9.0, Color(0, 200, 0, 160))\n  \
                            Draw.line(-0.0, NaN, Infinity, 20.0, 3.0, Color(255, 255, 0, 255))\n";

#[test]
fn equal_draws_match_with_and_without_a_tolerance() {
    assert_eq!(compare_draws(DRAWS_ANSWER, DRAWS_ANSWER, None), Ok(0.0));
    assert_eq!(
        compare_draws(DRAWS_ANSWER, DRAWS_ANSWER, Some(0.01)),
        Ok(0.0)
    );
    assert_eq!(compare_draws("", "", None), Ok(0.0));
}

#[test]
fn without_a_tolerance_draws_must_be_equal() {
    let near = DRAWS_ANSWER.replace("12.5,", "12.500001,");
    assert!(compare_draws(DRAWS_ANSWER, &near, None).is_err());
}

#[test]
fn a_shape_s_numbers_may_differ_within_the_tolerance() {
    let near = DRAWS_ANSWER
        .replace("12.5,", "12.51,")
        .replace("1e+30,", "1.001e+30,")
        .replace("20.0,", "20,");
    let difference = compare_draws(DRAWS_ANSWER, &near, Some(0.002)).unwrap();
    assert!((difference - 0.001).abs() < 1e-9, "{difference}");
    let far = DRAWS_ANSWER.replace("12.5,", "12.6,");
    assert!(compare_draws(DRAWS_ANSWER, &far, Some(0.002)).is_err());
    let small = DRAWS_ANSWER.replace("-0.0,", "0.001,");
    assert!(compare_draws(DRAWS_ANSWER, &small, Some(0.002)).is_ok());
}

#[test]
fn not_a_number_and_infinity_match_only_themselves() {
    let number = DRAWS_ANSWER.replace("NaN,", "1.0,");
    assert!(compare_draws(DRAWS_ANSWER, &number, Some(0.5)).is_err());
    let finite = DRAWS_ANSWER.replace("Infinity,", "3.4e+38,");
    assert!(compare_draws(DRAWS_ANSWER, &finite, Some(0.5)).is_err());
}

#[test]
fn everything_but_a_shape_s_numbers_must_be_equal_within_a_tolerance() {
    for (from, to) in [
        ("Color(0, 200, 0, 160)", "Color(0, 201, 0, 160)"),
        ("Draw.circle", "Draw.rect"),
        ("1 tick", "2 ticks"),
        ("2 draws", "3 draws"),
        ("Color(0, 0, 0, 255)", "Color(0, 0, 1, 255)"),
        ("9.0, Color", "9.0, 1.0, Color"),
    ] {
        let changed = DRAWS_ANSWER.replace(from, to);
        assert!(
            compare_draws(DRAWS_ANSWER, &changed, Some(1.0)).is_err(),
            "{from} to {to}"
        );
    }
    let fewer = DRAWS_ANSWER.lines().take(3).collect::<Vec<_>>().join("\n") + "\n";
    assert!(compare_draws(DRAWS_ANSWER, &fewer, Some(1.0)).is_err());
}

#[test]
fn a_program_states_why_a_block_runs_on_the_cpu() {
    let program = "# on the CPU: it binds 1000 storage buffers\n# Wide.\n";
    assert_eq!(
        stated_cpu_reason(program).as_deref(),
        Some("it binds 1000 storage buffers")
    );
    assert_eq!(stated_cpu_reason("# Wide.\nputs 1\n"), None);
}

#[test]
fn a_block_runs_on_the_cpu_only_as_stated() {
    let said = "midnight: gpu: wide (each 1) runs on the CPU: it binds 1000\n";
    let other = "midnight: gpu: narrow (each 1) runs on the CPU: no device\n";
    let strict = "midnight: gpu: s (each 1) runs on the CPU: MIDNIGHT_GPU has strict-cpu\n";
    let rest = "midnight: gpu: strict check passed\n";
    let stderr = format!("{said}{rest}");
    assert_eq!(
        cpu_runs_as_stated(&stderr, Some("it binds 1000"), false),
        Ok(rest.to_string())
    );
    assert!(cpu_runs_as_stated(&stderr, None, false).is_err());
    assert!(cpu_runs_as_stated(rest, Some("it binds 1000"), false).is_err());
    assert!(cpu_runs_as_stated(&format!("{said}{other}"), Some("it binds 1000"), false).is_err());
    assert_eq!(cpu_runs_as_stated(strict, None, true), Ok(String::new()));
    assert!(cpu_runs_as_stated(strict, None, false).is_err());
    assert_eq!(cpu_runs_as_stated(rest, None, false), Ok(rest.to_string()));
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
    /// The answers to `MIDNIGHT_DRIVE`'s commands, without their
    /// `midnight: drive: `. They are not in `stderr`.
    drive: String,
    /// Standard error as the program wrote it, answers included.
    raw: String,
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
        let raw = text(&read_err.join().expect("read standard error"));
        let (drive, stderr) = split_drive(&raw);
        TimedRun {
            status: if timed_out { None } else { status.code() },
            stdout: text(&read_out.join().expect("read standard output")),
            stderr,
            drive,
            raw,
            timed_out,
        }
    })
}

/// Runs `command` as it is, then again with `MIDNIGHT_FORMAT=json`, and
/// checks that the two say the same: the same status and standard output,
/// a JSON form on standard error, and its lines, written as text, the
/// standard error of the first run. Returns the first run.
fn run_both_forms(command: &mut Command, limit: Duration) -> Result<TimedRun, String> {
    let plain = run_within(command, limit);
    command.env("MIDNIGHT_FORMAT", "json");
    let json = run_within(command, limit);
    command.env_remove("MIDNIGHT_FORMAT");
    let differs = |what: &str, plain: &str, json: &str| {
        format!("{what} differs in JSON\n--- text\n{plain}--- JSON\n{json}---")
    };
    if plain.status != json.status || plain.stdout != json.stdout {
        return Err(differs(
            &format!("status {:?} or standard output", json.status),
            &plain.stdout,
            &json.stdout,
        ));
    }
    let lines = json_lines::parse_form(&json.raw).map_err(|e| format!("{e}\n{}", json.raw))?;
    let said = json_lines::runtime_text(&lines)?;
    if said != plain.raw {
        return Err(differs("standard error", &plain.raw, &said));
    }
    Ok(plain)
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

/// The programs of `tests/run`, `examples` and `tests/panic`, and the
/// reference solutions of the eval tasks, that have a loop that can be
/// split, each with whether it must stop with an error.
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
    let mut expected_draws = if source.with_extension("draws").exists() {
        Some(read("draws")?)
    } else {
        None
    };
    for setting in settings {
        let label = format!("{} ({how}, MIDNIGHT_THREADS={setting})", source.display());
        let mut command = on_cpu(exe, source);
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
        let draws = draws_answers(&run.drive);
        match &expected_draws {
            Some(expected) if *expected != draws => {
                return Err(format!(
                    "{label}: the answers of `draws` differ\n--- expected\n{expected}--- actual\n{draws}---"
                ));
            }
            Some(_) => {}
            None => expected_draws = Some(draws),
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
        let mut command = on_cpu(&exe, source);
        command.env("MIDNIGHT_THREADS", "4,small,info");
        let run = run_both_forms(&mut command, RUN_LIMIT).map_err(|e| format!("{label}: {e}"))?;
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

/// Builds a program in release mode and runs it with no GPU device and
/// `MIDNIGHT_THREADS` set to `threads`. Returns what it wrote to standard
/// error; it must exit 0 and print `NAME.out`.
fn threads_said(dir: &Path, source: &Path, threads: &str) -> String {
    let exe = exe_path(dir, source, Mode::Release);
    build(source, &exe, Mode::Release).unwrap_or_else(|message| panic!("{message}"));
    let mut command = on_cpu(&exe, source);
    command.env("MIDNIGHT_THREADS", threads);
    let run = run_both_forms(&mut command, RUN_LIMIT)
        .unwrap_or_else(|message| panic!("{}: {message}", source.display()));
    let expected = std::fs::read_to_string(source.with_extension("out")).expect("read .out");
    assert_eq!(
        (run.status, run.stdout.as_str()),
        (Some(0), expected.as_str()),
        "{} with MIDNIGHT_THREADS={threads}\n{}",
        source.display(),
        run.stderr
    );
    run.stderr
}

/// The size of a piece and the number of threads follow the work of a
/// loop. The first loop of `tests/run/cores_emit.12am` has 30,000 rows of
/// weight 59: pieces of 25,000 / 59 = 423 rows, so 71 of them, and one
/// thread for every 200,000 of its 1,770,000 of work, so 8, or as many as
/// there are. Its second loop, 15,993 rows of weight 19, has less than
/// 800,000 of work and is not split.
#[test]
fn pieces_and_threads_follow_the_work() {
    let dir = scratch("pieces");
    let source = Path::new("tests/run/cores_emit.12am");
    let split = "tests/run/cores_emit.12am:28: 30000 rows in 71 pieces on";
    assert_eq!(
        threads_said(&dir, source, "16,info"),
        format!("{THREADS_SAID}16\n{THREADS_SAID}{split} 8 threads\n")
    );
    assert_eq!(
        threads_said(&dir, source, "4,info"),
        format!("{THREADS_SAID}4\n{THREADS_SAID}{split} 4 threads\n")
    );
    assert_eq!(
        threads_said(&dir, source, "1,info"),
        format!("{THREADS_SAID}1\n")
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A program with a window that opens it, lets ticks run until it is
/// closed, and ticks twice more.
const DRIVEN: &str = "\
Window.open(\"driven\", 100, 100)
World.run
World.tick
World.tick
";

/// Builds `text` in release mode as `dir/NAME` and returns its path.
fn build_text(dir: &Path, name: &str, text: &str) -> PathBuf {
    let source = dir.join(format!("{name}.12am"));
    std::fs::write(&source, text).expect("write a test program");
    let exe = dir.join(name);
    build(&source, &exe, Mode::Release).unwrap_or_else(|message| panic!("{message}"));
    exe
}

/// Runs a program with no device, `MIDNIGHT_WINDOW` set to `words` and the
/// commands in `commands`, written to `dir/NAME.drive`, in both forms.
/// Returns the run and the name of the file of commands.
fn drive(dir: &Path, exe: &Path, name: &str, words: &str, commands: &str) -> (TimedRun, String) {
    let file = dir.join(format!("{name}.drive"));
    std::fs::write(&file, commands).expect("write commands");
    let mut command = Command::new(exe);
    command
        .env("MIDNIGHT_GPU", "off")
        .env("MIDNIGHT_WINDOW", words)
        .env("MIDNIGHT_DRIVE", &file);
    no_display(&mut command);
    let run = run_both_forms(&mut command, PANIC_LIMIT)
        .unwrap_or_else(|message| panic!("{name}: {message}"));
    (run, file.to_string_lossy().into_owned())
}

/// Every program of `tests/run`, `examples` and `tests/panic`, in both
/// build modes and both forms: with `MIDNIGHT_FORMAT=json` it prints what
/// it prints without, and its standard error is that of the text form as
/// JSON lines.
#[test]
fn programs_in_json() {
    let dir = scratch("programs-json");
    let mut sources = programs("tests/run");
    sources.extend(examples());
    sources.extend(programs("tests/panic"));
    let jobs: Vec<(&PathBuf, Mode)> = sources
        .iter()
        .flat_map(|source| MODES.map(|mode| (source, mode)))
        .collect();
    let failures = for_each_parallel(&jobs, |(source, mode)| {
        let label = |message: String| format!("{} ({mode:?}): {message}", source.display());
        let exe = exe_path(&dir, source, *mode);
        build(source, &exe, *mode)?;
        let run = run_both_forms(&mut on_cpu(&exe, source), RUN_LIMIT).map_err(label)?;
        let out = source.with_extension("out");
        if out.exists() && std::fs::read_to_string(&out).unwrap() != run.stdout {
            return Err(label("standard output differs from its `.out`".to_string()));
        }
        Ok(())
    });
    let _ = std::fs::remove_dir_all(&dir);
    report("programs in JSON", jobs.len(), failures);
}

/// Builds `tests/runtime/lines.c` in release mode, or with `MN_DEBUG`.
fn build_lines(dir: &Path, debug: bool) -> PathBuf {
    let exe = dir.join(if debug { "lines-debug" } else { "lines" });
    let mut args: Vec<OsString> = ["-std=c11", "-pthread", "-ffp-contract=off"]
        .map(OsString::from)
        .to_vec();
    args.push(if debug { "-DMN_DEBUG" } else { "-O2" }.into());
    args.extend(STRICT_CFLAGS.split_whitespace().map(OsString::from));
    args.extend([
        "-o".into(),
        exe.clone().into(),
        "tests/runtime/lines.c".into(),
        "-lm".into(),
    ]);
    let args: Vec<&OsStr> = args.iter().map(OsString::as_os_str).collect();
    cc(&args);
    exe
}

/// Each finite `Float32`'s text reads back as the same value through
/// `strtof` and through `strtod` rounded to 32 bits, and no text with
/// fewer significant digits does both: every pattern with
/// `MIDNIGHT_TEST_FLOAT32=1`, otherwise every power of two, the two values
/// the rule changes and a million others.
#[test]
fn float32_text_round_trips() {
    let dir = scratch("float32");
    let exe = build_lines(&dir, false);
    let all = std::env::var_os("MIDNIGHT_TEST_FLOAT32").is_some();
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    let output = Command::new(&exe)
        .args(["float32", if all { "all" } else { "sample" }])
        .arg(threads.to_string())
        .output()
        .expect("run the Float32 check");
    let said = text(&output.stdout);
    assert!(output.status.success(), "{said}{}", text(&output.stderr));
    note(said.trim_end());
    let _ = std::fs::remove_dir_all(&dir);
}

/// The lines no program can be made to write on demand, in both forms: a
/// GPU error, a window error, running out of memory (the runtime's and the
/// pool's) and buffers alive at exit.
#[test]
fn lines_no_program_writes_on_demand() {
    let dir = scratch("lines");
    let exe = build_lines(&dir, true);
    let first = "{\"kind\":\"midnight\",\"format\":1,\"version\":\"test\"}\n";
    for (mode, status, text_line, json_line) in [
        (
            "gpu_error",
            Some(70),
            "midnight: GPU error: lost the device\n",
            "{\"kind\":\"gpu_error\",\"message\":\"lost the device\"}\n",
        ),
        (
            "window_error",
            Some(70),
            "midnight: window error: lost the \"window\"\n",
            "{\"kind\":\"window_error\",\"message\":\"lost the \\\"window\\\"\"}\n",
        ),
        (
            "out_of_memory",
            None,
            "midnight: out of memory\n",
            "{\"kind\":\"out_of_memory\"}\n",
        ),
        (
            "buffers_alive",
            Some(70),
            "midnight: 1 buffer still alive at exit\n",
            "{\"kind\":\"buffers_alive\",\"count\":1}\n",
        ),
    ] {
        let run = |format: &str| {
            let output = Command::new(&exe)
                .arg(mode)
                .env("MIDNIGHT_FORMAT", format)
                .output()
                .expect("run the lines");
            (output.status.code(), text(&output.stderr))
        };
        assert_eq!(run("text"), (status, text_line.to_string()), "{mode}");
        assert_eq!(
            run("json"),
            (status, format!("{first}{json_line}")),
            "{mode}"
        );
    }

    let pool = dir.join("pool-line");
    cc(&[
        OsStr::new("-std=c11"),
        OsStr::new("-pthread"),
        OsStr::new("-o"),
        pool.as_os_str(),
        OsStr::new("tests/runtime/pool_line.c"),
    ]);
    for (word, line) in [
        ("json", "{\"kind\":\"out_of_memory\"}\n"),
        ("text", "midnight: out of memory\n"),
    ] {
        let output = Command::new(&pool)
            .arg(word)
            .output()
            .expect("run the pool line");
        assert_eq!(
            (output.status.code(), text(&output.stderr).as_str()),
            (None, line)
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// `MIDNIGHT_FORMAT` takes `text` or `json`; any other value is reported
/// once and the program writes text.
#[test]
fn the_format_variable_takes_one_word() {
    let dir = scratch("format-variable");
    let exe = dir.join("hello");
    build(Path::new("examples/hello.12am"), &exe, Mode::Release)
        .unwrap_or_else(|message| panic!("{message}"));
    let run = |value: &str| {
        let output = Command::new(&exe)
            .env("MIDNIGHT_FORMAT", value)
            .output()
            .expect("run hello");
        (text(&output.stdout), text(&output.stderr))
    };
    let plain = run("text");
    assert_eq!(plain.1, "");
    assert_eq!(run(""), plain);
    for value in ["JSON", "json,text", "xml"] {
        assert_eq!(
            run(value),
            (
                plain.0.clone(),
                format!("midnight: MIDNIGHT_FORMAT: unknown word `{value}`\n")
            ),
            "{value}"
        );
    }
    let (stdout, stderr) = run("json");
    assert_eq!(stdout, plain.0);
    assert_eq!(json_lines::parse_form(&stderr), Ok(vec![]));
    let _ = std::fs::remove_dir_all(&dir);
}

/// `midnight run --format json` writes the compiler's lines, then the
/// program's, which begin with its own first line; `--format text` takes
/// `MIDNIGHT_FORMAT` away from the program.
#[test]
fn run_passes_the_format() {
    let output = midnight()
        .args(["run", "tests/run/gpu_transfers.12am", "--format", "json"])
        .env("MIDNIGHT_GPU", "off,bogus")
        .env_remove("MIDNIGHT_GPU_FLAGS")
        .env_remove("MIDNIGHT_FORMAT")
        .output()
        .expect("run midnight");
    assert_eq!(output.status.code(), Some(0));
    let stderr = text(&output.stderr);
    let first = json_first_line();
    let (compiler, program) = stderr[first.len()..]
        .split_once(&first)
        .unwrap_or_else(|| panic!("no second first line:\n{stderr}"));
    assert!(stderr.starts_with(&first), "{stderr}");
    assert!(compiler.contains("\"kind\":\"no_flags\""), "{compiler}");
    assert_eq!(
        program,
        "{\"kind\":\"unknown_word\",\"variable\":\"MIDNIGHT_GPU\",\"word\":\"bogus\"}\n"
    );
    let output = midnight()
        .args(["run", "tests/run/gpu_transfers.12am", "--format", "text"])
        .env("MIDNIGHT_GPU", "off,bogus")
        .env("MIDNIGHT_FORMAT", "json")
        .output()
        .expect("run midnight");
    let stderr = text(&output.stderr);
    assert!(
        stderr.ends_with("midnight: MIDNIGHT_GPU: unknown word `bogus`\n"),
        "{stderr}"
    );
    assert!(!stderr.contains('{'), "{stderr}");
}

/// Every way a command can be wrong stops the program with status 1 and a
/// line that names the file and the line, after the answers to the
/// commands before it.
#[test]
fn drive_errors() {
    let dir = scratch("drive-errors");
    let exe = build_text(&dir, "driven", DRIVEN);
    let cases: &[(&str, &str, &str)] = &[
        ("tick\nfly\n", "tick\n", "2: unknown command `fly`"),
        (
            "tick 0\n",
            "",
            "1: `tick` takes a number of ticks from 1 up, or nothing",
        ),
        (
            "tick two\n",
            "",
            "1: `tick` takes a number of ticks from 1 up, or nothing",
        ),
        (
            "tick 1 2\n",
            "",
            "1: `tick` takes a number of ticks from 1 up, or nothing",
        ),
        (
            "# a comment\n\nresize 10\n",
            "",
            "3: `resize` takes a width and a height in points, each from 1 to 8192",
        ),
        (
            "resize 0 10\n",
            "",
            "1: `resize` takes a width and a height in points, each from 1 to 8192",
        ),
        (
            "resize 8193 10\n",
            "",
            "1: `resize` takes a width and a height in points, each from 1 to 8192",
        ),
        (
            "resize 10 10 10\n",
            "",
            "1: `resize` takes a width and a height in points, each from 1 to 8192",
        ),
        ("scale 0.4\n", "", "1: `scale` takes a number from 0.5 to 4"),
        (
            "scale 4.01\n",
            "",
            "1: `scale` takes a number from 0.5 to 4",
        ),
        ("scale 1e2\n", "", "1: `scale` takes a number from 0.5 to 4"),
        ("scale -1\n", "", "1: `scale` takes a number from 0.5 to 4"),
        ("scale\n", "", "1: `scale` takes a number from 0.5 to 4"),
        (
            "scale 4\nresize 4000 10\n",
            "scale 4\n",
            "2: a frame of 16000x40 pixels is larger than 8192 by 8192",
        ),
        ("close now\n", "", "1: `close` takes nothing"),
        ("capture\n", "", "1: `capture` takes the name of a file"),
        (
            "capture frame.png\n",
            "",
            "1: can't capture: the program has no device",
        ),
        (
            "close\ntick\ncapture frame.png\n",
            "close\ntick\n",
            "3: can't capture: no window is open",
        ),
        ("draws all\n", "", "1: `draws` takes nothing"),
        (
            "close\ntick\ndraws\n",
            "close\ntick\n",
            "3: can't list the draws: no window is open",
        ),
        // The keyboard and the mouse: names and numbers first.
        ("key Q2\n", "", "1: `Q2` is not a key"),
        ("key a\n", "", "1: `a` is not a key"),
        (
            "key\n",
            "",
            "1: `key` takes `down` or `up` and a key, or a key",
        ),
        (
            "key press A\n",
            "",
            "1: `key` takes `down` or `up` and a key, or a key",
        ),
        ("mouse Middle2\n", "", "1: `Middle2` is not a mouse button"),
        ("mouse down left\n", "", "1: `left` is not a mouse button"),
        (
            "mouse\n",
            "",
            "1: `mouse` takes `move` and a position, `down` or `up` and a button, `leave`, or a button",
        ),
        (
            "mouse move 1.123456789 3\n",
            "",
            "1: `mouse move` takes a position in points, each a number from -8192 to 8192",
        ),
        (
            "mouse move 1. 3\n",
            "",
            "1: `mouse move` takes a position in points, each a number from -8192 to 8192",
        ),
        (
            "mouse move 3 .5\n",
            "",
            "1: `mouse move` takes a position in points, each a number from -8192 to 8192",
        ),
        (
            "mouse move -8192.00000001 3\n",
            "",
            "1: `mouse move` takes a position in points, each a number from -8192 to 8192",
        ),
        (
            "mouse move 1e2 3\n",
            "",
            "1: `mouse move` takes a position in points, each a number from -8192 to 8192",
        ),
        (
            "mouse move 3\n",
            "",
            "1: `mouse move` takes a position in points, each a number from -8192 to 8192",
        ),
        (
            "scroll 1001 0\n",
            "",
            "1: `scroll` takes two numbers of notches, each from -1000 to 1000",
        ),
        (
            "scroll +1 0\n",
            "",
            "1: `scroll` takes two numbers of notches, each from -1000 to 1000",
        ),
        (
            "scroll 1\n",
            "",
            "1: `scroll` takes two numbers of notches, each from -1000 to 1000",
        ),
        ("blur now\n", "", "1: `blur` takes nothing"),
        ("focus now\n", "", "1: `focus` takes nothing"),
        ("blur\nkey down Nope\n", "blur\n", "2: `Nope` is not a key"),
        // Then the focus.
        (
            "blur\nkey A\n",
            "blur\n",
            "2: `key A` while the window does not have the focus",
        ),
        (
            "blur\nkey up A\n",
            "blur\n",
            "2: `key up A` while the window does not have the focus",
        ),
        (
            "close\nkey down A\n",
            "close\n",
            "2: `key down A` while the window does not have the focus",
        ),
        // Then what is held.
        (
            "key down A\nkey down A\n",
            "key down A\n",
            "2: `A` is already down",
        ),
        (
            "key down A\nkey A\n",
            "key down A\n",
            "2: `A` is already down",
        ),
        ("key up A\n", "", "1: `A` is already up"),
        (
            "key down A\nblur\nfocus\nkey up A\n",
            "key down A\nblur\nfocus\n",
            "4: `A` is already up",
        ),
        (
            "mouse move 3 3\nmouse down Left\nmouse Left\n",
            "mouse move 3 3\nmouse down Left\n",
            "3: `Left` is already down",
        ),
        ("mouse up Back\n", "", "1: `Back` is already up"),
        // Then the pointer.
        (
            "mouse move 100 3\n",
            "",
            "1: `mouse move 100 3` is outside the window of 100x100 points while no button is held; a pointer that leaves is `mouse leave`",
        ),
        (
            "mouse move 5 5\nmouse down Left\nmouse move 200 -3\nmouse up Left\nmouse move 300 3\n",
            "mouse move 5 5\nmouse down Left\nmouse move 200 -3\nmouse up Left\n",
            "5: `mouse move 300 3` is outside the window of 100x100 points while no button is held; a pointer that leaves is `mouse leave`",
        ),
        (
            "mouse Left\n",
            "",
            "1: `mouse Left` while the pointer is not over the window and no button is held",
        ),
        (
            "mouse move 3 3\nmouse leave\nmouse down Right\n",
            "mouse move 3 3\nmouse leave\n",
            "3: `mouse down Right` while the pointer is not over the window and no button is held",
        ),
        (
            "mouse move 50 50\nresize 40 40\nmouse Left\n",
            "mouse move 50 50\nresize 40 40\n",
            "3: `mouse Left` while the pointer is not over the window and no button is held",
        ),
        (
            "mouse leave\n",
            "",
            "1: `mouse leave` while the pointer is not over the window",
        ),
        (
            "mouse move 3 3\nmouse leave\nmouse leave\n",
            "mouse move 3 3\nmouse leave\n",
            "3: `mouse leave` while the pointer is not over the window",
        ),
        // Then the focus commands.
        ("focus\n", "", "1: `focus` while the window has the focus"),
        (
            "blur\nblur\n",
            "blur\n",
            "2: `blur` while the window does not have the focus",
        ),
    ];
    let mut failures = Vec::new();
    for (index, (commands, answers, error)) in cases.iter().enumerate() {
        let (run, file) = drive(&dir, &exe, &format!("case{index}"), "headless", commands);
        let expected = format!("{answers}{file}:{error}\n");
        if run.status != Some(1) || run.drive != expected || !run.stderr.is_empty() {
            failures.push(format!(
                "{commands:?}: exited with {:?}\n--- expected\n{expected}--- answers\n{}--- stderr\n{}---",
                run.status, run.drive, run.stderr
            ));
        }
    }
    // A program that shows its window takes no `resize`; one that never
    // opens it carries the command out when it ends.
    let quiet = build_text(&dir, "quiet", "Window.background = Color(1, 2, 3)\n");
    let (run, file) = drive(&dir, &quiet, "shown", "", "resize 10 10\n");
    let expected = format!("{file}:1: `resize` needs MIDNIGHT_WINDOW=headless\n");
    if run.status != Some(1) || run.drive != expected {
        failures.push(format!(
            "resize with a window: exited with {:?}\n--- expected\n{expected}--- answers\n{}---",
            run.status, run.drive
        ));
    }
    // A file that can't be opened.
    let missing = dir.join("missing").join("commands");
    let mut command = Command::new(&exe);
    command
        .env("MIDNIGHT_GPU", "off")
        .env("MIDNIGHT_WINDOW", "headless")
        .env("MIDNIGHT_DRIVE", &missing);
    let run = run_within(&mut command, PANIC_LIMIT);
    let expected = format!(
        "can't open {}: No such file or directory\n",
        missing.display()
    );
    if run.status != Some(1) || run.drive != expected {
        failures.push(format!(
            "a missing file: exited with {:?}\n--- expected\n{expected}--- answers\n{}---",
            run.status, run.drive
        ));
    }
    let _ = std::fs::remove_dir_all(&dir);
    report("command errors", cases.len() + 2, failures);
}

/// Each command is answered with the command as written, `tick` when its
/// last tick has finished or with the ticks that ran when the program
/// ended first; when the program ends the commands up to the next `tick`
/// are carried out, and that `tick` is not.
#[test]
fn drive_answers() {
    let dir = scratch("drive-answers");
    let exe = build_text(&dir, "driven", DRIVEN);
    let (run, _) = drive(
        &dir,
        &exe,
        "ended",
        "headless",
        "  tick   2  \n# no answer\n\nclose\ntick 5\nclose\n\tclose\ntick\nclose\n",
    );
    assert_eq!(run.status, Some(0), "{}", run.stderr);
    assert_eq!(
        run.drive,
        "tick   2\nclose\ntick 5: the program ended after 3 ticks\nclose\nclose\n"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// Without `headless`, a program that cannot show a window stops at
/// `Window.open` with a run-time error that says why and names
/// `MIDNIGHT_WINDOW`: here, `MIDNIGHT_GPU` has `off`, or no display can be
/// reached, whether the display variables name none or one that is not
/// there.
#[test]
fn a_window_that_cannot_be_shown_is_an_error() {
    let dir = scratch("window-not-shown");
    let exe = build_text(&dir, "driven", DRIVEN);
    let error = |reason: &str| {
        format!(
            "{}:1: runtime error: Window.open: no window can be shown: {reason}; \
             set MIDNIGHT_WINDOW=headless to run without one\n",
            dir.join("driven.12am").display()
        )
    };
    let (run, _) = drive(&dir, &exe, "off", "", "tick\n");
    assert_eq!(
        (run.status, run.stderr),
        (Some(1), error("MIDNIGHT_GPU has `off`"))
    );
    for named in [true, false] {
        let mut command = Command::new(&exe);
        command
            .env_remove("MIDNIGHT_GPU")
            .env_remove("MIDNIGHT_WINDOW")
            .env_remove("MIDNIGHT_DRIVE");
        no_display(&mut command);
        if !named {
            command.env_remove("WAYLAND_DISPLAY").env_remove("DISPLAY");
        }
        let run = run_within(&mut command, PANIC_LIMIT);
        assert_eq!(
            (run.status, run.stderr),
            (Some(1), error("no display was found")),
            "display variables named: {named}"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// Without `MIDNIGHT_GPU_FLAGS` or `MIDNIGHT_WINDOW_FLAGS`, `midnight` says
/// so in one line and builds a program with a window that runs headless
/// and captures nothing, with GPU systems or without, in both modes.
#[test]
fn a_window_built_without_the_flags_runs_headless() {
    let dir = scratch("window-without-flags");
    let cases = [
        ("MIDNIGHT_WINDOW_FLAGS", "MIDNIGHT_WINDOW_FLAGS is"),
        ("MIDNIGHT_GPU_FLAGS", "MIDNIGHT_GPU_FLAGS is"),
    ];
    let mut failures = Vec::new();
    let mut runs = 0;
    for (unset, said) in cases {
        for source in ["tests/run/window_gpu.12am", "tests/run/frame_sizes.12am"] {
            for mode in MODES {
                runs += 1;
                let label = format!("{source} ({mode:?}) without {unset}");
                let exe = dir.join(format!("program-{runs}"));
                let mut command = midnight();
                command
                    .arg("build")
                    .arg(source)
                    .arg("-o")
                    .arg(&exe)
                    .env("MIDNIGHT_CFLAGS", STRICT_CFLAGS)
                    .env_remove(unset);
                if mode == Mode::Debug {
                    command.arg("--debug");
                }
                let output = command.output().expect("run midnight");
                let line = format!(
                    "midnight: {said} not set; the window will run only headless, and capture nothing\n"
                );
                let stderr = text(&output.stderr);
                if !output.status.success() || !stderr.ends_with(&line) {
                    failures.push(format!("{label}: the build said\n{stderr}"));
                    continue;
                }
                let source = Path::new(source);
                let run = run_within(&mut on_cpu(&exe, source), RUN_LIMIT);
                let out = std::fs::read_to_string(source.with_extension("out")).unwrap();
                if run.status != Some(0) || run.stdout != out || !run.stderr.is_empty() {
                    failures.push(format!(
                        "{label}: exited with {:?}\n--- stdout\n{}--- stderr\n{}---",
                        run.status, run.stdout, run.stderr
                    ));
                }
                let commands = dir.join("capture.drive");
                std::fs::write(&commands, "capture frame.png\n").expect("write commands");
                // No device: the capture is refused for the build's flags
                // before one would be used.
                let mut command = Command::new(&exe);
                command
                    .env("MIDNIGHT_GPU", "off")
                    .env("MIDNIGHT_WINDOW", "headless")
                    .env("MIDNIGHT_DRIVE", &commands)
                    .current_dir(&dir);
                let run = run_within(&mut command, PANIC_LIMIT);
                let expected = format!(
                    "{}:1: can't capture: the program was built without the window flags\n",
                    commands.display()
                );
                if run.status != Some(1) || run.drive != expected {
                    failures.push(format!(
                        "{label}: a capture exited with {:?}\n--- answers\n{}---",
                        run.status, run.drive
                    ));
                }
            }
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    report("programs built without the flags", runs, failures);
}

/// A program with a window reports each word of `MIDNIGHT_WINDOW` it does
/// not know, and goes on. A program without one reads neither that
/// variable nor `MIDNIGHT_DRIVE`.
#[test]
fn only_programs_with_a_window_read_the_window_variables() {
    let dir = scratch("window-variables");
    let exe = build_text(&dir, "driven", DRIVEN);
    let (run, _) = drive(&dir, &exe, "words", "bogus,headless,,info", "close\ntick\n");
    assert_eq!(run.status, Some(0), "{}", run.stderr);
    assert_eq!(
        run.stderr,
        "midnight: MIDNIGHT_WINDOW: unknown word `bogus`\n\
         midnight: window: 100x100 points, 100x100 pixels, headless\n\
         midnight: window: input from commands\n"
    );
    let hello = dir.join("hello");
    build(Path::new("examples/hello.12am"), &hello, Mode::Release)
        .unwrap_or_else(|message| panic!("{message}"));
    let mut command = Command::new(&hello);
    command
        .env("MIDNIGHT_WINDOW", "bogus")
        .env("MIDNIGHT_DRIVE", dir.join("missing").join("commands"));
    let run = run_within(&mut command, PANIC_LIMIT);
    assert_eq!(run.status, Some(0));
    assert_eq!((run.stderr.as_str(), run.drive.as_str()), ("", ""));
    let _ = std::fs::remove_dir_all(&dir);
}

/// A program with a loop that can be split reports a word of
/// `MIDNIGHT_THREADS` it does not know, and goes on. A program with no
/// such loop does not read the variable.
#[test]
fn only_programs_that_split_read_the_threads_variable() {
    let dir = scratch("threads-variable");
    assert_eq!(
        threads_said(
            &dir,
            Path::new("tests/run/cores_gpu_rand.12am"),
            "2,bogus,0,300"
        ),
        "midnight: MIDNIGHT_THREADS: unknown word `bogus`\n\
         midnight: MIDNIGHT_THREADS: unknown word `0`\n\
         midnight: MIDNIGHT_THREADS: unknown word `300`\n"
    );
    assert_eq!(
        threads_said(&dir, Path::new("examples/hello.12am"), "2,bogus,info"),
        ""
    );
    let _ = std::fs::remove_dir_all(&dir);
}

// ---- Window mode -----------------------------------------------------------

/// The device the window tests draw on, when `MIDNIGHT_TEST_WINDOW` asks
/// for them: the one `MIDNIGHT_TEST_GPU` names, or the hardware device.
/// They must run inside `tests/window/compositor.sh`.
fn window_device() -> Option<Device> {
    if std::env::var_os("MIDNIGHT_TEST_WINDOW").is_none_or(|value| value.is_empty()) {
        return None;
    }
    assert!(
        std::env::var_os("SWAYSOCK").is_some(),
        "MIDNIGHT_TEST_WINDOW is set: run the window tests inside tests/window/compositor.sh"
    );
    Some(Device::from_env().unwrap_or(Device::Hardware))
}

/// Held by each window test: they share one compositor and its output.
static COMPOSITOR: Mutex<()> = Mutex::new(());

/// Runs `swaymsg` with `args` on the compositor and returns what it says.
fn sway(args: &[&str]) -> String {
    let output = Command::new("swaymsg")
        .args(args)
        .output()
        .expect("run swaymsg");
    assert!(
        output.status.success(),
        "swaymsg {args:?}: {}",
        text(&output.stdout)
    );
    text(&output.stdout)
}

/// Whether the compositor shows the window titled `title`.
fn sway_shows(title: &str) -> Option<bool> {
    let tree: String = sway(&["-t", "get_tree"])
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    let title: String = title.chars().filter(|c| !c.is_whitespace()).collect();
    let at = tree.find(&format!("\"name\":\"{title}\""))?;
    let node = &tree[at..];
    let node = &node[..node.find('{').unwrap_or(node.len())];
    Some(node.contains("\"visible\":true"))
}

/// A program running with a window in the compositor.
struct Shown {
    child: std::process::Child,
    stderr: std::sync::mpsc::Receiver<String>,
    said: String,
    stdout: Option<std::thread::JoinHandle<Vec<u8>>>,
}

impl Shown {
    /// Starts `exe` in `dir` with `MIDNIGHT_WINDOW=info` on `device`, with
    /// `MIDNIGHT_DRIVE` set to `drive` when that is given.
    fn start(exe: &Path, dir: &Path, drive: Option<&Path>, device: Device) -> Shown {
        use std::io::{BufRead, Read};
        let mut command = Command::new(exe);
        command
            .env("MIDNIGHT_WINDOW", "info")
            .env_remove("MIDNIGHT_DRIVE")
            .current_dir(dir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        on_device(&mut command, device);
        if let Some(drive) = drive {
            command.env("MIDNIGHT_DRIVE", drive);
        }
        let mut child = command.spawn().expect("run program");
        let mut stdout = child.stdout.take().expect("piped");
        let stderr = child.stderr.take().expect("piped");
        let (send, stderr_lines) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for line in std::io::BufReader::new(stderr).lines() {
                let Ok(line) = line else { break };
                if send.send(line).is_err() {
                    break;
                }
            }
        });
        let stdout = std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let _ = stdout.read_to_end(&mut bytes);
            bytes
        });
        Shown {
            child,
            stderr: stderr_lines,
            said: String::new(),
            stdout: Some(stdout),
        }
    }

    /// Waits until what the program has written to standard error passes
    /// `done`, or the program ends, or `limit` passes. Returns whether it
    /// passed.
    fn wait_for(&mut self, limit: Duration, done: impl Fn(&str) -> bool) -> bool {
        let started = Instant::now();
        while !done(&self.said) {
            let left = limit.saturating_sub(started.elapsed());
            match self
                .stderr
                .recv_timeout(left.min(Duration::from_millis(50)))
            {
                Ok(line) => {
                    self.said.push_str(&line);
                    self.said.push('\n');
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    if left.is_zero() {
                        return false;
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return false,
            }
        }
        true
    }

    /// Waits for the program to end, and kills it when it has not ended
    /// within `limit`. Returns its run.
    fn finish(mut self, limit: Duration) -> TimedRun {
        let started = Instant::now();
        let mut timed_out = false;
        let status = loop {
            if let Some(status) = self.child.try_wait().expect("wait for program") {
                break status;
            }
            if started.elapsed() >= limit {
                timed_out = true;
                let _ = self.child.kill();
                break self.child.wait().expect("wait for program");
            }
            std::thread::sleep(Duration::from_millis(5));
        };
        while let Ok(line) = self.stderr.recv() {
            self.said.push_str(&line);
            self.said.push('\n');
        }
        let stdout = self
            .stdout
            .take()
            .unwrap()
            .join()
            .expect("read standard output");
        let (drive, stderr) = split_drive(&self.said);
        TimedRun {
            status: if timed_out { None } else { status.code() },
            stdout: text(&stdout),
            stderr,
            drive,
            raw: self.said.clone(),
            timed_out,
        }
    }
}

/// Takes the `info` lines of a shown window out of a run's standard error:
/// the device's, the window's, and the one that says a driven window takes
/// its input from its commands. The window's must say it is shown on
/// Wayland. Returns the window's.
fn window_said(run: &mut TimedRun, device: Device) -> Result<String, String> {
    device_said(run, device)?;
    let mut said = None;
    let mut rest = String::new();
    for line in run.stderr.split_inclusive('\n') {
        match line.strip_prefix("midnight: window: ") {
            Some(window) if said.is_none() => said = Some(window.trim_end().to_string()),
            Some("input from commands\n") => {}
            _ => rest.push_str(line),
        }
    }
    run.stderr = rest;
    let said = said.ok_or("no `info` line for the window")?;
    if !said.contains(", wayland, ") {
        return Err(format!("the window is not shown on Wayland: {said}"));
    }
    Ok(said)
}

/// The pixels of a binary PPM file of 8-bit samples: width, height and
/// three bytes a pixel.
fn ppm_pixels(bytes: &[u8]) -> Result<(usize, usize, &[u8]), String> {
    let mut fields = Vec::new();
    let mut at = 0;
    while fields.len() < 4 {
        while bytes.get(at).is_some_and(u8::is_ascii_whitespace) {
            at += 1;
        }
        let start = at;
        while bytes
            .get(at)
            .is_some_and(|byte| !byte.is_ascii_whitespace())
        {
            at += 1;
        }
        fields.push(String::from_utf8_lossy(&bytes[start..at]).into_owned());
    }
    let number = |field: &str| {
        field
            .parse::<usize>()
            .map_err(|_| "not a PPM file".to_string())
    };
    if fields[0] != "P6" || fields[3] != "255" {
        return Err("not a binary PPM file of 8-bit samples".to_string());
    }
    let (width, height) = (number(&fields[1])?, number(&fields[2])?);
    let pixels = bytes.get(at + 1..).ok_or("the file is cut short")?;
    if pixels.len() != width * height * 3 {
        return Err("the file is cut short".to_string());
    }
    Ok((width, height, pixels))
}

/// The programs with a `.frames` file, and those `window_drawn` runs, whose
/// commands a shown window can carry out: none of them is `resize` or
/// `scale`.
fn shown_frame_programs() -> Vec<PathBuf> {
    let mut sources = frame_programs();
    sources.extend(drawn_programs());
    sources.sort();
    sources.dedup();
    sources
        .into_iter()
        .filter(|source| {
            let commands =
                std::fs::read_to_string(source.with_extension("drive")).unwrap_or_default();
            !commands.lines().any(|line| {
                let line = line.trim_start();
                line.starts_with("resize") || line.starts_with("scale")
            })
        })
        .collect()
}

/// Window mode: the frame programs and the programs of `window_drawn` that
/// need neither `resize` nor `scale` run with a window on Wayland and
/// their commands from a pipe, in release and debug builds. Their output
/// must be that of the headless run, and so must their answers when they
/// have a `.frames` file; each capture after a `draws` must agree with the
/// reference renderer; and a picture of the compositor taken after the
/// last command must hold the pixels of the last capture at the window's
/// place.
#[test]
fn window_shown() {
    let Some(device) = window_device() else {
        return;
    };
    let _compositor = COMPOSITOR
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    let sources = shown_frame_programs();
    assert!(
        !sources.is_empty(),
        "no frame program a shown window can run"
    );
    let dir = scratch("window-shown");
    let mut failures = Vec::new();
    for source in &sources {
        for mode in MODES {
            let label = format!("{} ({mode:?}) with a window", source.display());
            if let Err(message) = shown_frames(&dir, source, mode, device) {
                failures.push(format!("{label}: {message}"));
            }
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    report("runs with a window", sources.len() * MODES.len(), failures);
}

/// One run of `window_shown`.
fn shown_frames(dir: &Path, source: &Path, mode: Mode, device: Device) -> Result<(), String> {
    let stem = source.file_stem().unwrap().to_string_lossy();
    let files = dir.join(format!("{stem}-{mode:?}"));
    std::fs::create_dir_all(&files).expect("create a directory for captures");
    let exe = files.join("program");
    build(source, &exe, mode)?;
    let pipe = files.join("commands");
    let made = Command::new("mkfifo")
        .arg(&pipe)
        .status()
        .expect("run mkfifo");
    assert!(made.success(), "mkfifo {}", pipe.display());
    let commands = std::fs::read(source.with_extension("drive")).expect("read commands");
    let expected = std::fs::read_to_string(source.with_extension("frames")).ok();
    let captures = String::from_utf8_lossy(&commands)
        .lines()
        .filter(|line| line.trim_start().starts_with("capture"))
        .count();
    // Without a `.frames` file the commands end at the last capture, so
    // that the window is still shown when the picture is taken.
    let commands = if expected.is_none() {
        let text = String::from_utf8_lossy(&commands).into_owned();
        let lines: Vec<&str> = text.split_inclusive('\n').collect();
        let last = lines
            .iter()
            .rposition(|line| line.trim_start().starts_with("capture"))
            .map_or(lines.len(), |at| at + 1);
        lines[..last].concat().into_bytes()
    } else {
        commands
    };

    let mut shown = Shown::start(&exe, &files, Some(&pipe), device);
    // The program opens the pipe when its first tick begins; the commands
    // are written, and the pipe stays open until the picture is taken.
    let (close, closed) = std::sync::mpsc::channel::<()>();
    let writer = {
        let pipe = pipe.clone();
        std::thread::spawn(move || {
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .open(&pipe)
                .expect("open the pipe");
            let _ = file.write_all(&commands);
            let _ = closed.recv();
        })
    };
    let answered = shown.wait_for(Duration::from_secs(120), |said| {
        let answers = split_drive(said).0;
        match &expected {
            Some(expected) => without_draws(&answers) == *expected,
            None => answers.lines().filter_map(capture_answer).count() == captures,
        }
    });
    let picture = files.join("screen.ppm");
    let taken = answered && {
        std::thread::sleep(Duration::from_millis(300));
        Command::new("grim")
            .args(["-t", "ppm"])
            .arg(&picture)
            .status()
            .is_ok_and(|status| status.success())
    };
    let _ = close.send(());
    if !answered {
        // Unblocks the writer when the program never opened the pipe.
        let _ = std::fs::OpenOptions::new().read(true).open(&pipe);
    }
    let _ = writer.join();
    let mut run = shown.finish(Duration::from_secs(30));
    let said = window_said(&mut run, device);
    run.stderr = cpu_runs_as_stated(&run.stderr, cpu_reason_of(source).as_deref(), false)?;
    if run.status != Some(0) || !run.stderr.is_empty() || !taken {
        return Err(format!(
            "exited with {:?}, picture taken {taken}, {said:?}\n--- answers\n{}--- stderr\n{}---",
            run.status, run.drive, run.stderr
        ));
    }
    said?;
    if let Some(expected) = &expected
        && without_draws(&run.drive) != *expected
    {
        return Err(format!(
            "the answers differ from the headless run's\n--- expected\n{expected}--- actual\n{}---",
            run.drive
        ));
    }
    let out = std::fs::read_to_string(source.with_extension("out")).unwrap_or_default();
    if run.stdout != out {
        return Err(format!(
            "standard output differs from the .out file\n--- actual\n{}---",
            run.stdout
        ));
    }
    check_captures(&run.drive, &files)?;
    check_drawn(&run.drive, &files)?;

    let last = run
        .drive
        .lines()
        .filter_map(capture_answer)
        .next_back()
        .ok_or("no capture")?;
    let capture = std::fs::read(files.join(last.0)).expect("read the capture");
    let (width, height, pixels) = png_pixels(&capture)?;
    let (width, height) = (width as usize, height as usize);
    let screen = std::fs::read(&picture).expect("read the picture");
    let (screen_width, screen_height, screen) = ppm_pixels(&screen)?;
    if width > screen_width || height > screen_height {
        return Err(format!(
            "a capture of {width}x{height} is larger than the picture"
        ));
    }
    let wrong = (0..height)
        .flat_map(|y| (0..width).map(move |x| (x, y)))
        .filter(|&(x, y)| {
            let captured = &pixels[(y * width + x) * 4..][..3];
            let seen = &screen[(y * screen_width + x) * 3..][..3];
            captured != seen
        })
        .count();
    if wrong != 0 {
        return Err(format!(
            "{wrong} of {} pixels of the picture are not those of {}",
            width * height,
            last.0
        ));
    }
    Ok(())
}

/// A program that opens a window titled `TITLE`, runs 120 ticks and prints
/// how many seconds they took.
const KEEPS_TIME: &str = "\
Window.open(\"TITLE\", 320, 200)
start = clock
while Time.tick < 120
  World.tick
end
puts clock - start
";

/// `KEEPS_TIME` with a system that draws ten thousand shapes a tick, of
/// the four kinds.
const KEEPS_TIME_DRAWING: &str = "\
system show
  i = 0
  while i < 2500
    x = (i * 37) % 320
    y = (i * 53) % 200
    c = Color(i % 256, (i * 3) % 256, 200)
    Draw.rect(x, y, 6, 4, c)
    Draw.circle(x + 3, y + 2, 2.5, c)
    Draw.line(x, y, x + 9, y + 5, 1.5, c)
    Draw.triangle(x, y, x + 5, y + 1, x + 2, y + 6, Color(0, 0, 0, 128))
    i += 1
  end
end

Window.open(\"TITLE\", 320, 200)
start = clock
while Time.tick < 120
  World.tick
end
puts clock - start
";

/// Window mode: 120 ticks of a window take 1.9 to 2.3 seconds with the
/// output at 60 Hz, at 144 Hz, and with the window on a workspace that is
/// not shown, in release and debug builds, for a program that draws
/// nothing and for one that draws ten thousand shapes a tick.
#[test]
fn window_keeps_time() {
    let Some(device) = window_device() else {
        return;
    };
    let _compositor = COMPOSITOR
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    let dir = scratch("window-keeps-time");
    let cases = [
        ("at 60 Hz", "midnight keeps time", "1280x720@60Hz"),
        ("at 144 Hz", "midnight keeps time", "1280x720@144Hz"),
        ("not shown", "midnight hidden", "1280x720@60Hz"),
    ];
    let mut failures = Vec::new();
    let mut times = Vec::new();
    let programs = [("", KEEPS_TIME), (", drawing", KEEPS_TIME_DRAWING)];
    for mode in MODES {
        for ((what, title, output), (drawing, program)) in cases
            .iter()
            .flat_map(|case| programs.iter().map(move |program| (*case, *program)))
        {
            let label = format!("{what}{drawing} ({mode:?})");
            let name = format!(
                "{}{}",
                title.replace(' ', "_"),
                drawing.replace([',', ' '], "_")
            );
            let source = dir.join(format!("{name}.12am"));
            std::fs::write(&source, program.replace("TITLE", title)).expect("write a program");
            let exe = dir.join(format!("{name}-{mode:?}"));
            if let Err(message) = build(&source, &exe, mode) {
                failures.push(message);
                continue;
            }
            sway(&["output", "HEADLESS-1", "mode", output]);
            let mut shown = Shown::start(&exe, &dir, None, device);
            let opened = shown.wait_for(Duration::from_secs(30), |said| {
                said.contains("midnight: window: ")
            });
            let visible = if opened { sway_shows(title) } else { None };
            let mut run = shown.finish(Duration::from_secs(30));
            let said = window_said(&mut run, device);
            let seconds = run.stdout.trim().parse::<f64>().ok();
            times.push(format!("{label} {seconds:?}"));
            let shown_as_asked = visible == Some(title != "midnight hidden");
            if run.status != Some(0)
                || !run.stderr.is_empty()
                || said.is_err()
                || !shown_as_asked
                || !seconds.is_some_and(|seconds| (1.9..=2.3).contains(&seconds))
            {
                failures.push(format!(
                    "{label}: exited with {:?}, {said:?}, shown {visible:?}, took {seconds:?} s\n\
                     --- stdout\n{}--- stderr\n{}---",
                    run.status, run.stdout, run.stderr
                ));
            }
        }
    }
    sway(&["output", "HEADLESS-1", "mode", "1280x720@60Hz"]);
    let _ = std::fs::remove_dir_all(&dir);
    note(&format!("window_keeps_time: {}", times.join(", ")));
    report(
        "runs that keep time",
        cases.len() * programs.len() * MODES.len(),
        failures,
    );
}

/// A program with a window that says what happens to it and ends when it
/// is closed.
const WATCHED: &str = "\
Window.open(\"midnight watched\", 320, 200)

system report
  on WindowResized do |size|
    puts \"#{size.width}x#{size.height} points, #{size.pixel_width}x#{size.pixel_height} pixels\"
  end
  on WindowClosed do
    puts \"closed, open #{Window.open?}\"
  end
end

World.run
puts \"World.run ended\"
";

/// Runs `WATCHED` in both build modes with a window, gives the compositor
/// the commands in `act` once it is shown, and requires `printed` on
/// standard output.
fn watched(name: &str, act: &[&[&str]], printed: &str) {
    let Some(device) = window_device() else {
        return;
    };
    let _compositor = COMPOSITOR
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    let dir = scratch(name);
    let source = dir.join("watched.12am");
    std::fs::write(&source, WATCHED).expect("write a program");
    let mut failures = Vec::new();
    for mode in MODES {
        let exe = exe_path(&dir, &source, mode);
        if let Err(message) = build(&source, &exe, mode) {
            failures.push(message);
            continue;
        }
        let mut shown = Shown::start(&exe, &dir, None, device);
        let opened = shown.wait_for(Duration::from_secs(30), |said| {
            said.contains("midnight: window: ")
        });
        if opened {
            for args in act {
                std::thread::sleep(Duration::from_millis(300));
                sway(args);
            }
        }
        let mut run = shown.finish(Duration::from_secs(10));
        let said = window_said(&mut run, device);
        if run.status != Some(0) || !run.stderr.is_empty() || said.is_err() || run.stdout != printed
        {
            failures.push(format!(
                "{mode:?}: exited with {:?}, {said:?}\n--- expected\n{printed}--- stdout\n{}--- stderr\n{}---",
                run.status, run.stdout, run.stderr
            ));
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    report("runs with a window", MODES.len(), failures);
}

/// Window mode: a close by the compositor ends `World.run`, after a tick
/// in which `WindowClosed` is seen.
#[test]
fn window_closes() {
    watched(
        "window-closes",
        &[&["[title=\"midnight watched\"]", "kill"]],
        "closed, open false\nWorld.run ended\n",
    );
}

/// Window mode: a resize by the compositor arrives as one `WindowResized`
/// with the new sizes.
#[test]
fn window_resized() {
    watched(
        "window-resized",
        &[
            &[
                "[title=\"midnight watched\"]",
                "resize",
                "set",
                "400",
                "300",
            ],
            &["[title=\"midnight watched\"]", "kill"],
        ],
        "400x300 points, 400x300 pixels\nclosed, open false\nWorld.run ended\n",
    );
}

// ---- Input from a compositor -----------------------------------------------

/// The evdev code of the key at each position `Key` names, in the order of
/// `Key`.
const KEY_CODES: [(&str, u32); 105] = [
    ("A", 30),
    ("B", 48),
    ("C", 46),
    ("D", 32),
    ("E", 18),
    ("F", 33),
    ("G", 34),
    ("H", 35),
    ("I", 23),
    ("J", 36),
    ("K", 37),
    ("L", 38),
    ("M", 50),
    ("N", 49),
    ("O", 24),
    ("P", 25),
    ("Q", 16),
    ("R", 19),
    ("S", 31),
    ("T", 20),
    ("U", 22),
    ("V", 47),
    ("W", 17),
    ("X", 45),
    ("Y", 21),
    ("Z", 44),
    ("Digit1", 2),
    ("Digit2", 3),
    ("Digit3", 4),
    ("Digit4", 5),
    ("Digit5", 6),
    ("Digit6", 7),
    ("Digit7", 8),
    ("Digit8", 9),
    ("Digit9", 10),
    ("Digit0", 11),
    ("Enter", 28),
    ("Escape", 1),
    ("Backspace", 14),
    ("Tab", 15),
    ("Space", 57),
    ("Minus", 12),
    ("Equal", 13),
    ("BracketLeft", 26),
    ("BracketRight", 27),
    ("Backslash", 43),
    ("Semicolon", 39),
    ("Quote", 40),
    ("Backquote", 41),
    ("Comma", 51),
    ("Period", 52),
    ("Slash", 53),
    ("CapsLock", 58),
    ("F1", 59),
    ("F2", 60),
    ("F3", 61),
    ("F4", 62),
    ("F5", 63),
    ("F6", 64),
    ("F7", 65),
    ("F8", 66),
    ("F9", 67),
    ("F10", 68),
    ("F11", 87),
    ("F12", 88),
    ("PrintScreen", 99),
    ("ScrollLock", 70),
    ("Pause", 119),
    ("Insert", 110),
    ("Home", 102),
    ("PageUp", 104),
    ("Delete", 111),
    ("End", 107),
    ("PageDown", 109),
    ("ArrowRight", 106),
    ("ArrowLeft", 105),
    ("ArrowDown", 108),
    ("ArrowUp", 103),
    ("NumLock", 69),
    ("NumpadDivide", 98),
    ("NumpadMultiply", 55),
    ("NumpadSubtract", 74),
    ("NumpadAdd", 78),
    ("NumpadEnter", 96),
    ("Numpad1", 79),
    ("Numpad2", 80),
    ("Numpad3", 81),
    ("Numpad4", 75),
    ("Numpad5", 76),
    ("Numpad6", 77),
    ("Numpad7", 71),
    ("Numpad8", 72),
    ("Numpad9", 73),
    ("Numpad0", 82),
    ("NumpadDecimal", 83),
    ("IntlBackslash", 86),
    ("ContextMenu", 127),
    ("ControlLeft", 29),
    ("ShiftLeft", 42),
    ("AltLeft", 56),
    ("MetaLeft", 125),
    ("ControlRight", 97),
    ("ShiftRight", 54),
    ("AltRight", 100),
    ("MetaRight", 126),
];

/// The evdev code of each `MouseButton`, in the order of `MouseButton`.
const BUTTON_CODES: [(&str, u32); 5] = [
    ("Left", 272),
    ("Middle", 274),
    ("Right", 273),
    ("Back", 275),
    ("Forward", 276),
];

/// The evdev code of the key `Key::name`.
fn key_code(name: &str) -> Option<u32> {
    KEY_CODES
        .iter()
        .find(|(key, _)| *key == name)
        .map(|&(_, code)| code)
}

/// The evdev code of the button `MouseButton::name`.
fn button_code(name: &str) -> Option<u32> {
    BUTTON_CODES
        .iter()
        .find(|(button, _)| *button == name)
        .map(|&(_, code)| code)
}

/// `tests/window/input.c`, built: the program `MIDNIGHT_TEST_INPUT_CLIENT`
/// names, when it is set.
fn input_client_path() -> Option<PathBuf> {
    std::env::var_os("MIDNIGHT_TEST_INPUT_CLIENT")
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
}

/// The socket of the compositor of `tests/window/compositor.sh`.
fn compositor_socket() -> PathBuf {
    let dir = std::env::var_os("XDG_RUNTIME_DIR").expect("XDG_RUNTIME_DIR inside compositor.sh");
    let display =
        std::env::var_os("WAYLAND_DISPLAY").expect("WAYLAND_DISPLAY inside compositor.sh");
    Path::new(&dir).join(display)
}

/// `tests/window/input.c` connected to a compositor: a virtual keyboard and
/// pointer. While it runs, every window in that compositor gets the focus
/// and the pointer. It is stopped when dropped.
struct InputClient {
    child: std::process::Child,
    stdin: Option<std::process::ChildStdin>,
    answers: std::sync::mpsc::Receiver<String>,
    stderr: Option<std::thread::JoinHandle<String>>,
}

impl InputClient {
    /// Starts the client on the compositor whose socket is `socket`, an
    /// absolute path, and waits until it says its devices are ready.
    fn start(socket: &Path) -> Result<InputClient, String> {
        use std::io::{BufRead, Read};
        let path = input_client_path().ok_or("MIDNIGHT_TEST_INPUT_CLIENT is not set")?;
        let mut child = Command::new(&path)
            .arg(socket)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| format!("can't run {}: {error}", path.display()))?;
        let stdout = child.stdout.take().expect("piped");
        let mut stderr = child.stderr.take().expect("piped");
        let (send, answers) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for line in std::io::BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if send.send(line).is_err() {
                    break;
                }
            }
        });
        let stderr = std::thread::spawn(move || {
            let mut text = String::new();
            let _ = stderr.read_to_string(&mut text);
            text
        });
        let mut client = InputClient {
            stdin: child.stdin.take(),
            child,
            answers,
            stderr: Some(stderr),
        };
        client.answer("ready")?;
        Ok(client)
    }

    /// Waits up to ten seconds for the line `expected`.
    fn answer(&mut self, expected: &str) -> Result<(), String> {
        match self.answers.recv_timeout(Duration::from_secs(10)) {
            Ok(line) if line == expected => Ok(()),
            Ok(line) => Err(format!("the input client said {line:?}, not {expected:?}")),
            Err(_) => Err(format!(
                "the input client did not say {expected:?}: {}",
                self.stop()
            )),
        }
    }

    /// Sends one command and waits until the compositor has handled it.
    fn send(&mut self, command: &str) -> Result<(), String> {
        let Some(stdin) = self.stdin.as_mut() else {
            return Err("the input client is stopped".to_string());
        };
        if let Err(error) = writeln!(stdin, "{command}").and_then(|()| stdin.flush()) {
            return Err(format!("`{command}`: {error}: {}", self.stop()));
        }
        self.answer("ok")
            .map_err(|error| format!("`{command}`: {error}"))
    }

    /// Ends the client's input, waits up to five seconds for it to end and
    /// kills it otherwise. Returns its status and what it wrote to
    /// standard error.
    fn stop(&mut self) -> String {
        self.stdin = None;
        let started = Instant::now();
        let status = loop {
            if let Ok(Some(status)) = self.child.try_wait() {
                break status.code();
            }
            if started.elapsed() > Duration::from_secs(5) {
                let _ = self.child.kill();
                let _ = self.child.wait();
                break None;
            }
            std::thread::sleep(Duration::from_millis(5));
        };
        let said = self
            .stderr
            .take()
            .map(|thread| thread.join().unwrap_or_default())
            .unwrap_or_default();
        format!("status {status:?}, said {said:?}")
    }
}

impl Drop for InputClient {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Runs the input client with `args` and `input`, and returns its status
/// and what it wrote to standard error, or `None` for the status when it
/// ran for more than ten seconds.
fn input_client_run(args: &[&OsStr], input: &str) -> (Option<i32>, String) {
    use std::io::Read;
    let path = input_client_path().expect("MIDNIGHT_TEST_INPUT_CLIENT is set");
    let mut child = Command::new(path)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("run the input client");
    let mut stdin = child.stdin.take().expect("piped");
    let _ = stdin.write_all(input.as_bytes());
    drop(stdin);
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().expect("wait for the input client") {
            break status.code();
        }
        if started.elapsed() > Duration::from_secs(10) {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    let mut said = String::new();
    if let Some(mut stderr) = child.stderr.take() {
        let _ = stderr.read_to_string(&mut said);
    }
    (status, said)
}

/// The input client connects only to an absolute socket path given as its
/// one argument: with none, two, a relative path or a path where no
/// socket is, it stops with status 1 and says why. It runs when
/// `MIDNIGHT_TEST_INPUT_CLIENT` names the client, and needs no compositor.
#[test]
fn input_client_needs_an_absolute_socket() {
    if input_client_path().is_none() {
        return;
    }
    let missing = std::env::temp_dir().join("midnight-no-socket-here");
    let usage = "input: usage: input /ABSOLUTE/PATH/OF/THE/SOCKET\n";
    let cases: [(Vec<&OsStr>, &str); 4] = [
        (vec![], usage),
        (vec![OsStr::new("/a"), OsStr::new("/b")], usage),
        (
            vec![OsStr::new("wayland-0")],
            "input: the socket must be an absolute path: wayland-0\n",
        ),
        (vec![missing.as_os_str()], "input: can't connect to "),
    ];
    let mut failures = Vec::new();
    for (args, said) in &cases {
        let run = input_client_run(args, "");
        if run.0 != Some(1) || !run.1.starts_with(said) {
            failures.push(format!("{args:?}: {run:?}, expected status 1 and {said:?}"));
        }
    }
    report("runs of the input client", cases.len(), failures);
}

/// Window mode: the input client refuses a compositor whose output is not
/// headless. Inside the test compositor a second sway runs with the Wayland
/// backend, whose output is `WL-1`; the client pointed at it stops with
/// status 1 and says so, and that sway lists no virtual device.
#[test]
fn input_client_refuses_a_desktop() {
    if window_device().is_none() {
        return;
    }
    let _compositor = COMPOSITOR
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    assert!(
        input_client_path().is_some(),
        "MIDNIGHT_TEST_INPUT_CLIENT is not set"
    );
    let dir = scratch("input-refuses");
    std::fs::write(dir.join("config"), "xwayland disable\n").expect("write sway's config");
    let mut sway = Command::new("sway")
        .arg("-c")
        .arg(dir.join("config"))
        .env_remove("SWAYSOCK")
        .env_remove("WLR_LIBINPUT_NO_DEVICES")
        .env("XDG_RUNTIME_DIR", &dir)
        .env("WAYLAND_DISPLAY", compositor_socket())
        .env("WLR_BACKENDS", "wayland")
        .env("WLR_RENDERER", "pixman")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(std::fs::File::create(dir.join("sway.log")).expect("create sway's log"))
        .spawn()
        .expect("run sway");
    let found = |prefix: &str, suffix: &str| {
        std::fs::read_dir(&dir).ok()?.flatten().find_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            (name.starts_with(prefix) && name.ends_with(suffix) && !name.ends_with(".lock"))
                .then(|| entry.path())
        })
    };
    let started = Instant::now();
    let (socket, ipc) = loop {
        if let (Some(socket), Some(ipc)) = (found("wayland-", ""), found("sway-ipc.", ".sock")) {
            break (socket, ipc);
        }
        if started.elapsed() > Duration::from_secs(10)
            || sway.try_wait().is_ok_and(|status| status.is_some())
        {
            let _ = sway.kill();
            let _ = sway.wait();
            panic!(
                "the second sway did not start:\n{}",
                std::fs::read_to_string(dir.join("sway.log")).unwrap_or_default()
            );
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let swaymsg = |kind: &str| {
        let output = Command::new("swaymsg")
            .arg("-s")
            .arg(&ipc)
            .args(["-t", kind])
            .output()
            .expect("run swaymsg");
        text(&output.stdout)
    };
    let outputs = swaymsg("get_outputs");
    let run = input_client_run(&[socket.as_os_str()], "key down 30\nkey up 30\n");
    let inputs = swaymsg("get_inputs");
    let _ = sway.kill();
    let _ = sway.wait();
    let mut failures = Vec::new();
    if !outputs.contains("\"name\": \"WL-1\"") {
        failures.push(format!("the second sway's output is not WL-1: {outputs}"));
    }
    let refused = "input: output WL-1 is not headless; nothing was sent\n";
    if run.0 != Some(1) || run.1 != refused {
        failures.push(format!("the input client did not refuse: {run:?}"));
    }
    if inputs.trim() != "[]" {
        failures.push(format!("the second sway has an input device: {inputs}"));
    }
    let _ = std::fs::remove_dir_all(&dir);
    report("refusals", 1, failures);
}

/// Window mode: the input client on the test compositor carries out a
/// key command for every `Key`, a button command for every `MouseButton`
/// and turns of the wheel, and stops with status 1 at a command it does
/// not know or a number out of range.
#[test]
fn input_client_takes_its_commands() {
    if window_device().is_none() {
        return;
    }
    let _compositor = COMPOSITOR
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    let socket = compositor_socket();
    let mut failures = Vec::new();
    let mut good = vec!["sync".to_string(), "scroll 0 0".to_string()];
    for (name, _) in KEY_CODES {
        let code = key_code(name).expect("a key of the table");
        good.push(format!("key down {code}"));
        good.push(format!("key up {code}"));
    }
    for (name, _) in BUTTON_CODES {
        let code = button_code(name).expect("a button of the table");
        good.push(format!("button down {code}"));
        good.push(format!("button up {code}"));
    }
    good.extend(["scroll 1 -2", "scroll -0.5 0.25", "scroll 1000 -1000"].map(String::from));
    match InputClient::start(&socket) {
        Ok(mut client) => {
            for command in &good {
                if let Err(message) = client.send(command) {
                    failures.push(message);
                    break;
                }
            }
            let said = client.stop();
            if said != "status Some(0), said \"\"" {
                failures.push(format!("at the end of its input: {said}"));
            }
        }
        Err(message) => failures.push(message),
    }
    let bad = [
        ("bogus", "unknown command `bogus`"),
        ("", "unknown command ``"),
        ("key down 0", "a key's code is from 1 to 767, not `0`"),
        ("key down 768", "a key's code is from 1 to 767, not `768`"),
        ("key down +30", "a key's code is from 1 to 767, not `+30`"),
        (
            "key sideways 30",
            "`key` takes `down` or `up`, not `sideways`",
        ),
        ("key down 30 31", "unknown command `key`"),
        (
            "button up 271",
            "a button's code is from 272 to 279, not `271`",
        ),
        (
            "button up 280",
            "a button's code is from 272 to 279, not `280`",
        ),
        ("scroll 0 1000.5", "`scroll` takes two numbers"),
        ("scroll 0 0.0001", "`scroll` takes two numbers"),
        ("scroll 1 .5", "`scroll` takes two numbers"),
        ("scroll 1", "unknown command `scroll`"),
    ];
    for (command, said) in bad {
        let run = input_client_run(&[socket.as_os_str()], &format!("{command}\n"));
        if run.0 != Some(1) || !run.1.starts_with(&format!("input: {said}")) {
            failures.push(format!(
                "{command:?}: {run:?}, expected status 1 and {said:?}"
            ));
        }
    }
    report("runs of the input client", 1 + bad.len(), failures);
}

/// The commands of a `.drive` file in groups that each end with a `tick`,
/// with the lines the headless run of `exe` prints in the tick that ends
/// each group.
fn tick_groups(exe: &Path, source: &Path) -> Vec<(Vec<String>, Vec<String>)> {
    let commands = std::fs::read_to_string(source.with_extension("drive")).expect("read commands");
    let mut groups = Vec::new();
    let mut group = Vec::new();
    for line in commands.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line == "tick" {
            groups.push(std::mem::take(&mut group));
        } else {
            group.push(line.to_string());
        }
    }
    assert!(
        group.is_empty(),
        "{}: commands after the last `tick`",
        source.display()
    );
    // Standard output and error in one pipe: the runtime flushes standard
    // output before each answer, so a `tick` answer follows what its tick
    // printed.
    let mut command = Command::new("bash");
    command
        .args(["-c", "exec \"$0\" 2>&1"])
        .arg(exe)
        .env("MIDNIGHT_GPU", "off");
    headless(&mut command, source, false);
    let output = command.output().expect("run the program headless");
    assert!(output.status.success(), "{}", text(&output.stdout));
    let mut printed = Vec::new();
    let mut lines = Vec::new();
    for line in text(&output.stdout).lines() {
        match line.strip_prefix(DRIVE_SAID) {
            Some("tick") => printed.push(std::mem::take(&mut lines)),
            Some(_) => {}
            None => lines.push(line.to_string()),
        }
    }
    assert_eq!(
        printed.len(),
        groups.len(),
        "{}: a `tick` was not answered",
        source.display()
    );
    groups.into_iter().zip(printed).collect()
}

/// A line a program printed, and where.
enum Said {
    Out(String),
    Err(String),
}

/// What a running program has printed so far.
struct Printed {
    lines: std::sync::mpsc::Receiver<Said>,
    stdout: Vec<String>,
    stderr: Vec<String>,
}

impl Printed {
    /// Takes what the program prints until `done` passes, for up to
    /// `limit`. Returns whether it passed.
    fn until(&mut self, limit: Duration, done: impl Fn(&Printed) -> bool) -> bool {
        let started = Instant::now();
        while !done(self) {
            let left = limit.saturating_sub(started.elapsed());
            match self.lines.recv_timeout(left) {
                Ok(Said::Out(line)) => self.stdout.push(line),
                Ok(Said::Err(line)) => self.stderr.push(line),
                Err(_) => return false,
            }
        }
        true
    }
}

/// One run of `window_input`: the program with a window, undriven, with
/// the keyboard and the pointer of `tests/window/input.c` and the
/// compositor's cursor and workspaces. Sends each group of commands, then
/// waits for what the headless run printed in the group's tick.
fn input_from_compositor(
    exe: &Path,
    groups: &[(Vec<String>, Vec<String>)],
    device: Device,
) -> Result<(), String> {
    use std::io::BufRead;
    sway(&["seat", "-", "cursor", "set", "100", "100"]);
    let mut client = InputClient::start(&compositor_socket())?;
    let mut command = Command::new(exe);
    command
        .env("MIDNIGHT_WINDOW", "info")
        .env_remove("MIDNIGHT_DRIVE")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    on_device(&mut command, device);
    let mut child = command.spawn().expect("run program");
    let (send, lines) = std::sync::mpsc::channel();
    let out = child.stdout.take().expect("piped");
    let err = child.stderr.take().expect("piped");
    let send_err = send.clone();
    std::thread::spawn(move || {
        for line in std::io::BufReader::new(out).lines().map_while(Result::ok) {
            let _ = send.send(Said::Out(line));
        }
    });
    std::thread::spawn(move || {
        for line in std::io::BufReader::new(err).lines().map_while(Result::ok) {
            let _ = send_err.send(Said::Err(line));
        }
    });
    let mut printed = Printed {
        lines,
        stdout: Vec::new(),
        stderr: Vec::new(),
    };
    let limit = Duration::from_secs(10);
    let mut sent = || -> Result<(), String> {
        let ready = printed.until(limit, |p| {
            [
                "midnight: window: focused",
                "midnight: window: pointer inside",
            ]
            .iter()
            .all(|line| p.stderr.iter().any(|said| said == line))
        });
        if !ready {
            return Err(format!(
                "the window got no focus or no pointer: {:?}",
                printed.stderr
            ));
        }
        let mut expected: Vec<String> = Vec::new();
        for (commands, lines) in groups {
            for line in commands {
                let words: Vec<&str> = line.split_whitespace().collect();
                match words.as_slice() {
                    ["key", way @ ("down" | "up"), name] => {
                        let code = key_code(name).ok_or(format!("no code for `{name}`"))?;
                        client.send(&format!("key {way} {code}"))?;
                    }
                    ["mouse", way @ ("down" | "up"), name] => {
                        let code = button_code(name).ok_or(format!("no code for `{name}`"))?;
                        client.send(&format!("button {way} {code}"))?;
                    }
                    ["mouse", "move", x, y] => {
                        sway(&["seat", "-", "cursor", "set", x, y]);
                    }
                    ["mouse", "leave"] => {}
                    ["scroll", dx, dy] => client.send(&format!("scroll {dx} {dy}"))?,
                    ["blur"] => {
                        sway(&["workspace", "2"]);
                    }
                    ["focus"] => {
                        sway(&["workspace", "1"]);
                    }
                    _ => return Err(format!("`{line}` can't be sent from a compositor")),
                }
            }
            expected.extend(lines.iter().cloned());
            let count = expected.len();
            printed.until(limit, |p| p.stdout.len() >= count);
            if printed.stdout != expected {
                return Err(format!(
                    "after {commands:?}\n--- expected\n{}\n--- printed\n{}",
                    expected.join("\n"),
                    printed.stdout.join("\n")
                ));
            }
        }
        Ok(())
    };
    let result = sent();
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().expect("wait for program") {
            break status.code();
        }
        if started.elapsed() > limit {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    printed.until(Duration::from_secs(5), |_| false);
    let client_said = client.stop();
    result?;
    let mut stderr = printed.stderr;
    let device_line = stderr
        .iter()
        .any(|line| line.starts_with("midnight: gpu: "));
    let wayland = stderr
        .iter()
        .any(|line| line.starts_with("midnight: window: ") && line.contains(", wayland, "));
    stderr.retain(|line| {
        !line.starts_with("midnight: window: ") && !line.starts_with("midnight: gpu: ")
    });
    let total: usize = groups.iter().map(|(_, lines)| lines.len()).sum();
    if status != Some(0)
        || !stderr.is_empty()
        || !device_line
        || !wayland
        || printed.stdout.len() != total
    {
        return Err(format!(
            "exited with {status:?}, device line {device_line}, on Wayland {wayland}, input client {client_said}\n--- stdout\n{}\n--- stderr\n{}",
            printed.stdout.join("\n"),
            stderr.join("\n")
        ));
    }
    Ok(())
}

/// `tests/run/input_events.12am` with a window, undriven, at a scale of
/// 1.5, with the keyboard and the pointer of `tests/window/input.c` and the
/// compositor's cursor and workspaces sending the input of its `.drive`
/// file one group at a time: it must print what the headless run prints,
/// 20 times in each build mode.
#[test]
fn window_input() {
    let Some(device) = window_device() else {
        return;
    };
    let _compositor = COMPOSITOR
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    let source = Path::new("tests/run/input_events.12am");
    let dir = scratch("window-input");
    sway(&["output", "HEADLESS-1", "scale", "1.5"]);
    let mut failures = Vec::new();
    const RUNS: usize = 20;
    for mode in MODES {
        let exe = dir.join(format!("input-{mode:?}"));
        build(source, &exe, mode).unwrap_or_else(|message| panic!("{message}"));
        let groups = tick_groups(&exe, source);
        for run in 0..RUNS {
            if let Err(message) = input_from_compositor(&exe, &groups, device) {
                failures.push(format!(
                    "{} ({mode:?}), run {run}: {message}",
                    source.display()
                ));
            }
        }
    }
    sway(&["output", "HEADLESS-1", "scale", "1"]);
    let _ = std::fs::remove_dir_all(&dir);
    report(
        "runs with input from a compositor",
        RUNS * MODES.len(),
        failures,
    );
}
