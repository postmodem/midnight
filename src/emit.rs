//! Typed program to C: one translation unit for the whole program.

use crate::source::SourceMap;
use crate::tir;

/// Generates the C for a program.
pub fn emit(program: &tir::Program, sources: &SourceMap) -> String {
    let mut literals = String::new();
    let mut body = String::new();
    let mut count = 0;
    for stmt in &program.main.block.stmts {
        let tir::StmtKind::Puts(args) = &stmt.kind else {
            continue;
        };
        let file = &sources.file(stmt.span.file).name;
        body.push_str(&format!("#line {} \"{file}\"\n", sources.line(stmt.span)));
        for arg in args {
            if let tir::ExprKind::Str(text) = &arg.kind
                && !text.is_empty()
            {
                literals.push_str(&format!(
                    "GR_STR_LIT(s{count}, {}, \"{}\");\n",
                    text.len(),
                    text.escape_default()
                ));
                body.push_str(&format!("    gr_out_str((GrString)&s{count});\n"));
                count += 1;
            }
            body.push_str("    gr_out_newline();\n");
        }
        if args.is_empty() {
            body.push_str("    gr_out_newline();\n");
        }
    }
    format!(
        "#include \"gremlin_rt.h\"\n\n\
         {literals}\n\
         static void gr_drop_globals(void) {{}}\n\n\
         static const GrProgram gr_program = {{NULL, 0, NULL, 0, NULL, 0, gr_drop_globals}};\n\n\
         int main(void) {{\n    gr_start(&gr_program);\n{body}    gr_finish();\n    return 0;\n}}\n"
    )
}
