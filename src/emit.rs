//! Typed program to C: one translation unit for the whole program.
//!
//! The file is laid out as: file names and string literals, types, helper
//! prototypes, list element descriptions, constants and resources, the
//! static data of queries, bundles and event cursors, function prototypes,
//! helpers, the code and descriptions of GPU blocks, the program's
//! functions and systems, and last the tables that describe the program to
//! the runtime and `main`.

mod analysis;
mod body;
mod expr;
mod gpu;
mod names;
mod place;
mod stmt;
#[cfg(test)]
mod tests;
mod types;

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;

use crate::source::{SourceMap, Span};
use crate::tir::{ComponentId, Program, Query, Term, Type};

use analysis::Folded;
use body::FnCx;

/// The name the driver gives the generated file.
const C_FILE: &str = "program.c";
/// Stands for a `#line` directive that points back at the generated file.
const RESET_LINE: &str = "#line RESET";

/// Turns off the C warnings about what a program may legitimately say:
/// `x == x`, `x = x`, a compact field compared with a constant it can't
/// hold.
const QUIET: &str = "\
#if defined(__clang__)
#pragma clang diagnostic ignored \"-Wtautological-compare\"
#pragma clang diagnostic ignored \"-Wliteral-range\"
#pragma clang diagnostic ignored \"-Wself-assign\"
#elif defined(__GNUC__)
#pragma GCC diagnostic ignored \"-Wtautological-compare\"
#endif

";

/// Generates the C for a program.
pub fn emit(program: &Program, sources: &SourceMap) -> String {
    Cx::new(program, sources).program()
}

/// The shader of every GPU block of a program, each after a comment line
/// that names its system and which `each` of the system it is.
pub fn emit_wgsl(program: &Program, sources: &SourceMap) -> String {
    let cx = Cx::new(program, sources);
    gpu::wgsl::listing(&gpu::lower(program, &cx.folded, cx.gpu.wide.clone()))
}

/// What is shared by everything generated for one program.
struct Cx<'a> {
    prog: &'a Program,
    sources: &'a SourceMap,
    /// For each record, whether it owns a string or list.
    managed_records: Vec<bool>,
    /// For each constant, its value when that is known here.
    folded: Vec<Option<Folded>>,
    /// String literals by content, numbered in order of first use.
    strings: HashMap<String, usize>,
    string_defs: String,
    /// Source file names by file id, numbered in order of first use.
    files: HashMap<u32, usize>,
    file_defs: String,
    /// Names of the helpers generated so far.
    helpers: HashSet<String>,
    helper_protos: String,
    helper_defs: String,
    elem_defs: String,
    enum_tables: String,
    /// Queries, bundles, component lists and event cursors.
    statics: String,
    static_count: usize,
    /// For each function, whether generated code calls it.
    func_used: Vec<bool>,
    /// What GPU systems require of the code of everything else.
    gpu: gpu::Plan,
}

impl<'a> Cx<'a> {
    fn new(prog: &'a Program, sources: &'a SourceMap) -> Cx<'a> {
        let mut folded: Vec<Option<Folded>> = Vec::new();
        for item in &prog.consts {
            let value = if analysis::foldable_type(&item.ty) {
                analysis::fold(&item.init, &folded)
            } else {
                None
            };
            folded.push(value);
        }
        Cx {
            prog,
            sources,
            managed_records: types::managed_records(prog),
            folded,
            strings: HashMap::new(),
            string_defs: String::new(),
            files: HashMap::new(),
            file_defs: String::new(),
            helpers: HashSet::new(),
            helper_protos: String::new(),
            helper_defs: String::new(),
            elem_defs: String::new(),
            enum_tables: String::new(),
            statics: String::new(),
            static_count: 0,
            func_used: vec![false; prog.funcs.len()],
            gpu: gpu::Plan::new(prog),
        }
    }

    /// A `MnString` expression for a literal.
    fn str_lit(&mut self, text: &str) -> String {
        if text.is_empty() {
            return "NULL".to_string();
        }
        let next = self.strings.len();
        let index = *self.strings.entry(text.to_string()).or_insert(next);
        if index == next {
            let _ = writeln!(
                self.string_defs,
                "MN_STR_LIT(str{index}, {}, {});",
                text.len(),
                names::c_string(text.as_bytes())
            );
        }
        format!("((MnString)&str{index})")
    }

    /// The static array that holds the name of a span's file.
    fn file(&mut self, span: Span) -> String {
        let next = self.files.len();
        let index = *self.files.entry(span.file.0).or_insert(next);
        if index == next {
            let name = &self.sources.file(span.file).name;
            let _ = writeln!(
                self.file_defs,
                "static const char src{index}[] = {};",
                names::c_string(name.as_bytes())
            );
        }
        format!("src{index}")
    }

    /// A `MnLoc` expression for the line a span starts on.
    fn loc(&mut self, span: Span) -> String {
        let file = self.file(span);
        format!("(MnLoc){{{file}, {}}}", self.sources.line(span))
    }

    /// A `#line` directive for the line a span starts on.
    fn line_directive(&self, span: Span) -> String {
        let name = &self.sources.file(span.file).name;
        format!(
            "#line {} {}",
            self.sources.line(span),
            names::c_string(name.as_bytes())
        )
    }

    fn next_static(&mut self) -> usize {
        self.static_count += 1;
        self.static_count - 1
    }

    /// Defines a static array of component ids and returns the expression
    /// for it and its length. An empty list is `NULL`.
    fn id_list(&mut self, name: &str, ids: &[ComponentId]) -> (String, usize) {
        if ids.is_empty() {
            return ("NULL".to_string(), 0);
        }
        let list: Vec<String> = ids.iter().map(|id| id.0.to_string()).collect();
        let _ = writeln!(
            self.statics,
            "static const uint32_t {name}[] = {{{}}};",
            list.join(", ")
        );
        (name.to_string(), ids.len())
    }

    /// Defines the static `MnQuery` of one `each` or `World.count`.
    fn query(&mut self, query: &Query) -> String {
        let name = format!("q{}", self.next_static());
        let mut with: Vec<ComponentId> = query
            .terms
            .iter()
            .filter_map(|term| match term {
                Term::Component { component, .. } => Some(*component),
                Term::Entity => None,
            })
            .collect();
        with.extend(&query.with);
        let (with, with_count) = self.id_list(&format!("{name}_with"), &with);
        let (without, without_count) = self.id_list(&format!("{name}_without"), &query.without);
        let _ = writeln!(
            self.statics,
            "static MnQuery {name} = {{{with}, {with_count}, {without}, {without_count}, NULL, 0, 0, 0}};"
        );
        name
    }

    /// Defines the static `MnBundle` of one `spawn` or `add`.
    fn bundle(&mut self, components: &[ComponentId]) -> String {
        let name = format!("b{}", self.next_static());
        let (ids, count) = self.id_list(&format!("{name}_ids"), components);
        let _ = writeln!(
            self.statics,
            "static MnBundle {name} = {{{ids}, {count}, 0, 0, false}};"
        );
        name
    }

    /// Defines the static component list of one `remove`.
    fn remove_list(&mut self, components: &[ComponentId]) -> (String, usize) {
        let name = format!("ids{}", self.next_static());
        self.id_list(&name, components)
    }

    /// Defines the static cursor of one `on`.
    fn cursor(&mut self) -> String {
        let name = format!("cur{}", self.next_static());
        let _ = writeln!(self.statics, "static uint64_t {name} = 0;");
        name
    }

    fn program(mut self) -> String {
        let prog = self.prog;
        let mut protos = Vec::new();
        let mut defs = String::new();

        for (index, func) in prog.funcs.iter().enumerate() {
            let name = names::func(prog, index);
            let ret = self.cty(&func.ret);
            let cx = FnCx::new(&mut self, &func.body, func.ret.clone());
            let header = format!("static {ret} {name}({})", cx.param_list(&func.params));
            let body = cx.function(&func.body);
            protos.push((header.clone(), Some(index)));
            let line = self.line_directive(func.span);
            let _ = writeln!(defs, "{line}\n{header} {{\n{body}}}\n");
        }
        let lowered = gpu::lower(prog, &self.folded, self.gpu.wide.clone());
        for system in &prog.systems {
            let header = format!("static void {}(void)", names::system(&system.name));
            let body = if system.gpu {
                // A GPU system runs its blocks in order.
                lowered
                    .blocks
                    .iter()
                    .filter(|block| std::ptr::eq(block.system, system.name.as_str()))
                    .map(|block| format!("    mn_gpu_run(&{});\n", gpu::c::block_name(block.id)))
                    .collect()
            } else {
                FnCx::new(&mut self, &system.body, Type::Void).function(&system.body)
            };
            protos.push((header.clone(), None));
            let line = self.line_directive(system.span);
            let _ = writeln!(defs, "{line}\n{header} {{\n{body}}}\n");
        }
        let main = FnCx::new(&mut self, &prog.main, Type::Void).function(&prog.main);
        let _ = writeln!(defs, "{RESET_LINE}");
        let _ = writeln!(defs, "static void run_main(void) {{\n{main}}}\n");

        let (globals, init, drop) = self.globals();
        let _ = writeln!(defs, "{RESET_LINE}");
        let _ = writeln!(defs, "static void init_globals(void) {{\n{init}}}\n");
        let _ = writeln!(defs, "{RESET_LINE}");
        let _ = writeln!(defs, "static void drop_globals(void) {{\n{drop}}}\n");
        let tables = self.tables();
        let gpu_section = gpu::c::section(&lowered, &mut self);

        let mut out = String::new();
        out.push_str("#include \"midnight_rt.h\"\n\n#include <math.h>\n\n");
        out.push_str(QUIET);
        for section in [&self.file_defs, &self.string_defs] {
            if !section.is_empty() {
                out.push_str(section);
                out.push('\n');
            }
        }
        out.push_str(&self.type_defs());
        for section in [
            &self.enum_tables,
            &self.helper_protos,
            &self.elem_defs,
            &globals,
            &self.statics,
        ] {
            if !section.is_empty() {
                out.push_str(section);
                out.push('\n');
            }
        }
        for (header, func) in &protos {
            let unused = match func {
                Some(index) if !self.func_used[*index] => " __attribute__((unused))",
                _ => "",
            };
            let _ = writeln!(out, "{header}{unused};");
        }
        out.push_str("static void run_main(void);\n\n");
        out.push_str(&self.helper_defs);
        out.push_str(&gpu_section);
        out.push_str(&defs);
        out.push_str(&tables);
        out.push_str(
            "int main(void) {\n    \
             mn_start(&program);\n    \
             init_globals();\n    \
             run_main();\n    \
             mn_finish();\n    \
             return 0;\n}\n",
        );
        number_lines(&out)
    }

    /// The definitions of the constants and resources, the body of the
    /// function that gives them their values, and the body of the one that
    /// releases what they own.
    fn globals(&mut self) -> (String, String, String) {
        let prog = self.prog;
        let mut defs = String::new();
        let mut drop = String::new();
        let mut cx = FnCx::new_bare(self);
        for (index, item) in prog.consts.iter().enumerate() {
            if cx.cx.folded[index].is_some() {
                continue;
            }
            let name = names::constant(&item.name);
            let _ = writeln!(defs, "static {} {name};", cx.cx.cty(&item.ty));
            cx.init_global(&name, &item.ty, &item.init);
            if cx.cx.managed(&item.ty) {
                let _ = writeln!(drop, "    {}", cx.cx.drop_stmt(&name, &item.ty));
            }
        }
        for resource in &prog.resources {
            if resource.fields.is_empty() {
                continue;
            }
            let name = names::resource(&resource.name);
            defs.push_str("static struct {\n");
            for index in cx
                .cx
                .layout_order(resource.fields.iter().map(|field| &field.ty))
            {
                let field = &resource.fields[index];
                let _ = writeln!(
                    defs,
                    "    {} {};",
                    cx.cx.cty(&field.ty),
                    names::field(&field.name)
                );
            }
            let _ = writeln!(defs, "}} {name};");
            for field in &resource.fields {
                let lv = format!("{name}.{}", names::field(&field.name));
                cx.init_global(&lv, &field.ty, &field.init);
                if cx.cx.managed(&field.ty) {
                    let _ = writeln!(drop, "    {}", cx.cx.drop_stmt(&lv, &field.ty));
                }
            }
        }
        let init = cx.out;
        (defs, init, drop)
    }

    /// The component and event descriptions, the list of systems and the
    /// `MnProgram`.
    fn tables(&mut self) -> String {
        let prog = self.prog;
        let mut out = String::new();
        let info = |cx: &mut Self, name: &str, record: Option<&Type>| {
            let name = names::c_string(name.as_bytes());
            match record {
                Some(ty) => {
                    let cty = cx.cty(ty);
                    let drop = if cx.managed(ty) {
                        cx.drop_fn(ty)
                    } else {
                        "NULL".to_string()
                    };
                    format!("    {{{name}, sizeof({cty}), _Alignof({cty}), {drop}}},\n")
                }
                None => format!("    {{{name}, 0, 1, NULL}},\n"),
            }
        };
        if !prog.components.is_empty() {
            out.push_str("static const MnComponentInfo component_info[] = {\n");
            for component in &prog.components {
                let ty = component.record.map(Type::Record);
                out.push_str(&info(self, &component.name, ty.as_ref()));
            }
            out.push_str("};\n\n");
        }
        if !prog.events.is_empty() {
            out.push_str("static const MnEventInfo event_info[] = {\n");
            for event in &prog.events {
                let ty = Type::Record(event.record);
                out.push_str(&info(self, &event.name, Some(&ty)));
            }
            out.push_str("};\n\n");
        }
        if !prog.systems.is_empty() {
            out.push_str("static void (*const systems[])(void) = {\n");
            for system in &prog.systems {
                let _ = writeln!(out, "    {},", names::system(&system.name));
            }
            out.push_str("};\n\n");
        }
        let table = |name: &str, count: usize| {
            if count == 0 {
                "NULL, 0".to_string()
            } else {
                format!("{name}, {count}")
            }
        };
        let _ = writeln!(
            out,
            "static const MnProgram program = {{\n    {},\n    {},\n    {},\n    drop_globals,\n}};\n",
            table("component_info", prog.components.len()),
            table("event_info", prog.events.len()),
            table("systems", prog.systems.len()),
        );
        out
    }
}

/// Replaces each reset marker with a `#line` directive that gives the
/// lines after it their own numbers in the generated file.
fn number_lines(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 64);
    for (index, line) in text.lines().enumerate() {
        if line == RESET_LINE {
            let _ = writeln!(out, "#line {} \"{C_FILE}\"", index + 2);
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}
