use crate::{interp::walk::Walk, sem::Shape};
use smallvec::SmallVec;
use smallvec::smallvec;
use ty_python_core::definition::Definition;
use ty_python_semantic::types::Type;
// src/interp/value.rs
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum AbstractValue<'db> {
    Function(Definition<'db>), // a def: something you can walk
    Ty(Type<'db>),             // ty's answer, for everything you don't model
    Unknown,                   // nothing known
}

/// Several possibilities, kept apart. Never collapsed into one ty union.
pub type Values<'db> = SmallVec<[AbstractValue<'db>; 2]>;

impl<'a, 'db> Walk<'a, 'db> {
    pub fn lift(&self, ty: Type<'db>) -> Values<'db> {
        match self.sem.shape(ty, self.frame.file) {
            Shape::Function(d) => smallvec![AbstractValue::Function(d)],
            Shape::Union(members) => members.into_iter().flat_map(|t| self.lift(t)).collect(),
            Shape::Unknown => smallvec![AbstractValue::Unknown],
            Shape::Other => smallvec![AbstractValue::Ty(ty)],
        }
    }
}
