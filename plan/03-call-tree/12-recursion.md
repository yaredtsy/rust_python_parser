# 03.12 — Recursion

Each body is walked once per context, recursion included. A recursive call is
recorded, not entered. The values that depend on it are found afterwards, by
repeating only the operations that waited on it until nothing changes.

The lifecycle of a summary in a cycle:

```
walk each body once
  → record the calls, returns and writes it can know, and the operations that wait
  → the cycle's walks are done: one group, one head
  → repeat the waiting operations over the whole group until no summary changes
  → finalise every member's summary
```

The solver code for all of this is in [`13`](13-solver-and-limits.md).

---

## Pending values

A call to a context that is **running** (its walk is on the stack), **parked**
(walked, waiting for its group) or **resolving** (its group is in the fixpoint,
and the reader is a new walk) gets `Got::Pending`. The edge `Target::Call(ctx)`
is recorded at once. The call's value is `AbstractValue::Pending(id)`, where `id`
indexes the walk's deferred operations.

A pending value is walk-local, like an origin, and never enters a summary. It
can be assigned to a local, stored in an attribute, joined at a branch, captured
by a closure or returned, like any value. Only a use that needs to know what the
value is becomes a deferred operation.

## Deferred operations

```rust
struct Deferred<'db> {
    id:     DeferId,
    op:     DeferredOp<'db>,
    before: Env<'db>,                         // the state at that point: an O(1) clone
}

enum DeferredOp<'db> {
    /// A call to a running, parked or resolving context. Resolves to its
    /// translated returns, and applies its effects and objects.
    Apply  { ctx: ContextId, at: (SiteId, u8, u8), combo: Combo<'db>,
             callee: AbstractValue<'db>, strong_ok: bool },
    /// `p.name` where `p` is pending.
    Member { base: Values<'db>, name: Name, site: SiteId },
    /// A call whose callee, or something its key would read, is pending.
    /// Keyed during the fixpoint.
    Call   { site: SiteId, callee: Values<'db>, args: EvaluatedArgs<'db> },
    /// `p.name = v` where `p` is pending.
    Write  { base: Values<'db>, name: Name, value: Values<'db> },
}
```

| use of a pending value `p` | becomes |
|---|---|
| a call to a running, parked or resolving context | `Apply` |
| `p.name` | `Member`, and its value is a new pending value |
| `p(...)`, or `f(p)` where `f`'s key would read `p` | `Call`, and its value is a new pending value |
| `p.name = v` | `Write` |
| anything else | carried along as a value |

An operation's inputs are concrete values, or the results of earlier operations
in the same walk. A walk with no deferred operations finishes once and never
takes part in a group.

---

## Groups

Pending values come only from recursion. A walk that reads a pending value
depends on a context that is still running. Every walk that depends on a running
context, directly or through another parked walk, belongs to that context's
cycle: its **group**. The oldest running context in the group is the **head**.

The solver finds the head the way Tarjan's algorithm finds a strongly connected
component. Each running context has a stack depth. A walk records `lowest`, the
smallest depth of a running context it read, directly or through a parked
context's head. When a walk ends:

| `lowest` | the walk |
|---|---|
| below its own depth | is in a cycle whose head is further down. It **parks** under that head, keeping its walk. Its caller inherits `lowest` |
| equal to its own depth | is the head. Its group is resolved now |
| none | is not in a cycle. It finishes alone |

Every call from one member of a group to another returns a pending value: the
callee is running or parked already, or it is walked now and parks itself,
because it is in the same cycle.

Two more cases, both found during the fixpoint:

- **Joining.** A call made during the fixpoint can walk a context for the first
  time. If that walk reaches a member of the group, the new context calls back
  into the cycle, so it is part of it. It gets `Pending` for that call, parks
  under the group's head when its walk ends, and joins the fixpoint as a member.
  Its body is walked once, like every other member's.
- **Merging.** If a walk or an operation during the fixpoint reaches a context
  still running further down the stack, this group is part of a bigger cycle.
  It can't finish now. The fixpoint stops, every member parks under the outer
  head with its walk and its current summary, and the outer head resolves
  everything when its own walk ends. Group heads are kept in a union-find, so a
  member parked under a head that later merged still finds the current head.

---

## The fixpoint

Every member starts from its **round-0 summary**: `finish()` on its walk with
every pending value empty. Then, in rounds:

1. For each member, in the order their walks ended: start again from its walk
   (exit envs, returns, calls, written set), run its deferred operations once,
   in program order, against their stored `before` states, then `finish()`.
   - `Apply` and `Call` that reach a member read that member's **latest**
     summary, through the same resolve and apply as [`10`](10-one-call.md).
   - `Member`, `Call` and `Write` run the normal `member`, `call_one` and
     `bind_attribute` on the values resolved so far. Every `Pending(id)` an
     operation reads, in its own values or in `before`'s attributes, is
     replaced by the values resolved so far.
   - A `Call` that keys a context outside the group gets it the normal way:
     reused if `Ready`, walked if new. A new walk may join the group.
   - An operation's writes, and the effects of the calls it makes, are joined
     into the exit state. They are never strong.
2. Add the contexts that joined during the round.
3. Stop when no member's returns, effects or objects changed and nobody joined.

Every member then settles with its last summary. Its call edges come from the
last round, so a context only an early round called is not in the final edges,
and the graph, built by reachability from the entries, leaves it out
([`14`](14-output.md)).

The fixpoint repeats operations, never statements. No body is walked again, and
a repeated `Call` with an unchanged key is a lookup.

### Identity does not go around a cycle

Inside a walk, an object returned by a member keeps its identity and its
attributes. But in a member's own summary, an object that came from a member's
summary (its own included) is exported as `Opaque(class)`, and a closure that
did as `Function(func)` with no captured values.

Without this, each round would add a new object: last round's objects, coming
back through the recursive call with new origins. The fixpoint would never end.
The price: an object built at a deeper level of the recursion keeps its class,
not its attributes, once it leaves the group.

### Why it stops

Pending values start empty and only grow. A call that waits on a value
contributes nothing until the value arrives ([`10`](10-one-call.md#a-call-with-no-value)).
Every value is bounded: fingerprints by `fp_depth`, paths by `max_path`, unions
by the fan-out caps, objects by the identity rule above, and closures nested in
closures by `fp_depth`, in keys and in summaries alike. (A recursive function
that returns a closure capturing its own recursive call, a middleware chain,
otherwise nests one level deeper every round: `g[a]`, `g[g[a], a]`, …) So the
summaries reach a point where no round changes them. Fan-out caps that turn a
union into `Unknown` act as widening: once `Unknown`, a value stays `Unknown`.

Each round **replaces** every deferred operation's value with this round's.
The operations are monotone (more values in, more values out; `Unknown` says
more, not less), so the new value already contains the old one. Rejected:
union with the previous round, which keeps every stale version of a closure
or an object next to the new one, each its own key and walk in the next
round; and a join that merges versions of "the same" closure, which gives
replace's answer when operations are monotone, and needs a cross-round
identity rule that silently mixes captures when it is wrong. Summaries are
compared **as sets** (`same_svals`, recursively through closures' captures):
order is not meaning, and `Type` has no order to sort by. A call is deferred
only if its key would read a pending value, which means the callee or a
relevant slot ([`08`](08-keys.md)): a pending value that lands only in
irrelevant parameters is keyed and called at once.

`max_rounds` is the safety net, and a counter (`Stats.rounds_capped`): on a
fixture, reaching it means some operation shrank a value. At the cap, one
last round runs with `Unknown` added to every deferred operation's result,
and every member gets the `CYCLE` gap. The cap depends only on the group's equations, which depend only on the
keys, so a capped summary is still reusable.

### Why the entry point does not matter

Inside a group, every call from one member to another returns a pending value,
whichever member was entered first. A context first reached during the fixpoint
sees the same thing: the member it calls is resolving, so it gets `Pending`. So
each member's walk is the same for every entry point. The fixpoint is the least
solution of the same equations, and with values that only grow, the order of the
rounds doesn't change it. So `Finished` and `CYCLE` summaries are safe to reuse.
The no-reuse check tests it on every fixture
([`15`](15-checks-and-build-order.md#no-reuse-mode)).

### Status after the fixpoint

- **Converged**: every member is `Finished`, apart from gaps of its own such as
  `DEEP_WRITE`.
- **Round cap**: every member has the `CYCLE` gap. Reusable.
- **A member read an `Incomplete(Cut)` result**: every member is
  `Incomplete(Cut)`, because the result depended on how the group was reached.
  Not stored; walked again when reached ([`13`](13-solver-and-limits.md#truncation)).

Recursion itself never causes a cut.

---

## Worked examples

### Two hops around a cycle

```python
def a(n):
    if n:
        return b(n)
    return Handle()

def b(n):
    return c(n)

def c(n):
    return a(n - 1)

def main():
    b(2).close()          # Python: Handle.close
```

`main` walks `b`, which walks `c`, which walks `a`, which calls `b`: running, so
pending. `a` and `c` park under `b`, and `b` is the head.

| | `a` | `c` | `b` |
|---|---|---|---|
| round 0 | `Handle` | nothing | nothing |
| round 1 | `Handle` | `Handle` (from `a`) | `Handle` (from `c`) |
| round 2 | same | same | same: stop |

`b` returns `Handle`, so `main` finds `Handle.close`. Every member is
`Finished`, with no `Unknown`.

### A parser: a context that joins

```python
def parse_expr(t):
    left = parse_atom(t)
    return combine(left, t)

def parse_atom(t):
    if t.peek() == "(":
        return parse_expr(t)        # cycle: parse_expr → parse_atom → parse_expr
    return Num()

def combine(left, t):
    if t.more():
        return BinOp(left, parse_expr(t))   # calls back into the cycle
    return left
```

1. `parse_expr` is walked. It calls `parse_atom`.
2. `parse_atom` is walked. Its call to `parse_expr` is pending. It parks.
3. In `parse_expr`, `left` is pending, so `combine(left, t)` can't be keyed. It
   becomes a deferred `Call`. `parse_expr` is the head.
4. Round 1: `parse_atom` returns `Num`. `parse_expr`'s `Call` keys
   `combine(left = Num)`, walked for the first time. It calls `parse_expr`,
   which is resolving, so `combine` gets `Pending`, parks, and joins.
5. Later rounds: `combine` returns `BinOp` and `Num`. `parse_expr` returns the
   same. `parse_atom` returns `Num` and `BinOp`. So `left` is `Num | BinOp`, and
   CPA keys a second context, `combine(left = BinOp)`, which joins too.
6. A round changes nothing: stop.

All four contexts are walked once and saved. `parse_expr` returns `Num` and
`BinOp`, with no `Unknown`, whichever function was called first.

Step 5 shows the cost: a deferred call's arguments can grow between rounds, and
each new shape is a new context. The caps bound it. `Stats` counts contexts
created per round, so the S5 benchmark shows how often it happens.

### A visitor: three steps

```python
class Node:
    left: "Node"
    def accept(self, v: "Visitor"): v.visit(self)

class Visitor:
    def visit(self, node: Node):
        self.visit_node(node)
    def visit_node(self, node: Node):
        self.visit_children(node)
    def visit_children(self, node: Node):
        node.left.accept(self)          # back to accept: a four-member cycle
        self.found = Report()

def main(root: Node):
    v = Visitor()
    root.accept(v)
    v.found.emit()                      # Report.emit
```

`root` comes from its annotation. `node.left` is named by its path, with class
`Node` from the declaration, so the inner `accept` has the same key as the outer
one: `accept` → `visit` → `visit_node` → `visit_children` → `accept` is a cycle
of four, headed by `accept`. `visit_children`'s write to `self.found` reaches
`main` through the group's summaries, weak because it comes through deferred
operations, and as `Opaque(Report)` because the object went around the cycle.
`main` still finds `Report.emit`. Each method is walked once.

(Iterating `node.children` instead would not form a cycle: containers are not
modelled, so the loop variable is `Unknown` and the inner call has no target.)

### A recursive call's value, used in the same body

```python
def walk(node):
    r = walk(node.first())   # pending: Apply
    r.close()                # Member, then Call
    return Handle()
```

`walk`'s round-0 summary already returns `Handle`. Round 1 resolves `r` to
`Handle`, so `r.close()` keys `Handle.close`, and `walk > Handle.close` is in
the final edges. `walk` is `Finished`.

---

## Known limit: effects inside a cycle reach only the exit state

```python
class Tree:
    def fill(self, n, h):
        if n:
            self.fill(n - 1, h)
            self.h.process()        # Python: h's process, set by the inner call
        self.h = h

def main():
    Tree().fill(2, JsonHandler())   # Python: JsonHandler.process. v1: no edge
```

The inner `fill` is pending when `self.h.process()` is walked. `self` is not
pending, so the read is concrete and sees no `h`. The inner call's effect
arrives in the fixpoint and is joined into the exit state, after the statement
that needed it. The fixpoint repeats operations, not statements, so this stays
a gap. It is pinned in [`15`](15-checks-and-build-order.md#known-limits).

---

## Fixtures

```python
# 1. a recursive call is recorded, not entered
def walk(n):
    walk(n.child)
def main():
    walk(root)
# main > walk > walk [Recursive], no children. walk's body is walked once.

# 2. a value cycle
def grow(n):
    if n:
        return grow(n - 1)
    return JsonHandler()
def main():
    grow(3).process()        # JsonHandler.process. grow is Finished, returns only JsonHandler.

# 3. recursion with no base value
def rec(n): return rec(n - 1)
def main(): rec(3).go()      # rec returns nothing; main > rec, and the .go() call has no target

# 4. the recursive call's value in the same body: walk > Handle.close (above)
# 5. two hops: the a / b / c example, Handle.close, every member Finished
# 6. the parser: four contexts, each walked once, no Unknown, the same from either entry
# 7. the visitor: main > Report.emit
# 8. mutual recursion: f → g → f
def f(): g()
def g(): f()
def main(): f()              # main > f > g > f [Recursive]
# 9. entering the parser at combine gives the same summaries as entering at parse_expr
#    (run with --no-reuse and compare)
# 10. the Tree.fill known limit: no JsonHandler.process edge
```

---

→ Next: [`13-solver-and-limits.md`](13-solver-and-limits.md)
