//! The intermediate form as C, and everything else a program's C file
//! holds for its GPU blocks: the 32-bit types and functions, the table of
//! failure sites and, for each block, the function that fills its uniform
//! words, its C implementation, its shader as a string and its
//! `MnGpuBlock`.
//!
//! The C implementation reads and writes columns through the components'
//! own structs. The shader finds the same fields by byte offset, so the
//! offsets it was generated with are asserted against the structs here.
//!
//! 32-bit code is compiled with floating-point contraction off and uses
//! no `double`, so that it computes the same thing at every optimization
//! level. What a run shares with the functions it calls (the uniform
//! words, the error word, the state of `rand`) is passed as `g`, to the
//! functions that need it.

use std::collections::HashSet;
use std::fmt::Write as _;

use crate::tir::{Compact, Program, Type};

use super::super::Cx;
use super::super::names;
use super::helpers;
use super::ir::{BinOp, Cell, Expr, Func, MathFn, Stmt, Storage, Ty, UnOp, Uses, closure};
use super::layout::{Source, gpu_struct};
use super::lower::{Block, Column, INNER_ROW, Lowered, ROW};
use super::wgsl;

const OPEN: &str = "\
/* ---- GPU systems ---- */

#if defined(__clang__)
#pragma STDC FP_CONTRACT OFF
#elif defined(__GNUC__)
#pragma GCC push_options
#pragma GCC optimize(\"fp-contract=off\")
#endif

/* What a run shares with the functions it calls. */
typedef struct Gcx {
    const uint32_t *u; /* the uniform words */
    uint32_t *error;   /* the lowest failure site so far */
    uint32_t key;      /* what `rand` depends on besides the draw count */
    uint32_t draws;    /* random numbers drawn by this run */
} Gcx;

__attribute__((unused)) static inline float g32_float(uint32_t bits) {
    float value;
    memcpy(&value, &bits, sizeof value);
    return value;
}

__attribute__((unused)) static inline uint32_t g32_bits(float value) {
    uint32_t bits;
    memcpy(&bits, &value, sizeof bits);
    return bits;
}

/* An `Int` from outside as GPU code holds it. `what` names it. */
__attribute__((unused)) static inline int32_t g32_int(int64_t value, const char *what, MnLoc loc) {
    if (value < INT32_MIN || value > INT32_MAX) {
        mn_panic(loc, \"%s is %lld, which does not fit in the 32-bit Int of a GPU system\", what,
                 (long long)value);
    }
    return (int32_t)value;
}

__attribute__((unused)) static inline void g32_fail(Gcx *g, uint32_t site) {
    if (site < *g->error) *g->error = site;
}

";

const CLOSE: &str = "\
#if defined(__clang__)
#pragma STDC FP_CONTRACT DEFAULT
#elif defined(__GNUC__)
#pragma GCC pop_options
#endif

";

/// The first line of a strict function: the compiler may not fold or move
/// floating-point operations as if subnormals were kept.
const FENV_ACCESS: &str = "#pragma STDC FENV_ACCESS ON";

/// The name of a block's `MnGpuBlock`.
pub fn block_name(id: u32) -> String {
    format!("Gb{id}")
}

/// The GPU part of a program's C file. Empty for a program without GPU
/// systems.
pub fn section(gpu: &Lowered, cx: &mut Cx) -> String {
    if gpu.blocks.is_empty() {
        return String::new();
    }
    let prog = gpu.prog;
    let mut out = String::from(OPEN);

    // Types.
    let mut all = Uses::default();
    for func in &gpu.library.funcs {
        all.merge(&Uses::of_func(func));
    }
    for block in &gpu.blocks {
        all.merge(&block.uses);
    }
    let (records, _) = wgsl::struct_order(gpu, all.records.iter().copied());
    for &record in &records {
        let item = gpu_struct(prog, record);
        let _ = writeln!(out, "typedef struct {} {{", item.name);
        for (name, ty) in &item.fields {
            let _ = writeln!(out, "    {} {name};", cty(gpu, *ty));
        }
        let _ = writeln!(out, "}} {};\n", item.name);
    }
    out.push_str(&layout_asserts(gpu));

    // Functions.
    let state: HashSet<String> = gpu
        .library
        .funcs
        .iter()
        .filter(|func| closure(&gpu.library, &Uses::of_func(func)).needs_state())
        .map(|func| func.name.clone())
        .collect();
    let mut printer = Printer {
        gpu,
        state: &state,
        in_block: false,
        out: String::new(),
        indent: 0,
    };
    let funcs: Vec<&Func> = gpu
        .library
        .funcs
        .iter()
        .filter(|func| !helpers::SHADER_ONLY.contains(&func.name.as_str()))
        .collect();
    for func in &funcs {
        let header = printer.header(func);
        let _ = writeln!(out, "{header};");
    }
    if !funcs.is_empty() {
        out.push('\n');
    }
    for func in &funcs {
        printer.function(func);
    }
    out.push_str(&printer.out);

    // Failure sites.
    if !gpu.sites.is_empty() {
        out.push_str("static const MnGpuSite gpu_sites[] = {\n");
        for site in &gpu.sites {
            let file = cx.file(site.span);
            let _ = writeln!(
                out,
                "    {{{{{file}, {}}}, {}}},",
                cx.sources.line(site.span),
                names::c_string(site.message.as_bytes())
            );
        }
        out.push_str("};\n\n");
    }

    for block in &gpu.blocks {
        let outer_query = cx.query(block.outer_query);
        let inner_query = block.inner_query.map(|query| cx.query(query));
        let name = block_name(block.id);

        let fill = fill(cx, gpu, block);
        out.push_str(&fill);

        let mut printer = Printer {
            gpu,
            state: &state,
            in_block: true,
            out: String::new(),
            indent: 0,
        };
        printer.cpu(block);
        out.push_str(&printer.out);

        let columns = |out: &mut String, which: &str, columns: &[Column]| {
            if columns.is_empty() {
                return "NULL".to_string();
            }
            let _ = writeln!(out, "static const MnGpuColumn {name}_{which}[] = {{");
            for column in columns {
                let (id, cty) = match column.component {
                    Some(component) => (component.to_string(), component_cty(prog, component)),
                    None => ("MN_GPU_ENTITIES".to_string(), "MnEntity".to_string()),
                };
                let _ = writeln!(
                    out,
                    "    {{{id}, sizeof({cty}), {}, {}}},",
                    column.write, column.overwrite
                );
            }
            out.push_str("};\n");
            format!("{name}_{which}")
        };
        let outer = columns(&mut out, "outer", &block.outer);
        let inner = columns(&mut out, "inner", &block.inner);

        let _ = writeln!(out, "static const char {name}_wgsl[] =");
        let shader = wgsl::module(gpu, block);
        for line in shader.lines() {
            let _ = writeln!(
                out,
                "    {}",
                names::c_string(format!("{line}\n").as_bytes())
            );
        }
        out.push_str("    ;\n");

        let _ = writeln!(out, "static MnGpuBlock {name} = {{");
        let _ = writeln!(
            out,
            "    .system = {},",
            names::c_string(block.system.as_bytes())
        );
        let _ = writeln!(out, "    .each = {},", block.each);
        let _ = writeln!(out, "    .id = {},", block.id);
        let _ = writeln!(out, "    .outer = &{outer_query},");
        let _ = writeln!(out, "    .outer_columns = {outer},");
        let _ = writeln!(out, "    .outer_count = {},", block.outer.len());
        match inner_query {
            Some(query) => {
                let _ = writeln!(out, "    .inner = &{query},");
            }
            None => out.push_str("    .inner = NULL,\n"),
        }
        let _ = writeln!(out, "    .inner_columns = {inner},");
        let _ = writeln!(out, "    .inner_count = {},", block.inner.len());
        let _ = writeln!(out, "    .uniform_words = {},", block.uniform_words);
        if fill.is_empty() {
            out.push_str("    .fill = NULL,\n");
        } else {
            let _ = writeln!(out, "    .fill = {name}_fill,");
        }
        let _ = writeln!(out, "    .cpu = {name}_cpu,");
        let _ = writeln!(out, "    .wgsl = {name}_wgsl,");
        let _ = writeln!(out, "    .fallible = {},", block.uses.fails);
        let sites = if block.uses.fails {
            "gpu_sites"
        } else {
            "NULL"
        };
        let _ = writeln!(out, "    .sites = {sites},");
        if block.strict {
            out.push_str("    .strict = true,\n");
        }
        out.push_str("    .state = NULL,\n};\n\n");
    }
    out.push_str(CLOSE);
    out
}

fn component_cty(prog: &Program, component: usize) -> String {
    let record = prog.components[component]
        .record
        .expect("a tag is not a query term");
    names::record(&prog.records[record.0].name)
}

fn cty(gpu: &Lowered, ty: Ty) -> String {
    match ty {
        Ty::Void => "void".to_string(),
        Ty::I32 => "int32_t".to_string(),
        Ty::U32 => "uint32_t".to_string(),
        Ty::F32 => "float".to_string(),
        Ty::Bool => "bool".to_string(),
        Ty::Entity => "MnEntity".to_string(),
        Ty::Record(id) => gpu_struct(gpu.prog, id).name,
    }
}

/// Assertions that the C structs of the components GPU blocks name, and
/// of the records they hold, are laid out as the shaders assume.
fn layout_asserts(gpu: &Lowered) -> String {
    fn visit(gpu: &Lowered, record: usize, done: &mut Vec<usize>, out: &mut String) {
        if done.contains(&record) {
            return;
        }
        done.push(record);
        let prog = gpu.prog;
        let item = &prog.records[record];
        let layout = gpu.layouts().record(record);
        let name = names::record(&item.name);
        let _ = writeln!(
            out,
            "_Static_assert(sizeof({name}) == {}, \"size of {}\");",
            layout.size, item.name
        );
        for (index, field) in item.fields.iter().enumerate() {
            let _ = writeln!(
                out,
                "_Static_assert(offsetof({name}, {}) == {}, \"offset of {}.{}\");",
                names::field(&field.name),
                layout.offsets[index],
                item.name,
                field.name
            );
        }
        for field in &item.fields {
            if let Type::Record(inner) = &field.ty {
                visit(gpu, inner.0, done, out);
            }
        }
    }
    let mut components: Vec<usize> = gpu
        .blocks
        .iter()
        .flat_map(|block| {
            let terms = |query: &crate::tir::Query| {
                query
                    .terms
                    .iter()
                    .filter_map(|term| match term {
                        crate::tir::Term::Component { component, .. } => Some(component.0),
                        crate::tir::Term::Entity => None,
                    })
                    .collect::<Vec<usize>>()
            };
            let mut all = terms(block.outer_query);
            if let Some(query) = block.inner_query {
                all.extend(terms(query));
            }
            all
        })
        .collect();
    components.sort_unstable();
    components.dedup();
    let mut out = String::new();
    let mut done = Vec::new();
    for component in components {
        if let Some(record) = gpu.prog.components[component].record {
            visit(gpu, record.0, &mut done, &mut out);
        }
    }
    if !out.is_empty() {
        out.push('\n');
    }
    out
}

/// Whether a value of this type has an `Int` in it.
fn has_int(prog: &Program, ty: &Type) -> bool {
    match ty {
        Type::Int => true,
        Type::Record(id) => prog.records[id.0]
            .fields
            .iter()
            .any(|field| has_int(prog, &field.ty)),
        _ => false,
    }
}

/// The function that writes a block's uniform words after the header.
/// Empty when the block has none. An `Int` that does not fit in 32 bits
/// stops the program there, at the line of the block's `each`.
fn fill(cx: &mut Cx, gpu: &Lowered, block: &Block) -> String {
    /// Writes the words of `value`, which the source calls `what`.
    fn words(
        prog: &Program,
        ty: &Type,
        value: &str,
        what: &str,
        loc: &str,
        word: &mut u32,
        out: &mut String,
    ) {
        let mut put = |out: &mut String, bits: String| {
            let _ = writeln!(out, "    words[{word}] = {bits};");
            *word += 1;
        };
        match ty {
            Type::Float => put(out, format!("g32_bits((float){value})")),
            Type::Compact(Compact::Float32) => put(out, format!("g32_bits({value})")),
            Type::Int => put(
                out,
                format!(
                    "(uint32_t)g32_int({value}, {}, {loc})",
                    names::c_string(what.as_bytes())
                ),
            ),
            Type::Compact(_) | Type::Bool | Type::Enum(_) => {
                put(out, format!("(uint32_t){value}"));
            }
            Type::Entity => {
                put(out, format!("{value}.index"));
                put(out, format!("{value}.generation"));
            }
            Type::Record(id) => {
                for field in &prog.records[id.0].fields {
                    let member = format!("{value}.{}", names::field(&field.name));
                    let what = format!("{what}.{}", field.name);
                    words(prog, &field.ty, &member, &what, loc, word, out);
                }
            }
            Type::Void | Type::String | Type::List(_) => unreachable!(
                "the checker lets GPU code read only plain constants and resource fields"
            ),
        }
    }
    if block.entries.is_empty() {
        return String::new();
    }
    let prog = gpu.prog;
    // The source file is named only by a block that has an `Int` to check.
    let checks = block.entries.iter().any(|&entry| {
        let ty = match gpu.uniforms.entries[entry].source {
            Source::Const(id) => &prog.consts[id].ty,
            Source::Resource(resource, index) => &prog.resources[resource].fields[index].ty,
        };
        has_int(prog, ty)
    });
    let loc = if checks {
        format!(
            "(MnLoc){{{}, {}}}",
            cx.file(block.span),
            cx.sources.line(block.span)
        )
    } else {
        String::new()
    };
    let mut out = format!(
        "static void {}_fill(uint32_t *words) {{\n",
        block_name(block.id)
    );
    for &entry in &block.entries {
        let entry = &gpu.uniforms.entries[entry];
        let mut word = entry.base;
        match entry.source {
            Source::Const(id) => {
                let item = &prog.consts[id];
                let value = names::constant(&item.name);
                words(
                    prog, &item.ty, &value, &item.name, &loc, &mut word, &mut out,
                );
            }
            Source::Resource(resource, index) => {
                let item = &prog.resources[resource];
                let field = &item.fields[index];
                let value = format!(
                    "{}.{}",
                    names::resource(&item.name),
                    names::field(&field.name)
                );
                let what = format!("{}.{}", item.name, field.name);
                words(prog, &field.ty, &value, &what, &loc, &mut word, &mut out);
            }
        }
    }
    out.push_str("}\n\n");
    out
}

struct Printer<'g, 'a> {
    gpu: &'g Lowered<'a>,
    /// The library functions that take `g`.
    state: &'g HashSet<String>,
    /// In a block's C implementation, where the uniform words it reads
    /// are in locals; otherwise in a library function, which reads them
    /// through `g`.
    in_block: bool,
    out: String,
    indent: usize,
}

fn symbol(op: BinOp) -> &'static str {
    match op {
        BinOp::Add => "+",
        BinOp::Sub => "-",
        BinOp::Mul => "*",
        BinOp::Div => "/",
        BinOp::Rem => "%",
        BinOp::Eq => "==",
        BinOp::Ne => "!=",
        BinOp::Lt => "<",
        BinOp::Le => "<=",
        BinOp::Gt => ">",
        BinOp::Ge => ">=",
        BinOp::And => "&&",
        BinOp::Or => "||",
        BinOp::BitAnd => "&",
        BinOp::BitXor => "^",
        BinOp::Shl => "<<",
        BinOp::Shr => ">>",
    }
}

/// The C expression for one uniform word as its type.
fn uniform_word(words: &str, word: u32, ty: Ty) -> String {
    match ty {
        Ty::F32 => format!("g32_float({words}[{word}])"),
        Ty::I32 => format!("(int32_t){words}[{word}]"),
        _ => format!("{words}[{word}]"),
    }
}

impl Printer<'_, '_> {
    fn line(&mut self, text: &str) {
        for _ in 0..self.indent {
            self.out.push_str("    ");
        }
        self.out.push_str(text);
        self.out.push('\n');
    }

    fn ty(&self, ty: Ty) -> String {
        cty(self.gpu, ty)
    }

    /// An expression as an operand: in parentheses unless it is a name,
    /// a literal, a cast or a call.
    fn expr(&self, expr: &Expr) -> String {
        match expr {
            Expr::Unary(..) | Expr::Binary(..) => format!("({})", self.top(expr)),
            Expr::Opaque(_, inner) => self.expr(inner),
            _ => self.top(expr),
        }
    }

    /// An expression where nothing surrounds it.
    fn top(&self, expr: &Expr) -> String {
        match expr {
            Expr::I32(i32::MIN) => "(-2147483647 - 1)".to_string(),
            Expr::I32(value) if *value < 0 => format!("({value})"),
            Expr::I32(value) => value.to_string(),
            Expr::U32(value) => format!("{value}u"),
            Expr::Hex(value) => format!("{value:#x}u"),
            Expr::F32(value) if value.is_sign_negative() => format!("({value:?}f)"),
            Expr::F32(value) => format!("{value:?}f"),
            Expr::F32Bits(bits) => format!("g32_float({bits:#x}u)"),
            Expr::Bool(value) => value.to_string(),
            Expr::Var(name) => name.clone(),
            Expr::Field(base, name) => format!("{}.{name}", self.expr(base)),
            Expr::Uniform(word, _) if self.in_block => format!("u{word}"),
            Expr::Uniform(word, ty) => uniform_word("g->u", *word, *ty),
            Expr::Key => "g->key".to_string(),
            Expr::Draws => "g->draws".to_string(),
            Expr::Site(site) => format!("{}u", self.gpu.site_index[*site]),
            Expr::Load(cell) => {
                let place = self.cell(cell);
                match cell.storage {
                    Storage::I8 | Storage::I16 | Storage::U8 | Storage::U16 | Storage::U32 => {
                        format!("(int32_t){place}")
                    }
                    Storage::Enum(_) => format!("(uint32_t){place}"),
                    Storage::F32 | Storage::I32 | Storage::Bool | Storage::Entity => place,
                }
            }
            Expr::Unary(op, inner) => {
                let symbol = match op {
                    UnOp::Neg => "-",
                    UnOp::Not => "!",
                };
                format!("{symbol}{}", self.expr(inner))
            }
            Expr::Binary(op, lhs, rhs) => {
                format!("{} {} {}", self.expr(lhs), symbol(*op), self.expr(rhs))
            }
            Expr::Select { cond, then, else_ } => format!(
                "({} ? {} : {})",
                self.expr(cond),
                self.expr(then),
                self.expr(else_)
            ),
            Expr::Cast(ty, inner) => format!("({}){}", self.ty(*ty), self.expr(inner)),
            Expr::Bits(Ty::F32, inner) => format!("g32_float({})", self.top(inner)),
            Expr::Bits(_, inner) => format!("g32_bits({})", self.top(inner)),
            Expr::Math(function, args) => {
                let name = match function {
                    MathFn::Sqrt => "sqrtf",
                    MathFn::Sin => "sinf",
                    MathFn::Cos => "cosf",
                    MathFn::Tan => "tanf",
                    MathFn::Atan2 => "atan2f",
                    MathFn::Exp => "expf",
                    MathFn::Log => "logf",
                    MathFn::Pow => "powf",
                    MathFn::Abs => "fabsf",
                    MathFn::Floor => "floorf",
                    MathFn::Ceil => "ceilf",
                    MathFn::Trunc => "truncf",
                };
                format!("{name}({})", self.args(args, false))
            }
            // In strict code `/` and `sqrt` are the hardware's, which flush
            // subnormals while a strict block runs.
            Expr::Call(name, args) if name == helpers::DIV_F => {
                format!("({} / {})", self.expr(&args[0]), self.expr(&args[1]))
            }
            Expr::Call(name, args) if name == helpers::SQRT_F => {
                format!("sqrtf({})", self.top(&args[0]))
            }
            Expr::Call(name, args) => {
                format!("{name}({})", self.args(args, self.state.contains(name)))
            }
            Expr::Construct(ty, args) => {
                format!("({}){{{}}}", self.ty(*ty), self.args(args, false))
            }
            Expr::Opaque(_, inner) => self.top(inner),
        }
    }

    fn args(&self, args: &[Expr], state: bool) -> String {
        let mut list: Vec<String> = Vec::new();
        if state {
            list.push("g".to_string());
        }
        list.extend(args.iter().map(|arg| self.top(arg)));
        list.join(", ")
    }

    /// The lvalue of a field of a row.
    fn cell(&self, cell: &Cell) -> String {
        let (array, row) = if cell.inner {
            ("s", INNER_ROW)
        } else {
            ("c", ROW)
        };
        format!("{array}{}[{row}]{}", cell.column, cell.member)
    }

    fn block(&mut self, stmts: &[Stmt]) {
        self.indent += 1;
        self.stmts(stmts);
        self.indent -= 1;
    }

    fn stmts(&mut self, stmts: &[Stmt]) {
        for stmt in stmts {
            self.stmt(stmt);
        }
    }

    fn stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Let {
                name,
                ty,
                init,
                unused,
            } => {
                let init = match (init, ty) {
                    (Some(init), _) => self.top(init),
                    (None, Ty::Entity | Ty::Record(_)) => "{0}".to_string(),
                    (None, Ty::F32) => "0.0f".to_string(),
                    (None, Ty::Bool) => "false".to_string(),
                    (None, _) => "0".to_string(),
                };
                let mark = if *unused {
                    " __attribute__((unused))"
                } else {
                    ""
                };
                let line = format!("{} {name}{mark} = {init};", self.ty(*ty));
                self.line(&line);
            }
            Stmt::Assign(target, value) => {
                let line = format!("{} = {};", self.top(target), self.top(value));
                self.line(&line);
            }
            Stmt::Store(cell, value) => {
                let cast = match cell.storage {
                    Storage::I8 => "(int8_t)",
                    Storage::I16 => "(int16_t)",
                    Storage::U8 | Storage::Enum(1) => "(uint8_t)",
                    Storage::U16 | Storage::Enum(2) => "(uint16_t)",
                    Storage::U32 => "(uint32_t)",
                    Storage::F32
                    | Storage::I32
                    | Storage::Bool
                    | Storage::Enum(_)
                    | Storage::Entity => "",
                };
                let line = format!("{} = {cast}{};", self.cell(cell), self.expr(value));
                self.line(&line);
            }
            Stmt::If(..) => {
                let mut current = stmt;
                let mut keyword = "if";
                while let Stmt::If(cond, then, else_) = current {
                    let cond = self.top(cond);
                    self.line(&format!("{keyword} ({cond}) {{"));
                    self.block(then);
                    match else_.as_slice() {
                        [] => break,
                        [next @ Stmt::If(..)] => {
                            keyword = "} else if";
                            current = next;
                        }
                        _ => {
                            self.line("} else {");
                            self.block(else_);
                            break;
                        }
                    }
                }
                self.line("}");
            }
            Stmt::While(cond, body) => {
                let cond = self.top(cond);
                self.line(&format!("while ({cond}) {{"));
                self.block(body);
                self.line("}");
            }
            Stmt::Loop(body) => {
                self.line("for (;;) {");
                self.block(body);
                self.line("}");
            }
            Stmt::Once(body) => {
                self.line("do {");
                self.block(body);
                self.line("} while (0);");
            }
            Stmt::For {
                var,
                ty,
                from,
                op,
                to,
                down,
                guard,
                body,
            } => {
                let step = if *down { "--" } else { "++" };
                match guard {
                    Some(wide) => {
                        // Counts in 64 bits, which can't step past a 32-bit
                        // bound.
                        let line = format!(
                            "for (int64_t {wide} = {}; {wide} {} {}; {wide}{step}) {{",
                            self.top(from),
                            symbol(*op),
                            self.expr(to),
                        );
                        self.line(&line);
                        let line = format!(
                            "    {ty} {var} __attribute__((unused)) = ({ty}){wide};",
                            ty = self.ty(*ty)
                        );
                        self.line(&line);
                    }
                    None => {
                        let line = format!(
                            "for ({} {var} = {}; {var} {} {}; {var}{step}) {{",
                            self.ty(*ty),
                            self.top(from),
                            symbol(*op),
                            self.expr(to),
                        );
                        self.line(&line);
                    }
                }
                self.block(body);
                self.line("}");
            }
            Stmt::Break => self.line("break;"),
            Stmt::Continue => self.line("continue;"),
            Stmt::Return(None) => self.line("return;"),
            Stmt::Return(Some(value)) => {
                let value = self.top(value);
                self.line(&format!("return {value};"));
            }
            Stmt::Call(call) => {
                let call = self.top(call);
                self.line(&format!("{call};"));
            }
            Stmt::Discard(value) => {
                let value = self.expr(value);
                self.line(&format!("(void){value};"));
            }
            Stmt::Fail(site) => {
                let site = self.top(site);
                self.line(&format!("g32_fail(g, {site});"));
            }
        }
    }

    fn header(&self, func: &Func) -> String {
        let mut params: Vec<String> = Vec::new();
        if self.state.contains(&func.name) {
            params.push("Gcx *g".to_string());
        }
        for param in &func.params {
            let mark = if param.unused {
                " __attribute__((unused))"
            } else {
                ""
            };
            params.push(format!("{} {}{mark}", self.ty(param.ty), param.name));
        }
        if params.is_empty() {
            params.push("void".to_string());
        }
        format!(
            "static {} {}({})",
            self.ty(func.ret),
            func.name,
            params.join(", ")
        )
    }

    fn function(&mut self, func: &Func) {
        let header = self.header(func);
        self.line(&format!("{header} {{"));
        if func.strict {
            self.line(FENV_ACCESS);
        }
        self.block(&func.body);
        self.line("}");
        self.out.push('\n');
    }

    /// A block's C implementation: every row of one table.
    fn cpu(&mut self, block: &Block) {
        let prog = self.gpu.prog;
        let direct = Uses::of_stmts(&block.body);
        let name = block_name(block.id);
        self.line(&format!("static void {name}_cpu(const MnGpuRun *run) {{"));
        self.indent = 1;
        if block.strict {
            self.line(FENV_ACCESS);
        }
        self.line("const uint32_t rows = run->uniform[0];");
        let state = direct.rand_state
            || direct.fails
            || direct.calls.iter().any(|name| self.state.contains(name));
        if state {
            self.line("Gcx run_state = {run->uniform, run->error, 0, 0};");
            self.line("Gcx *const g = &run_state;");
        }
        for &word in &direct.uniforms {
            let ty = self.gpu.uniforms.words[word as usize];
            let line = format!(
                "const {} u{word} = {};",
                self.ty(ty),
                uniform_word("run->uniform", word, ty)
            );
            self.line(&line);
        }
        for (index, column) in block.outer.iter().enumerate() {
            let line = match column.component {
                Some(component) if column.write => format!(
                    "{} *restrict c{index} = run->outer[{index}];",
                    component_cty(prog, component)
                ),
                Some(component) => format!(
                    "const {} *c{index} = run->outer[{index}];",
                    component_cty(prog, component)
                ),
                None => format!("const MnEntity *c{index} = run->outer[{index}];"),
            };
            self.line(&line);
        }
        for (index, column) in block.inner.iter().enumerate() {
            let cty = match column.component {
                Some(component) => component_cty(prog, component),
                None => "MnEntity".to_string(),
            };
            self.line(&format!("const {cty} *s{index} = run->inner[{index}];"));
        }
        if block.strict {
            self.line("const uint64_t fp_state = mn_strict_enter();");
        }
        self.line(&format!(
            "for (uint32_t {ROW} = 0; {ROW} < rows; {ROW}++) {{"
        ));
        self.block(&block.body);
        self.line("}");
        if block.strict {
            self.line("mn_strict_leave(fp_state);");
        }
        self.indent = 0;
        self.line("}");
        self.out.push('\n');
    }
}

/// A library function as C, the strict `/` and `sqrt` included.
#[cfg(test)]
pub fn function(gpu: &Lowered, name: &str) -> String {
    let state = HashSet::new();
    let mut printer = Printer {
        gpu,
        state: &state,
        in_block: false,
        out: String::new(),
        indent: 0,
    };
    printer.function(gpu.library.get(name));
    printer.out
}
