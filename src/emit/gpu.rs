//! GPU systems. Each `each` of a `gpu system` is a GPU block, which
//! becomes a C function, a WGSL compute shader with the same meaning, and
//! the `MnGpuBlock` that describes both to the runtime.
//!
//! [`lower`] walks the typed tree once and produces the intermediate form
//! in [`ir`]; [`c`] and [`wgsl`] print it. Code in a GPU block, and in a
//! function while it is called from one, computes at 32 bits, so the
//! functions a block reaches are lowered again, apart from their ordinary
//! 64-bit form.
//!
//! | Thing                              | Name                          |
//! | ---------------------------------- | ----------------------------- |
//! | 32-bit form of record `Vec2`       | `G_Vec2`                      |
//! | its field `x`                      | `f_x`                         |
//! | 32-bit form of function `F_clamp`  | `GF_clamp`                    |
//! | equality of two `G_Vec2`           | `Geq_Vec2`                    |
//! | helpers                            | `g32_div`, `g32_rand`, …      |
//! | block 0 and what belongs to it     | `Gb0`, `Gb0_cpu`, `Gb0_fill`, `Gb0_wgsl`, `Gb0_outer`, `Gb0_inner` |
//! | outer and inner columns            | `c0`, `c1`, … and `s0`, `s1`, … |
//! | the run's row, the snapshot's row  | `row`, `j`                    |
//!
//! Locals keep the names they have in ordinary code (`pos_3`), which end
//! in their id; no other name here does. Every field has the prefix `f_`
//! because WGSL reserves many plain words (`target`, `filter`, `mod`).

pub(super) mod c;
mod helpers;
pub(super) mod ir;
pub(super) mod layout;
mod lower;
pub(super) mod wgsl;

use crate::tir::{Program, StmtKind, Term};

use super::analysis::{self, Node};

pub(super) use lower::lower;

/// What the GPU systems of a program mean for the code of everything
/// else.
pub(super) struct Plan {
    /// By component: some GPU block has it as a query term, so its
    /// columns can have a GPU copy.
    pub named: Vec<bool>,
    /// By record: it is the record of a component some GPU block writes.
    /// Its alignment is raised to 4, which makes every row whole words.
    pub wide: Vec<bool>,
}

impl Plan {
    pub fn new(prog: &Program) -> Plan {
        let mut plan = Plan {
            named: vec![false; prog.components.len()],
            wide: vec![false; prog.records.len()],
        };
        for system in prog.systems.iter().filter(|system| system.gpu) {
            analysis::walk_block(&system.body.block, &mut |node| {
                let Node::Stmt(stmt) = node else {
                    return;
                };
                let StmtKind::Each { query, .. } = &stmt.kind else {
                    return;
                };
                for term in &query.terms {
                    let Term::Component { component, write } = term else {
                        continue;
                    };
                    plan.named[component.0] = true;
                    if *write && let Some(record) = prog.components[component.0].record {
                        plan.wide[record.0] = true;
                    }
                }
            });
        }
        plan
    }
}
