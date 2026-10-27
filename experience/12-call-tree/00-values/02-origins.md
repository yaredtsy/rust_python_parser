# 00.02 — Origins: which object, specifically

Two small `u32` newtypes carry almost all of "which object" from
[the previous file](01-abstractvalue.md). This file is about why two integers
are enough, and what breaks without them.

**Reading:** [`plan/03-call-tree/04-values.md`](../../../plan/03-call-tree/04-values.md)
§ "`OriginId`" and § "`ValueId`".

---

## The problem

```python
a = Pipeline(JsonHandler())
b = Pipeline(CsvHandler())
a.run(1)   # self.handler.process → JsonHandler.process
b.run(2)   # self.handler.process → CsvHandler.process
```

Without something telling `a`'s `Pipeline` from `b`'s, both are
`Instance { class: Pipeline }`: identical values. Attribute state is keyed by
object, so the two `handler`s would land in one slot, and the second write would
win for both.

## `OriginId`: one per object, inside one walk

```rust
pub struct OriginId(u32);   // index into the walk's origin interner

enum OriginKey {
    Entry(Path),                          // an object the caller passed in, named by its path
    Beyond { at: Path, attr: Name },      // an input object deeper than max_path: no state
    New { site: SiteId, t: u8, combo: u8, made: Made },   // an object built in this walk
}
enum Made { Instance, Obj(u16) }
```

Three kinds, by where the object came from:

| kind | example | named by |
|---|---|---|
| `Entry(path)` | inside `run(self)`, `self` is `Entry(Param(0))`, and `self.handler` is `Entry(Param(0).handler)` | where it sits in the call's inputs |
| `New { site, .. }` | `Pipeline(JsonHandler())` in this body | the call site that built it |
| `Beyond` | an input object more than `max_path` steps deep | the deepest path that fits, plus one step |

Two `Pipeline(...)` expressions are two sites, so `a` and `b` get two `New`
origins, and their `handler`s never collide. `t` and `combo` exist because one
site can build several objects: `(A if c else B)()` builds an `A` and a `B`, and
a call split by argument type ([`01-summaries/04`](../01-summaries/04-one-call.md))
builds one per combination.

**Origins are walk-local.** The same `Pipeline` object is `New { site }` in the
function that built it, and `Entry(Param(0))` inside `run`, which received it.
No origin is ever stored in a summary. The next folder shows how identity
crosses a call anyway.

## `OPAQUE`: everything ty hands you

One reserved origin for every instance that came from ty rather than from the
walk: a module global, a stdlib return, a value ty typed.

```python
a = get_logger()        # Instance{Logger,   OPAQUE}
b = get_db()            # Instance{Database, OPAQUE}
a.handler = FileHandler()
b.handler.write()       # if (OPAQUE, "handler") were stored, b would see a's write
```

They all share one origin, so **an `OPAQUE` instance holds no state**: writes to
it are dropped, and reads go straight to the class and ty.

Objects a caller passes in are not `OPAQUE`. They are `Entry`, named by path, so
writes to them are tracked and reported back to the caller
([`03-classes/05`](../03-classes/05-writes-anywhere-and-deep-writes.md)).

## `ValueId`: interning, to stay `Copy`

`BoundMethod` needs its receiver — `self.handler.process` must remember which
handler `process` is bound to. A `Box<AbstractValue>` would make the enum
non-`Copy`, so every env clone would chase pointers. Intern instead:

```rust
BoundMethod { func: Definition<'db>, receiver: ValueId }

pub struct ValueArena<'db> {
    values: Vec<AbstractValue<'db>>,
    dedup:  FxHashMap<AbstractValue<'db>, ValueId>,
}
```

One arena per walk. Receivers repeat constantly, so the dedup pays.

## Why newtypes, not `u32`

They index different tables and mean different things. A raw `u32` would let
the compiler accept a `ValueId` where an `OriginId` belongs. The newtype turns
that bug into a compile error.

---

## Exercise

```python
def two_things():
    x = Thing()
    y = Thing()
    return x.method, y.method

def alias():
    x = Thing()
    y = x
    return y
```

1. In `two_things`, do `x` and `y` get the same origin? What kind?
2. `x.method` and `y.method`: same `ValueId` or different?
3. In `alias`, what origin does `y` have? What does that imply for
   `x.attr = v; y.attr`?
4. Inside `def use(p): p.go()`, called as `use(Thing())`, what is `p`'s origin?
   Is it the same origin number as the `Thing()` in the caller?

<details><summary>Answers</summary>

1. Different. Two `Thing()` sites, two `New` origins.
2. Different. `Instance { Thing, origin A }` and `Instance { Thing, origin B }`
   are different values, so the arena gives them separate slots.
3. The same as `x`'s. `y = x` binds a name; it doesn't build anything. So
   `x.attr = v; y.attr` sees `v`, as in Python. Aliasing falls out of tying
   identity to construction, not to names.
4. `Entry(Param(0))`, in `use`'s walk. It is not the caller's origin number:
   origins are walk-local. The caller's `New` origin and the callee's `Entry`
   origin name the same Python object from two sides of the call.

</details>

---

→ Next: [`03-env-and-scope.md`](03-env-and-scope.md)
