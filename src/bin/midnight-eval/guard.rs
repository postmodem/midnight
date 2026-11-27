//! The agent's `midnight`: the guards it applies to a call, the fixed
//! environment it gives the real compiler, and its call log. The harness
//! copies its own executable into a directory on the agent's PATH under
//! the name `midnight`, with `midnight.json` beside it; run under that
//! name, the executable reads that file and acts as `main` here.

use std::fs;
use std::io::Write;
use std::time::SystemTime;

use crate::Failure;
use crate::json::{self, Map, Value};
use crate::obj;
use crate::proc;
use crate::sha256::Sha256;
use crate::text::{self, inside};

/// The name of the configuration file beside the agent's `midnight`.
pub const CONFIG: &str = "midnight.json";

/// What the wrapper is told by the harness that writes it.
pub struct Config {
    /// The run's directory, as a real path.
    pub root: String,
    /// The call log, one JSON record a line.
    pub log: String,
    /// Where the programs of check, build and run calls are kept.
    pub programs: String,
    /// The real compiler.
    pub compiler: String,
    /// The compiler's PATH: the coreutils directory, where `kill` is too.
    pub path: String,
    pub home: String,
    pub xdg: String,
    pub tmp: String,
    /// Seconds a call may take.
    pub limit: u64,
    /// Bytes of each stream a call may write.
    pub cap: usize,
}

impl Config {
    pub fn to_json(&self) -> Map {
        obj! {
            "root" => self.root.as_str(),
            "log" => self.log.as_str(),
            "programs" => self.programs.as_str(),
            "compiler" => self.compiler.as_str(),
            "path" => self.path.as_str(),
            "home" => self.home.as_str(),
            "xdg" => self.xdg.as_str(),
            "tmp" => self.tmp.as_str(),
            "limit" => self.limit,
            "cap" => self.cap,
        }
    }

    pub fn from_json(v: &Value) -> Result<Config, Failure> {
        let s = |k: &str| {
            v.get(k)
                .as_str()
                .map(str::to_string)
                .ok_or_else(|| Failure(format!("{CONFIG}: no `{k}`")))
        };
        let n = |k: &str| {
            v.get(k)
                .as_i64()
                .ok_or_else(|| Failure(format!("{CONFIG}: no `{k}`")))
        };
        Ok(Config {
            root: s("root")?,
            log: s("log")?,
            programs: s("programs")?,
            compiler: s("compiler")?,
            path: s("path")?,
            home: s("home")?,
            xdg: s("xdg")?,
            tmp: s("tmp")?,
            limit: n("limit")? as u64,
            cap: n("cap")? as usize,
        })
    }
}

pub type Env = Vec<(String, String)>;

fn lookup<'a>(env: &'a Env, name: &str) -> Option<&'a str> {
    env.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
}

/// The absolute path path names from dir, taken a part at a time as the
/// kernel does: a symbolic link is followed where it stands, so a `..`
/// after it leaves the link's target. Past the first part that does not
/// exist, the rest is joined as written. A leading `~` is a name.
pub fn resolve(path: &str, dir: &str) -> String {
    resolve_at(path, dir, 0)
}

fn resolve_at(path: &str, dir: &str, depth: usize) -> String {
    let full = if path.starts_with('/') {
        path.to_string()
    } else {
        format!("{dir}/{path}")
    };
    let parts: Vec<&str> = full.split('/').filter(|p| !p.is_empty()).collect();
    let mut cur = "/".to_string();
    for (i, part) in parts.iter().enumerate() {
        if *part == "." {
            continue;
        }
        if *part == ".." {
            cur = text::dirname(&cur);
            continue;
        }
        let probe = text::join(&[&cur, part]);
        if proc::is_link(&probe)
            && !proc::exists(&probe)
            && depth < 40
            && let Ok(target) = fs::read_link(&probe)
        {
            let mut rest = vec![text::path_text(&target)];
            rest.extend(parts[i + 1..].iter().map(|p| p.to_string()));
            return resolve_at(&rest.join("/"), &cur, depth + 1);
        }
        if !proc::exists(&probe) {
            let mut all = vec![cur.as_str()];
            all.extend(parts[i..].iter());
            return text::join(&all);
        }
        cur = proc::realpath(&probe).unwrap_or(probe);
    }
    cur
}

/// What follows `require` at the start of a line, its spaces skipped; None
/// for a line that is no require.
pub fn require_rest(line: &str) -> Option<&str> {
    let t = line.trim_start_matches(text::is_space);
    let rest = t.strip_prefix("require")?;
    if rest
        .chars()
        .next()
        .is_some_and(|c| text::is_word(c) || c == '?' || c == '!')
    {
        return None;
    }
    Some(rest.trim_start_matches(text::is_space))
}

/// The path of the string a require line goes on with, or None unless it
/// is one plain string in either quote, with only a comment after it.
pub fn require_path(rest: &str) -> Option<String> {
    let c: Vec<char> = rest.chars().collect();
    let quote = *c.first()?;
    if quote != '\'' && quote != '"' {
        return None;
    }
    let mut out = String::new();
    let mut i = 1;
    while i < c.len() {
        let ch = c[i];
        let next = c.get(i + 1).copied();
        if ch == quote {
            let tail: String = c[i + 1..].iter().collect();
            let tail = text::strip(&tail);
            return if tail.is_empty() || tail.starts_with('#') {
                Some(out)
            } else {
                None
            };
        } else if ch == '\n' || (quote == '"' && ch == '#' && next == Some('{')) {
            return None;
        } else if ch == '\\' && quote == '\'' {
            if matches!(next, Some('\'' | '\\')) {
                out.push(next.unwrap_or('\\'));
                i += 1;
            } else {
                out.push(ch);
            }
        } else if ch == '\\' {
            match next {
                Some(n @ ('\\' | '"' | '#')) => {
                    out.push(n);
                    i += 1;
                }
                _ => return None,
            }
        } else {
            out.push(ch);
        }
        i += 1;
    }
    None
}

/// What is wrong with the requires of a source file: each path that leads
/// outside root, and each require this guard cannot read.
pub fn bad_requires(file: &str, root: &str) -> Vec<String> {
    let Ok(bytes) = fs::read(file) else {
        return Vec::new();
    };
    let source = text::scrub(&bytes);
    let dir = text::dirname(file);
    text::lines(&source)
        .into_iter()
        .filter_map(|line| {
            let rest = require_rest(line)?;
            match require_path(rest) {
                None => Some(format!(
                    "`{}`, which this guard cannot read",
                    text::strip(rest)
                )),
                Some(path) => {
                    if inside(&resolve(&format!("{path}.12am"), &dir), root) {
                        None
                    } else {
                        Some(format!("`{path}`, outside this directory"))
                    }
                }
            }
        })
        .collect()
}

/// The first of the caller's variables that changes how the dynamic loader
/// starts a program, or None. LD_LIBRARY_PATH may name directories of the
/// Nix store, as Claude Code's launcher sets it.
pub fn bad_variable(env: &Env) -> Option<String> {
    let mut names: Vec<&str> = env.iter().map(|(k, _)| k.as_str()).collect();
    names.sort();
    names
        .into_iter()
        .find(|name| {
            if *name == "LD_LIBRARY_PATH" {
                lookup(env, name)
                    .unwrap_or("")
                    .split(':')
                    .any(|d| !d.starts_with("/nix/store/"))
            } else {
                name.starts_with("LD_") || name.starts_with("GCONV")
            }
        })
        .map(str::to_string)
}

/// The file arguments of a compiler call: the files, and what `-o` names.
pub fn file_args(argv: &[String]) -> (Vec<String>, Option<String>) {
    let mut files = Vec::new();
    let mut out = None;
    let mut rest = argv.iter().skip(1);
    while let Some(arg) = rest.next() {
        if arg == "-o" {
            out = rest.next().cloned();
        } else if arg == "--format" {
            rest.next();
        } else if !arg.starts_with('-') {
            files.push(arg.clone());
        }
    }
    (files, out)
}

/// Why a call must be refused, or None. root is the run's directory.
pub fn refusal(argv: &[String], caller: &Env, cwd: &str, root: &str) -> Option<String> {
    if let Some(variable) = bad_variable(caller) {
        return Some(format!("the variable `{variable}` is not allowed"));
    }
    let (files, out) = file_args(argv);
    let mut named: Vec<(&str, String)> = files.iter().map(|f| ("file", f.clone())).collect();
    if let Some(out) = &out {
        named.push(("-o", out.clone()));
    }
    if let Some(drive) = lookup(caller, "MIDNIGHT_DRIVE") {
        named.push(("MIDNIGHT_DRIVE", drive.to_string()));
    }
    for (what, path) in &named {
        if !inside(&resolve(path, cwd), root) {
            return Some(format!("{what} `{path}` is outside this directory"));
        }
    }
    let mut sources: Vec<String> = proc::glob_suffix(root, ".12am")
        .iter()
        .map(|f| text::join(&[root, f]))
        .collect();
    sources.extend(
        files
            .iter()
            .map(|f| resolve(f, cwd))
            .filter(|f| proc::is_file(f)),
    );
    let mut seen = Vec::new();
    for file in sources {
        if seen.contains(&file) {
            continue;
        }
        if let Some(bad) = bad_requires(&file, root).into_iter().next() {
            return Some(format!("`{}` requires {bad}", text::basename(&file)));
        }
        seen.push(file);
    }
    None
}

/// The compiler's environment, and the caller's variables it drops.
pub fn environment(caller: &Env, config: &Config) -> (Env, Vec<String>) {
    let window = lookup(caller, "MIDNIGHT_WINDOW").unwrap_or("");
    let mut words: Vec<&str> = window
        .split(',')
        .map(text::strip)
        .filter(|w| !w.is_empty() && *w != "headless")
        .collect();
    words.push("headless");
    let mut env: Env = vec![
        ("PATH".into(), config.path.clone()),
        ("HOME".into(), config.home.clone()),
        ("TMPDIR".into(), config.tmp.clone()),
        ("XDG_RUNTIME_DIR".into(), config.xdg.clone()),
        ("MIDNIGHT_GPU".into(), "off".into()),
        ("MIDNIGHT_WINDOW".into(), words.join(",")),
    ];
    const KEPT: [&str; 3] = ["MIDNIGHT_THREADS", "MIDNIGHT_DRIVE", "MIDNIGHT_FORMAT"];
    for name in KEPT {
        if let Some(v) = lookup(caller, name) {
            env.push((name.to_string(), v.to_string()));
        }
    }
    let mut dropped: Vec<String> = caller
        .iter()
        .map(|(k, _)| k.clone())
        .filter(|k| !KEPT.contains(&k.as_str()))
        .collect();
    dropped.sort();
    (env, dropped)
}

/// The SHA-256 of a regular file, or None.
pub fn sha256(path: &str) -> Option<String> {
    if !proc::is_file(path) {
        return None;
    }
    fs::read(path).ok().map(|b| crate::sha256::hex(&b))
}

/// The program in dir: its .12am files outside docs/, as sorted relative
/// paths.
pub fn program_files(dir: &str) -> Vec<String> {
    let mut files: Vec<String> = proc::glob_suffix(dir, ".12am")
        .into_iter()
        .filter(|f| !f.starts_with("docs/"))
        .collect();
    files.sort();
    files
}

/// A hash of the program in dir: each file's path and contents.
pub fn program_sha256(dir: &str) -> Option<String> {
    let mut d = Sha256::new();
    for f in program_files(dir) {
        let bytes = fs::read(text::join(&[dir, &f])).ok()?;
        d.update(f.as_bytes());
        d.update(b"\0");
        d.update(&bytes);
        d.update(b"\0");
    }
    Some(d.hex())
}

pub fn load_average() -> Option<f64> {
    let text = fs::read_to_string("/proc/loadavg").ok()?;
    let first = text.split_whitespace().next()?;
    Some(first.parse::<f64>().unwrap_or(0.0))
}

/// The variables this process was started with, in their order.
pub fn caller_environment() -> Env {
    std::env::vars_os()
        .map(|(k, v)| {
            (
                k.to_string_lossy().into_owned(),
                v.to_string_lossy().into_owned(),
            )
        })
        .collect()
}

/// The wrapper itself: guards the call, runs the compiler, appends to the
/// log. Returns the exit status.
pub fn main(argv: &[String], config: &Config) -> Result<i32, Failure> {
    let caller = caller_environment();
    let cwd = std::env::current_dir()
        .map(|p| text::path_text(&p))
        .map_err(|e| Failure(format!("cannot read the working directory: {e}")))?;
    let root = config.root.as_str();
    let (env, dropped) = environment(&caller, config);
    let (files, _) = file_args(argv);
    let file = files.first().map(|f| resolve(f, &cwd));
    let shown_cwd = if inside(&cwd, root) {
        let rel = cwd.strip_prefix(root).unwrap_or(&cwd);
        rel.strip_prefix('/').unwrap_or(rel).to_string()
    } else {
        cwd.clone()
    };
    let shown_file = file.as_ref().map(|f| {
        if inside(f, root) {
            f.strip_prefix(&format!("{root}/")).unwrap_or(f).to_string()
        } else {
            f.clone()
        }
    });
    let mut passed = Map::new();
    for (k, v) in &env {
        if !["PATH", "HOME", "XDG_RUNTIME_DIR"].contains(&k.as_str()) {
            passed.set(k, v.as_str());
        }
    }
    let mut from_caller = Map::new();
    for (k, v) in &caller {
        if k.starts_with("MIDNIGHT_") {
            from_caller.set(k, v.as_str());
        }
    }
    let program = program_sha256(root);
    let mut record = obj! {
        "time" => text::iso8601_ms(SystemTime::now()),
        "cwd" => shown_cwd,
        "args" => argv.to_vec(),
        "file" => shown_file,
        "sha256" => file.as_ref().and_then(|f| sha256(f)),
        "program_sha256" => program.clone(),
        "passed" => passed,
        "dropped" => dropped,
        "caller" => from_caller,
        "load" => load_average(),
    };
    if argv
        .first()
        .is_some_and(|a| ["check", "build", "run"].contains(&a.as_str()))
    {
        keep_program(root, &config.programs, program.as_deref());
    }
    if let Some(reason) = refusal(argv, &caller, &cwd, root) {
        let line = format!("midnight: refused: {reason}");
        eprintln!("{line}");
        record.set("status", 2);
        record.set("refused", reason);
        record.set("stderr", format!("{line}\n"));
        record.set("stdout", "");
        append(&config.log, &record)?;
        return Ok(2);
    }
    let mut command = vec![config.compiler.clone()];
    command.extend(argv.iter().cloned());
    let mut r = proc::capture(
        &env,
        &command,
        std::path::Path::new(&cwd),
        config.limit,
        config.cap,
        true,
        &config.path,
    )?;
    if r.timeout {
        let line = format!("midnight: stopped after {} seconds", config.limit);
        eprintln!("{line}");
        r.err.extend_from_slice(format!("{line}\n").as_bytes());
    }
    record.set("status", r.status);
    record.set("timeout", r.timeout);
    record.set("ms", r.ms);
    record.set("stdout", text::scrub(&r.out));
    record.set("stderr", text::scrub(&r.err));
    record.set("stdout_cut", r.out_cut);
    record.set("stderr_cut", r.err_cut);
    append(&config.log, &record)?;
    Ok(r.status)
}

/// Copies the program in root to programs/SHA/, at the same relative
/// paths, unless that program is already kept.
pub fn keep_program(root: &str, programs: &str, sha: Option<&str>) {
    let Some(sha) = sha else { return };
    let dest = text::join(&[programs, sha]);
    if proc::is_dir(&dest) {
        return;
    }
    let partial = format!("{dest}.{}", std::process::id());
    let copied = (|| -> Result<(), Failure> {
        for f in program_files(root) {
            let target = text::join(&[&partial, &f]);
            proc::mkdir_p(&text::dirname(&target))?;
            proc::copy(&text::join(&[root, &f]), &target)?;
        }
        proc::mkdir_p(&partial)?;
        fs::rename(&partial, &dest).map_err(|e| Failure(e.to_string()))
    })();
    if copied.is_err() {
        proc::rm_rf(&partial);
    }
}

/// Appends a record to the log as one line, holding the file's lock.
pub fn append(log: &str, record: &Map) -> Result<(), Failure> {
    let mut f = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log)
        .map_err(|e| Failure(format!("{log}: {e}")))?;
    f.lock().map_err(|e| Failure(format!("{log}: {e}")))?;
    let line = format!("{}\n", json::generate(&Value::Obj(record.clone())));
    f.write_all(line.as_bytes())
        .map_err(|e| Failure(format!("{log}: {e}")))
}

/// Runs as the agent's `midnight` with the configuration beside exe.
pub fn run_as_wrapper(exe: &std::path::Path, argv: &[String]) -> i32 {
    let dir = exe.parent().map(text::path_text).unwrap_or_default();
    let config = proc::read(&text::join(&[&dir, CONFIG]))
        .and_then(|t| json::parse(&t).map_err(Failure))
        .and_then(|v| Config::from_json(&v));
    let result = config.and_then(|c| main(argv, &c));
    match result {
        Ok(status) => status,
        Err(Failure(message)) => {
            eprintln!("midnight: {message}");
            1
        }
    }
}
