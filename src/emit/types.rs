//! C types for Midnight types, record layout, and the per-type helpers:
//! retain, drop, list element descriptions, formatting and equality.

use std::cmp::Reverse;
use std::fmt::Write as _;

use crate::tir::{Compact, EnumId, Program, Type};

use super::Cx;
use super::names;

/// A form a value of the world is written in for the drive's `query` and
/// `resource`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Form {
    Text,
    Json,
}

pub fn compact_cty(compact: Compact) -> &'static str {
    match compact {
        Compact::Float32 => "float",
        Compact::Int8 => "int8_t",
        Compact::Int16 => "int16_t",
        Compact::Int32 => "int32_t",
        Compact::UInt8 => "uint8_t",
        Compact::UInt16 => "uint16_t",
        Compact::UInt32 => "uint32_t",
    }
}

/// The runtime's checked conversion into a compact integer type.
pub fn narrow_fn(compact: Compact) -> &'static str {
    match compact {
        Compact::Float32 => unreachable!("Float32 stores are casts"),
        Compact::Int8 => "mn_to_int8",
        Compact::Int16 => "mn_to_int16",
        Compact::Int32 => "mn_to_int32",
        Compact::UInt8 => "mn_to_uint8",
        Compact::UInt16 => "mn_to_uint16",
        Compact::UInt32 => "mn_to_uint32",
    }
}

/// For each record, whether it holds a string or a list anywhere inside.
pub fn managed_records(prog: &Program) -> Vec<bool> {
    fn visit(prog: &Program, record: usize, memo: &mut [Option<bool>]) -> bool {
        if let Some(known) = memo[record] {
            return known;
        }
        memo[record] = Some(false);
        let managed = prog.records[record]
            .fields
            .iter()
            .any(|field| match &field.ty {
                Type::String | Type::List(_) => true,
                Type::Record(id) => visit(prog, id.0, memo),
                _ => false,
            });
        memo[record] = Some(managed);
        managed
    }
    let mut memo = vec![None; prog.records.len()];
    (0..prog.records.len())
        .map(|record| visit(prog, record, &mut memo))
        .collect()
}

/// The size in bytes of the integer an enum is stored as.
pub fn enum_bytes(prog: &Program, id: EnumId) -> u32 {
    match prog.enums[id.0].variants.len() {
        0..=0x100 => 1,
        0x101..=0x1_0000 => 2,
        _ => 4,
    }
}

/// The alignment of a type's C representation. `wide[record]` marks the
/// records whose alignment is raised to 4.
pub fn align_of(prog: &Program, wide: &[bool], ty: &Type) -> u32 {
    match ty {
        Type::Void | Type::Bool => 1,
        Type::Int | Type::Float | Type::String | Type::List(_) => 8,
        Type::Entity => 4,
        Type::Compact(compact) => match compact {
            Compact::Float32 | Compact::Int32 | Compact::UInt32 => 4,
            Compact::Int16 | Compact::UInt16 => 2,
            Compact::Int8 | Compact::UInt8 => 1,
        },
        Type::Enum(id) => enum_bytes(prog, *id),
        Type::Record(id) => {
            let fields = prog.records[id.0]
                .fields
                .iter()
                .map(|field| align_of(prog, wide, &field.ty))
                .max()
                .unwrap_or(1);
            if wide[id.0] { fields.max(4) } else { fields }
        }
    }
}

/// The members of a struct with these field types, largest alignment
/// first, as indexes into the fields.
pub fn layout_order<'t>(
    prog: &Program,
    wide: &[bool],
    types: impl Iterator<Item = &'t Type>,
) -> Vec<usize> {
    let aligns: Vec<u32> = types.map(|ty| align_of(prog, wide, ty)).collect();
    let mut order: Vec<usize> = (0..aligns.len()).collect();
    order.sort_by_key(|&index| Reverse(aligns[index]));
    order
}

impl Cx<'_> {
    pub fn cty(&self, ty: &Type) -> String {
        match ty {
            Type::Void => "void".to_string(),
            Type::Int => "int64_t".to_string(),
            Type::Float => "double".to_string(),
            Type::Bool => "bool".to_string(),
            Type::String => "MnString".to_string(),
            Type::Entity => "MnEntity".to_string(),
            Type::Compact(compact) => compact_cty(*compact).to_string(),
            Type::List(_) => "MnList".to_string(),
            Type::Record(id) => names::record(&self.prog.records[id.0].name),
            Type::Enum(id) => names::enum_(&self.prog.enums[id.0].name),
        }
    }

    /// The initializer that makes a variable of this type all zero.
    pub fn zero(&self, ty: &Type) -> &'static str {
        match ty {
            Type::Void => "",
            Type::Int | Type::Enum(_) => "0",
            Type::Compact(Compact::Float32) | Type::Float => "0.0",
            Type::Compact(_) => "0",
            Type::Bool => "false",
            Type::String | Type::List(_) => "NULL",
            Type::Entity | Type::Record(_) => "{0}",
        }
    }

    /// The all-zero value of this type as an expression.
    pub fn zero_expr(&self, ty: &Type) -> String {
        match ty {
            Type::Entity | Type::Record(_) => format!("({}){{0}}", self.cty(ty)),
            _ => self.zero(ty).to_string(),
        }
    }

    /// Whether values of this type own a string or list buffer.
    pub fn managed(&self, ty: &Type) -> bool {
        match ty {
            Type::String | Type::List(_) => true,
            Type::Record(id) => self.managed_records[id.0],
            _ => false,
        }
    }

    fn enum_cty(&self, id: EnumId) -> &'static str {
        match enum_bytes(self.prog, id) {
            1 => "uint8_t",
            2 => "uint16_t",
            _ => "uint32_t",
        }
    }

    /// The members of a struct with these field types, largest alignment
    /// first, as indexes into the fields.
    pub fn layout_order<'t>(&self, types: impl Iterator<Item = &'t Type>) -> Vec<usize> {
        layout_order(self.prog, &self.gpu.wide, types)
    }

    /// The enum typedefs, then every record's struct, each after the
    /// records it holds by value.
    pub fn type_defs(&self) -> String {
        let prog = self.prog;
        let mut out = String::new();
        for (index, item) in prog.enums.iter().enumerate() {
            let _ = writeln!(
                out,
                "typedef {} {};",
                self.enum_cty(EnumId(index)),
                names::enum_(&item.name)
            );
        }
        if !prog.enums.is_empty() {
            out.push('\n');
        }
        let mut done = vec![false; prog.records.len()];
        for record in 0..prog.records.len() {
            self.record_def(record, &mut done, &mut out);
        }
        out
    }

    fn record_def(&self, record: usize, done: &mut [bool], out: &mut String) {
        if done[record] {
            return;
        }
        done[record] = true;
        let item = &self.prog.records[record];
        for field in &item.fields {
            if let Type::Record(inner) = &field.ty {
                self.record_def(inner.0, done, out);
            }
        }
        let name = names::record(&item.name);
        let _ = writeln!(out, "typedef struct {name} {{");
        // A record marked wide is aligned to 4 bytes through its first
        // member, which rounds its size up to a multiple of 4 as well.
        let mut widen = self.gpu.wide[record]
            && item
                .fields
                .iter()
                .all(|field| align_of(self.prog, &self.gpu.wide, &field.ty) < 4);
        for index in self.layout_order(item.fields.iter().map(|field| &field.ty)) {
            let field = &item.fields[index];
            let _ = writeln!(
                out,
                "    {}{} {};",
                if widen { "_Alignas(4) " } else { "" },
                self.cty(&field.ty),
                names::field(&field.name)
            );
            widen = false;
        }
        if item.fields.is_empty() {
            out.push_str("    char empty;\n");
        }
        let _ = writeln!(out, "}} {name};\n");
    }

    /// Claims a helper's name. True the first time, when the caller must
    /// define it.
    fn claim(&mut self, name: &str) -> bool {
        self.helpers.insert(name.to_string())
    }

    /// A statement that releases what the value at lvalue `lv` owns.
    pub fn drop_stmt(&mut self, lv: &str, ty: &Type) -> String {
        match ty {
            Type::String => format!("mn_str_release({lv});"),
            Type::List(elem) => format!("mn_list_release({lv}, &{});", self.elem(elem)),
            Type::Record(_) => format!("{}(&{lv});", self.drop_fn(ty)),
            _ => unreachable!("only managed types are dropped"),
        }
    }

    /// A statement that adds a reference to everything the value at
    /// lvalue `lv` owns.
    pub fn retain_stmt(&mut self, lv: &str, ty: &Type) -> String {
        match ty {
            Type::String => format!("mn_str_retain({lv});"),
            Type::List(_) => format!("mn_list_retain({lv});"),
            Type::Record(_) => format!("{}(&{lv});", self.retain_fn(ty)),
            _ => unreachable!("only managed types are retained"),
        }
    }

    /// The `void (void *)` function that drops a value of a managed type.
    pub fn drop_fn(&mut self, ty: &Type) -> String {
        let name = format!("Drop_{}", names::mangle(self.prog, ty));
        if self.claim(&name) {
            let body = self.owned_fields(ty, true);
            self.pointer_helper(&name, ty, &body);
        }
        name
    }

    /// The `void (void *)` function that retains a value of a managed type.
    pub fn retain_fn(&mut self, ty: &Type) -> String {
        let name = format!("Retain_{}", names::mangle(self.prog, ty));
        if self.claim(&name) {
            let body = self.owned_fields(ty, false);
            self.pointer_helper(&name, ty, &body);
        }
        name
    }

    /// One retain or drop statement for each buffer a `*v` of this type
    /// owns directly or in a nested record.
    fn owned_fields(&mut self, ty: &Type, drop: bool) -> Vec<String> {
        let prog = self.prog;
        let one = |cx: &mut Self, lv: &str, ty: &Type| {
            if drop {
                cx.drop_stmt(lv, ty)
            } else {
                cx.retain_stmt(lv, ty)
            }
        };
        match ty {
            Type::Record(id) => {
                let mut body = Vec::new();
                for field in &prog.records[id.0].fields {
                    if self.managed(&field.ty) {
                        let lv = format!("v->{}", names::field(&field.name));
                        body.push(one(self, &lv, &field.ty));
                    }
                }
                body
            }
            _ => vec![one(self, "*v", ty)],
        }
    }

    fn pointer_helper(&mut self, name: &str, ty: &Type, body: &[String]) {
        let cty = self.cty(ty);
        let _ = writeln!(self.helper_protos, "static void {name}(void *p);");
        let _ = writeln!(self.helper_defs, "static void {name}(void *p) {{");
        let _ = writeln!(self.helper_defs, "    {cty} *v = p;");
        for line in body {
            let _ = writeln!(self.helper_defs, "    {line}");
        }
        self.helper_defs.push_str("}\n\n");
    }

    /// The `MnElem` that describes lists whose elements are stored as
    /// `elem`.
    pub fn elem(&mut self, elem: &Type) -> String {
        let name = format!("Elem_{}", names::mangle(self.prog, elem));
        if self.claim(&name) {
            let (retain, drop) = if self.managed(elem) {
                (self.retain_fn(elem), self.drop_fn(elem))
            } else {
                ("NULL".to_string(), "NULL".to_string())
            };
            let cty = self.cty(elem);
            let _ = writeln!(
                self.elem_defs,
                "static const MnElem {name} = {{sizeof({cty}), {retain}, {drop}}};"
            );
        }
        name
    }

    /// The table of an enum's variant names as static strings.
    pub fn enum_names(&mut self, id: EnumId) -> String {
        let prog = self.prog;
        let item = &prog.enums[id.0];
        let name = format!("Names_{}", names::enum_(&item.name));
        if self.claim(&name) {
            let literals: Vec<String> = item
                .variants
                .iter()
                .map(|variant| self.str_lit(variant))
                .collect();
            let _ = writeln!(
                self.enum_tables,
                "static const MnString {name}[] = {{{}}};",
                literals.join(", ")
            );
        }
        name
    }

    /// A statement that appends the text of the value `c`, stored as `ty`,
    /// to the builder `sb` points at. `quoted` shows strings as literals.
    pub fn fmt_stmt(&mut self, sb: &str, c: &str, ty: &Type, quoted: bool) -> String {
        match ty {
            Type::Int => format!("mn_sb_int({sb}, {c});"),
            Type::Float | Type::Compact(Compact::Float32) => format!("mn_sb_float({sb}, {c});"),
            Type::Compact(_) => format!("mn_sb_int({sb}, {c});"),
            Type::Bool => format!("mn_sb_bool({sb}, {c});"),
            Type::String if quoted => format!("mn_sb_quoted({sb}, {c});"),
            Type::String => format!("mn_sb_str({sb}, {c});"),
            Type::Entity => format!("mn_sb_entity({sb}, {c});"),
            Type::Enum(id) => format!("mn_sb_str({sb}, {}[{c}]);", self.enum_names(*id)),
            Type::Record(_) => format!("{}({sb}, &{c});", self.fmt_fn(ty)),
            Type::List(_) => format!("{}({sb}, {c});", self.fmt_fn(ty)),
            Type::Void => unreachable!("no text for no value"),
        }
    }

    /// The function that appends a record's or a list's text to a builder:
    /// `(MnBuilder *, const T_X *)` or `(MnBuilder *, MnList)`.
    pub fn fmt_fn(&mut self, ty: &Type) -> String {
        let prog = self.prog;
        let name = format!("Fmt_{}", names::mangle(prog, ty));
        if !self.claim(&name) {
            return name;
        }
        let mut body = String::new();
        let signature = match ty {
            Type::Record(id) => {
                let item = &prog.records[id.0];
                let mut text = format!("{}(", item.name);
                for (index, field) in item.fields.iter().enumerate() {
                    if index > 0 {
                        text.push_str(", ");
                    }
                    let _ = write!(text, "{}: ", field.name);
                    let _ = writeln!(body, "    {}", sb_bytes("sb", &text));
                    text.clear();
                    let value = format!("v->{}", names::field(&field.name));
                    let line = self.fmt_stmt("sb", &value, &field.ty, true);
                    let _ = writeln!(body, "    {line}");
                }
                text.push(')');
                let _ = writeln!(body, "    {}", sb_bytes("sb", &text));
                if item.fields.is_empty() {
                    body.push_str("    (void)v;\n");
                }
                format!(
                    "static void {name}(MnBuilder *sb, const {} *v)",
                    self.cty(ty)
                )
            }
            Type::List(elem) => {
                let cty = self.cty(elem);
                let line = self.fmt_stmt("sb", "d[i]", elem, true);
                let _ = writeln!(body, "    int64_t n = mn_list_len(v);");
                let _ = writeln!(body, "    {}", sb_bytes("sb", "["));
                let _ = writeln!(body, "    for (int64_t i = 0; i < n; i++) {{");
                let _ = writeln!(body, "        const {cty} *d = mn_list_data(v);");
                let _ = writeln!(body, "        if (i > 0) {}", sb_bytes("sb", ", "));
                let _ = writeln!(body, "        {line}");
                let _ = writeln!(body, "    }}");
                let _ = writeln!(body, "    {}", sb_bytes("sb", "]"));
                format!("static void {name}(MnBuilder *sb, MnList v)")
            }
            _ => unreachable!("only records and lists have formatting functions"),
        };
        let _ = writeln!(self.helper_protos, "{signature};");
        let _ = writeln!(self.helper_defs, "{signature} {{\n{body}}}\n");
        name
    }

    /// A statement that appends the value `c`, stored as `ty`, to the
    /// builder `sb` points at, in a form of the drive: text as `to_s`
    /// writes it inside a record but for `Float32`, which has the fewest
    /// digits a 32-bit and a 64-bit reader both read back, or JSON.
    pub fn show_stmt(&mut self, sb: &str, c: &str, ty: &Type, form: Form) -> String {
        match (form, ty) {
            (Form::Text, Type::Compact(Compact::Float32)) => format!("mn_sb_float32({sb}, {c});"),
            (Form::Text, Type::Record(_) | Type::List(_)) => self.show_call(sb, c, ty, form),
            (Form::Text, _) => self.fmt_stmt(sb, c, ty, true),
            (Form::Json, Type::Float) => format!("mn_json_float({sb}, {c});"),
            (Form::Json, Type::Compact(Compact::Float32)) => format!("mn_json_float32({sb}, {c});"),
            (Form::Json, Type::Int | Type::Compact(_)) => format!("mn_sb_int({sb}, {c});"),
            (Form::Json, Type::Bool) => format!("mn_sb_bool({sb}, {c});"),
            (Form::Json, Type::String) => format!("mn_json_str({sb}, {c});"),
            (Form::Json, Type::Entity) => format!("mn_json_entity({sb}, {c});"),
            (Form::Json, Type::Enum(id)) => {
                format!("mn_json_str({sb}, {}[{c}]);", self.enum_names(*id))
            }
            (Form::Json, Type::Record(_) | Type::List(_)) => self.show_call(sb, c, ty, form),
            (_, Type::Void) => unreachable!("no text for no value"),
        }
    }

    /// A call of [`Cx::show_fn`] for the value `c`: a record by address, a
    /// list by value.
    fn show_call(&mut self, sb: &str, c: &str, ty: &Type, form: Form) -> String {
        let name = self.show_fn(ty, form);
        match ty {
            Type::Record(_) => format!("{name}({sb}, &{c});"),
            _ => format!("{name}({sb}, {c});"),
        }
    }

    /// The function that appends a record's or a list's form of the drive
    /// to a builder: `(MnBuilder *, const void *)` for a record, whose
    /// address it takes, or `(MnBuilder *, MnList)`. The built-in `Color`
    /// is `[r,g,b,a]` in JSON.
    pub fn show_fn(&mut self, ty: &Type, form: Form) -> String {
        let prog = self.prog;
        let prefix = match form {
            Form::Text => "Show",
            Form::Json => "Json",
        };
        let name = format!("{prefix}_{}", names::mangle(prog, ty));
        if !self.claim(&name) {
            return name;
        }
        let mut body = String::new();
        let signature = match ty {
            Type::Record(id) => {
                let item = &prog.records[id.0];
                let _ = writeln!(body, "    const {} *v = p;", self.cty(ty));
                let color = form == Form::Json && item.name == "Color";
                let (open, close) = match (form, color) {
                    (Form::Text, _) => (format!("{}(", item.name), ")"),
                    (Form::Json, true) => ("[".to_string(), "]"),
                    (Form::Json, false) => ("{".to_string(), "}"),
                };
                let mut text = open;
                for (index, field) in item.fields.iter().enumerate() {
                    if index > 0 {
                        text.push_str(match form {
                            Form::Text => ", ",
                            Form::Json => ",",
                        });
                    }
                    match (form, color) {
                        (Form::Text, _) => {
                            let _ = write!(text, "{}: ", field.name);
                        }
                        (Form::Json, false) => {
                            let _ = write!(text, "\"{}\":", field.name);
                        }
                        (Form::Json, true) => {}
                    }
                    let _ = writeln!(body, "    {}", sb_bytes("sb", &text));
                    text.clear();
                    let value = format!("v->{}", names::field(&field.name));
                    let line = self.show_stmt("sb", &value, &field.ty, form);
                    let _ = writeln!(body, "    {line}");
                }
                text.push_str(close);
                let _ = writeln!(body, "    {}", sb_bytes("sb", &text));
                if item.fields.is_empty() {
                    body.push_str("    (void)v;\n");
                }
                format!("static void {name}(MnBuilder *sb, const void *p)")
            }
            Type::List(elem) => {
                let cty = self.cty(elem);
                let line = self.show_stmt("sb", "d[i]", elem, form);
                let comma = match form {
                    Form::Text => ", ",
                    Form::Json => ",",
                };
                let _ = writeln!(body, "    int64_t n = mn_list_len(v);");
                let _ = writeln!(body, "    {}", sb_bytes("sb", "["));
                let _ = writeln!(body, "    for (int64_t i = 0; i < n; i++) {{");
                let _ = writeln!(body, "        const {cty} *d = mn_list_data(v);");
                let _ = writeln!(body, "        if (i > 0) {}", sb_bytes("sb", comma));
                let _ = writeln!(body, "        {line}");
                let _ = writeln!(body, "    }}");
                let _ = writeln!(body, "    {}", sb_bytes("sb", "]"));
                format!("static void {name}(MnBuilder *sb, MnList v)")
            }
            _ => unreachable!("only records and lists have functions of the drive"),
        };
        let _ = writeln!(self.helper_protos, "{signature};");
        let _ = writeln!(self.helper_defs, "{signature} {{\n{body}}}\n");
        name
    }

    /// A `bool` expression comparing two values stored as `ty`. `a` and
    /// `b` are operands that need no parentheses; for a record they are
    /// lvalues.
    pub fn eq_c(&mut self, a: &str, b: &str, ty: &Type) -> String {
        match ty {
            Type::String => format!("mn_str_eq({a}, {b})"),
            Type::Entity => {
                if self.claim("eq_entity") {
                    self.helper_protos
                        .push_str("static inline bool eq_entity(MnEntity a, MnEntity b);\n");
                    self.helper_defs.push_str(
                        "static inline bool eq_entity(MnEntity a, MnEntity b) {\n    \
                         return a.index == b.index && a.generation == b.generation;\n}\n\n",
                    );
                }
                format!("eq_entity({a}, {b})")
            }
            Type::Record(_) => format!("{}(&{a}, &{b})", self.eq_fn(ty)),
            Type::List(_) => format!("{}({a}, {b})", self.eq_fn(ty)),
            _ => format!("{a} == {b}"),
        }
    }

    /// Whether `eq_c` for this type is a call (and so needs no
    /// parentheses as an operand).
    pub fn eq_is_call(ty: &Type) -> bool {
        matches!(
            ty,
            Type::String | Type::Entity | Type::Record(_) | Type::List(_)
        )
    }

    /// The function that compares two records or two lists:
    /// `(const T_X *, const T_X *)` or `(MnList, MnList)`.
    pub fn eq_fn(&mut self, ty: &Type) -> String {
        let prog = self.prog;
        let name = format!("Eq_{}", names::mangle(prog, ty));
        if !self.claim(&name) {
            return name;
        }
        let mut body = String::new();
        let signature = match ty {
            Type::Record(id) => {
                let item = &prog.records[id.0];
                let mut terms = Vec::new();
                for field in &item.fields {
                    let member = names::field(&field.name);
                    terms.push(self.eq_c(
                        &format!("a->{member}"),
                        &format!("b->{member}"),
                        &field.ty,
                    ));
                }
                if terms.is_empty() {
                    body.push_str("    (void)a;\n    (void)b;\n    return true;\n");
                } else {
                    let _ = writeln!(body, "    return {};", terms.join("\n        && "));
                }
                let cty = self.cty(ty);
                format!("static bool {name}(const {cty} *a, const {cty} *b)")
            }
            Type::List(elem) => {
                let cty = self.cty(elem);
                let equal = self.eq_c("x[i]", "y[i]", elem);
                let _ = writeln!(body, "    int64_t n = mn_list_len(a);");
                let _ = writeln!(body, "    if (n != mn_list_len(b)) return false;");
                let _ = writeln!(body, "    for (int64_t i = 0; i < n; i++) {{");
                let _ = writeln!(body, "        const {cty} *x = mn_list_data(a);");
                let _ = writeln!(body, "        const {cty} *y = mn_list_data(b);");
                let _ = writeln!(body, "        if (!({equal})) return false;");
                let _ = writeln!(body, "    }}");
                let _ = writeln!(body, "    return true;");
                format!("static bool {name}(MnList a, MnList b)")
            }
            _ => unreachable!("only records and lists have equality functions"),
        };
        let _ = writeln!(self.helper_protos, "{signature};");
        let _ = writeln!(self.helper_defs, "{signature} {{\n{body}}}\n");
        name
    }

    /// The function behind `include?` for a list type:
    /// `(MnList, value) -> bool`.
    pub fn include_fn(&mut self, list: &Type) -> String {
        let Type::List(elem) = list else {
            unreachable!("include? is a list method");
        };
        let name = format!("Has_{}", names::mangle(self.prog, list));
        if !self.claim(&name) {
            return name;
        }
        let value = elem.value_type();
        let cty = self.cty(elem);
        let equal = match &**elem {
            Type::Compact(_) => format!("({})d[i] == v", self.cty(&value)),
            _ => self.eq_c("d[i]", "v", elem),
        };
        let signature = format!("static bool {name}(MnList list, {} v)", self.cty(&value));
        let _ = writeln!(self.helper_protos, "{signature};");
        let _ = writeln!(
            self.helper_defs,
            "{signature} {{\n    \
             int64_t n = mn_list_len(list);\n    \
             for (int64_t i = 0; i < n; i++) {{\n        \
             const {cty} *d = mn_list_data(list);\n        \
             if ({equal}) return true;\n    \
             }}\n    \
             return false;\n}}\n"
        );
        name
    }
}

/// A statement that appends literal text to the builder `sb` points at.
pub fn sb_bytes(sb: &str, text: &str) -> String {
    format!(
        "mn_sb_bytes({sb}, {}, {});",
        names::c_string(text.as_bytes()),
        text.len()
    )
}
