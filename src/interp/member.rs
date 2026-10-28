use ruff_python_ast::{ExprAttribute, ExprRef};

use crate::interp::{
    Walk,
    env::Env,
    value::{AbstractValue, Values, dedup},
};

// src/interp/member.rs
impl<'a, 'db> Walk<'a, 'db> {
    pub fn eval_attribute(&mut self, attr: &ExprAttribute, env: &mut Env<'db>) -> Values<'db> {
        let recvs = self.eval(&attr.value, env);
        let mut out = Values::new();
        let mut asked_ty = false;
        for recv in recvs {
            match recv {
                AbstractValue::Module(m) => {
                    match self.sem.member(m, &attr.attr.id, self.frame.file) {
                        Some(t) => out.extend(self.lift(t)),
                        None => out.push(AbstractValue::Unknown),
                    }
                }
                _ if !asked_ty => {
                    asked_ty = true;
                    let ty = self.sem.type_of(&self.frame.model, ExprRef::from(attr));
                    out.extend(self.lift(ty));
                }
                _ => {}
            }
        }
        dedup(&mut out);
        out
    }
}
