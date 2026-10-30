// src/interp/bind.rs
use ruff_python_ast::{
    ArgOrKeyword, Arguments, Expr, ExprRef, ParameterWithDefault, StmtFunctionDef,
};
use smallvec::{SmallVec, smallvec};

use crate::interp::env::Env;
use crate::interp::value::{AbstractValue, ActivationId, Values};
use crate::interp::walk::Walk;
use crate::sem::{Definition, Name, SIGNATURE_KEEPING, Shape};

#[derive(Clone, Default)]
pub struct EvaluatedArgs<'db> {
    pub positional: SmallVec<[Values<'db>; 4]>,
    pub keywords: SmallVec<[(Name, Values<'db>); 2]>,
    pub has_splat: bool,
}

impl<'db> EvaluatedArgs<'db> {
    fn keyword(&self, name: &str) -> Option<&Values<'db>> {
        self.keywords
            .iter()
            .find(|(n, _)| n.as_str() == name)
            .map(|(_, v)| v)
    }
    /// Python never passes "no value"; an empty slot binds as `Unknown`.
    fn with_unknown_for_empty(&self) -> Self {
        let fix = |v: &Values<'db>| if v.is_empty() { smallvec![AbstractValue::Unknown] } else { v.clone() };
        EvaluatedArgs {
            positional: self.positional.iter().map(fix).collect(),
            keywords: self.keywords.iter().map(|(n, v)| (n.clone(), fix(v))).collect(),
            has_splat: self.has_splat,
        }
    }
    pub fn has_empty(&self) -> bool {
        self.positional.iter().any(|v| v.is_empty())
            || self.keywords.iter().any(|(_, v)| v.is_empty())
    }
}

impl<'a, 'db> Walk<'a, 'db> {
    /// In the caller's frame and env, left to right, each argument once.
    pub fn eval_arguments(&mut self, args: &Arguments, env: &mut Env<'db>) -> EvaluatedArgs<'db> {
        let mut out = EvaluatedArgs::default();
        for arg in args.iter_source_order() {
            match arg {
                ArgOrKeyword::Arg(Expr::Starred(st)) => {
                    self.eval(&st.value, env);
                    out.has_splat = true;
                }
                ArgOrKeyword::Arg(e) => {
                    let v = self.eval(e, env);
                    out.positional.push(v);
                }
                ArgOrKeyword::Keyword(kw) => {
                    let v = self.eval(&kw.value, env);
                    if let Some(name) = &kw.arg {
                        out.keywords.push((name.id.clone(), v));
                    }
                }
            }
        }
        out
    }
    /// The callee's starting env. Must run with the callee's frame active.
    pub fn bind_parameters(
        &mut self,
        func: Definition<'db>,
        def: &StmtFunctionDef,
        args: &EvaluatedArgs<'db>,
        enclosing: Option<ActivationId>,
    ) -> Env<'db> {
        let mut env = Env::default();
        let args = &args.with_unknown_for_empty();
        let mut positional = args.positional.iter();
        let params = &def.parameters;
        let unknown = || smallvec![AbstractValue::Unknown];

        if args.has_splat || self.decorator_changes_signature(def) {
            for p in params.iter() {
                // [check] ruff's iterator over every parameter
                env.bind(&p.name().id, unknown());
            }
            return env;
        }

        for p in params.posonlyargs.iter().chain(&params.args) {
            let name = &p.parameter.name.id;
            let v = args
                .keyword(name)
                .cloned()
                .or_else(|| positional.next().cloned())
                .or_else(|| self.default_value(func, enclosing, p))
                .unwrap_or_else(|| smallvec![AbstractValue::Unknown]);
            env.bind(name, v);
        }
        for p in &params.kwonlyargs {
            let name = &p.parameter.name.id;
            let v = args
                .keyword(name)
                .cloned()
                .or_else(|| self.default_value(func, enclosing, p))
                .unwrap_or_else(|| smallvec![AbstractValue::Unknown]);
            env.bind(name, v);
        }
        if let Some(p) = &params.vararg {
            env.bind(&p.name.id, unknown());
        }
        if let Some(p) = &params.kwarg {
            env.bind(&p.name.id, unknown());
        }
        env
    }

    /// Binding is only trusted for project decorators and the signature-keeping allow-list.
    fn decorator_changes_signature(&mut self, def: &StmtFunctionDef) -> bool {
        for dec in &def.decorator_list {
            // `@retry` and `@functools.lru_cache(maxsize=1)` alike: look at the callee.
            let target = match &dec.expression {
                Expr::Call(c) => &*c.func,
                e => e,
            };
            let ty = self.sem.type_of(&self.frame.model, ExprRef::from(target));
            let trusted = match self.sem.shape(ty, self.frame.file) {
                Shape::Function(d) => {
                    self.sem.is_project(d) || SIGNATURE_KEEPING.contains(&self.sem.qname(d).as_str())
                }
                _ => false,
            };
            if !trusted {
                return true;
            }
        }
        false
    }

    fn default_value(
        &mut self,
        func: Definition<'db>,
        enclosing: Option<ActivationId>,
        p: &ParameterWithDefault,
    ) -> Option<Values<'db>> {
        let expr = p.default.as_deref()?;
        if let Some(act) = enclosing {
            // a nested def: the value computed when its `def` ran
            return self
                .defaults
                .get(&(func, act))?
                .iter()
                .find(|(n, _)| *n == p.parameter.name.id)
                .map(|(_, v)| v.clone());
        }
        let ty = self.sem.type_of(&self.frame.model, ExprRef::from(expr));
        Some(self.lift(ty))
    }
}
