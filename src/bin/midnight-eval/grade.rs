//! Grading one program against one task, and the self-test of every
//! task's own programs.

use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use crate::draws::{self, Difference};
use crate::guard::{self, Config, Env};
use crate::json::{Map, Value};
use crate::obj;
use crate::proc;
use crate::task::Task;
use crate::text::{self, join};
use crate::tools;
use crate::{BUILD_SECONDS, CAP, Failure, LIMIT, root};

static LOCK: Mutex<()> = Mutex::new(());
static COMPILER: OnceLock<String> = OnceLock::new();

/// The compiler grading uses: MIDNIGHT_BIN, or the working tree's package.
pub fn compiler() -> Result<String, Failure> {
    if let Ok(bin) = std::env::var("MIDNIGHT_BIN")
        && !bin.is_empty()
    {
        return Ok(bin);
    }
    if let Some(c) = COMPILER.get() {
        return Ok(c.clone());
    }
    let flake = format!("{}#", root());
    let (out, ok) = proc::output_loud(&["nix", "build", &flake, "--no-link", "--print-out-paths"])?;
    if !ok {
        return Err(Failure("nix build failed".to_string()));
    }
    let path = join(&[last_line(&out), "bin", "midnight"]);
    Ok(COMPILER.get_or_init(|| path).clone())
}

fn last_line(out: &str) -> &str {
    text::strip(text::lines(out).last().copied().unwrap_or(""))
}

/// The compiler of a commit.
pub fn compiler_of(commit: &str) -> Result<String, Failure> {
    let flake = format!("git+file://{}?rev={commit}", root());
    let (out, ok) = proc::output_loud(&["nix", "build", &flake, "--no-link", "--print-out-paths"])?;
    if !ok {
        return Err(Failure(format!(
            "nix build of {} failed",
            text::first_chars(commit, 7)
        )));
    }
    Ok(join(&[last_line(&out), "bin", "midnight"]))
}

/// An environment like the wrapper's for a directory of its own.
pub fn environment(dir: &str) -> Result<Env, Failure> {
    let home = join(&[dir, ".home"]);
    let xdg = join(&[dir, ".xdg"]);
    let tmp = join(&[dir, ".tmp"]);
    for d in [&home, &xdg, &tmp] {
        proc::mkdir_p(d)?;
    }
    let config = Config {
        root: String::new(),
        log: String::new(),
        programs: String::new(),
        compiler: String::new(),
        path: tools::coreutils()?,
        home,
        xdg,
        tmp,
        limit: LIMIT,
        cap: CAP,
    };
    Ok(guard::environment(&Vec::new(), &config).0)
}

/// Copies the program in src (or src itself, as solution.12am; nothing
/// when src does not exist) into a fresh directory with the task's
/// check.drive, and grades it. With keep, writes what the program printed
/// there as graded.out (and graded.draws).
pub fn grade(task: &Task, src: &str, compiler: &str, keep: Option<&str>) -> Result<Map, Failure> {
    let _held = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = proc::mktemp()?;
    let result = (|| {
        if proc::is_dir(src) || !proc::exists(src) {
            crate::runner::keep_program(src, &dir)?;
        } else {
            proc::copy(src, &join(&[&dir, "solution.12am"]))?;
        }
        if let Some(drive) = task.drive() {
            proc::copy(&drive, &join(&[&dir, "check.drive"]))?;
        }
        let (result, stdout, draws_text) = grade_dir(task, &dir, compiler)?;
        if let Some(keep) = keep {
            proc::mkdir_p(keep)?;
            proc::write(&join(&[keep, "graded.out"]), stdout)?;
            if task.has_draws() {
                proc::write(&join(&[keep, "graded.draws"]), draws_text)?;
            }
        }
        Ok(result)
    })();
    proc::rm_rf(&dir);
    result
}

/// The grade of the program in dir, what it printed, and its `draws`
/// answers.
pub fn grade_dir(task: &Task, dir: &str, compiler: &str) -> Result<(Map, String, String), Failure> {
    let solution = join(&[dir, "solution.12am"]);
    if !proc::is_file(&solution) {
        let out = obj! {
            "verdict" => "no solution",
            "checks" => Map::new(),
            "sha256" => Value::Null,
        };
        return Ok((out, String::new(), String::new()));
    }
    let env = environment(dir)?;
    let kill = tools::coreutils()?;
    let mut checks = Map::new();
    let mut notes = Map::new();
    let sha = guard::sha256(&solution);
    let program_sha = guard::program_sha256(dir);
    let load = guard::load_average();
    let start = Instant::now();
    let mut timed_out = false;
    let real = proc::realpath(dir)?;
    let run = |env: &Env, argv: Vec<String>| {
        proc::capture(env, &argv, Path::new(dir), LIMIT, CAP, false, &kill)
    };
    let build_args: Vec<String> = vec!["build".into(), "solution.12am".into()];
    if let Some(refused) = guard::refusal(&build_args, &Vec::new(), dir, &real) {
        notes.set("build", refused);
        checks.set("build", false);
    } else {
        let b = run(
            &env,
            vec![
                compiler.into(),
                "build".into(),
                "solution.12am".into(),
                "-o".into(),
                "solution".into(),
            ],
        )?;
        timed_out |= b.timeout;
        checks.set("build", b.status == 0);
        if b.status != 0 {
            let note = if b.timeout {
                "time limit".to_string()
            } else {
                text::lines(&text::scrub(&b.err))
                    .into_iter()
                    .take(5)
                    .collect::<String>()
            };
            notes.set("build", note);
        }
    }
    let mut stdout = String::new();
    let mut draws_text = String::new();
    let mut difference: Option<Difference> = None;
    if checks.get("build").truthy() {
        let mut run_env = env.clone();
        if task.drive().is_some() {
            run_env.push(("MIDNIGHT_DRIVE".into(), "check.drive".into()));
        }
        let r = run(&run_env, vec![join(&[dir, "solution"])])?;
        timed_out |= r.timeout;
        stdout = text::scrub(&r.out);
        let expected = task.expected_bytes()?;
        let ok = r.status == 0 && r.out == expected;
        checks.set("output", ok);
        if !ok {
            let mut note = if r.timeout {
                "time limit".to_string()
            } else {
                format!("status {}", r.status)
            };
            if r.status == 0 {
                note.push_str(&format!(", {}", first_difference(&r.out, &expected)));
            }
            notes.set("output", note);
        }
        if task.has_draws() {
            draws_text = draws::answers_text(&text::scrub(&r.err));
            difference = draws_difference(task, &draws_text, r.err_cut)?;
            checks.set("draws", difference.is_none());
            if let Some(d) = &difference {
                notes.set("draws", draws::note(d));
            }
        }
    } else {
        checks.set("output", false);
        if task.has_draws() {
            checks.set("draws", false);
        }
    }
    let wanted_loops = task.loops()?;
    let wanted_systems = task.systems()?;
    let mut loops: Option<String> = None;
    let mut loops_out = String::new();
    if !(wanted_loops.is_empty() && wanted_systems.is_empty()) {
        let l = run(
            &env,
            vec![compiler.into(), "loops".into(), "solution.12am".into()],
        )?;
        timed_out |= l.timeout;
        loops_out = text::scrub(&l.out);
        if l.status == 0 {
            loops = Some(loops_out.clone());
        }
    }
    if !wanted_loops.is_empty() {
        let ok = loops.as_deref().is_some_and(|l| loops_ok(l, &wanted_loops));
        checks.set("loops", ok);
        if !ok {
            let first: String = text::lines(&loops_out).into_iter().take(10).collect();
            notes.set("loops", first);
        }
    }
    if !wanted_systems.is_empty() {
        let missing = missing_systems(dir, loops.as_deref(), &wanted_systems);
        let order = systems_out_of_order(dir, &wanted_systems, task.others_after()?);
        let ok = missing.is_empty() && order.is_none();
        checks.set("systems", ok);
        if !ok {
            let mut parts = Vec::new();
            if !missing.is_empty() {
                parts.push(format!("missing: {}", missing.join(", ")));
            }
            if let Some(o) = order {
                parts.push(o);
            }
            notes.set("systems", parts.join("; "));
        }
    }
    let verdict = if checks.iter().all(|(_, v)| v.truthy()) {
        "pass"
    } else {
        "fail"
    };
    let mut out = obj! {
        "verdict" => verdict,
        "checks" => checks,
        "notes" => notes,
        "sha256" => sha,
        "program_sha256" => program_sha,
        "timeout" => timed_out,
        "load" => load,
        "ms" => start.elapsed().as_millis() as u64,
    };
    if let Some(d) = difference {
        out.set("draws", d.to_json());
    }
    Ok((out, stdout, draws_text))
}

/// Where the `draws` answers of a run (text, with `midnight: drive: `
/// removed) first differ from the task's, or None when they agree. cut:
/// standard error was cut at the cap.
pub fn draws_difference(task: &Task, text: &str, cut: bool) -> Result<Option<Difference>, Failure> {
    if cut {
        return Ok(Some(Difference {
            answer: None,
            how: "cut".to_string(),
            got: None,
            want: None,
        }));
    }
    Ok(draws::difference(
        &draws::parse(text),
        &draws::parse(&task.expected_draws()?),
        task.draws_any_order()?,
    ))
}

fn byte_lines(b: &[u8]) -> Vec<&[u8]> {
    b.split_inclusive(|&c| c == b'\n').collect()
}

fn chomp_bytes(b: &[u8]) -> &[u8] {
    let b = b
        .strip_suffix(b"\n")
        .map_or(b, |x| x.strip_suffix(b"\r").unwrap_or(x));
    if b.ends_with(b"\r") && !b.is_empty() {
        &b[..b.len() - 1]
    } else {
        b
    }
}

/// The first line where got and want differ, each quoted, with bytes that
/// are not UTF-8 shown as U+FFFD.
pub fn first_difference(got: &[u8], want: &[u8]) -> String {
    let g = byte_lines(got);
    let w = byte_lines(want);
    let i = (0..g.len().max(w.len()))
        .find(|&k| g.get(k) != w.get(k))
        .unwrap_or(g.len().max(w.len()));
    let show = |l: Option<&&[u8]>| {
        let line = l.map_or(&[][..], |l| chomp_bytes(l));
        format!("{:?}", String::from_utf8_lossy(line))
    };
    format!(
        "line {}: got {}, want {}",
        i + 1,
        show(g.get(i)),
        show(w.get(i))
    )
}

/// The name a line declares a system of, as in `strict gpu system NAME`.
pub fn declaration(line: &str) -> Option<String> {
    let mut rest = line.trim_start_matches(text::is_space);
    for word in ["strict", "gpu"] {
        if let Some(r) = rest.strip_prefix(word)
            && r.starts_with(text::is_space)
        {
            rest = r.trim_start_matches(text::is_space);
        }
    }
    let r = rest.strip_prefix("system")?;
    if !r.starts_with(text::is_space) {
        return None;
    }
    let r = r.trim_start_matches(text::is_space);
    let c: Vec<char> = r.chars().collect();
    let first = *c.first()?;
    if !(first.is_ascii_lowercase() || first == '_') {
        return None;
    }
    let mut name: String = c.iter().take_while(|&&x| text::is_word(x)).collect();
    if let Some(&x) = c.get(name.chars().count())
        && (x == '?' || x == '!')
    {
        name.push(x);
    }
    Some(name)
}

/// The systems of the program whose root is solution.12am in dir, in the
/// order they run: source order, a required file's systems where its first
/// `require` stands.
pub fn system_order(dir: &str) -> Vec<String> {
    let mut seen = Vec::new();
    order_of(&join(&[dir, "solution.12am"]), &mut seen)
}

fn order_of(file: &str, seen: &mut Vec<String>) -> Vec<String> {
    let real = text::normalize(file);
    if seen.contains(&real) || !proc::is_file(&real) {
        return Vec::new();
    }
    seen.push(real.clone());
    let source = proc::read(&real).unwrap_or_default();
    let mut out = Vec::new();
    for line in text::lines(&source) {
        let required = guard::require_rest(line).and_then(guard::require_path);
        if let Some(path) = required {
            let next = join(&[&text::dirname(&real), &format!("{path}.12am")]);
            out.extend(order_of(&next, seen));
        } else if let Some(name) = declaration(line) {
            out.push(name);
        }
    }
    out
}

/// Why the program's systems are not in the order wanted, or None: the
/// named ones must run in their order and, with others_after, before any
/// other. A named system the program does not declare is left to
/// `missing_systems`.
pub fn systems_out_of_order(
    dir: &str,
    wanted: &[(String, bool)],
    others_after: bool,
) -> Option<String> {
    let order = system_order(dir);
    let names: Vec<&String> = wanted
        .iter()
        .map(|(n, _)| n)
        .filter(|n| order.contains(n))
        .collect();
    let mut named: Vec<&String> = Vec::new();
    for n in &order {
        if names.contains(&n) && !named.contains(&n) {
            named.push(n);
        }
    }
    if named != names {
        let list: Vec<String> = named.iter().map(|n| format!("`{n}`")).collect();
        return Some(format!("runs {} in that order", list.join(", ")));
    }
    let last = names.last()?;
    if !others_after {
        return None;
    }
    let at = order.iter().position(|n| n == *last)?;
    order[..at]
        .iter()
        .find(|n| !names.contains(n))
        .map(|early| format!("`{early}` runs before `{last}`"))
}

fn whole_word(body: &str, word: &str) -> bool {
    body.match_indices(word).any(|(i, _)| {
        let before = body[..i].chars().next_back();
        let after = body[i + word.len()..].chars().next();
        !before.is_some_and(text::is_word) && !after.is_some_and(text::is_word)
    })
}

fn is_system_row(w: &str, name: &str) -> bool {
    let mut r = w;
    if let Some(x) = r.strip_prefix("strict ") {
        r = x;
    }
    if let Some(x) = r.strip_prefix("gpu ") {
        r = x;
    }
    r.strip_prefix("system ") == Some(name)
}

/// The function name of a `def NAME` row of `midnight loops`.
fn def_name(w: &str) -> Option<&str> {
    let name = w.strip_prefix("def ")?;
    let mut cs = name.chars();
    let first = cs.next()?;
    if !(first.is_ascii_alphabetic() || first == '_') {
        return None;
    }
    let body = name.trim_end_matches(['?', '!']);
    let tail = &name[body.len()..];
    if tail.chars().count() > 1 || !body.chars().all(text::is_word) {
        return None;
    }
    Some(name)
}

/// The (name, each) of `systems` a program in dir does not meet: a system
/// it does not declare on a line of its own, or, with each, one with no
/// loop: `midnight loops` (loops, None when it failed) shows none under the
/// system, and none in a function whose name the system's body (its
/// declaration to the next `end` at the start of a line) has.
pub fn missing_systems(dir: &str, loops: Option<&str>, wanted: &[(String, bool)]) -> Vec<String> {
    let mut bodies: Vec<(String, String)> = Vec::new();
    for f in proc::glob_suffix(dir, ".12am") {
        let source = proc::read(&join(&[dir, &f])).unwrap_or_default();
        let lines = text::lines(&source);
        for (i, line) in lines.iter().enumerate() {
            let Some(name) = declaration(line) else {
                continue;
            };
            let stop = (i + 1..lines.len())
                .find(|&k| {
                    lines[k]
                        .strip_prefix("end")
                        .is_some_and(|r| !r.chars().next().is_some_and(text::is_word))
                })
                .unwrap_or(lines.len());
            let body: String = lines[i + 1..stop].concat();
            match bodies.iter_mut().find(|(n, _)| *n == name) {
                Some(slot) => slot.1 = body,
                None => bodies.push((name, body)),
            }
        }
    }
    let rows: Vec<String> = text::lines(loops.unwrap_or(""))
        .into_iter()
        .map(|l| {
            text::split_wide(text::chomp(l))
                .get(1)
                .cloned()
                .unwrap_or_default()
        })
        .collect();
    let mut functions: Vec<&str> = Vec::new();
    for w in &rows {
        if let Some(f) = def_name(w)
            && !functions.contains(&f)
        {
            functions.push(f);
        }
    }
    wanted
        .iter()
        .filter(|(name, each)| {
            let body = bodies.iter().find(|(n, _)| n == name).map(|(_, b)| b);
            let met = body.is_some_and(|body| {
                !each
                    || rows.iter().any(|w| is_system_row(w, name))
                    || functions.iter().any(|f| whole_word(body, f))
            });
            !met
        })
        .map(|(name, each)| {
            if *each {
                format!("{name} each")
            } else {
                name.clone()
            }
        })
        .collect()
}

/// Whether `midnight loops` output meets every (where, place).
pub fn loops_ok(output: &str, wanted: &[(String, String)]) -> bool {
    let rows: Vec<Vec<String>> = text::lines(output)
        .into_iter()
        .map(|l| text::split_wide(text::chomp(l)))
        .collect();
    wanted.iter().all(|(place_where, place)| {
        let mine: Vec<&Vec<String>> = rows
            .iter()
            .filter(|r| r.get(1) == Some(place_where))
            .collect();
        !mine.is_empty()
            && mine.iter().all(|r| {
                let last = r.last().map(String::as_str).unwrap_or("");
                last == place || last.starts_with(&format!("{place}:"))
            })
    })
}

// The self-test.

fn rel<'a>(task: &Task, path: &'a str) -> &'a str {
    path.strip_prefix(&format!("{}/", task.dir)).unwrap_or(path)
}

/// Whether a code block of TASK.md is a proper prefix of expected.out.
fn shown_prefix(task: &Task) -> Result<bool, Failure> {
    let doc = task.read(&["TASK.md"])?;
    let expected = task.expected()?;
    Ok(code_blocks(&doc)
        .iter()
        .any(|b| expected.starts_with(b.as_str()) && b.len() < expected.len() && !b.is_empty()))
}

/// The bodies of the fenced code blocks of a Markdown text.
pub fn code_blocks(doc: &str) -> Vec<String> {
    let at_line_start = |i: usize| i == 0 || doc.as_bytes()[i - 1] == b'\n';
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(off) = doc[from..].find("```") {
        let p = from + off;
        if !at_line_start(p) {
            from = p + 1;
            continue;
        }
        let Some(nl) = doc[p..].find('\n') else {
            break;
        };
        let start = p + nl + 1;
        let mut q = start;
        let mut close = None;
        while let Some(o) = doc[q..].find("```") {
            let c = q + o;
            if at_line_start(c) {
                close = Some(c);
                break;
            }
            q = c + 1;
        }
        match close {
            Some(c) => {
                out.push(doc[start..c].to_string());
                from = c + 3;
            }
            None => from = p + 1,
        }
    }
    out
}

fn runs_with(task: &Task, drive: &str, compiler: &str) -> Result<bool, Failure> {
    let dir = proc::mktemp()?;
    let result = (|| {
        proc::copy(
            &task.path(&["solution.12am"]),
            &join(&[&dir, "solution.12am"]),
        )?;
        proc::copy(&task.path(&["given", drive]), &join(&[&dir, "try.drive"]))?;
        let mut env = environment(&dir)?;
        env.push(("MIDNIGHT_DRIVE".into(), "try.drive".into()));
        let r = proc::capture(
            &env,
            &[compiler.into(), "run".into(), "solution.12am".into()],
            Path::new(&dir),
            LIMIT,
            CAP,
            false,
            &tools::coreutils()?,
        )?;
        Ok(r.status == 0)
    })();
    proc::rm_rf(&dir);
    result
}

/// The checks every task's programs must meet: the count of tasks checked
/// and every problem found.
pub fn self_test(
    tasks_root: &str,
    compiler: &str,
    only: &[String],
) -> Result<(usize, Vec<String>), Failure> {
    let mut problems = Vec::new();
    let mut tasks = Task::all(tasks_root)?;
    let unknown: Vec<String> = only
        .iter()
        .filter(|n| !tasks.iter().any(|t| &t.name == *n))
        .map(|n| format!("`{n}`"))
        .collect();
    if !unknown.is_empty() {
        return Err(Failure(format!("no task {}", unknown.join(", "))));
    }
    if !only.is_empty() {
        tasks.retain(|t| only.contains(&t.name));
    }
    for task in &tasks {
        let mut check = |ok: bool, what: String| {
            if !ok {
                problems.push(format!("{}: {what}", task.name));
            }
        };
        for f in ["TASK.md", "meta", "solution.12am", "expected.out"] {
            check(task.exists(&[f]), format!("no {f}"));
        }
        if !(task.exists(&["solution.12am"]) && task.exists(&["expected.out"])) {
            continue;
        }
        check(
            !task.programs("right").is_empty(),
            "no program in right/".to_string(),
        );
        check(
            !task.programs("wrong").is_empty(),
            "no program in wrong/".to_string(),
        );
        check(
            shown_prefix(task)?,
            "the output in TASK.md is not a proper prefix of expected.out".to_string(),
        );
        if task.has_draws() || task.meta("draws").is_some() {
            let ok =
                task.has_draws() && matches!(task.meta("draws"), Some("in order" | "any order"));
            check(
                ok,
                "expected.draws and `draws: in order` or `draws: any order` in meta go together"
                    .to_string(),
            );
            if !ok {
                continue;
            }
        }
        let mut right = vec![task.path(&["solution.12am"])];
        right.extend(task.programs("right"));
        for prog in &right {
            let t = Instant::now();
            let g = grade(task, prog, compiler, None)?;
            let secs = t.elapsed().as_secs_f64();
            check(
                g.get("verdict").as_str() == Some("pass"),
                format!(
                    "{} does not pass: {}",
                    rel(task, prog),
                    g.get("notes").to_text()
                ),
            );
            check(
                secs < BUILD_SECONDS,
                format!("{} took {secs:.1} s", rel(task, prog)),
            );
        }
        let mut only_fails = [false; 3];
        let names = ["loops", "systems", "draws"];
        for prog in task.programs("wrong") {
            let t = Instant::now();
            let g = grade(task, &prog, compiler, None)?;
            let secs = t.elapsed().as_secs_f64();
            check(
                g.get("verdict").as_str() != Some("pass"),
                format!("{} passes", rel(task, &prog)),
            );
            check(
                secs < BUILD_SECONDS,
                format!("{} took {secs:.1} s", rel(task, &prog)),
            );
            let checks = g.get("checks");
            for (k, name) in names.iter().enumerate() {
                let others = checks
                    .members()
                    .iter()
                    .filter(|(c, _)| c != name)
                    .all(|(_, v)| v.truthy());
                only_fails[k] |= others && checks.get(name) == &Value::Bool(false);
            }
        }
        if !task.loops()?.is_empty() {
            check(
                only_fails[0],
                "no program in wrong/ fails only `loops`".to_string(),
            );
        }
        if !task.systems()?.is_empty() {
            check(
                only_fails[1],
                "no program in wrong/ fails only `systems`".to_string(),
            );
        }
        if task.has_draws() {
            check(
                only_fails[2],
                "no program in wrong/ fails only `draws`".to_string(),
            );
        }
        if task.is_repair() {
            let g = grade(
                task,
                &task.path(&["given", "solution.12am"]),
                compiler,
                None,
            )?;
            check(
                g.get("verdict").as_str() != Some("pass"),
                "the given solution.12am passes".to_string(),
            );
        }
        for drive in task.given_files().iter().filter(|f| f.ends_with(".drive")) {
            check(
                runs_with(task, drive, compiler)?,
                format!("the reference stops with {drive}"),
            );
        }
    }
    Ok((tasks.len(), problems))
}
