//! Compile-time diagnostics and how they are printed.
//!
//! ```text
//! error: unknown component `Postion`
//!   --> move.grem:14:8
//!    |
//! 14 |   each Postion, Velocity do |pos, vel|
//!    |        ^^^^^^^ did you mean `Position`?
//! ```

use crate::source::{SourceMap, Span};

/// The checker stops reporting after this many errors.
pub const MAX_ERRORS: usize = 20;

#[derive(Clone, Debug)]
pub struct Diagnostic {
    /// One line, no trailing period: "unknown component `Postion`".
    pub message: String,
    /// The offending source text. Only its first line is marked.
    pub span: Span,
    /// Printed after the marker: a hint, a suggestion or a reason.
    pub label: Option<String>,
    /// Further places worth showing, such as an earlier definition.
    pub notes: Vec<Note>,
}

#[derive(Clone, Debug)]
pub struct Note {
    /// One line: "first defined here".
    pub message: String,
    pub span: Span,
}

impl Diagnostic {
    pub fn new(message: impl Into<String>, span: Span) -> Diagnostic {
        Diagnostic {
            message: message.into(),
            span,
            label: None,
            notes: Vec::new(),
        }
    }

    pub fn label(mut self, label: impl Into<String>) -> Diagnostic {
        self.label = Some(label.into());
        self
    }

    pub fn note(mut self, message: impl Into<String>, span: Span) -> Diagnostic {
        self.notes.push(Note {
            message: message.into(),
            span,
        });
        self
    }
}

const RED: &str = "\x1b[1;31m";
const BLUE: &str = "\x1b[1;34m";
const BOLD: &str = "\x1b[1m";
const RESET: &str = "\x1b[0m";

/// Renders diagnostics in order, separated by blank lines.
pub fn render(sources: &SourceMap, diagnostics: &[Diagnostic], color: bool) -> String {
    let mut out = String::new();
    for (i, diagnostic) in diagnostics.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let (red, bold, reset) = if color {
            (RED, BOLD, RESET)
        } else {
            ("", "", "")
        };
        out.push_str(&format!(
            "{red}error{reset}{bold}: {}{reset}\n",
            diagnostic.message
        ));
        excerpt(
            &mut out,
            sources,
            diagnostic.span,
            diagnostic.label.as_deref(),
            color,
            true,
        );
        for note in &diagnostic.notes {
            let (blue, reset) = if color { (BLUE, RESET) } else { ("", "") };
            out.push_str(&format!("{blue}note{reset}: {}\n", note.message));
            excerpt(&mut out, sources, note.span, None, color, false);
        }
    }
    out
}

fn excerpt(
    out: &mut String,
    sources: &SourceMap,
    span: Span,
    label: Option<&str>,
    color: bool,
    primary: bool,
) {
    let file = sources.file(span.file);
    let (line, col) = file.line_col(span.start);
    let text = file.line_text(line);
    let number = line.to_string();
    let pad = " ".repeat(number.len());

    // The marker covers the span's part of its first line, at least one column.
    let line_chars = text.chars().count() as u32;
    let width = if span.end > span.start {
        let (end_line, end_col) = file.line_col(span.end);
        if end_line == line {
            end_col - col
        } else {
            (line_chars + 1).saturating_sub(col)
        }
    } else {
        1
    }
    .max(1);

    let (blue, mark, reset) = match (color, primary) {
        (true, true) => (BLUE, RED, RESET),
        (true, false) => (BLUE, BLUE, RESET),
        (false, _) => ("", "", ""),
    };
    let marker = if primary { "^" } else { "-" }.repeat(width as usize);
    let indent = " ".repeat(col as usize - 1);

    out.push_str(&format!(
        "{pad}{blue}-->{reset} {}:{line}:{col}\n",
        file.name
    ));
    out.push_str(&format!("{pad} {blue}|{reset}\n"));
    out.push_str(&format!("{blue}{number} |{reset} {text}\n"));
    out.push_str(&format!("{pad} {blue}|{reset} {indent}{mark}{marker}"));
    if let Some(label) = label {
        out.push_str(&format!(" {label}"));
    }
    out.push_str(&format!("{reset}\n"));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn renders_the_reference_example() {
        let mut sources = SourceMap::new();
        let mut text = "\n".repeat(13);
        text.push_str("  each Postion, Velocity do |pos, vel|\n");
        let file = sources.add("move.grem".into(), PathBuf::from("move.grem"), text);
        let diagnostic = Diagnostic::new("unknown component `Postion`", Span::new(file, 20, 27))
            .label("did you mean `Position`?");
        let expected = "\
error: unknown component `Postion`
  --> move.grem:14:8
   |
14 |   each Postion, Velocity do |pos, vel|
   |        ^^^^^^^ did you mean `Position`?
";
        assert_eq!(render(&sources, &[diagnostic], false), expected);
    }

    #[test]
    fn renders_a_note_with_dashes() {
        let mut sources = SourceMap::new();
        let file = sources.add(
            "a.grem".into(),
            PathBuf::from("a.grem"),
            "tag A\ntag A\n".into(),
        );
        let diagnostic = Diagnostic::new("`A` is already defined", Span::new(file, 10, 11))
            .note("first defined here", Span::new(file, 4, 5));
        let expected = "\
error: `A` is already defined
 --> a.grem:2:5
  |
2 | tag A
  |     ^
note: first defined here
 --> a.grem:1:5
  |
1 | tag A
  |     -
";
        assert_eq!(render(&sources, &[diagnostic], false), expected);
    }
}
