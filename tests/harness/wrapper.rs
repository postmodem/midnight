//! The agent's `midnight`, called directly with no agent, in front of a
//! stub compiler that prints its arguments and environment.

use std::process::{Command, Stdio};

use crate::guard;
use crate::help::{Temp, script};
use crate::json::Value;
use crate::logs;
use crate::proc;
use crate::runner;
use crate::sha256;
use crate::text::{basename, inside, join};

const STUB: &str = "case \"$1\" in
  sleep) exec sleep 60 ;;
  flood)
    head -c 3000000 /dev/zero | tr '\\0' x
    head -c 3000000 /dev/zero | tr '\\0' y >&2
    ;;
  *)
    for a in \"$@\"; do printf 'arg %s\\n' \"$a\"; done
    if [ -r /proc/$$/environ ]; then tr '\\0' '\\n' < /proc/$$/environ; else env; fi
    ;;
esac
";

struct Setup {
    root: Temp,
    logs: Temp,
    other: Temp,
    _wrapper: Temp,
    midnight: String,
}

impl Setup {
    fn new() -> Setup {
        let root = Temp::new();
        let wrapper = Temp::new();
        let logs = Temp::new();
        let other = Temp::new();
        let stub = other.path("stub");
        script(&stub, STUB);
        root.write("solution.12am", "puts 1\n");
        other.write("outside.12am", "puts 2\n");
        other.write("commands.drive", "tick\n");
        let midnight = runner::write_wrapper(
            &wrapper.0,
            &root.0,
            &logs.0,
            &stub,
            env!("CARGO_BIN_EXE_midnight-eval"),
            3,
            1 << 20,
        )
        .expect("a wrapper");
        Setup {
            root,
            logs,
            other,
            _wrapper: wrapper,
            midnight,
        }
    }

    fn call_in(&self, args: &[&str], env: &[(&str, &str)], chdir: &str) -> (i32, String, String) {
        let o = output(
            Command::new(&self.midnight)
                .args(args)
                .env_clear()
                .envs(env.iter().copied())
                .current_dir(chdir),
        );
        (
            o.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&o.stdout).into_owned(),
            String::from_utf8_lossy(&o.stderr).into_owned(),
        )
    }

    fn call(&self, args: &[&str], env: &[(&str, &str)]) -> (i32, String, String) {
        self.call_in(args, env, &self.root.0)
    }

    fn log(&self) -> Vec<Value> {
        logs::read_calls(&self.logs.path("calls.jsonl")).expect("a log")
    }

    fn last(&self) -> Value {
        self.log().pop().expect("a call")
    }

    fn assert_refused(&self, args: &[&str], env: &[(&str, &str)]) {
        let (status, out, err) = self.call(args, env);
        assert_eq!(status, 2, "{args:?} {env:?}: {err}");
        assert!(out.is_empty());
        assert_eq!(err.lines().count(), 1, "{err}");
        assert!(err.starts_with("midnight: refused: "), "{err}");
        let last = self.last();
        assert!(
            last.get("refused").truthy(),
            "the call log records the refusal"
        );
        assert_eq!(last.get("status").as_i64(), Some(2));
    }

    fn other_name(&self) -> String {
        basename(&self.other.0).to_string()
    }
}

/// What a command wrote and its status, its start tried again while the
/// executable is busy.
fn output(command: &mut Command) -> std::process::Output {
    let child = proc::spawn(
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
    )
    .expect("the wrapper runs");
    child.wait_with_output().expect("the wrapper's output")
}

/// The stub's arguments and environment, from what it printed.
fn stub_env(out: &str) -> Vec<(String, String)> {
    out.lines()
        .filter(|l| !l.starts_with("arg "))
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

fn get<'a>(env: &'a [(String, String)], key: &str) -> Option<&'a str> {
    env.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
}

#[test]
fn runs_the_compiler_with_its_own_environment() {
    let s = Setup::new();
    let (status, out, _) = s.call(&["check", "solution.12am"], &[]);
    assert_eq!(status, 0);
    let env = stub_env(&out);
    let mut keys: Vec<&str> = env.iter().map(|(k, _)| k.as_str()).collect();
    keys.sort();
    assert_eq!(
        keys,
        [
            "HOME",
            "MIDNIGHT_GPU",
            "MIDNIGHT_WINDOW",
            "PATH",
            "TMPDIR",
            "XDG_RUNTIME_DIR"
        ]
    );
    assert_eq!(get(&env, "MIDNIGHT_GPU"), Some("off"));
    assert_eq!(get(&env, "MIDNIGHT_WINDOW"), Some("headless"));
    let tmp = get(&env, "TMPDIR").expect("TMPDIR");
    assert!(
        inside(tmp, &s.logs.0),
        "the compiler's TMPDIR is under the log directory"
    );
    assert!(proc::is_dir(tmp));
    assert!(proc::children(get(&env, "XDG_RUNTIME_DIR").expect("XDG_RUNTIME_DIR")).is_empty());
    let entry = s.last();
    assert_eq!(
        entry.get("args"),
        &Value::from(vec!["check", "solution.12am"])
    );
    assert_eq!(entry.get("file").as_str(), Some("solution.12am"));
    assert_eq!(
        entry.get("sha256").as_str(),
        Some(sha256::hex(b"puts 1\n").as_str())
    );
    assert_eq!(entry.get("status").as_i64(), Some(0));
    assert!(matches!(entry.get("load"), Value::Float(_)));
}

fn files_under(dir: &str) -> Vec<String> {
    let mut files: Vec<String> = proc::walk(dir, false)
        .into_iter()
        .filter(|f| proc::is_file(&join(&[dir, f])))
        .collect();
    files.sort();
    files
}

#[test]
fn keeps_the_program_at_each_check_build_and_run() {
    let s = Setup::new();
    proc::mkdir_p(&s.root.path("lib/docs")).expect("dirs");
    s.root.write("lib/util.12am", "def f = 1\n");
    s.root.write("docs/ref.12am", "puts 3\n");
    s.root.write("notes.txt", "not a program\n");
    s.call(&["check", "solution.12am"], &[]);
    let first = s.last().get("program_sha256").to_text();
    let kept = s.logs.path(&format!("programs/{first}"));
    assert_eq!(files_under(&kept), ["lib/util.12am", "solution.12am"]);
    assert_eq!(
        proc::read(&join(&[&kept, "solution.12am"])).expect("kept"),
        "puts 1\n"
    );
    s.call(&["build", "solution.12am", "-o", "x"], &[]);
    assert_eq!(s.last().get("program_sha256").to_text(), first);
    s.root.write("solution.12am", "puts 4\n");
    s.call(&["run", "solution.12am"], &[]);
    s.call(&["loops", "solution.12am"], &[]);
    let log = s.log();
    let second = log[log.len() - 2].get("program_sha256").to_text();
    let mut want = vec![first.clone(), second.clone()];
    want.sort();
    assert_eq!(proc::children(&s.logs.path("programs")), want);
    assert_eq!(
        proc::read(&s.logs.path(&format!("programs/{second}/solution.12am"))).expect("kept"),
        "puts 4\n"
    );

    let keep = s.other.path("keep");
    runner::keep_compiled(&s.logs.0, &keep).expect("copied");
    assert_eq!(proc::children(&join(&[&keep, "programs"])).len(), 2);
}

#[test]
fn drops_the_compiler_variables_and_says_so() {
    let s = Setup::new();
    for name in [
        "MIDNIGHT_CC",
        "MIDNIGHT_CFLAGS",
        "MIDNIGHT_GPU_FLAGS",
        "MIDNIGHT_WINDOW_FLAGS",
    ] {
        let (status, out, _) = s.call(&["build", "solution.12am"], &[(name, "/bin/false")]);
        assert_eq!(status, 0);
        assert_eq!(
            get(&stub_env(&out), name),
            None,
            "{name} reaches the compiler"
        );
        let last = s.last();
        assert!(last.get("dropped").items().contains(&Value::from(name)));
        assert_eq!(last.get("caller").get(name).as_str(), Some("/bin/false"));
    }
}

#[test]
fn overrides_the_gpu_and_the_window() {
    let s = Setup::new();
    let (_, out, _) = s.call(&["run", "solution.12am"], &[("MIDNIGHT_GPU", "on")]);
    assert_eq!(get(&stub_env(&out), "MIDNIGHT_GPU"), Some("off"));
    let last = s.last();
    assert_eq!(last.get("caller").get("MIDNIGHT_GPU").as_str(), Some("on"));
    assert_eq!(last.get("passed").get("MIDNIGHT_GPU").as_str(), Some("off"));

    let (_, out, _) = s.call(&["run", "solution.12am"], &[("MIDNIGHT_WINDOW", "")]);
    assert_eq!(get(&stub_env(&out), "MIDNIGHT_WINDOW"), Some("headless"));
    assert_eq!(
        s.last().get("caller").get("MIDNIGHT_WINDOW").as_str(),
        Some("")
    );

    let (_, out, _) = s.call(&["run", "solution.12am"], &[("MIDNIGHT_WINDOW", "info")]);
    assert_eq!(
        get(&stub_env(&out), "MIDNIGHT_WINDOW"),
        Some("info,headless")
    );
}

#[test]
fn passes_threads_and_commands() {
    let s = Setup::new();
    s.root.write("c.drive", "tick\n");
    let (_, out, _) = s.call(
        &["run", "solution.12am"],
        &[("MIDNIGHT_THREADS", "4"), ("MIDNIGHT_DRIVE", "c.drive")],
    );
    let env = stub_env(&out);
    assert_eq!(get(&env, "MIDNIGHT_THREADS"), Some("4"));
    assert_eq!(get(&env, "MIDNIGHT_DRIVE"), Some("c.drive"));
}

#[test]
fn passes_the_format() {
    let s = Setup::new();
    let (_, out, _) = s.call(
        &["run", "solution.12am", "--format", "json"],
        &[("MIDNIGHT_FORMAT", "json")],
    );
    assert_eq!(get(&stub_env(&out), "MIDNIGHT_FORMAT"), Some("json"));
    let last = s.last();
    assert!(
        !last
            .get("dropped")
            .items()
            .contains(&Value::from("MIDNIGHT_FORMAT"))
    );
    assert!(
        !last.get("refused").truthy(),
        "`json` after `--format` is not a file"
    );
}

#[test]
fn refuses_a_file_outside() {
    let s = Setup::new();
    s.assert_refused(&["check", &s.other.path("outside.12am")], &[]);
    s.assert_refused(
        &["check", &format!("../{}/outside.12am", s.other_name())],
        &[],
    );
}

#[test]
fn refuses_an_output_outside() {
    let s = Setup::new();
    s.assert_refused(
        &["build", "solution.12am", "-o", &s.other.path("prog")],
        &[],
    );
    s.assert_refused(
        &[
            "build",
            "solution.12am",
            "-o",
            &format!("../{}/prog", s.other_name()),
        ],
        &[],
    );
}

#[test]
fn refuses_commands_outside() {
    let s = Setup::new();
    s.assert_refused(
        &["run", "solution.12am"],
        &[("MIDNIGHT_DRIVE", &s.other.path("commands.drive"))],
    );
    s.assert_refused(
        &["run", "solution.12am"],
        &[(
            "MIDNIGHT_DRIVE",
            &format!("../{}/commands.drive", s.other_name()),
        )],
    );
}

#[test]
fn refuses_a_link_that_leads_outside() {
    let s = Setup::new();
    std::os::unix::fs::symlink(s.other.path("outside.12am"), s.root.path("link.12am"))
        .expect("a link");
    s.assert_refused(&["check", "link.12am"], &[]);
}

#[test]
fn refuses_a_require_outside() {
    let s = Setup::new();
    s.root.write(
        "solution.12am",
        &format!("require \"../{}/outside\"\nputs 1\n", s.other_name()),
    );
    s.assert_refused(&["check", "solution.12am"], &[]);
    s.root.write(
        "solution.12am",
        &format!("require \"{}/outside\"\n", s.other.0),
    );
    s.assert_refused(&["check", "solution.12am"], &[]);
}

#[test]
fn refuses_while_any_file_requires_outside() {
    let s = Setup::new();
    s.root.write("lib/helper.12am", "require \"../../x\"\n");
    s.assert_refused(&["check", "solution.12am"], &[]);
}

#[test]
fn refuses_every_form_of_a_require_outside() {
    let s = Setup::new();
    let name = s.other_name();
    let other = s.other.0.clone();
    for line in [
        format!("require '../{name}/outside'"),
        format!("require\"../{name}/outside\""),
        format!("require '{other}/outside'"),
        format!("  require \"{other}/outside\"  # a comment"),
        format!("require \"..\\/{name}/outside\""),
        "require \"a#{1}\"".to_string(),
        format!("require '../{name}/outside' x"),
    ] {
        s.root.write("solution.12am", &format!("{line}\nputs 1\n"));
        s.assert_refused(&["check", "solution.12am"], &[]);
    }
}

#[test]
fn refuses_a_require_through_a_link() {
    let s = Setup::new();
    proc::mkdir_p(&s.other.path("sub")).expect("a directory");
    std::os::unix::fs::symlink(s.other.path("sub"), s.root.path("deep")).expect("a link");
    s.root
        .write("solution.12am", "require \"deep/../outside\"\n");
    s.assert_refused(&["check", "solution.12am"], &[]);
    s.root.write("solution.12am", "require \"deep/x\"\n");
    s.assert_refused(&["check", "solution.12am"], &[]);
}

#[test]
fn accepts_a_require_inside() {
    let s = Setup::new();
    s.root.write("helper.12am", "def two -> Int\n  2\nend\n");
    s.root.write("sub/three.12am", "require '../helper'\n");
    for line in [
        "require \"helper\"",
        "require 'helper' # two",
        "require \"sub/three\"",
        "require \"~nobody/x\"",
    ] {
        s.root
            .write("solution.12am", &format!("{line}\nputs two\n"));
        let (status, _, err) = s.call(&["check", "solution.12am"], &[]);
        assert_eq!(status, 0, "{line}: {err}");
    }
}

#[test]
fn refuses_variables_that_change_how_it_starts() {
    let s = Setup::new();
    for env in [
        vec![("MIDNIGHT_A", "1"), ("LD_PRELOAD", "")],
        vec![("MIDNIGHT_WINDOW", "headless"), ("LD_AUDIT", "")],
        vec![("LD_BIND_NOW", "1")],
        vec![("LD_LIBRARY_PATH", ".")],
        vec![("LD_LIBRARY_PATH", ":/nix/store/x-lib/lib")],
        vec![("GCONV_PATH", ".")],
    ] {
        s.assert_refused(&["check", "solution.12am"], &env);
    }
}

#[test]
fn interpreter_variables_are_dropped() {
    let s = Setup::new();
    let (status, out, _) = s.call(
        &["check", "solution.12am"],
        &[("RUBYOPT", "-r./x"), ("PYTHONPATH", ".")],
    );
    assert_eq!(status, 0);
    let env = stub_env(&out);
    assert_eq!(get(&env, "RUBYOPT"), None);
    assert_eq!(get(&env, "PYTHONPATH"), None);
    assert_eq!(
        s.last().get("dropped"),
        &Value::from(vec!["PYTHONPATH", "RUBYOPT"])
    );
}

#[test]
fn accepts_a_library_path_in_the_store() {
    let s = Setup::new();
    let (status, _, _) = s.call(
        &["check", "solution.12am"],
        &[("LD_LIBRARY_PATH", "/nix/store/x-alsa-lib/lib")],
    );
    assert_eq!(status, 0);
}

#[test]
fn resolves_from_the_callers_directory() {
    let s = Setup::new();
    proc::mkdir_p(&s.root.path("sub")).expect("a directory");
    let (status, _, _) = s.call_in(&["check", "../solution.12am"], &[], &s.root.path("sub"));
    assert_eq!(status, 0);
    assert_eq!(s.last().get("cwd").as_str(), Some("sub"));
}

#[test]
fn stops_a_call_at_the_time_limit() {
    let s = Setup::new();
    let (status, _, err) = s.call(&["sleep"], &[]);
    assert_eq!(status, 124);
    assert!(err.contains("stopped after 3 seconds"), "{err}");
    assert!(s.last().get("timeout").truthy());
}

#[test]
fn cuts_output_at_the_cap() {
    let s = Setup::new();
    let o = output(
        Command::new(&s.midnight)
            .arg("flood")
            .env_clear()
            .current_dir(&s.root.0),
    );
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(o.stdout.len(), 1 << 20);
    assert_eq!(o.stderr.len(), 1 << 20);
    let last = s.last();
    assert!(last.get("stdout_cut").truthy());
    assert!(last.get("stderr_cut").truthy());
}

#[test]
fn the_configuration_is_read_from_beside_the_wrapper() {
    let s = Setup::new();
    let config = proc::read(&join(&[&crate::text::dirname(&s.midnight), guard::CONFIG]))
        .expect("a configuration");
    let config = guard::Config::from_json(&crate::json::parse(&config).expect("JSON"))
        .expect("a configuration");
    assert_eq!(config.root, proc::realpath(&s.root.0).expect("a path"));
    assert_eq!(config.limit, 3);
    assert_eq!(config.cap, 1 << 20);
    let other = Temp::new();
    let copy = other.path("midnight");
    proc::copy(&s.midnight, &copy).expect("a copy");
    let o = output(
        Command::new(&copy)
            .arg("check")
            .env_clear()
            .current_dir(&s.root.0),
    );
    assert_eq!(o.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&o.stderr).starts_with("midnight: "));
}
