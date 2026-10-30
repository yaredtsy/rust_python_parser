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
    parse_expr(1)()    # num, binop

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

# The parser again, with helpers first reached inside its fixpoint:
def walk_list(xs):                 # its own cycle: resolves inside the parser's fixpoint
    if xs: return walk_list(xs)
    return done
def walk_back(xs):                 # its own cycle, whose REPLAY reaches the parser
    if xs:
        f = walk_back(xs)
        r = f(xs)                  # f is nested_expr: only known in walk_back's fixpoint
        r()                        # summary: num, binop
        return f
    return nested_expr
def nested_combine(left, t):       # nested_expr / nested_atom: as parse_expr / parse_atom
    if t:
        walk_list(t); walk_back(t); nested_expr(t)
        return binop
    return left

def up_combine(left, t):           # up_expr / up_atom: as parse_expr / parse_atom
    if t:
        helper(t)
        return binop
    return left
def helper(t):
    if t: return helper2(t)
    return num
def helper2(t):
    r = up_expr(t)                 # reaches the resolving group: helper2 joins
    r()                            # summary: num, binop
    return helper(t)               # helper is still running: helper2 parks under it
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
        _ if self.reader_is_fixpoint_of(head) => Got::Ready(self.slots[ctx].current()), // its group's operation reads it
        _ => { self.reader_joins(head); Got::Pending }                                 // a new walk reaches the group
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

**The first `combine` stays a member anyway.** No call reaches
`combine(left = num)` in the last round, but its summary is updated every
round like the others, so when the group settles it is `Ready` and correct: a
later call that keys `combine(left = num)` reuses it safely. Dropping it from
the group early would save its replays, but leave it with a summary from a
middle round, which must then not be `Ready`: it would have to go back to
`New`, to be walked again if reached. Keep it, unless profiling says the
replays cost something.

Three details make joining hold up beyond this one example.

**A joining context needs a current summary at once.** `get` parks the new
context and answers the replay that called it with its `current`. It hasn't
been through `resolving_from_round0` yet (that happens after the round), so
`park` computes the round-0 summary itself (every pending value empty) when
the slot has none.

**`joins` is passed up, like `lowest`.** In `helper2`, one walk reaches the
resolving group (`up_expr`) *and* a context still running above it
(`helper`). It parks under `helper`, and `helper` inherits its `lowest`. If
`helper` didn't inherit `joins` too, `helper` would end as the head of its
own small cycle and resolve it on the spot, reading `up_expr`'s unfinished
summary: `r()` would find `num` only, and `helper`, `helper2` would be
`Ready` with that wrong answer. With `joins` passed up, `helper` joins the
parser's group, and so does `helper2`: `park` unions `helper` into the
group's head, and `helper2`, parked under `helper`, follows it. (The next
rule would also catch this case, one step later: `helper`'s own fixpoint
replays `helper2`'s read of `up_expr` and joins then. Passing `joins` up gets
there directly, without starting a fixpoint that can't finish. So `up_entry`
fails only if both rules are missing.)

**A fixpoint can start inside another one.** `walk_list` is first reached
inside the parser's fixpoint, and it is the head of its own cycle. Its group
resolves right there: a second fixpoint, inside one operation of the first.
Fixpoints nest like calls, so `resolve_group` saves the outer `fixpoint` and
restores it on every exit. If it overwrote it, the outer fixpoint would lose
itself: its replay's reads of members would look like new walks and join the
wrong reader, and its merges would go unnoticed.

Nesting has a second consequence. `walk_back`'s *walk* never touches the
parser, but its *replay* does: `f` is only known in its own fixpoint, and
`f(xs)` calls `nested_expr`. That read is the inner fixpoint reaching the
outer group, which is a new reader, not one of the group's own operations. So
"the fixpoint reads a member" must mean a member of **that fixpoint's own
group** (`reader_is_fixpoint_of(head)`). Anything else joins: the inner
group stops, parks under the outer head, and becomes members of the outer
fixpoint, exactly like a new walk would. If it took the outer group's
`current` instead, `walk_back` would be `Ready` with `r()` finding nothing.

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

**Why the order of the rounds doesn't change where they end.** Values never
go above that least solution. Everything starts empty, every operation is
monotone, and an unfinished summary is at most what it will end as. Values
can go *down* for a round: a member that joins late restarts from its round
0, and a nested group that joins brings values from its own rounds. So
"values only grow" isn't quite true. What holds instead: after n more rounds,
every value is at least what n rounds from empty would give, and at most the
solution. The rounds are squeezed onto it, in any order. A round that changes
nothing is a solution, and a solution no bigger than the least one is the
least one. That is also why each round can replace the last round's values
(step 05): a value that dropped is recomputed, not lost. Order changes how
many rounds there are, not the answer.

That is what makes a group member's final summary `Ready`: reusable, by any
later caller, from any direction.

**The test:** enter the parser at `parse_atom` instead of `parse_expr`, and
compare every member's returns. They must be equal as sets.

### Decision 6: what a capped summary means

Step 05 stops at `max_rounds` (32) and counts it, nothing more: that summary
is missing whatever later rounds would have added. Now the cap covers a
group, and makes the result sound. At the cap, one last round runs with
`Unknown` added to every deferred operation's result, and every member gets
the `CYCLE` gap:

```rust
pub struct Status { pub gaps: Gaps }        // step 08 adds `cut`
bitflags! { pub struct Gaps: u8 { const CYCLE = 1; } }
```

A node whose summary has a gap is marked `[incomplete]` when rendered.

The cap depends only on the group's equations, which depend only on the keys
and the code. So a capped summary is still reusable: rule 3 holds.

For functions, values form a small finite set, and closures nest at most
`fp_depth` deep (step 07), so the cap is hard to reach. Reaching it on a
fixture still means an operation shrank a value (step 05): `rounds_capped`
stays 0 on every fixture that isn't about the cap.

### One invariant, checked on every fixture

The three details in decision 3 guard against one failure: a context becomes
`Ready` with a summary built from something unfinished. Two debug assertions
state that directly. Every fixture then checks it, not only the blocks
written for each case:

```rust
// finish_alone: a walk that finishes alone never waited on anything
debug_assert!(w.deferred.is_empty(), "{ctx:?} finishes alone with deferred operations");

// settle, before the walks are dropped: every member waited only on its own group
let group = self.heads.find(members[0]);
for &m in members {
    for d in &self.slots[m].walk().deferred {
        if let DeferredOp::Apply { ctx } = d.op {
            debug_assert_eq!(self.heads.find(ctx), group, "{m:?} waited on {ctx:?}, outside its group");
        }
    }
}
```

**Why not "in its group, or `Ready`":** an `Apply` is recorded only when
`get` answered `Pending`, and every `Pending` answer puts the reader into the
target's group: it parks under it, joins it, or merges with it. So at settle
time, a target outside the group means the reader waited on something it
never became part of, whatever that target's state is by now. The
`finish_alone` assertion is the same rule for a group of one: no `Pending`
answer means no pending value, so no deferred operation at all.

### Yours

- `Parked`, `Slot.walk`, `Slot.joins`, `heads` (a small union-find over
  `ContextId`), `Fixpoint { head, base, lowest }` on the solver.
- `get`'s full shape, `read_group_member` with `reader_is_fixpoint_of`,
  `park` (round-0 current, union into the head), `resolve_group`
  (generalising `resolve_one`, saving the outer fixpoint), merging, and
  joining from a nested fixpoint (`Resolved::Joined`).
- `Status`, `Gaps::CYCLE`, the last round at `max_rounds`, and `[incomplete]`
  in `render`.
- The two debug assertions.
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
    if let Some(j) = joins {
        self.reader_joins(j);                // and reaches the resolving group too
    }
    return Got::Pending;
}
if let Some(head) = joins {
    self.park(ctx, w, head);                 // first reached during a fixpoint: join
    return self.read_group_member(ctx);
}
if lowest == depth {
    match self.resolve_group(ctx, w) {
        Resolved::Done => {}
        Resolved::Merged(d) => { self.reader_depends_on(d); return Got::Pending }
        Resolved::Joined(_) => return self.read_group_member(ctx),   // its replay reached an outer fixpoint
    }
} else {
    self.finish_alone(ctx, w);
}
Got::Ready(self.slots[ctx].result())
```

```rust
/// A read of a parked or resolving context.
fn read_group_member(&mut self, ctx: ContextId) -> Got<'db> {
    let head = self.heads.find(self.slots[ctx].head());
    match self.slots[head].state {
        State::Running { depth } => { self.reader_depends_on(depth); Got::Pending }   // still being walked
        _ if self.reader_is_fixpoint_of(head) => Got::Ready(self.slots[ctx].current()),   // its own group's operation
        _ => { self.reader_joins(head); Got::Pending }   // a new walk, or another group's fixpoint
    }
}

/// The reader is the replay of `head`'s own fixpoint, not a walk on top of it
/// and not a fixpoint nested inside it.
fn reader_is_fixpoint_of(&mut self, head: ContextId) -> bool {
    let Some(f) = self.fixpoint else { return false };
    self.stack.len() == f.base && self.heads.find(f.head) == head
}

/// Keep a walked context under `head` until its group resolves.
fn park(&mut self, ctx: ContextId, w: Walk<'db>, head: ContextId) {
    if self.slots[ctx].current.is_none() {
        // A joining context is read at once: it needs its round-0 summary now.
        let round0 = w.finish_round(self, &no_values(&w));
        self.slots[ctx].current = Some(Arc::new(round0));
    }
    self.heads.union_into(ctx, head);        // whatever is parked under ctx follows it
    self.slots[ctx].walk = Some(Box::new(w));
    self.slots[ctx].state = State::Parked { head };
}
```

`reader_joins(head)` updates the reader: the slot on top of the stack, or,
when the stack is at a fixpoint's `base`, that fixpoint's `joins`.
`Fixpoint` gets `joins: Option<ContextId>` next to `lowest`, and derives
`Clone, Copy` (all its fields are), which `reader_is_fixpoint_of` relies on.

```rust
fn resolve_group(&mut self, head: ContextId, head_walk: Walk<'db>) -> Resolved {
    self.park(head, head_walk, head);
    let mut members = self.take_parked(head);                 // walk-end order
    for &m in &members { self.slots[m].resolving_from_round0() }
    // Fixpoints nest: this one may run inside an operation of another. Keep it.
    let outer = self.fixpoint.replace(Fixpoint {
        head, base: self.stack.len(), lowest: usize::MAX, joins: None,
    });

    let mut round = 0;
    let resolved = 'rounds: loop {
        round += 1;
        let capped = round > self.opts.max_rounds;
        let mut changed = false;
        for &m in &members {
            let next = self.replay_member(m, capped);         // its operations, then finish
            if let Some(d) = self.fixpoint_lowest() {         // reached a context running further down
                self.park_all(&members, self.stack[d]);
                break 'rounds Resolved::Merged(d);
            }
            if let Some(g) = self.fixpoint_joins() {          // reached an outer group in its fixpoint
                self.park_all(&members, g);
                break 'rounds Resolved::Joined(g);
            }
            changed |= self.slots[m].set_current(next);       // returns, with same_svals (step 05)
        }
        let joined = self.take_parked(head);
        changed |= !joined.is_empty();
        for &j in &joined { self.slots[j].resolving_from_round0() }
        members.extend(joined);
        if !changed || capped {
            self.settle(&members, if capped { Gaps::CYCLE } else { Gaps::empty() });
            break Resolved::Done;
        }
    };
    self.fixpoint = outer;                                    // on every exit
    resolved
}
```

`replay_member(m)` takes the member's walk out of its slot, runs step 05's
`replay` and `finish_round`, and puts the walk back. A dependency on a depth
below `fixpoint.base` sets `fixpoint.lowest`, which is what
`fixpoint_lowest()` reads; `reader_joins` on the fixpoint sets
`fixpoint.joins`, which is what `fixpoint_joins()` reads.

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
    // The same contexts, solved from two entries: the group's members are
    // walked in a different order, and must settle on the same returns.
    let f = "02-function-summaries/python/groups.py";
    let from_expr = returns_of(f, "parse_expr", "parse_expr");
    assert_eq!(from_expr, BTreeSet::from(["groups.binop", "groups.num"].map(String::from)));
    for func in ["parse_expr", "parse_atom", "combine"] {
        assert_eq!(returns_of(f, "parse_expr", func), returns_of(f, "parse_atom", func), "{func}");
    }
}
```

`returns_of` runs summary mode from `entry` with `solve` (step 03) and reads
the summaries of every context of `func`:

```rust
// src/interp/fixture.rs
/// The functions `func` can return (qualified names), over all its contexts,
/// solved in summary mode from `entry`.
pub fn returns_of(fixture: &str, entry: &str, func: &str) -> BTreeSet<String> {
    let path = std::fs::canonicalize(format!("experience/12-call-tree/{fixture}")).unwrap();
    let root = path.parent().unwrap();
    let db = open_project(SystemPath::from_std_path(root).unwrap()).unwrap();
    let (file, _) = open(&db, SystemPath::from_std_path(&path).unwrap()).unwrap();
    let module = path.file_stem().unwrap().to_str().unwrap();

    let (s, _) = solve(&db, file, entry, &summary()).unwrap();
    let want = format!("{module}.{func}");
    let mut out = BTreeSet::new();
    for (ctx, c) in s.contexts.iter() {
        if s.sem.qname(c.func) != want { continue }
        let Some(res) = &s.slots[ctx].result else { continue };
        for v in &res.returns {
            if let SVal::Plain(PlainVal::Function(d)) = v {
                out.insert(s.sem.qname(*d));
            }
        }
    }
    out
}
```

`Interner` needs an iterator for this:
`pub fn iter(&self) -> impl Iterator<Item = (I, &K)> { self.items.iter_enumerated() }`.

**Why enter at `parse_expr` and `parse_atom`, not at `main`:** the test must
compare the same keys. An entry is called with no arguments, so `t` is
`Unknown` in both runs, and both reach `parse_expr(Unknown)`,
`parse_atom(Unknown)` and the same `combine` contexts. `t` is relevant (it is
passed to `parse_atom` and `combine`, and a call argument is a sink), so from
`main`, `parse_expr(1)` keys `parse_expr(int)` instead: a different context,
whose returns happen to be equal, which says nothing about entry order.

**Why all contexts of `func`:** `combine` has two (`left = num`, and
`left = num | binop`), and both are part of the answer. If the sets differ,
print each run's contexts with their keys: a key that differs is step 04's
business, a summary that differs under the same key is this step's.

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
