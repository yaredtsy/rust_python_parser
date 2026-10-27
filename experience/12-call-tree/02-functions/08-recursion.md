# 01.08 — Recursion and termination

Closing this folder with the case `00-values/05` already gave you the
vocabulary for — now applied directly to function calls, with a callback
riding along to prove the guard is precise, not blunt.

**Fixture:** `python/recursion.py`. **Reading:**
[`00-values/05-frame-pathkey-and-budget.md`](../00-values/05-frame-pathkey-and-budget.md)
§"The ancestor guard" (review), then
[`plan/03-call-tree/08-termination-and-cycles.md`](../../../plan/03-call-tree/08-termination-and-cycles.md).

---

## Direct recursion

```python
def f(): f()
```

```
resolve_call(f(), env, frame=<main's frame>)
  → child = frame.add_child("f", id)     — ancestors so far: [root, main, f]
  → callee_env = bind_parameters(f, ...)
  → walk_body(f, callee_env, child) finds the inner f() call
  → resolve_call(f(), callee_env, child)
      → frame.is_ancestor("f")?  → TRUE (it's child itself)
      → the call is DROPPED — no further recursion, no second child added
```

**One** `f` node in the output tree, no infinite loop, no special-casing
required beyond the ancestor check every `resolve_call` already runs.

## Mutual recursion

```python
def g(): h()
def h(): g()
```

```
main → g (ancestors: [root, main, g])
         → h (ancestors: [root, main, g, h])
             → calls g → is_ancestor("g")? TRUE → dropped
```

Same mechanism, one more hop. The guard doesn't care whether the cycle is
length 1 or length 5 — it's a linear walk up the `parent` chain
(`00-values/05`'s `is_ancestor`), checking the *qname*, not the specific
call site.

## The callback that rides along, unaffected

```python
def walk(node, visit):
    visit(node)
    for child in node.children:
        walk(child, visit)
```

`walk` recurses; `visit` does not. The question this fixture is built to
force you to answer precisely: **does the ancestor guard, applied to
`walk`, have any effect on resolving `visit`?**

It shouldn't, and tracing why clarifies what the guard actually tracks.
Each `resolve_call(visit(node), ...)` is evaluated independently, against
whatever `env` the *current* `walk` frame has — and `visit` is just a
parameter name in that env, resolved by the ordinary `eval_callee` lookup
from [`06-callbacks-and-higher-order.md`](06-callbacks-and-higher-order.md).
The ancestor guard only ever fires when the **qname being resolved** is
already on the ancestor chain — `visit`'s underlying function (say,
`on_node_found`) is never itself an ancestor of `walk`, so nothing stops it
from resolving correctly at every depth `walk` reaches, however deep that
goes (subject only to the **depth cap**, which is a separate brake from the
ancestor guard — see `00-values/06`).

## Exercise

Using `python/recursion.py`:

1. Draw `use_direct()`'s tree. How many nodes total, and at what depth does
   the guard fire?
2. Draw `use_mutual()`'s tree.
3. Suppose `walk` is called as `walk(root, on_node_found)` where `root` has
   three levels of nested children. How many `walk` nodes appear in the
   tree? How many `on_node_found` nodes? (Hint: re-read `00-values/05`'s
   `(parent, qname)` identity rule before answering the second part —
   this is the one place in this folder where that rule's consequence is
   least intuitive.)

<details><summary>Answers</summary>

1. `main → f`. Two nodes total (`main`'s own frame plus one `f` node) — the
   guard fires at depth 2 (inside `f`'s own frame, trying to add a *second*
   `f`), so only one `f` node ever gets created.
2. `main → g → h`. Three nodes — the guard fires inside `h`'s frame, trying
   to add `g` again, which is already two levels up the ancestor chain.
3. **Three** distinct `walk` frames — one per level of recursion, because
   each is a genuinely different frame in the `(parent, qname)` sense (each
   `walk` call's parent is the *previous* `walk` frame, not `main`, so
   they're never merged with each other). But only **one** `on_node_found`
   node total some might expect three (one "under" each walk level) — and
   that's wrong: each `walk` frame calls `visit(node)` exactly once in its
   own body, so each of the three `walk` frames gets its **own**
   `on_node_found` child (since each is a different frame) — so the answer
   is actually three `on_node_found` nodes too, one per `walk` frame, each
   with `call_count == 0` (called once in that frame). The subtlety worth
   sitting with: it's three, not one, specifically *because* frame identity
   is per-frame, not global — a shallower intuition ("the callback only
   shows up once, since it's the 'same' callback") is exactly the graph
   intuition `00-values/05` warned against carrying over from a call-graph
   tool.

</details>

---

With `01-functions/` done, you've built every mechanism except the
object-specific ones: `self` binding, instance state, and the MRO. Do the
folder exam, then move to classes.

→ [`exam.md`](exam.md), then [`../02-classes/README.md`](../02-classes/README.md)
