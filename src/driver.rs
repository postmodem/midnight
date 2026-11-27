//! Turns generated C into an executable by invoking the C compiler.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::json::{self, Format, Object};

const RUNTIME_H: &str = include_str!("../runtime/midnight_rt.h");
const RUNTIME_C: &str = include_str!("../runtime/midnight_rt.c");
const GPU_H: &str = include_str!("../runtime/midnight_gpu.h");
const GPU_C: &str = include_str!("../runtime/midnight_gpu.c");
const WINDOW_H: &str = include_str!("../runtime/midnight_window.h");
const WINDOW_C: &str = include_str!("../runtime/midnight_window.c");
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
    /// The program has a window.
    pub window: bool,
}

/// A message of the compiler's own, kept in parts so that it can be
/// written as text or as JSON.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Message {
    /// A file of the build could not be written.
    CantWrite { file: String, error: String },
    /// No build directory could be made: `error` is the system's reason,
    /// `None` when every name tried was taken.
    BuildDirectory {
        directory: String,
        error: Option<String>,
    },
    /// The C compiler did not start (`status` is `None`, `error` says why)
    /// or failed (`kept` is the generated C, `output` what it printed).
    CCompiler {
        compiler: String,
        status: Option<i32>,
        output: String,
        kept: Option<String>,
        error: Option<String>,
    },
    /// The built program could not be started.
    CantRun { error: String },
    /// A program is built without a part because flags variables are not
    /// set. `window` when the part is the window's, else GPU systems'.
    NoFlags {
        variables: Vec<&'static str>,
        window: bool,
    },
}

impl Message {
    /// The message as text, ending in a newline.
    pub fn text(&self) -> String {
        let mut text = match self {
            Message::CantWrite { file, error } => {
                format!("midnight: can't write `{file}`: {error}")
            }
            Message::BuildDirectory { directory, error } => match error {
                Some(error) => {
                    format!("midnight: can't create a build directory in `{directory}`: {error}")
                }
                None => format!("midnight: can't create a build directory in `{directory}`"),
            },
            Message::CCompiler {
                compiler,
                error: Some(error),
                ..
            } => format!(
                "midnight: can't run the C compiler `{compiler}`: {error}\n\
                 Set MIDNIGHT_CC to a C11 compiler."
            ),
            Message::CCompiler { output, kept, .. } => format!(
                "midnight: the C compiler rejected the generated code. This is a bug in midnight.\n\
                 The generated C was kept at {}\n\n{output}",
                kept.as_deref().unwrap_or("")
            ),
            Message::CantRun { error } => format!("midnight: can't run the program: {error}"),
            Message::NoFlags { .. } => format!("midnight: {}", self.no_flags_line()),
        };
        text.push('\n');
        text
    }

    /// The line of a [`Message::NoFlags`], without `midnight: `.
    fn no_flags_line(&self) -> String {
        let Message::NoFlags { variables, window } = self else {
            unreachable!("a message about flags")
        };
        let missing = match variables.as_slice() {
            [one] => format!("{one} is"),
            many => format!("{} are", many.join(" and ")),
        };
        if *window {
            format!("{missing} not set; the window will run only headless, and capture nothing")
        } else {
            format!("{missing} not set; GPU systems will run on the CPU")
        }
    }

    /// The message as a JSON line.
    pub fn json(&self) -> String {
        let line = match self {
            Message::CantWrite { file, error } => {
                let mut line = Object::new("cant_write");
                line.str("file", file).str("message", error);
                line
            }
            Message::BuildDirectory { directory, error } => {
                let mut line = Object::new("build_directory");
                line.str("directory", directory);
                if let Some(error) = error {
                    line.str("message", error);
                }
                line
            }
            Message::CCompiler {
                compiler,
                status,
                output,
                kept,
                error,
            } => {
                let mut line = Object::new("c_compiler");
                line.str("compiler", compiler);
                if let Some(status) = status {
                    line.int("status", *status as i64);
                }
                line.str("output", output);
                if let Some(kept) = kept {
                    line.str("kept", kept);
                }
                if let Some(error) = error {
                    line.str("message", error);
                }
                line
            }
            Message::CantRun { error } => {
                let mut line = Object::new("cant_run");
                line.str("message", error);
                line
            }
            Message::NoFlags { variables, window } => {
                let mut line = Object::new("no_flags");
                line.raw(
                    "variables",
                    &json::array(variables.iter().map(|name| json::quoted(name))),
                )
                .str("affects", if *window { "window" } else { "gpu_systems" })
                .str("message", &self.no_flags_line());
                line
            }
        };
        line.line()
    }

    /// The message in a format.
    pub fn render(&self, format: Format) -> String {
        match format {
            Format::Text => self.text(),
            Format::Json => self.json(),
        }
    }
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

/// Compiles `c_source` and the runtime into the executable `out`. Says on
/// standard error, in `format`, what runs without flags.
pub fn build(
    c_source: &str,
    out: &Path,
    debug: bool,
    parts: Parts,
    format: Format,
) -> Result<(), Message> {
    let dir = make_build_dir()?;
    let result = compile_in(&dir, c_source, out, debug, parts, format);
    if result.is_ok() {
        let _ = std::fs::remove_dir_all(&dir);
    }
    result
}

/// Compiles into a temporary directory, runs the program with this
/// process's standard streams, and returns its exit status. The program
/// runs without `LD_LIBRARY_PATH`: it finds its libraries through its
/// runpaths. With `program_format`, it runs with `MIDNIGHT_FORMAT=json`,
/// or without the variable for text.
pub fn run(
    c_source: &str,
    debug: bool,
    parts: Parts,
    format: Format,
    program_format: Option<Format>,
) -> Result<u8, Message> {
    let dir = make_build_dir()?;
    let exe = dir.join("program");
    compile_in(&dir, c_source, &exe, debug, parts, format)?;
    let mut command = Command::new(&exe);
    command.env_remove("LD_LIBRARY_PATH");
    match program_format {
        Some(Format::Json) => {
            command.env("MIDNIGHT_FORMAT", "json");
        }
        Some(Format::Text) => {
            command.env_remove("MIDNIGHT_FORMAT");
        }
        None => {}
    }
    let status = command.status();
    let _ = std::fs::remove_dir_all(&dir);
    let status = status.map_err(|error| Message::CantRun {
        error: error.to_string(),
    })?;
    Ok(exit_code(status))
}

/// The value of a flags variable, or `None` when it is unset or blank.
fn flags_variable(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .filter(|flags| !flags.trim().is_empty())
}

/// The flags that build and link `midnight_gpu.c` and, for a window,
/// `midnight_window.c`. The device part is built for a GPU system when
/// `MIDNIGHT_GPU_FLAGS` is set, and for a window when that and
/// `MIDNIGHT_WINDOW_FLAGS` both are; the window part for a window when
/// both are. Says on standard error what runs without them.
fn part_flags(gpu: bool, window: bool, format: Format) -> (Option<String>, Option<String>) {
    let gpu_flags = flags_variable("MIDNIGHT_GPU_FLAGS");
    let window_flags = flags_variable("MIDNIGHT_WINDOW_FLAGS");
    if gpu && gpu_flags.is_none() {
        let message = Message::NoFlags {
            variables: vec!["MIDNIGHT_GPU_FLAGS"],
            window: false,
        };
        eprint!("{}", message.render(format));
    }
    if window {
        let mut variables = Vec::new();
        if gpu_flags.is_none() {
            variables.push("MIDNIGHT_GPU_FLAGS");
        }
        if window_flags.is_none() {
            variables.push("MIDNIGHT_WINDOW_FLAGS");
        }
        if !variables.is_empty() {
            let message = Message::NoFlags {
                variables,
                window: true,
            };
            eprint!("{}", message.render(format));
        }
    }
    let frames = window && gpu_flags.is_some() && window_flags.is_some();
    let device = (gpu || frames).then_some(gpu_flags).flatten();
    (device, window_flags.filter(|_| frames))
}

fn compile_in(
    dir: &Path,
    c_source: &str,
    out: &Path,
    debug: bool,
    parts: Parts,
    format: Format,
) -> Result<(), Message> {
    let Parts {
        gpu,
        threads,
        window,
    } = parts;
    let write = |name: &str, text: &str| {
        std::fs::write(dir.join(name), text).map_err(|error| Message::CantWrite {
            file: dir.join(name).display().to_string(),
            error: error.to_string(),
        })
    };
    write("program.c", c_source)?;
    write("midnight_rt.h", RUNTIME_H)?;
    write("midnight_rt.c", RUNTIME_C)?;
    let (gpu_flags, window_flags) = part_flags(gpu, window, format);
    if gpu_flags.is_some() {
        write("midnight_gpu.h", GPU_H)?;
        write("midnight_gpu.c", GPU_C)?;
    }
    if window_flags.is_some() {
        write("midnight_window.h", WINDOW_H)?;
        write("midnight_window.c", WINDOW_C)?;
    }
    if threads {
        write("midnight_pool.h", POOL_H)?;
        write("midnight_pool.c", POOL_C)?;
    }

    let cc = std::env::var("MIDNIGHT_CC").unwrap_or_else(|_| "cc".to_string());
    let mut command = Command::new(&cc);
    command.env_remove("LD_LIBRARY_PATH");
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
    // The runtime keeps the window's state and reads MIDNIGHT_WINDOW,
    // MIDNIGHT_DRIVE and MIDNIGHT_GPU when the program has a window, and
    // shows it and draws its frames when midnight_window.c is built with it.
    if window {
        command.arg("-DMN_WINDOW");
    }
    if window_flags.is_some() {
        command.arg("-DMN_FRAMES");
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
    if window_flags.is_some() {
        command.arg(dir.join("midnight_window.c"));
    }
    if threads {
        command.arg(dir.join("midnight_pool.c"));
    }
    command.arg("-lm");
    // Libraries come after the files that use them.
    for flags in [&gpu_flags, &window_flags].into_iter().flatten() {
        command.args(flags.split_whitespace());
    }

    let output = command.output().map_err(|error| Message::CCompiler {
        compiler: cc.clone(),
        status: None,
        output: String::new(),
        kept: None,
        error: Some(error.to_string()),
    })?;
    if output.status.success() {
        return Ok(());
    }
    Err(Message::CCompiler {
        compiler: cc,
        status: Some(i32::from(exit_code(output.status))),
        output: String::from_utf8_lossy(&output.stderr)
            .trim_end()
            .to_string(),
        kept: Some(dir.join("program.c").display().to_string()),
        error: None,
    })
}

fn make_build_dir() -> Result<PathBuf, Message> {
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
                return Err(Message::BuildDirectory {
                    directory: base.display().to_string(),
                    error: Some(error.to_string()),
                });
            }
        }
    }
    Err(Message::BuildDirectory {
        directory: base.display().to_string(),
        error: None,
    })
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
