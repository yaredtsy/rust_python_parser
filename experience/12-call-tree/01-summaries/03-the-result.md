# 01.03 — The result: what a call did

**Reading:** [`plan/03-call-tree/09-summaries.md`](../../../plan/03-call-tree/09-summaries.md).
**Fixture:** `python/results.py`.

---

## The summary

```rust
pub struct FunctionResult<'db> {
    pub calls:   Vec<CallEdge<'db>>,      // every call site reached, and its targets
    pub returns: SVals<'db>,              // what it returns
    pub effects: Vec<Effect<'db>>,        // writes on objects the caller can see
    pub objects: Vec<NewObject<'db>>,     // objects it built that escape
    pub status:  Status,                  // Finished, or which gaps / which cut
}
```

One summary serves every caller with the same key, so nothing in it can name a
caller's object directly. Values are written **relative to the inputs**:

```rust
pub enum SVal<'db> {
    Plain(PlainVal<'db>),     // a function, class, module, ty type, Unknown, or Opaque(class)
    Ref(Path),                // "the caller's object at this path, when the call started"
    Obj(u16),                 // "objects[j]: the object I built"
    Bound   { func, recv },   // a bound method, receiver as an SVal
    Closure { func, free },   // a closure, captured values as SVals
}
```

## The four shapes, by example

```python
def ident(x):
    return x
```
`returns: [Ref(Param(0))]`. The caller reads its own parameter-0 object back:
same object, attributes and all. Identity crosses the call through the path.

```python
def make():
    return JsonHandler()
```
`returns: [Obj(0)]`, `objects: [NewObject { class: JsonHandler, site, attrs: [] }]`.
The caller gives the object a new origin of its own.

```python
def install(p, h):
    p.handler = h
```
`returns: [Plain(Ty(None))]`, `effects: [Effect { target: Param(0), attr: handler, value: [Ref(Param(1))], strong: true }]`.
`strong` because the write happens on every path that returns normally.

```python
def maybe(p, h):
    if c:
        p.handler = h
```
Same effect, `strong: false`: only one arm writes. The caller joins the new value
with the old one instead of replacing it.

## Where effects come from

The walk tracks every attribute write in `Env.written`, marked `Must` (on every
path so far) or `May`. A branch join keeps `Must` only if every arm has it. At
the end:

- a write to an `Entry(path)` object becomes an `Effect` at `path`, `strong` if
  `Must`;
- a write to an object built in this walk becomes part of that object's
  `attrs`, if the object escapes;
- a write to an `OPAQUE` object was already dropped.

Reading effects off `written` (instead of comparing attributes before and after)
also catches a write whose value happens to equal the old one.

## `Ref` paths go deeper than the key

The key stops at `fp_depth`, but a path doesn't have to. Inside a walk, reading
a missing attribute on an input object gives an object named by its path,
`Entry(Param(0).app.config.sink)`. So a write deep inside the caller's objects
still comes back as an effect at that path, and the caller applies it to its own
real object. Paths stop at `max_path`; past that, the write becomes a marked,
weak `Unknown` ([`03-classes/05`](../03-classes/05-writes-anywhere-and-deep-writes.md)).

---

## Exercise

Write the summary (returns, effects, objects) for each function in
`python/results.py`:

1. `ident`, `make`, `install`, `maybe`.
2. `swap_handler(p, h)`: `old = p.handler; p.handler = h; return old`.
3. `wrap(h)`: `return Box(h)` where `Box.__init__` does `self.item = h`.

<details><summary>Answers</summary>

1. As above.
2. `returns: [Ref(Param(0).handler)]`, `effects: [Param(0).handler = [Ref(Param(1))], strong]`.
   The return refers to the old handler, the one at that path **when the call
   started**. [`04-one-call.md`](04-one-call.md) is about making that true in
   the caller.
3. `returns: [Obj(0)]`, `objects: [NewObject { class: Box, attrs: [(item, [Ref(Param(0))])] }]`.
   `Box.__init__`'s write to `self.item` reached `wrap` as an effect on the new
   object, so it is part of the object's final state.

</details>

---

→ Next: [`04-one-call.md`](04-one-call.md)
