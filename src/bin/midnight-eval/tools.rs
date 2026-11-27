//! Where the tools the harness hands on come from: the flake app sets
//! these; elsewhere the caller's PATH is searched.

use crate::Failure;
use crate::proc;
use crate::text;

/// The real path of an executable file name in PATH, or None.
pub fn which(name: &str) -> Option<String> {
    let path = std::env::var("PATH").unwrap_or_default();
    for dir in path.split(':') {
        let candidate = text::join(&[dir, name]);
        if proc::is_executable(&candidate) {
            return proc::realpath(&candidate).ok();
        }
    }
    None
}

pub fn tool_dir(name: &str) -> Result<String, Failure> {
    let path = which(name).ok_or_else(|| Failure(format!("`{name}` is not on the PATH")))?;
    Ok(text::dirname(&path))
}

fn set(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

/// The directories the agent's PATH ends with.
pub fn agent_tools() -> Result<String, Failure> {
    if let Some(t) = set("EVAL_AGENT_TOOLS") {
        return Ok(t);
    }
    let mut dirs: Vec<String> = Vec::new();
    for t in ["ls", "bash", "find", "grep", "sed", "rg"] {
        let d = tool_dir(t)?;
        if !dirs.contains(&d) {
            dirs.push(d);
        }
    }
    Ok(dirs.join(":"))
}

/// The compiler's PATH inside the wrapper, where `kill` and `date` are too.
pub fn coreutils() -> Result<String, Failure> {
    match set("EVAL_COREUTILS") {
        Some(c) => Ok(c),
        None => tool_dir("ls"),
    }
}
