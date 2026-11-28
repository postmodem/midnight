//! `midnight session` and `midnight drive`: a program held in a session.
//!
//! `session start` builds a program and starts a holder: `midnight` again,
//! with a word of its own, in a process group of its own. The holder runs
//! the program with `MIDNIGHT_DRIVE=-`, its three standard streams as
//! pipes, and serves `drive`, `session stop` and `session list` over a Unix
//! socket in the session's directory. What passes over the socket is
//! `midnight`'s own: frames of a header line, `TAG LEN`, and `LEN` bytes.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, ExitStatus, Stdio};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use crate::json::{self, Format, Object};

/// The word that starts `midnight` as a holder.
pub const HOLDER: &str = "session-holder";

/// The longest socket path a session may have, in bytes.
const SOCKET_MOST: usize = 100;

/// How long `session start` waits for the program's first `ready`.
const START_SECONDS: u64 = 60;

/// How long `session stop` waits for the program to end before it kills
/// it.
const STOP_WAIT: Duration = Duration::from_secs(5);

/// Exit status for a usage error.
const USAGE_ERROR: u8 = 2;

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// What a command of this module did wrong in its arguments: a usage error.
pub struct Usage(pub String);

/// Runs `session …`, `drive …` or the holder, given the arguments after
/// `midnight`. Returns the exit status, or a usage error.
pub fn main(args: &[String]) -> Result<u8, Usage> {
    match args.first().map(String::as_str) {
        Some("drive") => drive(&args[1..]),
        Some(HOLDER) => Ok(holder(&args[1..])),
        _ => match args.get(1).map(String::as_str) {
            Some("start") => start(&args[2..]),
            Some("stop") => stop(&args[2..]),
            Some("list") => list(&args[2..]),
            Some(other) => Err(Usage(format!("unknown command `session {other}`"))),
            None => Err(Usage(
                "`session` needs `start`, `stop` or `list`".to_string(),
            )),
        },
    }
}

// ---- Arguments --------------------------------------------------------------

/// Reads `--format text|json` from `args` at `at`, given twice or not.
fn format_arg(args: &[String], at: &mut usize, format: &mut Option<Format>) -> Result<(), Usage> {
    if format.is_some() {
        return Err(Usage("`--format` is given twice".to_string()));
    }
    *at += 1;
    *format = Some(match args.get(*at).map(String::as_str) {
        Some("text") => Format::Text,
        Some("json") => Format::Json,
        Some(other) => {
            return Err(Usage(format!(
                "`--format` takes `text` or `json`, not `{other}`"
            )));
        }
        None => return Err(Usage("`--format` needs `text` or `json`".to_string())),
    });
    Ok(())
}

/// The value after an option at `at`.
fn value_arg<'a>(args: &'a [String], at: &mut usize, option: &str) -> Result<&'a str, Usage> {
    *at += 1;
    args.get(*at)
        .map(String::as_str)
        .ok_or_else(|| Usage(format!("`{option}` needs a value")))
}

// ---- Messages ----------------------------------------------------------------

/// Where the lines of a command of this module go, and in which form.
#[derive(Clone, Copy)]
struct Say {
    format: Format,
}

impl Say {
    /// Writes the JSON form's first line to standard error.
    fn first_line(self) {
        if self.format == Format::Json {
            eprint!("{}", json::first_line());
        }
    }

    /// Writes a message, `text` without `midnight: `, to standard error.
    fn error(self, text: &str) {
        match self.format {
            Format::Text => eprintln!("midnight: {text}"),
            Format::Json => {
                let mut line = Object::new("session_error");
                line.str("message", text);
                eprint!("{}", line.line());
            }
        }
    }

    /// Says that the session `name` started.
    fn started(self, name: &str) {
        match self.format {
            Format::Text => eprintln!("midnight: session: started `{name}`"),
            Format::Json => {
                let mut line = Object::new("session");
                line.str("name", name).str("event", "started");
                eprint!("{}", line.line());
            }
        }
    }

    /// Says that the session `name` stopped with `status`.
    fn stopped(self, name: &str, status: i64) {
        match self.format {
            Format::Text => {
                eprintln!("midnight: session: `{name}` stopped with status {status}")
            }
            Format::Json => {
                let mut line = Object::new("session");
                line.str("name", name)
                    .str("event", "stopped")
                    .int("status", status);
                eprint!("{}", line.line());
            }
        }
    }
}

/// Writes bytes of the program's to standard output or standard error.
fn pass_on(out: bool, bytes: &[u8]) {
    if bytes.is_empty() {
        return;
    }
    if out {
        let mut stdout = std::io::stdout().lock();
        let _ = stdout.write_all(bytes);
        let _ = stdout.flush();
    } else {
        let mut stderr = std::io::stderr().lock();
        let _ = stderr.write_all(bytes);
        let _ = stderr.flush();
    }
}

// ---- Where sessions live ------------------------------------------------------

/// The user's number: that of a file made for the purpose in the temporary
/// directory.
fn user_number() -> Result<u32, String> {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |time| time.subsec_nanos());
    let probe = std::env::temp_dir().join(format!("midnight-user-{}-{nanos}", std::process::id()));
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)
        .map_err(|error| format!("can't make a file in the temporary directory: {error}"))?;
    let uid = file.metadata().map(|metadata| metadata.uid());
    drop(file);
    let _ = std::fs::remove_file(&probe);
    uid.map_err(|error| format!("can't read a file in the temporary directory: {error}"))
}

/// The directory that holds the sessions' directories.
fn sessions_dir(uid: u32) -> PathBuf {
    match std::env::var_os("XDG_RUNTIME_DIR").filter(|dir| !dir.is_empty()) {
        Some(dir) => PathBuf::from(dir).join("midnight").join("sessions"),
        None => std::env::temp_dir()
            .join(format!("midnight-{uid}"))
            .join("sessions"),
    }
}

/// Requires `dir`, when it exists, to be a directory of the user's own
/// with mode 0700, and makes it so when it does not exist and `make`.
/// Returns whether it exists now.
fn private_dir(dir: &Path, uid: u32, make: bool) -> Result<bool, String> {
    let refused = || {
        format!(
            "can't keep sessions in `{}`: it must be the user's own, with mode 0700",
            dir.display()
        )
    };
    match std::fs::symlink_metadata(dir) {
        Ok(metadata) => {
            if metadata.is_dir()
                && metadata.uid() == uid
                && metadata.permissions().mode() & 0o777 == 0o700
            {
                Ok(true)
            } else {
                Err(refused())
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            if !make {
                return Ok(false);
            }
            match std::fs::DirBuilder::new().mode(0o700).create(dir) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => {
                    return Err(format!("can't make `{}`: {error}", dir.display()));
                }
            }
            private_dir(dir, uid, false)
        }
        Err(error) => Err(format!("can't read `{}`: {error}", dir.display())),
    }
}

/// The directory of sessions, each directory on its way checked, made
/// when `make`; `None` when it does not exist and not `make`.
fn sessions(make: bool) -> Result<Option<PathBuf>, String> {
    let uid = user_number()?;
    let dir = sessions_dir(uid);
    let parent = dir.parent().expect("the sessions directory has a parent");
    if !private_dir(parent, uid, make)? {
        return Ok(None);
    }
    if !private_dir(&dir, uid, make)? {
        return Ok(None);
    }
    Ok(Some(dir))
}

/// Whether `name` can name a session.
fn valid_name(name: &str) -> bool {
    (1..=32).contains(&name.len())
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

/// What a session's holder answers about it.
struct Status {
    file: String,
    tick: i64,
    ended: bool,
}

/// Asks the session in `dir` how it stands. `None` when its socket does
/// not answer: the session is stale.
fn status_of(dir: &Path) -> Option<Status> {
    let mut link = Link::open(dir, "status").ok()?;
    let (tag, payload) = link.read().ok()??;
    if tag != "status" {
        return None;
    }
    let payload = String::from_utf8_lossy(&payload).into_owned();
    let mut parts = payload.splitn(3, '\0');
    let tick = parts.next()?.parse().ok()?;
    let ended = parts.next()? == "true";
    let file = parts.next()?.to_string();
    Some(Status { file, tick, ended })
}

/// The sessions that run, by name, with how each stands; the directory of
/// each stale one is removed.
fn running(dir: &Path) -> Vec<(String, Status)> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
        .filter(|name| valid_name(name))
        .collect();
    names.sort();
    let mut found = Vec::new();
    for name in names {
        let session = dir.join(&name);
        match status_of(&session) {
            Some(status) => found.push((name, status)),
            None => {
                let _ = std::fs::remove_dir_all(&session);
            }
        }
    }
    found
}

/// The session a command names, or the only one that runs.
fn chosen(name: Option<&str>) -> Result<(String, PathBuf), String> {
    let none = || "no session is running; start one with `midnight session start FILE`".to_string();
    let dir = sessions(false)?.ok_or_else(none)?;
    if let Some(name) = name {
        if !valid_name(name) {
            return Err(format!(
                "`{name}` can't name a session: a name is 1 to 32 ASCII letters, digits, `-` and `_`"
            ));
        }
        let session = dir.join(name);
        if status_of(&session).is_some() {
            return Ok((name.to_string(), session));
        }
        if session.exists() {
            let _ = std::fs::remove_dir_all(&session);
        }
        return Err(format!(
            "no session `{name}` is running; `midnight session list` lists those that are"
        ));
    }
    let found = running(&dir);
    match found.as_slice() {
        [] => Err(none()),
        [(name, _)] => Ok((name.clone(), dir.join(name))),
        many => Err(format!(
            "{} sessions are running ({}); name one with `--session`",
            many.len(),
            many.iter()
                .map(|(name, _)| name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

// ---- Frames -----------------------------------------------------------------

/// Writes one frame: `TAG LEN`, a line break, and the payload.
fn send(out: &mut impl Write, tag: &str, payload: &[u8]) -> std::io::Result<()> {
    out.write_all(format!("{tag} {}\n", payload.len()).as_bytes())?;
    out.write_all(payload)?;
    out.flush()
}

/// Reads one frame; `None` at the end of the stream.
fn receive(input: &mut impl BufRead) -> std::io::Result<Option<(String, Vec<u8>)>> {
    let mut head = String::new();
    if input.read_line(&mut head)? == 0 {
        return Ok(None);
    }
    let bad = || std::io::Error::new(std::io::ErrorKind::InvalidData, "a bad frame");
    let (tag, len) = head.trim_end().split_once(' ').ok_or_else(bad)?;
    let len: usize = len.parse().map_err(|_| bad())?;
    let mut payload = vec![0; len];
    input.read_exact(&mut payload)?;
    Ok(Some((tag.to_string(), payload)))
}

/// A client's connection to a holder.
struct Link {
    reader: BufReader<UnixStream>,
    writer: UnixStream,
}

/// Why a connection to a holder failed.
enum LinkError {
    /// The socket does not answer.
    Stale,
    /// The holder is of another version, which it gives.
    Version(String),
}

impl Link {
    /// Connects to the holder of the session in `dir` and asks for
    /// `request`.
    fn open(dir: &Path, request: &str) -> Result<Link, LinkError> {
        let stream = UnixStream::connect(dir.join("socket")).map_err(|_| LinkError::Stale)?;
        let writer = stream.try_clone().map_err(|_| LinkError::Stale)?;
        let mut link = Link {
            reader: BufReader::new(stream),
            writer,
        };
        send(
            &mut link.writer,
            "hello",
            format!("{VERSION} {request}").as_bytes(),
        )
        .map_err(|_| LinkError::Stale)?;
        match link.read() {
            Ok(Some((tag, _))) if tag == "hello" => Ok(link),
            Ok(Some((tag, version))) if tag == "version" => Err(LinkError::Version(
                String::from_utf8_lossy(&version).into_owned(),
            )),
            _ => Err(LinkError::Stale),
        }
    }

    fn read(&mut self) -> std::io::Result<Option<(String, Vec<u8>)>> {
        receive(&mut self.reader)
    }

    fn send(&mut self, tag: &str, payload: &[u8]) -> std::io::Result<()> {
        send(&mut self.writer, tag, payload)
    }
}

/// What a holder's frames ended with, after their output was passed on.
enum End {
    /// The command was carried out, or refused.
    Done { refused: bool },
    /// The session started.
    Started,
    /// The program stopped, with this status.
    Stopped(i64),
    /// The program wrote no `ready` within this many seconds.
    TimedOut(u64),
    /// The holder said this went wrong.
    Failed(String),
    /// The holder went away.
    Lost,
}

/// Passes on the output frames of one answer and returns how it ended.
fn answer(link: &mut Link) -> End {
    loop {
        let Ok(Some((tag, payload))) = link.read() else {
            return End::Lost;
        };
        let text = String::from_utf8_lossy(&payload).into_owned();
        match tag.as_str() {
            "out" => pass_on(true, &payload),
            "err" => pass_on(false, &payload),
            "done" => {
                return End::Done {
                    refused: text == "refused",
                };
            }
            "started" => return End::Started,
            "stopped" => return End::Stopped(text.parse().unwrap_or(-1)),
            "timeout" => return End::TimedOut(text.parse().unwrap_or(START_SECONDS)),
            "error" => return End::Failed(text),
            _ => return End::Lost,
        }
    }
}

// ---- session start ----------------------------------------------------------

fn start(args: &[String]) -> Result<u8, Usage> {
    let mut file = None;
    let mut name = None;
    let mut debug = false;
    let mut format = None;
    let mut idle: u64 = 60;
    let mut at = 0;
    while at < args.len() {
        match args[at].as_str() {
            "--name" => name = Some(value_arg(args, &mut at, "--name")?.to_string()),
            "--debug" => debug = true,
            "--format" => format_arg(args, &mut at, &mut format)?,
            "--idle" => {
                let value = value_arg(args, &mut at, "--idle")?;
                idle = match value.parse::<u64>() {
                    Ok(minutes) if (1..=1440).contains(&minutes) => minutes,
                    _ => {
                        return Err(Usage(format!(
                            "`--idle` takes a number of minutes from 1 to 1440, not `{value}`"
                        )));
                    }
                };
            }
            flag if flag.starts_with('-') => {
                return Err(Usage(format!(
                    "unknown option `{flag}` for `session start`"
                )));
            }
            path if file.is_none() => file = Some(path.to_string()),
            extra => return Err(Usage(format!("unexpected argument `{extra}`"))),
        }
        at += 1;
    }
    let file = file.ok_or_else(|| Usage("`session start` needs a file".to_string()))?;
    let format = format.unwrap_or(Format::Text);
    let say = Say { format };
    say.first_line();
    let name = name.unwrap_or_else(|| {
        let base = Path::new(&file)
            .file_name()
            .map_or_else(|| file.clone(), |name| name.to_string_lossy().into_owned());
        base.strip_suffix(".12am").unwrap_or(&base).to_string()
    });
    match begin(&file, &name, debug, format, idle) {
        Ok(status) => Ok(status),
        Err(message) => {
            say.error(&message);
            Ok(1)
        }
    }
}

/// Checks what `session start` can check before it builds, builds, starts
/// the holder and waits for the program's first `ready`.
fn begin(file: &str, name: &str, debug: bool, format: Format, idle: u64) -> Result<u8, String> {
    let say = Say { format };
    if !valid_name(name) {
        return Err(format!(
            "`{name}` can't name a session: a name is 1 to 32 ASCII letters, digits, `-` and `_`; give one with `--name`"
        ));
    }
    let threads = std::env::var("MIDNIGHT_THREADS").unwrap_or_default();
    if threads.split(',').any(|word| word.trim() == "awake") {
        return Err(
            "MIDNIGHT_THREADS has `awake`, which keeps every thread busy while a session waits"
                .to_string(),
        );
    }
    let dir = sessions(true)?.expect("the sessions directory was made");
    let session = dir.join(name);
    let socket = session.join("socket");
    if socket.as_os_str().len() > SOCKET_MOST {
        return Err(format!(
            "the socket `{}` would be longer than {SOCKET_MOST} bytes; set XDG_RUNTIME_DIR to a shorter directory",
            socket.display()
        ));
    }
    if session.exists() {
        if status_of(&session).is_some() {
            return Err(format!(
                "session `{name}` is already running; stop it, or give another `--name`"
            ));
        }
        let _ = std::fs::remove_dir_all(&session);
    }
    match std::fs::DirBuilder::new().mode(0o700).create(&session) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            return Err(format!(
                "session `{name}` is already running; stop it, or give another `--name`"
            ));
        }
        Err(error) => return Err(format!("can't make `{}`: {error}", session.display())),
    }
    let program = session.join("program");
    let Some((c_source, parts)) = crate::compile(Path::new(file), false, format) else {
        let _ = std::fs::remove_dir_all(&session);
        return Ok(1);
    };
    if let Err(message) = crate::driver::build(&c_source, &program, debug, parts, format) {
        eprint!("{}", message.render(format));
        let _ = std::fs::remove_dir_all(&session);
        return Ok(1);
    }
    let format_word = if format == Format::Json {
        "json"
    } else {
        "text"
    };
    let exe = std::env::current_exe().map_err(|error| format!("can't find midnight: {error}"))?;
    let mut holder = Command::new(exe)
        .arg(HOLDER)
        .arg(&session)
        .arg(name)
        .arg(file)
        .arg(format_word)
        .arg(idle.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .map_err(|error| {
            let _ = std::fs::remove_dir_all(&session);
            format!("can't start the session's holder: {error}")
        })?;
    let started = Instant::now();
    let mut link = loop {
        match Link::open(&session, "start") {
            Ok(link) => break link,
            Err(LinkError::Version(version)) => {
                return Err(format!("the holder is midnight {version}"));
            }
            Err(LinkError::Stale) => {}
        }
        if holder.try_wait().ok().flatten().is_some() || started.elapsed() > STOP_WAIT {
            let _ = std::fs::remove_dir_all(&session);
            return Err("the session's holder ended before it started".to_string());
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    match answer(&mut link) {
        End::Started => {
            say.started(name);
            Ok(0)
        }
        End::Stopped(_) => Ok(1),
        End::TimedOut(seconds) => Err(format!(
            "the program wrote no `ready` within {seconds} seconds; a program reads its first command at its first tick, or at its end"
        )),
        End::Failed(message) => Err(message),
        End::Done { .. } | End::Lost => Err("the session's holder went away".to_string()),
    }
}

// ---- drive -----------------------------------------------------------------

fn drive(args: &[String]) -> Result<u8, Usage> {
    let mut session = None;
    let mut format = None;
    let mut commands: Vec<String> = Vec::new();
    let mut from_input = false;
    let mut at = 0;
    while at < args.len() {
        match args[at].as_str() {
            "--session" => session = Some(value_arg(args, &mut at, "--session")?.to_string()),
            "--format" => format_arg(args, &mut at, &mut format)?,
            "-" if commands.is_empty() && !from_input => from_input = true,
            flag if flag.starts_with("--") => {
                return Err(Usage(format!("unknown option `{flag}` for `drive`")));
            }
            command => {
                if from_input {
                    return Err(Usage(
                        "`drive -` takes its commands from standard input alone".to_string(),
                    ));
                }
                if command.contains(['\n', '\r']) {
                    return Err(Usage(
                        "a command is one line; give each as its own argument".to_string(),
                    ));
                }
                commands.push(command.to_string());
            }
        }
        at += 1;
    }
    if commands.is_empty() && !from_input {
        return Err(Usage(
            "`drive` needs a command, or `-` for commands on standard input".to_string(),
        ));
    }
    let say = Say {
        format: format.unwrap_or(Format::Text),
    };
    say.first_line();
    let (name, dir) = match chosen(session.as_deref()) {
        Ok(found) => found,
        Err(message) => {
            say.error(&message);
            return Ok(1);
        }
    };
    let mut link = match Link::open(&dir, "drive") {
        Ok(link) => link,
        Err(LinkError::Stale) => {
            let _ = std::fs::remove_dir_all(&dir);
            say.error(&format!(
                "no session `{name}` is running; `midnight session list` lists those that are"
            ));
            return Ok(1);
        }
        Err(LinkError::Version(version)) => {
            say.error(&format!(
                "session `{name}` is held by midnight {version}, and this is midnight {VERSION}"
            ));
            return Ok(1);
        }
    };
    let mut input = std::io::stdin().lock();
    let mut given = commands.into_iter();
    let mut refused = false;
    loop {
        let command = if from_input {
            let mut line = String::new();
            match input.read_line(&mut line) {
                Ok(0) | Err(_) => break,
                Ok(_) => line.trim_end_matches(['\n', '\r']).to_string(),
            }
        } else {
            match given.next() {
                Some(command) => command,
                None => break,
            }
        };
        if link.send("command", command.as_bytes()).is_err() {
            say.error(&format!("session `{name}` went away"));
            return Ok(1);
        }
        match answer(&mut link) {
            End::Done { refused: was } => refused |= was,
            End::Stopped(status) => {
                say.stopped(&name, status);
                let quit = command.split_whitespace().collect::<Vec<_>>() == ["quit"];
                return Ok(if quit && !refused { 0 } else { 1 });
            }
            End::Failed(message) => {
                say.error(&message);
                return Ok(1);
            }
            End::Started | End::TimedOut(_) | End::Lost => {
                say.error(&format!("session `{name}` went away"));
                return Ok(1);
            }
        }
    }
    Ok(if refused { 1 } else { 0 })
}

// ---- session stop and list --------------------------------------------------

fn stop(args: &[String]) -> Result<u8, Usage> {
    let mut name = None;
    let mut format = None;
    let mut at = 0;
    while at < args.len() {
        match args[at].as_str() {
            "--format" => format_arg(args, &mut at, &mut format)?,
            flag if flag.starts_with('-') => {
                return Err(Usage(format!("unknown option `{flag}` for `session stop`")));
            }
            given if name.is_none() => name = Some(given.to_string()),
            extra => return Err(Usage(format!("unexpected argument `{extra}`"))),
        }
        at += 1;
    }
    let say = Say {
        format: format.unwrap_or(Format::Text),
    };
    say.first_line();
    let (name, dir) = match chosen(name.as_deref()) {
        Ok(found) => found,
        Err(message) => {
            say.error(&message);
            return Ok(1);
        }
    };
    let Ok(mut link) = Link::open(&dir, "stop") else {
        let _ = std::fs::remove_dir_all(&dir);
        say.error(&format!(
            "no session `{name}` is running; `midnight session list` lists those that are"
        ));
        return Ok(1);
    };
    match answer(&mut link) {
        End::Stopped(status) => {
            say.stopped(&name, status);
            Ok(0)
        }
        _ => {
            say.error(&format!("session `{name}` went away"));
            Ok(1)
        }
    }
}

fn list(args: &[String]) -> Result<u8, Usage> {
    let mut format = None;
    let mut at = 0;
    while at < args.len() {
        match args[at].as_str() {
            "--format" => format_arg(args, &mut at, &mut format)?,
            flag if flag.starts_with('-') => {
                return Err(Usage(format!("unknown option `{flag}` for `session list`")));
            }
            extra => return Err(Usage(format!("unexpected argument `{extra}`"))),
        }
        at += 1;
    }
    let format = format.unwrap_or(Format::Text);
    let say = Say { format };
    if format == Format::Json {
        print!("{}", json::first_line());
    }
    let found = match sessions(false) {
        Ok(Some(dir)) => running(&dir),
        Ok(None) => Vec::new(),
        Err(message) => {
            say.error(&message);
            return Ok(1);
        }
    };
    if format == Format::Json {
        for (name, status) in &found {
            let mut line = Object::new("session_listed");
            line.str("name", name)
                .str("file", &status.file)
                .int("tick", status.tick)
                .bool("ended", status.ended);
            print!("{}", line.line());
        }
        return Ok(0);
    }
    let rows: Vec<[String; 4]> = found
        .iter()
        .map(|(name, status)| {
            [
                name.clone(),
                status.file.clone(),
                format!(
                    "{} tick{}",
                    status.tick,
                    if status.tick == 1 { "" } else { "s" }
                ),
                if status.ended { "ended" } else { "running" }.to_string(),
            ]
        })
        .collect();
    let mut widths = [0; 3];
    for row in &rows {
        for (width, cell) in widths.iter_mut().zip(row.iter()) {
            *width = (*width).max(cell.chars().count());
        }
    }
    for row in &rows {
        let mut line = String::new();
        for (cell, width) in row.iter().zip(widths.iter()) {
            line.push_str(cell);
            line.push_str(&" ".repeat(width - cell.chars().count() + 2));
        }
        line.push_str(&row[3]);
        println!("{line}");
    }
    Ok(0)
}

// ---- The holder ----------------------------------------------------------------

/// What the holder knows of its program, shared by its threads.
#[derive(Default)]
struct State {
    /// Standard output read and not yet handed on, and how many bytes were
    /// handed on before it.
    out: Vec<u8>,
    out_before: u64,
    out_closed: bool,
    /// Lines of standard error read and not yet handed on.
    lines: VecDeque<Vec<u8>>,
    err_closed: bool,
    /// From the last `ready` line.
    tick: i64,
    ended: bool,
    readies: u64,
    last_ready: Option<Instant>,
    /// A command has been sent and its `ready` has not come.
    in_command: bool,
    /// The program's status, once it has ended.
    status: Option<i64>,
    /// The program was stopped for writing no first `ready` in time.
    timed_out: bool,
    /// Clients being answered.
    clients: u32,
}

struct Holder {
    dir: PathBuf,
    file: String,
    json: bool,
    state: Mutex<State>,
    changed: Condvar,
    stdin: Mutex<Option<ChildStdin>>,
    child: Mutex<Child>,
    /// Taken by a `drive` for as long as it is connected.
    turn: Mutex<()>,
}

/// What a `ready` line says: Time.tick, the bytes of standard output, and
/// whether the program has ended.
fn ready_line(line: &[u8], json: bool) -> Option<(i64, u64, bool)> {
    let line = std::str::from_utf8(line).ok()?.trim_end_matches('\n');
    if json {
        let rest = line.strip_prefix("{\"kind\":\"ready\",\"tick\":")?;
        let (tick, rest) = rest.split_once(",\"output_bytes\":")?;
        let (bytes, rest) = rest.split_once(",\"ended\":")?;
        return Some((tick.parse().ok()?, bytes.parse().ok()?, rest == "true}"));
    }
    let rest = line.strip_prefix("midnight: drive: ready: ")?;
    let mut parts = rest.split(", ");
    let tick = parts.next()?.split(' ').next()?.parse().ok()?;
    let bytes = parts.next()?.split(' ').next()?.parse().ok()?;
    Some((tick, bytes, parts.next() == Some("the program has ended")))
}

/// Whether a line refuses a command.
fn refusal(line: &[u8], json: bool) -> bool {
    if json {
        line.starts_with(b"{\"kind\":\"refused\",")
    } else {
        line.starts_with(b"midnight: drive: refused: ")
    }
}

/// What a program said on standard error for one line of commands.
struct Said {
    err: Vec<u8>,
    refused: bool,
    /// The `ready` line did not come: the program ended.
    ended: bool,
}

impl Holder {
    /// Waits until a `ready` line comes or standard error ends, and takes
    /// the lines before it. The standard output written meanwhile, up to
    /// the count the `ready` line gives, goes to `out` as it comes; after
    /// the end, all of it, and the program's status is known.
    fn take(&self, out: &mut dyn FnMut(&[u8])) -> Said {
        let mut state = self.state.lock().unwrap();
        let mut err = Vec::new();
        let mut refused = false;
        loop {
            if !state.out.is_empty() {
                // Everything written before the next `ready` belongs to
                // this line: the program writes nothing while it waits.
                let chunk = std::mem::take(&mut state.out);
                state.out_before += chunk.len() as u64;
                drop(state);
                out(&chunk);
                state = self.state.lock().unwrap();
                continue;
            }
            while let Some(line) = state.lines.pop_front() {
                if let Some((_, bytes, _)) = ready_line(&line, self.json) {
                    while state.out_before + (state.out.len() as u64) < bytes && !state.out_closed {
                        state = self.changed.wait(state).unwrap();
                    }
                    let count =
                        (bytes.saturating_sub(state.out_before) as usize).min(state.out.len());
                    let chunk: Vec<u8> = state.out.drain(..count).collect();
                    state.out_before += count as u64;
                    drop(state);
                    out(&chunk);
                    return Said {
                        err,
                        refused,
                        ended: false,
                    };
                }
                refused |= refusal(&line, self.json);
                err.extend_from_slice(&line);
            }
            if state.err_closed && state.out_closed && state.status.is_some() {
                let chunk = std::mem::take(&mut state.out);
                drop(state);
                out(&chunk);
                return Said {
                    err,
                    refused,
                    ended: true,
                };
            }
            state = self.changed.wait(state).unwrap();
        }
    }

    fn status(&self) -> i64 {
        self.state.lock().unwrap().status.unwrap_or(-1)
    }

    /// Closes the program's standard input, which ends it, and kills it
    /// when it has not ended after `STOP_WAIT`.
    fn close_input(&self) {
        self.stdin.lock().unwrap().take();
        let started = Instant::now();
        let mut state = self.state.lock().unwrap();
        while state.status.is_none() {
            let left = STOP_WAIT.saturating_sub(started.elapsed());
            if left.is_zero() {
                drop(state);
                let _ = self.child.lock().unwrap().kill();
                return;
            }
            state = self.changed.wait_timeout(state, left).unwrap().0;
        }
    }

    /// Removes the session's directory and ends the holder once the
    /// program has ended, its streams are read and no client is being
    /// answered.
    fn finish_if_done(&self) {
        let state = self.state.lock().unwrap();
        if state.status.is_some() && state.err_closed && state.out_closed && state.clients == 0 {
            let _ = std::fs::remove_dir_all(&self.dir);
            std::process::exit(0);
        }
    }

    /// Takes what the program says for one line, as `take` does, its
    /// standard output sent on to `out` as it comes. Returns false with it
    /// when the client has gone.
    fn take_to(&self, out: &mut UnixStream) -> (Said, bool) {
        let mut here = true;
        let said = self.take(&mut |chunk| {
            here = here && (chunk.is_empty() || send(out, "out", chunk).is_ok());
        });
        (said, here)
    }

    /// Sends the rest of what the program said: its lines, then how the
    /// line ended. Returns false when the client has gone.
    fn hand_on(&self, out: &mut UnixStream, said: &Said, done: bool) -> bool {
        let ended = if said.ended {
            // The session is gone before its client hears so, so that the
            // name is free when the client goes on.
            let _ = std::fs::remove_dir_all(&self.dir);
            ("stopped", self.status().to_string())
        } else if done {
            (
                "done",
                if said.refused { "refused" } else { "ok" }.to_string(),
            )
        } else {
            ("started", String::new())
        };
        send(out, "err", &said.err).is_ok() && send(out, ended.0, ended.1.as_bytes()).is_ok()
    }

    /// Answers one client.
    fn serve(&self, stream: UnixStream) {
        let Ok(mut out) = stream.try_clone() else {
            return;
        };
        let mut input = BufReader::new(stream);
        let Ok(Some((tag, hello))) = receive(&mut input) else {
            return;
        };
        let hello = String::from_utf8_lossy(&hello).into_owned();
        let (version, request) = hello.split_once(' ').unwrap_or((&hello, ""));
        if tag != "hello" || (version != VERSION && request != "stop" && request != "status") {
            let _ = send(&mut out, "version", VERSION.as_bytes());
            return;
        }
        if send(&mut out, "hello", b"").is_err() {
            return;
        }
        match request {
            "status" => {
                let state = self.state.lock().unwrap();
                let payload = format!("{}\0{}\0{}", state.tick, state.ended, self.file);
                drop(state);
                let _ = send(&mut out, "status", payload.as_bytes());
            }
            "start" => {
                let (said, _) = self.take_to(&mut out);
                let timed_out = self.state.lock().unwrap().timed_out;
                if said.ended && timed_out {
                    let _ = send(&mut out, "err", &said.err);
                    let _ = send(&mut out, "timeout", start_seconds().to_string().as_bytes());
                } else {
                    self.hand_on(&mut out, &said, false);
                }
            }
            "stop" => {
                self.close_input();
                let (said, _) = self.take_to(&mut out);
                self.hand_on(&mut out, &said, true);
            }
            "drive" => {
                let _turn = self.turn.lock().unwrap();
                while let Ok(Some((tag, command))) = receive(&mut input) {
                    if tag != "command" {
                        break;
                    }
                    // A program whose input is closed ends, and `take` says
                    // so.
                    if let Some(stdin) = self.stdin.lock().unwrap().as_mut() {
                        self.state.lock().unwrap().in_command = true;
                        let _ = stdin
                            .write_all(&command)
                            .and_then(|()| stdin.write_all(b"\n"))
                            .and_then(|()| stdin.flush());
                    }
                    let (said, here) = self.take_to(&mut out);
                    if !here || !self.hand_on(&mut out, &said, true) || said.ended {
                        break;
                    }
                }
            }
            _ => {}
        }
    }
}

/// How long the holder waits for the program's first `ready`.
fn start_seconds() -> u64 {
    std::env::var("MIDNIGHT_SESSION_START_SECONDS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(START_SECONDS)
}

/// Reads `MIDNIGHT_WINDOW` for the program: `headless`, and `info` when the
/// caller's variable has it.
fn window_words() -> String {
    let caller = std::env::var("MIDNIGHT_WINDOW").unwrap_or_default();
    if caller.split(',').any(|word| word == "info") {
        "headless,info".to_string()
    } else {
        "headless".to_string()
    }
}

/// The holder: `DIR NAME FILE FORMAT IDLE`. Serves the session in `DIR`
/// until its program ends.
fn holder(args: &[String]) -> u8 {
    let [dir, _name, file, format, idle] = args else {
        return USAGE_ERROR;
    };
    let dir = PathBuf::from(dir);
    let json = format == "json";
    let idle = std::env::var("MIDNIGHT_SESSION_IDLE_SECONDS")
        .ok()
        .and_then(|value| value.parse().ok())
        .map(Duration::from_secs)
        .unwrap_or_else(|| Duration::from_secs(idle.parse::<u64>().unwrap_or(60) * 60));
    let socket = dir.join("socket");
    let _ = std::fs::remove_file(&socket);
    let Ok(listener) = UnixListener::bind(&socket) else {
        let _ = std::fs::remove_dir_all(&dir);
        return 1;
    };
    let mut command = Command::new(dir.join("program"));
    command
        .env("MIDNIGHT_DRIVE", "-")
        .env("MIDNIGHT_WINDOW", window_words())
        .env_remove("LD_LIBRARY_PATH")
        .env_remove("MIDNIGHT_SESSION_IDLE_SECONDS")
        .env_remove("MIDNIGHT_SESSION_START_SECONDS")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if json {
        command.env("MIDNIGHT_FORMAT", "json");
    } else {
        command.env_remove("MIDNIGHT_FORMAT");
    }
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            serve_failure(&listener, &format!("can't run the program: {error}"));
            let _ = std::fs::remove_dir_all(&dir);
            return 1;
        }
    };
    let stdin = child.stdin.take();
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let holder = Arc::new(Holder {
        dir,
        file: file.clone(),
        json,
        state: Mutex::new(State::default()),
        changed: Condvar::new(),
        stdin: Mutex::new(stdin),
        child: Mutex::new(child),
        turn: Mutex::new(()),
    });
    if let Some(mut stdout) = stdout {
        let holder = holder.clone();
        std::thread::spawn(move || {
            let mut buf = [0u8; 65536];
            loop {
                let got = stdout.read(&mut buf).unwrap_or(0);
                let mut state = holder.state.lock().unwrap();
                if got == 0 {
                    state.out_closed = true;
                    holder.changed.notify_all();
                    break;
                }
                state.out.extend_from_slice(&buf[..got]);
                holder.changed.notify_all();
            }
            holder.finish_if_done();
        });
    }
    if let Some(stderr) = stderr {
        let holder = holder.clone();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stderr);
            loop {
                let mut line = Vec::new();
                let got = reader.read_until(b'\n', &mut line).unwrap_or(0);
                let mut state = holder.state.lock().unwrap();
                if got == 0 {
                    state.err_closed = true;
                    holder.changed.notify_all();
                    break;
                }
                if let Some((tick, _, ended)) = ready_line(&line, holder.json) {
                    state.tick = tick;
                    state.ended = ended;
                    state.readies += 1;
                    state.last_ready = Some(Instant::now());
                    state.in_command = false;
                }
                state.lines.push_back(line);
                holder.changed.notify_all();
            }
            holder.finish_if_done();
        });
    }
    {
        // Waits for the program: its status, 128 plus a signal's number.
        let holder = holder.clone();
        std::thread::spawn(move || {
            loop {
                let status: Option<ExitStatus> =
                    holder.child.lock().unwrap().try_wait().ok().flatten();
                if let Some(status) = status {
                    let code = match status.code() {
                        Some(code) => i64::from(code),
                        None => 128 + i64::from(status.signal().unwrap_or(0)),
                    };
                    let mut state = holder.state.lock().unwrap();
                    state.status = Some(code);
                    holder.changed.notify_all();
                    break;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            holder.finish_if_done();
        });
    }
    {
        // Stops a program that writes no first `ready` in time, and a
        // session idle for too long.
        let holder = holder.clone();
        let deadline = Instant::now() + Duration::from_secs(start_seconds());
        std::thread::spawn(move || {
            loop {
                std::thread::sleep(Duration::from_millis(100));
                let state = holder.state.lock().unwrap();
                if state.status.is_some() {
                    break;
                }
                if state.readies == 0 && Instant::now() >= deadline {
                    drop(state);
                    holder.state.lock().unwrap().timed_out = true;
                    let _ = holder.child.lock().unwrap().kill();
                    break;
                }
                let idle_for = state.last_ready.map(|at| at.elapsed());
                if !state.in_command && state.clients == 0 && idle_for.is_some_and(|d| d >= idle) {
                    drop(state);
                    holder.close_input();
                    break;
                }
            }
        });
    }
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let holder = holder.clone();
        holder.state.lock().unwrap().clients += 1;
        std::thread::spawn(move || {
            holder.serve(stream);
            holder.state.lock().unwrap().clients -= 1;
            holder.finish_if_done();
        });
    }
    0
}

/// Answers every client with `message` until one asks to start, then
/// returns.
fn serve_failure(listener: &UnixListener, message: &str) {
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let Ok(mut out) = stream.try_clone() else {
            continue;
        };
        let mut input = BufReader::new(stream);
        if let Ok(Some((_, hello))) = receive(&mut input) {
            let _ = send(&mut out, "hello", b"");
            let _ = send(&mut out, "error", message.as_bytes());
            if hello.ends_with(b" start") {
                return;
            }
        }
    }
}
