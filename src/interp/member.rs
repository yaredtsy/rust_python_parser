use ruff_python_ast::{ExprAttribute, ExprRef};
use ruff_text_size::{Ranged, TextRange};
use smallvec::smallvec;

use crate::interp::{
    Mode, Walk,
    bind::EvaluatedArgs,
    env::Env,
    solver::Solver,
    value::{AbstractValue, Values, dedup},
};
use crate::sem::{BoundTo, Name, Shape};

// src/interp/member.rs
impl<'db> Walk<'db> {
    pub fn eval_attribute(
        &mut self,
        sol: &mut Solver<'db>,
        attr: &ExprAttribute,
        env: &mut Env<'db>,
    ) -> Values<'db> {
        let recvs = self.eval(sol, &attr.value, env);
        let mut out = Values::new();
        let mut asked_ty = false;
        for recv in recvs {
            match recv {
                AbstractValue::Instance { .. } | AbstractValue::Class(_) | AbstractValue::Module(_) => {
                    out.extend(self.member(sol, recv, &attr.attr.id, attr.range(), env));
                }
                _ if !asked_ty => {
                    asked_ty = true;
                    let ty = sol.sem.type_of(&self.frame.model, ExprRef::from(attr));
                    out.extend(self.lift(sol, ty));
                }
                _ => {}
            }
        }
        dedup(&mut out);
        out
    }

    /// What `.name` is on one receiver. ty finds it; the receiver bound is ours.
    /// Its own state comes first, then the class (through ty).
    fn member(
        &mut self,
        sol: &mut Solver<'db>,
        recv: AbstractValue<'db>,
        name: &str,
        site: TextRange,
        env: &mut Env<'db>,
    ) -> Values<'db> {
        let file = self.frame.file;
        match recv {
            AbstractValue::Instance { class, origin } => {
                if origin.holds_state() {
                    if let Some(v) = env.attrs.get(&(origin, Name::from(name))) {
                        return v.clone(); // 1. this object, on this path
                    }
                }
                // 2. a property: its getter runs here (inline mode only)
                if sol.opts.mode == Mode::Inline {
                    if let Some(getter) = sol.sem.property_parts(class, name).and_then(|p| p.getter) {
                        let args = EvaluatedArgs::default();
                        return self.call_function(sol, getter, None, Some(recv), site, &args, env);
                    }
                }
                // 3. the class side, through ty
                let t = sol.sem.member(sol.sem.instance_type(class), name, file);
                match t.map(|t| (t, sol.sem.shape(t, file))) {
                    Some((t, Shape::BoundMethod { func, .. })) => {
                        let receiver = match sol.sem.bound_to(t) {
                            BoundTo::Object => recv, // a plain method: OUR object
                            BoundTo::Class => AbstractValue::Class(class), // a classmethod: the object's class
                            BoundTo::Other => return self.lift(sol, t),
                        };
                        let receiver = self.values.intern(receiver);
                        smallvec![AbstractValue::BoundMethod { func, receiver }]
                    }
                    Some((t, _)) => self.lift(sol, t), // a class variable, a declared attribute
                    None => smallvec![AbstractValue::Unknown],
                }
            }
            AbstractValue::Class(c) => match sol.sem.member(sol.sem.class_type(c), name, file) {
                Some(t) => self.lift(sol, t), // a method stays an unbound Function
                None => smallvec![AbstractValue::Unknown],
            },
            AbstractValue::Module(m) => match sol.sem.member(m, name, file) {
                Some(t) => self.lift(sol, t),
                None => smallvec![AbstractValue::Unknown],
            },
            _ => unreachable!("member is only called for Instance, Class and Module"),
        }
    }

    /// `a.b = v`: the value goes to every object `a` may be.
    pub fn bind_attribute(
        &mut self,
        sol: &mut Solver<'db>,
        attr: &ExprAttribute,
        v: Values<'db>,
        env: &mut Env<'db>,
    ) {
        let recvs = self.eval(sol, &attr.value, env);
        // Counted before the filter: `{a, OPAQUE}` or `{a, Unknown}` may be the
        // other one, so `a` only may have changed.
        let strong = recvs.len() == 1; // exactly one receiver: replace
        for r in recvs.iter().copied() {
            let AbstractValue::Instance { class, origin } = r else { continue };
            match sol.sem.property_parts(class, &attr.attr.id) {
                // a property: its setter runs, and nothing is stored under its name
                Some(parts) => {
                    if let (Some(setter), Mode::Inline) = (parts.setter, sol.opts.mode) {
                        let args = EvaluatedArgs { positional: smallvec![v.clone()], ..Default::default() };
                        self.call_function(sol, setter, None, Some(r), attr.range(), &args, env);
                    }
                }
                None if origin.holds_state() => env.bind_attr(origin, &attr.attr.id, v.clone(), strong),
                None => {}
            }
        }
    }
}
