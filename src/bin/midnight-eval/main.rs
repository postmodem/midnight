//! The evals' harness: runs agents on the tasks under evals/tasks, grades
//! their programs, and writes results and reports. Run under the name
//! `midnight`, it is the agent's compiler: the guard in front of the real
//! one (see `guard`).
//!
//!   midnight-eval run [--tasks A,B] [--models M,N] [--trials K]
//!                     [--effort E] [--kit FILE] [--jobs J] [--label L]
//!   midnight-eval run --resume RESULTS
//!   midnight-eval grade [--run RESULTS] TASK FILE
//!   midnight-eval grade --run RESULTS
//!   midnight-eval self-test [TASK...]
//!   midnight-eval report RESULTS
//!   midnight-eval compare A B
//!   midnight-eval isolation

mod audit;
mod base;
mod cli;
mod draws;
mod grade;
mod guard;
mod json;
mod logs;
mod proc;
mod report;
mod runner;
mod sha256;
mod task;
mod text;
mod tools;

pub use base::*;

fn main() {
    let args: Vec<String> = std::env::args_os()
        .skip(1)
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    let exe = std::env::current_exe().unwrap_or_default();
    let status = if exe.file_name().is_some_and(|n| n == "midnight") {
        guard::run_as_wrapper(&exe, &args)
    } else {
        cli::main(args)
    };
    std::process::exit(status);
}
