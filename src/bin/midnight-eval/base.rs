//! What every part of the harness shares: its error, its limits, and the
//! repository it works in.

use std::sync::OnceLock;

/// An error that stops the harness with `eval: MESSAGE` and status 1.
#[derive(Debug, Clone, PartialEq)]
pub struct Failure(pub String);

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// The tools an agent has.
pub const TOOLS: [&str; 6] = ["Bash", "Read", "Edit", "Write", "Glob", "Grep"];
/// What an agent is told.
pub const PROMPT: &str = "Read TASK.md and do the task.";
/// Seconds a compiler call, a build or a program may take.
pub const LIMIT: u64 = 30;
/// Bytes of each stream a call may write.
pub const CAP: usize = 1 << 20;
/// Seconds the self-test allows for grading one program.
pub const BUILD_SECONDS: f64 = 5.0;

static ROOT: OnceLock<String> = OnceLock::new();

/// The repository: the working directory the harness was started in, as
/// a real path.
pub fn root() -> &'static str {
    ROOT.get_or_init(|| {
        let cwd = std::env::current_dir().unwrap_or_default();
        std::fs::canonicalize(&cwd)
            .unwrap_or(cwd)
            .to_string_lossy()
            .into_owned()
    })
}
