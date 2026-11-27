//! Names used in the generated C, and C string literals.
//!
//! | Thing                    | C name                                   |
//! | ------------------------ | ---------------------------------------- |
//! | record `Foo`             | `T_Foo`                                  |
//! | enum `Team`              | `E_Team`                                 |
//! | field `x`                | `x` (`f_x` when `x` can't be used in C)  |
//! | function `clamp`         | `F_clamp`                                |
//! | function `alive?`, `go!` | `Fq_alive`, `Fb_go`                      |
//! | method (function id 7)   | `M7_Vec2_length`                         |
//! | system `move`            | `S_move`                                 |
//! | constant `GRAVITY`       | `C_GRAVITY`                              |
//! | resource `Score`         | `R_Score`                                |
//! | local `pos` (local id 3) | `pos_3`                                  |
//!
//! Every file-scope name that contains program text starts with an
//! uppercase letter and a fixed prefix; every local ends in `_` and its id.
//! Names the emitter makes up itself (`t4`, `q0`, `str2`, `c0`, `table`)
//! match neither pattern.

use crate::tir::{Program, Type};

/// Words a field can't be called in C: keywords, and lowercase macros of
/// the headers the generated code includes.
const RESERVED: &[&str] = &[
    "alignas",
    "alignof",
    "asm",
    "auto",
    "bool",
    "break",
    "case",
    "char",
    "complex",
    "const",
    "continue",
    "default",
    "do",
    "double",
    "else",
    "enum",
    "errno",
    "extern",
    "false",
    "float",
    "for",
    "goto",
    "i386",
    "if",
    "imaginary",
    "inline",
    "int",
    "linux",
    "long",
    "math_errhandling",
    "noreturn",
    "offsetof",
    "register",
    "restrict",
    "return",
    "short",
    "signed",
    "sizeof",
    "static",
    "static_assert",
    "stderr",
    "stdin",
    "stdout",
    "struct",
    "switch",
    "thread_local",
    "true",
    "typedef",
    "typeof",
    "union",
    "unix",
    "unsigned",
    "void",
    "volatile",
    "while",
];

/// `name` with every character C can't have in an identifier replaced.
pub fn sanitize(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

pub fn local(name: &str, id: usize) -> String {
    let base = match name.strip_prefix('$') {
        Some(hidden) => format!("h_{}", sanitize(hidden)),
        None => sanitize(name),
    };
    if base.starts_with('_') {
        format!("u{base}_{id}")
    } else {
        format!("{base}_{id}")
    }
}

pub fn field(name: &str) -> String {
    let name = sanitize(name);
    if name.starts_with('_') || name.starts_with("f_") || RESERVED.contains(&name.as_str()) {
        format!("f_{name}")
    } else {
        name
    }
}

pub fn record(name: &str) -> String {
    format!("T_{}", sanitize(name))
}

pub fn enum_(name: &str) -> String {
    format!("E_{}", sanitize(name))
}

pub fn system(name: &str) -> String {
    format!("S_{}", sanitize(name))
}

pub fn constant(name: &str) -> String {
    format!("C_{}", sanitize(name))
}

pub fn resource(name: &str) -> String {
    format!("R_{}", sanitize(name))
}

pub fn func(prog: &Program, id: usize) -> String {
    let func = &prog.funcs[id];
    let (base, suffix) = match func.name.as_str() {
        "+" => ("add", ""),
        "-" => ("sub", ""),
        "*" => ("mul", ""),
        "/" => ("div", ""),
        "%" => ("mod", ""),
        "-@" => ("neg", ""),
        name => match name.strip_suffix('?') {
            Some(base) => (base, "q"),
            None => match name.strip_suffix('!') {
                Some(base) => (base, "b"),
                None => (name, ""),
            },
        },
    };
    let base = sanitize(base);
    match func.owner {
        Some(owner) => {
            let record = sanitize(&prog.records[owner.0].name);
            let mark = if suffix.is_empty() { "" } else { "_" };
            format!("M{id}_{record}_{base}{mark}{suffix}")
        }
        None => format!("F{suffix}_{base}"),
    }
}

/// A type as part of a helper's name: `Int`, `T_Vec2`, `L_L_Float32`.
pub fn mangle(prog: &Program, ty: &Type) -> String {
    match ty {
        Type::Void => "Void".to_string(),
        Type::Int => "Int".to_string(),
        Type::Float => "Float".to_string(),
        Type::Bool => "Bool".to_string(),
        Type::String => "String".to_string(),
        Type::Entity => "Entity".to_string(),
        Type::Compact(compact) => compact.name().to_string(),
        Type::List(elem) => format!("L_{}", mangle(prog, elem)),
        Type::Record(id) => record(&prog.records[id.0].name),
        Type::Enum(id) => enum_(&prog.enums[id.0].name),
    }
}

/// A C string literal holding exactly these bytes.
pub fn c_string(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() + 2);
    out.push('"');
    for &byte in bytes {
        match byte {
            b'"' => out.push_str("\\\""),
            b'\\' => out.push_str("\\\\"),
            b'?' => out.push_str("\\?"),
            b'\n' => out.push_str("\\n"),
            b'\t' => out.push_str("\\t"),
            b'\r' => out.push_str("\\r"),
            0x20..=0x7e => out.push(byte as char),
            _ => out.push_str(&format!("\\{byte:03o}")),
        }
    }
    out.push('"');
    out
}

/// A `Float` as a C expression of type `double`.
pub fn float_lit(value: f64) -> String {
    if value.is_nan() {
        "(double)NAN".to_string()
    } else if value == f64::INFINITY {
        "(double)INFINITY".to_string()
    } else if value == f64::NEG_INFINITY {
        "-(double)INFINITY".to_string()
    } else {
        format!("{value:?}")
    }
}

/// An `Int` as a C expression.
pub fn int_lit(value: i64) -> String {
    if value == i64::MIN {
        "INT64_MIN".to_string()
    } else {
        value.to_string()
    }
}

/// The suffix for the loop variables of an `each` nested `depth` deep.
pub fn depth_suffix(depth: usize) -> String {
    match depth {
        0 => String::new(),
        1..=25 => ((b'a' + depth as u8) as char).to_string(),
        _ => format!("z{depth}x"),
    }
}

/// The runtime's name for the kind of a shape's record.
pub fn draw_kind(shape: crate::tir::Shape) -> &'static str {
    match shape {
        crate::tir::Shape::Rect => "MN_DRAW_RECT",
        crate::tir::Shape::Circle => "MN_DRAW_CIRCLE",
        crate::tir::Shape::Line => "MN_DRAW_LINE",
        crate::tir::Shape::Triangle => "MN_DRAW_TRIANGLE",
    }
}
