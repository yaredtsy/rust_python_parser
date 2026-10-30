// src/interp/bind.rs
use ruff_python_ast::{
    ArgOrKeyword, Arguments, Expr, ExprRef, ParameterWithDefault, StmtFunctionDef,
};
use smallvec::{SmallVec, smallvec};

use crate::interp::env::Env;
use crate::interp::value::{AbstractValue, ActivationId, Values};
use crate::interp::walk::Walk;
use crate::sem::Name;

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
        def: &StmtFunctionDef,
        args: &EvaluatedArgs<'db>,
        enclosing: Option<ActivationId>,
    ) -> Env<'db> {
        let mut env = Env::default();
        let args = &args.with_unknown_for_empty();
        let mut positional = args.positional.iter();
        let params = &def.parameters;
        let unknown = || smallvec![AbstractValue::Unknown];

        if args.has_splat {
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
                .or_else(|| self.default_value(p, enclosing))
                .unwrap_or_else(|| smallvec![AbstractValue::Unknown]);
            env.bind(name, v);
        }
        for p in &params.kwonlyargs {
            let name = &p.parameter.name.id;
            let v = args
                .keyword(name)
                .cloned()
                .or_else(|| self.default_value(p, enclosing))
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

    fn default_value(
        &mut self,
        p: &ParameterWithDefault,
        enclosing: Option<ActivationId>,
    ) -> Option<Values<'db>> {
        let expr = p.default.as_deref()?;
        // `def inner(h=h)`: a plain name is read from the closure's run.
        if let (Expr::Name(n), Some(act)) = (expr, enclosing) {
            if let Some(v) = self.activations[act].get(&n.id) {
                return Some(v.clone());
            }
        }
        let ty = self.sem.type_of(&self.frame.model, ExprRef::from(expr));
        Some(self.lift(ty))
    }
}
