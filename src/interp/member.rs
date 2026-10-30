use ruff_python_ast::{ExprAttribute, ExprRef};

use crate::interp::{
    Walk,
    solver::Solver,
    env::Env,
    value::{AbstractValue, Values, dedup},
};

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
                AbstractValue::Module(m) => {
                    match sol.sem.member(m, &attr.attr.id, self.frame.file) {
                        Some(t) => out.extend(self.lift(sol, t)),
                        None => out.push(AbstractValue::Unknown),
                    }
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
}
