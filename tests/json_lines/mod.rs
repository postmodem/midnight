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

/// `N word` or `N words`.
fn counted(n: i64, word: &str) -> String {
    if n == 1 {
        format!("{n} {word}")
    } else {
        format!("{n} {word}s")
    }
}

/// A number of an answer as the text writes it: its JSON text, or a
/// non-finite one's string.
fn number_text(value: &Value) -> Result<String, String> {
    match value {
        Value::Number(text) => Ok(text.clone()),
        Value::Str(text) if matches!(text.as_str(), "NaN" | "Infinity" | "-Infinity") => {
            Ok(text.clone())
        }
        other => Err(format!("not a number: {}", other.text())),
    }
}

/// A color `[r,g,b,a]` as the text writes it: `Color(r, g, b, a)`.
fn color_text(value: &Value) -> Result<String, String> {
    let bytes: Vec<i64> = value
        .as_array()
        .ok_or("a color is an array")?
        .iter()
        .map(|byte| byte.as_i64().filter(|b| (0..=255).contains(b)))
        .collect::<Option<_>>()
        .ok_or_else(|| format!("not a color: {}", value.text()))?;
    if bytes.len() != 4 {
        return Err(format!("not a color: {}", value.text()));
    }
    Ok(format!(
        "Color({}, {}, {}, {})",
        bytes[0], bytes[1], bytes[2], bytes[3]
    ))
}

/// The argument names of each shape, in the order of its statement.
const SHAPES: [(&str, &[&str]); 5] = [
    ("rect", &["x", "y", "width", "height"]),
    ("circle", &["x", "y", "radius"]),
    ("line", &["x1", "y1", "x2", "y2", "width"]),
    ("triangle", &["x1", "y1", "x2", "y2", "x3", "y3"]),
    ("text", &["x", "y", "text", "size"]),
];

/// A string as the text form writes it inside a struct and in a draw of
/// text: quoted, with `\n \t \r \0 \e \\ \"` escaped, `\#` before `{`, and
/// other bytes below 0x20 as `\x` and two upper-case hexadecimal digits.
fn quoted_text(text: &str) -> String {
    let mut out = String::from("\"");
    let chars: Vec<char> = text.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        match c {
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            '\0' => out.push_str("\\0"),
            '\x1b' => out.push_str("\\e"),
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '#' if chars.get(i + 1) == Some(&'{') => out.push_str("\\#"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\x{:02X}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The leaves of a value of the world in its text form, in order: numbers
/// as written (`n:`), Booleans (`b:`), and as strings (`s:`) strings
/// unescaped, entities, enum variants and non-finite numbers. Struct and
/// field names and punctuation are left out, so that the JSON form, which
/// has no struct names, gives the same leaves.
pub fn text_leaves(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut leaves = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() || matches!(c, ',' | '(' | ')' | '[' | ']') {
            i += 1;
        } else if c == '"' {
            let mut out = String::new();
            i += 1;
            while i < chars.len() && chars[i] != '"' {
                if chars[i] == '\\' && i + 1 < chars.len() {
                    i += 1;
                    match chars[i] {
                        'n' => out.push('\n'),
                        't' => out.push('\t'),
                        'r' => out.push('\r'),
                        '0' => out.push('\0'),
                        'e' => out.push('\x1b'),
                        'x' => {
                            let hex: String = chars[i + 1..i + 3].iter().collect();
                            out.push(u8::from_str_radix(&hex, 16).unwrap_or(0) as char);
                            i += 2;
                        }
                        other => out.push(other),
                    }
                } else {
                    out.push(chars[i]);
                }
                i += 1;
            }
            i += 1;
            leaves.push(format!("s:{out}"));
        } else if c.is_alphabetic() || c == '_' {
            let start = i;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            match chars.get(i) {
                Some(':') => i += 1,
                Some('(') if word == "Entity" => {
                    let end = chars[i..]
                        .iter()
                        .position(|&c| c == ')')
                        .map_or(chars.len(), |p| i + p + 1);
                    let entity: String = chars[start..end].iter().collect();
                    leaves.push(format!("s:{entity}"));
                    i = end;
                }
                Some('(') => {}
                _ if word == "true" || word == "false" => leaves.push(format!("b:{word}")),
                _ => leaves.push(format!("s:{word}")),
            }
        } else if c == '-' && chars[i..].starts_with(&['-', 'I']) {
            leaves.push("s:-Infinity".to_string());
            i += 9;
        } else {
            let start = i;
            while i < chars.len()
                && (chars[i].is_ascii_digit() || matches!(chars[i], '-' | '+' | '.' | 'e'))
            {
                i += 1;
            }
            if i == start {
                i += 1;
                continue;
            }
            leaves.push(format!("n:{}", chars[start..i].iter().collect::<String>()));
        }
    }
    leaves
}

/// The leaves of a JSON value, as [`text_leaves`] gives them.
pub fn json_leaves(value: &Value, leaves: &mut Vec<String>) {
    match value {
        Value::Null => leaves.push("null".to_string()),
        Value::Bool(b) => leaves.push(format!("b:{b}")),
        Value::Number(text) => leaves.push(format!("n:{text}")),
        Value::Str(text) => leaves.push(format!("s:{text}")),
        Value::Array(items) => items.iter().for_each(|item| json_leaves(item, leaves)),
        Value::Object(members) => members.iter().for_each(|(_, v)| json_leaves(v, leaves)),
    }
}

/// A run's standard error with the values of each `query` row and
/// `resource` answer written as their leaves, as [`runtime_text`] writes
/// them from JSON.
pub fn with_leaves(stderr: &str) -> String {
    let mut out = String::new();
    let mut in_query = false;
    for line in stderr.split_inclusive('\n') {
        let said = line.strip_prefix("midnight: drive: ");
        match said {
            Some(answer) if answer.starts_with("query ") => {
                in_query = true;
                out.push_str(line);
            }
            Some(answer) if in_query && answer.starts_with("  ") => {
                let row = answer.trim_start().trim_end_matches('\n');
                let (entity, rest) = row.split_once(' ').unwrap_or((row, ""));
                out.push_str(&format!(
                    "midnight: drive:   {entity} {}\n",
                    text_leaves(rest).join(" ")
                ));
            }
            Some(answer) if answer.starts_with("resource ") => {
                in_query = false;
                let answer = answer.trim_end_matches('\n');
                let (head, value) = answer
                    .split_once(" ticks, ")
                    .or_else(|| answer.split_once(" tick, "))
                    .unwrap_or((answer, ""));
                let ticks = if answer.contains(" ticks, ") {
                    "ticks"
                } else {
                    "tick"
                };
                out.push_str(&format!(
                    "midnight: drive: {head} {ticks}, {}\n",
                    text_leaves(value).join(" ")
                ));
            }
            _ => {
                in_query = false;
                out.push_str(line);
            }
        }
    }
    out
}

/// The text of an answer, each line without `midnight: drive: `.
fn answer_text(line: &Value) -> Result<Vec<String>, String> {
    let command = line.str("command")?;
    let name = line.str("name")?;
    if command.split([' ', '\t']).next() != Some(name) {
        return Err(format!(
            "`name` is not the command's first word: {}",
            line.text()
        ));
    }
    line.int("line")?;
    line.int("tick")?;
    Ok(match name {
        "tick" if line.bool("ended")? => vec![format!(
            "{command}: the program ended after {}",
            counted(line.int("ran")?, "tick")
        )],
        "tick" => {
            line.int("ran")?;
            vec![command.to_string()]
        }
        "capture" => {
            line.str("file")?;
            vec![format!(
                "{command}: {}, {}x{}, crc {}",
                counted(line.int("frame_tick")?, "tick"),
                line.int("pixel_width")?,
                line.int("pixel_height")?,
                line.str("crc")?
            )]
        }
        "draws" => {
            let draws = line.array("draws")?;
            let mut lines = vec![
                format!(
                    "draws: {}, {}x{}, {}",
                    counted(line.int("frame_tick")?, "tick"),
                    line.int("width")?,
                    line.int("height")?,
                    counted(draws.len() as i64, "draw")
                ),
                format!(
                    "  Window.background = {}",
                    color_text(line.get("background").ok_or("no background")?)?
                ),
            ];
            for draw in draws {
                let shape = draw.str("shape")?;
                let (_, names) = SHAPES
                    .iter()
                    .find(|(name, _)| *name == shape)
                    .ok_or_else(|| format!("no shape `{shape}`"))?;
                let mut args = Vec::new();
                for name in names.iter() {
                    if *name == "text" {
                        args.push(quoted_text(draw.str("text")?));
                        continue;
                    }
                    args.push(number_text(draw.get(name).ok_or(format!("no `{name}`"))?)?);
                }
                args.push(color_text(draw.get("color").ok_or("no color")?)?);
                lines.push(format!("  Draw.{shape}({})", args.join(", ")));
            }
            lines
        }
        "query" => {
            let tick = line.int("tick")?;
            let count = line.int("count")?;
            let from = line.int("from")?;
            let rows = line.array("rows")?;
            let listed = rows.len() as i64;
            let mut head = format!(
                "{command}: {}, {}",
                counted(tick, "tick"),
                counted(count, "row")
            );
            if listed < count && listed == 0 {
                head.push_str(&format!(", none from {from}"));
            } else if listed < count {
                head.push_str(&format!(", {from} to {}", from + listed - 1));
            }
            let mut lines = vec![head];
            for row in rows {
                let members = row.members().ok_or("a row is an object")?;
                let (key, entity) = members.first().ok_or("an empty row")?;
                if key != "entity" {
                    return Err(format!("a row begins with `{key}`"));
                }
                let mut leaves = Vec::new();
                for (_, value) in &members[1..] {
                    json_leaves(value, &mut leaves);
                }
                lines.push(format!(
                    "  {} {}",
                    entity.as_str().ok_or("an entity")?,
                    leaves.join(" ")
                ));
            }
            lines
        }
        "resource" => {
            line.str("resource")?;
            let mut leaves = Vec::new();
            json_leaves(line.get("fields").ok_or("no fields")?, &mut leaves);
            vec![format!(
                "{command}: {}, {}",
                counted(line.int("tick")?, "tick"),
                leaves.join(" ")
            )]
        }
        _ => vec![command.to_string()],
    })
}

/// The text a program writes to standard error for the JSON lines it
/// wrote with `MIDNIGHT_FORMAT=json`, its first line left out.
pub fn runtime_text(lines: &[Value]) -> Result<String, String> {
    let mut out = String::new();
    for line in lines {
        let text = match line.kind() {
            "runtime_error" => format!(
                "{}:{}: runtime error: {}",
                line.str("file")?,
                line.int("line")?,
                line.str("message")?
            ),
            "gpu_error" => format!("midnight: GPU error: {}", line.str("message")?),
            "window_error" => format!("midnight: window error: {}", line.str("message")?),
            "out_of_memory" => "midnight: out of memory".to_string(),
            "buffers_alive" => format!(
                "midnight: {} still alive at exit",
                counted(line.int("count")?, "buffer")
            ),
            "unknown_word" => format!(
                "midnight: {}: unknown word `{}`",
                line.str("variable")?,
                line.str("word")?
            ),
            "gpu_device" => match line.get("device") {
                Some(device) => format!("midnight: gpu: {}", device.as_str().ok_or("device")?),
                None => "midnight: gpu: no device".to_string(),
            },
            "gpu_strict" if line.bool("passed")? => {
                "midnight: gpu: strict check passed".to_string()
            }
            "gpu_strict" => format!(
                "midnight: gpu: strict blocks run on the CPU: {}",
                line.str("difference")?
            ),
            "gpu_cpu_block" => format!(
                "midnight: gpu: {} (each {}) runs on the CPU: {}",
                line.str("system")?,
                line.int("each")?,
                line.str("reason")?
            ),
            "threads" => format!("midnight: threads: {}", line.int("threads")?),
            "threads_split" => format!(
                "midnight: threads: {}:{}: {} in {} on {} threads",
                line.str("file")?,
                line.int("line")?,
                counted(line.int("rows")?, "row"),
                counted(line.int("pieces")?, "piece"),
                line.int("threads")?
            ),
            "window_opened" => {
                let mut text = format!(
                    "midnight: window: {}x{} points, {}x{} pixels, {}",
                    line.int("width")?,
                    line.int("height")?,
                    line.int("pixel_width")?,
                    line.int("pixel_height")?,
                    line.str("shown")?
                );
                if let Some(present) = line.get("present") {
                    text.push_str(&format!(", {}", present.as_str().ok_or("present")?));
                }
                if line.bool("driven")? {
                    text.push_str("\nmidnight: window: input from commands");
                }
                text
            }
            "window_event" => match line.str("event")? {
                "focused" => "midnight: window: focused".to_string(),
                "pointer_inside" => "midnight: window: pointer inside".to_string(),
                other => return Err(format!("no window event `{other}`")),
            },
            "stats" => format!(
                "midnight: gpu: tick {}: {}, {}, {} up, {} down, {}, {}",
                line.int("tick")?,
                counted(line.int("uploads")?, "upload"),
                counted(line.int("downloads")?, "download"),
                counted(line.int("rows_up")?, "row"),
                counted(line.int("rows_down")?, "row"),
                counted(line.int("submissions")?, "submission"),
                counted(line.int("waits")?, "wait")
            ),
            "answer" => answer_text(line)?
                .iter()
                .map(|text| format!("midnight: drive: {text}"))
                .collect::<Vec<_>>()
                .join("\n"),
            "drive_error" => format!(
                "midnight: drive: {}:{}: {}",
                line.str("file")?,
                line.int("line")?,
                line.str("message")?
            ),
            "drive_unopened" => format!(
                "midnight: drive: can't open {}: {}",
                line.str("file")?,
                line.str("message")?
            ),
            other => return Err(format!("no kind `{other}` in a program: {}", line.text())),
        };
        out.push_str(&text);
        out.push('\n');
    }
    Ok(out)
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
