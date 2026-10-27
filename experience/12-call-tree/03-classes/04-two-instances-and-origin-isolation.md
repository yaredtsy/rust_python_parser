# 03.04 — Two instances, no cross-talk

One fixture, given its own file, because the plan says: *make it the first test
you write.* Everything in [`03`](03-attribute-state.md) looks right until this
case is checked.

**Fixtures:** `python/isolation.py`, `../01-summaries/python/split.py`.
**Reading:** [`plan/03-call-tree/07-attributes-and-self.md`](../../../plan/03-call-tree/07-attributes-and-self.md)
§ "Fixtures", fixture 2, and [`10-one-call.md`](../../../plan/03-call-tree/10-one-call.md)
§ "CPA".

---

## The shape

```python
a = Pipeline(JsonHandler())
b = Pipeline(CsvHandler())
a.run(1)      # must be JsonHandler.process
b.run(2)      # must be CsvHandler.process
```

## What it tests: origins, not `member`

```
a = Pipeline(JsonHandler())   → New{site A}; attrs[(A, handler)] = JsonHandler
b = Pipeline(CsvHandler())    → New{site B}; attrs[(B, handler)] = CsvHandler   (B ≠ A)

a.run(1) → key run { self: Instance{Pipeline, [handler: Instance{JsonHandler}]} } → JsonHandler.process
b.run(2) → key run { self: Instance{Pipeline, [handler: Instance{CsvHandler}]} }  → CsvHandler.process
```

If both constructions got one origin (say, keyed by class instead of by site),
`attrs` would have one `(O, handler)` entry, the second write would win, and
both `run` keys would carry a `CsvHandler`. `a.run(1)` would find
`CsvHandler.process`.

A one-instance fixture can't catch this: with one object there is nothing to
collide with. Isolation bugs only show when two objects of one class are alive
on the same path.

Note what is shared and what isn't. `Pipeline.__init__` has two contexts here
(different handler types). If both were `JsonHandler`s, `__init__` would have
**one** context and `run` one too — and that is still correct, because the
summaries talk about `Param(0)`, never about a specific object. Origins keep
the objects apart; keys let them share work.

## The harder version: one site, several objects

```python
def build(h):
    return Job(h, h.key())

def main(c):
    h = JsonHandler() if c else CsvHandler()
    build(h).go()
```

Here there is **one** site, `build(h)`, and it produces two `Job`s: one per
combination of argument types. Each returned `Job` gets an origin with the
combination index: `New { site, t: 0, combo: k, made: Obj(0) }`. Without `k`,
the two `Job`s would be one object, and `go()` would see a `JsonHandler` paired
with a `CsvKey`. That's why `combo` is part of an origin
([`01-summaries/04`](../01-summaries/04-one-call.md)).

## Aliases are not a second object

`a = Pipeline(JsonHandler()); b = a` is one construction. `b` has `a`'s origin,
and `a.handler = X(); b.handler` sees `X`. Isolation is about separate
constructions, not about names.

---

## Exercise

1. With broken origin allocation (one origin per class), what does `attrs` hold
   after both constructions, and what does `a.run(1)` find?
2. The two constructions are in the same function. Does that matter?
3. In `split.py`, how many `Job` origins does `main`'s walk have after
   `build(h)`?

<details><summary>Answers</summary>

1. One entry, `(O, handler) → CsvHandler`. `a.run(1)` keys `run` with a
   `CsvHandler` and finds `CsvHandler.process`.
2. No. Origins are per site (and callee and combination), not per function.
   Two sites in one function are two origins.
3. Two: `New{site, t: 0, combo: 0, Obj(0)}` and `New{site, t: 0, combo: 1, Obj(0)}`.

</details>

---

→ Next: [`05-writes-anywhere-and-deep-writes.md`](05-writes-anywhere-and-deep-writes.md)
