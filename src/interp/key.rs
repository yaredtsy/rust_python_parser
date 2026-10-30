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
use crate::interp::value::{AbstractValue, ActivationId, Values};
use crate::interp::walk::Walk;
use crate::sem::{Definition, Name, Type};
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
    Closure {
        func: Definition<'db>,
        free: SmallVec<[(Name, FpId); 2]>,     // sorted by name
        defaults: SmallVec<[(Name, FpId); 2]>, // the defaults it was made with
    },
    Back(u8), // the closure k levels up
}

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct Context<'db> {
    pub func: Definition<'db>,
    pub args: SmallVec<[FpId; 4]>,
    /// The called closure's own free values. Empty for a plain function.
    pub free: SmallVec<[(Name, FpId); 2]>,
    /// ... and its own defaults.
    pub defaults: SmallVec<[(Name, FpId); 2]>,
}

/// The closures being fingerprinted, innermost last, compared by `(func, activation)`.
#[derive(Default)]
pub struct ClosureStack<'db>(SmallVec<[(Definition<'db>, ActivationId); 2]>);

impl<'db> ClosureStack<'db> {
    pub fn position(&self, f: Definition<'db>, a: ActivationId) -> Option<u8> {
        self.0.iter().rev().position(|&(g, b)| g == f && b == a).map(|k| k as u8)
    }
    pub fn with<R>(&mut self, f: Definition<'db>, a: ActivationId, body: impl FnOnce(&mut Self) -> R) -> R {
        self.0.push((f, a));
        let r = body(self);
        self.0.pop();
        r
    }
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
    pub fn iter(&self) -> impl Iterator<Item = (I, &K)> {
        self.items.iter_enumerated()
    }
}

impl<K, I: ruff_index::Idx> std::ops::Index<I> for Interner<K, I> {
    type Output = K;
    fn index(&self, i: I) -> &K {
        &self.items[i]
    }
}

impl<'db> Solver<'db> {
    /// `depth`: how many closures these values sit inside. 0 for a parameter slot.
    pub fn fingerprint(
        &mut self,
        w: &Walk<'db>,
        file: ProgramFile<'db>,
        vals: &Values<'db>,
        depth: u8,
        cl: &mut ClosureStack<'db>,
    ) -> FpId {
        let mut ids: SmallVec<[FpId; 4]> =
            vals.iter().map(|v| self.fp_one(w, file, *v, depth, cl)).collect();
        ids.sort();
        ids.dedup();
        match ids.len() {
            0 => self.fps.intern(Fp::Unknown), // no value reaches: keyed as Unknown
            1 => ids[0],
            _ => self.fps.intern(Fp::Union(ids)),
        }
    }

    fn fp_one(
        &mut self,
        w: &Walk<'db>,
        file: ProgramFile<'db>,
        v: AbstractValue<'db>,
        depth: u8,
        cl: &mut ClosureStack<'db>,
    ) -> FpId {
        let fp = match v {
            AbstractValue::Function(d) => Fp::Function(d),
            AbstractValue::Module(t) => Fp::Module(t),
            AbstractValue::Ty(t) => Fp::Data(self.sem.widen(t, file)),
            AbstractValue::Unknown => Fp::Unknown,
            AbstractValue::Closure { func, activation } => match cl.position(func, activation) {
                Some(k) => Fp::Back(k),
                None if depth == self.opts.limits.fp_depth => self.fp_closure_at_limit(w, func, activation),
                None => cl.with(func, activation, |cl| Fp::Closure {
                    func,
                    free: self.fp_free(w, func, activation, cl, depth + 1),
                    defaults: self.fp_defaults(w, func, activation, cl, depth + 1),
                }),
            },
            AbstractValue::Pending(_) => Fp::Unknown, // a call with a pending argument is deferred, never keyed
        };
        self.fps.intern(fp)
    }

    /// At the depth limit a closure keeps its function; every captured value
    /// and default is `Unknown`.
    fn fp_closure_at_limit(&mut self, w: &Walk<'db>, func: Definition<'db>, act: ActivationId) -> Fp<'db> {
        let unknown = self.fps.intern(Fp::Unknown);
        Fp::Closure {
            func,
            free: self.sem.free_variables(func).into_iter().map(|n| (n, unknown)).collect(),
            defaults: w
                .defaults
                .get(&(func, act))
                .into_iter()
                .flatten()
                .map(|(n, _)| (n.clone(), unknown))
                .collect(),
        }
    }

    fn fp_free(
        &mut self,
        w: &Walk<'db>,
        func: Definition<'db>,
        activation: ActivationId,
        cl: &mut ClosureStack<'db>,
        depth: u8,
    ) -> SmallVec<[(Name, FpId); 2]> {
        let file = func.program_file(self.sem.db);
        let mut out = SmallVec::new();
        for n in self.sem.free_variables(func) {
            let vals = w.lookup_in(self, func, activation, &n);
            out.push((n, self.fingerprint(w, file, &vals, depth, cl)));
        }
        out
    }

    fn fp_defaults(
        &mut self,
        w: &Walk<'db>,
        func: Definition<'db>,
        activation: ActivationId,
        cl: &mut ClosureStack<'db>,
        depth: u8,
    ) -> SmallVec<[(Name, FpId); 2]> {
        let file = func.program_file(self.sem.db);
        let mut out = SmallVec::new();
        for (n, vals) in w.defaults.get(&(func, activation)).into_iter().flatten() {
            out.push((n.clone(), self.fingerprint(w, file, vals, depth, cl)));
        }
        out
    }

    /// `closure`: the activation of the closure being called, when the callee is one.
    pub fn key(
        &mut self,
        w: &Walk<'db>,
        func: Definition<'db>,
        bound: &Bound<'db>,
        closure: Option<ActivationId>,
    ) -> ContextId {
        let file = func.program_file(self.sem.db);
        // a parameter the walk can't be affected by is Unknown: callers share one summary
        let relevant = relevant_params(self.sem.db, func);
        debug_assert_eq!(
            relevant.len(),
            bound.slots.len(),
            "relevance out of sync with binding"
        );
        let args = bound
            .slots
            .iter()
            .enumerate()
            .map(|(i, v)| {
                if relevant.get(i).copied().unwrap_or(true) {
                    self.fingerprint(w, file, v, 0, &mut ClosureStack::default())
                } else {
                    self.fps.intern(Fp::Unknown)
                }
            })
            .collect();
        // The called closure counts as a closure: its free values sit at depth 1.
        let (free, defaults) = match closure {
            Some(act) => ClosureStack::default().with(func, act, |cl| {
                (
                    self.fp_free(w, func, act, cl, 1),
                    self.fp_defaults(w, func, act, cl, 1),
                )
            }),
            None => (SmallVec::new(), SmallVec::new()),
        };
        self.contexts.intern(Context { func, args, free, defaults })
    }
}

impl<'db> Walk<'db> {
    /// A value from a fingerprint. `cl`: the closures being rebuilt, innermost last.
    fn from_fp(
        &mut self,
        sol: &Solver<'db>,
        fp: FpId,
        cl: &mut Vec<(Definition<'db>, ActivationId)>,
    ) -> Values<'db> {
        match &sol.fps[fp] {
            Fp::Function(d) => smallvec![AbstractValue::Function(*d)],
            Fp::Module(t) => smallvec![AbstractValue::Module(*t)],
            Fp::Data(t) => smallvec![AbstractValue::Ty(*t)],
            Fp::Unknown => smallvec![AbstractValue::Unknown],
            Fp::Union(ids) => ids.iter().flat_map(|&i| self.from_fp(sol, i, cl)).collect(),
            Fp::Back(k) => {
                let (func, activation) = cl[cl.len() - 1 - *k as usize];
                smallvec![AbstractValue::Closure { func, activation }]
            }
            Fp::Closure { func, free, defaults } => {
                let activation = self.activations.push(Env::flat());
                cl.push((*func, activation));
                for (name, fp) in free {
                    let v = self.from_fp(sol, *fp, cl);
                    self.activations[activation].bind(name, v);
                }
                let d = defaults
                    .iter()
                    .map(|(n, fp)| (n.clone(), self.from_fp(sol, *fp, cl)))
                    .collect();
                self.defaults.insert((*func, activation), d);
                cl.pop();
                smallvec![AbstractValue::Closure { func: *func, activation }]
            }
        }
    }

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
        if !key.free.is_empty() || !key.defaults.is_empty() {
            // the called closure's own activation, so a Back(0) inside reaches it
            let activation = self.activations.push(Env::flat());
            let mut cl = vec![(key.func, activation)];
            for (name, fp) in &key.free {
                let v = self.from_fp(sol, *fp, &mut cl);
                self.activations[activation].bind(name, v);
            }
            let d = key
                .defaults
                .iter()
                .map(|(n, fp)| (n.clone(), self.from_fp(sol, *fp, &mut cl)))
                .collect();
            self.defaults.insert((key.func, activation), d);
            env.enclosing = Some(activation);
        }
        for (param, &fp) in param_names(node).zip(&key.args) {
            env.bind(param, self.from_fp(sol, fp, &mut Vec::new()));
        }
        env
    }
}
