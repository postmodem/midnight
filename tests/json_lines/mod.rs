//! A reader of Midnight's JSON lines, strict about the text of a line as
//! the spec of the JSON forms states it: one object, no space between
//! tokens, no member twice, and strings escaped exactly as Midnight
//! escapes them (`\"`, `\\`, `\n`, `\r`, `\t`, and `\u00xx` in lower case
//! for the other bytes below 0x20 and 0x7f, everything else as itself).

/// A JSON value. A number keeps its text, so that a test can compare it
/// exactly.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Number(String),
    Str(String),
    Array(Vec<Value>),
    Object(Vec<(String, Value)>),
}

impl Value {
    /// The member `key` of an object.
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Object(members) => members
                .iter()
                .find(|(name, _)| name == key)
                .map(|(_, value)| value),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(text) => Some(text),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Value::Number(text) => text.parse().ok(),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(value) => Some(*value),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[Value]> {
        match self {
            Value::Array(items) => Some(items),
            _ => None,
        }
    }

    pub fn members(&self) -> Option<&[(String, Value)]> {
        match self {
            Value::Object(members) => Some(members),
            _ => None,
        }
    }

    /// The string member `key`, or an error naming it.
    pub fn str(&self, key: &str) -> Result<&str, String> {
        self.get(key)
            .and_then(Value::as_str)
            .ok_or_else(|| format!("no string `{key}` in {}", self.text()))
    }

    /// The integer member `key`, or an error naming it.
    pub fn int(&self, key: &str) -> Result<i64, String> {
        self.get(key)
            .and_then(Value::as_i64)
            .ok_or_else(|| format!("no integer `{key}` in {}", self.text()))
    }

    /// The Boolean member `key`, or an error naming it.
    pub fn bool(&self, key: &str) -> Result<bool, String> {
        self.get(key)
            .and_then(Value::as_bool)
            .ok_or_else(|| format!("no Boolean `{key}` in {}", self.text()))
    }

    /// The array member `key`, or an error naming it.
    pub fn array(&self, key: &str) -> Result<&[Value], String> {
        self.get(key)
            .and_then(Value::as_array)
            .ok_or_else(|| format!("no array `{key}` in {}", self.text()))
    }

    /// The value's `kind`, or the empty string.
    pub fn kind(&self) -> &str {
        self.get("kind").and_then(Value::as_str).unwrap_or("")
    }

    /// The value as JSON text, as Midnight writes it.
    pub fn text(&self) -> String {
        let mut out = String::new();
        self.write(&mut out);
        out
    }

    fn write(&self, out: &mut String) {
        match self {
            Value::Null => out.push_str("null"),
            Value::Bool(value) => out.push_str(if *value { "true" } else { "false" }),
            Value::Number(text) => out.push_str(text),
            Value::Str(text) => write_string(out, text),
            Value::Array(items) => {
                out.push('[');
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    item.write(out);
                }
                out.push(']');
            }
            Value::Object(members) => {
                out.push('{');
                for (i, (key, value)) in members.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    write_string(out, key);
                    out.push(':');
                    value.write(out);
                }
                out.push('}');
            }
        }
    }
}

fn write_string(out: &mut String, text: &str) {
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 || c == '\x7f' => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

/// Parses one line, without its newline, as one object.
pub fn parse_line(line: &str) -> Result<Value, String> {
    let mut reader = Reader {
        bytes: line.as_bytes(),
        pos: 0,
    };
    let value = reader
        .value()
        .map_err(|message| format!("{message} at byte {} of {line:?}", reader.pos))?;
    if reader.pos != line.len() {
        return Err(format!(
            "text after the value at byte {} of {line:?}",
            reader.pos
        ));
    }
    if !matches!(value, Value::Object(_)) {
        return Err(format!("not an object: {line:?}"));
    }
    Ok(value)
}

/// Parses a JSON form: every line one object with a string `kind`, each
/// ended by a newline, the first `{"kind":"midnight","format":1,...}` with
/// a string `version`. Returns the lines after the first.
pub fn parse_form(text: &str) -> Result<Vec<Value>, String> {
    if text.is_empty() {
        return Err("no first line".to_string());
    }
    if !text.ends_with('\n') {
        return Err(format!("the last line has no newline: {text:?}"));
    }
    let mut values = Vec::new();
    for line in text[..text.len() - 1].split('\n') {
        let value = parse_line(line)?;
        if value.get("kind").and_then(Value::as_str).is_none() {
            return Err(format!("no string `kind`: {line}"));
        }
        values.push(value);
    }
    let first = values.remove(0);
    let keys: Vec<&str> = first
        .members()
        .unwrap_or_default()
        .iter()
        .map(|(key, _)| key.as_str())
        .collect();
    if first.kind() != "midnight"
        || first.int("format") != Ok(1)
        || first.str("version").is_err()
        || keys != ["kind", "format", "version"]
    {
        return Err(format!(
            "the first line is not the `midnight` line: {}",
            first.text()
        ));
    }
    Ok(values)
}

struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl Reader<'_> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn expect(&mut self, byte: u8) -> Result<(), String> {
        if self.peek() == Some(byte) {
            self.pos += 1;
            Ok(())
        } else {
            Err(format!("expected `{}`", byte as char))
        }
    }

    fn word(&mut self, word: &str, value: Value) -> Result<Value, String> {
        if self.bytes[self.pos..].starts_with(word.as_bytes()) {
            self.pos += word.len();
            Ok(value)
        } else {
            Err("unexpected text".to_string())
        }
    }

    fn value(&mut self) -> Result<Value, String> {
        match self.peek() {
            Some(b'{') => self.object(),
            Some(b'[') => self.array(),
            Some(b'"') => Ok(Value::Str(self.string()?)),
            Some(b't') => self.word("true", Value::Bool(true)),
            Some(b'f') => self.word("false", Value::Bool(false)),
            Some(b'n') => self.word("null", Value::Null),
            Some(b'-' | b'0'..=b'9') => self.number(),
            Some(b' ' | b'\t' | b'\n' | b'\r') => Err("a space between tokens".to_string()),
            _ => Err("expected a value".to_string()),
        }
    }

    fn object(&mut self) -> Result<Value, String> {
        self.expect(b'{')?;
        let mut members: Vec<(String, Value)> = Vec::new();
        if self.peek() == Some(b'}') {
            self.pos += 1;
            return Ok(Value::Object(members));
        }
        loop {
            if self.peek() != Some(b'"') {
                return Err("expected a key".to_string());
            }
            let key = self.string()?;
            if members.iter().any(|(name, _)| *name == key) {
                return Err(format!("member `{key}` twice"));
            }
            self.expect(b':')?;
            let value = self.value()?;
            members.push((key, value));
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b'}') => {
                    self.pos += 1;
                    return Ok(Value::Object(members));
                }
                _ => return Err("expected `,` or `}`".to_string()),
            }
        }
    }

    fn array(&mut self) -> Result<Value, String> {
        self.expect(b'[')?;
        let mut items = Vec::new();
        if self.peek() == Some(b']') {
            self.pos += 1;
            return Ok(Value::Array(items));
        }
        loop {
            items.push(self.value()?);
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b']') => {
                    self.pos += 1;
                    return Ok(Value::Array(items));
                }
                _ => return Err("expected `,` or `]`".to_string()),
            }
        }
    }

    fn number(&mut self) -> Result<Value, String> {
        let start = self.pos;
        let digits = |reader: &mut Reader| {
            let from = reader.pos;
            while matches!(reader.peek(), Some(b'0'..=b'9')) {
                reader.pos += 1;
            }
            reader.pos - from
        };
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        if self.peek() == Some(b'0') {
            self.pos += 1;
            if matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err("a number with a leading zero".to_string());
            }
        } else if digits(self) == 0 {
            return Err("a number with no digits".to_string());
        }
        if self.peek() == Some(b'.') {
            self.pos += 1;
            if digits(self) == 0 {
                return Err("no digits after `.`".to_string());
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.pos += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            if digits(self) == 0 {
                return Err("no digits in the exponent".to_string());
            }
        }
        let text = std::str::from_utf8(&self.bytes[start..self.pos]).unwrap();
        Ok(Value::Number(text.to_string()))
    }

    fn string(&mut self) -> Result<String, String> {
        self.expect(b'"')?;
        let mut out = String::new();
        loop {
            let start = self.pos;
            while !matches!(self.peek(), None | Some(b'"' | b'\\')) {
                if self.peek().unwrap() < 0x20 || self.peek() == Some(0x7f) {
                    return Err("a control character in a string".to_string());
                }
                self.pos += 1;
            }
            out.push_str(std::str::from_utf8(&self.bytes[start..self.pos]).unwrap());
            match self.peek() {
                None => return Err("an unterminated string".to_string()),
                Some(b'"') => {
                    self.pos += 1;
                    return Ok(out);
                }
                _ => {}
            }
            self.pos += 1;
            let escape = self.peek().ok_or("an unterminated escape")?;
            self.pos += 1;
            out.push(match escape {
                b'"' => '"',
                b'\\' => '\\',
                b'n' => '\n',
                b'r' => '\r',
                b't' => '\t',
                b'u' => {
                    let hex = self
                        .bytes
                        .get(self.pos..self.pos + 4)
                        .and_then(|hex| std::str::from_utf8(hex).ok())
                        .ok_or("a short `\\u` escape")?;
                    if !hex.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
                        return Err("a `\\u` escape not in lower-case hex".to_string());
                    }
                    let code = u32::from_str_radix(hex, 16).unwrap();
                    if !(code < 0x20 || code == 0x7f) || matches!(code, 0x09 | 0x0a | 0x0d) {
                        return Err(format!("`\\u{hex}` for a character written as itself"));
                    }
                    self.pos += 4;
                    char::from_u32(code).unwrap()
                }
                other => return Err(format!("the escape `\\{}`", other as char)),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_every_kind_of_value() {
        let value = parse_line(
            r#"{"kind":"x","a":[1,-2.5e-3,true,false,null],"s":"\"\\\n\r\t\u0001é","o":{}}"#,
        )
        .unwrap();
        assert_eq!(value.kind(), "x");
        assert_eq!(
            value.array("a").unwrap()[1],
            Value::Number("-2.5e-3".into())
        );
        assert_eq!(value.str("s").unwrap(), "\"\\\n\r\t\u{1}é");
        assert_eq!(
            value.text(),
            r#"{"kind":"x","a":[1,-2.5e-3,true,false,null],"s":"\"\\\n\r\t\u0001é","o":{}}"#
        );
    }

    #[test]
    fn refuses_what_midnight_does_not_write() {
        for line in [
            r#"{"a": 1}"#,
            r#"{"a":1,"a":2}"#,
            r#"{"a":"\/"}"#,
            r#"{"a":"\u001B"}"#,
            concat!(r#"{"a":""#, "\\", r#"u0041"}"#),
            r#"{"a":"\u000a"}"#,
            "{\"a\":\"\t\"}",
            r#"{"a":01}"#,
            r#"{"a":1.}"#,
            r#"[1]"#,
            r#"{"a":1}x"#,
            r#"{"a":NaN}"#,
        ] {
            assert!(parse_line(line).is_err(), "{line}");
        }
    }

    #[test]
    fn a_form_begins_with_the_midnight_line() {
        let first = "{\"kind\":\"midnight\",\"format\":1,\"version\":\"0.9.0\"}\n";
        assert_eq!(parse_form(first).unwrap(), vec![]);
        let lines = parse_form(&format!("{first}{{\"kind\":\"x\"}}\n")).unwrap();
        assert_eq!(lines[0].kind(), "x");
        assert!(parse_form("{\"kind\":\"x\"}\n").is_err());
        assert!(parse_form(&format!("{first}{{\"k\":1}}\n")).is_err());
        assert!(parse_form(first.trim_end()).is_err());
        assert!(parse_form("").is_err());
    }
}
