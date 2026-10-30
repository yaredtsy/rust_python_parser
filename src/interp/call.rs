// src/interp/call.rs
use crate::interp::Mark;
use crate::interp::bind::EvaluatedArgs;
use crate::interp::env::Env;
use crate::interp::tree::Node;
use crate::interp::value::{AbstractValue, Values};
use crate::interp::value::dedup;
use crate::interp::value::ActivationId;
use crate::interp::walk::{Walk, contains_yield};
use crate::sem::Definition;
use ruff_python_ast::{ExprCall, ExprRef};
use ruff_text_size::{Ranged, TextRange};
use smallvec::smallvec;

impl<'a, 'db> Walk<'a, 'db> {
    pub fn eval_call(&mut self, call: &ExprCall, env: &mut Env<'db>) -> Values<'db> {
        let callees = self.eval(&call.func, env);
        let args = self.eval_arguments(&call.arguments, env);

        // One node per function, in definition order: the join order is an accident.
        let mut targets: Vec<(Definition<'db>, Option<ActivationId>)> = Vec::new();
        let mut ask_ty = false;
        for c in &callees {
            match *c {
                AbstractValue::Function(def) if self.sem.is_project(def) => {
                    targets.push((def, None))
                }
                AbstractValue::Closure { func, activation } if self.sem.is_project(func) => {
                    targets.push((func, Some(activation)))
                }
                _ => ask_ty = true,
            }
        }
        targets.sort_by_key(|(d, _)| self.sem.def_position(*d));
        let mut out = Values::new();
        for (def, enclosing) in targets {
            out.extend(self.call_function(def, enclosing, call.range(), &args));
        }
        if ask_ty {
            let ty = self.sem.type_of(&self.frame.model, ExprRef::from(call));
            out.extend(self.lift(ty));
        }
        dedup(&mut out);
        out
    }
    /// Inline mode: walk the callee's body now, as a child of this call.
    fn call_function(
        &mut self,
        def: Definition<'db>,
        enclosing: Option<ActivationId>,
        site: TextRange,
        args: &EvaluatedArgs<'db>,
    ) -> Values<'db> {
        let qname = self.sem.qname(def);
        if self.path.contains(&qname) {
            let mut node = Node::call(qname, site);
            node.marks.push(Mark::Recursive);
            self.frame.children.push(node);
            return smallvec![AbstractValue::Unknown];
        }

        let parsed = self.sem.parsed(def);
        let Some(node) = self.sem.function_node(def, &parsed) else {
            return smallvec![AbstractValue::Unknown];
        };
        let runs = !args.has_empty();
        self.path.push(qname.clone());

        let new = self.new_frame(def);
        let caller = std::mem::replace(&mut self.frame, new);
        let mut env = self.bind_parameters(def, node, args, enclosing);
        env.scope = Some(self.sem.body_scope(def, node));
        env.enclosing = enclosing;
        if self.walk_stmts(&node.body, env).is_some() {
            let none = self.none_value();
            self.frame.returns.extend(none); // fell off the end
        }
        let callee = std::mem::replace(&mut self.frame, caller);
        self.path.pop();

        self.frame.children.push(Node {
            qname,
            site: Some(site),
            marks: Vec::new(),
            children: callee.children,
        });
        if !runs {
            Values::new() // Python never makes this call
        } else if contains_yield(&node.body) {
            smallvec![AbstractValue::Unknown] // a generator object
        } else {
            callee.returns
        }
    }
}
