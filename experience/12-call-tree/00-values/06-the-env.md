# 00.06 — The env: what one walk knows

Inside the walk: the environment. It holds what each local name is bound to and
what each object's attributes hold, as of the statement being evaluated.

**Reading:** [`plan/03-call-tree/05-the-walk.md`](../../../plan/03-call-tree/05-the-walk.md)
§ "The environment".

---

## The structure

```rust
#[derive(Clone)]
pub struct Env<'db> {
    locals:    rpds::HashTrieMap<Name, Values<'db>>,              // name → values
    attrs:     rpds::HashTrieMap<(OriginId, Name), Values<'db>>,  // (object, attribute) → values
    written:   rpds::HashTrieMap<(OriginId, Name), Write>,        // attributes this walk wrote
    scope:     BodyScope,                                          // which function body these locals belong to
    enclosing: Option<ActivationId>,                               // where this function was defined (closures)
}

pub enum Write { Must, May }   // written on every path so far, or only on some
```

| field | answers |
|---|---|
| `locals` | what does `x` hold here? |
| `attrs` | what does this object's `.attr` hold? Keyed by object, not by name: `(OriginId, Name)` |
| `written` | which attributes did this walk write, on every path or only some? `finish` turns it into effects |
| `scope` | may this name be seen here? Guards against one function's `x` leaking into another |
| `enclosing` | for a free variable of a closure: the run where the closure's `def` ran |

**No link to the caller.** Values from the caller arrive one way: through the
key, materialized into the starting env. Nothing looks a name up through the
caller. `enclosing` is where the function was **defined**, never where it was
**called from**.

**Persistent maps.** `rpds` maps share structure, so a clone is a pointer copy.
The walk clones the env at every call site (the snapshot `before`) and at every
branch, so this is what keeps it cheap.

## How it changes

- **An assignment** `x = v` replaces `x` in `locals`. On one path, a later
  assignment wins.
- **An attribute write** `o.a = v`: if `o` is exactly one traced object, replace
  `(o, a)` and record `Must`; if it may be several, join the new value with the
  old and record `May`. If `o` can't hold state (`OPAQUE`, `Unknown`), drop it.
- **A branch** forks: each arm starts from the env before it, and the arms are
  joined after. A join unions values per name and per attribute, and keeps
  `Must` only if every arm wrote `Must`.
- **A call** applies the callee's effects and new objects to `attrs`, recording
  `Must` or `May` in `written`, so they reach this walk's own summary too.

## Activations

```rust
activations: IndexVec<ActivationId, Env<'db>>   // in Walk
```

An activation is one run of one body inside the walk, with a slot holding its
current env. The walk's own body is one. A `def` statement binds its name to
`Closure { func, activation: <this run> }`, and a lookup of a free variable
reads that activation's slot at call time, so later assignments are seen, as in
Python. Closures are [`01-functions/11`](../01-functions/11-closures.md).

---

## The running example, at this level

**ctx#1, `read_json`**, the env after each statement:

| after | `locals` | `attrs` | `written` |
|---|---|---|---|
| start | — | — | — |
| `p = Pipeline(JsonWriter())` | `p → [Instance{Pipeline, o1}]` | `(o1, writer) → [Instance{JsonWriter, o0}]` | `(o1, writer) → Must` |
| `w = p.run(1)` | `+ w → [Instance{JsonWriter, o0}]` | same | same |
| `w.write(2)` | same | same | same |

- `(o1, writer)` came from `__init__`'s effect, applied when `Pipeline(...)`
  returned: one object, a strong effect, so `Must`.
- `w` came from `run`'s returns, `Ref(Param(0).writer)`, resolved against the
  env as it was before the call ([`09`](09-the-four-conversions.md)).
- At `finish`, `(o1, writer)` is in `written`, but `o1` is an object built here,
  not an input, and it doesn't escape. So it produces no effect and no object.

**ctx#3, `run`**: its env never changes after the start (shown in
[`05`](05-the-walk.md)): it reads, calls and returns, but writes nothing.

---

## Exercise

1. Add `p.writer = CsvWriter()` after `w = p.run(1)` in `read_json`. Write the
   env after it. Does `w` change?
2. Instead add `if c: p.writer = CsvWriter()`. What is `(o1, writer)` after the
   `if`, and what is `written`?
3. Why is `attrs` keyed by `(OriginId, Name)` and not by `(variable, Name)`?

<details><summary>Answers</summary>

1. `attrs (o1, writer) → [Instance{CsvWriter, o2}]` (replaced: one object),
   `written (o1, writer) → Must`. `w` doesn't change: it holds the `JsonWriter`
   object `o0` itself, not "whatever `p.writer` is".
2. `(o1, writer) → [Instance{JsonWriter, o0}, Instance{CsvWriter, o2}]`: the
   `if` arm and the skip arm are joined. `written (o1, writer) → Must`: both
   arms reach the join with `Must`, the `if` arm from its own write and the skip
   arm from `__init__`'s earlier effect. `Must` means "written on every path so
   far", not "written inside this branch". The value is still the union.
3. Because two names can point at one object (`q = p`), and one name can point
   at different objects over time. State belongs to the object.

</details>

---

→ Next: [`07-origins-and-ids.md`](07-origins-and-ids.md)
