//! Text and path helpers: lines, whitespace, shell words, glob patterns,
//! paths, times.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/// Whitespace: space, tab, line feed, vertical tab, form feed, return.
pub fn is_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\u{b}' | '\u{c}' | '\r')
}

/// text without whitespace or NULs at either end.
pub fn strip(text: &str) -> &str {
    text.trim_matches(|c| is_space(c) || c == '\0')
}

/// The lines of text, each with its newline.
pub fn lines(text: &str) -> Vec<&str> {
    text.split_inclusive('\n').collect()
}

/// line without one newline (`\n`, `\r\n` or `\r`) at its end.
pub fn chomp(line: &str) -> &str {
    if let Some(l) = line.strip_suffix('\n') {
        l.strip_suffix('\r').unwrap_or(l)
    } else {
        line.strip_suffix('\r').unwrap_or(line)
    }
}

/// The words of text between runs of whitespace.
pub fn words(text: &str) -> Vec<&str> {
    text.split(is_space).filter(|w| !w.is_empty()).collect()
}

/// text split at runs of two or more spaces, trailing empty fields dropped.
pub fn split_wide(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut field = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == ' ' && chars.get(i + 1) == Some(&' ') {
            while i < chars.len() && chars[i] == ' ' {
                i += 1;
            }
            out.push(std::mem::take(&mut field));
        } else {
            field.push(chars[i]);
            i += 1;
        }
    }
    out.push(field);
    while out.last().is_some_and(|f| f.is_empty()) {
        out.pop();
    }
    out
}

/// The first n characters of text.
pub fn first_chars(text: &str, n: usize) -> &str {
    match text.char_indices().nth(n) {
        Some((i, _)) => &text[..i],
        None => text,
    }
}

/// Bytes as text, each invalid sequence replaced by U+FFFD.
pub fn scrub(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// Whether an ASCII letter, digit or `_`.
pub fn is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// Whether text starts with a name (`[A-Za-z_][A-Za-z_0-9]*`) and `=`: the
/// name and what follows the `=`.
pub fn assignment(text: &str) -> Option<(&str, &str)> {
    let eq = text.find('=')?;
    let name = &text[..eq];
    let mut cs = name.chars();
    let first = cs.next()?;
    if !(first.is_ascii_alphabetic() || first == '_') || !cs.all(is_word) {
        return None;
    }
    Some((name, &text[eq + 1..]))
}

/// The words of a shell command, split at whitespace, with quotes and
/// backslashes removed; an error for an unmatched quote or a NUL.
pub fn shell_split(line: &str) -> Result<Vec<String>, String> {
    let c: Vec<char> = line.chars().collect();
    let mut words = Vec::new();
    let mut field = String::new();
    let mut i = 0;
    loop {
        while i < c.len() && is_space(c[i]) {
            i += 1;
        }
        if i >= c.len() {
            break;
        }
        let ch = c[i];
        if ch != '\0' && !is_space(ch) && ch != '\\' && ch != '\'' && ch != '"' {
            while i < c.len()
                && c[i] != '\0'
                && !is_space(c[i])
                && c[i] != '\\'
                && c[i] != '\''
                && c[i] != '"'
            {
                field.push(c[i]);
                i += 1;
            }
        } else if ch == '\'' {
            let close = (i + 1..c.len()).find(|&k| c[k] == '\'' || c[k] == '\0');
            match close {
                Some(k) if c[k] == '\'' => {
                    field.extend(&c[i + 1..k]);
                    i = k + 1;
                }
                _ => return Err(format!("Unmatched quote at {i}")),
            }
        } else if ch == '"' {
            let mut k = i + 1;
            let mut body = String::new();
            let mut closed = false;
            while k < c.len() {
                match c[k] {
                    '"' => {
                        closed = true;
                        break;
                    }
                    '\0' => break,
                    '\\' => {
                        if k + 1 >= c.len() || c[k + 1] == '\0' {
                            break;
                        }
                        let n = c[k + 1];
                        if matches!(n, '$' | '`' | '"' | '\\' | '\n') {
                            body.push(n);
                        } else {
                            body.push('\\');
                            body.push(n);
                        }
                        k += 2;
                    }
                    x => {
                        body.push(x);
                        k += 1;
                    }
                }
            }
            if !closed {
                return Err(format!("Unmatched quote at {i}"));
            }
            field.push_str(&body);
            i = k + 1;
        } else if ch == '\\' {
            match c.get(i + 1) {
                Some('\0') | None => {
                    field.push('\\');
                    i += 1;
                }
                Some('\n') => {
                    field.push_str("\\\n");
                    i += 2;
                }
                Some(&n) => {
                    field.push(n);
                    i += 2;
                }
            }
        } else {
            return Err(format!("Nul character at {i}"));
        }
        if i >= c.len() {
            words.push(std::mem::take(&mut field));
            break;
        }
        if is_space(c[i]) {
            i += 1;
            words.push(std::mem::take(&mut field));
        }
    }
    Ok(words)
}

// Glob patterns.

/// The patterns `{a,b}` stands for, outermost braces first; a pattern with
/// no matched braces as it is.
pub fn brace_expand(pattern: &str) -> Vec<String> {
    let c: Vec<char> = pattern.chars().collect();
    let mut open = None;
    let mut depth = 0;
    let mut i = 0;
    while i < c.len() {
        match c[i] {
            '\\' => i += 1,
            '{' => {
                if depth == 0 {
                    open = Some(i);
                }
                depth += 1;
            }
            '}' if depth > 0 => {
                depth -= 1;
                if depth == 0 {
                    let start = open.unwrap_or(0);
                    let head: String = c[..start].iter().collect();
                    let tail: String = c[i + 1..].iter().collect();
                    let mut parts = Vec::new();
                    let mut part = String::new();
                    let mut d = 0;
                    let mut k = start + 1;
                    while k < i {
                        match c[k] {
                            '\\' if k + 1 < i => {
                                part.push('\\');
                                part.push(c[k + 1]);
                                k += 2;
                                continue;
                            }
                            '{' => d += 1,
                            '}' => d -= 1,
                            ',' if d == 0 => {
                                parts.push(std::mem::take(&mut part));
                                k += 1;
                                continue;
                            }
                            _ => {}
                        }
                        part.push(c[k]);
                        k += 1;
                    }
                    parts.push(part);
                    return parts
                        .iter()
                        .flat_map(|p| brace_expand(&format!("{head}{p}{tail}")))
                        .collect();
                }
            }
            _ => {}
        }
        i += 1;
    }
    vec![pattern.to_string()]
}

/// Whether path matches a glob pattern, with `{a,b}` alternatives. When
/// pathname, `*` and `?` match no `/`, and `**/` matches any directories.
/// A wildcard matches no `.` at the start of a name.
pub fn fnmatch(pattern: &str, path: &str, pathname: bool) -> bool {
    brace_expand(pattern).iter().any(|p| {
        let p: Vec<char> = p.chars().collect();
        let s: Vec<char> = path.chars().collect();
        if pathname {
            match_path(&p, &s)
        } else {
            match_chars(&p, &s)
        }
    })
}

fn split_slash(c: &[char]) -> Vec<&[char]> {
    c.split(|&x| x == '/').collect()
}

fn match_path(p: &[char], s: &[char]) -> bool {
    let ps = split_slash(p);
    let ss = split_slash(s);
    match_segments(&ps, &ss)
}

fn match_segments(ps: &[&[char]], ss: &[&[char]]) -> bool {
    match ps.first() {
        None => ss.is_empty(),
        Some(p) if *p == ['*', '*'] && ps.len() > 1 => {
            if match_segments(&ps[1..], ss) {
                return true;
            }
            let mut k = 0;
            while k < ss.len() && !ss[k].starts_with(&['.']) {
                if match_segments(&ps[1..], &ss[k + 1..]) {
                    return true;
                }
                k += 1;
            }
            false
        }
        Some(p) => match ss.first() {
            Some(s) => match_chars(p, s) && match_segments(&ps[1..], &ss[1..]),
            None => false,
        },
    }
}

/// One pattern against one string. A string that starts with `.` matches
/// only a pattern that starts with `.`.
fn match_chars(p: &[char], s: &[char]) -> bool {
    if s.first() == Some(&'.') {
        let first = if p.first() == Some(&'\\') {
            p.get(1)
        } else {
            p.first()
        };
        if first != Some(&'.') {
            return false;
        }
    }
    let mut pi = 0;
    let mut si = 0;
    let mut star: Option<(usize, usize)> = None;
    loop {
        if pi < p.len() {
            let ok = match p[pi] {
                '*' => {
                    while pi < p.len() && p[pi] == '*' {
                        pi += 1;
                    }
                    star = Some((pi, si));
                    continue;
                }
                '?' => si < s.len(),
                '[' => match (si < s.len()).then(|| bracket(p, pi + 1, s[si])).flatten() {
                    Some((true, next)) => {
                        pi = next - 1;
                        true
                    }
                    _ => false,
                },
                '\\' if pi + 1 < p.len() => {
                    let hit = si < s.len() && s[si] == p[pi + 1];
                    if hit {
                        pi += 1;
                    }
                    hit
                }
                c => si < s.len() && s[si] == c,
            };
            if ok {
                pi += 1;
                si += 1;
                continue;
            }
        } else if si == s.len() {
            return true;
        }
        match star {
            Some((sp, ss)) if ss < s.len() => {
                star = Some((sp, ss + 1));
                pi = sp;
                si = ss + 1;
            }
            _ => return false,
        }
    }
}

/// Whether c is in the bracket expression whose body starts at p[i], and
/// the index after its `]`; None when it has no `]`.
fn bracket(p: &[char], mut i: usize, c: char) -> Option<(bool, usize)> {
    let negate = matches!(p.get(i), Some('!' | '^'));
    if negate {
        i += 1;
    }
    let mut found = false;
    loop {
        let mut lo = *p.get(i)?;
        if lo == ']' {
            return Some((found != negate, i + 1));
        }
        if lo == '\\' {
            i += 1;
            lo = *p.get(i)?;
        }
        i += 1;
        if p.get(i) == Some(&'-') && p.get(i + 1).is_some_and(|&x| x != ']') {
            let mut hi = p[i + 1];
            i += 2;
            if hi == '\\' {
                hi = *p.get(i)?;
                i += 1;
            }
            if lo <= c && c <= hi {
                found = true;
            }
        } else if lo == c {
            found = true;
        }
    }
}

// Paths.

/// The home directory: HOME.
pub fn home() -> String {
    std::env::var("HOME").unwrap_or_default()
}

/// The absolute path path names from dir, with `.`, `..` and repeated
/// slashes taken out by their text, without looking at the file system.
/// `~` and `~/…` are under the home directory; `~user` gives None.
pub fn expand_path(path: &str, dir: &str) -> Option<String> {
    let full = if path == "~" || path.starts_with("~/") {
        format!("{}{}", home(), &path[1..])
    } else if path.starts_with('~') {
        return None;
    } else if path.starts_with('/') {
        path.to_string()
    } else {
        let base = if dir.starts_with('~') {
            expand_path(dir, "/")?
        } else {
            dir.to_string()
        };
        format!("{base}/{path}")
    };
    Some(normalize(&full))
}

/// An absolute path with `.`, `..` and empty parts taken out.
pub fn normalize(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            p => parts.push(p),
        }
    }
    format!("/{}", parts.join("/"))
}

/// The directory part of a path: `.` for none, `/` for the root.
pub fn dirname(path: &str) -> String {
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        return if path.starts_with('/') { "/" } else { "." }.to_string();
    }
    match trimmed.rfind('/') {
        Some(i) => {
            let head = trimmed[..i].trim_end_matches('/');
            if head.is_empty() {
                "/".to_string()
            } else {
                head.to_string()
            }
        }
        None => ".".to_string(),
    }
}

/// The last part of a path, trailing slashes ignored.
pub fn basename(path: &str) -> &str {
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        return if path.is_empty() { "" } else { "/" };
    }
    match trimmed.rfind('/') {
        Some(i) => &trimmed[i + 1..],
        None => trimmed,
    }
}

/// Parts joined with `/`, without doubled slashes where they meet.
pub fn join(parts: &[&str]) -> String {
    let mut out = String::new();
    for (i, p) in parts.iter().enumerate() {
        if i == 0 {
            out.push_str(p);
            continue;
        }
        let a = out.ends_with('/');
        let b = p.starts_with('/');
        if a && b {
            out.push_str(&p[1..]);
        } else if a || b {
            out.push_str(p);
        } else {
            out.push('/');
            out.push_str(p);
        }
    }
    out
}

/// A path as text.
pub fn path_text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// Whether path is dir or under it.
pub fn inside(path: &str, dir: &str) -> bool {
    if path == dir {
        return true;
    }
    if dir.ends_with('/') {
        path.starts_with(dir)
    } else {
        path.starts_with(&format!("{dir}/"))
    }
}

// Times.

/// The year, month, day, hour, minute, second and millisecond of a time,
/// in UTC.
pub fn utc_parts(t: SystemTime) -> (i64, u32, u32, u32, u32, u32, u32) {
    let d = t.duration_since(UNIX_EPOCH).unwrap_or_default();
    let secs = d.as_secs() as i64;
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400) as u32;
    let (y, m, day) = civil(days);
    (
        y,
        m,
        day,
        rem / 3600,
        rem / 60 % 60,
        rem % 60,
        d.subsec_millis(),
    )
}

/// The date of a count of days since 1970-01-01.
fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    (y, m, d)
}

/// A time as `2026-11-27T10:00:00Z`.
pub fn iso8601(t: SystemTime) -> String {
    let (y, mo, d, h, mi, s, _) = utc_parts(t);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z")
}

/// A time as `2026-11-27T10:00:00.123Z`.
pub fn iso8601_ms(t: SystemTime) -> String {
    let (y, mo, d, h, mi, s, ms) = utc_parts(t);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}.{ms:03}Z")
}
