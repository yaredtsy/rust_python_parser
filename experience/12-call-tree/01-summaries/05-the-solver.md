# 01.05 — The solver: reuse, cuts, and the no-reuse check

**Reading:** [`plan/03-call-tree/13-solver-and-limits.md`](../../../plan/03-call-tree/13-solver-and-limits.md),
and [`15`](../../../plan/03-call-tree/15-checks-and-build-order.md) § "No-reuse mode".
**Fixture:** `python/cut.py`.

---

## `get(ctx)`

The solver holds one slot per context. `get` is what step 5 of a call
([`04-one-call.md`](04-one-call.md)) asks:

| slot state | `get` |
|---|---|
| `Ready` | return the stored summary. **Reuse.** Nothing is walked |
| `New` | walk the body once, finish the summary, store it as `Ready`, return it |
| `Running` | its walk is on the stack: recursion. Return `Pending` ([`02-functions/08`](../02-functions/08-recursion.md)) |
| `Parked`, `Resolving` | part of a recursion group still being solved. Same lesson |
| `Cut` | walk it again (below) |

The walk of a new context can itself reach calls, so `get` recurses: `main`'s
walk asks for `emit`, whose walk asks for `JsonWriter.write`. The walk and the
solver are separate structs, passed side by side, so the recursion doesn't fight
the borrow checker.

## Cost limits truncate, and are never reused

Four limits depend on *how* a context was reached, not on its key:

| limit | default | fires when |
|---|---|---|
| `max_stack` | 256 | the walk would go deeper than this many nested gets |
| `max_contexts` | 50,000 | the request has created this many contexts |
| `contexts_per_func` | 64 | one function has this many contexts in this request |
| `deadline` | 10 s | the request has run this long |

When one fires, that call gets an `Incomplete(Cut)` result: `[Unknown]`, no
effects, no edges. A walk that reads a cut result is itself `Incomplete(Cut)`.

Cut results are **never stored as `Ready`**. A context truncated because the
stack was nearly full may be reached again from a shallower caller with plenty
of room. Reusing the truncated answer there would make the answer depend on
which caller came first: rule 3 ([`01-why-summaries.md`](01-why-summaries.md)).
So a `Cut` slot is walked again every time it is reached. The deadline bounds
how slow that can get.

Every cut is marked in the output (`Mark::Incomplete`) and counted in `Stats`.

## No-reuse mode: does reuse change anything?

`limits.reuse = false` makes `get` walk every context fresh instead of returning
a `Ready` summary. The output, graph and tree, must be byte-for-byte the same as
with reuse.

If a walk depends on something its key doesn't hold (a broken rule 1), reuse
gives the first caller's answer to the second, recomputing gives the second
caller its own answer, and the two outputs differ. That's what the check
catches. Turn the deadline off and raise the cost limits for it, or a cut will
make the two runs differ for the wrong reason.

What it does **not** prove: that the graph is right. Both modes can drop the
same edge and agree. That's what the expected-edge fixtures are for.

No-reuse is as slow as the exponential walk. Run it on fixtures and small
projects, not large ones.

---

## Exercise

`python/cut.py`, run with `max_stack = 4`:

```python
def leaf(): return JsonHandler()
def mid():  return leaf()
def top():  return mid()
def b():    top()
def a():    b()

def main():
    a()
    top().process()
```

1. Walk `main`'s first statement. At which call does `max_stack` fire? Which
   contexts end `Incomplete(Cut)`?
2. Walk the second statement. Is `top`'s first result reused? What does
   `.process()` find?
3. What would happen if cut results were stored as `Ready`?

<details><summary>Answers</summary>

1. Depths: `main` 0, `a` 1, `b` 2, `top` 3. `mid` would be at 4, so `get(mid)`
   is cut and returns `[Unknown]`. `top` read a cut result, so `top` is
   `Incomplete(Cut)`, then `b`, then `a`. `main` reads `a`'s cut result, so
   `main` ends `Incomplete(Cut)` too, and the run reports it.
2. No. `top`'s slot is `Cut`, so it is walked again, now at depth 1: `mid` at 2,
   `leaf` at 3. It returns a `JsonHandler`, and `.process()` finds
   `JsonHandler.process`.
3. The second `top()` would reuse `[Unknown]`, and `JsonHandler.process` would
   be missing, only because a deeper caller reached `top` first.

</details>

---

→ Next: [`06-graph-and-tree.md`](06-graph-and-tree.md)
