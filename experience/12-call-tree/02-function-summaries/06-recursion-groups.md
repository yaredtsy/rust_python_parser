# 02.06 — Recursion: groups

**You build:** mutual recursion. Functions that call each other in a cycle
form a **group**. Each member is walked once. A member that depends on a
context further down the stack **parks**, keeping its walk, until the oldest
member, the **head**, finishes. Then the whole group runs one fixpoint
together. Contexts first reached during the fixpoint **join** it, and a group
that reaches further down **merges** into a bigger one.
**Fixture:** `python/groups.py`
**Plan:** `plan/03-call-tree/12-recursion.md` § "Groups", § "The fixpoint",
§ "Why the entry point does not matter", `13-solver-and-limits.md`
§ "`get`", § "`resolve_group`"

---

## The fixture

```python
def f(n): return g(n)
def g(n):
    if n: return f(n - 1)
    return done

def a(n):
    if n: return b(n)
    return done
def b(n): return c(n)
def c(n): return a(n - 1)

def parse_expr(t):
    left = parse_atom(t)
    return combine(left, t)
def parse_atom(t):
    if t: return parse_expr(t)
    return num
def combine(left, t):
    if t:
        parse_expr(t)
        return binop
    return left

def main():
    f(1)()             # done
    b(2)()             # done
    parse_expr(1)()    # binop, num

def ping(n):
    r = pong(n)
    r()                # summary: done
    return done
def pong(n): return ping(n - 1)

def outer(n): return x(n)
def x(n):
    if n: return y(n)
    return outer
def y(n):
    h = x(n - 1)
    return h(n)        # outer [recursive], found in the fixpoint
```

## Predict

1. `main` calls `b`, which calls `c`, which calls `a`, which calls `b`. When
   `a`'s walk ends, `b` is still running. What can `a`'s summary say about its
   returns at that point?
2. In the parser, `combine`'s first call has `left` pending. When is
   `combine` walked for the first time?
3. In `y`, `h(n)` calls `outer` only once the fixpoint knows `h`. At that
   moment, `outer` is still running, below the group `{x, y}`. Why can't
   `{x, y}` finish on its own?

---

## Build

### Decision 1: a walk that depends on a context further down parks

Step 05's three cases for `lowest`, completed:

| `lowest` when the walk ends | the walk |
|---|---|
| `usize::MAX` | finishes alone |
| its own depth | is the **head** of a group: resolve the group now |
| below its own depth | is in a cycle whose head is further down: **park** |

Parking keeps the walk, with its deferred operations and their `before` envs,
in the slot, under the head:

```rust
pub enum State {
    New,
    Running { depth: usize },
    Parked { head: ContextId },   // walked, waiting for its group
    Resolving,                    // its group is in the fixpoint
    Ready,
}

pub struct Slot<'db> {
    pub state: State,
    pub result: Option<Arc<FunctionResult<'db>>>,
    pub current: Option<Arc<FunctionResult<'db>>>,
    pub walk: Option<Box<Walk<'db>>>,     // kept while Parked or Resolving
    pub lowest: usize,
    pub joins: Option<ContextId>,         // a resolving group this walk reached
}
```

A parked context answers `Pending` to its caller, and its caller inherits
`lowest`: whoever called into the cycle is in it too, up to the head. In the
`a → b → c` example, `a` parks under `b`, then `c` (which called `a`) parks
under `b`, then `b`'s own walk ends with `lowest` equal to its depth: `b` is
the head of `{a, c, b}`.

**This is Tarjan's algorithm** for strongly connected components, run on the
fly: each running context has a depth, each walk records the lowest depth it
reached, and a context whose lowest is its own depth closes a component.

**Why this is the reason for step 02's `s: &mut Solver` parameter:** a parked
walk lives in `Slot.walk`, inside the solver. If a walk held a borrow of the
solver, it couldn't be stored there.

### Decision 2: the group's fixpoint runs over every member, in walk-end order

```
round 0:  each member's summary with every pending value empty
round n:  for each member, in the order their walks ended:
            repeat its deferred operations against their `before`, reading the
            other members' LATEST summaries; finish
stop:     when no member's returns changed, and nobody joined
```

`a / b / c`:

| | `a` | `c` | `b` |
|---|---|---|---|
| round 0 | `done` | nothing | nothing |
| round 1 | `done` | `done` (from `a`) | `done` (from `c`) |
| round 2 | same | same | same: stop |

A member reading another member gets that member's `current`, the latest
summary. Each member's body was walked once.

### Decision 3: a context first reached during the fixpoint joins the group

In round 1 of the parser, `parse_expr`'s deferred `Call` keys
`combine(left = num)` for the first time. `combine` is walked right then, and
calls `parse_expr`, which is `Resolving`. A **new walk** reaching a resolving
member gets `Pending` (not the current summary), parks under the group's head
when it ends, and **joins** the fixpoint as a member from the next round.

```rust
/// A read of a parked or resolving context.
fn read_group_member(&mut self, ctx: ContextId) -> Got<'db> {
    let head = self.heads.find(self.slots[ctx].head());
    match self.slots[head].state {
        State::Running { depth } => { self.reader_depends_on(depth); Got::Pending }   // still being walked
        _ if self.reader_is_fixpoint() => Got::Ready(self.slots[ctx].current()),       // an operation reads a member
        _ => { self.reader_joins(head); Got::Pending }                                // a new walk reaches the group
    }
}
```

**Why `Pending` and not the current summary, for a new walk:** `combine`
calls back into the cycle, so it *is* part of it. If it took `parse_expr`'s
current summary, its own summary would be built from a value that is still
changing, and it would be `Ready` (reusable) with a wrong answer. Joining makes
it a member, recomputed every round like the others.

Round 2 makes `left` hold `num | binop`, and keys a second context,
`combine(left = num | binop)`, which joins too. Round 3 changes nothing: stop.
The final edges come from the last round, so only the second `combine` is in
the tree.

### Decision 4: a group that reaches further down merges into a bigger one

In `y`'s first fixpoint round, `h` becomes `outer`, and `h(n)` calls `outer`.
`outer` is `Running`, *below* the group `{x, y}`. So `{x, y}` is part of a
bigger cycle that includes `outer`, and its fixpoint can't be finished now:
`outer`'s summary isn't known.

The fixpoint stops, every member parks under `outer` with its walk and current
summary, and the group's head answers `Pending`. When `outer`'s own walk ends,
`outer` is the head of `{y, x, outer}`, and one fixpoint resolves all three.

Group heads live in a union-find (`heads`), so a member that parked under `x`
finds its real head, `outer`, after the merge.

### Decision 5: the answer doesn't depend on which member was entered first

Every call from one member to another returns a pending value, whichever
member was walked first. So each member's single walk is the same for every
entry point, and the fixpoint is the least solution of the same equations.
Values only grow, so the order of the rounds doesn't change where they end.

That is what makes a group member's final summary `Ready`: reusable, by any
later caller, from any direction.

**The test:** enter the parser at `parse_atom` instead of `parse_expr`, and
compare every member's returns. They must be equal as sets.

### Decision 6: a round cap, as a safety net

`max_rounds` (32). At the cap, one last round runs with `Unknown` added to
every deferred operation's result, and every member gets the `CYCLE` gap:

```rust
pub struct Status { pub gaps: Gaps }        // step 08 adds `cut`
bitflags! { pub struct Gaps: u8 { const CYCLE = 1; } }
```

A node whose summary has a gap is marked `[incomplete]` when rendered.

The cap depends only on the group's equations, which depend only on the keys
and the code. So a capped summary is still reusable: rule 3 holds.

For functions, values form a small finite set, and the cap is hard to reach.
It matters for objects (part 04) and closures nesting inside closures.

### Yours

- `Parked`, `Slot.walk`, `Slot.joins`, `heads` (a small union-find over
  `ContextId`), `Fixpoint { head, base, lowest }` on the solver.
- `get`'s full shape, `read_group_member`, `resolve_group` (generalising
  `resolve_one`), merging.
- `Status`, `Gaps::CYCLE`, `max_rounds`, and `[incomplete]` in `render`.
- The entry-independence test.

<details><summary>Reference: this step's code</summary>

`plan/03-call-tree/13-solver-and-limits.md` § "`get`" and § "`resolve_group`"
are the reference for this step, almost line for line. The pieces that are
new relative to step 05:

```rust
// after the walk, in get
let (lowest, joins) = (self.slots[ctx].lowest, self.slots[ctx].joins);
if lowest < depth {
    let head = self.heads.find(self.stack[lowest]);
    self.park(ctx, w, head);
    self.reader_depends_on(lowest);          // the caller is in the cycle too
    return Got::Pending;
}
if let Some(head) = joins {
    self.park(ctx, w, head);                 // first reached during a fixpoint: join
    return self.read_group_member(ctx);
}
if lowest == depth {
    if let Resolved::Merged(d) = self.resolve_group(ctx, w) {
        self.reader_depends_on(d);
        return Got::Pending;
    }
} else {
    self.finish_alone(ctx, w);
}
Got::Ready(self.slots[ctx].result())
```

```rust
fn resolve_group(&mut self, head: ContextId, head_walk: Walk<'db>) -> Resolved {
    self.park(head, head_walk, head);
    let mut members = self.take_parked(head);                 // walk-end order
    for &m in &members { self.slots[m].resolving_from_round0() }
    self.fixpoint = Some(Fixpoint { head, base: self.stack.len(), lowest: usize::MAX });

    let mut round = 0;
    loop {
        round += 1;
        let capped = round > self.opts.max_rounds;
        let mut changed = false;
        for &m in &members {
            let next = self.replay_member(m, capped);         // its operations, then finish
            if let Some(d) = self.fixpoint_lowest() {         // reached a context running further down
                self.park_all(&members, self.stack[d]);
                self.fixpoint = None;
                return Resolved::Merged(d);
            }
            changed |= self.slots[m].set_current(next);       // compares returns only, for now
        }
        let joined = self.take_parked(head);
        changed |= !joined.is_empty();
        for &j in &joined { self.slots[j].resolving_from_round0() }
        members.extend(joined);
        if !changed || capped { break }
    }
    self.fixpoint = None;
    self.settle(&members, if round > self.opts.max_rounds { Gaps::CYCLE } else { Gaps::empty() });
    Resolved::Done
}
```

`replay_member(m)` takes the member's walk out of its slot, runs step 05's
`replay` and `finish_round`, and puts the walk back. `reader_is_fixpoint()` is
true while `self.fixpoint` is set and the reader is the replay, not a new walk
on top of the stack. A dependency on a depth below `fixpoint.base` sets
`fixpoint.lowest`, which is what `fixpoint_lowest()` reads.

</details>

---

## Run

```rust
#[test]
fn p02_06_groups() {
    check_both("02-function-summaries/python/groups.py");
}

#[test]
fn p02_06_entry_independence() {
    // parse_expr's returns, as a set, entered from parse_expr and from parse_atom
    let from_expr = returns_of("02-function-summaries/python/groups.py", "main", "parse_expr");
    let from_atom = returns_of("02-function-summaries/python/groups.py", "parse_atom", "parse_expr");
    assert_eq!(from_expr, from_atom);
}
```

`returns_of(fixture, entry, func)` runs summary mode from `entry` and returns
the set of qualified names in `func`'s summary returns.

---

## Where you are

```
State = New | Running{depth} | Parked{head} | Resolving | Ready
Slot  { state, result, current, walk, lowest, joins }
Solver += heads (union-find), fixpoint
FunctionResult += status: Status { gaps }
```

Every recursion in plain functions is walked once per context, and its values
are found.

---

## Exercise

1. `def p(): q()` and `def q(): p()`, entered at `p`. Who parks, who is the
   head, and how many rounds does the fixpoint take?
2. In the parser, why is `combine(left = num)` not in the final tree, although
   it was walked?
3. Run `ping_entry` with `max_rounds = 1`. Which nodes get `[incomplete]`, and
   does the `done` edge under `ping` survive?

<details><summary>Answers</summary>

1. `q` parks under `p`, and `p` is the head of `{q, p}`. Neither has a
   `return`: both fall off the end and return `None`, so nothing pending
   reaches their returns. Round 1 repeats the two `Apply` operations, changes
   nothing, and the fixpoint stops.
2. The final edges come from the last round, and in the last round
   `parse_expr`'s deferred `Call` keyed `combine(left = num | binop)`. The
   first `combine` context is not reachable from the entry through the final
   edges, so the graph leaves it out and the tree never shows it.
3. `ping` and `pong` are both marked `[incomplete]`: the group needs a second
   round to see that nothing changed, the cap stops it after one, and one last
   round runs with `Unknown` added. The `done` edge survives: round 1 already
   made `r` hold `done`, and adding `Unknown` only adds a value with no edge.

</details>

---

→ Next: [`07-closures-across-a-summary.md`](07-closures-across-a-summary.md)
