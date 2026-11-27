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
use crate::tir::{self, ComponentId, Cores, Program, Query, QueryId, Run, StmtKind, Term, Type};

use analysis::Folded;
use body::FnCx;
use types::Form;

/// The name the driver gives the generated file.
const C_FILE: &str = "program.c";
/// Stands for a `#line` directive that points back at the generated file.
const RESET_LINE: &str = "#line RESET";

/// The tag of the struct of the `Window` resource.
const WINDOW_STRUCT: &str = "mn_window_fields";

/// The tag of the struct of the `Mouse` resource.
const MOUSE_STRUCT: &str = "mn_mouse_fields";

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
    /// The name of the `MnQuery` of each query generated so far, by query
    /// id.
    query_names: HashMap<usize, String>,
    /// The functions of the independent `each`es, with the types of what
    /// they are given.
    loop_defs: String,
    /// The independent `each`es, in the order their functions were
    /// generated.
    loops: Vec<LoopDesc<'a>>,
}

/// One independent `each`: what its `MnLoop` is made from once every query
/// of the program has its name.
struct LoopDesc<'a> {
    /// What its names end in: the number of its `MnQuery`.
    number: String,
    cores: &'a Cores,
    span: Span,
    /// The `MnUse` of each term whose component some GPU block names.
    uses: Vec<String>,
    /// The components of the terms its function flags assignments of.
    wrote: Vec<usize>,
}

/// Whether a program has a loop that can be split across threads: a GPU
/// block or an independent `each`.
pub fn has_split_loops(program: &Program) -> bool {
    if program.systems.iter().any(|system| system.gpu) {
        return true;
    }
    let mut found = false;
    let mut look = |node: analysis::Node| {
        if let analysis::Node::Stmt(stmt) = node
            && let StmtKind::Each {
                run: Run::Cores(_), ..
            } = &stmt.kind
        {
            found = true;
        }
    };
    for func in &program.funcs {
        analysis::walk_block(&func.body.block, &mut look);
    }
    for system in &program.systems {
        analysis::walk_block(&system.body.block, &mut look);
    }
    analysis::walk_block(&program.main.block, &mut look);
    found
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
            query_names: HashMap::new(),
            loop_defs: String::new(),
            loops: Vec::new(),
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
        self.query_names.insert(query.id.0, name.clone());
        name
    }

    /// The `MnQuery` of a query whose code has been generated.
    fn query_name(&self, id: QueryId) -> &str {
        self.query_names
            .get(&id.0)
            .expect("every query an independent `each` reaches has been generated")
    }

    /// The weight function, the lists and the `MnLoop` of every
    /// independent `each`.
    fn loop_descs(&mut self) -> String {
        let mut out = String::new();
        for index in 0..self.loops.len() {
            let number = self.loops[index].number.clone();
            let cores = self.loops[index].cores;
            let _ = writeln!(out, "static uint64_t loop{number}_weight(void) {{");
            out.push_str("    uint64_t weight = 0;\n");
            for term in &cores.weight.terms {
                let mut product = format!("UINT64_C({})", term.factor);
                for query in &term.rows {
                    product = format!(
                        "mn_work_mul({product}, mn_query_rows(&{}))",
                        self.query_name(*query)
                    );
                }
                let _ = writeln!(out, "    weight = mn_work_add(weight, {product});");
            }
            out.push_str("    return weight;\n}\n");

            let queries = if cores.queries.is_empty() {
                "NULL".to_string()
            } else {
                let list: Vec<String> = cores
                    .queries
                    .iter()
                    .map(|query| format!("&{}", self.query_name(*query)))
                    .collect();
                let _ = writeln!(
                    out,
                    "static MnQuery *const loop{number}_queries[] = {{{}}};",
                    list.join(", ")
                );
                format!("loop{number}_queries")
            };
            let uses = if self.loops[index].uses.is_empty() {
                "NULL".to_string()
            } else {
                let _ = writeln!(
                    out,
                    "static const MnUse loop{number}_uses[] = {{{}}};",
                    self.loops[index].uses.join(", ")
                );
                format!("loop{number}_uses")
            };
            let wrote = if self.loops[index].wrote.is_empty() {
                "NULL".to_string()
            } else {
                let list: Vec<String> = self.loops[index]
                    .wrote
                    .iter()
                    .map(|component| component.to_string())
                    .collect();
                let _ = writeln!(
                    out,
                    "static const uint32_t loop{number}_wrote[] = {{{}}};",
                    list.join(", ")
                );
                format!("loop{number}_wrote")
            };
            let loc = self.loc(self.loops[index].span);
            let desc = &self.loops[index];
            let _ = writeln!(
                out,
                "static MnLoop loop{number} = {{L{number}, &q{number}, loop{number}_weight, {}, \
                 {queries}, {}, {uses}, {}, {wrote}, {}, {}, false}};\n",
                cores.sums.len(),
                cores.queries.len(),
                desc.uses.len(),
                desc.wrote.len(),
                loc.trim_start_matches("(MnLoc)"),
            );
        }
        out
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
        for (index, system) in prog.systems.iter().enumerate() {
            let header = format!("static void {}(void)", names::system(&system.name));
            let body = if system.gpu {
                // A GPU system runs its blocks in order, and hands them to
                // the device when an ordinary system follows it.
                let mut body: String = lowered
                    .blocks
                    .iter()
                    .filter(|block| std::ptr::eq(block.system, system.name.as_str()))
                    .map(|block| format!("    mn_gpu_run(&{});\n", gpu::c::block_name(block.id)))
                    .collect();
                if prog.systems.get(index + 1).is_some_and(|next| !next.gpu) {
                    body.push_str("    mn_gpu_submit();\n");
                }
                body
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
        let loop_descs = self.loop_descs();
        let check = if lowered.check {
            format!("    mn_gpu_check = &{};\n", gpu::c::CHECK_NAME)
        } else {
            String::new()
        };
        let window = if prog.window.is_some() {
            "    mn_window_program = &window_program;\n"
        } else {
            ""
        };

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
        if !self.loops.is_empty() {
            out.push_str(&self.loop_defs);
            let _ = writeln!(out, "{RESET_LINE}");
            out.push_str(&loop_descs);
        }
        out.push_str(&defs);
        out.push_str(&tables);
        out.push_str("int main(void) {\n");
        out.push_str(&check);
        out.push_str(window);
        out.push_str(
            "    mn_start(&program);\n    \
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
        for (index, resource) in prog.resources.iter().enumerate() {
            if resource.fields.is_empty() {
                continue;
            }
            let name = names::resource(&resource.name);
            if prog.window.is_some_and(|window| window.resource.0 == index) {
                let _ = writeln!(defs, "static struct {WINDOW_STRUCT} {{");
            } else if prog.window.is_some_and(|window| window.mouse.0 == index) {
                let _ = writeln!(defs, "static struct {MOUSE_STRUCT} {{");
            } else {
                let _ = writeln!(
                    defs,
                    "static struct {} {{",
                    names::resource_struct(&resource.name)
                );
            }
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

    /// The resources the drive's `resource` reads: those the program
    /// declares, not the built-in `Window` and `Mouse`.
    fn drive_resources(&self) -> Vec<usize> {
        let prog = self.prog;
        (0..prog.resources.len())
            .filter(|&index| {
                !prog
                    .window
                    .is_some_and(|window| window.resource.0 == index || window.mouse.0 == index)
            })
            .collect()
    }

    fn resource_count(&self) -> usize {
        self.drive_resources().len()
    }

    /// The lines of the `MnResourceInfo` table: each resource's name, its
    /// storage (NULL with no fields) and its two forms of the drive.
    fn resource_info(&mut self) -> String {
        let prog = self.prog;
        let mut out = String::new();
        for index in self.drive_resources() {
            let resource = &prog.resources[index];
            let data = if resource.fields.is_empty() {
                "NULL".to_string()
            } else {
                format!("&{}", names::resource(&resource.name))
            };
            let text = self.resource_show_fn(index, Form::Text);
            let json = self.resource_show_fn(index, Form::Json);
            let _ = writeln!(
                out,
                "    {{{}, {data}, {text}, {json}}},",
                names::c_string(resource.name.as_bytes())
            );
        }
        out
    }

    /// The function that appends a resource's form of the drive, as a
    /// struct of its fields, given its storage.
    fn resource_show_fn(&mut self, index: usize, form: Form) -> String {
        let prog = self.prog;
        let resource = &prog.resources[index];
        let prefix = match form {
            Form::Text => "Show",
            Form::Json => "Json",
        };
        let name = format!("{prefix}_{}", names::resource(&resource.name));
        let mut body = String::new();
        if resource.fields.is_empty() {
            body.push_str("    (void)p;\n");
        } else {
            let _ = writeln!(
                body,
                "    const struct {} *v = p;",
                names::resource_struct(&resource.name)
            );
        }
        let (mut text, close, comma) = match form {
            Form::Text => (format!("{}(", resource.name), ")", ", "),
            Form::Json => ("{".to_string(), "}", ","),
        };
        for (i, field) in resource.fields.iter().enumerate() {
            if i > 0 {
                text.push_str(comma);
            }
            match form {
                Form::Text => {
                    let _ = write!(text, "{}: ", field.name);
                }
                Form::Json => {
                    let _ = write!(text, "\"{}\":", field.name);
                }
            }
            let _ = writeln!(body, "    {}", types::sb_bytes("sb", &text));
            text.clear();
            let value = format!("v->{}", names::field(&field.name));
            let line = self.show_stmt("sb", &value, &field.ty, form);
            let _ = writeln!(body, "    {line}");
        }
        text.push_str(close);
        let _ = writeln!(body, "    {}", types::sb_bytes("sb", &text));
        let signature = format!("static void {name}(MnBuilder *sb, const void *p)");
        let _ = writeln!(self.helper_protos, "{signature};");
        let _ = writeln!(self.helper_defs, "{signature} {{\n{body}}}\n");
        name
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
                let line = info(self, &component.name, ty.as_ref());
                let shown = match &ty {
                    Some(ty) => format!(
                        ", {}, {}}},\n",
                        self.show_fn(ty, Form::Text),
                        self.show_fn(ty, Form::Json)
                    ),
                    None => ", NULL, NULL},\n".to_string(),
                };
                out.push_str(line.strip_suffix("},\n").unwrap_or(&line));
                out.push_str(&shown);
            }
            out.push_str("};\n\n");
        }
        let resources = self.resource_info();
        if !resources.is_empty() {
            out.push_str("static const MnResourceInfo resource_info[] = {\n");
            out.push_str(&resources);
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
        if let Some(window) = prog.window {
            out.push_str(&self.window_layout(window));
            if draws(prog) {
                out.push_str(DRAW_LAYOUT);
            }
            let _ = writeln!(
                out,
                "static const MnWindowProgram window_program = {{\n    (MnWindow *)&{},\n    (MnMouse *)&{},\n    {},\n    {},\n    {},\n    {},\n    {},\n    {},\n    {},\n    {},\n}};\n",
                names::resource(&prog.resources[window.resource.0].name),
                names::resource(&prog.resources[window.mouse.0].name),
                window.resized.0,
                window.closed.0,
                window.key_pressed.0,
                window.key_released.0,
                window.mouse_pressed.0,
                window.mouse_released.0,
                window.focused.0,
                window.unfocused.0
            );
        }
        let _ = writeln!(
            out,
            "static const MnProgram program = {{\n    {},\n    {},\n    {},\n    drop_globals,\n    \"{}\",\n    {},\n}};\n",
            table("component_info", prog.components.len()),
            table("event_info", prog.events.len()),
            table("systems", prog.systems.len()),
            env!("CARGO_PKG_VERSION"),
            table("resource_info", self.resource_count()),
        );
        out
    }

    /// Assertions that the window's resources and events are laid out as
    /// `MnWindow`, `MnColor`, `MnWindowResized`, `MnMouse`, `MnKeyEvent`
    /// and `MnButtonEvent` in the runtime's header, and that `Key` and
    /// `MouseButton` have as many variants as it lists.
    fn window_layout(&self, window: tir::Window) -> String {
        let prog = self.prog;
        let event =
            |id: tir::EventId| names::record(&prog.records[prog.events[id.0].record.0].name);
        let resized = event(window.resized);
        let color = names::record(&prog.records[window.color.0].name);
        let mut checks = vec![format!(
            "sizeof(struct {WINDOW_STRUCT}) == sizeof(MnWindow)"
        )];
        for field in [
            "width",
            "height",
            "pixel_width",
            "pixel_height",
            "background",
            "open",
            "focused",
        ] {
            checks.push(format!(
                "offsetof(struct {WINDOW_STRUCT}, {}) == offsetof(MnWindow, {field})",
                names::field(field)
            ));
        }
        checks.push(format!("sizeof(struct {MOUSE_STRUCT}) == sizeof(MnMouse)"));
        for field in ["x", "y", "wheel_x", "wheel_y", "inside"] {
            checks.push(format!(
                "offsetof(struct {MOUSE_STRUCT}, {}) == offsetof(MnMouse, {field})",
                names::field(field)
            ));
        }
        for id in [window.key_pressed, window.key_released] {
            let name = event(id);
            checks.push(format!("sizeof({name}) == sizeof(MnKeyEvent)"));
            checks.push(format!(
                "offsetof({name}, {}) == offsetof(MnKeyEvent, key)",
                names::field("key")
            ));
        }
        for id in [window.mouse_pressed, window.mouse_released] {
            let name = event(id);
            checks.push(format!("sizeof({name}) == sizeof(MnButtonEvent)"));
            for field in ["x", "y", "button"] {
                checks.push(format!(
                    "offsetof({name}, {}) == offsetof(MnButtonEvent, {field})",
                    names::field(field)
                ));
            }
        }
        checks.push(format!(
            "{} == MN_KEY_COUNT",
            prog.enums[window.key.0].variants.len()
        ));
        checks.push(format!(
            "{} == MN_MOUSE_BUTTON_COUNT",
            prog.enums[window.button.0].variants.len()
        ));
        checks.push(format!("sizeof({color}) == sizeof(MnColor)"));
        for field in ["r", "g", "b", "a"] {
            checks.push(format!(
                "offsetof({color}, {}) == offsetof(MnColor, {field})",
                names::field(field)
            ));
        }
        checks.push(format!("sizeof({resized}) == sizeof(MnWindowResized)"));
        for field in ["width", "height", "pixel_width", "pixel_height"] {
            checks.push(format!(
                "offsetof({resized}, {}) == offsetof(MnWindowResized, {field})",
                names::field(field)
            ));
        }
        let mut out = String::new();
        for check in checks {
            let _ = writeln!(out, "_Static_assert({check}, \"the window's layout\");");
        }
        out.push('\n');
        out
    }
}

/// Assertions that a draw's record is laid out as a shader writes it.
const DRAW_LAYOUT: &str = "\
_Static_assert(sizeof(MnDraw) == 32, \"the layout of a draw\");
_Static_assert(offsetof(MnDraw, kind) == 0, \"the layout of a draw\");
_Static_assert(offsetof(MnDraw, n) == 4, \"the layout of a draw\");
_Static_assert(offsetof(MnDraw, color) == 28, \"the layout of a draw\");

";

/// Whether the program has a draw statement.
fn draws(prog: &Program) -> bool {
    let mut found = false;
    let mut visit = |node: analysis::Node<'_>| {
        if let analysis::Node::Stmt(stmt) = node
            && matches!(stmt.kind, StmtKind::Draw { .. })
        {
            found = true;
        }
    };
    for func in &prog.funcs {
        analysis::walk_block(&func.body.block, &mut visit);
    }
    for system in &prog.systems {
        analysis::walk_block(&system.body.block, &mut visit);
    }
    analysis::walk_block(&prog.main.block, &mut visit);
    found
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
