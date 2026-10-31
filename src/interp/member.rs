use ruff_python_ast::{ExprAttribute, ExprRef};
use smallvec::{SmallVec, smallvec};

use crate::interp::{
    Walk,
    env::Env,
    solver::Solver,
    value::{AbstractValue, OriginId, Values, dedup},
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
                    out.extend(self.member(sol, recv, &attr.attr.id, env));
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
        env: &mut Env<'db>,
    ) -> Values<'db> {
        let file = self.frame.file;
        match recv {
            AbstractValue::Instance { class, origin } => {
                if let Some(v) = env.attrs.get(&(origin, Name::from(name))) {
                    return v.clone(); // 1. this object, on this path
                }
                let t = sol.sem.member(sol.sem.instance_type(class), name, file);
                match t.map(|t| (t, sol.sem.shape(t, file))) {
                    Some((t, Shape::BoundMethod(func))) => {
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
        let targets: SmallVec<[OriginId; 2]> = recvs
            .iter()
            .filter_map(|r| match r {
                AbstractValue::Instance { origin, .. } => Some(*origin),
                _ => None,
            })
            .collect();
        let strong = targets.len() == 1; // exactly one object: replace
        for origin in targets {
            env.bind_attr(origin, &attr.attr.id, v.clone(), strong);
        }
    }
}
