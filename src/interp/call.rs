// src/interp/call.rs
use ruff_python_ast::{ExprCall, ExprRef};
use ruff_text_size::{Ranged, TextRange};

use crate::interp::env::Env;
use crate::interp::tree::Node;
use crate::interp::value::{AbstractValue, Values};
use crate::interp::walk::{Frame, Walk};
use crate::sem::Definition;

impl<'a, 'db> Walk<'a, 'db> {
    pub fn eval_call(&mut self, call: &ExprCall, env: &mut Env<'db>) -> Values<'db> {
        let callees = self.eval(&call.func, env);
        for arg in call.arguments.iter_source_order() {
            self.eval(arg.value(), env);
        }
        for callee in &callees {
            if let AbstractValue::Function(def) = *callee
                && self.sem.is_project(def)
            {
                self.call_function(def, call.range());
            }
        }
        let ty = self.sem.type_of(&self.frame.model, ExprRef::from(call));
        self.lift(ty)
    }
    /// Inline mode: walk the callee's body now, as a child of this call.
    fn call_function(&mut self, def: Definition<'db>, site: TextRange) {
        let parsed = self.sem.parsed(def);
        let Some(node) = self.sem.function_node(def, &parsed) else {
            return;
        };

        let caller = std::mem::replace(&mut self.frame, Frame::new(self.sem, def));
        self.walk_stmts(&node.body, Env::default());
        let callee = std::mem::replace(&mut self.frame, caller);

        self.frame.children.push(Node {
            qname: self.sem.qname(def),
            site: Some(site),
            marks: Vec::new(),
            children: callee.children,
        });
    }
}
