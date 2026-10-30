// src/interp/solver.rs
use std::sync::Arc;

use ruff_index::IndexVec;
use ruff_text_size::TextRange;

use crate::interp::key::{Context, ContextId, Fp, FpId, Interner};
use crate::interp::summary::{FunctionResult, SiteId};
use crate::interp::walk::Walk;
use crate::interp::{Options, Stats};
use crate::sem::{Definition, Sem};

#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub enum State {
    #[default]
    New,
    Running, // its walk is in progress
    Ready,   // walked: its summary can be reused
}

#[derive(Default)]
pub struct Slot<'db> {
    pub state: State,
    pub result: Option<Arc<FunctionResult<'db>>>,
}

pub enum Got<'db> {
    Ready(Arc<FunctionResult<'db>>),
    Recursive, // the context is running: a recursive call. Step 05 replaces this
}

/// What outlives one walk: the request's options, interners, slots and counters.
pub struct Solver<'db> {
    pub sem: Sem<'db>,
    pub opts: Options,
    pub fps: Interner<Fp<'db>, FpId>,
    pub contexts: Interner<Context<'db>, ContextId>,
    pub slots: IndexVec<ContextId, Slot<'db>>, // summary mode: one per context
    pub stack: Vec<ContextId>,                 // summary mode: the walks in progress
    pub path: Vec<String>, // inline mode only: qualified names being walked, its recursion guard
    pub stats: Stats,
}

impl<'db> Solver<'db> {
    pub fn new(sem: Sem<'db>, opts: Options) -> Self {
        Solver {
            sem,
            opts,
            fps: Interner::default(),
            contexts: Interner::default(),
            slots: IndexVec::new(),
            stack: Vec::new(),
            path: Vec::new(),
            stats: Stats::default(),
        }
    }

    /// The summary of `ctx`: walked now, or handed out from its slot.
    pub fn get(&mut self, ctx: ContextId) -> Got<'db> {
        while self.slots.len() < self.contexts.len() {
            self.slots.push(Slot::default());
        }
        match self.slots[ctx].state {
            State::Ready if self.opts.reuse => {
                self.stats.reuse_hits += 1;
                return Got::Ready(self.slots[ctx].result.clone().unwrap());
            }
            State::Running => return Got::Recursive,
            _ => {}
        }
        self.slots[ctx].state = State::Running;
        self.stack.push(ctx);
        let result = Arc::new(walk_context(self, ctx));
        self.stack.pop();
        self.slots[ctx] = Slot {
            state: State::Ready,
            result: Some(result.clone()),
        };
        Got::Ready(result)
    }

    /// The file range of a site recorded relative to `func`'s `def`.
    pub fn site_range(&self, func: Definition<'db>, site: SiteId) -> TextRange {
        let start = self.sem.def_position(func).1;
        TextRange::new(start + site.start, start + site.end)
    }
}

pub fn walk_context<'db>(s: &mut Solver<'db>, ctx: ContextId) -> FunctionResult<'db> {
    let func = s.contexts[ctx].func;
    let parsed = s.sem.parsed(func);
    let node = s
        .sem
        .function_node(func, &parsed)
        .expect("contexts are only made for defs");
    s.stats.walks += 1;
    let mut w = Walk::new(s, func);
    let env = w.materialize(s, ctx, node);
    let end = w.walk_stmts(s, &node.body, env);
    w.finish(s, end, node)
}
