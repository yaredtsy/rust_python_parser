# 02.04 — Two instances, no cross-talk

One fixture, given its own file, because the plan is explicit about its
importance: *"make it the first test you write."* Everything in
[`03`](03-attribute-state-after-init.md) is correct in theory until this
specific case is checked.

**Fixture:** `python/isolation.py`. **Reading:**
[`plan/03-call-tree/06-attributes-and-self.md`](../../../plan/03-call-tree/06-attributes-and-self.md)
§"Fixtures", fixture 2.

---

## The shape

```python
a = Pipeline(JsonHandler())
b = Pipeline(CsvHandler())
a.run(1)      # must resolve to JsonHandler.process
b.run(2)      # must resolve to CsvHandler.process
```

Two `Pipeline` instances, alive at the same time, each constructed with a
*different* handler. If your attribute store is wrong in the specific way
this fixture is built to catch, `b.run(2)` ends up reading `a`'s handler
(or vice versa) — one instance's state silently leaking into the other's.

## What this actually tests: `OriginId` allocation, not `member()`'s logic

This is not a new mechanism — it's `03`'s exact mechanism, run twice, with
the one variable that determines correctness being whether step 5 of
[`01-constructing-an-instance.md`](01-constructing-an-instance.md)'s trace
("a FRESH `OriginId` minted here") actually produces **two different**
`OriginId`s for `Pipeline(JsonHandler())` and `Pipeline(CsvHandler())`.

```
a = Pipeline(JsonHandler())
  → construct Pipeline → origin Oa minted
  → __init__ runs with self = Instance{Pipeline, Oa}
  → attrs[(Oa, "handler")] = Instance{JsonHandler, Ox}

b = Pipeline(CsvHandler())
  → construct Pipeline → origin Ob minted  — MUST differ from Oa
  → __init__ runs with self = Instance{Pipeline, Ob}
  → attrs[(Ob, "handler")] = Instance{CsvHandler, Oy}

a.run(1)
  → self = Instance{Pipeline, Oa}
  → member(self, "handler") → attrs.get((Oa, "handler")) → Instance{JsonHandler, Ox}  ✓

b.run(2)
  → self = Instance{Pipeline, Ob}
  → member(self, "handler") → attrs.get((Ob, "handler")) → Instance{CsvHandler, Oy}  ✓
```

If `OriginId` allocation were broken — say, if it were keyed on the
*class* being constructed rather than the *construction* (expression,
class, argument values) — `Oa` and `Ob` would collide, `attrs` would have only **one** entry
under key `(O, "handler")`, and whichever assignment ran *second*
(`CsvHandler()`, in program order) would silently win for **both** `a.run`
and `b.run`.

## Why this is the right fixture to catch that, and others aren't

Compare to [`03`](03-attribute-state-after-init.md)'s own fixture
(`pipeline.py`) — a single instance, written once, read once. That fixture
would pass even with origin allocation completely broken (there's only one
instance, so there's nothing for it to collide *with*). **Isolation bugs
only show up when two instances of the same class exist on overlapping
paths** — which is exactly what makes this fixture, not `03`'s, the one
that actually exercises the identity mechanism rather than just the
read/write plumbing around it.

## Exercise

1. Trace through the broken-allocation scenario above by hand: if `Oa ==
   Ob`, what does `attrs` look like right after both `__init__` calls
   finish, and which handler does `a.run(1)` end up resolving to?
2. The fixture constructs `a` and `b` in the **same frame** (both inside
   `two_instances()`). Does that matter for whether `Oa` and `Ob` must
   differ? Re-read [`00-values/02-origin-and-identity.md`](../00-values/02-origin-and-identity.md) —
   is `OriginId` uniqueness about which *frame* you're in, or something
   else?
3. Suppose instead of `Pipeline(JsonHandler())` / `Pipeline(CsvHandler())`,
   the fixture had been `a = Pipeline(JsonHandler()); b = a` (an alias, not
   a second construction — the exercise from `00-values/02`, question 3).
   Would `a.run` and `b.run` sharing the *same* origin now be a bug, or
   correct behaviour? Why does this not contradict the isolation
   requirement above?

<details><summary>Answers</summary>

1. `attrs` would have exactly one entry: `(O, "handler") → Instance{CsvHandler, Oy}`
   — the second `__init__` call (`b`'s) overwrites the first's entry under
   the shared key. `a.run(1)` would then resolve `self.handler` to
   `CsvHandler`'s instance, which is wrong — `a` was constructed with a
   `JsonHandler`.
2. It matters in the opposite direction from what intuition might suggest:
   `OriginId` uniqueness is **not** about which frame you're in (both
   constructions here are in the *same* frame, `two_instances`) — it's
   about *which construction* (expression, class, argument values), and
   `Pipeline(JsonHandler())` and `Pipeline(CsvHandler())` are two textually
   distinct `Pipeline(...)`
   expressions, so each gets its own fresh origin regardless of being in
   the same frame. (Contrast with calling the *same* `Pipeline(...)`
   expression twice in a loop — that's a harder case this design
   deliberately doesn't attempt to fully distinguish; see the "what you
   deliberately do not model" list in `00-values/01`.)
3. Correct behaviour, not a bug — `b = a` is a plain alias (no construction
   happened, `00-values/02`'s exercise 3), so `b` genuinely is the *same*
   object as `a` in real Python semantics, and sharing the origin is
   exactly right: `a.handler = X(); b.handler` really should see `X()`. The
   isolation requirement in this file is about **separate constructions**
   getting separate identities — it has nothing to say about aliases of
   the *same* construction, which correctly collapse to one identity by
   design, not by bug.

</details>

---

→ Next: [`05-assignment-outside-init-and-external-mutation.md`](05-assignment-outside-init-and-external-mutation.md)
