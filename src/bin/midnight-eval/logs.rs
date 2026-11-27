//! What runs leave behind: the compiler's standard error as records, the
//! wrapper's call log, and Claude Code's stream-json transcript.

use crate::draws::SAID;
use crate::json::{self, Map, Value};
use crate::obj;
use crate::proc;
use crate::text;
use crate::{Failure, TOOLS};

// The compiler's standard error.

/// message with every backquoted span written as `` `…` ``.
pub fn template(message: &str) -> String {
    let mut out = String::new();
    let mut rest = message;
    while let Some(open) = rest.find('`') {
        match rest[open + 1..].find('`') {
            Some(close) => {
                out.push_str(&rest[..open]);
                out.push_str("`…`");
                rest = &rest[open + 1 + close + 1..];
            }
            None => break,
        }
    }
    out.push_str(rest);
    out
}

/// The backquoted spans of message.
pub fn spans(message: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = message;
    while let Some(open) = rest.find('`') {
        match rest[open + 1..].find('`') {
            Some(close) => {
                out.push(rest[open + 1..open + 1 + close].to_string());
                rest = &rest[open + 1 + close + 1..];
            }
            None => break,
        }
    }
    out
}

fn record(kind: &str, message: &str, file: Option<String>, line: Option<i64>) -> Map {
    obj! {
        "kind" => kind,
        "message" => message,
        "template" => template(message),
        "spans" => spans(message),
        "file" => file,
        "line" => line,
    }
}

/// The file and line of a `  --> FILE:LINE:COL` line.
fn location(line: &str) -> Option<(String, i64)> {
    let rest = line
        .trim_start_matches(text::is_space)
        .strip_prefix("--> ")?;
    let (head, col) = rest.rsplit_once(':')?;
    let (file, num) = head.rsplit_once(':')?;
    let digits = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit());
    if !digits(col) || !digits(num) {
        return None;
    }
    Some((file.to_string(), num.parse().ok()?))
}

/// The file, line and message of a `FILE:LINE: runtime error: MESSAGE` line.
fn runtime_error(line: &str) -> Option<(String, i64, String)> {
    const MARK: &str = ": runtime error: ";
    for (i, _) in line.match_indices(':') {
        let rest = &line[i + 1..];
        let n = rest.chars().take_while(|c| c.is_ascii_digit()).count();
        if n == 0 {
            continue;
        }
        if let Some(message) = rest[n..].strip_prefix(MARK) {
            return Some((
                line[..i].to_string(),
                rest[..n].parse().ok()?,
                message.to_string(),
            ));
        }
    }
    None
}

/// Each diagnostic, run-time error and `midnight:` line. Of a `draws`
/// answer, its first line is recorded as `drive: draws`, and the lines
/// after it are not.
pub fn parse_stderr(text: &str) -> Vec<Map> {
    let lines: Vec<&str> = text.split('\n').collect();
    let lines: Vec<&str> = if text.ends_with('\n') {
        lines[..lines.len() - 1].to_vec()
    } else {
        lines
    };
    let lines: Vec<&str> = lines.iter().map(|l| text::chomp(l)).collect();
    let mut out = Vec::new();
    let mut answer = false;
    let draws = format!("{SAID}draws: ");
    let more = format!("{SAID}  ");
    for (i, line) in lines.iter().enumerate() {
        if line.starts_with(&draws) {
            answer = true;
            out.push(record("midnight", "drive: draws", None, None));
            continue;
        }
        if answer && line.starts_with(&more) {
            continue;
        }
        answer = false;
        if let Some(m) = line.strip_prefix("error: ") {
            let at = lines.get(i + 1).and_then(|l| location(l));
            let (file, n) = match at {
                Some((f, n)) => (Some(f), Some(n)),
                None => (None, None),
            };
            out.push(record("error", m, file, n));
        } else if let Some((file, n, m)) = runtime_error(line) {
            out.push(record("runtime", &m, Some(file), Some(n)));
        } else if let Some(m) = line.strip_prefix("midnight: ") {
            out.push(record("midnight", m, None, None));
        }
    }
    out
}

pub fn compiler_bug(text: &str) -> bool {
    text.contains("the C compiler rejected the generated code")
}

// The wrapper's call log.

/// The records of a call log; none when it is not there.
pub fn read_calls(path: &str) -> Result<Vec<Value>, Failure> {
    if !proc::exists(path) {
        return Ok(Vec::new());
    }
    let t = proc::read(path)?;
    text::lines(&t)
        .into_iter()
        .map(|l| json::parse(l).map_err(|e| Failure(format!("{path}: {e}"))))
        .collect()
}

pub fn subcommand(call: &Value) -> String {
    call.get("args")
        .items()
        .first()
        .map(Value::to_text)
        .unwrap_or_default()
}

/// What a call ended in: ok, compile, runtime, other, timeout, refused.
pub fn outcome(call: &Value) -> &'static str {
    if call.get("refused").truthy() {
        return "refused";
    }
    if call.get("timeout").truthy() {
        return "timeout";
    }
    if call.get("status").as_i64() == Some(0) {
        return "ok";
    }
    let parsed = parse_stderr(&call.get("stderr").to_text());
    if parsed
        .iter()
        .any(|d| d.get("kind").as_str() == Some("error"))
    {
        "compile"
    } else if parsed
        .iter()
        .any(|d| d.get("kind").as_str() == Some("runtime"))
    {
        "runtime"
    } else {
        "other"
    }
}

fn count(m: &mut Map, key: &str) {
    let n = m.get(key).as_i64().unwrap_or(0);
    m.set(key, n + 1);
}

/// Counts of calls by subcommand, and of failures by kind.
pub fn summary(calls: &[Value]) -> Map {
    let mut by = Map::new();
    let mut failed = Map::new();
    for c in calls {
        count(&mut by, &subcommand(c));
        let o = outcome(c);
        if o != "ok" {
            count(&mut failed, o);
        }
    }
    let total: i64 = failed.iter().filter_map(|(_, v)| v.as_i64()).sum();
    obj! {
        "total" => calls.len(),
        "by_subcommand" => by,
        "failed" => failed,
        "failed_total" => total,
    }
}

/// Every diagnostic of the run, each marked repeated when the same
/// template and spans came up at a later call.
pub fn diagnostics(calls: &[Value]) -> Vec<Map> {
    let mut all: Vec<Map> = Vec::new();
    for (i, c) in calls.iter().enumerate() {
        for mut d in parse_stderr(&c.get("stderr").to_text()) {
            d.set("call", i);
            all.push(d);
        }
    }
    let marks: Vec<bool> = all
        .iter()
        .map(|d| {
            all.iter().any(|e| {
                e.get("call").as_i64() > d.get("call").as_i64()
                    && e.get("template") == d.get("template")
                    && e.get("spans") == d.get("spans")
            })
        })
        .collect();
    for (d, m) in all.iter_mut().zip(marks) {
        d.set("repeated", m);
    }
    all
}

const COMPILES: [&str; 3] = ["check", "build", "run"];

/// Whether the first check, build or run of solution.12am with content the
/// agent wrote exited 0; None when there was none.
pub fn first_try(calls: &[Value], given_sha: Option<&str>) -> Option<bool> {
    let first = calls.iter().find(|c| {
        !c.get("refused").truthy()
            && COMPILES.contains(&subcommand(c).as_str())
            && c.get("file").as_str() == Some("solution.12am")
            && c.get("sha256").truthy()
            && c.get("sha256").as_str() != given_sha
    })?;
    Some(first.get("status").as_i64() == Some(0))
}

/// Whether a run of solution.12am exited 0 with the graded content: the
/// whole program's, where the call log has its hash.
pub fn verified(calls: &[Value], graded_sha: Option<&str>, graded_program: Option<&str>) -> bool {
    let Some(graded_sha) = graded_sha else {
        return false;
    };
    calls.iter().any(|c| {
        let same = if c.get("program_sha256").truthy() && graded_program.is_some() {
            c.get("program_sha256").as_str() == graded_program
        } else {
            c.get("sha256").as_str() == Some(graded_sha)
        };
        !c.get("refused").truthy()
            && subcommand(c) == "run"
            && c.get("file").as_str() == Some("solution.12am")
            && same
            && c.get("status").as_i64() == Some(0)
    })
}

// Claude Code's transcript.

/// Lowercase ASCII.
fn lower(s: &str) -> String {
    s.to_ascii_lowercase()
}

fn says_usage_limit(text: &str) -> bool {
    let t = lower(text);
    [
        "usage limit",
        "hit your limit",
        "limit reached",
        "limit will reset",
        "resets at",
    ]
    .iter()
    .any(|w| t.contains(w))
}

fn says_api_error(text: &str) -> bool {
    let t = lower(text);
    if [
        "api error",
        "overloaded",
        "internal server error",
        "server_error",
        "econnreset",
        "socket hang up",
    ]
    .iter()
    .any(|w| t.contains(w))
    {
        return true;
    }
    t.match_indices("rate").any(|(i, _)| {
        let rest = &t[i + 4..];
        if rest.starts_with("limit") {
            return true;
        }
        let mut cs = rest.chars();
        match cs.next() {
            Some(c) if c != '\n' => cs.as_str().starts_with("limit"),
            _ => false,
        }
    })
}

fn denied_text(text: &str) -> bool {
    text.starts_with("Permission for this tool use was denied")
        || text.contains("--restricted confines the file tools")
}

pub struct Transcript {
    pub lines: Vec<Value>,
}

impl Transcript {
    pub fn new(text: &str) -> Transcript {
        Transcript {
            lines: text::lines(text)
                .into_iter()
                .filter_map(|l| json::parse(l).ok())
                .collect(),
        }
    }

    pub fn load(path: &str) -> Transcript {
        let text = if proc::exists(path) {
            proc::read(path).unwrap_or_default()
        } else {
            String::new()
        };
        Transcript::new(&text)
    }

    pub fn init(&self) -> Option<&Value> {
        self.lines.iter().find(|l| {
            l.get("type").as_str() == Some("system") && l.get("subtype").as_str() == Some("init")
        })
    }

    pub fn result(&self) -> Option<&Value> {
        self.lines
            .iter()
            .rev()
            .find(|l| l.get("type").as_str() == Some("result"))
    }

    pub fn version(&self) -> Value {
        self.init()
            .map(|i| i.get("claude_code_version").clone())
            .unwrap_or(Value::Null)
    }

    /// Every tool call, in order, with its result: objects with "id",
    /// "tool", "input", "result" (text or null), "error", "denied".
    pub fn tool_calls(&self) -> Vec<Map> {
        let mut results: Vec<(Value, &Value)> = Vec::new();
        for l in &self.lines {
            if l.get("type").as_str() != Some("user") {
                continue;
            }
            for c in l.get("message").get("content").items() {
                if c.get("type").as_str() == Some("tool_result") {
                    let id = c.get("tool_use_id").clone();
                    match results.iter_mut().find(|(k, _)| *k == id) {
                        Some(slot) => slot.1 = c,
                        None => results.push((id, c)),
                    }
                }
            }
        }
        let denied_ids: Vec<&Value> = self
            .result()
            .map(|r| {
                r.get("permission_denials")
                    .items()
                    .iter()
                    .map(|d| d.get("tool_use_id"))
                    .collect()
            })
            .unwrap_or_default();
        let mut calls = Vec::new();
        for l in &self.lines {
            if l.get("type").as_str() != Some("assistant") {
                continue;
            }
            for c in l.get("message").get("content").items() {
                if c.get("type").as_str() != Some("tool_use") {
                    continue;
                }
                let id = c.get("id");
                let r = results.iter().find(|(k, _)| k == id).map(|(_, r)| *r);
                let text = r.and_then(|r| content_text(r.get("content")));
                let denied = denied_ids.contains(&id)
                    || r.is_some_and(|r| {
                        r.get("is_error").truthy() && denied_text(text.as_deref().unwrap_or(""))
                    });
                let input = if c.get("input").truthy() {
                    c.get("input").clone()
                } else {
                    Value::Obj(Map::new())
                };
                calls.push(obj! {
                    "id" => id.clone(),
                    "tool" => c.get("name").clone(),
                    "input" => input,
                    "result" => text,
                    "error" => r.map(|r| r.get("is_error") == &Value::Bool(true)),
                    "denied" => denied,
                });
            }
        }
        calls
    }

    /// None, or why the run is an infrastructure error, and whether it is a
    /// usage limit. Every result with is_error is one: an agent that
    /// finishes never sets it. A usage limit is told by its text; a rate
    /// limit is an API error.
    pub fn infrastructure(&self) -> Option<(String, bool)> {
        let Some(r) = self.result() else {
            return Some(("no result line".to_string(), false));
        };
        if !r.get("is_error").truthy() {
            return None;
        }
        let text = r.get("result").to_text();
        let head = text::first_chars(&text, 200);
        if says_api_error(&text) {
            return Some((format!("API error: {head}"), false));
        }
        if says_usage_limit(&text) {
            return Some((format!("usage limit: {head}"), true));
        }
        if r.get("api_error_status").truthy() {
            return Some((format!("API error: {head}"), false));
        }
        Some((format!("error: {head}"), false))
    }

    /// Turns, duration, tokens and cost.
    pub fn usage(&self) -> Map {
        if let Some(r) = self.result() {
            let u = r.get("usage");
            return obj! {
                "turns" => r.get("num_turns").clone(),
                "duration_ms" => r.get("duration_ms").clone(),
                "input_tokens" => u.get("input_tokens").to_int(),
                "output_tokens" => u.get("output_tokens").to_int(),
                "cache_read_tokens" => u.get("cache_read_input_tokens").to_int(),
                "cache_creation_tokens" => u.get("cache_creation_input_tokens").to_int(),
                "cost_usd" => r.get("total_cost_usd").clone(),
                "partial" => false,
            };
        }
        let mut seen: Vec<(Value, Value)> = Vec::new();
        for m in &self.lines {
            if m.get("type").as_str() != Some("assistant") {
                continue;
            }
            let id = m.get("message").get("id");
            if id.truthy() {
                let u = m.get("message").get("usage");
                let u = if u.truthy() {
                    u.clone()
                } else {
                    Value::Obj(Map::new())
                };
                match seen.iter_mut().find(|(k, _)| k == id) {
                    Some(slot) => slot.1 = u,
                    None => seen.push((id.clone(), u)),
                }
            }
        }
        let mut out = obj! { "turns" => seen.len(), "duration_ms" => Value::Null };
        if !seen.is_empty() {
            let sum = |k: &str| -> i64 { seen.iter().map(|(_, u)| u.get(k).to_int()).sum() };
            out.set("input_tokens", sum("input_tokens"));
            out.set("output_tokens", sum("output_tokens"));
            out.set("cache_read_tokens", sum("cache_read_input_tokens"));
            out.set("cache_creation_tokens", sum("cache_creation_input_tokens"));
        }
        out.set("cost_usd", Value::Null);
        out.set("partial", true);
        out
    }

    pub fn models_reported(&self) -> Vec<String> {
        let mut keys: Vec<String> = self
            .result()
            .map(|r| {
                r.get("modelUsage")
                    .members()
                    .iter()
                    .map(|(k, _)| k.clone())
                    .collect()
            })
            .unwrap_or_default();
        keys.sort();
        keys
    }
}

/// The text of a tool result's content: a string as it is, the texts of
/// an array's objects on lines of their own.
pub fn content_text(content: &Value) -> Option<String> {
    match content {
        Value::Str(s) => Some(s.clone()),
        Value::Arr(a) => Some(
            a.iter()
                .filter(|c| c.as_obj().is_some() && !c.get("text").is_null())
                .map(|c| c.get("text").to_text())
                .collect::<Vec<_>>()
                .join("\n"),
        ),
        _ => None,
    }
}

/// Why the init line shows the flags no longer isolate, or None. root is
/// the run's directory, when known.
pub fn isolation_problem(init: Option<&Value>, root: Option<&str>) -> Option<String> {
    let Some(init) = init else {
        return Some("no init line".to_string());
    };
    let mut problems = Vec::new();
    if let Some(root) = root
        && init.get("cwd").as_str() != Some(root)
    {
        problems.push(format!("cwd {}", json::generate(init.get("cwd"))));
    }
    if init.get("permissionMode").as_str() != Some("default") {
        problems.push(format!(
            "permission mode {}",
            json::generate(init.get("permissionMode"))
        ));
    }
    if !tools_are_ours(init) {
        problems.push(format!("tools {}", json::generate(init.get("tools"))));
    }
    for (key, name) in [
        ("mcp_servers", "MCP servers"),
        ("skills", "skills"),
        ("slash_commands", "slash commands"),
    ] {
        if !init.get(key).items().is_empty() {
            problems.push(format!("{name} {}", json::generate(init.get(key))));
        }
    }
    let outside: Vec<Value> = init
        .get("plugins")
        .items()
        .iter()
        .filter(|p| {
            !(p.get("path").as_str() == Some("builtin")
                && p.get("source").to_text().ends_with("@builtin"))
        })
        .map(|p| p.get("name").clone())
        .collect();
    if !outside.is_empty() {
        problems.push(format!("plugins {}", json::generate(&Value::Arr(outside))));
    }
    if problems.is_empty() {
        None
    } else {
        Some(problems.join(", "))
    }
}

/// Whether the init line's tools are exactly the harness's.
pub fn tools_are_ours(init: &Value) -> bool {
    let mut got: Vec<String> = init
        .get("tools")
        .items()
        .iter()
        .map(Value::to_text)
        .collect();
    got.sort();
    let mut want: Vec<String> = TOOLS.iter().map(|t| t.to_string()).collect();
    want.sort();
    got == want
        && init
            .get("tools")
            .items()
            .iter()
            .all(|t| t.as_str().is_some())
}
