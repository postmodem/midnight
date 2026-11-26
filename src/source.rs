//! Source files: positions, loading, and `require` resolution.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::ast;
use crate::diag::Diagnostic;
use crate::parser;

/// Index of a file in the [`SourceMap`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FileId(pub u32);

/// A range of bytes in one file. `start` is inclusive, `end` exclusive.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Span {
    pub file: FileId,
    pub start: u32,
    pub end: u32,
}

impl Span {
    pub fn new(file: FileId, start: u32, end: u32) -> Span {
        Span { file, start, end }
    }

    /// The smallest span covering both `self` and `other` (same file).
    pub fn to(self, other: Span) -> Span {
        Span {
            file: self.file,
            start: self.start.min(other.start),
            end: self.end.max(other.end),
        }
    }
}

pub struct SourceFile {
    /// The path as shown in diagnostics and runtime errors: the root file as
    /// given on the command line, a required file relative to it.
    pub name: String,
    pub path: PathBuf,
    pub text: String,
    /// Byte offset of the start of each line.
    line_starts: Vec<u32>,
}

impl SourceFile {
    /// 1-based line number of a byte offset.
    pub fn line_of(&self, offset: u32) -> u32 {
        match self.line_starts.binary_search(&offset) {
            Ok(i) => i as u32 + 1,
            Err(i) => i as u32,
        }
    }

    /// Text of a 1-based line, without its newline.
    pub fn line_text(&self, line: u32) -> &str {
        let start = self.line_starts[line as usize - 1] as usize;
        let end = self
            .line_starts
            .get(line as usize)
            .map_or(self.text.len(), |&e| e as usize);
        self.text[start..end].trim_end_matches(['\n', '\r'])
    }

    /// 1-based line and 1-based column (in characters) of a byte offset.
    pub fn line_col(&self, offset: u32) -> (u32, u32) {
        let line = self.line_of(offset);
        let start = self.line_starts[line as usize - 1] as usize;
        let end = (offset as usize).min(self.text.len());
        let col = self.text[start..end].chars().count() as u32 + 1;
        (line, col)
    }
}

#[derive(Default)]
pub struct SourceMap {
    files: Vec<SourceFile>,
}

impl SourceMap {
    pub fn new() -> SourceMap {
        SourceMap::default()
    }

    pub fn add(&mut self, name: String, path: PathBuf, text: String) -> FileId {
        let mut line_starts = vec![0];
        for (i, b) in text.bytes().enumerate() {
            if b == b'\n' {
                line_starts.push(i as u32 + 1);
            }
        }
        self.files.push(SourceFile {
            name,
            path,
            text,
            line_starts,
        });
        FileId(self.files.len() as u32 - 1)
    }

    pub fn file(&self, id: FileId) -> &SourceFile {
        &self.files[id.0 as usize]
    }

    /// 1-based line on which a span starts.
    pub fn line(&self, span: Span) -> u32 {
        self.file(span.file).line_of(span.start)
    }
}

/// Parses the root file and every file it requires, each once, and returns
/// one program: a required file's items stand where its first `require` was.
pub fn load(sources: &mut SourceMap, root: FileId) -> Result<ast::Program, Vec<Diagnostic>> {
    let mut loaded = HashSet::new();
    if let Ok(canonical) = sources.file(root).path.canonicalize() {
        loaded.insert(canonical);
    }
    let mut items = Vec::new();
    let mut errors = Vec::new();
    load_file(sources, root, &mut loaded, &mut items, &mut errors);
    if errors.is_empty() {
        Ok(with_builtins(sources, items))
    } else {
        Err(errors)
    }
}

/// The built-in declarations written in Midnight. `Window` has fields the
/// source cannot name: `open`, which `Window.open?` reads, and `focused`,
/// which `Window.focused?` reads; `Mouse` has `inside`, which
/// `Mouse.inside?` reads. `Key` lists its variants in the order of
/// `MN_KEYS` in `midnight_rt.h`, and `MouseButton` in that of
/// `MN_MOUSE_BUTTONS`.
const COLOR: &str = "\
struct Color
  r: UInt8 = 0
  g: UInt8 = 0
  b: UInt8 = 0
  a: UInt8 = 255
end
";

const WINDOW: &str = "\
resource Window
  width: Int = 0
  height: Int = 0
  pixel_width: Int = 0
  pixel_height: Int = 0
  background: Color = Color(0, 0, 0)
  open: Bool = false
  focused: Bool = false
end

event WindowResized
  width: Int
  height: Int
  pixel_width: Int
  pixel_height: Int
end

event WindowClosed
end

resource Mouse
  x: Float = 0.0
  y: Float = 0.0
  wheel_x: Float = 0.0
  wheel_y: Float = 0.0
  inside: Bool = false
end

event KeyPressed
  key: Key
end

event KeyReleased
  key: Key
end

event MousePressed
  button: MouseButton
  x: Float
  y: Float
end

event MouseReleased
  button: MouseButton
  x: Float
  y: Float
end

event WindowFocused
end

event WindowUnfocused
end
";

const KEYS: &str = "\
enum Key
  A, B, C, D, E, F, G, H, I, J, K, L, M, N, O, P, Q, R, S, T, U, V, W, X, Y, Z
  Digit1, Digit2, Digit3, Digit4, Digit5, Digit6, Digit7, Digit8, Digit9, Digit0
  Enter, Escape, Backspace, Tab, Space, Minus, Equal, BracketLeft, BracketRight
  Backslash, Semicolon, Quote, Backquote, Comma, Period, Slash, CapsLock
  F1, F2, F3, F4, F5, F6, F7, F8, F9, F10, F11, F12
  PrintScreen, ScrollLock, Pause, Insert, Home, PageUp, Delete, End, PageDown
  ArrowRight, ArrowLeft, ArrowDown, ArrowUp
  NumLock, NumpadDivide, NumpadMultiply, NumpadSubtract, NumpadAdd, NumpadEnter
  Numpad1, Numpad2, Numpad3, Numpad4, Numpad5, Numpad6, Numpad7, Numpad8, Numpad9
  Numpad0, NumpadDecimal, IntlBackslash, ContextMenu
  ControlLeft, ShiftLeft, AltLeft, MetaLeft
  ControlRight, ShiftRight, AltRight, MetaRight
end

enum MouseButton
  Left, Middle, Right, Back, Forward
end
";

/// The names that give a program a window.
const WINDOW_NAMES: [&str; 11] = [
    "Window",
    "WindowResized",
    "WindowClosed",
    "Keyboard",
    "Mouse",
    "KeyPressed",
    "KeyReleased",
    "MousePressed",
    "MouseReleased",
    "WindowFocused",
    "WindowUnfocused",
];

/// The program of `items`, from every file in `sources`, preceded by the
/// built-in declarations the files name: all of them when one names
/// something of the window or of the keyboard and the mouse, `Key` and
/// `MouseButton` alone when only they are named, and `Color` alone when
/// only it is.
pub fn with_builtins(sources: &mut SourceMap, items: Vec<ast::Item>) -> ast::Program {
    let mut names = HashSet::new();
    for file in &sources.files {
        if let Ok(tokens) = crate::lexer::lex(FileId(0), &file.text) {
            collect_names(&tokens, &mut names);
        }
    }
    let window = WINDOW_NAMES.iter().any(|name| names.contains(*name));
    let keys = names.contains("Key") || names.contains("MouseButton");
    let mut parts = Vec::new();
    if window || names.contains("Color") {
        parts.push(COLOR);
    }
    if window || keys {
        parts.push(KEYS);
    }
    if window {
        parts.push(WINDOW);
    }
    if parts.is_empty() {
        return ast::Program {
            items,
            builtins: None,
        };
    }
    let text = parts.join("\n");
    let file = sources.add("<built-in>".to_string(), PathBuf::from("<built-in>"), text);
    let mut all =
        parser::parse(file, &sources.file(file).text).expect("the built-in declarations parse");
    all.extend(items);
    ast::Program {
        items: all,
        builtins: Some(file),
    }
}

/// Every capitalized name among the tokens, interpolations included, but
/// for the variants of an enum: those declared in an `enum` and those
/// written after `::`.
fn collect_names(tokens: &[crate::lexer::Token], names: &mut HashSet<String>) {
    use crate::lexer::{Kw, StrPiece, TokenKind};
    // 0 outside an enum, 1 before its name, 2 among its variants.
    let mut in_enum = 0;
    let mut after_path = false;
    for token in tokens {
        let path = std::mem::replace(&mut after_path, token.kind == TokenKind::ColonColon);
        match &token.kind {
            TokenKind::Kw(Kw::Enum) => in_enum = 1,
            TokenKind::Kw(Kw::End) if in_enum == 2 => in_enum = 0,
            TokenKind::Const(name) => {
                if in_enum == 1 {
                    in_enum = 2;
                    names.insert(name.clone());
                } else if in_enum == 0 && !path {
                    names.insert(name.clone());
                }
            }
            TokenKind::Str(pieces) => {
                for piece in pieces {
                    if let StrPiece::Interp(inner) = piece {
                        collect_names(inner, names);
                    }
                }
            }
            _ => {}
        }
    }
}

fn load_file(
    sources: &mut SourceMap,
    file: FileId,
    loaded: &mut HashSet<PathBuf>,
    out: &mut Vec<ast::Item>,
    errors: &mut Vec<Diagnostic>,
) {
    let parsed = match parser::parse(file, &sources.file(file).text) {
        Ok(items) => items,
        Err(error) => {
            errors.push(error);
            return;
        }
    };
    for item in parsed {
        let ast::Item::Require(require) = item else {
            out.push(item);
            continue;
        };
        let file_name = format!("{}.12am", require.path);
        let path = sibling(&sources.file(file).path, &file_name);
        let name = sibling(Path::new(&sources.file(file).name), &file_name)
            .to_string_lossy()
            .into_owned();
        let Ok(text) = std::fs::read_to_string(&path) else {
            errors.push(
                Diagnostic::new(format!("can't find `{name}`"), require.span)
                    .label(format!("required here as \"{}\"", require.path)),
            );
            continue;
        };
        let canonical = path.canonicalize().unwrap_or_else(|_| path.clone());
        if !loaded.insert(canonical) {
            continue;
        }
        let id = sources.add(name, path, text);
        load_file(sources, id, loaded, out, errors);
    }
}

/// `name` in the directory that holds `file`.
fn sibling(file: &Path, name: &str) -> PathBuf {
    match file.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => dir.join(name),
        _ => PathBuf::from(name),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The names of an X-macro list in `midnight_rt.h`, in order: the first
    /// argument of each `X(…)` in the definition of `macro`.
    fn header_list(macro_name: &str) -> Vec<String> {
        let header = include_str!("../runtime/midnight_rt.h");
        let start = header
            .find(&format!("#define {macro_name}(X)"))
            .expect("the header defines the list");
        let mut names = Vec::new();
        for line in header[start..].lines().skip(1) {
            let line = line.trim();
            let Some(rest) = line.strip_prefix("X(") else {
                break;
            };
            names.push(rest.split(',').next().unwrap().trim().to_string());
            if !line.ends_with('\\') {
                break;
            }
        }
        names
    }

    /// The variants of `name` in the built-in declarations.
    fn builtin_variants(name: &str) -> Vec<String> {
        let mut sources = SourceMap::new();
        let file = sources.add("t".to_string(), PathBuf::from("t"), KEYS.to_string());
        let items = parser::parse(file, &sources.file(file).text).unwrap();
        items
            .into_iter()
            .find_map(|item| match item {
                ast::Item::Enum(decl) if decl.name.name == name => {
                    Some(decl.variants.into_iter().map(|v| v.name).collect())
                }
                _ => None,
            })
            .unwrap()
    }

    #[test]
    fn keys_and_buttons_match_the_runtime() {
        let keys = builtin_variants("Key");
        assert_eq!(keys.len(), 105);
        assert_eq!(keys, header_list("MN_KEYS"));
        assert_eq!(
            builtin_variants("MouseButton"),
            header_list("MN_MOUSE_BUTTONS")
        );
    }

    fn builtin_names(text: &str) -> Vec<String> {
        let mut sources = SourceMap::new();
        let file = sources.add("t".to_string(), PathBuf::from("t"), text.to_string());
        let items = parser::parse(file, &sources.file(file).text).unwrap();
        let program = with_builtins(&mut sources, items);
        let Some(builtins) = program.builtins else {
            return Vec::new();
        };
        program
            .items
            .iter()
            .filter_map(|item| match item {
                ast::Item::Enum(d) if d.span.file == builtins => Some(d.name.name.clone()),
                ast::Item::Record(d)
                    if d.span.file == builtins && d.kind == ast::RecordKind::Resource =>
                {
                    Some(d.name.name.clone())
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn names_that_give_the_built_ins() {
        assert_eq!(builtin_names("puts 1\n"), Vec::<String>::new());
        assert_eq!(builtin_names("x = Key::A\n"), ["Key", "MouseButton"]);
        assert_eq!(
            builtin_names("puts Mouse.x\n"),
            ["Key", "MouseButton", "Window", "Mouse"]
        );
        assert_eq!(
            builtin_names("enum Shape\n  Mouse, Window\nend\nputs Shape::Keyboard\n"),
            Vec::<String>::new()
        );
    }
}
