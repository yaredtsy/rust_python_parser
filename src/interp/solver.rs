// src/interp/solver.rs
use std::sync::Arc;

use ruff_index::{Idx, IndexVec};
use ruff_text_size::TextRange;

use crate::interp::key::{Context, ContextId, Fp, FpId, Interner};
use crate::interp::defer::{DeferredOp, no_values};
use crate::interp::summary::{FunctionResult, Gaps, SiteId, Status};
use crate::interp::walk::{Walk, contains_yield};
use crate::interp::{Options, Stats};
use crate::sem::{Definition, Sem};

#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub enum State {
    #[default]
    New,
    Running { depth: usize }, // its walk is in progress, at this stack depth
    Parked { head: ContextId }, // walked, waiting for its group's fixpoint
    Resolving,                // its group is in the fixpoint: `current` is the latest round
    Ready,                    // walked: its summary can be reused
}

pub struct Slot<'db> {
    pub state: State,
    pub result: Option<Arc<FunctionResult<'db>>>,
    /// The latest summary, during the fixpoint.
    pub current: Option<Arc<FunctionResult<'db>>>,
    /// Smallest stack depth of a running context this walk read. `usize::MAX`: none.
    pub lowest: usize,
    /// The walk, with its deferred operations: kept while Parked or Resolving.
    pub walk: Option<Box<Walk<'db>>>,
    /// A resolving group this walk reached: it joins that group.
    pub joins: Option<ContextId>,
}

impl Default for Slot<'_> {
    fn default() -> Self {
        Slot {
            state: State::New,
            result: None,
            current: None,
            lowest: usize::MAX,
            walk: None,
            joins: None,
        }
    }
}

impl<'db> Slot<'db> {
    fn current(&self) -> Arc<FunctionResult<'db>> {
        self.current.clone().expect("a group member has a current summary")
    }

    /// Back to round 0: the group's fixpoint (re)starts this member.
    fn resolving_from_round0(&mut self) {
        let w = self.walk.as_ref().expect("a member keeps its walk");
        self.current = Some(Arc::new(w.finish_round(&no_values(w))));
        self.walk.as_mut().unwrap().resolved = no_values(w);
        self.state = State::Resolving;
    }

    /// Store the round's summary. True when what a caller reads changed.
    fn set_current(&mut self, next: FunctionResult<'db>) -> bool {
        let changed = self.current.as_ref().is_none_or(|c| c.returns != next.returns);
        self.current = Some(Arc::new(next));
        changed
    }
}

/// Union-find over contexts: the head of the group a parked context belongs to.
#[derive(Default)]
pub struct Heads {
    parent: Vec<ContextId>,
}

impl Heads {
    fn grow(&mut self, ctx: ContextId) {
        while self.parent.len() <= ctx.index() {
            self.parent.push(ContextId::new(self.parent.len()));
        }
    }
    pub fn find(&mut self, ctx: ContextId) -> ContextId {
        self.grow(ctx);
        let mut root = ctx;
        while self.parent[root.index()] != root {
            root = self.parent[root.index()];
        }
        let mut c = ctx;
        while self.parent[c.index()] != root {
            c = std::mem::replace(&mut self.parent[c.index()], root);
        }
        root
    }
    /// `ctx`'s whole group goes under `head`'s.
    fn union_into(&mut self, ctx: ContextId, head: ContextId) {
        let (a, b) = (self.find(ctx), self.find(head));
        self.parent[a.index()] = b;
    }
    fn reset(&mut self, ctx: ContextId) {
        self.grow(ctx);
        self.parent[ctx.index()] = ctx;
    }
}

/// A group's fixpoint in progress. Fixpoints nest like calls.
#[derive(Clone, Copy)]
pub struct Fixpoint {
    pub head: ContextId,
    /// The stack depth the operations are replayed at.
    pub base: usize,
    /// Smallest depth below `base` an operation read. `usize::MAX`: none.
    pub lowest: usize,
    /// A resolving group an operation reached.
    pub joins: Option<ContextId>,
}

pub enum Resolved {
    Done,
    Merged(usize),      // part of a bigger cycle, whose head is at this depth
    Joined(ContextId),  // its replay reached an outer group's fixpoint
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
    pub heads: Heads,
    pub fixpoint: Option<Fixpoint>,
    pub parked: Vec<ContextId>, // parked contexts, in the order their walks ended
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
            heads: Heads::default(),
            fixpoint: None,
            parked: Vec::new(),
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
            State::Parked { .. } | State::Resolving => return self.read_group_member(ctx),
            _ => {}
        }

        self.heads.reset(ctx);
        self.slots[ctx].state = State::Running { depth };
        self.slots[ctx].lowest = usize::MAX;
        self.slots[ctx].joins = None;
        self.stack.push(ctx);
        let w = walk_context(self, ctx);
        self.stack.pop();

        let (lowest, joins) = (self.slots[ctx].lowest, self.slots[ctx].joins);
        if lowest < depth {
            // In a cycle whose head is further down the stack: park.
            let head = self.heads.find(self.stack[lowest]);
            self.park(ctx, w, head);
            self.reader_depends_on(lowest); // the caller is in the cycle too
            if let Some(j) = joins {
                self.reader_joins(j); // and reaches the resolving group too
            }
            return Got::Pending;
        }
        if let Some(head) = joins {
            self.park(ctx, w, head); // first reached during a fixpoint: join it
            return self.read_group_member(ctx);
        }
        if lowest == depth {
            match self.resolve_group(ctx, w) {
                Resolved::Done => {}
                Resolved::Merged(d) => {
                    self.reader_depends_on(d);
                    return Got::Pending;
                }
                Resolved::Joined(_) => return self.read_group_member(ctx),
            }
        } else {
            self.finish_alone(ctx, w);
        }
        Got::Ready(self.slots[ctx].result.clone().unwrap())
    }

    /// A walk that waited on nothing: its summary is final.
    fn finish_alone(&mut self, ctx: ContextId, w: Walk<'db>) {
        debug_assert!(w.deferred.is_empty(), "{ctx:?} finishes alone with deferred operations");
        let result = Arc::new(w.finish_round(&no_values(&w)));
        self.slots[ctx].state = State::Ready;
        self.slots[ctx].result = Some(result);
    }

    /// A read of a parked or resolving context.
    fn read_group_member(&mut self, ctx: ContextId) -> Got<'db> {
        let head = self.heads.find(ctx);
        let state = self.slots[head].state;
        match state {
            State::Running { depth } => {
                self.reader_depends_on(depth); // still being walked
                Got::Pending
            }
            _ if self.reader_is_fixpoint_of(head) => Got::Ready(self.slots[ctx].current()),
            _ => {
                self.reader_joins(head); // a new walk, or another group's fixpoint
                Got::Pending
            }
        }
    }

    /// The reader is the replay of `head`'s own fixpoint, not a walk on top of
    /// it and not a fixpoint nested inside it.
    fn reader_is_fixpoint_of(&mut self, head: ContextId) -> bool {
        let Some(f) = self.fixpoint else { return false };
        self.stack.len() == f.base && self.heads.find(f.head) == head
    }

    /// Keep a walked context under `head` until its group resolves.
    fn park(&mut self, ctx: ContextId, w: Walk<'db>, head: ContextId) {
        if self.slots[ctx].current.is_none() {
            // A joining context is read at once: it needs its round-0 summary now.
            self.slots[ctx].current = Some(Arc::new(w.finish_round(&no_values(&w))));
        }
        self.heads.union_into(ctx, head); // whatever is parked under ctx follows it
        self.slots[ctx].walk = Some(Box::new(w));
        self.slots[ctx].state = State::Parked { head };
        self.parked.push(ctx);
    }

    /// Members whose fixpoint stopped go under another head, walks and summaries kept.
    fn park_all(&mut self, members: &[ContextId], head: ContextId) {
        let head = self.heads.find(head);
        for &m in members {
            self.heads.union_into(m, head);
            self.slots[m].state = State::Parked { head };
            self.parked.push(m);
        }
    }

    /// Every context parked under `head`, in walk-end order.
    fn take_parked(&mut self, head: ContextId) -> Vec<ContextId> {
        let all = std::mem::take(&mut self.parked);
        let (mine, rest): (Vec<_>, Vec<_>) =
            all.into_iter().partition(|&c| self.heads.find(c) == head);
        self.parked = rest;
        mine
    }

    /// The head of a cycle: one fixpoint over every context parked under it.
    fn resolve_group(&mut self, head: ContextId, head_walk: Walk<'db>) -> Resolved {
        self.park(head, head_walk, head);
        let mut members = self.take_parked(head);
        for &m in &members {
            self.slots[m].resolving_from_round0();
        }
        // Fixpoints nest: this one may run inside an operation of another. Keep it.
        let outer = self.fixpoint.replace(Fixpoint {
            head,
            base: self.stack.len(),
            lowest: usize::MAX,
            joins: None,
        });

        let mut round = 0;
        let resolved = 'rounds: loop {
            round += 1;
            let capped = round > self.opts.max_rounds;
            let mut changed = false;
            for &m in &members {
                let next = self.replay_member(m, capped);
                let f = self.fixpoint.unwrap();
                if f.lowest != usize::MAX {
                    // reached a context running further down: part of a bigger cycle
                    let under = self.stack[f.lowest];
                    self.park_all(&members, under);
                    break 'rounds Resolved::Merged(f.lowest);
                }
                if let Some(g) = f.joins {
                    // reached an outer group in its fixpoint
                    self.park_all(&members, g);
                    break 'rounds Resolved::Joined(g);
                }
                changed |= self.slots[m].set_current(next);
            }
            let joined = self.take_parked(head);
            changed |= !joined.is_empty();
            for &j in &joined {
                self.slots[j].resolving_from_round0();
            }
            members.extend(joined);
            if !changed || capped {
                if capped {
                    self.stats.rounds_capped += 1;
                }
                self.settle(&members, if capped { Gaps::CYCLE } else { Gaps::default() });
                break Resolved::Done;
            }
        };
        self.fixpoint = outer;
        resolved
    }

    /// One member's deferred operations, then its summary for the round.
    fn replay_member(&mut self, m: ContextId, capped: bool) -> FunctionResult<'db> {
        let mut w = self.slots[m].walk.take().expect("a member keeps its walk");
        w.resolved = w.replay(self, capped);
        let next = w.finish_round(&w.resolved);
        self.slots[m].walk = Some(w);
        next
    }

    /// The group is final: every member's last summary is its result.
    fn settle(&mut self, members: &[ContextId], gaps: Gaps) {
        let group = self.heads.find(members[0]);
        for &m in members {
            for d in &self.slots[m].walk.as_ref().unwrap().deferred {
                if let DeferredOp::Apply { ctx } = d.op {
                    debug_assert_eq!(
                        self.heads.find(ctx),
                        group,
                        "{m:?} waited on {ctx:?}, outside its group"
                    );
                }
            }
        }
        for &m in members {
            let slot = &mut self.slots[m];
            let mut result = Arc::unwrap_or_clone(slot.current.take().unwrap());
            result.status = Status { gaps };
            slot.result = Some(Arc::new(result));
            slot.walk = None;
            slot.state = State::Ready;
        }
    }

    /// The context on top of the stack, or the fixpoint when an operation is
    /// being replayed, read a running context at depth `d`.
    fn reader_depends_on(&mut self, d: usize) {
        if let Some(f) = self.fixpoint.as_mut().filter(|f| f.base == self.stack.len()) {
            f.lowest = f.lowest.min(d);
        } else if let Some(&reader) = self.stack.last() {
            let slot = &mut self.slots[reader];
            slot.lowest = slot.lowest.min(d);
        }
    }

    /// The reader reached a resolving group, and joins it.
    fn reader_joins(&mut self, head: ContextId) {
        if let Some(f) = self.fixpoint.as_mut().filter(|f| f.base == self.stack.len()) {
            f.joins.get_or_insert(head);
        } else if let Some(&reader) = self.stack.last() {
            self.slots[reader].joins.get_or_insert(head);
        }
    }

    /// Its summary is missing what later rounds would have added.
    pub fn is_incomplete(&self, ctx: ContextId) -> bool {
        self.slots[ctx].result.as_ref().is_some_and(|r| !r.status.gaps.is_empty())
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
