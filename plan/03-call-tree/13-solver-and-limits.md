# 03.13 — The solver and its limits

The solver owns every context and every summary for one request. It reuses a
`Ready` summary, walks a new context once, finds recursion groups and runs their
fixpoint ([`12`](12-recursion.md)), and enforces the cost limits.

---

## The solver

```rust
pub struct Solver<'db> {
    sem:      Sem<'db>,                       // the only door into ty
    policy:   KeyPolicy,
    limits:   Limits,
    contexts: Interner<Context<'db>, ContextId>,
    fps:      Interner<Fp<'db>, FpId>,
    slots:    IndexVec<ContextId, Slot<'db>>,
    stack:    Vec<ContextId>,                 // contexts whose walk is in progress
    heads:    UnionFind<ContextId>,           // group heads; merging groups unions them
    fixpoint: Option<Fixpoint>,               // the group currently in its fixpoint, if any
    per_func: FxHashMap<Definition<'db>, u32>,   // contexts created this request, for the cost limit
    stats:    Stats,
}

struct Slot<'db> {
    state:   State,
    result:  Option<Arc<FunctionResult<'db>>>,   // Ready or Cut
    current: Option<Arc<FunctionResult<'db>>>,   // latest summary while Parked or Resolving
    walk:    Option<Box<Walk<'db>>>,             // kept while Parked or Resolving
    lowest:  usize,                              // smallest stack depth of a running context this walk read. MAX if none
    joins:   Option<ContextId>,                  // a resolving group this walk reached
    cut:     Option<Cut>,                        // this walk read an Incomplete(Cut) result
}

enum State {
    New,
    Running { depth: usize },    // its one walk is in progress
    Parked { head: ContextId },  // walked, waiting for its group
    Resolving,                   // its group is in the fixpoint
    Ready,                       // reusable: no cut. Finished, or with reusable gaps
    Cut,                         // Incomplete(Cut). Walked again when reached
}

pub enum Got<'db> {
    Ready(Arc<FunctionResult<'db>>),   // a final summary, or a member's latest one inside its fixpoint
    Pending,                           // record an Apply, use a pending value
}
```

Keep `Walk` and `Solver` separate. Functions take `(s: &mut Solver, w: &mut Walk)`,
so a call can re-enter the solver without fighting the borrow checker.

---

## `get`

```rust
pub fn get(&mut self, ctx: ContextId) -> Got<'db> {
    let depth = self.stack.len();
    match self.slots[ctx].state {
        State::Ready if self.limits.reuse => return Got::Ready(self.slots[ctx].result()),
        State::Running { depth: d }       => { self.reader_depends_on(d); return Got::Pending }
        State::Parked { .. } | State::Resolving => return self.read_group_member(ctx),
        _ => {}                                          // New, Cut, or a Ready slot in no-reuse mode
    }
    if let Some(cut) = self.cost_limit(ctx, depth) {
        self.reader_read_cut(cut);
        return Got::Ready(Arc::new(FunctionResult::cut(cut)));   // returns [Unknown]. Not stored
    }

    self.stack.push(ctx);
    self.slots[ctx].start(depth);                        // Running, lowest = MAX, no joins, no cut
    let w = walk(self, ctx);                             // the one walk of this body
    self.stack.pop();

    let (lowest, joins) = (self.slots[ctx].lowest, self.slots[ctx].joins);
    if lowest < depth {
        // In a cycle whose head is further down the stack: park, answer Pending.
        let head = self.heads.find(self.stack[lowest]);
        self.park(ctx, w, head);                         // a group it joined merges into `head` too
        self.reader_depends_on(lowest);
        return Got::Pending;
    }
    if let Some(head) = joins {
        // Reached a group that is in its fixpoint: join it.
        self.park(ctx, w, head);
        return self.read_group_member(ctx);
    }
    if lowest == depth {
        // ctx is the head of a cycle.
        if let Merged(d) = self.resolve_group(ctx, w) {  // the group turned out to be part of a bigger one
            self.reader_depends_on(d);
            return Got::Pending;
        }
    } else {
        self.finish_alone(ctx, w);                       // no cycle: Ready, or Cut
    }
    self.pass_cut_up(ctx);
    Got::Ready(self.slots[ctx].result())
}

/// A read of a parked or resolving context.
fn read_group_member(&mut self, ctx: ContextId) -> Got<'db> {
    let head = self.heads.find(self.slots[ctx].head());
    match self.slots[head].state {
        // The head's walk is still on the stack: the reader is in that cycle.
        State::Running { depth } => { self.reader_depends_on(depth); Got::Pending }
        // The group is in its fixpoint.
        _ if self.reader_is_fixpoint() => Got::Ready(self.slots[ctx].current()),   // an operation reads a member
        _ => { self.reader_joins(head); Got::Pending }                            // a new walk reaches the group
    }
}
```

`reader_depends_on(d)`, `reader_joins(head)`, `reader_read_cut(c)` and
`pass_cut_up(ctx)` update the reader: the slot on top of the stack, or the
fixpoint when an operation is being replayed. A dependency on a depth below the
fixpoint's head is what makes the group merge.

## `resolve_group`

```rust
fn resolve_group(&mut self, head: ContextId, head_walk: Walk<'db>) -> Resolved {
    self.park(head, head_walk, head);
    let mut members = self.take_parked(head);            // every slot parked under head, in walk-end order
    for m in &members { self.slots[*m].resolving_from_round0() }   // state Resolving, current = round-0 summary
    self.fixpoint = Some(Fixpoint { head, base: self.stack.len(), lowest: usize::MAX });

    let mut round = 0;
    loop {
        round += 1;
        let capped = round > self.limits.max_rounds;
        let mut changed = false;
        for &m in &members {
            let next = replay(self, m, capped);           // the member's deferred ops, then finish()
            if let Some(d) = self.fixpoint_lowest() {     // reached a context running further down
                self.park_all(&members, self.stack[d]);   // keep walks and current summaries
                self.fixpoint = None;
                return Resolved::Merged(d);
            }
            changed |= self.slots[m].set_current(next);   // compares returns, effects, objects
        }
        let joined = self.take_parked(head);              // contexts first walked this round that joined
        changed |= !joined.is_empty();
        for j in &joined { self.slots[*j].resolving_from_round0() }
        members.extend(joined);
        self.stats.contexts_per_round.push(self.contexts_created_since_last_round());
        if !changed || capped { break }
    }
    self.fixpoint = None;
    self.settle(&members, /* gaps: */ if round > self.limits.max_rounds { Gaps::CYCLE } else { Gaps::empty() });
    Resolved::Done
}
```

`settle` stores each member's last summary as its result: `State::Ready`, or
`State::Cut` for every member if any of them read a cut result.
`set_current` compares the parts a caller uses (returns, effects, objects), not
the calls, because a caller never reads a callee's calls.

---

## Truncation

`cost_limit` checks `max_stack`, `max_contexts`, `contexts_per_func` and the
deadline. When one fires, the call gets an `Incomplete(Cut)` result: returns
`[Unknown]`, no effects, no objects, no calls. It is not stored.

A walk that reads an `Incomplete(Cut)` result is itself `Incomplete(Cut)`. Its
slot is `State::Cut`, never `Ready`, and it is never reused. The next time it is
reached it is walked again, because the next path may reach it with more room
(fixture 2 in [`15`](15-checks-and-build-order.md#fixtures-for-the-core)). That
can be slow in a large truncated region. The deadline bounds it.

These limits depend on how a context was reached, not on its key. That is why
rule 3 keeps them out of `Ready`.

`contexts_per_func` only truncates, and only within one request. Widening the
extra contexts into a catch-all would make a summary depend on which calls came
first, and with a cross-request cache, on which request ran first.

### Stack depth in Rust

`get` recurses through `walk`, `eval` and `eval_call`. 256 levels of that can
overflow the default thread stack. Run the solver on a thread built with a large
stack (`std::thread::Builder::stack_size`), or grow it on demand with
`stacker::maybe_grow` at the top of `get`. **[check]** the stack used per level,
then set `max_stack` from it.

---

## Limits

```rust
pub struct Limits {
    // Precision. Decided by the key and the code (rule 3). Reusable.
    pub fp_depth:          u8    = 2,      // steps in a key
    pub max_path:          u8    = 6,      // steps in a Ref, an effect target or an Entry origin. >= fp_depth
    pub split_cap:         usize = 4,      // CPA combos per call. Above it, collapse to Fp::Union
    pub max_rounds:        u32   = 32,     // fixpoint rounds per group. Above it: CYCLE gap
    pub max_union_fanout:  Option<usize> = Some(4),   // cap_fanout (04). None: no cap
    pub max_chain_fanout:  usize = 8,      // values carried per attribute hop (07)

    // Cost. Each one gives Incomplete(Cut), is marked, and is never reused.
    pub max_contexts:      u32   = 50_000,
    pub contexts_per_func: u32   = 64,     // counted in this request only
    pub max_stack:         usize = 256,
    pub deadline:          Option<Duration> = Some(Duration::from_secs(10)),

    // View. Render only.
    pub render_depth:      Option<usize> = None,    // no depth limit
    pub render_nodes:      u32   = 100_000,         // the guard. Every cut is marked

    // Checks.
    pub reuse:             bool  = true,   // false: recompute every Ready summary (15)
}
```

Every field can be overridden from `initialize`'s `config` dict and from the dev
CLI's `--limits` ([`04-build/00`](../04-build/00-dev-cli.md)). `fp_depth`,
`max_path`, `max_rounds` and the fan-out defaults are guesses: S5 and S10 measure
them ([`15`](15-checks-and-build-order.md#build-order)).

## Stats

One counter per limit and per stage, returned in the response. A missing edge
should be explainable from a number.

| counter | counts |
|---|---|
| `contexts`, `walks` | contexts created, bodies walked |
| `reuse_hits` | `Ready` summaries reused |
| `parked`, `groups`, `group_sizes` | walks that parked, groups resolved, members per group |
| `rounds` | fixpoint rounds per group |
| `contexts_per_round` | contexts created in each fixpoint round |
| `joins`, `merges` | contexts that joined a group, groups merged into a bigger one |
| `cycle_gaps`, `deep_write_fallbacks` | summaries with the `CYCLE` gap, writes past `max_path` |
| `cuts` | by `Cut` kind, including `contexts_per_func` |
| `cpa_collapses` | calls over `split_cap` |
| `union_fanout_hits`, `union_pruned`, `chain_fanout_hits` | fan-out caps |
| `types_dropped` | `Ty` values keyed as `Unknown` because `StableType` can't encode them |
| `annotation_fallbacks` | parameters started from their annotation |
| `render_cuts` | `Mark::Cut` nodes |

---

→ Next: [`14-output.md`](14-output.md)
