mod env;

mod bind;
mod call;
mod defer;
#[cfg(test)]
mod fixture;
mod key;
mod member;
mod relevant;
mod render;
mod solver;
mod summary;
mod tree;
mod value;
mod walk;
pub use tree::{Mark, Node, to_text};

use std::time::Duration;

use ruff_db::files::File;
use summary::Cut;
use ty_project::ProjectDatabase;

use crate::{
    interp::{key::ContextId, render::render, solver::Solver, walk::Walk},
    sem::Sem,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Follow every call into the callee's body (parts 01 and 03).
    Inline,
    /// One walk per call, started from its key. The caller reads the summary.
    Summary,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Cuts {
    pub stack: u32,
    pub contexts: u32,
    pub contexts_per_func: u32,
    pub deadline: u32,
}

impl Cuts {
    pub fn count(&mut self, cut: Cut) {
        match cut {
            Cut::Stack => self.stack += 1,
            Cut::Contexts => self.contexts += 1,
            Cut::ContextsPerFunc => self.contexts_per_func += 1,
            Cut::Deadline => self.deadline += 1,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Stats {
    pub walks: u32,    // bodies walked, the entry included
    pub nodes: u32,    // tree nodes produced
    pub contexts: u32, // distinct keys interned
    pub reuse_hits: u32, // calls answered from a Ready slot
    pub rounds_capped: u32, // group fixpoints stopped by `max_rounds`
    pub cuts: Cuts,         // calls cut by a cost limit, by kind
}
#[derive(Clone, Debug)]
pub struct Options {
    pub mode: Mode,
    pub limits: Limits,
}

#[derive(Clone, Debug)]
pub struct Limits {
    // precision (rule 3: decided by the key and the code)
    pub max_rounds: u32, // a group's fixpoint stops here, marked incomplete
    pub fp_depth: u8,    // closures nest at most this deep, in keys and in summaries
    // cost (each one cuts, is marked, and is never reused)
    pub max_stack: usize,       // 256
    pub max_contexts: u32,      // 50_000
    pub contexts_per_func: u32, // 64, counted in this request only
    pub deadline: Option<Duration>,
    // checks
    pub reuse: bool,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            max_rounds: 32,
            fp_depth: 2,
            max_stack: 256,
            max_contexts: 50_000,
            contexts_per_func: 64,
            deadline: Some(Duration::from_secs(10)),
            reuse: true,
        }
    }
}

impl Default for Options {
    fn default() -> Self {
        Options {
            mode: Mode::Inline,
            limits: Limits::default(),
        }
    }
}

/// The call tree under `entry`, a function in `file`. `None` when there is no
/// such function.
pub fn call_tree(
    db: &ProjectDatabase,
    file: File,
    entry: &str,
    opts: &Options,
) -> Option<(Node, Stats)> {
    match opts.mode {
        Mode::Inline => inline_tree(db, file, entry, opts),
        Mode::Summary => {
            let (mut s, root_ctx) = solve(db, file, entry, opts)?;
            let qname = s.sem.qname(s.contexts[root_ctx].func);
            let mut path = vec![qname.clone()];
            let mut root = Node::root(qname);
            root.children = render(&s, &[root_ctx], &mut path);
            if s.is_incomplete(root_ctx) {
                root.marks.push(Mark::Incomplete);
            }
            s.stats.contexts = s.contexts.len() as u32;
            s.stats.nodes = root.children.iter().map(count_nodes).sum();
            Some((root, s.stats))
        }
    }
}

fn count_nodes(n: &Node) -> u32 {
    1 + n.children.iter().map(count_nodes).sum::<u32>()
}

/// Summary mode, stopped before rendering: the solver, with the entry solved.
pub fn solve<'db>(
    db: &'db ProjectDatabase,
    file: File,
    entry: &str,
    opts: &Options,
) -> Option<(Solver<'db>, ContextId)> {
    let sem = Sem::new(db);
    let def = sem.entry(file, entry)?;
    let parsed = sem.parsed(def);
    let node = sem.function_node(def, &parsed)?;
    let mut s = Solver::new(sem, opts.clone());
    // Called with no arguments (step 01.11). A walk of the entry's own frame
    // does the binding, exactly as step 02 did.
    let mut w = Walk::new(&s, def);
    let bound = w.bind_parameters(&mut s, def, node, None, &bind::EvaluatedArgs::default(), None);
    let root_ctx = s.key(&w, def, &bound, None);
    s.get(root_ctx);
    Some((s, root_ctx))
}

fn inline_tree(
    db: &ProjectDatabase,
    file: File,
    entry: &str,
    opts: &Options,
) -> Option<(Node, Stats)> {
    let sem = Sem::new(db);
    let def = sem.entry(file, entry)?;
    let parsed = sem.parsed(def);
    let node = sem.function_node(def, &parsed)?;
    let mut s = Solver::new(sem, opts.clone());
    let qname = s.sem.qname(def);
    s.path.push(qname.clone());
    s.stats.walks += 1; // the entry

    // Called with no arguments (step 01.11), and keyed like any call.
    let mut w = Walk::new(&s, def);
    let bound = w.bind_parameters(&mut s, def, node, None, &bind::EvaluatedArgs::default(), None);
    s.key(&w, def, &bound, None);
    let mut env = w.env_from_bound(node, &bound);
    env.scope = Some(s.sem.body_scope(def, node));
    w.walk_stmts(&mut s, &node.body, env);

    s.stats.contexts = s.contexts.len() as u32;
    let mut root = Node::root(qname);
    root.children = w.frame.children;
    Some((root, s.stats))
}
