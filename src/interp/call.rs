// src/interp/call.rs
use crate::interp::Mark;
use crate::interp::Mode;
use crate::interp::bind::{Bound, EvaluatedArgs};
use crate::interp::defer::DeferredOp;
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
        let site = SiteId::new(call.range(), self.frame.def_start);
        if sol.opts.mode == Mode::Summary && self.needs_defer(sol, &callees, &args) {
            // It can't be keyed yet: record it, and find its value in the fixpoint.
            let ty = sol.sem.type_of(&self.frame.model, ExprRef::from(call));
            let fallback = self.lift(sol, ty);
            let edge = self.frame.calls.len();
            self.frame.calls.push(CallEdge {
                site,
                targets: SmallVec::new(), // refilled by every replay, so the edge keeps its place
            });
            let id = self.defer(
                DeferredOp::Call {
                    site,
                    callee: callees,
                    args,
                    fallback,
                    edge,
                },
                env,
            );
            return smallvec![AbstractValue::Pending(id)];
        }
        self.call_values(sol, site, &callees, &args, Some(call), &Values::new(), None, env)
    }

    /// A callee that is pending, or a project callee given a pending argument:
    /// its key would read a value that is not known yet.
    fn needs_defer(
        &self,
        sol: &Solver<'db>,
        callees: &Values<'db>,
        args: &EvaluatedArgs<'db>,
    ) -> bool {
        let pending = |v: &Values<'db>| v.iter().any(|x| matches!(x, AbstractValue::Pending(_)));
        if pending(callees) {
            return true;
        }
        let has_pending_arg =
            args.positional.iter().any(pending) || args.keywords.iter().any(|(_, v)| pending(v));
        has_pending_arg
            && callees.iter().any(|c| match *c {
                AbstractValue::Function(d) => sol.sem.is_project(d),
                AbstractValue::Closure { func, .. } => sol.sem.is_project(func),
                _ => false,
            })
    }

    /// The second half of a call: the callee values and the arguments are known.
    /// A deferred `Call` runs this again in the fixpoint, with `edge` set to the
    /// slot its `CallEdge` already holds, and no expression to ask ty about.
    pub fn call_values(
        &mut self,
        sol: &mut Solver<'db>,
        site: SiteId,
        callees: &Values<'db>,
        args: &EvaluatedArgs<'db>,
        call: Option<&ExprCall>,
        fallback: &Values<'db>,
        edge: Option<usize>,
        env: &Env<'db>,
    ) -> Values<'db> {
        // One node per function, in definition order: the join order is an accident.
        let mut targets: Vec<(Definition<'db>, Option<ActivationId>)> = Vec::new();
        let mut ask_ty = false;
        for c in callees {
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
        let mut edge_targets: SmallVec<[Target; 2]> = SmallVec::new();
        let mut out = Values::new();
        for (def, enclosing) in targets {
            let v = match sol.opts.mode {
                Mode::Inline => {
                    let range = self.site_range(site);
                    self.call_function(sol, def, enclosing, range, args)
                }
                Mode::Summary => {
                    self.call_project(sol, def, enclosing, args, &mut edge_targets, env)
                }
            };
            out.extend(v);
        }
        match edge {
            Some(i) => self.frame.calls[i].targets = edge_targets,
            None if !edge_targets.is_empty() => self.frame.calls.push(CallEdge {
                site,
                targets: edge_targets,
            }),
            None => {}
        }
        if ask_ty {
            match call {
                Some(call) => {
                    let ty = sol.sem.type_of(&self.frame.model, ExprRef::from(call));
                    out.extend(self.lift(sol, ty));
                }
                None => out.extend(fallback.iter().copied()),
            }
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
        env: &Env<'db>,
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
            _ if !runs => Values::new(), // Python never makes this call, whatever `get` answered
            Got::Ready(res) => self.resolve(&res.returns),
            Got::Pending if self.replaying => Values::new(), // the solver flagged the reader
            Got::Pending => {
                let id = self.defer(DeferredOp::Apply { ctx }, env);
                smallvec![AbstractValue::Pending(id)]
            }
        }
    }
}
