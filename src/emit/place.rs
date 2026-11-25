//! Places: where a value lives, and how generated code gets to it.
//!
//! A place is used in two steps. `prepare` evaluates its operands (the
//! entity of `e[C]`, the indexes) into values that stay valid while other
//! code runs. `addr` then generates the lookups and returns the lvalue,
//! which must be used before any user code runs again.

use crate::source::Span;
use crate::tir::{Compact, Expr, Place, PlaceKind, TimeField, Type};

use super::analysis::{self, Folded, HoistKey};
use super::body::{ATOM, Dep, FnCx, Own, UNARY, Val};
use super::names;
use super::types;

enum Root {
    /// An lvalue that needs no lookup.
    Lv(String),
    /// `entity[Component]`.
    Component {
        entity: Val,
        id: usize,
        cty: String,
        loc: String,
        /// For a component that can have a GPU copy: what this use does
        /// with it, an `MN_ROW_` constant.
        sync: Option<&'static str>,
    },
}

enum Step {
    Field(String),
    Index {
        index: Val,
        /// The element's storage type.
        elem: Type,
        loc: String,
    },
}

/// A place whose operands have been evaluated.
pub(super) struct Prepared {
    root: Root,
    steps: Vec<Step>,
    /// Whether the value lives in a local, a constant or a list one of
    /// them holds, where other functions can't change it.
    pub stable: bool,
    /// What can change the value, when the place needs no lookup.
    pub dep: Dep,
    /// Whether the root is a variable that is never assigned again.
    pub slot: bool,
}

impl Prepared {
    /// Whether the lvalue is found without calling the runtime.
    pub fn direct(&self) -> bool {
        matches!(self.root, Root::Lv(_))
            && self.steps.iter().all(|step| matches!(step, Step::Field(_)))
    }
}

/// The lvalue of a place.
pub(super) struct Lv {
    pub c: String,
    pub prec: u8,
    /// Whether no lookup was generated: the expression can be evaluated
    /// later or more than once.
    pub direct: bool,
}

impl Lv {
    /// The address of the lvalue.
    pub fn addr(&self) -> String {
        match self.c.strip_prefix('*') {
            Some(pointer) => pointer.to_string(),
            None => format!("&{}", self.c),
        }
    }
}

/// The C expression that reads storage of type `ty` through lvalue `c` as
/// a value, and how it binds.
pub(super) fn load(c: &str, prec: u8, ty: &Type) -> (String, u8) {
    match ty {
        Type::Compact(Compact::Float32) => (format!("(double){c}"), UNARY),
        Type::Compact(_) => (format!("(int64_t){c}"), UNARY),
        _ => (c.to_string(), prec),
    }
}

impl<'a> FnCx<'a, '_> {
    /// Evaluates the operands of a place. `write` says whether the place
    /// will be written. The `tail` flags describe the code that will run
    /// between this call and `addr`: whether it is inert, and whether it
    /// leaves the locals alone.
    pub(super) fn prepare(
        &mut self,
        place: &'a Place,
        write: bool,
        tail_inert: bool,
        tail_safe: bool,
    ) -> Prepared {
        let mut chain: Vec<&'a Place> = Vec::new();
        let mut current = place;
        loop {
            chain.push(current);
            match &current.kind {
                PlaceKind::Field { base, .. } | PlaceKind::Index { base, .. } => current = base,
                _ => break,
            }
        }
        chain.reverse();
        let root_place = chain[0];

        let mut operands: Vec<&'a Expr> = Vec::new();
        if let PlaceKind::Component { entity, .. } = &root_place.kind {
            operands.push(entity);
        }
        for &link in &chain[1..] {
            if let PlaceKind::Index { index, .. } = &link.kind {
                operands.push(index);
            }
        }
        let mut values = self
            .operands_then(&operands, super::expr::Want::Peek, tail_inert, tail_safe)
            .into_iter();

        if write
            && let PlaceKind::Local(id) = &root_place.kind
            && let Some(flag) = self.flags.get(&id.0)
        {
            let line = format!("{flag} = true;");
            self.line(&line);
        }
        let (root, stable, dep) = match &root_place.kind {
            PlaceKind::Local(id) => match self.views.get(&id.0) {
                Some(view) => (Root::Lv(view.clone()), false, Dep::World),
                None => {
                    let dep = if self.fixed[id.0] {
                        Dep::Fixed
                    } else {
                        Dep::Local
                    };
                    (Root::Lv(self.names[id.0].clone()), true, dep)
                }
            },
            PlaceKind::Const(id) => {
                let name = names::constant(&self.cx.prog.consts[id.0].name);
                (Root::Lv(name), true, Dep::Fixed)
            }
            PlaceKind::ResourceField(resource, field) => {
                let key = HoistKey::Resource(resource.0, *field);
                match self.hoisted(key, write) {
                    Some(name) => (Root::Lv(name), false, Dep::Fixed),
                    None => {
                        let item = &self.cx.prog.resources[resource.0];
                        let lv = format!(
                            "{}.{}",
                            names::resource(&item.name),
                            names::field(&item.fields[*field].name)
                        );
                        (Root::Lv(lv), false, Dep::World)
                    }
                }
            }
            PlaceKind::Time(field) => match self.hoisted(analysis::time_key(*field), write) {
                Some(name) => (Root::Lv(name), false, Dep::Fixed),
                None => (Root::Lv(time_lvalue(*field).to_string()), false, Dep::World),
            },
            PlaceKind::Component {
                component, write, ..
            } => {
                let record = self.cx.prog.components[component.0]
                    .record
                    .expect("a tag has no value to reach");
                let root = Root::Component {
                    entity: values.next().expect("the entity was evaluated"),
                    id: component.0,
                    cty: self.cx.cty(&Type::Record(record)),
                    loc: self.cx.loc(root_place.span),
                    sync: self.cx.gpu.named[component.0].then_some(if !*write {
                        "MN_ROW_READ"
                    } else if chain.len() == 1 && self.whole_store {
                        "MN_ROW_WHOLE"
                    } else {
                        "MN_ROW_PART"
                    }),
                };
                (root, false, Dep::World)
            }
            PlaceKind::Field { .. } | PlaceKind::Index { .. } => {
                unreachable!("the chain starts at its root")
            }
        };

        let mut steps = Vec::new();
        for &link in &chain[1..] {
            match &link.kind {
                PlaceKind::Field { base, field } => {
                    let Type::Record(record) = &base.ty else {
                        unreachable!("a field of something that is not a record");
                    };
                    let name = &self.cx.prog.records[record.0].fields[*field].name;
                    steps.push(Step::Field(names::field(name)));
                }
                PlaceKind::Index { base, .. } => {
                    let Type::List(elem) = &base.ty else {
                        unreachable!("an index into something that is not a list");
                    };
                    steps.push(Step::Index {
                        index: values.next().expect("the index was evaluated"),
                        elem: (**elem).clone(),
                        loc: self.cx.loc(link.span),
                    });
                }
                _ => unreachable!("only fields and indexes follow the root"),
            }
        }
        let slot = match &root_place.kind {
            PlaceKind::Local(id) => !self.views.contains_key(&id.0) && self.fixed[id.0],
            PlaceKind::Const(_) => true,
            _ => false,
        };
        Prepared {
            root,
            steps,
            stable,
            dep,
            slot,
        }
    }

    /// The local that holds a hoisted resource or `Time` field, for a
    /// read inside a loop that hoisted it.
    fn hoisted(&self, key: HoistKey, write: bool) -> Option<String> {
        if write {
            return None;
        }
        self.hoists
            .iter()
            .rev()
            .find_map(|level| level.get(&key).cloned())
    }

    /// Generates the lookups of a prepared place and returns its lvalue.
    /// With `write`, every list on the way is made private first.
    pub(super) fn addr(&mut self, place: &Prepared, write: bool) -> Lv {
        let konst = if write { "" } else { "const " };
        let mut direct = true;
        let (mut current, mut pointer) = match &place.root {
            Root::Lv(c) => (c.clone(), false),
            Root::Component {
                entity,
                id,
                cty,
                loc,
                sync,
            } => {
                let name = self.fresh("p");
                let get = match sync {
                    Some(usage) => format!(
                        "mn_get_synced({}, {id}, sizeof({cty}), {usage}, {loc})",
                        entity.c
                    ),
                    None => format!("mn_get({}, {id}, sizeof({cty}), {loc})", entity.c),
                };
                self.line(&format!("{konst}{cty} *{name} = {get};"));
                direct = false;
                (name, true)
            }
        };
        for step in &place.steps {
            match step {
                Step::Field(name) => {
                    current = if pointer {
                        format!("{current}->{name}")
                    } else {
                        format!("{current}.{name}")
                    };
                    pointer = false;
                }
                Step::Index { index, elem, loc } => {
                    let list = if pointer {
                        format!("(*{current})")
                    } else {
                        current.clone()
                    };
                    if write {
                        let elem = self.cx.elem(elem);
                        let target = if pointer {
                            current.clone()
                        } else {
                            format!("&{list}")
                        };
                        self.line(&format!("mn_list_unique({target}, &{elem});"));
                    }
                    let position = self.tmp();
                    self.line(&format!(
                        "int64_t {position} = mn_list_index({list}, {}, {loc});",
                        index.c
                    ));
                    let name = self.fresh("p");
                    let cty = self.cx.cty(elem);
                    self.line(&format!(
                        "{konst}{cty} *{name} = ({konst}{cty} *)mn_list_data({list}) + {position};"
                    ));
                    current = name;
                    pointer = true;
                    direct = false;
                }
            }
        }
        if pointer {
            Lv {
                c: format!("*{current}"),
                prec: UNARY,
                direct,
            }
        } else {
            Lv {
                c: current,
                prec: ATOM,
                direct,
            }
        }
    }

    /// The current value of a place.
    pub(super) fn read_place(&mut self, place: &'a Place) -> Val {
        if let PlaceKind::Const(id) = &place.kind
            && let Some(value) = self.cx.folded[id.0]
        {
            return match value {
                Folded::Int(value) => Val::int(value),
                Folded::Float(value) => Val::float(value),
                Folded::Bool(value) => Val::atom(if value { "true" } else { "false" }),
            };
        }
        let prepared = self.prepare(place, false, true, true);
        let lv = self.addr(&prepared, false);
        let value_type = place.ty.value_type();
        let own = if self.cx.managed(&value_type) {
            Own::Borrowed {
                stable: prepared.stable,
            }
        } else {
            Own::Free
        };
        let (c, prec) = load(&lv.c, lv.prec, &place.ty);
        if lv.direct {
            Val {
                simple: c == lv.c,
                slot: c == lv.c && prepared.slot,
                c,
                prec,
                lit: None,
                own,
                dep: prepared.dep,
                pending: false,
            }
        } else {
            Val {
                prec,
                dep: borrow_dep(own),
                ..Val::pending(c, own)
            }
        }
    }

    /// The C expression that converts a value for storage as `storage`.
    /// A compact integer is range-checked unless the value is a literal
    /// that fits.
    pub(super) fn store_c(&mut self, value: &Val, storage: &Type, span: Span) -> String {
        match storage {
            Type::Compact(Compact::Float32) => format!("(float){}", value.p(UNARY)),
            Type::Compact(compact) => {
                let (low, high) = compact.int_range().expect("an integer type");
                match value.lit {
                    Some(literal) if (low..=high).contains(&literal) => value.c.clone(),
                    _ => format!(
                        "{}({}, {})",
                        types::narrow_fn(*compact),
                        value.c,
                        self.cx.loc(span)
                    ),
                }
            }
            _ => value.c.clone(),
        }
    }

    /// As `store_c`, with any range check run now, so that the result has
    /// no effects.
    pub(super) fn store_checked(&mut self, value: &Val, storage: &Type, span: Span) -> String {
        let c = self.store_c(value, storage, span);
        match storage {
            Type::Compact(compact) if c != value.c && *compact != Compact::Float32 => {
                self.declare(storage, &c)
            }
            _ => c,
        }
    }
}

/// What can end the validity of a value read through a lookup: for a
/// borrowed one, whatever can change the storage it was read from.
pub(super) fn borrow_dep(own: Own) -> Dep {
    match own {
        Own::Borrowed { stable: true } => Dep::Local,
        Own::Borrowed { stable: false } => Dep::World,
        Own::Owned | Own::Free => Dep::Fixed,
    }
}

fn time_lvalue(field: TimeField) -> &'static str {
    match field {
        TimeField::Tick => "mn_time.tick",
        TimeField::Dt => "mn_time.dt",
        TimeField::Elapsed => "mn_time.elapsed",
    }
}
