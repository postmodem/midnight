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
        Ok(ast::Program { items })
    } else {
        Err(errors)
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
