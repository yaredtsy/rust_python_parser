// src/interp/solver.rs
use std::sync::Arc;

use ruff_index::IndexVec;
use ruff_text_size::TextRange;

use crate::interp::key::{Context, ContextId, Fp, FpId, Interner};
use crate::interp::summary::{FunctionResult, SiteId};
use crate::interp::defer::no_values;
use crate::interp::walk::{Walk, contains_yield};
use crate::interp::{Options, Stats};
use crate::sem::{Definition, Sem};

#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub enum State {
    #[default]
    New,
    Running { depth: usize }, // its walk is in progress, at this stack depth
    Resolving,                // its fixpoint is in progress: `current` is the latest round
    Ready,                    // walked: its summary can be reused
}

pub struct Slot<'db> {
    pub state: State,
    pub result: Option<Arc<FunctionResult<'db>>>,
    /// The latest summary, during the fixpoint.
    pub current: Option<Arc<FunctionResult<'db>>>,
    /// Smallest stack depth of a running context this walk read. `usize::MAX`: none.
    pub lowest: usize,
}

impl Default for Slot<'_> {
    fn default() -> Self {
        Slot {
            state: State::New,
            result: None,
            current: None,
            lowest: usize::MAX,
        }
    }
}

pub enum Got<'db> {
    Ready(Arc<FunctionResult<'db>>),
    Pending, // the context is running: a recursive call, its value found later
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
        let depth = self.stack.len();
        match self.slots[ctx].state {
            State::Ready if self.opts.reuse => {
                self.stats.reuse_hits += 1;
                return Got::Ready(self.slots[ctx].result.clone().unwrap());
            }
            State::Running { depth: d } => {
                self.reader_depends_on(d);
                return Got::Pending;
            }
            State::Resolving => return Got::Ready(self.slots[ctx].current.clone().unwrap()),
            _ => {}
        }

        self.slots[ctx].state = State::Running { depth };
        self.slots[ctx].lowest = usize::MAX;
        self.stack.push(ctx);
        let w = walk_context(self, ctx);
        self.stack.pop();

        let lowest = self.slots[ctx].lowest;
        let result = if lowest == depth {
            self.resolve_one(ctx, w) // the head of its own cycle
        } else {
            w.finish_round(&no_values(&w)) // alone (step 06 handles lowest < depth)
        };
        let result = Arc::new(result);
        self.slots[ctx].state = State::Ready;
        self.slots[ctx].result = Some(result.clone());
        Got::Ready(result)
    }

    /// The context on top of the stack read a running context at depth `d`.
    fn reader_depends_on(&mut self, d: usize) {
        if let Some(&reader) = self.stack.last() {
            let slot = &mut self.slots[reader];
            slot.lowest = slot.lowest.min(d);
        }
    }

    /// The file range of a site recorded relative to `func`'s `def`.
    pub fn site_range(&self, func: Definition<'db>, site: SiteId) -> TextRange {
        let start = self.sem.def_position(func).1;
        TextRange::new(start + site.start, start + site.end)
    }
}

/// Walks the body once. The summary is the solver's to finish: it may need rounds.
pub fn walk_context<'db>(s: &mut Solver<'db>, ctx: ContextId) -> Walk<'db> {
    let func = s.contexts[ctx].func;
    let parsed = s.sem.parsed(func);
    let node = s
        .sem
        .function_node(func, &parsed)
        .expect("contexts are only made for defs");
    s.stats.walks += 1;
    let mut w = Walk::new(s, func);
    w.generator = contains_yield(&node.body);
    let env = w.materialize(s, ctx, node);
    if w.walk_stmts(s, &node.body, env).is_some() {
        let none = w.none_value(s);
        w.frame.returns.extend(none); // fell off the end
    }
    w
}
