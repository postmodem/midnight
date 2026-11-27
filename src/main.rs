//! The `midnight` command.

mod ast;
mod check;
mod diag;
mod driver;
mod emit;
mod json;
mod lexer;
mod loops;
mod parser;
mod plan;
mod source;
mod tir;

use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use json::{Format, Object};
use source::SourceMap;

const USAGE: &str = "\
Usage:
  midnight build FILE [-o OUT] [--debug]   compile to an executable
  midnight run FILE [--debug]              compile to a temporary directory and run
  midnight check FILE                      report errors without generating code
  midnight plan FILE                       print the CPU-GPU transfers of one tick
  midnight loops FILE                      print where each loop runs
  midnight emit-c FILE                     print the generated C
  midnight emit-wgsl FILE                  print the shader of every GPU block
  midnight --version | --help

Options of build, run, check, plan and loops:
  --format text|json   write diagnostics and output as text (the default) or
                       as JSON lines; `run --format json` also sets
                       MIDNIGHT_FORMAT=json for the program

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
    /// The format given with `--format`, if any.
    format: Option<Format>,
}

impl Args {
    fn format(&self) -> Format {
        self.format.unwrap_or(Format::Text)
    }
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
        "build" | "run" | "check" | "plan" | "loops" | "emit-c" | "emit-wgsl"
    ) {
        return Err(format!("unknown command `{command}`"));
    }
    let mut file = None;
    let mut out = None;
    let mut debug = false;
    let mut format = None;
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--debug" if matches!(command.as_str(), "build" | "run") => debug = true,
            "-o" if command == "build" => {
                let path = iter.next().ok_or("`-o` needs a path")?;
                out = Some(PathBuf::from(path));
            }
            "--format"
                if matches!(
                    command.as_str(),
                    "build" | "run" | "check" | "plan" | "loops"
                ) =>
            {
                if format.is_some() {
                    return Err("`--format` is given twice".to_string());
                }
                format = Some(match iter.next().map(String::as_str) {
                    Some("text") => Format::Text,
                    Some("json") => Format::Json,
                    Some(other) => {
                        return Err(format!("`--format` takes `text` or `json`, not `{other}`"));
                    }
                    None => return Err("`--format` needs `text` or `json`".to_string()),
                });
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
        format,
    })
}

fn run(args: &Args) -> u8 {
    let format = args.format();
    let json = format == Format::Json;
    if matches!(args.command.as_str(), "plan" | "loops") {
        if json {
            print!("{}", json::first_line());
        }
        let Some((program, sources)) = check_file(&args.file, format, json) else {
            return FAILURE;
        };
        let out = match (args.command.as_str(), format) {
            ("plan", Format::Text) => plan::render(&plan::plan(&program)),
            ("plan", Format::Json) => plan::render_json(&plan::plan(&program)),
            (_, Format::Text) => loops::render(&program, &sources),
            (_, Format::Json) => loops::render_json(&program, &sources),
        };
        print!("{out}");
        return 0;
    }
    if args.command == "emit-wgsl" {
        let Some((program, sources)) = check_file(&args.file, format, false) else {
            return FAILURE;
        };
        print!("{}", emit::emit_wgsl(&program, &sources));
        return 0;
    }
    if json {
        eprint!("{}", json::first_line());
    }
    let Some((c_source, parts)) = compile(&args.file, args.command == "check", format) else {
        return FAILURE;
    };
    let result = match args.command.as_str() {
        "check" => Ok(0),
        "emit-c" => {
            print!("{c_source}");
            Ok(0)
        }
        "build" => {
            let out = args
                .out
                .clone()
                .unwrap_or_else(|| default_output(&args.file));
            driver::build(&c_source, &out, args.debug, parts, format).map(|()| 0)
        }
        "run" => driver::run(&c_source, args.debug, parts, format, args.format),
        _ => unreachable!(),
    };
    match result {
        Ok(status) => status,
        Err(message) => {
            eprint!("{}", message.render(format));
            FAILURE
        }
    }
}

/// Runs the front end and, unless `check_only`, the emitter. Returns the
/// generated C and what the program needs of the runtime. Prints
/// diagnostics and returns `None` when the program has errors.
fn compile(path: &Path, check_only: bool, format: Format) -> Option<(String, driver::Parts)> {
    let (program, sources) = check_file(path, format, false)?;
    if check_only {
        return Some((String::new(), driver::Parts::default()));
    }
    let gpu = program.systems.iter().any(|system| system.gpu);
    let parts = driver::Parts {
        gpu,
        threads: emit::has_split_loops(&program),
        window: program.window.is_some(),
    };
    Some((emit::emit(&program, &sources), parts))
}

/// Runs the front end. Prints diagnostics in `format` and returns `None`
/// when the program has errors. With `first_line`, a JSON form's first
/// line goes before them on standard error.
fn check_file(path: &Path, format: Format, first_line: bool) -> Option<(tir::Program, SourceMap)> {
    let say = |text: String| {
        if first_line {
            eprint!("{}", json::first_line());
        }
        eprint!("{text}");
    };
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) => {
            let name = path.to_string_lossy();
            say(match format {
                Format::Text => format!("midnight: can't read `{name}`: {error}\n"),
                Format::Json => {
                    let mut line = Object::new("cant_read");
                    line.str("file", &name).str("message", &error.to_string());
                    line.line()
                }
            });
            return None;
        }
    };
    let mut sources = SourceMap::new();
    let root = sources.add(path.to_string_lossy().into_owned(), path.to_owned(), text);
    let program = source::load(&mut sources, root)
        .and_then(|program| check::check(&program, &sources))
        .map_err(|diagnostics| {
            say(match format {
                Format::Text => {
                    let color = std::io::stderr().is_terminal();
                    diag::render(&sources, &diagnostics, color)
                }
                Format::Json => diag::render_json(&sources, &diagnostics),
            });
        })
        .ok()?;
    Some((program, sources))
}

/// The root file's name without its extension, in the current directory.
fn default_output(file: &Path) -> PathBuf {
    PathBuf::from(file.file_stem().unwrap_or(file.as_os_str()))
}
