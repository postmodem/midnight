//! JSON: a value, a reader of any JSON text, and the two forms the harness
//! writes: one line with no spaces (call logs) and an indented file
//! (results). Numbers are written as Ruby writes them, so results files
//! read and written again come out the same.

use std::fmt::Write as _;

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    Arr(Vec<Value>),
    Obj(Map),
}

/// An object: its members in the order they were added.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Map(pub Vec<(String, Value)>);

static NULL: Value = Value::Null;

impl Map {
    pub fn new() -> Map {
        Map(Vec::new())
    }

    pub fn get(&self, key: &str) -> &Value {
        self.0
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v)
            .unwrap_or(&NULL)
    }

    pub fn get_mut(&mut self, key: &str) -> Option<&mut Value> {
        self.0.iter_mut().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    pub fn contains(&self, key: &str) -> bool {
        self.0.iter().any(|(k, _)| k == key)
    }

    /// Sets a member: in its place when the key is there, else at the end.
    pub fn set(&mut self, key: &str, value: impl Into<Value>) {
        let value = value.into();
        match self.get_mut(key) {
            Some(slot) => *slot = value,
            None => self.0.push((key.to_string(), value)),
        }
    }

    pub fn remove(&mut self, key: &str) -> Option<Value> {
        let i = self.0.iter().position(|(k, _)| k == key)?;
        Some(self.0.remove(i).1)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &Value)> {
        self.0.iter().map(|(k, v)| (k.as_str(), v))
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Sets each member of other, in its order.
    pub fn merge(&mut self, other: Map) {
        for (k, v) in other.0 {
            self.set(&k, v);
        }
    }

    /// The members whose keys are given, in the order given, where present.
    pub fn slice(&self, keys: &[&str]) -> Map {
        let mut out = Map::new();
        for key in keys {
            if self.contains(key) {
                out.set(key, self.get(key).clone());
            }
        }
        out
    }
}

impl Value {
    /// The member key of an object; null for anything else or a missing key.
    pub fn get(&self, key: &str) -> &Value {
        match self {
            Value::Obj(m) => m.get(key),
            _ => &NULL,
        }
    }

    pub fn get_mut(&mut self, key: &str) -> Option<&mut Value> {
        match self {
            Value::Obj(m) => m.get_mut(key),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Value::Int(n) => Some(*n),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Int(n) => Some(*n as f64),
            Value::Float(x) => Some(*x),
            _ => None,
        }
    }

    /// The items of an array; none for anything else.
    pub fn items(&self) -> &[Value] {
        match self {
            Value::Arr(a) => a,
            _ => &[],
        }
    }

    pub fn as_obj(&self) -> Option<&Map> {
        match self {
            Value::Obj(m) => Some(m),
            _ => None,
        }
    }

    pub fn as_obj_mut(&mut self) -> Option<&mut Map> {
        match self {
            Value::Obj(m) => Some(m),
            _ => None,
        }
    }

    /// The members of an object; none for anything else.
    pub fn members(&self) -> &[(String, Value)] {
        match self {
            Value::Obj(m) => &m.0,
            _ => &[],
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }

    /// Anything but null and false.
    pub fn truthy(&self) -> bool {
        !matches!(self, Value::Null | Value::Bool(false))
    }

    /// The text interpolation gives: nothing for null, a string as it is.
    pub fn to_text(&self) -> String {
        match self {
            Value::Null => String::new(),
            Value::Bool(b) => b.to_string(),
            Value::Int(n) => n.to_string(),
            Value::Float(x) => float_text(*x),
            Value::Str(s) => s.clone(),
            Value::Arr(_) | Value::Obj(_) => inspect(self),
        }
    }

    /// A whole number: 0 for null and text that starts with none.
    pub fn to_int(&self) -> i64 {
        match self {
            Value::Int(n) => *n,
            Value::Float(x) => *x as i64,
            Value::Str(s) => {
                let t = s.trim_start();
                let (sign, rest) = match t.strip_prefix('-') {
                    Some(r) => (-1, r),
                    None => (1, t.strip_prefix('+').unwrap_or(t)),
                };
                let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
                digits.parse::<i64>().map(|n| sign * n).unwrap_or(0)
            }
            Value::Bool(_) | Value::Null | Value::Arr(_) | Value::Obj(_) => 0,
        }
    }
}

impl From<&str> for Value {
    fn from(s: &str) -> Value {
        Value::Str(s.to_string())
    }
}

impl From<String> for Value {
    fn from(s: String) -> Value {
        Value::Str(s)
    }
}

impl From<&String> for Value {
    fn from(s: &String) -> Value {
        Value::Str(s.clone())
    }
}

impl From<bool> for Value {
    fn from(b: bool) -> Value {
        Value::Bool(b)
    }
}

impl From<i64> for Value {
    fn from(n: i64) -> Value {
        Value::Int(n)
    }
}

impl From<i32> for Value {
    fn from(n: i32) -> Value {
        Value::Int(n as i64)
    }
}

impl From<u64> for Value {
    fn from(n: u64) -> Value {
        Value::Int(n as i64)
    }
}

impl From<usize> for Value {
    fn from(n: usize) -> Value {
        Value::Int(n as i64)
    }
}

impl From<f64> for Value {
    fn from(x: f64) -> Value {
        Value::Float(x)
    }
}

impl From<Map> for Value {
    fn from(m: Map) -> Value {
        Value::Obj(m)
    }
}

impl<T: Into<Value>> From<Vec<T>> for Value {
    fn from(items: Vec<T>) -> Value {
        Value::Arr(items.into_iter().map(Into::into).collect())
    }
}

impl<T: Into<Value>> From<Option<T>> for Value {
    fn from(v: Option<T>) -> Value {
        v.map(Into::into).unwrap_or(Value::Null)
    }
}

/// An object from key and value pairs.
#[macro_export]
macro_rules! obj {
    ($($k:expr => $v:expr),* $(,)?) => {{
        #[allow(unused_mut)]
        let mut m = $crate::json::Map::new();
        $(m.set($k, $v);)*
        m
    }};
}

// Reading.

/// The value of a JSON text: one value with only whitespace around it.
pub fn parse(text: &str) -> Result<Value, String> {
    let mut p = Parser {
        s: text.as_bytes(),
        i: 0,
    };
    p.space();
    let v = p.value(0)?;
    p.space();
    if p.i != p.s.len() {
        return Err(format!("unexpected text at byte {}", p.i));
    }
    Ok(v)
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn space(&mut self) {
        while self.i < self.s.len() && matches!(self.s[self.i], b' ' | b'\t' | b'\n' | b'\r') {
            self.i += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.s.get(self.i).copied()
    }

    fn fail<T>(&self, what: &str) -> Result<T, String> {
        Err(format!("{what} at byte {}", self.i))
    }

    fn value(&mut self, depth: usize) -> Result<Value, String> {
        if depth > 512 {
            return self.fail("nested too deeply");
        }
        match self.peek() {
            Some(b'{') => {
                self.i += 1;
                let mut m = Map::new();
                self.space();
                if self.peek() == Some(b'}') {
                    self.i += 1;
                    return Ok(Value::Obj(m));
                }
                loop {
                    self.space();
                    if self.peek() != Some(b'"') {
                        return self.fail("expected a key");
                    }
                    let key = self.string()?;
                    self.space();
                    if self.peek() != Some(b':') {
                        return self.fail("expected `:`");
                    }
                    self.i += 1;
                    self.space();
                    let v = self.value(depth + 1)?;
                    m.set(&key, v);
                    self.space();
                    match self.peek() {
                        Some(b',') => self.i += 1,
                        Some(b'}') => {
                            self.i += 1;
                            return Ok(Value::Obj(m));
                        }
                        _ => return self.fail("expected `,` or `}`"),
                    }
                }
            }
            Some(b'[') => {
                self.i += 1;
                let mut a = Vec::new();
                self.space();
                if self.peek() == Some(b']') {
                    self.i += 1;
                    return Ok(Value::Arr(a));
                }
                loop {
                    self.space();
                    a.push(self.value(depth + 1)?);
                    self.space();
                    match self.peek() {
                        Some(b',') => self.i += 1,
                        Some(b']') => {
                            self.i += 1;
                            return Ok(Value::Arr(a));
                        }
                        _ => return self.fail("expected `,` or `]`"),
                    }
                }
            }
            Some(b'"') => Ok(Value::Str(self.string()?)),
            Some(b't') => self.word("true", Value::Bool(true)),
            Some(b'f') => self.word("false", Value::Bool(false)),
            Some(b'n') => self.word("null", Value::Null),
            Some(b'-' | b'0'..=b'9') => self.number(),
            _ => self.fail("unexpected character"),
        }
    }

    fn word(&mut self, w: &str, v: Value) -> Result<Value, String> {
        if self.s[self.i..].starts_with(w.as_bytes()) {
            self.i += w.len();
            Ok(v)
        } else {
            self.fail("unexpected word")
        }
    }

    fn digits(&mut self) -> usize {
        let start = self.i;
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.i += 1;
        }
        self.i - start
    }

    fn number(&mut self) -> Result<Value, String> {
        let start = self.i;
        if self.peek() == Some(b'-') {
            self.i += 1;
        }
        let int_start = self.i;
        let n = self.digits();
        if n == 0 || (n > 1 && self.s[int_start] == b'0') {
            return self.fail("bad number");
        }
        let mut float = false;
        if self.peek() == Some(b'.') {
            self.i += 1;
            float = true;
            if self.digits() == 0 {
                return self.fail("bad number");
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.i += 1;
            float = true;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.i += 1;
            }
            if self.digits() == 0 {
                return self.fail("bad number");
            }
        }
        let text = std::str::from_utf8(&self.s[start..self.i]).unwrap_or("0");
        if !float && let Ok(n) = text.parse::<i64>() {
            return Ok(Value::Int(n));
        }
        text.parse::<f64>()
            .map(Value::Float)
            .or_else(|_| self.fail("bad number"))
    }

    fn hex4(&mut self) -> Result<u32, String> {
        if self.i + 4 > self.s.len() {
            return self.fail("bad escape");
        }
        let text = std::str::from_utf8(&self.s[self.i..self.i + 4]).unwrap_or("");
        let n = u32::from_str_radix(text, 16).or_else(|_| self.fail("bad escape"))?;
        self.i += 4;
        Ok(n)
    }

    fn string(&mut self) -> Result<String, String> {
        self.i += 1;
        let mut out: Vec<u8> = Vec::new();
        loop {
            let Some(c) = self.peek() else {
                return self.fail("unterminated string");
            };
            self.i += 1;
            match c {
                b'"' => break,
                b'\\' => {
                    let Some(e) = self.peek() else {
                        return self.fail("bad escape");
                    };
                    self.i += 1;
                    let ch = match e {
                        b'"' => '"',
                        b'\\' => '\\',
                        b'/' => '/',
                        b'b' => '\u{8}',
                        b'f' => '\u{c}',
                        b'n' => '\n',
                        b'r' => '\r',
                        b't' => '\t',
                        b'u' => {
                            let hi = self.hex4()?;
                            if (0xD800..0xDC00).contains(&hi) {
                                if self.s[self.i..].starts_with(b"\\u") {
                                    let save = self.i;
                                    self.i += 2;
                                    let lo = self.hex4()?;
                                    if (0xDC00..0xE000).contains(&lo) {
                                        let c = 0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00);
                                        char::from_u32(c).unwrap_or('\u{FFFD}')
                                    } else {
                                        self.i = save;
                                        '\u{FFFD}'
                                    }
                                } else {
                                    '\u{FFFD}'
                                }
                            } else {
                                char::from_u32(hi).unwrap_or('\u{FFFD}')
                            }
                        }
                        _ => return self.fail("bad escape"),
                    };
                    let mut buf = [0u8; 4];
                    out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                }
                c => out.push(c),
            }
        }
        Ok(String::from_utf8_lossy(&out).into_owned())
    }
}

// Writing.

/// The value on one line with no spaces.
pub fn generate(v: &Value) -> String {
    let mut out = String::new();
    write_value(&mut out, v, None, 0);
    out
}

/// The value indented by two spaces a level, with `": "` after each key.
pub fn pretty(v: &Value) -> String {
    let mut out = String::new();
    write_value(&mut out, v, Some("  "), 0);
    out
}

fn newline(out: &mut String, indent: Option<&str>, depth: usize) {
    if let Some(unit) = indent {
        out.push('\n');
        for _ in 0..depth {
            out.push_str(unit);
        }
    }
}

fn write_value(out: &mut String, v: &Value, indent: Option<&str>, depth: usize) {
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Int(n) => {
            let _ = write!(out, "{n}");
        }
        Value::Float(x) => out.push_str(&float_text(*x)),
        Value::Str(s) => string(out, s),
        Value::Arr(a) => {
            if a.is_empty() {
                out.push_str("[]");
                return;
            }
            out.push('[');
            for (i, item) in a.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                newline(out, indent, depth + 1);
                write_value(out, item, indent, depth + 1);
            }
            newline(out, indent, depth);
            out.push(']');
        }
        Value::Obj(m) => {
            if m.is_empty() {
                out.push_str("{}");
                return;
            }
            out.push('{');
            for (i, (k, item)) in m.0.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                newline(out, indent, depth + 1);
                string(out, k);
                out.push(':');
                if indent.is_some() {
                    out.push(' ');
                }
                write_value(out, item, indent, depth + 1);
            }
            newline(out, indent, depth);
            out.push('}');
        }
    }
}

/// Appends text as a JSON string: `"`, `\` and control characters escaped.
pub fn string(out: &mut String, text: &str) {
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// The digits of a number written `D.DDDe±N`, and the place of its decimal
/// point: 0.DIGITS * 10^point.
fn digits_of(e: &str) -> (String, i32) {
    let (mantissa, exp) = e.split_once('e').unwrap_or((e, "0"));
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    let exp: i32 = exp.parse().unwrap_or(0);
    (digits, exp + 1)
}

/// The shortest digits that read back as x, and the place of the decimal
/// point: x = 0.DIGITS * 10^point. Where x lies exactly halfway between two
/// such strings, the one whose last digit is even.
fn shortest(x: f64) -> (String, i32) {
    let x = x.abs();
    let (digits, point) = digits_of(&format!("{x:e}"));
    let k = digits.len();
    let (exact, exact_point) = digits_of(&format!("{x:.800e}"));
    let last = digits.as_bytes()[k - 1] - b'0';
    if last.is_multiple_of(2) || exact_point != point || exact.len() <= k {
        return (digits, point);
    }
    let rest = &exact[k..];
    let tie = rest.starts_with('5') && rest[1..].bytes().all(|b| b == b'0');
    if !tie {
        return (digits, point);
    }
    let truncated = &exact[..k];
    let other = if truncated == digits {
        increment(truncated)
    } else {
        truncated.to_string()
    };
    let (other_digits, other_point) = if other.len() > k {
        (other[..k].to_string(), point + 1)
    } else {
        (other, point)
    };
    let text = format!("0.{other_digits}e{other_point}");
    if text.parse::<f64>().ok() == Some(x) {
        let trimmed = other_digits.trim_end_matches('0');
        let trimmed = if trimmed.is_empty() { "0" } else { trimmed };
        (trimmed.to_string(), other_point)
    } else {
        (digits, point)
    }
}

/// A string of decimal digits plus one.
fn increment(digits: &str) -> String {
    let mut d: Vec<u8> = digits.bytes().collect();
    let mut i = d.len();
    loop {
        if i == 0 {
            d.insert(0, b'1');
            break;
        }
        i -= 1;
        if d[i] == b'9' {
            d[i] = b'0';
        } else {
            d[i] += 1;
            break;
        }
    }
    String::from_utf8(d).unwrap_or_default()
}

/// A float as Ruby's `Float#to_s` writes it: `1.0`, `0.25`, `1.0e-05`,
/// `1.0e+16`.
pub fn float_text(x: f64) -> String {
    if x.is_nan() {
        return "NaN".to_string();
    }
    if x.is_infinite() {
        return if x > 0.0 { "Infinity" } else { "-Infinity" }.to_string();
    }
    let sign = if x.is_sign_negative() { "-" } else { "" };
    if x == 0.0 {
        return format!("{sign}0.0");
    }
    let (digits, point) = shortest(x);
    let n = digits.len() as i32;
    let limit = if point < n { 16 } else { 15 };
    let body = if !(-3..=limit).contains(&point) {
        let rest = if digits.len() > 1 { &digits[1..] } else { "0" };
        let e = point - 1;
        format!(
            "{}.{}e{}{:02}",
            &digits[..1],
            rest,
            if e < 0 { '-' } else { '+' },
            e.abs()
        )
    } else if point <= 0 {
        format!("0.{}{}", "0".repeat((-point) as usize), digits)
    } else if point >= n {
        format!("{}{}.0", digits, "0".repeat((point - n) as usize))
    } else {
        format!(
            "{}.{}",
            &digits[..point as usize],
            &digits[point as usize..]
        )
    };
    format!("{sign}{body}")
}

const TENS: [f64; 23] = [
    1e0, 1e1, 1e2, 1e3, 1e4, 1e5, 1e6, 1e7, 1e8, 1e9, 1e10, 1e11, 1e12, 1e13, 1e14, 1e15, 1e16,
    1e17, 1e18, 1e19, 1e20, 1e21, 1e22,
];

/// x with n digits after the point, as Ruby's `format("%.Nf")` writes it.
/// Its digits come from double arithmetic with an error bound: a remainder
/// within the bound of one half is a tie, rounded to an even last digit.
/// Where that cannot decide, the exact value is rounded.
pub fn fixed(x: f64, n: usize) -> String {
    if !x.is_finite() || x == 0.0 {
        return format!("{x:.n$}");
    }
    if x < 0.0 {
        let body = fixed(-x, n);
        return format!("-{body}");
    }
    match quick_fixed(x, n) {
        Some(s) => s,
        None => format!("{x:.n$}"),
    }
}

fn quick_fixed(x: f64, n: usize) -> Option<String> {
    let mut k = x.log10().floor() as i32;
    if k.unsigned_abs() as usize >= TENS.len() || n > 20 {
        return None;
    }
    let mut d = if k > 0 {
        x / TENS[k as usize]
    } else if k < 0 {
        x * TENS[(-k) as usize]
    } else {
        x
    };
    let mut ieps = 2.0;
    let mut ilim = n as i32 + k + 1;
    if d < 1.0 && ilim > 0 {
        ilim -= 1;
        k -= 1;
        d *= 10.0;
        ieps += 1.0;
    }
    if !(1..=14).contains(&ilim) {
        return None;
    }
    let mut eps = (ieps * d + 7.0) * 2f64.powi(-52);
    eps *= TENS[(ilim - 1) as usize];
    let mut digits: Vec<u8> = Vec::new();
    let mut i = 1;
    loop {
        let l = d as i64;
        d -= l as f64;
        digits.push(l as u8);
        if i == ilim {
            let up = if d > 0.5 + eps {
                true
            } else if d < 0.5 - eps {
                false
            } else {
                digits.last().is_some_and(|d| d % 2 == 1)
            };
            if up {
                let mut j = digits.len();
                loop {
                    if j == 0 {
                        digits.insert(0, 1);
                        k += 1;
                        break;
                    }
                    j -= 1;
                    digits[j] += 1;
                    if digits[j] < 10 {
                        break;
                    }
                    digits[j] = 0;
                }
            }
            break;
        }
        d *= 10.0;
        i += 1;
    }
    let text: String = digits.iter().map(|d| (b'0' + d) as char).collect();
    let e = k + 1;
    let (int, frac) = if e > 0 {
        let e = e as usize;
        let int = if text.len() >= e {
            text[..e].to_string()
        } else {
            format!("{text}{}", "0".repeat(e - text.len()))
        };
        (int, text.get(e..).unwrap_or("").to_string())
    } else {
        (
            "0".to_string(),
            format!("{}{}", "0".repeat((-e) as usize), text),
        )
    };
    let mut frac: String = frac.chars().take(n).collect();
    while frac.len() < n {
        frac.push('0');
    }
    Some(if n == 0 { int } else { format!("{int}.{frac}") })
}

/// A value as Ruby's `inspect` writes it: strings quoted and escaped,
/// arrays `[a, b]`, objects `{"k" => v}`, null `nil`.
pub fn inspect(v: &Value) -> String {
    match v {
        Value::Null => "nil".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Int(n) => n.to_string(),
        Value::Float(x) => float_text(*x),
        Value::Str(s) => inspect_str(s),
        Value::Arr(a) => format!("[{}]", a.iter().map(inspect).collect::<Vec<_>>().join(", ")),
        Value::Obj(m) => {
            if m.is_empty() {
                return "{}".to_string();
            }
            let parts: Vec<String> = m
                .iter()
                .map(|(k, v)| format!("{} => {}", inspect_str(k), inspect(v)))
                .collect();
            format!("{{{}}}", parts.join(", "))
        }
    }
}

/// A string as Ruby's `String#inspect` writes it.
pub fn inspect_str(s: &str) -> String {
    let mut out = String::from("\"");
    let chars: Vec<char> = s.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{c}' => out.push_str("\\f"),
            '\u{b}' => out.push_str("\\v"),
            '\u{8}' => out.push_str("\\b"),
            '\u{7}' => out.push_str("\\a"),
            '\u{1b}' => out.push_str("\\e"),
            '#' if matches!(chars.get(i + 1), Some('{' | '$' | '@')) => out.push_str("\\#"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04X}", c as u32);
            }
            '\u{7f}' => out.push_str("\\u007F"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Bytes as Ruby's `inspect` writes a binary string: printable ASCII as
/// it is, every other byte as `\xHH`.
pub fn inspect_bytes(b: &[u8]) -> String {
    let mut out = String::from("\"");
    for (i, &c) in b.iter().enumerate() {
        match c {
            b'"' => out.push_str("\\\""),
            b'\\' => out.push_str("\\\\"),
            b'\n' => out.push_str("\\n"),
            b'\r' => out.push_str("\\r"),
            b'\t' => out.push_str("\\t"),
            0x0c => out.push_str("\\f"),
            0x0b => out.push_str("\\v"),
            0x08 => out.push_str("\\b"),
            0x07 => out.push_str("\\a"),
            0x1b => out.push_str("\\e"),
            b'#' if matches!(b.get(i + 1), Some(b'{' | b'$' | b'@')) => out.push_str("\\#"),
            0x20..=0x7e => out.push(c as char),
            _ => {
                let _ = write!(out, "\\x{c:02X}");
            }
        }
    }
    out.push('"');
    out
}
