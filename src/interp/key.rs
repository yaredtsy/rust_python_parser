use ruff_index::{IndexVec, newtype_index};
use ruff_python_ast::StmtFunctionDef;
use rustc_hash::FxHashMap;
use smallvec::SmallVec;
use std::hash::Hash;

use smallvec::smallvec;

use crate::interp::bind::{Bound, param_names};
use crate::interp::env::Env;
use crate::interp::relevant::relevant_params;
use crate::interp::solver::Solver;
use crate::interp::value::{AbstractValue, Values};
use crate::interp::walk::Walk;
use crate::sem::{Definition, Type};
use ty_python_core::ProgramFile;

#[newtype_index]
#[derive(Ord, PartialOrd)] // `fingerprint` sorts FpIds
pub struct FpId;

#[newtype_index]
pub struct ContextId;

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum Fp<'db> {
    Unknown,
    Data(Type<'db>),
    Function(Definition<'db>),
    Module(Type<'db>),
    Union(SmallVec<[FpId; 4]>),
}

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct Context<'db> {
    pub func: Definition<'db>,
    pub args: SmallVec<[FpId; 4]>,
}

pub struct Interner<K, I: ruff_index::Idx> {
    items: IndexVec<I, K>,
    ids: FxHashMap<K, I>,
}

impl<K, I: ruff_index::Idx> Default for Interner<K, I> {
    fn default() -> Self {
        Interner {
            items: IndexVec::new(),
            ids: FxHashMap::default(),
        }
    }
}

impl<K: Clone + Eq + Hash, I: ruff_index::Idx> Interner<K, I> {
    pub fn intern(&mut self, k: K) -> I {
        if let Some(&id) = self.ids.get(&k) {
            return id;
        }
        let id = self.items.push(k.clone());
        self.ids.insert(k, id);
        id
    }
    pub fn len(&self) -> usize {
        self.items.len()
    }
}

impl<K, I: ruff_index::Idx> std::ops::Index<I> for Interner<K, I> {
    type Output = K;
    fn index(&self, i: I) -> &K {
        &self.items[i]
    }
}

impl<'db> Solver<'db> {
    pub fn fingerprint(&mut self, file: ProgramFile<'db>, vals: &Values<'db>) -> FpId {
        let mut ids: SmallVec<[FpId; 4]> = vals.iter().map(|v| self.fp_one(file, *v)).collect();
        ids.sort();
        ids.dedup();
        match ids.len() {
            0 => self.fps.intern(Fp::Unknown), // no value reaches: keyed as Unknown
            1 => ids[0],
            _ => self.fps.intern(Fp::Union(ids)),
        }
    }

    fn fp_one(&mut self, file: ProgramFile<'db>, v: AbstractValue<'db>) -> FpId {
        let fp = match v {
            AbstractValue::Function(d) => Fp::Function(d),
            AbstractValue::Module(t) => Fp::Module(t),
            AbstractValue::Ty(t) => Fp::Data(self.sem.widen(t, file)),
            AbstractValue::Unknown => Fp::Unknown,
            AbstractValue::Closure { .. } => Fp::Unknown, // placeholder until step 07
        };
        self.fps.intern(fp)
    }

    pub fn key(&mut self, func: Definition<'db>, bound: &Bound<'db>) -> ContextId {
        let file = func.program_file(self.sem.db);
        // a parameter the walk can't be affected by is Unknown: callers share one summary
        let relevant = relevant_params(self.sem.db, func);
        let args = bound
            .slots
            .iter()
            .enumerate()
            .map(|(i, v)| {
                if relevant.get(i).copied().unwrap_or(true) {
                    self.fingerprint(file, v)
                } else {
                    self.fps.intern(Fp::Unknown)
                }
            })
            .collect();
        self.contexts.intern(Context { func, args })
    }

    fn from_fp(&self, fp: FpId) -> Values<'db> {
        match &self.fps[fp] {
            Fp::Function(d) => smallvec![AbstractValue::Function(*d)],
            Fp::Module(t) => smallvec![AbstractValue::Module(*t)],
            Fp::Data(t) => smallvec![AbstractValue::Ty(*t)],
            Fp::Unknown => smallvec![AbstractValue::Unknown],
            Fp::Union(ids) => ids.iter().flat_map(|&i| self.from_fp(i)).collect(),
        }
    }
}

impl<'db> Walk<'db> {
    /// The starting env of a context's walk. Reads the key and the code. Nothing else.
    pub fn materialize(
        &mut self,
        sol: &Solver<'db>,
        ctx: ContextId,
        node: &StmtFunctionDef,
    ) -> Env<'db> {
        let key = &sol.contexts[ctx];
        let mut env = Env::default();
        env.scope = Some(sol.sem.body_scope(key.func, node));
        for (param, &fp) in param_names(node).zip(&key.args) {
            env.bind(param, sol.from_fp(fp));
        }
        env
    }
}
