//! Processes and files: running a command with limits, killing a process
//! group, temporary directories, walking a directory as a glob does.

use std::fs;
use std::io::{Read, Write};
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::Path;
use std::process::{Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use crate::Failure;

/// What a command did: its status (124 on the time limit, 128 + signal
/// when killed), what it wrote (at most the cap of each stream), whether a
/// stream was cut, whether the limit stopped it, and how long it took.
pub struct Captured {
    pub status: i32,
    pub out: Vec<u8>,
    pub err: Vec<u8>,
    pub out_cut: bool,
    pub err_cut: bool,
    pub timeout: bool,
    pub ms: u64,
}

/// Where a stream's bytes go besides the capture.
#[derive(Clone, Copy)]
enum Sink {
    None,
    Out,
    Err,
}

fn read_capped(mut from: impl Read, cap: usize, sink: Sink) -> (Vec<u8>, bool) {
    let mut buf = Vec::new();
    let mut cut = false;
    let mut chunk = vec![0u8; 65_536];
    let mut passing = true;
    loop {
        let n = match from.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        };
        let room = cap.saturating_sub(buf.len());
        if room == 0 {
            cut = true;
            continue;
        }
        let part = &chunk[..n.min(room)];
        if part.len() < n {
            cut = true;
        }
        buf.extend_from_slice(part);
        if passing {
            let r = match sink {
                Sink::None => Ok(()),
                Sink::Out => {
                    let mut o = std::io::stdout().lock();
                    o.write_all(part).and_then(|_| o.flush())
                }
                Sink::Err => {
                    let mut e = std::io::stderr().lock();
                    e.write_all(part).and_then(|_| e.flush())
                }
            };
            passing = r.is_ok();
        }
    }
    (buf, cut)
}

/// Starts a command. An executable another thread has just written can be
/// busy for a moment, while a process forked meanwhile still holds it
/// open: that start is tried again.
pub fn spawn(command: &mut Command) -> std::io::Result<std::process::Child> {
    let mut tries = 0;
    loop {
        match command.spawn() {
            Err(e) if e.kind() == std::io::ErrorKind::ExecutableFileBusy && tries < 100 => {
                tries += 1;
                thread::sleep(Duration::from_millis(10));
            }
            r => return r,
        }
    }
}

fn status_code(s: ExitStatus) -> i32 {
    match s.code() {
        Some(c) => c,
        None => 128 + s.signal().unwrap_or(0),
    }
}

/// Runs argv in a process group of its own with exactly env and stdin from
/// /dev/null, in chdir. Keeps at most cap bytes of each stream, and with
/// pass also writes them to this process's streams. Kills the group after
/// limit seconds, and once the command is done; kill names the directory
/// of the `kill` program.
pub fn capture(
    env: &[(String, String)],
    argv: &[String],
    chdir: &Path,
    limit: u64,
    cap: usize,
    pass: bool,
    kill: &str,
) -> Result<Captured, Failure> {
    let start = Instant::now();
    let mut child = spawn(
        Command::new(&argv[0])
            .args(&argv[1..])
            .env_clear()
            .envs(env.iter().map(|(k, v)| (k, v)))
            .current_dir(chdir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0),
    )
    .map_err(|e| Failure(format!("cannot run {}: {e}", argv[0])))?;
    let pid = child.id();
    let out = child.stdout.take();
    let err = child.stderr.take();
    let (osink, esink) = if pass {
        (Sink::Out, Sink::Err)
    } else {
        (Sink::None, Sink::None)
    };
    let out_reader = thread::spawn(move || match out {
        Some(o) => read_capped(o, cap, osink),
        None => (Vec::new(), false),
    });
    let err_reader = thread::spawn(move || match err {
        Some(e) => read_capped(e, cap, esink),
        None => (Vec::new(), false),
    });
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let _ = tx.send(child.wait());
    });
    let mut timeout = false;
    let status = match rx.recv_timeout(Duration::from_secs(limit)) {
        Ok(s) => {
            kill_group(pid, kill, "KILL");
            s
        }
        Err(_) => {
            timeout = true;
            kill_group(pid, kill, "KILL");
            rx.recv()
                .unwrap_or_else(|_| Err(std::io::Error::other("lost")))
        }
    };
    let (out, out_cut) = out_reader.join().unwrap_or_default();
    let (err, err_cut) = err_reader.join().unwrap_or_default();
    let status = if timeout {
        124
    } else {
        status.map(status_code).unwrap_or(1)
    };
    Ok(Captured {
        status,
        out,
        err,
        out_cut,
        err_cut,
        timeout,
        ms: start.elapsed().as_millis() as u64,
    })
}

/// Sends signal to the process group pgid with the `kill` in dir; does
/// nothing when there is no such group.
pub fn kill_group(pgid: u32, dir: &str, signal: &str) {
    let _ = Command::new(format!("{dir}/kill"))
        .arg(format!("-{signal}"))
        .arg("--")
        .arg(format!("-{pgid}"))
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

/// Random bytes from the system.
pub fn random_bytes(n: usize) -> Result<Vec<u8>, Failure> {
    let mut buf = vec![0u8; n];
    fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut buf))
        .map_err(|e| Failure(format!("/dev/urandom: {e}")))?;
    Ok(buf)
}

/// A new directory /tmp/eval.XXXXXXXX that only this user can enter.
pub fn mktemp() -> Result<String, Failure> {
    const LETTERS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    for _ in 0..100 {
        let name: String = random_bytes(8)?
            .iter()
            .map(|b| LETTERS[*b as usize % LETTERS.len()] as char)
            .collect();
        let path = format!("/tmp/eval.{name}");
        match fs::DirBuilder::new().mode(0o700).create(&path) {
            Ok(()) => return Ok(path),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(Failure(format!("mktemp failed: {e}"))),
        }
    }
    Err(Failure("mktemp failed".to_string()))
}

/// Whether path is a regular file (a link to one counts).
pub fn is_file(path: &str) -> bool {
    fs::metadata(path).map(|m| m.is_file()).unwrap_or(false)
}

/// Whether path is a directory (a link to one counts).
pub fn is_dir(path: &str) -> bool {
    fs::metadata(path).map(|m| m.is_dir()).unwrap_or(false)
}

/// Whether path is a symbolic link.
pub fn is_link(path: &str) -> bool {
    fs::symlink_metadata(path)
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false)
}

/// Whether anything is at path, links followed.
pub fn exists(path: &str) -> bool {
    fs::metadata(path).is_ok()
}

pub fn is_executable(path: &str) -> bool {
    fs::metadata(path)
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

pub fn realpath(path: &str) -> Result<String, Failure> {
    fs::canonicalize(path)
        .map(|p| p.to_string_lossy().into_owned())
        .map_err(|e| Failure(format!("{path}: {e}")))
}

pub fn read(path: &str) -> Result<String, Failure> {
    fs::read(path)
        .map(|b| String::from_utf8_lossy(&b).into_owned())
        .map_err(|e| Failure(format!("{path}: {e}")))
}

pub fn read_bytes(path: &str) -> Result<Vec<u8>, Failure> {
    fs::read(path).map_err(|e| Failure(format!("{path}: {e}")))
}

pub fn write(path: &str, data: impl AsRef<[u8]>) -> Result<(), Failure> {
    fs::write(path, data).map_err(|e| Failure(format!("{path}: {e}")))
}

pub fn mkdir_p(path: &str) -> Result<(), Failure> {
    fs::create_dir_all(path).map_err(|e| Failure(format!("{path}: {e}")))
}

pub fn copy(from: &str, to: &str) -> Result<(), Failure> {
    fs::copy(from, to)
        .map(|_| ())
        .map_err(|e| Failure(format!("{from}: {e}")))
}

/// Removes path and everything under it; nothing when it is not there.
pub fn rm_rf(path: &str) {
    match fs::symlink_metadata(path) {
        Ok(m) if m.is_dir() => {
            let _ = fs::remove_dir_all(path);
        }
        Ok(_) => {
            let _ = fs::remove_file(path);
        }
        Err(_) => {}
    }
}

/// Copies the tree at from to to, links as links.
pub fn copy_tree(from: &str, to: &str) -> Result<(), Failure> {
    let meta = fs::symlink_metadata(from).map_err(|e| Failure(format!("{from}: {e}")))?;
    if meta.file_type().is_symlink() {
        let target = fs::read_link(from).map_err(|e| Failure(format!("{from}: {e}")))?;
        std::os::unix::fs::symlink(target, to).map_err(|e| Failure(format!("{to}: {e}")))
    } else if meta.is_dir() {
        mkdir_p(to)?;
        let mut names: Vec<String> = fs::read_dir(from)
            .map_err(|e| Failure(format!("{from}: {e}")))?
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        for name in names {
            copy_tree(&format!("{from}/{name}"), &format!("{to}/{name}"))?;
        }
        Ok(())
    } else {
        copy(from, to)
    }
}

/// The names in a directory, sorted; none when it cannot be read.
pub fn children(dir: &str) -> Vec<String> {
    let mut names: Vec<String> = match fs::read_dir(dir) {
        Ok(rd) => rd
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect(),
        Err(_) => Vec::new(),
    };
    names.sort();
    names
}

/// Every entry under base as a relative path, each directory's entries in
/// sorted order, a directory before what is in it, as Ruby's `**/*` gives
/// them: links to directories are listed but not entered, and names that
/// start with `.` are skipped unless dot.
pub fn walk(base: &str, dot: bool) -> Vec<String> {
    let mut out = Vec::new();
    walk_into(base, "", dot, &mut out);
    out
}

fn walk_into(base: &str, rel: &str, dot: bool, out: &mut Vec<String>) {
    let dir = if rel.is_empty() {
        base.to_string()
    } else {
        format!("{base}/{rel}")
    };
    for name in children(&dir) {
        if !dot && name.starts_with('.') {
            continue;
        }
        let path = if rel.is_empty() {
            name.clone()
        } else {
            format!("{rel}/{name}")
        };
        let full = format!("{base}/{path}");
        out.push(path.clone());
        if !is_link(&full) && is_dir(&full) {
            walk_into(base, &path, dot, out);
        }
    }
}

/// The relative paths under base whose last part ends with suffix, in the
/// order of `walk`.
pub fn glob_suffix(base: &str, suffix: &str) -> Vec<String> {
    walk(base, false)
        .into_iter()
        .filter(|p| crate::text::basename(p).ends_with(suffix))
        .collect()
}

/// The output of a command and whether it succeeded; its standard error
/// goes nowhere.
pub fn output(argv: &[&str]) -> Result<(String, bool), Failure> {
    let o = Command::new(argv[0])
        .args(&argv[1..])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map_err(|e| Failure(format!("cannot run {}: {e}", argv[0])))?;
    Ok((
        String::from_utf8_lossy(&o.stdout).into_owned(),
        o.status.success(),
    ))
}

/// The output of a command, its standard error passed on.
pub fn output_loud(argv: &[&str]) -> Result<(String, bool), Failure> {
    let o = Command::new(argv[0])
        .args(&argv[1..])
        .stdin(Stdio::null())
        .stderr(Stdio::inherit())
        .output()
        .map_err(|e| Failure(format!("cannot run {}: {e}", argv[0])))?;
    Ok((
        String::from_utf8_lossy(&o.stdout).into_owned(),
        o.status.success(),
    ))
}
