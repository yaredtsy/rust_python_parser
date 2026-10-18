# 00.02 — `OriginId` and `ValueId`: staying `Copy` while staying precise

Two small `u32` newtypes carry almost the entire weight of "which instance,
specifically" from [the previous file](01-abstractvalue.md). This file is
about why two integers are enough, and what breaks if you skip them.

**Reading:** [`plan/03-call-tree/04-value-domain.md`](../../../plan/03-call-tree/04-value-domain.md)
§"`OriginId`" and §"`ValueId`".

---

## The problem `OriginId` solves

```python
a = Pipeline(JsonHandler())
b = Pipeline(CsvHandler())
a.run(1)   # must resolve self.handler.process → JsonHandler.process
b.run(2)   # must resolve self.handler.process → CsvHandler.process
```

Without something distinguishing `a`'s `Pipeline` from `b`'s, both are just
`AbstractValue::Instance { class: Pipeline }` — identical values, and your
attribute store (`02-classes/03-attribute-state-after-init.md`) would have
nowhere to keep their `handler`s apart. Whichever was written last would
silently win for both.

```rust
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct OriginId(u32);   // indexes an arena of (File, TextRange, PathHash)
```

**`OriginId` identifies a construction site on a path** — which `Foo()`
expression, reached by which sequence of calls. Two `Pipeline(...)` calls
get two different `OriginId`s even though they construct the same class, and
that's what lets `a` and `b` carry independent `handler`s. This is the
property you must prove with a fixture before you trust anything downstream
— see [`02-classes/04-two-instances-and-origin-isolation.md`](../02-classes/04-two-instances-and-origin-isolation.md),
which plan/03-call-tree/06 calls out as "make it the first test you write."

A sentinel value, `OriginId::OPAQUE`, is reserved for instances that came
from `ty` rather than from a construction site your interpreter actually
walked (`lift()`'s `Type::NominalInstance` arm, in
[`03-env-and-scope.md`](03-env-and-scope.md)). It must never compare equal
to a real site — otherwise two unrelated opaque instances would appear to
share state.

## The problem `ValueId` solves

`BoundMethod` needs to carry its receiver — `self.handler.process` needs to
remember *which* `JsonHandler` `process` is bound to. The obvious way to
write that is:

```rust
BoundMethod { func: FunctionType<'db>, receiver: Box<AbstractValue<'db>> }
```

This compiles and is wrong for exactly the reason [the previous
file](01-abstractvalue.md#sizing-the-rule-that-keeps-everything-else-fast)
flagged: `Box` is a heap allocation, so `AbstractValue` stops being `Copy`,
and every environment clone now walks pointers instead of copying words.

```rust
BoundMethod { func: FunctionType<'db>, receiver: ValueId }

pub struct ValueArena<'db> {
    values: Vec<AbstractValue<'db>>,
    dedup: FxHashMap<AbstractValue<'db>, ValueId>,
}
```

**Intern instead.** `ValueId` is a `u32` index into one arena, allocated per
request. Looking up `dedup` before inserting means two bound methods with
the same receiver — common, since receivers repeat constantly across a
method-heavy codebase — share one arena slot.

## Why both are newtypes, not raw `u32`

```rust
fn member(self, cx: &Cx<'db>, name: &str) -> Values<'db> { ... }
//                                              ^^^^^^^^^^^^
// signature takes `OriginId`, not `u32` — the compiler stops you
// from accidentally passing a ValueId where an OriginId belongs
```

They index *different* arenas and mean different things — one identifies
"which instance", the other "which already-seen value". A raw `u32` would
let the compiler accept either in either slot; the newtype wrapper turns
that class of bug into a compile error instead of a debugging session two
weeks later when `self.handler` resolves to the wrong handler on an
unrelated path.

## Exercise

Using `python/shapes.py`'s `uses_all_five` (previous file) plus this
addition:

```python
def two_things():
    """ID: 00000000-0005-0000-0000-000000000000"""
    x = Thing()
    y = Thing()
    return x.method, y.method
```

1. Do `x` and `y` get the same `OriginId` or different ones? Why?
2. `x.method` and `y.method` — same `ValueId` or different? (Hint: `ValueId`
   interns *values*, and `x` and `y` are different values even though they're
   the same class.)
3. Now suppose the function instead read `x = Thing(); y = x` (an alias, not
   a second construction). What `OriginId` does `y` get, and what does that
   imply about aliasing in this design?

<details><summary>Answers</summary>

1. **Different.** Each `Thing()` call is a separate construction site reached
   on this path — that's the entire point of `OriginId`.
2. **Different.** `ValueId` dedupes identical `AbstractValue`s, and
   `Instance { class: Thing, origin: A }` ≠ `Instance { class: Thing, origin: B }`
   because the origins differ — so the arena gives them separate slots, and
   `x.method` and `y.method` end up as distinct `BoundMethod`s.
3. `y` gets the **same** `OriginId` as `x` — `y = x` is a plain name binding
   (`02-classes/03-attribute-state-after-init.md`'s machinery, and
   [`01-functions/05-returns-and-value-flow.md`](../01-functions/05-returns-and-value-flow.md)),
   not a construction, so no fresh origin is minted. This is correct: `x`
   and `y` really are the same object in the Python semantics, and
   `x.attr = v; y.attr` should see `v`. Aliasing falls out for free because
   origin identity is tied to *construction*, not to *name*.

</details>

---

→ Next: [`03-env-and-scope.md`](03-env-and-scope.md)
