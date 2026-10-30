// src/interp/defer.rs
use std::sync::Arc;

use ruff_index::IndexVec;
use smallvec::smallvec;

use crate::interp::bind::EvaluatedArgs;
use crate::interp::env::Env;
use crate::interp::key::ContextId;
use crate::interp::solver::{Got, Solver};
use crate::interp::summary::{FunctionResult, SiteId};
use crate::interp::value::{AbstractValue, DeferId, Values};
use crate::interp::walk::Walk;

#[derive(Clone)]
pub struct Deferred<'db> {
    pub op: DeferredOp<'db>,
    pub before: Env<'db>, // the env at that point: an O(1) clone
}

#[derive(Clone)]
pub enum DeferredOp<'db> {
    /// A call to a context that is still running. Its value is that context's returns.
    Apply { ctx: ContextId },
    /// A call whose callee, or an argument a key would read, is pending.
    /// It can't be keyed now. It is keyed in the fixpoint.
    Call {
        site: SiteId,
        callee: Values<'db>,
        args: EvaluatedArgs<'db>,
        /// ty's answer for the call expression, for a callee that is not project code.
        fallback: Values<'db>,
        /// The `CallEdge` pushed at this point of the walk, refilled by every replay.
        edge: usize,
    },
}

/// Every pending value empty: round 0.
pub fn no_values<'db>(w: &Walk<'db>) -> IndexVec<DeferId, Values<'db>> {
    IndexVec::from_raw(vec![Values::new(); w.deferred.len()])
}

/// Replace every Pending(id) by what it resolved to.
fn substitute<'db>(vals: &Values<'db>, now: &IndexVec<DeferId, Values<'db>>) -> Values<'db> {
    vals.iter()
        .flat_map(|v| match *v {
            AbstractValue::Pending(id) => now[id].clone(),
            other => smallvec![other],
        })
        .collect()
}

impl<'db> Solver<'db> {
    /// The head of its own cycle: repeat the deferred operations, never the body,
    /// until the summary's returns stop changing.
    pub fn resolve_one(&mut self, ctx: ContextId, mut w: Walk<'db>) -> FunctionResult<'db> {
        let mut resolved = no_values(&w); // round 0: every pending value empty
        let mut current = Arc::new(w.finish_round(&resolved));
        self.slots[ctx].state = crate::interp::solver::State::Resolving;
        loop {
            self.slots[ctx].current = Some(current.clone());
            resolved = w.replay(self, &resolved); // each operation once, in order
            let next = w.finish_round(&resolved);
            let changed = next.returns != current.returns; // what a caller reads
            current = Arc::new(next);
            if !changed {
                break;
            }
        }
        self.slots[ctx].current = None;
        Arc::unwrap_or_clone(current)
    }
}

impl<'db> Walk<'db> {
    pub fn defer(&mut self, op: DeferredOp<'db>, env: &Env<'db>) -> DeferId {
        self.deferred.push(Deferred {
            op,
            before: env.clone(),
        })
    }

    /// Each deferred operation once, in order. An operation may read the
    /// results of earlier ones in this same round.
    fn replay(
        &mut self,
        sol: &mut Solver<'db>,
        last: &IndexVec<DeferId, Values<'db>>,
    ) -> IndexVec<DeferId, Values<'db>> {
        let mut now = last.clone();
        self.replaying = true;
        let ids: Vec<DeferId> = self.deferred.indices().collect();
        for id in ids {
            let d = self.deferred[id].clone();
            let v = match d.op {
                DeferredOp::Apply { ctx } => match sol.get(ctx) {
                    Got::Ready(res) => self.resolve(&res.returns),
                    Got::Pending => Values::new(), // step 06
                },
                DeferredOp::Call {
                    site,
                    callee,
                    args,
                    fallback,
                    edge,
                } => {
                    let callee = substitute(&callee, &now);
                    let args = EvaluatedArgs {
                        positional: args.positional.iter().map(|v| substitute(v, &now)).collect(),
                        keywords: args
                            .keywords
                            .iter()
                            .map(|(n, v)| (n.clone(), substitute(v, &now)))
                            .collect(),
                        has_splat: args.has_splat,
                    };
                    self.call_values(sol, site, &callee, &args, None, &fallback, Some(edge), &d.before)
                }
            };
            now[id] = v;
        }
        self.replaying = false;
        now
    }
}
