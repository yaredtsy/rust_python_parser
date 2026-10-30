// src/interp/render.rs
use smallvec::SmallVec;

use crate::interp::Mark;
use crate::interp::key::ContextId;
use crate::interp::solver::Solver;
use crate::interp::summary::{SiteId, Target};
use crate::interp::tree::Node;
use crate::sem::Definition;

/// The children of a node that stands for `ctxs`: every context of ONE function
/// reached at one site on this path. One child per (site, callee function).
///
/// The invariant: a path in the rendered tree is a sequence of contexts, and
/// each callee was resolved under the context above it.
pub fn render(s: &Solver<'_>, ctxs: &[ContextId], path: &mut Vec<String>) -> Vec<Node> {
    let mut out = Vec::new();
    let func = s.contexts[ctxs[0]].func;
    for (site, targets) in sites_union(s, ctxs) {
        // same body: the sites line up
        for (callee, callee_ctxs) in group_by_function(s, &targets) {
            let q = s.sem.qname(callee);
            let mut node = Node::call(q.clone(), s.site_range(func, site));
            if path.contains(&q) {
                node.marks.push(Mark::Recursive); // shown, not entered
            } else {
                path.push(q);
                node.children = render(s, &callee_ctxs, path);
                path.pop();
            }
            out.push(node);
        }
    }
    out
}

/// Every site of the contexts' function, in the first context's order, with
/// the union of their targets.
fn sites_union<'db>(
    s: &Solver<'db>,
    ctxs: &[ContextId],
) -> Vec<(SiteId, SmallVec<[ContextId; 2]>)> {
    let mut out: Vec<(SiteId, SmallVec<[ContextId; 2]>)> = Vec::new();
    for &c in ctxs {
        let Some(res) = &s.slots[c].result else {
            continue;
        };
        for edge in &res.calls {
            let i = match out.iter().position(|(site, _)| *site == edge.site) {
                Some(i) => i,
                None => {
                    out.push((edge.site, SmallVec::new()));
                    out.len() - 1
                }
            };
            for t in &edge.targets {
                let Target::Call(ctx) = *t;
                if !out[i].1.contains(&ctx) {
                    out[i].1.push(ctx)
                }
            }
        }
    }
    out
}

/// Targets grouped by function, ordered by where each function is defined
/// (step 01.09): the same order inline mode uses.
fn group_by_function<'db>(
    s: &Solver<'db>,
    targets: &[ContextId],
) -> Vec<(Definition<'db>, Vec<ContextId>)> {
    let mut out: Vec<(Definition<'db>, Vec<ContextId>)> = Vec::new();
    for &t in targets {
        let f = s.contexts[t].func;
        match out.iter_mut().find(|(g, _)| *g == f) {
            Some((_, v)) => v.push(t),
            None => out.push((f, vec![t])),
        }
    }
    out.sort_by_key(|(f, _)| s.sem.def_position(*f));
    out
}
