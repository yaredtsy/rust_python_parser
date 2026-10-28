// src/interp/call.rs
use ruff_python_ast::{ExprCall, ExprRef};
use ruff_text_size::{Ranged, TextRange};

use crate::interp::Mark;
use crate::interp::bind::EvaluatedArgs;
use crate::interp::env::Env;
use crate::interp::tree::Node;
use crate::interp::value::{AbstractValue, Values};
use crate::interp::walk::{Frame, Walk};
use crate::sem::Definition;

impl<'a, 'db> Walk<'a, 'db> {
    pub fn eval_call(&mut self, call: &ExprCall, env: &mut Env<'db>) -> Values<'db> {
        let callees = self.eval(&call.func, env);
        let args = self.eval_arguments(&call.arguments, env);

        // One node per function, in definition order: the join order is an accident.
        let mut defs: Vec<_> = callees
            .iter()
            .filter_map(|c| match *c {
                AbstractValue::Function(def) if self.sem.is_project(def) => Some(def),
                _ => None,
            })
            .collect();
        defs.sort_by_key(|d| self.sem.def_position(*d));
        for def in defs {
            self.call_function(def, call.range(), &args);
        }
        let ty = self.sem.type_of(&self.frame.model, ExprRef::from(call));
        self.lift(ty)
    }
    /// Inline mode: walk the callee's body now, as a child of this call.
    fn call_function(&mut self, def: Definition<'db>, site: TextRange, args: &EvaluatedArgs<'db>) {
        let qname = self.sem.qname(def);
        if self.path.contains(&qname) {
            let mut node = Node::call(qname, site);
            node.marks.push(Mark::Recursive);
            self.frame.children.push(node);
            return;
        }

        let parsed = self.sem.parsed(def);
        let Some(node) = self.sem.function_node(def, &parsed) else {
            return;
        };

        self.path.push(qname.clone());
        let caller = std::mem::replace(&mut self.frame, Frame::new(self.sem, def));
        let env = self.bind_parameters(node, args);
        self.walk_stmts(&node.body, env);
        let callee = std::mem::replace(&mut self.frame, caller);
        self.path.pop();

        self.frame.children.push(Node {
            qname,
            site: Some(site),
            marks: Vec::new(),
            children: callee.children,
        });
    }
}
