//! The `midnight` command.

mod ast;
mod check;
mod diag;
mod driver;
mod emit;
mod lexer;
mod parser;
mod plan;
mod source;
mod tir;

use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use source::SourceMap;

const USAGE: &str = "\
Usage:
  midnight build FILE [-o OUT] [--debug]   compile to an executable
  midnight run FILE [--debug]              compile to a temporary directory and run
  midnight check FILE                      report errors without generating code
  midnight plan FILE                       print the CPU-GPU transfers of one tick
  midnight emit-c FILE                     print the generated C
  midnight emit-wgsl FILE                  print the shader of every GPU block
  midnight --version | --help

Environment:
  MIDNIGHT_CC          the C compiler to use (default: cc)
  MIDNIGHT_CFLAGS      extra flags for the C compiler
  MIDNIGHT_GPU_FLAGS   flags that build and link the GPU runtime against
                       wgpu-native; without them GPU systems run on the CPU
";

/// Exit status for a program with errors.
const FAILURE: u8 = 1;
/// Exit status for a usage error.
const USAGE_ERROR: u8 = 2;

struct Args {
    command: String,
    file: PathBuf,
    out: Option<PathBuf>,
    debug: bool,
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--help" | "-h") => {
            print!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Some("--version" | "-V") => {
            println!("midnight {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        _ => {}
    }
    let args = match parse_args(&args) {
        Ok(args) => args,
        Err(message) => {
            eprintln!("midnight: {message}\n\n{USAGE}");
            return ExitCode::from(USAGE_ERROR);
        }
    };
    ExitCode::from(run(&args))
}

fn parse_args(args: &[String]) -> Result<Args, String> {
    let mut iter = args.iter();
    let command = iter.next().ok_or("no command given")?.clone();
    if !matches!(
        command.as_str(),
        "build" | "run" | "check" | "plan" | "emit-c" | "emit-wgsl"
    ) {
        return Err(format!("unknown command `{command}`"));
    }
    let mut file = None;
    let mut out = None;
    let mut debug = false;
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--debug" if matches!(command.as_str(), "build" | "run") => debug = true,
            "-o" if command == "build" => {
                let path = iter.next().ok_or("`-o` needs a path")?;
                out = Some(PathBuf::from(path));
            }
            flag if flag.starts_with('-') => {
                return Err(format!("unknown option `{flag}` for `{command}`"));
            }
            path if file.is_none() => file = Some(PathBuf::from(path)),
            extra => return Err(format!("unexpected argument `{extra}`")),
        }
    }
    let file = file.ok_or(format!("`{command}` needs a file"))?;
    Ok(Args {
        command,
        file,
        out,
        debug,
    })
}

fn run(args: &Args) -> u8 {
    if args.command == "plan" {
        let Some((program, _)) = check_file(&args.file) else {
            return FAILURE;
        };
        print!("{}", plan::render(&plan::plan(&program)));
        return 0;
    }
    if args.command == "emit-wgsl" {
        let Some((program, sources)) = check_file(&args.file) else {
            return FAILURE;
        };
        print!("{}", emit::emit_wgsl(&program, &sources));
        return 0;
    }
    let Some((c_source, gpu)) = compile(&args.file, args.command == "check") else {
        return FAILURE;
    };
    match args.command.as_str() {
        "check" => 0,
        "emit-c" => {
            print!("{c_source}");
            0
        }
        "build" => {
            let out = args
                .out
                .clone()
                .unwrap_or_else(|| default_output(&args.file));
            match driver::build(&c_source, &out, args.debug, gpu) {
                Ok(()) => 0,
                Err(message) => {
                    eprintln!("{message}");
                    FAILURE
                }
            }
        }
        "run" => match driver::run(&c_source, args.debug, gpu) {
            Ok(status) => status,
            Err(message) => {
                eprintln!("{message}");
                FAILURE
            }
        },
        _ => unreachable!(),
    }
}

/// Runs the front end and, unless `check_only`, the emitter. Returns the
/// generated C and whether the program declares a GPU system. Prints
/// diagnostics and returns `None` when the program has errors.
fn compile(path: &Path, check_only: bool) -> Option<(String, bool)> {
    let (program, sources) = check_file(path)?;
    if check_only {
        return Some((String::new(), false));
    }
    let gpu = program.systems.iter().any(|system| system.gpu);
    Some((emit::emit(&program, &sources), gpu))
}

/// Runs the front end. Prints diagnostics and returns `None` when the
/// program has errors.
fn check_file(path: &Path) -> Option<(tir::Program, SourceMap)> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("midnight: can't read `{}`: {error}", path.display());
            return None;
        }
    };
    let mut sources = SourceMap::new();
    let root = sources.add(path.to_string_lossy().into_owned(), path.to_owned(), text);
    let program = source::load(&mut sources, root)
        .and_then(|program| check::check(&program, &sources))
        .map_err(|diagnostics| {
            let color = std::io::stderr().is_terminal();
            eprint!("{}", diag::render(&sources, &diagnostics, color));
        })
        .ok()?;
    Some((program, sources))
}

/// The root file's name without its extension, in the current directory.
fn default_output(file: &Path) -> PathBuf {
    PathBuf::from(file.file_stem().unwrap_or(file.as_os_str()))
}
