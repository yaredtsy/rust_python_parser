// src/interp/env.rs
use crate::interp::value::Values;
use crate::sem::Name;

/// What each name holds on the path being walked. Cloning is a pointer copy.
#[derive(Clone, Default)]
pub struct Env<'db> {
    pub locals: rpds::HashTrieMap<Name, Values<'db>>,
}

impl<'db> Env<'db> {
    pub fn get(&self, name: &Name) -> Option<&Values<'db>> {
        self.locals.get(name)
    }
    pub fn bind(&mut self, name: &Name, v: Values<'db>) {
        self.locals.insert_mut(name.clone(), v);
    }
}
