use crate::{
    interp::{solver::Solver, walk::Walk},
    sem::Shape,
};
use ruff_index::newtype_index;
use smallvec::SmallVec;
use smallvec::smallvec;
use ty_python_core::definition::Definition;
use ty_python_semantic::types::Type;
// src/interp/value.rs
/// One run of a function body inside the walk. Walk-local: never leaves a summary.
#[newtype_index]
pub struct ActivationId;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum AbstractValue<'db> {
    Function(Definition<'db>), // a def: something you can walk
    Closure {
        func: Definition<'db>,
        activation: ActivationId, // the run whose `def` statement made it
    },
    Ty(Type<'db>), // ty's answer, for everything you don't model
    Module(Type<'db>),
    Unknown, // nothing known
}

/// Several possibilities, kept apart. Never collapsed into one ty union.
pub type Values<'db> = SmallVec<[AbstractValue<'db>; 2]>;

pub fn dedup(vals: &mut Values<'_>) {
    let mut seen = Vec::with_capacity(vals.len());
    vals.retain(|v| {
        if seen.contains(v) {
            false
        } else {
            seen.push(*v);
            true
        }
    });
}
impl<'db> Walk<'db> {
    pub fn lift(&self, sol: &Solver<'db>, ty: Type<'db>) -> Values<'db> {
        match sol.sem.shape(ty, self.frame.file) {
            Shape::Module => smallvec![AbstractValue::Module(ty)],
            Shape::Function(d) => smallvec![AbstractValue::Function(d)],
            Shape::Union(members) => members.into_iter().flat_map(|t| self.lift(sol, t)).collect(),
            Shape::Unknown => smallvec![AbstractValue::Unknown],
            Shape::Other => smallvec![AbstractValue::Ty(ty)],
        }
    }
}
