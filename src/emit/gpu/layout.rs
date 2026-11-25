//! Where things are: the bytes of a record as C lays it out, and the
//! uniform words that hold the constants and resource fields GPU code
//! reads.

use std::collections::HashMap;

use crate::tir::{Compact, Program, Type};

use super::super::names;
use super::super::types;
use super::ir::{self, Expr, Storage, Ty};

/// The size of a record's C struct and the byte offset of each field, by
/// field index.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecordLayout {
    pub size: u32,
    pub offsets: Vec<u32>,
}

/// A field of a component that holds no further fields.
#[derive(Clone, Debug, PartialEq)]
pub struct Leaf {
    /// Field indexes, from the component's record down.
    pub path: Vec<usize>,
    /// The member in C, written after the row: `.pos.x`.
    pub member: String,
    /// The members of the 32-bit value, from the variable down.
    pub fields: Vec<String>,
    /// Byte offset in the row.
    pub offset: u32,
    pub storage: Storage,
}

/// The name of a field in the 32-bit form of a record. The prefix keeps
/// every field clear of the words WGSL reserves.
pub fn field_name(name: &str) -> String {
    format!("f_{}", names::sanitize(name))
}

/// The member that stands in for the fields of a record that has none.
pub const EMPTY_FIELD: &str = "f_";

pub struct Layouts<'a> {
    pub prog: &'a Program,
    /// By record: its alignment is raised to 4.
    pub wide: &'a [bool],
}

fn round_up(value: u32, align: u32) -> u32 {
    value.div_ceil(align) * align
}

impl Layouts<'_> {
    pub fn align(&self, ty: &Type) -> u32 {
        types::align_of(self.prog, self.wide, ty)
    }

    pub fn size(&self, ty: &Type) -> u32 {
        match ty {
            Type::Void => 0,
            Type::Bool => 1,
            Type::Int | Type::Float | Type::String | Type::List(_) | Type::Entity => 8,
            Type::Compact(compact) => match compact {
                Compact::Float32 | Compact::Int32 | Compact::UInt32 => 4,
                Compact::Int16 | Compact::UInt16 => 2,
                Compact::Int8 | Compact::UInt8 => 1,
            },
            Type::Enum(id) => types::enum_bytes(self.prog, *id),
            Type::Record(id) => self.record(id.0).size,
        }
    }

    pub fn record(&self, record: usize) -> RecordLayout {
        let fields = &self.prog.records[record].fields;
        let order = types::layout_order(self.prog, self.wide, fields.iter().map(|f| &f.ty));
        let mut offsets = vec![0; fields.len()];
        let mut end = 0;
        for index in order {
            let ty = &fields[index].ty;
            let start = round_up(end, self.align(ty));
            offsets[index] = start;
            end = start + self.size(ty);
        }
        // A record without fields is one `char`.
        let align = self.align(&Type::Record(crate::tir::RecordId(record)));
        RecordLayout {
            size: round_up(end.max(1), align),
            offsets,
        }
    }

    /// The leaves of a component's record, in source order.
    pub fn leaves(&self, record: usize) -> Vec<Leaf> {
        let mut out = Vec::new();
        self.collect(record, 0, &mut Vec::new(), "", &mut Vec::new(), &mut out);
        out
    }

    fn collect(
        &self,
        record: usize,
        base: u32,
        path: &mut Vec<usize>,
        member: &str,
        fields: &mut Vec<String>,
        out: &mut Vec<Leaf>,
    ) {
        let layout = self.record(record);
        for (index, field) in self.prog.records[record].fields.iter().enumerate() {
            let offset = base + layout.offsets[index];
            let member = format!("{member}.{}", names::field(&field.name));
            path.push(index);
            fields.push(field_name(&field.name));
            match &field.ty {
                Type::Record(inner) => self.collect(inner.0, offset, path, &member, fields, out),
                other => out.push(Leaf {
                    path: path.clone(),
                    member,
                    fields: fields.clone(),
                    offset,
                    storage: self.storage(other),
                }),
            }
            path.pop();
            fields.pop();
        }
    }

    fn storage(&self, ty: &Type) -> Storage {
        match ty {
            Type::Bool => Storage::Bool,
            Type::Entity => Storage::Entity,
            Type::Enum(id) => Storage::Enum(types::enum_bytes(self.prog, *id)),
            Type::Compact(Compact::Float32) => Storage::F32,
            Type::Compact(Compact::Int8) => Storage::I8,
            Type::Compact(Compact::Int16) => Storage::I16,
            Type::Compact(Compact::Int32) => Storage::I32,
            Type::Compact(Compact::UInt8) => Storage::U8,
            Type::Compact(Compact::UInt16) => Storage::U16,
            Type::Compact(Compact::UInt32)
            | Type::Int
            | Type::Float
            | Type::String
            | Type::List(_)
            | Type::Void
            | Type::Record(_) => unreachable!(
                "the checker rejects a component with such a field as a term of a GPU query"
            ),
        }
    }
}

/// The type a value of this type has in GPU code.
pub fn value_ty(ty: &Type) -> Ty {
    match ty {
        Type::Void => Ty::Void,
        Type::Int => Ty::I32,
        Type::Float | Type::Compact(Compact::Float32) => Ty::F32,
        Type::Compact(_) => Ty::I32,
        Type::Bool => Ty::Bool,
        Type::Entity => Ty::Entity,
        Type::Enum(_) => Ty::U32,
        Type::Record(id) => Ty::Record(id.0),
        Type::String | Type::List(_) => {
            unreachable!("the checker keeps strings and lists out of GPU code")
        }
    }
}

/// The 32-bit form of a record.
pub fn gpu_struct(prog: &Program, record: usize) -> ir::Struct {
    let item = &prog.records[record];
    let mut fields: Vec<(String, Ty)> = item
        .fields
        .iter()
        .map(|field| (field_name(&field.name), value_ty(&field.ty)))
        .collect();
    if fields.is_empty() {
        fields.push((EMPTY_FIELD.to_string(), Ty::U32));
    }
    ir::Struct {
        name: format!("G_{}", names::sanitize(&item.name)),
        fields,
    }
}

/// The all-zero value of a type.
pub fn zero(prog: &Program, ty: Ty) -> Expr {
    match ty {
        Ty::Void => unreachable!("no value of no type"),
        Ty::I32 => Expr::I32(0),
        Ty::U32 => Expr::U32(0),
        Ty::F32 => Expr::F32(0.0),
        Ty::Bool => Expr::Bool(false),
        Ty::Entity => Expr::Construct(Ty::Entity, vec![Expr::U32(0), Expr::U32(0)]),
        Ty::Record(id) => {
            let fields = gpu_struct(prog, id).fields;
            Expr::Construct(ty, fields.iter().map(|(_, ty)| zero(prog, *ty)).collect())
        }
    }
}

/// Something GPU code reads through the uniform words.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Source {
    /// A constant whose value is not known to the compiler, by id.
    Const(usize),
    /// A resource field, by resource id and field index.
    Resource(usize, usize),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub source: Source,
    /// The first of its words.
    pub base: u32,
    pub words: u32,
}

/// The first words of every block's uniform data, which the runtime
/// fills: rows, snapshot rows, seed, block id, tick, dt, elapsed, zero.
pub const HEADER: [Ty; 8] = [
    Ty::U32,
    Ty::U32,
    Ty::U32,
    Ty::U32,
    Ty::I32,
    Ty::F32,
    Ty::F32,
    Ty::U32,
];
pub const WORD_SNAPSHOT_ROWS: u32 = 1;
pub const WORD_SEED: u32 = 2;
pub const WORD_BLOCK: u32 = 3;
pub const WORD_TICK: u32 = 4;
pub const WORD_DT: u32 = 5;
pub const WORD_ELAPSED: u32 = 6;

/// The uniform words of the program. One layout serves every block, so
/// that a function shared by several blocks reads the same words in each:
/// after the header, each constant or resource field that GPU code reads
/// has its words, in the order they were first read. A value is flattened
/// in the source order of its fields: a `Float` is one `F32`, an `Int` one
/// `I32`, a `Bool` or an enum one `U32`, an entity two `U32`s.
pub struct Uniforms {
    pub entries: Vec<Entry>,
    index: HashMap<Source, usize>,
    /// The type of every word, from word 0.
    pub words: Vec<Ty>,
}

impl Uniforms {
    pub fn new() -> Uniforms {
        Uniforms {
            entries: Vec::new(),
            index: HashMap::new(),
            words: HEADER.to_vec(),
        }
    }

    /// The value at `path` (field indexes) inside a source of type `ty`.
    pub fn read(&mut self, prog: &Program, source: Source, ty: &Type, path: &[usize]) -> Expr {
        let entry = match self.index.get(&source) {
            Some(&entry) => entry,
            None => {
                let base = self.words.len() as u32;
                word_types(prog, ty, &mut self.words);
                self.entries.push(Entry {
                    source,
                    base,
                    words: self.words.len() as u32 - base,
                });
                self.index.insert(source, self.entries.len() - 1);
                self.entries.len() - 1
            }
        };
        let mut word = self.entries[entry].base;
        let mut current = ty;
        for &index in path {
            let Type::Record(record) = current else {
                unreachable!("a field of something that is not a record");
            };
            let fields = &prog.records[record.0].fields;
            word += fields[..index]
                .iter()
                .map(|field| word_count(prog, &field.ty))
                .sum::<u32>();
            current = &fields[index].ty;
        }
        value(prog, current, word)
    }

    /// The entry a word belongs to. `None` for the header.
    pub fn entry_of(&self, word: u32) -> Option<usize> {
        self.entries
            .iter()
            .position(|entry| (entry.base..entry.base + entry.words).contains(&word))
    }
}

fn word_types(prog: &Program, ty: &Type, out: &mut Vec<Ty>) {
    match ty {
        Type::Float | Type::Compact(Compact::Float32) => out.push(Ty::F32),
        Type::Int | Type::Compact(_) => out.push(Ty::I32),
        Type::Bool | Type::Enum(_) => out.push(Ty::U32),
        Type::Entity => out.extend([Ty::U32, Ty::U32]),
        Type::Record(id) => {
            for field in &prog.records[id.0].fields {
                word_types(prog, &field.ty, out);
            }
        }
        Type::Void | Type::String | Type::List(_) => {
            unreachable!("the checker lets GPU code read only plain constants and resource fields")
        }
    }
}

pub fn word_count(prog: &Program, ty: &Type) -> u32 {
    let mut words = Vec::new();
    word_types(prog, ty, &mut words);
    words.len() as u32
}

/// The value of type `ty` whose words start at `word`.
fn value(prog: &Program, ty: &Type, word: u32) -> Expr {
    match ty {
        Type::Float | Type::Compact(Compact::Float32) => Expr::Uniform(word, Ty::F32),
        Type::Int | Type::Compact(_) => Expr::Uniform(word, Ty::I32),
        Type::Bool => ir::binary(ir::BinOp::Ne, Expr::Uniform(word, Ty::U32), Expr::U32(0)),
        Type::Enum(_) => Expr::Uniform(word, Ty::U32),
        Type::Entity => Expr::Construct(
            Ty::Entity,
            vec![
                Expr::Uniform(word, Ty::U32),
                Expr::Uniform(word + 1, Ty::U32),
            ],
        ),
        Type::Record(id) => {
            let mut word = word;
            let mut fields = Vec::new();
            for field in &prog.records[id.0].fields {
                fields.push(value(prog, &field.ty, word));
                word += word_count(prog, &field.ty);
            }
            if fields.is_empty() {
                fields.push(Expr::U32(0));
            }
            Expr::Construct(Ty::Record(id.0), fields)
        }
        Type::Void | Type::String | Type::List(_) => {
            unreachable!("the checker lets GPU code read only plain constants and resource fields")
        }
    }
}
