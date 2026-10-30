// src/interp/call.rs
use crate::interp::Mark;
use crate::interp::Mode;
use crate::interp::bind::{Bound, EvaluatedArgs};
use crate::interp::env::Env;
use crate::interp::solver::{Got, Solver};
use crate::interp::summary::{CallEdge, SiteId, Target};
use crate::interp::tree::Node;
use crate::interp::value::ActivationId;
use crate::interp::value::dedup;
use crate::interp::value::{AbstractValue, Values};
use crate::interp::walk::{Walk, contains_yield};
use crate::sem::Definition;
use ruff_python_ast::{ExprCall, ExprRef, StmtFunctionDef};
use ruff_text_size::{Ranged, TextRange};
use smallvec::{SmallVec, smallvec};

impl<'db> Walk<'db> {
    pub fn eval_call(
        &mut self,
        sol: &mut Solver<'db>,
        call: &ExprCall,
        env: &mut Env<'db>,
    ) -> Values<'db> {
        let callees = self.eval(sol, &call.func, env);
        let args = self.eval_arguments(sol, &call.arguments, env);

        // One node per function, in definition order: the join order is an accident.
        let mut targets: Vec<(Definition<'db>, Option<ActivationId>)> = Vec::new();
        let mut ask_ty = false;
        for c in &callees {
            match *c {
                AbstractValue::Function(def) if sol.sem.is_project(def) => {
                    targets.push((def, None))
                }
                AbstractValue::Closure { func, activation } if sol.sem.is_project(func) => {
                    targets.push((func, Some(activation)))
                }
                _ => ask_ty = true,
            }
        }
        targets.sort_by_key(|(d, _)| sol.sem.def_position(*d));
        let site = SiteId::new(call.range(), self.frame.def_start);
        let mut edge_targets: SmallVec<[Target; 2]> = SmallVec::new();
        let mut out = Values::new();
        for (def, enclosing) in targets {
            let v = match sol.opts.mode {
                Mode::Inline => self.call_function(sol, def, enclosing, call.range(), &args),
                Mode::Summary => {
                    self.call_project(sol, def, enclosing, &args, &mut edge_targets)
                }
            };
            out.extend(v);
        }
        if !edge_targets.is_empty() {
            self.frame.calls.push(CallEdge {
                site,
                targets: edge_targets,
            });
        }
        if ask_ty {
            let ty = sol.sem.type_of(&self.frame.model, ExprRef::from(call));
            out.extend(self.lift(sol, ty));
        }
        dedup(&mut out);
        out
    }

    /// Inline mode: walk the callee's body now, as a child of this call.
    fn call_function(
        &mut self,
        sol: &mut Solver<'db>,
        def: Definition<'db>,
        enclosing: Option<ActivationId>,
        site: TextRange,
        args: &EvaluatedArgs<'db>,
    ) -> Values<'db> {
        let qname = sol.sem.qname(def);
        if sol.path.contains(&qname) {
            let mut node = Node::call(qname, site);
            node.marks.push(Mark::Recursive);
            self.frame.children.push(node);
            sol.stats.nodes += 1;
            return smallvec![AbstractValue::Unknown];
        }

        let parsed = sol.sem.parsed(def);
        let Some(node) = sol.sem.function_node(def, &parsed) else {
            return smallvec![AbstractValue::Unknown];
        };
        let runs = !args.has_empty();
        sol.path.push(qname.clone());
        sol.stats.walks += 1;

        let new = self.new_frame(sol, def);
        let caller = std::mem::replace(&mut self.frame, new);
        let bound = self.bind_parameters(sol, def, node, args, enclosing);
        sol.key(def, &bound);
        let mut env = self.env_from_bound(node, &bound);
        env.scope = Some(sol.sem.body_scope(def, node));
        env.enclosing = enclosing;
        if self.walk_stmts(sol, &node.body, env).is_some() {
            let none = self.none_value(sol);
            self.frame.returns.extend(none); // fell off the end
        }
        let callee = std::mem::replace(&mut self.frame, caller);
        sol.path.pop();

        self.frame.children.push(Node {
            qname,
            site: Some(site),
            marks: Vec::new(),
            children: callee.children,
        });
        sol.stats.nodes += 1;
        if !runs {
            Values::new() // Python never makes this call
        } else if contains_yield(&node.body) {
            smallvec![AbstractValue::Unknown] // a generator object
        } else {
            callee.returns
        }
    }

    /// Bind in the callee's frame (defaults and decorators are typed with its
    /// model), then drop that frame: only the slots leave.
    fn bind_for_callee(
        &mut self,
        sol: &mut Solver<'db>,
        def: Definition<'db>,
        node: &StmtFunctionDef,
        args: &EvaluatedArgs<'db>,
    ) -> Bound<'db> {
        let new = self.new_frame(sol, def);
        let caller = std::mem::replace(&mut self.frame, new);
        let bound = self.bind_parameters(sol, def, node, args, None);
        self.frame = caller;
        bound
    }

    /// Summary mode: key the call, record the edge, ask the solver. The callee's
    /// body is walked at most once per key; the tree is rendered from the graph.
    fn call_project(
        &mut self,
        sol: &mut Solver<'db>,
        def: Definition<'db>,
        enclosing: Option<ActivationId>,
        args: &EvaluatedArgs<'db>,
        targets: &mut SmallVec<[Target; 2]>,
    ) -> Values<'db> {
        if enclosing.is_some() {
            return smallvec![AbstractValue::Unknown]; // closures across a summary: step 07
        }
        let parsed = sol.sem.parsed(def);
        let Some(node) = sol.sem.function_node(def, &parsed) else {
            return smallvec![AbstractValue::Unknown];
        };
        let runs = !args.has_empty();
        let bound = self.bind_for_callee(sol, def, node, args);
        let ctx = sol.key(def, &bound);
        targets.push(Target::Call(ctx)); // recorded now, whatever `get` answers
        match sol.get(ctx) {
            Got::Ready(res) if runs => self.resolve(&res.returns),
            Got::Ready(_) => Values::new(), // Python never makes this call
            Got::Recursive => smallvec![AbstractValue::Unknown],
        }
    }
}
