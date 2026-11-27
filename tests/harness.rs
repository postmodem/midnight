//! The tests of the evals' harness, `src/bin/midnight-eval`: its modules
//! are compiled in here, and the wrapper is run as the built executable.
//! Fixtures are in `evals/test/`.

#![allow(dead_code)]

#[path = "../src/bin/midnight-eval/audit.rs"]
mod audit;
#[path = "../src/bin/midnight-eval/base.rs"]
mod base;
#[path = "../src/bin/midnight-eval/cli.rs"]
mod cli;
#[path = "../src/bin/midnight-eval/draws.rs"]
mod draws;
#[path = "../src/bin/midnight-eval/grade.rs"]
mod grade;
#[path = "../src/bin/midnight-eval/guard.rs"]
mod guard;
#[path = "../src/bin/midnight-eval/json.rs"]
mod json;
#[path = "../src/bin/midnight-eval/logs.rs"]
mod logs;
#[path = "../src/bin/midnight-eval/proc.rs"]
mod proc;
#[path = "../src/bin/midnight-eval/report.rs"]
mod report;
#[path = "../src/bin/midnight-eval/runner.rs"]
mod runner;
#[path = "../src/bin/midnight-eval/sha256.rs"]
mod sha256;
#[path = "../src/bin/midnight-eval/task.rs"]
mod task;
#[path = "../src/bin/midnight-eval/text.rs"]
mod text;
#[path = "../src/bin/midnight-eval/tools.rs"]
mod tools;

pub use base::*;

#[path = "harness/audit.rs"]
mod audit_tests;
#[path = "harness/basics.rs"]
mod basics_tests;
#[path = "harness/draws.rs"]
mod draws_tests;
#[path = "harness/parse.rs"]
mod parse_tests;
#[path = "harness/report.rs"]
mod report_tests;
#[path = "harness/systems.rs"]
mod systems_tests;
#[path = "harness/wrapper.rs"]
mod wrapper_tests;

/// Test helpers.
pub mod help {
    use crate::json::{self, Value};
    use crate::proc;
    use crate::text::join;

    /// The repository.
    pub fn repo() -> String {
        env!("CARGO_MANIFEST_DIR").to_string()
    }

    pub fn fixtures() -> String {
        join(&[&repo(), "evals", "test", "fixtures"])
    }

    pub fn fixture(name: &str) -> String {
        proc::read(&join(&[&fixtures(), name])).expect("a fixture")
    }

    pub fn fixture_json(name: &str) -> Value {
        json::parse(&fixture(name)).expect("JSON")
    }

    /// A fresh directory, removed when dropped.
    pub struct Temp(pub String);

    impl Default for Temp {
        fn default() -> Temp {
            Temp::new()
        }
    }

    impl Temp {
        pub fn new() -> Temp {
            Temp(proc::mktemp().expect("a directory"))
        }

        pub fn path(&self, rel: &str) -> String {
            join(&[&self.0, rel])
        }

        /// Writes a file under the directory, making its directories.
        pub fn write(&self, rel: &str, text: &str) -> String {
            let p = self.path(rel);
            proc::mkdir_p(&crate::text::dirname(&p)).expect("a directory");
            proc::write(&p, text).expect("a file");
            p
        }
    }

    impl Drop for Temp {
        fn drop(&mut self) {
            proc::rm_rf(&self.0);
        }
    }

    static GRADING: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// A turn of the tests that grade: the self-test times each grade,
    /// waiting for grading's lock included.
    pub fn grading() -> std::sync::MutexGuard<'static, ()> {
        GRADING.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The real path of `sh` on PATH, for the first line of a script.
    pub fn sh() -> String {
        crate::tools::which("sh").expect("sh on the PATH")
    }

    /// Writes an executable script.
    pub fn script(path: &str, body: &str) {
        use std::os::unix::fs::PermissionsExt;
        proc::write(path, format!("#!{}\n{body}", sh())).expect("a script");
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
            .expect("permissions");
    }
}
