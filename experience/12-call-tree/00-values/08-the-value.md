# 00.08 — The value: the bottom of the picture

Every slot in `locals` and `attrs` holds `Values`: a short list of
`AbstractValue`s. This is the smallest unit the walk works with.

**Reading:** [`plan/03-call-tree/04-values.md`](../../../plan/03-call-tree/04-values.md).
**Fixture:** `python/shapes.py`.

---

## The structure

```rust
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(in crate::interp) enum AbstractValue<'db> {
    Function(Definition<'db>),                                   // a module-level function, named
    Closure { func: Definition<'db>, activation: ActivationId }, // a nested def, with the run it ran in
    Class(Definition<'db>),                                      // a class object, named
    Instance { class: Definition<'db>, origin: OriginId },       // one object: class + which one
    BoundMethod { func: Definition<'db>, receiver: ValueId },    // `obj.method`, receiver interned
    Module(Module<'db>),                                         // `import m`, then `m`
    Ty(Type<'db>),                                               // ty's answer: literals, stdlib returns
    Pending(DeferId),                                            // a value waiting on recursion
    Unknown,                                                     // a value nobody knows
}

pub type Values<'db> = SmallVec<[AbstractValue<'db>; 2]>;
```

Functions and classes are identified by `Definition<'db>`, the `def` or `class`
statement: public, 4 bytes, and what the qname, docstring ID and project check
start from. ty's own `FunctionType` and `ClassLiteral` can't be named outside
ty's crate.

### Why not `ty::Type`?

`Type` is a set: `Type::NominalInstance(JsonWriter)` means "some `JsonWriter`".
The walk needs "this `JsonWriter`, built there", to keep two objects apart. And
`Type::Union` is one value; `Values` keeps the alternatives separate, so each
produces its own edges. Collapse `Values` into a `Type::Union` and you have
rebuilt `ty_ide::outgoing_calls`. `Ty(..)` is still used constantly, for every
value that carries no path-specific information.

### `Values`: several possibilities

One expression can have several values: `x if c else y`, a union ty hands over,
a function with two `return`s. `SmallVec<[_; 2]>` because one value is by far
the common case and should not allocate.

**Empty is not `Unknown`.** An empty `Values` means no value reaches here: every
path to it raised, or it waits on a cycle that hasn't produced anything. `[Unknown]`
means a value exists and nobody knows what it is.

**`Unknown` is not `Ty(Unknown)`.** `Unknown` means nobody asked ty;
`Ty(Dynamic)` means ty was asked and doesn't know. Keep them apart: a debug
dump then tells you whether a gap came from the walk or from ty.

## Where values come from

| source | gives |
|---|---|
| the env (`locals`, `attrs`) | whatever was bound or written there |
| the key, at the start of the walk | input objects as `Instance { origin: Entry(path) }`, and other values ([`05`](05-the-walk.md)) |
| a constructor call | `Instance { origin: New { site, … } }` |
| a project call | the callee's returns, resolved ([`09`](09-the-four-conversions.md)) |
| ty, when the env has nothing | `lift(type)`: a function or class literal → `Function` / `Class`; a union → each member separately; an instance → `Instance { origin: OPAQUE }`; `Any` → `Unknown`; the rest → `Ty` |

Env first, ty second. That order is the whole design: the env holds what this
path knows; ty knows what's true on every path.

## The precision policy, in values

- **Conditions are never evaluated.** Every arm of an `if`, `match`, ternary,
  `and` / `or` is possible: each starts from the state before it, and the arms
  are joined after. `build("json")` with a ternary on `kind` gives both writers.
- **Assignment order is respected.** On one path a later assignment replaces an
  earlier one: `w = JsonWriter(); w.write(); w = CsvWriter(); w.write()` gives
  one write each.
- **A wide union is capped.** Over `max_union_fanout` (4), `cap_fanout` first
  drops values that can't produce a node (stdlib, `Ty`), then gives up with
  `Unknown`.

## Small and `Copy`

Every variant holds salsa handles or `u32` ids, never a `String` or a `Vec`.
`AbstractValue` is copied into envs constantly, so a heap allocation in it would
make every env clone allocate. The target is 16 bytes; `Ty(Type)` alone is 16,
so it fits only if rustc packs the tag into `Type`'s spare values. If the size
test fails, store types in a per-walk arena and keep a `u32` in `Ty`
([`plan/03-call-tree/04`](../../../plan/03-call-tree/04-values.md#sizing)).

---

## The running example, at this level

Every value in `read_json`'s walk:

| expression | `Values` | from |
|---|---|---|
| `JsonWriter` | `[Class(JsonWriter)]` | ty, lifted |
| `JsonWriter()` | `[Instance{JsonWriter, o0}]` | constructor |
| `Pipeline` | `[Class(Pipeline)]` | ty, lifted |
| `Pipeline(JsonWriter())` | `[Instance{Pipeline, o1}]` | constructor |
| `p.run` | `[BoundMethod{Pipeline.run, receiver: v0}]` | `member`, our receiver |
| `1` | `[Ty(Literal[1])]` | ty |
| `p.run(1)` | `[Instance{JsonWriter, o0}]` | `run`'s returns, resolved |
| `w.write(2)` | `[Ty(None)]` | `write`'s returns |

And in `run`'s walk: `self` is `[Instance{Pipeline, e0}]`, `self.writer` is
`[Instance{JsonWriter, e1}]`, `data` is `[Ty(int)]`.

---

## Exercise

`python/shapes.py`, `uses_all_five`:

1. Which variant is each of `a`–`e`?
2. Which of them would be fingerprinted to the same `Fp` if passed to a call,
   and which are the same `AbstractValue`?
3. In `two_returns`-style code — `return A() if c else B()` — how many values
   does the call's result have, and does a literal `c` change that?

<details><summary>Answers</summary>

1. `a = plain` → `Function(plain)`; `b = Thing` → `Class(Thing)`;
   `c = Thing()` → `Instance{Thing, New{site}}`;
   `d = c.method` → `BoundMethod{Thing.method, receiver: <c>}`;
   `e = json` → `Module(json)`.
2. None of them share an `Fp` with each other; but a second `Thing()` on another
   line would share `c`'s fingerprint (`Instance{Thing, []}`) while being a
   different `AbstractValue` (a different origin).
3. Two. Conditions are never evaluated, so a literal changes nothing.

</details>

---

→ Next: [`09-the-four-conversions.md`](09-the-four-conversions.md)
