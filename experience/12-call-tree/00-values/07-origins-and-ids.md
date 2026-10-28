# 00.07 — Origins and the other walk-local ids

`attrs` is keyed by `OriginId`. This file is about what an origin is, and the
three other small ids a walk uses. All four are `u32` newtypes, and all four
mean something only inside one walk.

**Reading:** [`plan/03-call-tree/04-values.md`](../../../plan/03-call-tree/04-values.md)
§ "`OriginId`: which object" and § "`ValueId`".

---

## `OriginId`: which object

```rust
pub struct OriginId(u32);              // index into Walk.origins

enum OriginKey {
    Entry(Path),                       // an object the caller passed in, named by its path from the inputs
    Beyond { at: Path, attr: Name },   // an input object deeper than max_path: holds no state
    New { site: SiteId, t: u8, combo: u8, made: Made },   // an object built in this walk
}
enum Made { Instance, Obj(u16) }       // built by this site's constructor, or a callee's objects[j]
```

`Walk.origins` interns `OriginKey`s: the same key always gives the same id. An
origin is named by **where the object came from**, never by its class:

| kind | when | example |
|---|---|---|
| `Entry(path)` | the object reached the walk through its inputs | in `run`, `self` is `Entry(Param(0))`, `self.writer` is `Entry(Param(0).writer)` |
| `New { site, t, combo, made }` | the object was built at a call site in this walk | in `read_json`, `JsonWriter()` is `New{site: JsonWriter(), t: 0, combo: 0, made: Instance}` |
| `Beyond { at, attr }` | an input object more than `max_path` steps deep | `Entry` paths stop at 6 steps; below that, objects share one `Beyond` and hold nothing |

`t` and `combo` separate objects that one site builds for different callees, or
for different combinations of argument types (`03-classes/04`). `made: Obj(j)`
is for an object a callee returned: the `j`-th entry of its summary's `objects`.

A missing attribute read on an `Entry` object gives another `Entry` object, one
step deeper, so a write deep inside a caller's object can still be reported to
the caller by path ([`04-class-summaries/05`](../04-class-summaries/05-deep-writes.md)).

### `OPAQUE`

One reserved origin, shared by every instance ty hands the walk: a module
global, a stdlib return, a value typed by ty. Because they all share it, an
`OPAQUE` object holds no state: writes to it are dropped, and reads go to the
class and ty.

```python
a = get_logger()        # Instance{Logger,   OPAQUE}
b = get_db()            # Instance{Database, OPAQUE}
a.handler = FileHandler()
b.handler.write()       # if (OPAQUE, "handler") were stored, b would see a's write
```

## `ValueId`: interned values

```rust
pub struct ValueId(u32);              // index into Walk.values
pub struct ValueArena<'db> { values: Vec<AbstractValue<'db>>, dedup: FxHashMap<AbstractValue<'db>, ValueId> }
```

A `BoundMethod` needs its receiver, which is itself a value. Storing it as a
`ValueId` instead of a `Box` keeps `AbstractValue` small and `Copy`. Receivers
repeat a lot, so the dedup pays.

## `ActivationId` and `DeferId`

- `ActivationId` indexes `Walk.activations`: one run of a body inside the walk,
  for closures ([`06`](06-the-env.md#activations)).
- `DeferId` indexes `Walk.deferred`: an operation waiting on recursion. A
  `Pending(DeferId)` value is its placeholder
  ([`02-function-summaries/05`](../02-function-summaries/05-recursion-pending-values.md) and [`06`](../02-function-summaries/06-recursion-groups.md)).

## Why they must never leave the walk

`o1` is "the second object built in `read_json`'s walk". In `run`'s walk, the
number 1 is a different object, and in the next request it is nothing at all.
So no id from this file may appear in a summary. `finish` replaces them with
paths (`Ref`) and indexes into `objects` (`Obj`), and the caller turns those
back into its own origins ([`09`](09-the-four-conversions.md)).

---

## The running example, at this level

The two objects built in `read_json`, and their names in each walk:

| walk | origin | key |
|---|---|---|
| `read_json` (ctx#1) | `o0` | `New { site: JsonWriter(), t: 0, combo: 0, made: Instance }` |
| `read_json` (ctx#1) | `o1` | `New { site: Pipeline(JsonWriter()), t: 0, combo: 0, made: Instance }` — the `Pipeline` |
| `run` (ctx#3) | `e0` | `Entry(Param(0))` — the same `Pipeline` |
| `run` (ctx#3) | `e1` | `Entry(Param(0).writer)` — the same `JsonWriter` as `o0` |

Value arenas:

| walk | id | value | why interned |
|---|---|---|---|
| `read_json` | `v0` | `Instance{Pipeline, o1}` | receiver of `p.run` |
| `read_json` | `v1` | `Instance{JsonWriter, o0}` | receiver of `w.write` |
| `run` | `v0` | `Instance{JsonWriter, e1}` | receiver of `self.writer.write` |

Both walks have a `v0`, and they are different values. That is what walk-local
means.

---

## Exercise

1. `read_json` builds two `JsonWriter()`s on two lines. How many origins?
2. In `run`, what would `self.writer.cfg` be, if the key had no `cfg`
   attribute and ty says `cfg: Config`?
3. `def f(a, b)` is called as `f(x, x)`. How do `a` and `b` get one origin in
   `f`'s walk?

<details><summary>Answers</summary>

1. Two: two sites, two `New` keys.
2. `Instance{Config, Entry(Param(0).writer.cfg)}`: a missing attribute on an
   input object is named by its path, class from ty.
3. The key's `alias` says parameter 1 holds the same object as parameter 0, so
   `materialize` gives both `Entry(Param(0))`.

</details>

---

→ Next: [`08-the-value.md`](08-the-value.md)
