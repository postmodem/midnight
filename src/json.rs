//! JSON lines: the `--format json` form of the compiler's output.
//!
//! Each line is one object with `kind` first, no space between tokens, and
//! a newline at its end. `docs/json.md` lists every kind.

/// The version of the JSON forms.
pub const FORMAT: u32 = 1;

/// How the compiler writes what it says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Text,
    Json,
}

/// The first line of every JSON form.
pub fn first_line() -> String {
    let mut line = Object::new("midnight");
    line.int("format", FORMAT as i64);
    line.str("version", env!("CARGO_PKG_VERSION"));
    line.line()
}

/// Appends `text` as a JSON string.
pub fn string(out: &mut String, text: &str) {
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 || c == '\x7f' => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// The text of a JSON string.
pub fn quoted(text: &str) -> String {
    let mut out = String::new();
    string(&mut out, text);
    out
}

/// An array of values, each already JSON text.
pub fn array<I: IntoIterator<Item = String>>(items: I) -> String {
    let items: Vec<String> = items.into_iter().collect();
    format!("[{}]", items.join(","))
}

/// An object being written, its members in the order they are added.
pub struct Object {
    out: String,
}

impl Object {
    /// An object whose first member is `kind`.
    pub fn new(kind: &str) -> Object {
        let mut object = Object::empty();
        object.str("kind", kind);
        object
    }

    /// An object with no members yet.
    pub fn empty() -> Object {
        Object {
            out: String::from("{"),
        }
    }

    fn key(&mut self, key: &str) {
        if self.out.len() > 1 {
            self.out.push(',');
        }
        string(&mut self.out, key);
        self.out.push(':');
    }

    pub fn str(&mut self, key: &str, value: &str) -> &mut Object {
        self.key(key);
        string(&mut self.out, value);
        self
    }

    pub fn int(&mut self, key: &str, value: i64) -> &mut Object {
        self.key(key);
        self.out.push_str(&value.to_string());
        self
    }

    pub fn bool(&mut self, key: &str, value: bool) -> &mut Object {
        self.key(key);
        self.out.push_str(if value { "true" } else { "false" });
        self
    }

    /// A member whose value is already JSON text.
    pub fn raw(&mut self, key: &str, value: &str) -> &mut Object {
        self.key(key);
        self.out.push_str(value);
        self
    }

    /// The object's text.
    pub fn finish(&self) -> String {
        format!("{}}}", self.out)
    }

    /// The object's text as a line.
    pub fn line(&self) -> String {
        format!("{}}}\n", self.out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strings_escape_quotes_backslashes_and_control_characters() {
        assert_eq!(quoted("a\"b\\c"), r#""a\"b\\c""#);
        assert_eq!(quoted("\n\r\t"), r#""\n\r\t""#);
        assert_eq!(
            quoted("\u{0}\u{1b}\u{1f}\u{7f}"),
            r#""\u0000\u001b\u001f\u007f""#
        );
        assert_eq!(quoted("é→😀"), "\"é→😀\"");
    }

    #[test]
    fn an_object_has_its_kind_first_and_no_spaces() {
        let mut object = Object::new("x");
        object.int("n", -3).bool("b", true).raw("a", &array(vec![]));
        assert_eq!(
            object.line(),
            "{\"kind\":\"x\",\"n\":-3,\"b\":true,\"a\":[]}\n"
        );
        assert_eq!(Object::empty().finish(), "{}");
    }

    #[test]
    fn the_first_line_names_the_format_and_the_version() {
        assert_eq!(
            first_line(),
            format!(
                "{{\"kind\":\"midnight\",\"format\":1,\"version\":\"{}\"}}\n",
                env!("CARGO_PKG_VERSION")
            )
        );
    }
}
