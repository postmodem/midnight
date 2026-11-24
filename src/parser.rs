//! Tokens to syntax tree.

use crate::ast::{Expr, ExprKind, Item, Stmt, StmtKind, StrPart};
use crate::diag::Diagnostic;
use crate::source::{FileId, Span};

/// Parses one file into its items. Reports the first syntax error.
pub fn parse(file: FileId, text: &str) -> Result<Vec<Item>, Diagnostic> {
    let mut items = Vec::new();
    let mut offset = 0u32;
    for line in text.split_inclusive('\n') {
        let start = offset + (line.len() - line.trim_start().len()) as u32;
        let code = line.trim();
        let span = Span::new(file, start, start + code.len() as u32);
        offset += line.len() as u32;
        if code.is_empty() || code.starts_with('#') {
            continue;
        }
        let literal = code
            .strip_prefix("puts \"")
            .and_then(|rest| rest.strip_suffix('"'))
            .ok_or_else(|| Diagnostic::new("unexpected token", span))?;
        let value = Expr {
            kind: ExprKind::Str(vec![StrPart::Lit(literal.to_string())]),
            span,
        };
        items.push(Item::Stmt(Stmt {
            kind: StmtKind::Puts(vec![value]),
            span,
        }));
    }
    Ok(items)
}
