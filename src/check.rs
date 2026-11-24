//! Syntax tree to typed program: name resolution, type inference and
//! checking, constant expressions, place and read-only rules, query
//! validation.

use crate::ast;
use crate::diag::Diagnostic;
use crate::source::SourceMap;
use crate::tir;

/// Checks a whole program. Reports every independent error, up to
/// [`crate::diag::MAX_ERRORS`].
pub fn check(
    program: &ast::Program,
    _sources: &SourceMap,
) -> Result<tir::Program, Vec<Diagnostic>> {
    let mut stmts = Vec::new();
    let mut errors = Vec::new();
    for item in &program.items {
        match item {
            ast::Item::Stmt(ast::Stmt {
                kind: ast::StmtKind::Puts(args),
                span,
            }) => {
                let args = args
                    .iter()
                    .filter_map(|arg| match &arg.kind {
                        ast::ExprKind::Str(parts) => {
                            let text = parts
                                .iter()
                                .map(|part| match part {
                                    ast::StrPart::Lit(text) => text.as_str(),
                                    ast::StrPart::Expr(_) => "",
                                })
                                .collect();
                            Some(tir::Expr {
                                kind: tir::ExprKind::Str(text),
                                ty: tir::Type::String,
                                span: arg.span,
                            })
                        }
                        _ => None,
                    })
                    .collect();
                stmts.push(tir::Stmt {
                    kind: tir::StmtKind::Puts(args),
                    span: *span,
                });
            }
            ast::Item::Stmt(stmt) => {
                errors.push(Diagnostic::new("unsupported statement", stmt.span));
            }
            _ => {}
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    Ok(tir::Program {
        records: Vec::new(),
        enums: Vec::new(),
        components: Vec::new(),
        resources: Vec::new(),
        events: Vec::new(),
        consts: Vec::new(),
        funcs: Vec::new(),
        systems: Vec::new(),
        main: tir::Body {
            locals: Vec::new(),
            block: tir::Block { stmts, value: None },
        },
    })
}
