//! Tasks, read from their directories, and kits: which documentation files
//! the agent gets, and where.

use crate::Failure;
use crate::json::Value;
use crate::proc;
use crate::sha256::Sha256;
use crate::text::{self, basename, join};

/// A task, read from a directory.
pub struct Task {
    pub name: String,
    pub dir: String,
    pub meta: Vec<(String, String)>,
}

impl Task {
    pub fn new(dir: &str) -> Result<Task, Failure> {
        let meta = Task::parse_meta(&proc::read(&join(&[dir, "meta"]))?)?;
        Ok(Task {
            name: basename(dir).to_string(),
            dir: dir.to_string(),
            meta,
        })
    }

    pub fn parse_meta(text: &str) -> Result<Vec<(String, String)>, Failure> {
        let mut out: Vec<(String, String)> = Vec::new();
        for line in text::lines(text) {
            let line = text::chomp(line);
            if text::strip(line).is_empty() {
                continue;
            }
            let Some((key, value)) = line.split_once(':') else {
                return Err(Failure(format!("meta: no `:` in `{line}`")));
            };
            let key = text::strip(key).to_string();
            let value = text::strip(value).to_string();
            match out.iter_mut().find(|(k, _)| *k == key) {
                Some(slot) => slot.1 = value,
                None => out.push((key, value)),
            }
        }
        Ok(out)
    }

    pub fn meta(&self, key: &str) -> Option<&str> {
        self.meta
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    pub fn path(&self, parts: &[&str]) -> String {
        let mut all = vec![self.dir.as_str()];
        all.extend_from_slice(parts);
        join(&all)
    }

    pub fn read(&self, parts: &[&str]) -> Result<String, Failure> {
        proc::read(&self.path(parts))
    }

    pub fn exists(&self, parts: &[&str]) -> bool {
        proc::exists(&self.path(parts))
    }

    pub fn minutes(&self) -> u64 {
        Value::Str(self.meta("minutes").unwrap_or("15").to_string()).to_int() as u64
    }

    pub fn areas(&self) -> Vec<String> {
        text::words(self.meta("areas").unwrap_or(""))
            .into_iter()
            .map(str::to_string)
            .collect()
    }

    pub fn expected(&self) -> Result<String, Failure> {
        self.read(&["expected.out"])
    }

    pub fn expected_bytes(&self) -> Result<Vec<u8>, Failure> {
        proc::read_bytes(&self.path(&["expected.out"]))
    }

    /// The task's commands for grading, when it has them.
    pub fn drive(&self) -> Option<String> {
        self.exists(&["check.drive"])
            .then(|| self.path(&["check.drive"]))
    }

    pub fn is_repair(&self) -> bool {
        self.given_files().iter().any(|f| f == "solution.12am")
    }

    pub fn has_draws(&self) -> bool {
        self.exists(&["expected.draws"])
    }

    pub fn expected_draws(&self) -> Result<String, Failure> {
        self.read(&["expected.draws"])
    }

    /// Whether the shapes of each `draws` answer are compared as a set:
    /// `draws: any order` in meta; `draws: in order` compares them line for
    /// line.
    pub fn draws_any_order(&self) -> Result<bool, Failure> {
        match self.meta("draws") {
            Some("any order") => Ok(true),
            Some("in order") => Ok(false),
            _ => Err(Failure(format!(
                "{}/meta: `draws:` is `in order` or `any order`",
                self.name
            ))),
        }
    }

    pub fn given_files(&self) -> Vec<String> {
        if !self.exists(&["given"]) {
            return Vec::new();
        }
        let given = self.path(&["given"]);
        let mut files: Vec<String> = proc::walk(&given, false)
            .into_iter()
            .filter(|f| proc::is_file(&join(&[&given, f])))
            .collect();
        files.sort();
        files
    }

    pub fn programs(&self, kind: &str) -> Vec<String> {
        let dir = self.path(&[kind]);
        proc::children(&dir)
            .into_iter()
            .filter(|f| !f.starts_with('.') && f.ends_with(".12am"))
            .map(|f| join(&[&dir, &f]))
            .collect()
    }

    fn systems_lines(&self) -> Result<Vec<Vec<String>>, Failure> {
        if !self.exists(&["systems"]) {
            return Ok(Vec::new());
        }
        let t = self.read(&["systems"])?;
        Ok(text::lines(&t)
            .into_iter()
            .map(|l| {
                text::words(l)
                    .into_iter()
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .filter(|w| !w.is_empty())
            .collect())
    }

    /// The systems a program must declare, in the order they run, as
    /// (name, each). A last line `others after` says that any other system
    /// runs after them.
    pub fn systems(&self) -> Result<Vec<(String, bool)>, Failure> {
        let mut lines = self.systems_lines()?;
        if lines.last().is_some_and(|l| l == &["others", "after"]) {
            lines.pop();
        }
        lines
            .into_iter()
            .map(|l| {
                let each = l.get(1);
                if l.len() > 2 || each.is_some_and(|e| e != "each") {
                    return Err(Failure(format!(
                        "{}/systems: `{}` is not `NAME [each]`",
                        self.name,
                        l.join(" ")
                    )));
                }
                Ok((l[0].clone(), each.is_some()))
            })
            .collect()
    }

    pub fn others_after(&self) -> Result<bool, Failure> {
        Ok(self
            .systems_lines()?
            .last()
            .is_some_and(|l| l == &["others", "after"]))
    }

    /// Each line of `loops` as (where, place).
    pub fn loops(&self) -> Result<Vec<(String, String)>, Failure> {
        if !self.exists(&["loops"]) {
            return Ok(Vec::new());
        }
        let t = self.read(&["loops"])?;
        Ok(text::lines(&t)
            .into_iter()
            .map(text::strip)
            .filter(|l| !l.is_empty())
            .map(|line| {
                if let Some(w) = line.strip_suffix(" one core") {
                    (w.to_string(), "one core".to_string())
                } else {
                    match line.rsplit_once(' ') {
                        Some((w, p)) => (w.to_string(), p.to_string()),
                        None => (String::new(), line.to_string()),
                    }
                }
            })
            .collect())
    }

    /// A hash of what the agent is given and graded against.
    pub fn sha256(&self) -> Result<String, Failure> {
        let mut d = Sha256::new();
        for f in [
            "TASK.md",
            "meta",
            "expected.out",
            "check.drive",
            "loops",
            "systems",
        ] {
            let content = if self.exists(&[f]) {
                proc::read_bytes(&self.path(&[f]))?
            } else {
                Vec::new()
            };
            d.update(f.as_bytes());
            d.update(b"\0");
            d.update(&content);
            d.update(b"\0");
        }
        if self.has_draws() {
            d.update(b"expected.draws\0");
            d.update(&proc::read_bytes(&self.path(&["expected.draws"]))?);
            d.update(b"\0");
        }
        for f in self.given_files() {
            d.update(format!("given/{f}\0").as_bytes());
            d.update(&proc::read_bytes(&self.path(&["given", &f]))?);
            d.update(b"\0");
        }
        Ok(d.hex())
    }

    /// The tasks under root: each directory with a `meta`, sorted.
    pub fn all(root: &str) -> Result<Vec<Task>, Failure> {
        proc::children(root)
            .into_iter()
            .filter(|n| !n.starts_with('.') && proc::exists(&join(&[root, n, "meta"])))
            .map(|n| Task::new(&join(&[root, &n])))
            .collect()
    }
}

/// Kit files: each line `SOURCE DEST`, `#` lines and blank lines skipped.
pub fn kit_parse(text: &str) -> Result<Vec<(String, String)>, Failure> {
    let mut out = Vec::new();
    for line in text::lines(text) {
        let line = text::strip(line);
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let w = text::words(line);
        if w.len() != 2 {
            return Err(Failure(format!("kit: `{line}` is not `SOURCE DEST`")));
        }
        let dest = w[1];
        if dest.starts_with('/') || dest.split('/').any(|p| p == "..") {
            return Err(Failure(format!(
                "kit: `{dest}` is not a relative path without `..`"
            )));
        }
        out.push((w[0].to_string(), dest.to_string()));
    }
    Ok(out)
}

/// The kit's files as (dest, source path) for a tree whose files
/// list_files names.
pub fn kit_files(
    entries: &[(String, String)],
    list_files: &[String],
) -> Result<Vec<(String, String)>, Failure> {
    let mut out: Vec<(String, String)> = Vec::new();
    let mut set = |dest: String, source: String| match out.iter_mut().find(|(d, _)| *d == dest) {
        Some(slot) => slot.1 = source,
        None => out.push((dest, source)),
    };
    for (source, dest) in entries {
        let prefix = format!("{source}/");
        let under: Vec<&String> = list_files
            .iter()
            .filter(|f| f.starts_with(&prefix))
            .collect();
        if list_files.contains(source) {
            set(dest.clone(), source.clone());
        } else if !under.is_empty() {
            for f in under {
                set(join(&[dest, &f[prefix.len()..]]), f.clone());
            }
        } else {
            return Err(Failure(format!("kit: `{source}` is not in the tree")));
        }
    }
    Ok(out)
}

/// The title of the section before a file's first heading.
pub const BEFORE: &str = "(before the first section)";

/// Each `##` and `###` heading of a Markdown file as (line, title), after
/// a section for the lines before the first one, below line 1, when any of
/// them has text; a file with none gets one section for the whole file.
pub fn sections(text: &str) -> Vec<(usize, Option<String>)> {
    let mut fence = false;
    let mut found = Vec::new();
    let lines = text::lines(text);
    for (i, line) in lines.iter().enumerate() {
        if line.starts_with("```") {
            fence = !fence;
        }
        if fence {
            continue;
        }
        let title = line
            .strip_prefix("## ")
            .or_else(|| line.strip_prefix("### "));
        if let Some(t) = title {
            let first_line = t.split('\n').next().unwrap_or("");
            if !first_line.is_empty() {
                found.push((i + 1, Some(text::strip(first_line).to_string())));
            }
        }
    }
    let Some(first) = found.first() else {
        return vec![(1, None)];
    };
    let end = first.0.saturating_sub(1).min(lines.len());
    let intro = lines
        .get(1..end)
        .unwrap_or(&[])
        .iter()
        .any(|l| !text::strip(l).is_empty());
    if intro {
        let mut out = vec![(1, Some(BEFORE.to_string()))];
        out.extend(found);
        out
    } else {
        found
    }
}
