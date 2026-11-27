//! The command line.

use crate::grade;
use crate::json::{self, Value};
use crate::proc;
use crate::report;
use crate::runner::{self, Opts};
use crate::task::Task;
use crate::text::{self, join};
use crate::{Failure, root};

const USAGE: &str = "\
usage: midnight-eval run [--tasks A,B] [--models M,N] [--trials K] [--effort E]
                         [--kit FILE] [--jobs J] [--label L]
       midnight-eval run --resume RESULTS
       midnight-eval grade [--run RESULTS] TASK FILE
       midnight-eval grade --run RESULTS
       midnight-eval self-test [TASK...]
       midnight-eval report RESULTS
       midnight-eval compare A B
       midnight-eval isolation
";

/// Why the harness stops: an error, a misused command line, or a status.
enum Stop {
    Failure(Failure),
    Usage,
    Status(i32),
}

impl From<Failure> for Stop {
    fn from(f: Failure) -> Stop {
        Stop::Failure(f)
    }
}

/// Runs the command line and returns the exit status.
pub fn main(args: Vec<String>) -> i32 {
    match dispatch(args) {
        Ok(()) => 0,
        Err(Stop::Status(n)) => n,
        Err(Stop::Usage) => {
            eprintln!("{USAGE}");
            2
        }
        Err(Stop::Failure(Failure(message))) => {
            eprintln!("eval: {message}");
            1
        }
    }
}

fn read_json(path: &str) -> Result<Value, Failure> {
    json::parse(&proc::read(path)?).map_err(|e| Failure(format!("{path}: {e}")))
}

fn dispatch(mut args: Vec<String>) -> Result<(), Stop> {
    if args.is_empty() {
        return Err(Stop::Usage);
    }
    let command = args.remove(0);
    match command.as_str() {
        "run" => {
            if !runner::run(run_options(args)?)? {
                return Err(Stop::Status(3));
            }
            Ok(())
        }
        "grade" => grade_command(args),
        "self-test" => self_test(args),
        "isolation" => {
            if !args.is_empty() {
                return Err(Stop::Usage);
            }
            if !runner::isolation()? {
                return Err(Stop::Status(3));
            }
            Ok(())
        }
        "report" => {
            if args.len() != 1 {
                return Err(Stop::Usage);
            }
            print!("{}", report::render(&read_json(&args[0])?));
            Ok(())
        }
        "compare" => {
            if args.len() != 2 {
                return Err(Stop::Usage);
            }
            let a = read_json(&args[0])?;
            let b = read_json(&args[1])?;
            print!("{}", report::render_compare(&a, &b));
            Ok(())
        }
        _ => Err(Stop::Usage),
    }
}

fn run_options(args: Vec<String>) -> Result<Opts, Stop> {
    let mut opts = Opts::default();
    let mut rest = args.into_iter();
    while let Some(flag) = rest.next() {
        let value = rest.next().ok_or(Stop::Usage)?;
        let list = |v: &str| v.split(',').map(str::to_string).collect::<Vec<_>>();
        match flag.as_str() {
            "--tasks" => opts.tasks = Some(list(&value)),
            "--models" => opts.models = list(&value),
            "--trials" => opts.trials = value.parse().map_err(|_| Stop::Usage)?,
            "--effort" => opts.effort = Some(value),
            "--kit" => opts.kit = Some(value),
            "--jobs" => opts.jobs = value.parse().map_err(|_| Stop::Usage)?,
            "--label" => opts.label = Some(value),
            "--resume" => opts.resume = Some(value),
            _ => return Err(Stop::Usage),
        }
    }
    Ok(opts)
}

/// grade TASK FILE: against the working tree's task. With --run RESULTS,
/// against that run's snapshot of its tasks, with the compiler of its
/// commit; and with no TASK FILE, every kept program of the run, saying
/// where the verdict differs from the recorded one.
fn grade_command(mut args: Vec<String>) -> Result<(), Stop> {
    let mut results = None;
    if args.first().map(String::as_str) == Some("--run") {
        args.remove(0);
        if args.is_empty() {
            return Err(Stop::Usage);
        }
        results = Some(args.remove(0));
    }
    if !(args.len() == 2 || (results.is_some() && args.is_empty())) {
        return Err(Stop::Usage);
    }
    let root = root();
    let mut tasks = join(&[root, "evals", "tasks"]);
    let compiler = match &results {
        None => grade::compiler()?,
        Some(path) => {
            let r = read_json(path)?;
            let stem = text::basename(path);
            let stem = stem.strip_suffix(".json").unwrap_or(stem);
            tasks = join(&[root, "evals", "runs", stem, "commit", "evals", "tasks"]);
            if !proc::is_dir(&tasks) {
                return Err(Failure(format!(
                    "no snapshot of the run's tasks in {}",
                    tasks.strip_prefix(&format!("{root}/")).unwrap_or(&tasks)
                ))
                .into());
            }
            let compiler = match std::env::var("MIDNIGHT_BIN") {
                Ok(bin) if !bin.is_empty() => bin,
                _ => grade::compiler_of(&r.get("commit").to_text())?,
            };
            if args.is_empty() {
                return grade_run(&r, &tasks, &compiler);
            }
            compiler
        }
    };
    let dir = join(&[&tasks, &args[0]]);
    if !proc::is_file(&join(&[&dir, "meta"])) {
        return Err(Failure(format!("no task `{}`", args[0])).into());
    }
    let cwd = text::path_text(&std::env::current_dir().unwrap_or_default());
    let mut src = text::expand_path(&args[1], &cwd).unwrap_or_else(|| args[1].clone());
    if text::basename(&src) == "solution.12am" {
        src = text::dirname(&src);
    }
    let g = grade::grade(&Task::new(&dir)?, &src, &compiler, None)?;
    println!("{}", g.get("verdict").to_text());
    for (check, note) in g.get("notes").members() {
        println!("{check}: {}", text::strip(&note.to_text()));
    }
    if g.get("verdict").as_str() == Some("pass") {
        Ok(())
    } else {
        Err(Stop::Status(1))
    }
}

fn grade_run(results: &Value, tasks: &str, compiler: &str) -> Result<(), Stop> {
    let mut differ = 0;
    let graded: Vec<&Value> = results
        .get("runs")
        .items()
        .iter()
        .filter(|r| r.get("checks").truthy())
        .collect();
    for rec in &graded {
        let task = Task::new(&join(&[tasks, &rec.get("task").to_text()]))?;
        let dir = rec.get("dir").to_text();
        let g = grade::grade(&task, &join(&[root(), &dir, "program"]), compiler, None)?;
        let checks = rec.get("checks");
        let recorded =
            if rec.get("outcome").as_str() == Some("no solution") || checks.members().is_empty() {
                "no solution"
            } else if checks.members().iter().all(|(_, v)| v.truthy()) {
                "pass"
            } else {
                "fail"
            };
        if g.get("verdict").as_str() == Some(recorded) {
            continue;
        }
        differ += 1;
        println!(
            "{}: recorded {recorded}, now {} {}",
            dir.strip_prefix("evals/runs/").unwrap_or(&dir),
            g.get("verdict").to_text(),
            g.get("notes").to_text()
        );
    }
    let summary = if differ == 0 {
        "every verdict as recorded".to_string()
    } else {
        format!("{differ} differ")
    };
    println!("grade: {} programs, {summary}", graded.len());
    if differ == 0 {
        Ok(())
    } else {
        Err(Stop::Status(1))
    }
}

fn self_test(args: Vec<String>) -> Result<(), Stop> {
    let compiler = grade::compiler()?;
    let (count, problems) = grade::self_test(&join(&[root(), "evals", "tasks"]), &compiler, &args)?;
    for p in &problems {
        println!("{p}");
    }
    let summary = if problems.is_empty() {
        "all checks pass".to_string()
    } else {
        format!("{} problems", problems.len())
    };
    println!("self-test: {count} tasks, {summary}");
    if problems.is_empty() {
        Ok(())
    } else {
        Err(Stop::Status(1))
    }
}
