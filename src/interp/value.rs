use crate::{
    interp::{solver::Solver, walk::Walk},
    sem::Shape,
};
use ruff_index::newtype_index;
use crate::interp::key::Interner;
use ruff_text_size::TextRange;
use smallvec::SmallVec;
use smallvec::smallvec;
use ty_python_core::definition::Definition;
use ty_python_semantic::types::Type;
// src/interp/value.rs
/// One run of a function body inside the walk. Walk-local: never leaves a summary.
#[newtype_index]
pub struct ActivationId;

/// An interned `AbstractValue`, so a value can hold another and stay `Copy`.
#[newtype_index]
pub struct ValueId;

/// Per walk. The same value is interned once.
pub type ValueArena<'db> = Interner<AbstractValue<'db>, ValueId>;

/// One object, named by where it was built. Walk-local, like `ActivationId`.
#[newtype_index]
pub struct OriginId;

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum OriginKey {
    /// An object built in this walk, at this site, in this run of its body.
    New { site: TextRange, act: ActivationId },
}

/// A deferred operation of one walk: a recursive call, or a use of its value.
#[newtype_index]
pub struct DeferId;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum AbstractValue<'db> {
    Function(Definition<'db>), // a def: something you can walk
    Closure {
        func: Definition<'db>,
        activation: ActivationId, // the run whose `def` statement made it
    },
    Class(Definition<'db>),
    Instance {
        class: Definition<'db>,
        origin: OriginId,
    },
    BoundMethod {
        func: Definition<'db>,
        receiver: ValueId, // our own object (or class), not ty's
    },
    Ty(Type<'db>), // ty's answer, for everything you don't model
    Module(Type<'db>),
    Unknown,          // nothing known
    Pending(DeferId), // the value of an operation waiting on a recursive call
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
            Shape::BoundMethod(_) => smallvec![AbstractValue::Ty(ty)], // step 04 gives it a receiver
            Shape::Class(d) => smallvec![AbstractValue::Class(d)],
            Shape::Union(members) => members
                .into_iter()
                .flat_map(|t| self.lift(sol, t))
                .collect(),
            Shape::Unknown => smallvec![AbstractValue::Unknown],
            Shape::Other => smallvec![AbstractValue::Ty(ty)],
        }
    }
}
