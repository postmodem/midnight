//! The answers of the command `draws`, read and compared.

use std::collections::HashMap;

use crate::json::Map;
use crate::obj;
use crate::text;

/// What a driven program puts before each answer on standard error.
pub const SAID: &str = "midnight: drive: ";

/// The count of numbers of each shape, by its statement's name.
fn numbers_of(kind: &str) -> Option<usize> {
    match kind {
        "rect" => Some(4),
        "circle" => Some(3),
        "line" => Some(5),
        "triangle" => Some(6),
        _ => None,
    }
}

/// What a line of a `draws` answer draws; a line that is no draw is text.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Shape {
    Draw {
        kind: String,
        numbers: Vec<u32>,
        color: [u32; 4],
    },
    Text(String),
}

impl Shape {
    fn kind(&self) -> &str {
        match self {
            Shape::Draw { kind, .. } => kind,
            Shape::Text(_) => "text",
        }
    }
}

/// One answer: its frame (tick, size, background bytes), its shapes, the
/// shapes' lines as printed, and its first two lines.
#[derive(Clone, Debug)]
pub struct Answer {
    pub frame: (i64, String, Option<[u32; 4]>),
    pub shapes: Vec<Shape>,
    pub lines: Vec<String>,
    pub header: Vec<String>,
}

/// The lines of every `draws` answer in a program's standard error, with
/// `midnight: drive: ` removed: each answer's first line and the lines
/// after it that go on with two spaces.
pub fn answers_text(stderr: &str) -> String {
    let mut out = String::new();
    let mut inside = false;
    let first = format!("{SAID}draws: ");
    let more = format!("{SAID}  ");
    for line in text::lines(stderr) {
        let line = text::chomp(line);
        if line.starts_with(&first) {
            inside = true;
        } else if !(inside && line.starts_with(&more)) {
            inside = false;
            continue;
        }
        out.push_str(&line[SAID.len()..]);
        out.push('\n');
    }
    out
}

/// A 32-bit float's bits from the text a draw prints it as; None when the
/// text is no number.
pub fn bits(text: &str) -> Option<u32> {
    let t = text::strip(text);
    let x = match t {
        "NaN" => f64::NAN,
        "Infinity" => f64::INFINITY,
        "-Infinity" => f64::NEG_INFINITY,
        _ => text::ruby_float(t)?,
    };
    if x.is_nan() {
        return Some(0x7fc0_0000);
    }
    Some((x as f32).to_bits())
}

fn byte_number(s: &str) -> Option<u32> {
    if s.is_empty() || !s.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

/// The four numbers of `Color(a, b, c, d)`.
fn color(s: &str) -> Option<[u32; 4]> {
    let inner = s.strip_prefix("Color(")?.strip_suffix(')')?;
    let parts: Vec<&str> = inner.split(", ").collect();
    if parts.len() != 4 {
        return None;
    }
    let mut out = [0; 4];
    for (o, p) in out.iter_mut().zip(parts) {
        *o = byte_number(p)?;
    }
    Some(out)
}

/// What a line of a `draws` answer draws.
pub fn shape(line: &str) -> Shape {
    let t = text::strip(line);
    let text_shape = || Shape::Text(t.to_string());
    let Some(rest) = t.strip_prefix("Draw.") else {
        return text_shape();
    };
    let Some(open) = rest.find('(') else {
        return text_shape();
    };
    let kind = &rest[..open];
    let Some(want) = numbers_of(kind) else {
        return text_shape();
    };
    let Some(args) = rest[open + 1..].strip_suffix(')') else {
        return text_shape();
    };
    let Some(at) = args.rfind("Color(") else {
        return text_shape();
    };
    let Some(c) = color(&args[at..]) else {
        return text_shape();
    };
    let prefix = &args[..at];
    if !prefix.is_empty() && !prefix.ends_with(", ") {
        return text_shape();
    }
    let items: Vec<&str> = if prefix.is_empty() {
        Vec::new()
    } else {
        prefix[..prefix.len() - 2].split(", ").collect()
    };
    if items.iter().any(|i| i.is_empty() || i.contains(',')) {
        return text_shape();
    }
    let numbers: Option<Vec<u32>> = items.iter().map(|n| bits(n)).collect();
    match numbers {
        Some(numbers) if numbers.len() == want => Shape::Draw {
            kind: kind.to_string(),
            numbers,
            color: c,
        },
        _ => text_shape(),
    }
}

/// `draws: N tick(s), SIZE, N draw(s)`: the tick and the size.
fn header(line: &str) -> Option<(i64, String)> {
    let rest = line.strip_prefix("draws: ")?;
    let n = rest.chars().take_while(|c| c.is_ascii_digit()).count();
    if n == 0 {
        return None;
    }
    let tick: i64 = rest[..n].parse().ok()?;
    let rest = &rest[n..];
    let rest = rest
        .strip_prefix(" ticks, ")
        .or_else(|| rest.strip_prefix(" tick, "))?;
    let comma = rest.find(", ")?;
    let size = &rest[..comma];
    if size.is_empty() || size.chars().any(text::is_space) {
        return None;
    }
    let rest = &rest[comma + 2..];
    let n = rest.chars().take_while(|c| c.is_ascii_digit()).count();
    if n == 0 {
        return None;
    }
    match &rest[n..] {
        " draws" | " draw" => Some((tick, size.to_string())),
        _ => None,
    }
}

/// Each answer of a text of `draws` answers.
pub fn parse(text: &str) -> Vec<Answer> {
    let mut answers: Vec<Answer> = Vec::new();
    for line in text::lines(text) {
        let line = text::chomp(line);
        if let Some((tick, size)) = header(line) {
            answers.push(Answer {
                frame: (tick, size, None),
                shapes: Vec::new(),
                lines: Vec::new(),
                header: vec![line.to_string()],
            });
            continue;
        }
        let Some(last) = answers.last_mut() else {
            continue;
        };
        let background = text::strip(line)
            .strip_prefix("Window.background = ")
            .and_then(color);
        if last.header.len() == 1
            && let Some(b) = background
        {
            last.frame.2 = Some(b);
            last.header.push(line.to_string());
        } else {
            last.shapes.push(shape(line));
            last.lines.push(text::strip(line).to_string());
        }
    }
    answers
}

/// Where answers first differ: its answer (from 1), how (`answers`,
/// `frame`, `missing`, `extra`, `order`, `differs`, `cut`), and the lines
/// got and wanted.
#[derive(Clone, Debug, PartialEq)]
pub struct Difference {
    pub answer: Option<usize>,
    pub how: String,
    pub got: Option<String>,
    pub want: Option<String>,
}

impl Difference {
    /// As a results file records it.
    pub fn to_json(&self) -> Map {
        if self.how == "answers" || self.how == "frame" || self.how == "cut" {
            obj! {
                "answer" => self.answer,
                "how" => self.how.as_str(),
                "got" => self.got.clone(),
                "want" => self.want.clone(),
            }
        } else {
            obj! {
                "how" => self.how.as_str(),
                "got" => self.got.clone(),
                "want" => self.want.clone(),
                "answer" => self.answer,
            }
        }
    }
}

/// Where the answers got first differ from those wanted, or None when they
/// are equal. any: each answer's shapes compared as a set with counts.
pub fn difference(got: &[Answer], want: &[Answer], any: bool) -> Option<Difference> {
    for i in 0..got.len().max(want.len()) {
        let (g, w) = match (got.get(i), want.get(i)) {
            (Some(g), Some(w)) => (g, w),
            (g, w) => {
                return Some(Difference {
                    answer: Some(i + 1),
                    how: "answers".to_string(),
                    got: g.map(|a| a.header[0].clone()),
                    want: w.map(|a| a.header[0].clone()),
                });
            }
        };
        if g.frame != w.frame {
            return Some(Difference {
                answer: Some(i + 1),
                how: "frame".to_string(),
                got: Some(g.header.join(" / ")),
                want: Some(w.header.join(" / ")),
            });
        }
        let d = if any {
            set_difference(g, w)
        } else {
            list_difference(g, w)
        };
        if let Some(mut d) = d {
            d.answer = Some(i + 1);
            return Some(d);
        }
    }
    None
}

fn at(how: &str, g: &Answer, gi: Option<usize>, w: &Answer, wi: Option<usize>) -> Difference {
    Difference {
        answer: None,
        how: how.to_string(),
        got: gi.map(|i| g.lines[i].clone()),
        want: wi.map(|i| w.lines[i].clone()),
    }
}

fn list_difference(g: &Answer, w: &Answer) -> Option<Difference> {
    let gs = &g.shapes;
    let ws = &w.shapes;
    let k = (0..gs.len().max(ws.len())).find(|&j| gs.get(j) != ws.get(j))?;
    let mut sg = gs.clone();
    let mut sw = ws.clone();
    sg.sort();
    sw.sort();
    if gs.len() == ws.len() && sg == sw {
        Some(at("order", g, Some(k), w, Some(k)))
    } else if k >= gs.len() || gs.get(k..) == ws.get(k + 1..) {
        Some(at("missing", g, None, w, Some(k)))
    } else if k >= ws.len() || gs.get(k + 1..) == ws.get(k..) {
        Some(at("extra", g, Some(k), w, None))
    } else {
        Some(at("differs", g, Some(k), w, Some(k)))
    }
}

/// The indices of a's shapes that b does not have, counting repeats.
fn take_away(a: &Answer, b: &Answer) -> Vec<usize> {
    let mut left: HashMap<&Shape, usize> = HashMap::new();
    for s in &b.shapes {
        *left.entry(s).or_insert(0) += 1;
    }
    (0..a.shapes.len())
        .filter(|&i| match left.get_mut(&a.shapes[i]) {
            Some(n) if *n > 0 => {
                *n -= 1;
                false
            }
            _ => true,
        })
        .collect()
}

fn likeness(a: &Shape, b: &Shape) -> i64 {
    if a.kind() != b.kind() {
        return -1;
    }
    match (a, b) {
        (
            Shape::Draw {
                numbers: an,
                color: ac,
                ..
            },
            Shape::Draw {
                numbers: bn,
                color: bc,
                ..
            },
        ) => an.iter().zip(bn).filter(|(x, y)| x == y).count() as i64 + i64::from(ac == bc),
        _ => 1,
    }
}

fn set_difference(g: &Answer, w: &Answer) -> Option<Difference> {
    let missing = take_away(w, g);
    let extra = take_away(g, w);
    if missing.is_empty() && extra.is_empty() {
        return None;
    }
    if missing.is_empty() {
        return Some(at("extra", g, Some(extra[0]), w, None));
    }
    if extra.is_empty() {
        return Some(at("missing", g, None, w, Some(missing[0])));
    }
    let want = &w.shapes[missing[0]];
    let mut near = extra[0];
    let mut best = likeness(&g.shapes[near], want);
    for &i in &extra[1..] {
        let l = likeness(&g.shapes[i], want);
        if l > best {
            best = l;
            near = i;
        }
    }
    Some(at("differs", g, Some(near), w, Some(missing[0])))
}

/// Whether the shape of line got is the one of line want with two of its
/// numbers in each other's places.
pub fn swapped(got: Option<&str>, want: Option<&str>) -> bool {
    let g = shape(got.unwrap_or(""));
    let w = shape(want.unwrap_or(""));
    let (
        Shape::Draw {
            kind: gk,
            numbers: gn,
            color: gc,
        },
        Shape::Draw {
            kind: wk,
            numbers: wn,
            color: wc,
        },
    ) = (&g, &w)
    else {
        return false;
    };
    if gk != wk || gc != wc {
        return false;
    }
    let diff: Vec<usize> = (0..gn.len())
        .filter(|&i| wn.get(i) != Some(&gn[i]))
        .collect();
    diff.len() == 2
        && wn.get(diff[1]) == Some(&gn[diff[0]])
        && wn.get(diff[0]) == Some(&gn[diff[1]])
}

/// Whether the shape of line got is a rectangle with want's width and
/// height at want's place plus half its width and half its height.
pub fn by_center(got: Option<&str>, want: Option<&str>) -> bool {
    let g = shape(got.unwrap_or(""));
    let w = shape(want.unwrap_or(""));
    let (
        Shape::Draw {
            kind: gk,
            numbers: gn,
            ..
        },
        Shape::Draw {
            kind: wk,
            numbers: wn,
            ..
        },
    ) = (&g, &w)
    else {
        return false;
    };
    if gk != "rect" || wk != "rect" {
        return false;
    }
    let f = |b: u32| f32::from_bits(b) as f64;
    let plus_half = |at: f64, size: f64| -> u32 {
        let half = (size * 0.5) as f32 as f64;
        ((at + half) as f32).to_bits()
    };
    let (x, y, width, height) = (f(wn[0]), f(wn[1]), f(wn[2]), f(wn[3]));
    gn == &vec![plus_half(x, width), plus_half(y, height), wn[2], wn[3]]
}

/// A line for a grade's notes.
pub fn note(d: &Difference) -> String {
    if d.how == "cut" {
        return "standard error was cut at 1 MB".to_string();
    }
    let mut parts = vec![format!(
        "answer {}: {}",
        d.answer.map(|a| a.to_string()).unwrap_or_default(),
        d.how
    )];
    if let Some(g) = &d.got {
        parts.push(format!("got `{g}`"));
    }
    if let Some(w) = &d.want {
        parts.push(format!("want `{w}`"));
    }
    parts.join(", ")
}
