//! Turns generated C into an executable by invoking the C compiler.

use std::path::{Path, PathBuf};
use std::process::Command;

const RUNTIME_H: &str = include_str!("../runtime/midnight_rt.h");
const RUNTIME_C: &str = include_str!("../runtime/midnight_rt.c");
const GPU_H: &str = include_str!("../runtime/midnight_gpu.h");
const GPU_C: &str = include_str!("../runtime/midnight_gpu.c");
const POOL_H: &str = include_str!("../runtime/midnight_pool.h");
const POOL_C: &str = include_str!("../runtime/midnight_pool.c");

/// What a program needs of the runtime besides `midnight_rt.c`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Parts {
    /// The program declares a GPU system.
    pub gpu: bool,
    /// The program has a loop that can be split across threads: a GPU
    /// block or an independent `each`.
    pub threads: bool,
}

const RELEASE_FLAGS: &[&str] = &["-O2", "-fwrapv", "-ffp-contract=off"];
const DEBUG_FLAGS: &[&str] = &[
    "-O0",
    "-g",
    "-fwrapv",
    "-ffp-contract=off",
    "-fsanitize=address,undefined",
    "-DMN_DEBUG",
];

/// Flags for both modes: the C compiler may not compute the inexact `libm`
/// functions itself, so a release build gives what a debug build gives.
const MATH_FLAGS: &[&str] = &[
    "-fno-builtin-pow",
    "-fno-builtin-powf",
    "-fno-builtin-sin",
    "-fno-builtin-sinf",
    "-fno-builtin-cos",
    "-fno-builtin-cosf",
    "-fno-builtin-tan",
    "-fno-builtin-tanf",
    "-fno-builtin-atan2",
    "-fno-builtin-atan2f",
    "-fno-builtin-exp",
    "-fno-builtin-expf",
    "-fno-builtin-log",
    "-fno-builtin-logf",
];

/// Flags for the machine `midnight` runs on, in both modes.
#[cfg(target_arch = "x86_64")]
const TARGET_FLAGS: &[&str] = &["-march=x86-64-v3"];
#[cfg(not(target_arch = "x86_64"))]
const TARGET_FLAGS: &[&str] = &[];

/// Compiles `c_source` and the runtime into the executable `out`. The
/// error is a message ready to print.
pub fn build(c_source: &str, out: &Path, debug: bool, parts: Parts) -> Result<(), String> {
    let dir = make_build_dir()?;
    let result = compile_in(&dir, c_source, out, debug, parts);
    if result.is_ok() {
        let _ = std::fs::remove_dir_all(&dir);
    }
    result
}

/// Compiles into a temporary directory, runs the program with this
/// process's standard streams, and returns its exit status.
pub fn run(c_source: &str, debug: bool, parts: Parts) -> Result<u8, String> {
    let dir = make_build_dir()?;
    let exe = dir.join("program");
    compile_in(&dir, c_source, &exe, debug, parts)?;
    let status = Command::new(&exe).status();
    let _ = std::fs::remove_dir_all(&dir);
    let status = status.map_err(|error| format!("midnight: can't run the program: {error}"))?;
    Ok(exit_code(status))
}

/// The flags that build and link `midnight_gpu.c`, when the program has a
/// GPU system and `MIDNIGHT_GPU_FLAGS` gives them. Says so on standard
/// error when the program has one and the variable does not.
fn gpu_flags(gpu: bool) -> Option<String> {
    if !gpu {
        return None;
    }
    let flags = std::env::var("MIDNIGHT_GPU_FLAGS").unwrap_or_default();
    if flags.trim().is_empty() {
        eprintln!("midnight: MIDNIGHT_GPU_FLAGS is not set; GPU systems will run on the CPU");
        return None;
    }
    Some(flags)
}

fn compile_in(
    dir: &Path,
    c_source: &str,
    out: &Path,
    debug: bool,
    parts: Parts,
) -> Result<(), String> {
    let Parts { gpu, threads } = parts;
    let write = |name: &str, text: &str| {
        std::fs::write(dir.join(name), text).map_err(|error| {
            format!(
                "midnight: can't write `{}`: {error}",
                dir.join(name).display()
            )
        })
    };
    write("program.c", c_source)?;
    write("midnight_rt.h", RUNTIME_H)?;
    write("midnight_rt.c", RUNTIME_C)?;
    let gpu_flags = gpu_flags(gpu);
    if gpu_flags.is_some() {
        write("midnight_gpu.h", GPU_H)?;
        write("midnight_gpu.c", GPU_C)?;
    }
    if threads {
        write("midnight_pool.h", POOL_H)?;
        write("midnight_pool.c", POOL_C)?;
    }

    let cc = std::env::var("MIDNIGHT_CC").unwrap_or_else(|_| "cc".to_string());
    let mut command = Command::new(&cc);
    command.arg("-std=c11");
    command.args(if debug { DEBUG_FLAGS } else { RELEASE_FLAGS });
    command.args(TARGET_FLAGS);
    command.args(MATH_FLAGS);
    if let Ok(extra) = std::env::var("MIDNIGHT_CFLAGS") {
        command.args(extra.split_whitespace());
    }
    // The runtime reads MIDNIGHT_GPU when the program has a GPU system, and
    // uses a device when midnight_gpu.c is built with it.
    if gpu {
        command.arg("-DMN_GPU_SYSTEMS");
    }
    if gpu_flags.is_some() {
        command.arg("-DMN_GPU");
    }
    // The runtime reads MIDNIGHT_THREADS and splits loops when the pool is
    // built with it.
    if threads {
        command.args(["-DMN_THREADS", "-pthread"]);
    }
    command
        .arg("-o")
        .arg(out)
        .arg(dir.join("program.c"))
        .arg(dir.join("midnight_rt.c"));
    if gpu_flags.is_some() {
        command.arg(dir.join("midnight_gpu.c"));
    }
    if threads {
        command.arg(dir.join("midnight_pool.c"));
    }
    command.arg("-lm");
    // Libraries come after the files that use them.
    if let Some(flags) = &gpu_flags {
        command.args(flags.split_whitespace());
    }

    let output = command.output().map_err(|error| {
        format!(
            "midnight: can't run the C compiler `{cc}`: {error}\n\
             Set MIDNIGHT_CC to a C11 compiler."
        )
    })?;
    if output.status.success() {
        return Ok(());
    }
    Err(format!(
        "midnight: the C compiler rejected the generated code. This is a bug in midnight.\n\
         The generated C was kept at {}\n\n{}",
        dir.join("program.c").display(),
        String::from_utf8_lossy(&output.stderr).trim_end()
    ))
}

fn make_build_dir() -> Result<PathBuf, String> {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.subsec_nanos());
    let base = std::env::temp_dir();
    for attempt in 0..100u32 {
        let dir = base.join(format!(
            "midnight-{}-{:08x}",
            std::process::id(),
            nanos.wrapping_add(attempt)
        ));
        match std::fs::create_dir(&dir) {
            Ok(()) => return Ok(dir),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(format!(
                    "midnight: can't create a build directory in `{}`: {error}",
                    base.display()
                ));
            }
        }
    }
    Err(format!(
        "midnight: can't create a build directory in `{}`",
        base.display()
    ))
}

/// The status to pass on: the program's own, or 128 plus the signal that
/// ended it.
fn exit_code(status: std::process::ExitStatus) -> u8 {
    use std::os::unix::process::ExitStatusExt;
    match (status.code(), status.signal()) {
        (Some(code), _) => code as u8,
        (None, Some(signal)) => 128u8.wrapping_add(signal as u8),
        (None, None) => 1,
    }
}
