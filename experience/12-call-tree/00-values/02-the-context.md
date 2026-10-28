# 00.02 — The context: what identifies a call

One level down: the keys the solver interns. A context says which function is
called, and what is known about its arguments. Two calls with equal contexts
share one summary.

**Reading:** [`plan/03-call-tree/08-keys.md`](../../../plan/03-call-tree/08-keys.md).

---

## The structures

```rust
pub struct ContextId(u32);
pub struct FpId(u32);

pub struct Context<'db> {
    pub func:  Definition<'db>,                // which function (the `def`)
    pub args:  SmallVec<[FpId; 4]>,            // one per parameter: receiver first, defaults filled
    pub alias: SmallVec<[u8; 4]>,              // alias[i] = first parameter holding the same object as i
    pub free:  SmallVec<[(Name, FpId); 2]>,    // a closure's captured values, sorted by name
}

pub enum Fp<'db> {
    Unknown,                                    // nothing known, or the parameter doesn't matter
    Data(Type<'db>),                            // a data type: `int`, `str`, `list[int]` (literals widened)
    Function(Definition<'db>),
    Class(Definition<'db>),
    Module(Module<'db>),
    Bound    { func: Definition<'db>, recv: FpId },                   // a bound method
    Closure  { func: Definition<'db>, free: SmallVec<[(Name, FpId); 2]> },
    Back(u8),                                   // "the closure k levels up": a closure that captures itself
    Instance { class: Definition<'db>, attrs: SmallVec<[(Name, FpId); 4]> },   // attrs sorted by name
    Opaque(Definition<'db>),                    // an object: class known, attributes not in the key
    Union(SmallVec<[FpId; 4]>),                 // several values, when splitting is over its cap
}
```

A fingerprint is a caller's value **reduced to what the key holds**. It keeps the
class and, to `fp_depth` steps (default 2), the attributes. It never says which
object, or where it was built:

| caller's value | fingerprint |
|---|---|
| an object the walk tracks | `Instance { class, attrs }`, each attribute fingerprinted one step deeper |
| an object at the depth limit, or one ty handed over | `Opaque(class)` |
| `3`, `"x"`, `[1, 2]` | `Data(int)`, `Data(str)`, `Data(list[int])` |
| a function, class or module | itself |
| `obj.method` | `Bound { func, recv: <obj's fingerprint> }` |
| a closure | `Closure { func, free }`, its captured values fingerprinted |
| nothing known | `Unknown` |

Three more rules decide what goes in:

- **Irrelevant parameters are `Unknown`.** A parameter that never reaches a call,
  a receiver, a project call's argument, a `return`, an attribute write or a
  nested `def` can't change what the call does. `relevant_params(func)` decides
  it from the code, once per function.
- **`alias`** records which parameters are the same object. Fingerprints can't:
  two different `JsonWriter`s look identical.
- **`free`** is empty unless the function is a closure.

## How a call's values become a context

At a call site, four short-lived structures appear in order:

```
EvaluatedArgs { positional: [Values], keywords: [(Name, Values)], has_splat }
    ↓ bind_parameters                      match values to parameters, receiver first, defaults filled
Bound { slots: [Values] }                  one entry per parameter, in the caller's terms
    ↓ split                                one combination per mix of argument types (up to split_cap)
Combo { slots: [Values] }                  one value per slot (several, if collapsed)
    ↓ key                                  fingerprint each relevant slot; intern
ContextId
```

None of these outlive the call. Only the `ContextId` (and what it points at) is
kept.

## Entry contexts

An entry is a function the request asks about. Its context has every parameter
`Unknown`. When its walk starts, each parameter falls back to its annotation, if
it has one ([`05`](05-the-walk.md#starting-materialize)).

---

## The running example, at this level

**ctx#2: `Pipeline(JsonWriter())` in `read_json`.**

```
EvaluatedArgs  positional: [[Instance{JsonWriter, o0}]]
receiver       the new object, Instance{Pipeline, o1}     (03-classes/01)
Bound          [self ← Instance{Pipeline, o1}, writer ← Instance{JsonWriter, o0}]
Combo          one: each slot has one value
fingerprint    self:   o1 has no attributes yet       → Instance{Pipeline, []}    → fp#0
               writer: o0 has no attributes          → Instance{JsonWriter, []}  → fp#1
alias          [0, 1]                                  two different objects
key            Context { Pipeline.__init__, args: [fp#0, fp#1], alias: [0, 1], free: [] } → ctx#2
```

**ctx#3: `p.run(1)` in `read_json`.** By now `__init__`'s effect has given `p`
its `writer`:

```
fingerprint    self: Instance{Pipeline, o1}, attrs (o1, writer) → Instance{JsonWriter, o0}
                     → Instance{Pipeline, [writer: Instance{JsonWriter, []}]}       → fp#2
               data: Ty(Literal[1]) → widened                     → Data(int)      → fp#3
key            Context { Pipeline.run, args: [fp#2, fp#3], alias: [0, 1], free: [] } → ctx#3
```

**ctx#4: `self.writer.write(data)` in `run`.** `write` uses neither parameter:

```
key            Context { JsonWriter.write, args: [fp#4, fp#4], … }  where fp#4 = Unknown   → ctx#4
```

**The reuse.** Back in `read_json`, `w.write(2)` builds the same key: `write`
still ignores both parameters. The interner returns ctx#4, which is already
`Ready`, so no walk happens.

**Sharing across functions.** `read_csv`'s `Pipeline(CsvWriter())` gives
`writer: Instance{CsvWriter, []}` (fp#5), so it is a different context, ctx#6.
If `read_csv` built a `JsonWriter` instead, it would get ctx#2 again: same key,
same summary, even though it is a different object built in a different
function.

---

## Exercise

1. `read_json` does `p = Pipeline(JsonWriter())` twice on two lines, and calls
   `.run(1)` on each. How many `run` contexts?
2. `Pipeline.run` is called with `data = "x"`. Same context as ctx#3?
3. Suppose `run` didn't return `self.writer` and didn't call anything on it.
   What would `self` be in `run`'s key?
4. Why can't the key hold "the object built on line 15"?

<details><summary>Answers</summary>

1. One: both objects fingerprint to fp#2. Two objects, one summary.
2. No: `data` is relevant (it is passed on to a call), and `Data(str)` is not
   `Data(int)`. A new context.
3. `Unknown` (fp#4): `self` would reach nothing that matters.
4. Then two calls with equal-looking objects built on different lines could
   never share a summary, and sharing is the point. Which object is which is
   kept another way: by paths in the summary ([`09`](09-the-four-conversions.md)).

</details>

---

→ Next: [`03-the-slot.md`](03-the-slot.md)
