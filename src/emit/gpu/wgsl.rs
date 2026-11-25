//! The intermediate form as WGSL: one module per block.
//!
//! A module declares, in this order: the uniform words, the types the
//! block uses, its bindings (the uniform words, each outer column, each
//! inner column, then the error word if the block can fail), the state of
//! `rand`, the functions the block reaches, and the entry point `main`,
//! which runs one row.
//!
//! A column is an `array<u32>` holding the bytes of the C array. A field
//! is read from the word that holds it by shifting and masking, and a
//! field narrower than a word is written by replacing its bits in that
//! word. In a column the block does not write, a row can be any number of
//! bytes, so the word and the shift of a one- or two-byte field are worked
//! out from the row.

use std::fmt::Write as _;

use super::ir::{BinOp, Cell, Expr, Func, MathFn, Stmt, Storage, Ty, UnOp};
use super::layout::gpu_struct;
use super::lower::{Block, INNER_ROW, Lowered, ROW};

/// Rows a dispatch covers along x: MN_GPU_ROW_SPAN.
const ROW_SPAN: u32 = 65535 * 64;

/// Every block's shader, each after a comment line that names it.
pub fn listing(gpu: &Lowered) -> String {
    let mut out = String::new();
    for block in &gpu.blocks {
        if block.id > 0 {
            out.push('\n');
        }
        let _ = writeln!(out, "// system {}, each {}", block.system, block.each);
        out.push_str(&module(gpu, block));
    }
    out
}

/// The records a piece of code names, each after the records it holds,
/// and whether any of it holds an entity.
pub fn struct_order(gpu: &Lowered, records: impl Iterator<Item = usize>) -> (Vec<usize>, bool) {
    fn visit(gpu: &Lowered, record: usize, order: &mut Vec<usize>, entity: &mut bool) {
        if order.contains(&record) {
            return;
        }
        for (_, ty) in gpu_struct(gpu.prog, record).fields {
            match ty {
                Ty::Record(inner) => visit(gpu, inner, order, entity),
                Ty::Entity => *entity = true,
                _ => {}
            }
        }
        order.push(record);
    }
    let mut order = Vec::new();
    let mut entity = false;
    for record in records {
        visit(gpu, record, &mut order, &mut entity);
    }
    (order, entity)
}

/// The shader of one block.
pub fn module(gpu: &Lowered, block: &Block) -> String {
    let mut printer = Printer {
        gpu,
        out: String::new(),
        indent: 0,
    };
    let p = &mut printer;

    p.line("struct Words {");
    for word in 0..block.uniform_words {
        let ty = p.ty(gpu.uniforms.words[word as usize]);
        p.line(&format!("    w{word}: {ty},"));
    }
    p.line("}");
    let (records, holds_entity) = struct_order(gpu, block.uses.records.iter().copied());
    if block.uses.entity || holds_entity {
        p.line("struct MnEntity {");
        p.line("    index: u32,");
        p.line("    generation: u32,");
        p.line("}");
    }
    for record in records {
        let item = gpu_struct(gpu.prog, record);
        p.line(&format!("struct {} {{", item.name));
        for (name, ty) in &item.fields {
            let ty = p.ty(*ty);
            p.line(&format!("    {name}: {ty},"));
        }
        p.line("}");
    }
    p.out.push('\n');

    let mut binding = 0;
    let mut bind = |p: &mut Printer, declaration: &str| {
        p.line(&format!("@group(0) @binding({binding}) {declaration}"));
        binding += 1;
    };
    bind(p, "var<uniform> u: Words;");
    for (index, column) in block.outer.iter().enumerate() {
        let access = if column.write { "read_write" } else { "read" };
        bind(p, &format!("var<storage, {access}> c{index}: array<u32>;"));
    }
    for index in 0..block.inner.len() {
        bind(p, &format!("var<storage, read> s{index}: array<u32>;"));
    }
    if block.uses.fails {
        bind(p, "var<storage, read_write> g_error: atomic<u32>;");
    }
    if block.uses.rand_state {
        p.line("var<private> g_key: u32;");
        p.line("var<private> g_draws: u32;");
    }
    p.out.push('\n');

    for func in &gpu.library.funcs {
        if block.uses.calls.contains(&func.name) {
            p.function(func);
            p.out.push('\n');
        }
    }

    p.line("@compute @workgroup_size(64)");
    p.line("fn main(@builtin(global_invocation_id) gid: vec3<u32>) {");
    p.indent = 1;
    p.line(&format!("let {ROW} = gid.x + gid.y * {ROW_SPAN}u;"));
    p.line(&format!("if ({ROW} >= u.w0) {{"));
    p.line("    return;");
    p.line("}");
    p.stmts(&block.body);
    p.indent = 0;
    p.line("}");
    printer.out
}

/// Where a field's bits start in the word that holds them.
enum Shift {
    Fixed(u32),
    /// An expression: it depends on the row.
    Computed(String),
}

struct Printer<'g, 'a> {
    gpu: &'g Lowered<'a>,
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

/// Whether control never reaches the end of these statements.
fn leaves(stmts: &[Stmt]) -> bool {
    matches!(
        stmts.last(),
        Some(Stmt::Break | Stmt::Continue | Stmt::Return(_))
    )
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
        match ty {
            Ty::Void => unreachable!("no value has no type"),
            Ty::I32 => "i32".to_string(),
            Ty::U32 => "u32".to_string(),
            Ty::F32 => "f32".to_string(),
            Ty::Bool => "bool".to_string(),
            Ty::Entity => "MnEntity".to_string(),
            Ty::Record(id) => gpu_struct(self.gpu.prog, id).name,
        }
    }

    /// An expression as an operand: in parentheses unless it is a name,
    /// a literal or a call.
    fn expr(&self, expr: &Expr) -> String {
        match expr {
            Expr::Unary(..) | Expr::Binary(..) => format!("({})", self.top(expr)),
            _ => self.top(expr),
        }
    }

    /// An expression where nothing surrounds it.
    fn top(&self, expr: &Expr) -> String {
        match expr {
            Expr::I32(i32::MIN) => "(-2147483647i - 1i)".to_string(),
            Expr::I32(value) if *value < 0 => format!("({value}i)"),
            Expr::I32(value) => format!("{value}i"),
            Expr::U32(value) => format!("{value}u"),
            Expr::Hex(value) => format!("{value:#x}u"),
            Expr::F32(value) if value.is_sign_negative() => format!("({value:?}f)"),
            Expr::F32(value) => format!("{value:?}f"),
            Expr::F32Bits(bits) => format!("bitcast<f32>({bits:#x}u)"),
            Expr::Bool(value) => value.to_string(),
            Expr::Var(name) => name.clone(),
            Expr::Field(base, name) => format!("{}.{name}", self.expr(base)),
            Expr::Uniform(word, _) => format!("u.w{word}"),
            Expr::Key => "g_key".to_string(),
            Expr::Draws => "g_draws".to_string(),
            Expr::Site(site) => format!("{}u", self.gpu.site_index[*site]),
            Expr::Load(cell) => self.load(cell),
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
                "select({}, {}, {})",
                self.top(else_),
                self.top(then),
                self.top(cond)
            ),
            Expr::Cast(ty, inner) => format!("{}({})", self.ty(*ty), self.top(inner)),
            Expr::Bits(ty, inner) => format!("bitcast<{}>({})", self.ty(*ty), self.top(inner)),
            Expr::Math(function, args) => {
                let name = match function {
                    MathFn::Sqrt => "sqrt",
                    MathFn::Sin => "sin",
                    MathFn::Cos => "cos",
                    MathFn::Tan => "tan",
                    MathFn::Atan2 => "atan2",
                    MathFn::Exp => "exp",
                    MathFn::Log => "log",
                    MathFn::Pow => "pow",
                    MathFn::Abs => "abs",
                    MathFn::Floor => "floor",
                    MathFn::Ceil => "ceil",
                    MathFn::Trunc => "trunc",
                };
                format!("{name}({})", self.args(args))
            }
            Expr::Call(name, args) => format!("{name}({})", self.args(args)),
            Expr::Construct(ty, args) => format!("{}({})", self.ty(*ty), self.args(args)),
        }
    }

    fn args(&self, args: &[Expr]) -> String {
        let list: Vec<String> = args.iter().map(|arg| self.top(arg)).collect();
        list.join(", ")
    }

    /// The element of the column's array that holds the byte `extra`
    /// bytes into a field, and where in that word the byte is.
    fn locate(&self, cell: &Cell, extra: u32) -> (String, Shift) {
        let row = if cell.inner { INNER_ROW } else { ROW };
        let array = if cell.inner { "s" } else { "c" };
        let offset = cell.offset + extra;
        if cell.row_size.is_multiple_of(4) {
            let index = match (cell.row_size / 4, offset / 4) {
                (1, 0) => row.to_string(),
                (1, word) => format!("{row} + {word}u"),
                (words, 0) => format!("{row} * {words}u"),
                (words, word) => format!("{row} * {words}u + {word}u"),
            };
            return (
                format!("{array}{}[{index}]", cell.column),
                Shift::Fixed(offset % 4 * 8),
            );
        }
        let byte = match (cell.row_size, offset) {
            (1, 0) => row.to_string(),
            (size, 0) => format!("{row} * {size}u"),
            (size, offset) => format!("{row} * {size}u + {offset}u"),
        };
        (
            format!("{array}{}[({byte}) >> 2u]", cell.column),
            Shift::Computed(format!("(({byte}) & 3u) << 3u")),
        )
    }

    /// The value of a field, as the type its storage loads as.
    fn load(&self, cell: &Cell) -> String {
        let (word, shift) = self.locate(cell, 0);
        if cell.storage.bytes() >= 4 {
            assert!(
                matches!(shift, Shift::Fixed(0)),
                "a field of whole words starts at a word"
            );
        }
        // The field's bits at the bottom of a word, under whatever
        // follows it.
        let low = match &shift {
            Shift::Fixed(0) => word.clone(),
            Shift::Fixed(bits) => format!("({word} >> {bits}u)"),
            Shift::Computed(bits) => format!("({word} >> ({bits}))"),
        };
        match cell.storage {
            Storage::F32 => format!("bitcast<f32>({word})"),
            Storage::I32 => format!("bitcast<i32>({word})"),
            Storage::Enum(4) => word,
            Storage::U8 => format!("i32({low} & 0xffu)"),
            Storage::U16 => format!("i32({low} & 0xffffu)"),
            Storage::I8 => format!("(i32({low} << 24u) >> 24u)"),
            Storage::I16 => format!("(i32({low} << 16u) >> 16u)"),
            Storage::Bool => format!("(({low} & 0xffu) != 0u)"),
            Storage::Enum(1) => format!("({low} & 0xffu)"),
            Storage::Enum(_) => format!("({low} & 0xffffu)"),
            Storage::Entity => {
                let (second, _) = self.locate(cell, 4);
                format!("MnEntity({word}, {second})")
            }
        }
    }

    /// Writes a field. The column's rows are whole words.
    fn store(&mut self, cell: &Cell, value: &Expr) {
        let (word, shift) = self.locate(cell, 0);
        let Shift::Fixed(shift) = shift else {
            unreachable!("a column a block writes has rows of whole words");
        };
        let (mask, bits) = match cell.storage {
            Storage::F32 | Storage::I32 => {
                let value = self.top(value);
                self.line(&format!("{word} = bitcast<u32>({value});"));
                return;
            }
            Storage::Enum(4) => {
                let value = self.top(value);
                self.line(&format!("{word} = {value};"));
                return;
            }
            Storage::Entity => {
                let (second, _) = self.locate(cell, 4);
                let value = self.expr(value);
                self.line(&format!("{word} = {value}.index;"));
                self.line(&format!("{second} = {value}.generation;"));
                return;
            }
            Storage::I8 | Storage::U8 => (0xffu32, format!("bitcast<u32>({})", self.top(value))),
            Storage::I16 | Storage::U16 => {
                (0xffffu32, format!("bitcast<u32>({})", self.top(value)))
            }
            Storage::Bool => (0xff, format!("u32({})", self.top(value))),
            Storage::Enum(1) => (0xff, self.expr(value)),
            Storage::Enum(_) => (0xffff, self.expr(value)),
        };
        let keep = !(mask << shift);
        let placed = if shift == 0 {
            format!("({bits} & {mask:#x}u)")
        } else {
            format!("(({bits} & {mask:#x}u) << {shift}u)")
        };
        self.line(&format!("{word} = ({word} & {keep:#x}u) | {placed};"));
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
            Stmt::Let { name, ty, init, .. } => {
                let ty = self.ty(*ty);
                match init {
                    Some(init) => {
                        let init = self.top(init);
                        self.line(&format!("var {name}: {ty} = {init};"));
                    }
                    None => self.line(&format!("var {name}: {ty};")),
                }
            }
            Stmt::Assign(target, value) => {
                let line = format!("{} = {};", self.top(target), self.top(value));
                self.line(&line);
            }
            Stmt::Store(cell, value) => self.store(cell, value),
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
                self.line("loop {");
                self.block(body);
                self.line("}");
            }
            Stmt::Once(body) => {
                self.line("loop {");
                self.block(body);
                if !leaves(body) {
                    self.line("    break;");
                }
                self.line("}");
            }
            Stmt::For {
                var,
                ty,
                from,
                op,
                to,
                down,
                body,
            } => {
                let one = if *ty == Ty::U32 { "1u" } else { "1i" };
                let step = if *down { "-" } else { "+" };
                let line = format!(
                    "for (var {var}: {} = {}; {var} {} {}; {var} = {var} {step} {one}) {{",
                    self.ty(*ty),
                    self.top(from),
                    symbol(*op),
                    self.expr(to),
                );
                self.line(&line);
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
                let value = self.top(value);
                self.line(&format!("_ = {value};"));
            }
            Stmt::Fail(site) => {
                let site = self.top(site);
                self.line(&format!("atomicMin(&g_error, {site});"));
            }
        }
    }

    fn function(&mut self, func: &Func) {
        let params: Vec<String> = func
            .params
            .iter()
            .map(|param| format!("{}: {}", param.name, self.ty(param.ty)))
            .collect();
        let ret = match func.ret {
            Ty::Void => String::new(),
            ty => format!(" -> {}", self.ty(ty)),
        };
        self.line(&format!("fn {}({}){ret} {{", func.name, params.join(", ")));
        self.block(&func.body);
        self.line("}");
    }
}
