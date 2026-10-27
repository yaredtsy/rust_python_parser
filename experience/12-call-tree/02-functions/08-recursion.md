# 02.08 — Recursion

The hardest part of the design, and the one real code stresses most: parsers,
visitors, tree walkers and interpreters are full of cycles. The rule that makes
it work:

> Each body is walked **once per context**, recursion included. A recursive call
> is recorded, not entered. The values that depend on it are found afterwards, by
> repeating **only the operations that waited on it** until nothing changes.

**Fixtures:** `python/recursion.py`, `python/parser.py`. **Reading:**
[`plan/03-call-tree/12-recursion.md`](../../../plan/03-call-tree/12-recursion.md),
and [`13-solver-and-limits.md`](../../../plan/03-call-tree/13-solver-and-limits.md) § "`get`".

---

## A recursive call is recorded, not entered

```python
def f(): f()
def use_direct(): f()
```

`use_direct`'s walk asks for `f {}`. `f` is new, so the solver pushes it on its
stack and walks it. Inside, the call `f()` keys `f {}` again. That context is
`Running`: its walk is on the stack. So `get` answers **`Pending`**:

- the edge `f → f {}` is recorded at once;
- `f`'s body is **not** walked again;
- the call's value is a **pending value**, `Pending(id)`.

The tree renders the inner call as a marked leaf:

```
use_direct
└── f
    └── f   [Recursive]
```

The Jedi driver drops that inner call; pylspt shows it, marked. The mark is
decided at render time by qname on the path, the same ancestor rule as
`call_resolver.py:157`.

Mutual recursion is the same, one hop longer: `g → h → g` gives
`use_mutual > g > h > g [Recursive]`.

## Pending values and deferred operations

A pending value is a placeholder, local to the walk. It can be assigned, stored,
joined, captured or returned like any value. Only a use that needs to know what
it *is* becomes a **deferred operation**:

```python
def walk(node):
    r = walk(node.first())   # Apply: r = Pending(0)
    r.close()                # Member: Pending(0).close → Pending(1); Call: Pending(1)() → Pending(2)
    return Handle()
```

| use of a pending value | becomes |
|---|---|
| a call into a running context | `Apply` (the recursive call itself) |
| `p.name` | `Member` |
| `p(...)`, or `f(p)` when `f`'s key would read `p` | `Call` |
| `p.name = v` | `Write` |
| anything else | carried along |

Each deferred operation stores the env at that point (an O(1) clone), so it can
run later exactly where it was.

## Groups

Every walk that depends on a running context belongs to its cycle: a **group**.
The oldest running context in it is the **head**. The solver finds it with stack
depths, like Tarjan's algorithm: each walk records the smallest depth of a
running context it read (`lowest`). When a walk ends:

- `lowest` below its own depth: it is in a cycle headed further down. It
  **parks**, keeping its walk and its deferred operations.
- `lowest` equal to its own depth: it is the head. Its group is resolved now.
- no `lowest`: no cycle. It finishes on its own.

## The fixpoint

Each member starts from its **round-0 summary**: its walk's summary with every
pending value empty. Then, in rounds, each member replays its deferred
operations in order, reading the other members' **latest** summaries, and
finishes again. When no member's returns, effects or objects change, stop.

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

`main` walks `b`, `b` walks `c`, `c` walks `a`, and `a`'s call to `b` is
pending. `a` and `c` park; `b` is the head.

| | `a` | `c` | `b` |
|---|---|---|---|
| round 0 | `Handle` | nothing | nothing |
| round 1 | `Handle` | `Handle` (from `a`) | `Handle` (from `c`) |
| round 2 | same | same | same: stop |

`b` returns `Handle`, so `main` finds `Handle.close`. `Handle` had to travel two
hops around the cycle; repeating the operations is what carries it.

What is repeated: only the deferred operations. Each is "read a stored summary,
translate it, add it in". No statement is walked twice.

## A context that joins: the parser

```python
def parse_expr(t):
    left = parse_atom(t)
    return combine(left, t)

def parse_atom(t):
    if t.peek() == "(":
        return parse_expr(t)
    return Num()

def combine(left, t):
    if t.more():
        return BinOp(left, parse_expr(t))
    return left
```

1. `parse_expr` is walked; it walks `parse_atom`, whose call back to
   `parse_expr` is pending. `parse_atom` parks.
2. In `parse_expr`, `left` is pending, so `combine(left, t)` can't be keyed
   yet. It becomes a deferred `Call`. `parse_expr` is the head.
3. Round 1: `parse_atom` returns `Num`. `parse_expr`'s `Call` keys
   `combine {left: Num}` and walks it for the first time. `combine` calls
   `parse_expr`, which is resolving, so `combine` gets `Pending`: it calls back
   into the cycle, so it **joins** the group.
4. Later rounds: `combine` returns `BinOp` and `Num`. `left` grows to
   `Num | BinOp`, which keys a second context, `combine {left: BinOp}`, which
   joins too.
5. A round changes nothing: stop.

Four contexts, each walked once, all saved and reused. `parse_expr` returns
`Num` and `BinOp`, and no `Unknown`.

Step 4 is the cost: a deferred call's arguments can grow between rounds, and
each new shape is a new context. The caps bound it, and `Stats` counts contexts
created per round.

One more case: if, during the fixpoint, a walk reaches a context running
**further down the stack**, this group is part of a bigger cycle. It parks under
the outer head and is resolved with it. Recursion never causes a cut.

## Why it ends, and why the entry doesn't matter

- Pending values start empty and only grow. A call that waits on a value adds
  nothing until the value arrives. Every value is bounded by the caps, so
  eventually a round changes nothing. `max_rounds` is the safety net: at the
  cap, the members keep what they found plus `Unknown`, and are marked.
- Inside a group, every call from one member to another answers `Pending`,
  whichever member was entered first, so every member's walk is the same. The
  fixpoint is the least solution of the same equations, which doesn't depend
  on the order. So the summaries are safe to reuse, and the no-reuse check
  confirms it on the parser entered from either end.

## Identity does not go around a cycle

```python
def grow(n):
    if n:
        return grow(n - 1)
    return JsonHandler()
```

Round 1 would bring back last round's `JsonHandler` through the recursive call,
with a new origin, then round 2 that one, forever. So in a member's own summary,
an object that came from a member's summary is exported as `Opaque(class)`.
`grow` returns its own `JsonHandler` (`Obj(0)`) and an `Opaque(JsonHandler)`,
and stops after one round. `grow(3).process()` finds `JsonHandler.process`.

The price: an object built deeper in a recursion keeps its class, not its
attributes, once it leaves the group.

## Known limit: effects inside a cycle reach only the exit

```python
class Tree:
    def fill(self, n, h):
        if n:
            self.fill(n - 1, h)
            self.h.process()        # Python: h's process, set by the inner call
        self.h = h
```

`self` isn't pending, so `self.h.process()` is walked with what the walk knew at
that statement: no `h` yet. The inner call's effect arrives in the fixpoint,
after the statement that needed it. The fixpoint repeats operations, not
statements, so this stays a gap, pinned by a fixture.

---

## Exercise

1. `python/recursion.py`, `use_walk()`: draw the tree for
   `walk(root, on_node_found)`. How many contexts of `walk`? How many times is
   `on_node_found` rendered?
2. Same file, `walk_handle`: does `r.close()` get an edge? Is the summary
   `Finished`?
3. `rec(n): return rec(n - 1)`, called as `rec(3).go()`. What does `rec`
   return? Is `rec` still in the tree?
4. `python/parser.py`: enter at `combine(Num(), t)` instead of `parse_expr(t)`.
   Is `parse_expr`'s final summary the same? Why?
5. Why is the a/b/c chain resolved, when one pass over the deferred
   operations would miss it?

<details><summary>Answers</summary>

1. `use_walk > walk > [on_node_found, walk [Recursive]]`. One context of `walk`
   (the recursive call has the same key: `node` is `Unknown` both times,
   `visit` is `on_node_found`). `on_node_found` is rendered once, under the one
   `walk` node: the recursive `walk` is a leaf, not entered.
2. Yes. Round 0 already has `walk` returning `Handle`; round 1 resolves `r` to
   `Handle`, and the deferred `Member` and `Call` key `Handle.close`. Nothing
   changes after that, so `walk` is `Finished`.
3. Nothing: the empty set. Its only return waits on itself and never gets a
   value, as in Python, where it never returns. `rec` is still in the tree
   (`main > rec > rec [Recursive]`), and `.go()` has no target.
4. Yes. Entered at `combine`, `combine` is running when `parse_expr`'s fixpoint
   reaches it, so the group merges into `combine`'s and is resolved when
   `combine`'s walk ends. Every walk sees the same thing (`Pending` for calls
   into the cycle), and the fixpoint is the same least solution.
5. `Handle` starts in `a`, and must reach `b` through `c`. One pass would let
   `b` read `c` before `c` had learned it. Rounds repeat until nothing changes,
   so it gets there.

</details>

---

You've covered every function-call shape. Do the folder exam, then classes.

→ [`exam.md`](exam.md), then [`../03-classes/README.md`](../03-classes/README.md)
