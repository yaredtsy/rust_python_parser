# 02.05 — Recursion: pending values

**You build:** the value of a recursive call. A call to a context that is
still being walked gets a **pending** value. Uses of it that need to know
what it is are recorded as **deferred operations**. When the walk ends, the
solver repeats only those operations until the function's summary stops
changing. The body is never walked twice.
**Fixture:** `python/pending.py`
**Plan:** `plan/03-call-tree/12-recursion.md` § "Pending values", § "Deferred
operations", § "The fixpoint", `13-solver-and-limits.md` § "`get`"

This step handles a function that calls itself. Step 06 extends it to
several functions calling each other.

---

## The fixture

```python
def grow(n):
    if n: return grow(n - 1)
    return done

def walk(n):
    r = walk(n - 1)
    r()                  # summary: done
    return done

def rec(n):
    return rec(n - 1)    # no base case

def main():
    grow(3)()            # done
    walk(3)
    rec(3)()             # nothing: rec returns nothing

# expect inline:
# main
#   grow
#     grow [recursive]
#   done
#   walk
#     walk [recursive]
#   rec
#     rec [recursive]
# expect summary:
#   … the same, plus `done` under `walk`
```

This is the first fixture where the modes differ. The harness learns
mode-specific blocks in this step.

## Predict

1. In inline mode, what is `r` in `walk`, and why is there no edge for `r()`?
2. `walk` returns `done` on every path that returns. So what *should* `r` be?
   Could you know that before finishing `walk`'s walk?
3. `rec` has no base case. What should `rec(3)` be: `Unknown`, empty, or
   something else?

---

## Build

### Decision 1: a recursive call is recorded, not entered, and its value is pending

```rust
pub enum AbstractValue<'db> {
    // …
    Pending(DeferId),   // the value of an operation waiting on a recursive call
}

pub enum Got<'db> {
    Ready(Arc<FunctionResult<'db>>),
    Pending,            // replaces `Recursive`
}
```

When `get` finds the context `Running`, it answers `Pending`. The edge was
already recorded (step 03, decision 1), so the tree is right. The call's value
becomes `Pending(id)`, where `id` names a deferred operation:

```rust
pub struct Deferred<'db> {
    pub op: DeferredOp<'db>,
    pub before: Env<'db>,      // the env at that point: an O(1) clone
}

pub enum DeferredOp<'db> {
    /// A call to a context that is still running. Its value is that context's returns.
    Apply { ctx: ContextId },
    /// A call whose callee, or an argument a key would read, is pending.
    /// It can't be keyed now. It is keyed in the fixpoint.
    Call { site: SiteId, callee: Values<'db>, args: EvaluatedArgs<'db> },
}
```

**Why not `Unknown`, as before:** `Unknown` is final. `r()` with `r = Unknown`
has no edge, forever. A pending value says "known later". The walk records
what it would do with the value, and does it once the value is known.

**Why not walk the recursive call:** it would never end, or end at an
arbitrary depth with a wrong answer. And it would walk the body again, which
is what summaries exist to avoid.

### Decision 2: carry a pending value until something needs to know it

A pending value is a value like any other. It can be assigned to a local,
joined at a branch, captured, returned. Only a use that needs to know *what*
it is becomes a deferred operation:

| use of a pending value `p` | becomes |
|---|---|
| a call to a running context | `Apply`, value `Pending(new id)` |
| `p(…)` | `Call`, value `Pending(new id)` |
| `f(p)`, where `f` is a project function (its key would read `p`) | `Call`, value `Pending(new id)` |
| `print(p)`: no key reads it | nothing: the call happens now |
| `x = p`, `return p`, `if p:` | nothing: carried along |
| `p.name`, `p.name = v` | part 04, with objects |

In `walk`: `walk(n - 1)` is `Apply { ctx: walk }`, value `Pending(0)`. `r()` is
`Call { callee: [Pending(0)] }`, value `Pending(1)`. `return done` is concrete.

**Why record `before`:** when the operation is repeated later, it must see the
env as it was at that point of the walk, not as it was at the end. Persistent
maps make that a pointer copy (step 01.03, decision 2).

### Decision 3: the walk tells the solver it read a running context

```rust
pub struct Slot<'db> {
    pub state: State,
    pub result: Option<Arc<FunctionResult<'db>>>,
    pub current: Option<Arc<FunctionResult<'db>>>,   // the latest summary, during the fixpoint
    pub lowest: usize,                               // smallest stack depth of a running context
}                                                    //   this walk read. usize::MAX: none

pub enum State { New, Running { depth: usize }, Resolving, Ready }
```

When `get` answers `Pending` for a context running at depth `d`, it lowers the
reader's `lowest` to `d`. The reader is the context on top of the stack. When
the reader's walk ends, `lowest` says where it stands:

| `lowest` | the walk |
|---|---|
| `usize::MAX` | read no running context: it finishes alone, as in step 03 |
| its own depth | read only itself: it is the **head** of its own cycle. Resolve it now |
| below its own depth | depends on a context further down: step 06 |

Until step 06, the third case finishes alone with its pending values empty.
`recursion.py` from part 01 still passes: its trees don't use any recursive
call's value.

### Decision 4: a fixpoint over the operations, never the body

```
round 0:  finish() with every pending value empty
round n:  repeat each deferred operation once, in order, against its `before`,
          reading this function's latest summary; then finish() again
stop:     when a round leaves the summary's returns unchanged
```

For `walk`:

| | `Pending(0)` = `walk(n-1)` | `Pending(1)` = `r()` | `walk` returns |
|---|---|---|---|
| round 0 | empty | empty | `done` |
| round 1 | `done` (round 0's returns) | `None` (calls `done`: a new edge) | `done` |
| stop: returns unchanged | | | |

The edge for `r()` comes from round 1, which ran with the final values. That is
where `walk > done` comes from.

`grow`: round 0 returns `done` (the base case), round 1 gives
`grow(n - 1) = done`, returns `done` again: stop. `rec`: round 0 returns
nothing, round 1 gives `rec(n - 1) =` nothing: stop. `rec(3)` is **empty**,
and `rec(3)()` has no callee value, so no node, which is exactly what Python
does (it never returns).

**Why round 0 starts from empty:** "no value yet" is the bottom of the order.
Values only grow from there, each round adding what the previous round's
summary made possible. Starting from `Unknown` would make every round's
answer at least `Unknown`, and `walk > done` would never appear.

**Why it stops:** each round can only add values, and for functions there are
finitely many: the project's functions, a few types, `Unknown`. Once a round
adds nothing, the next one would add nothing either. Step 06 adds a round cap
for the cases where "finitely many" is still too many.

**Why the body is not walked again:** every statement's effect on the env was
recorded on the first walk. Only the operations that waited on the unknown
value can change, and they were recorded with everything they need. Repeating
them is enough, and it is the whole cost of recursion: a few operations per
round, never a body.

### Decision 5: the solver answers a resolving context with its latest summary

During the fixpoint, `walk`'s slot is `Resolving`, and its `current` holds the
latest round's summary. When a repeated `Apply { ctx: walk }` calls `get`,
`get` returns `Got::Ready(current)`. That is how round n reads round n−1.

### Yours

- `Pending`, `DeferId`, `Deferred`, `DeferredOp`, `Walk.deferred`.
- Factor the second half of `eval_call` into `call_values(s, site, callees,
  args, env)`, so a repeated `Call` runs the same code as a live one.
- `get` with `Running { depth }`, `lowest`, and the three outcomes.
- `resolve_one(ctx, walk)`: round 0, rounds, stop. `finish` takes the resolved
  values of the pending ids and substitutes them.
- `walk_context` returns the `Walk`, unfinished. Finishing is the solver's
  decision now.
- The harness: `# expect inline:` and `# expect summary:` blocks.

<details><summary>Reference: this step's code</summary>

```rust
// src/interp/solver.rs: get, with pending values
pub fn get(&mut self, ctx: ContextId) -> Got<'db> {
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
        self.resolve_one(ctx, w)                 // the head of its own cycle
    } else {
        w.finish(self, &no_values(&w))           // alone (step 06 handles lowest < depth)
    };
    let result = Arc::new(result);
    self.slots[ctx].state = State::Ready;
    self.slots[ctx].result = Some(result.clone());
    Got::Ready(result)
}

fn reader_depends_on(&mut self, d: usize) {
    if let Some(&reader) = self.stack.last() {
        let slot = &mut self.slots[reader];
        slot.lowest = slot.lowest.min(d);
    }
}
```

```rust
// src/interp/defer.rs
impl<'db> Solver<'db> {
    pub fn resolve_one(&mut self, ctx: ContextId, mut w: Walk<'db>) -> FunctionResult<'db> {
        let mut resolved = no_values(&w);                       // round 0: every pending value empty
        let mut current = Arc::new(w.finish_round(self, &resolved));
        self.slots[ctx].state = State::Resolving;
        loop {
            self.slots[ctx].current = Some(current.clone());
            resolved = w.replay(self, &resolved);               // each operation once, in order
            let next = w.finish_round(self, &resolved);
            let changed = next.returns != current.returns;      // what a caller reads
            current = Arc::new(next);
            if !changed { break }
        }
        Arc::unwrap_or_clone(current)
    }
}

impl<'db> Walk<'db> {
    /// Each deferred operation once, in order. An operation may read the
    /// results of earlier ones in this same round.
    fn replay(&mut self, s: &mut Solver<'db>, last: &IndexVec<DeferId, Values<'db>>)
        -> IndexVec<DeferId, Values<'db>>
    {
        let mut now = last.clone();
        self.clear_deferred_edges();                            // a Call op's edge: this round's targets only
        for id in self.deferred.indices() {
            let d = self.deferred[id].clone();
            let v = match d.op {
                DeferredOp::Apply { ctx } => match s.get(ctx) {
                    Got::Ready(res) => self.resolve(&res.returns),
                    Got::Pending => Values::new(),             // step 06
                },
                DeferredOp::Call { site, callee, args } => {
                    let callee = substitute(&callee, &now);
                    let args = args.map(|v| substitute(v, &now));
                    let mut env = d.before.clone();
                    self.call_values(s, site, &callee, &args, &mut env)
                }
            };
            now[id] = v;
        }
        now
    }
}

/// Replace every Pending(id) by what it resolved to.
fn substitute<'db>(vals: &Values<'db>, now: &IndexVec<DeferId, Values<'db>>) -> Values<'db> {
    vals.iter().flat_map(|v| match *v {
        AbstractValue::Pending(id) => now[id].clone(),
        other => smallvec![other],
    }).collect()
}
```

`finish_round` is `finish` from step 02, with returns passed through
`substitute` first.

**Where a deferred `Call`'s edge goes:** when the walk records a `Call`
operation, it also pushes an empty `CallEdge { site, targets: [] }` into
`frame.calls`, at that point. So the edge keeps its place in evaluation order,
and the tree's children come out in source order. Each replay clears those
edges' targets and fills them again (`call_values` takes the index of the
edge to fill), so the edges in the final summary are the last round's.

**The harness, for mode-specific blocks:** each block remembers which mode it
is for, and a run checks only the blocks for its mode.

```rust
// src/interp/fixture.rs
pub struct Expect {
    pub entry: String,
    pub tree: String,
    pub mode: Option<Mode>,      // None: `# expect:`, every mode
}

// expectations(): the opening line picks the mode. The rest is unchanged.
let mode = match line.trim_end() {
    "# expect:" => None,
    "# expect inline:" => Some(Mode::Inline),
    "# expect summary:" => Some(Mode::Summary),
    _ => continue,
};
// … collect the tree lines and the entry as before …
out.push(Expect { entry, tree, mode });

// failures(): skip the blocks written for the other mode
for e in blocks.iter().filter(|e| e.mode.map_or(true, |m| m == opts.mode)) {
```

The loop that collects a block's lines already stops at the next line that
starts with `# expect`, so a tagged block ends the one before it. Both summary
runs, with and without reuse, read the `summary` blocks. The "no `# expect:`
block" assertion stays on the unfiltered list. Add a case to `reads_blocks`:

```rust
let b = expectations("# expect inline:\n# f\n#   f [recursive]\n# expect summary:\n# f\n");
assert_eq!((b[0].mode, b[1].mode), (Some(Mode::Inline), Some(Mode::Summary)));
assert_eq!(b[1].entry, "f");
```

</details>

---

## Run

```rust
#[test]
fn p02_05_pending() {
    check_both("02-function-summaries/python/pending.py");
}
```

Then the whole suite, `recursion.py` included.

---

## Where you are

```
AbstractValue += Pending(DeferId)
Walk  += deferred: [Deferred { op: Apply | Call, before: Env }]
Slot  { state: New | Running{depth} | Resolving | Ready, result, current, lowest }
get:  Running → Pending, and the reader's lowest drops to that depth
```

A function that calls itself is walked once, and the value of its recursive
call is found by repeating a handful of operations.

---

## Exercise

1. `def f(n): x = f(n - 1); return x`. Write the deferred operations, then
   round 0 and round 1. What does `f` return?
2. `def g(n): print(g(n - 1)); return done`. How many deferred operations?
3. Why does the fixpoint compare `returns` and not `calls` to decide when to
   stop?

<details><summary>Answers</summary>

1. One: `Apply { ctx: f }` → `Pending(0)`, and `x = Pending(0)`, returned.
   Round 0: `Pending(0)` empty, `f` returns nothing. Round 1: `Pending(0)` is
   round 0's returns, nothing, so `f` still returns nothing: stop. `f` returns
   empty, like `rec`. Python agrees: `f` never returns.
2. One: the `Apply` for `g(n - 1)`. `print` is not project code, so no key
   reads its argument, and the call happens immediately with a pending argument
   carried along.
3. A caller reads only the returns (and, in part 04, effects and new objects).
   The calls matter for the tree, but they are a consequence of the values: the
   last round runs with values that no longer change, so its edges are final.
   Comparing calls too would only add a round.

</details>

---

→ Next: [`06-recursion-groups.md`](06-recursion-groups.md)
