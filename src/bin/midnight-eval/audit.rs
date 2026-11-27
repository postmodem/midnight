//! Which paths a run's tool calls reached, and what an agent read of
//! docs/: the lines each tool result showed.

use crate::json::{Map, Value};
use crate::obj;
use crate::text::{self, basename, inside};

const PATTERN_FIRST: [&str; 6] = ["grep", "egrep", "fgrep", "rg", "sed", "awk"];
const NO_PATHS: [&str; 2] = ["echo", "printf"];
const FILE_OPTIONS: [&str; 4] = ["-f", "--file", "--exclude-from", "--ignore-file"];
const RUNS: [&str; 5] = ["--pre", "-exec", "-execdir", "-ok", "-okdir"];
const REDIRECTS: [&str; 11] = [
    "&>>", "&>", ">>", ">|", ">&", ">", "<<<", "<<", "<&", "<>", "<",
];

/// A word found in a command: the word, the directory it is taken from
/// (None when not known), and whether it is a word of a `midnight` command.
pub type Found = (String, Option<String>, bool);

/// A shell command split into simple commands at unquoted `|`, `;`, `&`,
/// `&&`, `||` and newlines, with a space put before each unquoted
/// redirection that follows a word.
pub fn segments(command: &str) -> Vec<String> {
    let chars: Vec<char> = command.chars().collect();
    let mut out = vec![String::new()];
    let mut quote: Option<char> = None;
    for (i, &ch) in chars.iter().enumerate() {
        let before = if i > 0 { chars[i - 1] } else { ' ' };
        let last = out.last_mut().expect("a segment");
        if let Some(q) = quote {
            if ch == q {
                quote = None;
            }
            last.push(ch);
        } else if ch == '\'' || ch == '"' {
            quote = Some(ch);
            last.push(ch);
        } else if ch == '|'
            || ch == ';'
            || ch == '\n'
            || (ch == '&' && before != '<' && before != '>' && chars.get(i + 1) != Some(&'>'))
        {
            out.push(String::new());
        } else if (ch == '<' || ch == '>')
            && !(text::is_space(before) || matches!(before, '<' | '>' | '&' | '0'..='9'))
        {
            last.push(' ');
            last.push(ch);
        } else {
            last.push(ch);
        }
    }
    out.iter()
        .map(|s| text::strip(s).to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// A simple command's words: as the shell splits them, or at whitespace
/// when the quotes do not match.
pub fn split(seg: &str) -> Vec<String> {
    text::shell_split(seg)
        .unwrap_or_else(|_| text::words(seg).into_iter().map(str::to_string).collect())
}

/// Calls each call that ran with the shell's directory when it began (None
/// when not known). Claude Code keeps the directory a Bash call's `cd`
/// left for the next one, unless the call's result says it reset it.
pub fn walk(calls: &[Map], root: &str, mut f: impl FnMut(&Map, Option<&str>)) {
    let mut cwd = Some(root.to_string());
    for c in calls {
        if c.get("denied").truthy() {
            continue;
        }
        f(c, cwd.as_deref());
        if c.get("tool").as_str() != Some("Bash") {
            continue;
        }
        let command = c.get("input").get("command").to_text();
        cwd = bash_words(&command, cwd.as_deref()).2;
        if let Some(reset) = reset_to(&c.get("result").to_text()) {
            cwd = Some(reset);
        }
    }
}

/// The directory of `Shell cwd was reset to DIR` in a result.
fn reset_to(result: &str) -> Option<String> {
    const MARK: &str = "Shell cwd was reset to ";
    for (i, _) in result.match_indices(MARK) {
        let rest = &result[i + MARK.len()..];
        let dir: String = rest.chars().take_while(|c| !text::is_space(*c)).collect();
        if !dir.is_empty() {
            return Some(dir);
        }
    }
    None
}

/// Where a word leads.
#[derive(Clone, Debug, PartialEq)]
pub enum Place {
    Path(String),
    /// `~+`, `~-`, `~user`: outside, wherever they are.
    Outside,
}

/// The absolute path of a word taken from dir; Outside for `~+`, `~-` and
/// `~user`; None for a relative word when dir is not known.
pub fn expand(word: &str, dir: Option<&str>) -> Option<Place> {
    if word == "~" {
        return Some(Place::Path(format!("{}/", text::home())));
    }
    if word.starts_with("~/") {
        return Some(Place::Path(text::join(&[&text::home(), &word[1..]])));
    }
    if word.starts_with('~') {
        return Some(Place::Outside);
    }
    if dir.is_none() && !word.starts_with('/') {
        return None;
    }
    text::expand_path(word, dir.unwrap_or("/")).map(Place::Path)
}

/// The directory `cd` with these words goes to from dir, or None when it
/// cannot be known.
pub fn cd(words: &[String], dir: Option<&str>) -> Option<String> {
    let target = words.get(1).map(String::as_str).unwrap_or("~");
    if target == "-" || target.contains(['$', '`']) {
        return None;
    }
    match expand(target, dir)? {
        Place::Path(p) => Some(p),
        Place::Outside => None,
    }
}

/// Each word of a shell command that may be a path; what the audit cannot
/// follow; and the directory the command leaves the shell in. `cd` alone
/// counts as `~`.
pub fn bash_words(command: &str, cwd: Option<&str>) -> (Vec<Found>, Vec<String>, Option<String>) {
    let mut dir = cwd.map(str::to_string);
    let mut found = Vec::new();
    let mut review = Vec::new();
    for seg in segments(command) {
        let words = split(&seg);
        let (paths, why) = segment_words(&words);
        let program = words
            .iter()
            .find(|w| text::assignment(w).is_none())
            .map(String::as_str)
            .unwrap_or("");
        let compiler = basename(program) == "midnight";
        found.extend(paths.into_iter().map(|w| (w, dir.clone(), compiler)));
        review.extend(why);
        if basename(words.first().map(String::as_str).unwrap_or("")) != "cd" {
            continue;
        }
        if words.get(1).is_none() {
            found.push(("~".to_string(), dir.clone(), false));
        }
        dir = cd(&words, dir.as_deref());
        if dir.is_none() {
            review.push(format!("`{seg}` goes where the audit cannot follow"));
        }
    }
    (found, review, dir)
}

fn path_like(word: &str) -> bool {
    word.starts_with('/') || word.starts_with('~') || word.contains("..")
}

fn redirect(word: &str) -> Option<&str> {
    let rest = word.trim_start_matches(|c: char| c.is_ascii_digit());
    REDIRECTS.iter().find_map(|op| rest.strip_prefix(op))
}

enum Take {
    Nothing,
    Program,
    File,
}

/// The path-like words of one simple command, and what the audit cannot
/// follow. Paths are its path-like words: assignments' values, the
/// command, redirection targets, the files options name, and arguments,
/// but not the program of grep, rg, sed and awk, nor what echo and printf
/// print. What the audit cannot follow: a word with `$` or a backquote, a
/// sed or awk program that reads, writes or runs something, and options
/// that run a command.
pub fn segment_words(words: &[String]) -> (Vec<String>, Vec<String>) {
    let mut seg: &[String] = words;
    let mut found: Vec<String> = Vec::new();
    let mut review: Vec<String> = Vec::new();
    let mut assigned: Vec<(String, String)> = Vec::new();
    while seg.len() > 1
        && let Some((name, value)) = text::assignment(&seg[0])
    {
        assigned.push((name.to_string(), value.to_string()));
        seg = &seg[1..];
    }
    if basename(seg.first().map(String::as_str).unwrap_or("")) == "midnight" {
        assigned.retain(|(name, _)| name == "MIDNIGHT_DRIVE");
    }
    found.extend(assigned.into_iter().map(|(_, v)| v));
    if let Some(first) = seg.first() {
        found.push(first.clone());
        let cmd = basename(first).to_string();
        let pattern_first = PATTERN_FIRST.contains(&cmd.as_str());
        let mut program_due = pattern_first;
        let mut take = Take::Nothing;
        for w in &seg[1..] {
            match take {
                Take::Program => {
                    if runs_something(&cmd, w) {
                        review.push(format!("{cmd} program `{w}`"));
                    }
                    take = Take::Nothing;
                    continue;
                }
                Take::File => {
                    found.push(w.clone());
                    take = Take::Nothing;
                    continue;
                }
                Take::Nothing => {}
            }
            if let Some(target) = redirect(w) {
                if !(target.is_empty() || target.starts_with('&')) {
                    found.push(target.to_string());
                }
            } else if NO_PATHS.contains(&cmd.as_str()) {
                continue;
            } else if pattern_first && ["-e", "--regexp", "--expression"].contains(&w.as_str()) {
                take = Take::Program;
                program_due = false;
            } else if FILE_OPTIONS.contains(&w.as_str()) {
                take = Take::File;
                program_due = false;
            } else if w.starts_with('-') {
                let (name, value) = match w.split_once('=') {
                    Some((n, v)) => (n, Some(v)),
                    None => (w.as_str(), None),
                };
                if RUNS.contains(&name) {
                    review.push(format!("`{w}` runs a command"));
                }
                let Some(value) = value else { continue };
                if name == "--regexp" || name == "--expression" {
                    if runs_something(&cmd, value) {
                        review.push(format!("{cmd} program `{value}`"));
                    }
                } else {
                    found.push(value.to_string());
                }
                if ["--file", "--regexp", "--expression"].contains(&name) {
                    program_due = false;
                }
            } else if program_due {
                if runs_something(&cmd, w) {
                    review.push(format!("{cmd} program `{w}`"));
                }
                program_due = false;
            } else {
                found.push(w.clone());
            }
        }
    }
    review.extend(
        found
            .iter()
            .filter(|w| w.contains(['$', '`']))
            .map(|w| format!("`{w}`")),
    );
    (found.into_iter().filter(|w| path_like(w)).collect(), review)
}

/// sed's program with each `/…/` regular expression written as `/`.
fn without_regexps(program: &str) -> String {
    let c: Vec<char> = program.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < c.len() {
        if c[i] == '/' {
            let mut k = i + 1;
            let mut closed = None;
            while k < c.len() {
                match c[k] {
                    '/' => {
                        closed = Some(k);
                        break;
                    }
                    '\\' => {
                        if k + 1 < c.len() && c[k + 1] != '\n' {
                            k += 2;
                        } else {
                            break;
                        }
                    }
                    _ => k += 1,
                }
            }
            if let Some(end) = closed {
                out.push('/');
                i = end + 1;
                continue;
            }
        }
        out.push(c[i]);
        i += 1;
    }
    out
}

/// Whether a sed or awk program may read or write a file or run a command:
/// sed's r, R, w, W and e outside its regular expressions, and awk's
/// getline, system, and output to a file or a pipe.
pub fn runs_something(cmd: &str, program: &str) -> bool {
    match cmd {
        "sed" => {
            let c: Vec<char> = without_regexps(program).chars().collect();
            (0..c.len()).any(|i| {
                matches!(c[i], 'r' | 'R' | 'w' | 'W' | 'e')
                    && (i == 0
                        || text::is_space(c[i - 1])
                        || matches!(
                            c[i - 1],
                            ';' | '{' | '}' | '$' | ',' | '!' | '/' | '0'..='9'
                        ))
                    && (i + 1 == c.len() || text::is_space(c[i + 1]) || c[i + 1] == '/')
            })
        }
        "awk" => {
            if program.contains("getline") || program.contains('|') {
                return true;
            }
            let after = |i: usize| program[i..].trim_start_matches(text::is_space);
            if program
                .match_indices("system")
                .any(|(i, w)| after(i + w.len()).starts_with('('))
            {
                return true;
            }
            program.match_indices('>').any(|(i, _)| {
                after(i + 1)
                    .chars()
                    .next()
                    .is_some_and(|c| c == '"' || c == '(' || c == '_' || c.is_ascii_alphabetic())
            })
        }
        _ => false,
    }
}

fn input_text(input: &Value, key: &str) -> Option<String> {
    let v = input.get(key);
    (!v.is_null()).then(|| v.to_text())
}

/// The paths of one tool call, each with the directory it is taken from,
/// and what the audit cannot follow.
pub fn paths(
    call: &Map,
    root: &str,
    cwd: Option<&str>,
) -> Result<(Vec<Found>, Vec<String>), String> {
    let input = call.get("input");
    if input.as_obj().is_none() {
        return Err("the call has no input".to_string());
    }
    let at_root = |p: Vec<Option<String>>| -> Vec<Found> {
        p.into_iter()
            .flatten()
            .map(|p| (p, Some(root.to_string()), false))
            .collect()
    };
    Ok(match call.get("tool").as_str() {
        Some("Bash") => {
            let (found, review, _) = bash_words(&input.get("command").to_text(), cwd);
            (found, review)
        }
        Some("Read" | "Edit" | "Write") => (at_root(vec![input_text(input, "file_path")]), vec![]),
        Some("Glob") => (
            at_root(vec![
                input_text(input, "path"),
                input_text(input, "pattern").and_then(|p| glob_base(&p)),
            ]),
            vec![],
        ),
        Some("Grep") => (
            at_root(vec![
                input_text(input, "path"),
                input_text(input, "glob").and_then(|p| glob_base(&p)),
            ]),
            vec![],
        ),
        _ => (vec![], vec![]),
    })
}

/// The part of a glob pattern before its first wildcard, to its last `/`;
/// the pattern itself when it has none; None when no `/` comes first.
pub fn glob_base(pattern: &str) -> Option<String> {
    let Some(i) = pattern.find(['*', '?', '[', '{']) else {
        return Some(pattern.to_string());
    };
    let cut = pattern[..i].rfind('/')?;
    Some(pattern[..cut + 1].to_string())
}

/// The places a run may reach: its directory, the wrapper's, Claude Code's
/// directory of spilled tool results, and its directory under /tmp.
pub struct Places<'a> {
    pub root: &'a str,
    pub wrapper: &'a str,
    pub tool_results: &'a str,
    pub scratch: Option<&'a str>,
}

/// The paths outside the allowed places that calls which ran reached
/// ("escapes"), and what the audit could not follow ("review"). An escape
/// found in a `midnight` command has "compiler" true.
pub fn check(calls: &[Map], places: &Places) -> (Vec<Map>, Vec<Map>) {
    let mut allowed = vec![places.root, places.wrapper, places.tool_results];
    if let Some(s) = places.scratch {
        allowed.push(s);
    }
    let mut escapes = Vec::new();
    let mut review = Vec::new();
    walk(calls, places.root, |c, cwd| {
        let tool = c.get("tool").clone();
        let (found, why) = match paths(c, places.root, cwd) {
            Ok(x) => x,
            Err(e) => {
                review.push(obj! { "tool" => tool, "what" => format!("the audit failed: {e}") });
                return;
            }
        };
        for w in why {
            review.push(obj! { "tool" => tool.clone(), "what" => w });
        }
        for (p, dir, compiler) in found {
            let full = match expand(&p, dir.as_deref()) {
                None => {
                    review.push(obj! {
                        "tool" => tool.clone(),
                        "what" => format!("`{p}` from a directory the audit cannot follow"),
                    });
                    continue;
                }
                Some(f) => f,
            };
            if let Place::Path(full) = &full
                && (["/dev/null", "/dev/stdout", "/dev/stderr"].contains(&full.as_str())
                    || allowed.iter().any(|a| inside(full, a)))
            {
                continue;
            }
            let mut e = obj! { "tool" => tool.clone(), "path" => p };
            if compiler {
                e.set("compiler", true);
            }
            escapes.push(e);
        }
    });
    (escapes, review)
}

/// Escapes and review once the wrapper's refusals are taken into account.
/// A refused call goes for review, and so does a path of a `midnight`
/// command that a refused call named: the compiler did not run with it.
/// Every other escape stays.
pub fn with_refusals(escapes: Vec<Map>, review: Vec<Map>, calls: &[Value]) -> (Vec<Map>, Vec<Map>) {
    let refused: Vec<&Value> = calls.iter().filter(|c| c.get("refused").truthy()).collect();
    let named = |e: &Map| {
        let path = e.get("path");
        e.get("compiler").truthy()
            && refused.iter().any(|c| {
                c.get("args").items().contains(path)
                    || c.get("caller").members().iter().any(|(_, v)| v == path)
            })
    };
    let (covered, kept): (Vec<Map>, Vec<Map>) = escapes.into_iter().partition(|e| named(e));
    let mut out = review;
    for e in covered {
        out.push(obj! {
            "tool" => e.get("tool").clone(),
            "what" => format!("`{}` in a call the wrapper refused", e.get("path").to_text()),
        });
    }
    for c in refused {
        out.push(obj! {
            "tool" => "midnight",
            "what" => format!("refused: {}", c.get("refused").to_text()),
        });
    }
    (kept, out)
}

// What was read.

const READERS: [&str; 8] = ["grep", "egrep", "fgrep", "rg", "sed", "head", "tail", "cat"];

/// The lines of one file, found by number or by text.
pub struct Index {
    lines: Vec<String>,
    at: Vec<(String, Vec<usize>)>,
}

fn rstrip(s: &str) -> &str {
    s.trim_end_matches(|c| text::is_space(c) || c == '\0')
}

/// `  NUMBER<tab or →>TEXT`: the number.
fn numbered(line: &str) -> Option<usize> {
    let t = line.trim_start_matches(text::is_space);
    let n = t.chars().take_while(|c| c.is_ascii_digit()).count();
    if n == 0 {
        return None;
    }
    let rest = &t[n..];
    if !(rest.starts_with('\t') || rest.starts_with('→')) {
        return None;
    }
    Some(t[..n].parse().unwrap_or(usize::MAX))
}

/// `FILE:NUMBER:TEXT`, `NUMBER-TEXT` and the like: the file (when given),
/// the number and the text.
fn prefixed(line: &str) -> Option<(Option<&str>, usize, &str)> {
    let number_then = |rest: &str| -> Option<(usize, usize)> {
        let n = rest.chars().take_while(|c| c.is_ascii_digit()).count();
        if n == 0 {
            return None;
        }
        match rest.as_bytes().get(n) {
            Some(b':' | b'-') => Some((rest[..n].parse().unwrap_or(usize::MAX), n + 1)),
            _ => None,
        }
    };
    for (i, _) in line.match_indices(':') {
        if i == 0 {
            continue;
        }
        if let Some((n, skip)) = number_then(&line[i + 1..]) {
            return Some((Some(&line[..i]), n, &line[i + 1 + skip..]));
        }
    }
    let (n, skip) = number_then(line)?;
    Some((None, n, &line[skip..]))
}

impl Index {
    pub fn new(content: &str) -> Index {
        let lines: Vec<String> = text::lines(content)
            .into_iter()
            .map(|l| text::chomp(l).to_string())
            .collect();
        let mut at: Vec<(String, Vec<usize>)> = Vec::new();
        for (i, l) in lines.iter().enumerate() {
            if text::strip(l).is_empty() {
                continue;
            }
            let key = rstrip(l).to_string();
            match at.iter_mut().find(|(k, _)| *k == key) {
                Some(slot) => slot.1.push(i + 1),
                None => at.push((key, vec![i + 1])),
            }
        }
        Index { lines, at }
    }

    pub fn line(&self, n: usize) -> Option<&str> {
        if n >= 1 && n <= self.lines.len() {
            Some(&self.lines[n - 1])
        } else {
            None
        }
    }

    fn numbers_of(&self, l: &str) -> &[usize] {
        self.at
            .iter()
            .find(|(k, _)| k == l)
            .map(|(_, v)| v.as_slice())
            .unwrap_or(&[])
    }

    /// The lines of the file that text shows: lines prefixed by their
    /// number (and file) where the number's line has that text; and lines
    /// of text found in the file, each after the one before, in runs of two
    /// or more lines in a row, one of them at least 12 characters without
    /// spaces.
    pub fn shown(&self, shown: &str, dest: &str) -> Vec<usize> {
        let mut found = Vec::new();
        let mut runs: Vec<Vec<usize>> = vec![Vec::new()];
        let mut cursor = 0;
        for raw in text::lines(shown) {
            let l = text::chomp(raw);
            if let Some((file, n, rest)) = prefixed(l)
                && file.is_none_or(|f| f.ends_with(dest))
                && self.line(n).map(rstrip) == Some(rstrip(rest))
            {
                found.push(n);
                continue;
            }
            if text::strip(l).is_empty() {
                continue;
            }
            let Some(&n) = self.numbers_of(rstrip(l)).iter().find(|&&x| x > cursor) else {
                runs.push(Vec::new());
                continue;
            };
            let last = runs.last().expect("a run");
            if !(last.is_empty() || self.next_line(*last.last().expect("a line"), n)) {
                runs.push(Vec::new());
            }
            runs.last_mut().expect("a run").push(n);
            cursor = n;
        }
        for run in runs {
            if run.len() < 2
                || !run.iter().any(|&n| {
                    self.lines[n - 1]
                        .chars()
                        .filter(|c| !text::is_space(*c))
                        .count()
                        >= 12
                })
            {
                continue;
            }
            found.extend(run);
        }
        found
    }

    /// Whether line b follows line a with only blank lines between.
    fn next_line(&self, a: usize, b: usize) -> bool {
        b > a && (a + 1..b).all(|k| text::strip(&self.lines[k - 1]).is_empty())
    }
}

/// For the files of docs/ (dest and text) whose lines the results of calls
/// that ran showed, those line numbers, sorted.
pub fn lines_read(
    calls: &[Map],
    root: &str,
    docs: &[(String, String)],
) -> Vec<(String, Vec<usize>)> {
    let index: Vec<(String, Index)> = docs
        .iter()
        .map(|(d, t)| (d.clone(), Index::new(t)))
        .collect();
    let dests: Vec<String> = docs.iter().map(|(d, _)| d.clone()).collect();
    let find = |d: &str| &index.iter().find(|(k, _)| k == d).expect("a doc").1;
    let mut seen: Vec<(String, Vec<usize>)> = Vec::new();
    fn slot<'a>(seen: &'a mut Vec<(String, Vec<usize>)>, d: &str) -> &'a mut Vec<usize> {
        if let Some(i) = seen.iter().position(|(k, _)| k == d) {
            &mut seen[i].1
        } else {
            seen.push((d.to_string(), Vec::new()));
            &mut seen.last_mut().expect("a slot").1
        }
    }
    walk(calls, root, |c, cwd| {
        let result = c.get("result").to_text();
        if result.is_empty() {
            return;
        }
        let found = targets(c, root, cwd, &dests);
        if found.is_empty() {
            return;
        }
        if c.get("tool").as_str() == Some("Read") {
            let dest = &found[0];
            let idx = find(dest);
            for l in text::lines(&result) {
                if let Some(n) = numbered(text::chomp(l))
                    && idx.line(n).is_some()
                {
                    slot(&mut seen, dest).push(n);
                }
            }
        } else {
            for dest in &found {
                let more = find(dest).shown(&result, dest);
                slot(&mut seen, dest).extend(more);
            }
        }
    });
    seen.into_iter()
        .map(|(d, mut v)| {
            v.sort();
            v.dedup();
            (d, v)
        })
        .filter(|(_, v)| !v.is_empty())
        .collect()
}

/// The files of docs/ a call reads; cwd is the shell's directory.
fn targets(call: &Map, root: &str, cwd: Option<&str>, dests: &[String]) -> Vec<String> {
    let input = call.get("input");
    match call.get("tool").as_str() {
        Some("Read") => match dest_of(&input.get("file_path").to_text(), root) {
            Some(d) if dests.contains(&d) => vec![d],
            _ => vec![],
        },
        Some("Grep") => {
            if input.get("output_mode").as_str() != Some("content") {
                return vec![];
            }
            let path = input_text(input, "path").unwrap_or_else(|| ".".to_string());
            let glob = input_text(input, "glob");
            under(&path, root, dests)
                .into_iter()
                .filter(|d| match &glob {
                    None => true,
                    Some(g) => text::fnmatch(g, basename(d), false) || text::fnmatch(g, d, true),
                })
                .collect()
        }
        Some("Bash") => shell_targets(&input.get("command").to_text(), cwd, root, dests),
        _ => vec![],
    }
}

/// The files of docs/ the reading commands of a shell command name, taken
/// from cwd, with `cd` followed. A directory stands for the files under it
/// only for grep and rg.
fn shell_targets(command: &str, cwd: Option<&str>, root: &str, dests: &[String]) -> Vec<String> {
    let mut dir = cwd.map(str::to_string);
    let mut found: Vec<String> = Vec::new();
    for seg in segments(command) {
        let words = split(&seg);
        let cmd = basename(words.first().map(String::as_str).unwrap_or(""));
        if cmd == "cd" {
            dir = cd(&words, dir.as_deref());
        } else if let Some(d) = &dir
            && READERS.contains(&cmd)
        {
            let recursive = ["grep", "egrep", "fgrep", "rg"].contains(&cmd);
            for w in &words[1..] {
                for m in matching(w, d, root, dests, recursive) {
                    if !found.contains(&m) {
                        found.push(m);
                    }
                }
            }
        }
    }
    found
}

/// The docs/ path of an absolute or relative path, or None.
fn dest_of(path: &str, root: &str) -> Option<String> {
    let full = text::expand_path(path, root)?;
    let docs = text::join(&[root, "docs"]);
    full.strip_prefix(&format!("{docs}/")).map(str::to_string)
}

fn under(path: &str, root: &str, dests: &[String]) -> Vec<String> {
    let Some(full) = text::expand_path(path, root) else {
        return vec![];
    };
    let docs = text::join(&[root, "docs"]);
    if full == root || full == docs {
        return dests.to_vec();
    }
    let Some(prefix) = dest_of(&full, root) else {
        return vec![];
    };
    dests
        .iter()
        .filter(|d| **d == prefix || d.starts_with(&format!("{prefix}/")))
        .cloned()
        .collect()
}

/// The files of docs/ a shell word, taken from dir, names, globs included.
fn matching(word: &str, dir: &str, root: &str, dests: &[String], recursive: bool) -> Vec<String> {
    if word.starts_with('-') {
        return vec![];
    }
    let Some(Place::Path(full)) = expand(word, Some(dir)) else {
        return vec![];
    };
    let docs = text::join(&[root, "docs"]);
    if full == docs || full == root {
        return if recursive { dests.to_vec() } else { vec![] };
    }
    let Some(rel) = full.strip_prefix(&format!("{docs}/")) else {
        return vec![];
    };
    if dests.iter().any(|d| d.starts_with(&format!("{rel}/"))) {
        return if recursive {
            under(&full, root, dests)
        } else {
            vec![]
        };
    }
    dests
        .iter()
        .filter(|d| text::fnmatch(rel, d, true))
        .cloned()
        .collect()
}

/// Sorted numbers as runs: [first, last] each.
pub fn ranges(numbers: &[usize]) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();
    let mut i = 0;
    while i < numbers.len() {
        let start = numbers[i];
        let mut end = start;
        while i + 1 < numbers.len() && numbers[i + 1] == end + 1 {
            i += 1;
            end = numbers[i];
        }
        out.push(Value::Arr(vec![
            Value::Int(start as i64),
            Value::Int(end as i64),
        ]));
        i += 1;
    }
    out
}
