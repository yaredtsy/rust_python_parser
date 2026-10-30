// src/interp/env.rs
use crate::interp::value::{Values, dedup};
use crate::interp::value::ActivationId;
use crate::sem::{BodyScope, Name};

/// What each name holds on the path being walked. Cloning is a pointer copy.
#[derive(Clone, Default)]
pub struct Env<'db> {
    pub locals: rpds::HashTrieMap<Name, Values<'db>>,
    /// Which function body these locals belong to. Fixed for a run.
    pub scope: Option<BodyScope<'db>>,
    /// The run where this function's `def` ran. NEVER the caller.
    pub enclosing: Option<ActivationId>,
}

impl<'db> Env<'db> {
    pub fn get(&self, name: &Name) -> Option<&Values<'db>> {
        self.locals.get(name)
    }
    pub fn bind(&mut self, name: &Name, v: Values<'db>) {
        self.locals.insert_mut(name.clone(), v);
    }
    /// Union per name. A name missing from an arm was not assigned on that path.
    pub fn join(arms: Vec<Option<Env<'db>>>) -> Option<Env<'db>> {
        let mut arms = arms.into_iter().flatten();
        let mut out = arms.next()?;
        for arm in arms {
            for (name, vals) in arm.locals.iter() {
                let merged = match out.locals.get(name) {
                    Some(old) if old == vals => continue,
                    Some(old) => {
                        let mut m = old.clone();
                        m.extend(vals.iter().copied());
                        dedup(&mut m);
                        m
                    }
                    None => vals.clone(),
                };
                out.locals.insert_mut(name.clone(), merged);
            }
        }
        Some(out)
    }
}
