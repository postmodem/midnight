//! Turns generated C into an executable by invoking the C compiler.

use std::path::{Path, PathBuf};
use std::process::Command;

const RUNTIME_H: &str = include_str!("../runtime/midnight_rt.h");
const RUNTIME_C: &str = include_str!("../runtime/midnight_rt.c");

const RELEASE_FLAGS: &[&str] = &["-O2", "-fwrapv"];
const DEBUG_FLAGS: &[&str] = &[
    "-O0",
    "-g",
    "-fwrapv",
    "-fsanitize=address,undefined",
    "-DMN_DEBUG",
];

/// Compiles `c_source` and the runtime into the executable `out`.
/// The error is a message ready to print.
pub fn build(c_source: &str, out: &Path, debug: bool) -> Result<(), String> {
    let dir = make_build_dir()?;
    let result = compile_in(&dir, c_source, out, debug);
    if result.is_ok() {
        let _ = std::fs::remove_dir_all(&dir);
    }
    result
}

/// Compiles into a temporary directory, runs the program with this
/// process's standard streams, and returns its exit status.
pub fn run(c_source: &str, debug: bool) -> Result<u8, String> {
    let dir = make_build_dir()?;
    let exe = dir.join("program");
    compile_in(&dir, c_source, &exe, debug)?;
    let status = Command::new(&exe).status();
    let _ = std::fs::remove_dir_all(&dir);
    let status = status.map_err(|error| format!("midnight: can't run the program: {error}"))?;
    Ok(exit_code(status))
}

fn compile_in(dir: &Path, c_source: &str, out: &Path, debug: bool) -> Result<(), String> {
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

    let cc = std::env::var("MIDNIGHT_CC").unwrap_or_else(|_| "cc".to_string());
    let mut command = Command::new(&cc);
    command.arg("-std=c11");
    command.args(if debug { DEBUG_FLAGS } else { RELEASE_FLAGS });
    if let Ok(extra) = std::env::var("MIDNIGHT_CFLAGS") {
        command.args(extra.split_whitespace());
    }
    command
        .arg("-o")
        .arg(out)
        .arg(dir.join("program.c"))
        .arg(dir.join("midnight_rt.c"))
        .arg("-lm");

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
