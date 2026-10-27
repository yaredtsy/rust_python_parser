# 00.01 — `AbstractValue`: what flows through the interpreter

Before function calls, before classes: the one type everything else is built
from. Get this wrong and both the `02-functions/` and `03-classes/` folders
are awkward; get it right and they mostly write themselves.

**Reading:** [`plan/03-call-tree/04-values.md`](../../../plan/03-call-tree/04-values.md)
is the source this file teaches. Read this first, that second, for the exact
`[check]` notes and the sizing test.

---

## The question this answers

Your Jedi driver, for any Python expression, can say "this is `JsonWriter`,
*the one constructed on line 12*" -- not just "this is some `JsonWriter`".
That distinction — *which one* — is the entire reason the tool exists
(`plan/03-call-tree/01-what-jedi-actually-does.md`). Everything in this
exercise is in service of keeping that distinction alive through a Rust
interpreter.

## Why not just reuse `ty::Type`

It's tempting — `Type<'db>` is already `Copy`, already has class machinery,
already exists. Try it on this example first:

```python
a = JsonWriter()
b = CsvWriter()
```

`ty`'s answer for both is drawn from `Type::NominalInstance` — correct, and
exactly as precise as you need *here*. Now:

```python
def build(writer_cls):
    return writer_cls()

a = build(JsonWriter)
b = build(CsvWriter)
```

`ty` gives `build` **one** return type, the same for every call — it
doesn't re-check the body with *this* call's arguments, so `a` and `b` get
the same answer (with `writer_cls` unannotated, that's `Unknown`). pylspt
binds `writer_cls` per call, and each call becomes its own context
(`build {writer_cls: Class(JsonWriter)}`, `build {writer_cls: Class(CsvWriter)}`),
so `a` is a `JsonWriter` instance and `b` a `CsvWriter` one. Two values your Jedi
driver tells apart collapse into one `ty::Type`. That collapse is the whole
gap this exercise exists to close.

The gap comes from **argument values**, not from picking a branch. Write
`build` with an `if` instead and we're no better than ty:

```python
def build(kind):
    return JsonWriter() if kind == "json" else CsvWriter()

a = build("json")   # ty:  JsonWriter | CsvWriter
b = build("csv")    # us:  Values[JsonWriter, CsvWriter] — the same two, for both
```

We don't evaluate `kind == "json"`, so both calls get both arms, same as ty
— see [§ Not flow-sensitive](#-not-flow-sensitive--for-now-an-ifelse-is-a-union-never-a-choice)
below. That's the intended scope, not a gap to fix.

## The enum

```rust
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(in crate::interp) enum AbstractValue<'db> {     // private to the interpreter
    Function(Definition<'db>),                       // the `def`
    Closure { func: Definition<'db>, activation: ActivationId },   // 02-functions/04
    Class(Definition<'db>),                          // the `class`
    Instance { class: Definition<'db>, origin: OriginId },      // which object: next file
    BoundMethod { func: Definition<'db>, receiver: ValueId },
    Module(Module<'db>),
    Ty(Type<'db>),     // escape hatch: literals, stdlib returns, arithmetic
    Pending(DeferId),  // waiting on a recursive call: 02-functions/08
    Unknown,            // "we didn't ask" — distinct from Ty(Dynamic)
}

pub type Values<'db> = SmallVec<[AbstractValue<'db>; 2]>;
```

**Why `Definition`, not ty's `FunctionType` / `ClassLiteral`:** those two
can't be named outside `ty_python_semantic` — `ClassLiteral` is re-exported
`pub(crate)`, `FunctionType` not at all — so `pylspt` can't use them as
field types (`types/vnoc.rs` rule 3). `Definition<'db>` is public, 4
bytes, and is what the qname, docstring ID and project check start from
anyway. ty's public `Type::definition(db, env)` turns ty's answers into
these (`plan/03-call-tree/04` § "`lift`").

What it costs: a specialisation like `Box[Foo]` on a value ty handed you
becomes plain `Box` — a known gap, pinned by a fixture in `plan/03-call-tree/04`.

Five everyday shapes (function, class, instance, bound method, module), one
for nested functions (`Closure`), one escape hatch (`Ty`), one for recursion
(`Pending`), and one admission of defeat (`Unknown`). `shapes.py` in this
folder's `python/` is exactly the five everyday ones, one function each — do
the exercise below before reading further.

## Exercise

Open `python/shapes.py`. For each of `a, b, c, d, e` in `uses_all_five`,
write down which `AbstractValue` variant it becomes, **without** running
anything — this is a vocabulary check, not an inference problem.

<details><summary>Answer (open after you've written yours down)</summary>

| name | expression | variant |
|---|---|---|
| `a` | `plain` | `Function(plain)` — named, not called |
| `b` | `Thing` | `Class(Thing)` — named, not called |
| `c` | `Thing()` | `Instance { class: Thing, origin: New{site} }` — called |
| `d` | `c.method` | `BoundMethod { func: Thing.method, receiver: <interned c> }` |
| `e` | `json` | `Module(json)` |

</details>

**`Pending`** is the one variant you won't meet until recursion
([`02-functions/08`](../02-functions/08-recursion.md)): the value of a call
into a cycle that hasn't been resolved yet. It never leaves its walk.

## `Values`, not `AbstractValue` — and why `SmallVec<[_; 2]>`

A single expression can evaluate to *more than one* value — `x if c else y`,
or a union ty hands you. `Values` is the plural, kept as a small, enumerable
list rather than collapsed. `SmallVec<[_; 2]>` because the overwhelmingly
common case is exactly one value, and you want that case to avoid a heap
allocation entirely. This is the type every function in `02-functions/` and
`03-classes/` returns — get comfortable reading it as "the possibilities at
this point in this path", not "the type".

**Empty `Values` is not `Unknown`.** It means no value reaches this point:
every path to it raised, or it waits on a cycle that hasn't produced anything
yet. `Unknown` means a value exists and nobody knows what it is.

> ⚠ The temptation to flatten `Values` back into one `ty::Type::Union` the
> moment it's inconvenient is how this whole design quietly turns back into
> `ty_ide::outgoing_calls` (`plan/03-call-tree/02`). Keep it a `Vec`.

## ★ Not flow-sensitive — for now: an `if`/`else` is a union, never a choice

State this rule explicitly, because every other file in this exercise
leans on it without re-deriving it: **this design does not evaluate
conditions.** `if`/`else`, `and`/`or`, a ternary, a `match` — none of them
are ever asked "which branch does this path actually take." Both (or all)
arms are evaluated and their results **unioned** into one `Values`,
unconditionally, every time:

```python
def build(kind):
    return JsonWriter() if kind == "json" else CsvWriter()

build("json")   # resolves to Values[Instance{JsonWriter}, Instance{CsvWriter}]
                 # — BOTH, even though "json" is a literal and the answer
                 #   is, in principle, fully knowable
```

This is **current behaviour**, not a shortcut invented for this exercise —
your Jedi driver doesn't evaluate conditions either
(`plan/03-call-tree/01`'s "what it does *not* do" list: *"No branch
analysis. `if cond: f() else: g()` yields **both** `f` and `g` as children
of the same frame. Conditions are never evaluated, paths never split on
them."*). Porting it line-by-line already gets this right; the risk is
only in "improving" it by adding condition evaluation later without
noticing that's a scope change, not a bug fix.

**"For now" is doing real work in that sentence.** Nothing about
`AbstractValue`/`Values` *prevents* adding real condition evaluation later
— a literal-valued condition (`if True:`, `if kind == "json":` where `kind`
is itself a traced literal) is exactly the kind of thing this design could
narrow, the way `ty` narrows on `isinstance` and literal checks inside one
function. But
that is a **deliberate, separate extension**, not today's baseline, and
it's exactly the line `plan/03-call-tree/01` draws on purpose:

> **Union at branches, precision at bindings.** That is the whole design
> rule. Path conditions are what make the search explode — deciding
> *which* branch runs requires evaluating conditions, which requires
> modelling comparisons, which requires modelling integers, and you are
> writing a symbolic executor.

**Ignoring conditions is not ignoring order.** On one path, a later
assignment still replaces an earlier one:

```python
w = JsonWriter()
w.write(1)        # JsonWriter.write only
w = CsvWriter()
w.write(2)        # CsvWriter.write only
```

Each arm of a branch starts from the state before the branch, and the arms are
joined after it. Combining every value ever assigned to a name would be a
different, much blunter policy, and this design doesn't do it
(`plan/03-call-tree/03` § "Precision policy").

So: **if you hit an `if`/`else` (or any other branch) while building this,
union the arms and move on.** Don't special-case a literal condition to
"help" — that's the one shortcut this design explicitly defers.
`02-functions/05-returns-and-value-flow.md`'s `two_returns_union` fixture
and `03-classes/01-constructing-an-instance.md`'s factory example are both
built to exercise exactly this.

## `Ty(Type<'db>)` — the escape hatch, used constantly

You will reach for `Ty(...)` far more than any other variant: every literal,
every stdlib return, every arithmetic expression, every comprehension lands
here. It is not a failure case — it is "this expression doesn't carry
path-specific information, so hand it to the oracle." It is also why most
values are cheap to fingerprint ([`01-summaries/02`](../01-summaries/02-the-key.md)).

## `Unknown` vs `Ty(Dynamic)` — two different kinds of "no idea"

```rust
Unknown              // "we didn't ask" — gave up before consulting ty
Ty(Type::Dynamic(_)) // "we asked, and ty itself doesn't know"
```

Keep them distinct, even though both usually render the same way downstream.
When your tree has an unexpected gap, which one you find in a debug dump
tells you *where* the gap came from — your binding code, or ty's inference.
Collapsing them trades a one-line `match` for a debugging session.

## Sizing: the rule that keeps everything else fast

```rust
// Same conditions ty uses for its own 16-byte assert on `Type`.
#[cfg(all(not(debug_assertions), target_pointer_width = "64"))]
#[test]
fn value_is_small() {
    assert!(std::mem::size_of::<AbstractValue>() <= 16);
}
```

`AbstractValue` gets copied, cloned into environments, and passed by value
millions of times on a real project. Every variant above holds salsa
handles (`u32`-ish) or small enums of them — **never** a `String` or a `Vec`
directly. The moment one variant owns a heap allocation, every `Env` clone
pays for it. This is why `Instance` carries an `OriginId` (a `u32` newtype)
rather than, say, the constructing arguments themselves (see the next file).

⚠ **16 is a goal, not a given.** `Ty(Type)` holds a whole ty `Type`, which
is exactly 16 bytes on its own (ty asserts it, `types.rs:11192`). The enum
only stays at 16 if rustc tucks its own tag into `Type`'s spare tag values —
likely, not promised. If the test fails, store the `Type` in a per-walk
arena and keep a `u32` `TyId` in the variant
([`plan/03-call-tree/04` § Sizing](../../../plan/03-call-tree/04-values.md#sizing)).
Run it with `cargo test --release value_is_small`; ty's assert is off in
debug builds, so a debug run can fail for reasons that aren't yours.

---

## Quick check

1. Why does `a if c else b` need to stay `Values` of length 2 instead of
   collapsing into one value?
2. Give one example where `Unknown` is the right answer and `Ty(Type::Dynamic)`
   would be wrong — i.e., where you genuinely never asked ty at all.
3. `AbstractValue::Class(Definition<'db>)` and `AbstractValue::Instance { class: Definition<'db>, .. }`
   both carry a class's `Definition`. What's the one-sentence difference in what
   they represent?
4. `build("json")` from the "not flow-sensitive" section above passes a
   **literal** string, so which branch runs is, in principle, fully
   determinable without any abstract interpretation at all — plain
   constant folding would do it. Does this design evaluate it anyway? What
   does it do instead, and is that a bug?

<details><summary>Answers</summary>

1. Because `c` is never evaluated (the "not flow-sensitive" rule above) —
   both `a` and `b` are reachable outcomes as far as this design is
   concerned, so both must be kept, enumerably, for whatever consumes this
   expression's value to build a correct subtree for each.
2. An unannotated parameter whose key holds nothing for it: `opaque` in
   `00-values/04`'s `run_unresolved`, when `run_unresolved` is an entry. The
   key says `Unknown`, there is no annotation to fall back to, and nobody
   asks ty what a parameter "is". `Ty(Type::Dynamic)` would be wrong there
   because it claims ty *was* asked and came back uninformative, which isn't
   what happened. (`needs_the_annotation`'s `h` is not an example: its key
   says `Unknown` too, but the walk starts it from its annotation.)
3. `Class(C)` represents the class object **itself** — what you get by
   naming `C` without calling it, and what you call to produce instances.
   `Instance{class: C, ..}` represents **one object of that class**,
   produced by an actual `C()` call at a specific, trackable site. The
   `Definition` in both is "which class"; `Instance` additionally carries
   *which construction*.
4. No — it does **not** evaluate the condition, even though `"json"` is a
   literal and a smarter design could. It unions both arms regardless,
   exactly like it would for a genuinely unknown `kind`. This is **not a
   bug** — it's the explicit, documented scope limit from this section:
   union at branches, precision at bindings, full stop, for now. Narrowing
   literal conditions is a real, separate future extension, not something
   silently missing today.

</details>

(The whole-folder [`exam.md`](exam.md) / [`answers.md`](answers.md) cover
different, cross-cutting questions — these quick checks are per-file.)

---

→ Next: [`02-origins.md`](02-origins.md)
