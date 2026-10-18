# 00.01 — `AbstractValue`: what flows through the interpreter

Before function calls, before classes: the one type everything else is built
from. Get this wrong and both the `01-functions/` and `02-classes/` folders
are awkward; get it right and they mostly write themselves.

**Reading:** [`plan/03-call-tree/04-value-domain.md`](../../../plan/03-call-tree/04-value-domain.md)
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
def build(kind):
    return JsonWriter() if kind == "json" else CsvWriter()

a = build("json")
b = build("csv")
```

`ty`'s answer for `a` and `b` is the **same** `Type::Union(JsonWriter | CsvWriter)`
in both cases — it has no way to remember which branch *this* call took. Two
values that your Jedi driver tells apart collapse into one `ty::Type`. That
collapse is the whole gap this exercise exists to close.

## The enum

```rust
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum AbstractValue<'db> {
    Function(FunctionType<'db>),
    Class(ClassLiteral<'db>),
    Instance { class: ClassLiteral<'db>, origin: OriginId },
    BoundMethod { func: FunctionType<'db>, receiver: ValueId },
    Module(Module<'db>),
    Ty(Type<'db>),     // escape hatch: literals, stdlib returns, arithmetic
    Unknown,            // "we didn't ask" — distinct from Ty(Dynamic)
}

pub type Values<'db> = SmallVec<[AbstractValue<'db>; 2]>;
```

Five "real" shapes, one escape hatch, one admission of defeat. `shapes.py`
in this folder's `python/` is exactly these five, one function each — do the
exercise below before reading further.

## Exercise

Open `python/shapes.py`. For each of `a, b, c, d, e` in `uses_all_five`,
write down which `AbstractValue` variant it becomes, **without** running
anything — this is a vocabulary check, not an inference problem.

<details><summary>Answer (open after you've written yours down)</summary>

| name | expression | variant |
|---|---|---|
| `a` | `plain` | `Function(plain)` — named, not called |
| `b` | `Thing` | `Class(Thing)` — named, not called |
| `c` | `Thing()` | `Instance { class: Thing, origin: <fresh> }` — called |
| `d` | `c.method` | `BoundMethod { func: Thing.method, receiver: <interned c> }` |
| `e` | `json` | `Module(json)` |

</details>

## `Values`, not `AbstractValue` — and why `SmallVec<[_; 2]>`

A single expression can evaluate to *more than one* value — `x if c else y`,
or a union ty hands you. `Values` is the plural, kept as a small, enumerable
list rather than collapsed. `SmallVec<[_; 2]>` because the overwhelmingly
common case is exactly one value, and you want that case to avoid a heap
allocation entirely. This is the type every function in `01-functions/` and
`02-classes/` returns — get comfortable reading it as "the possibilities at
this point in this path", not "the type".

> ⚠ The temptation to flatten `Values` back into one `ty::Type::Union` the
> moment it's inconvenient is how this whole design quietly turns back into
> `ty_ide::outgoing_calls` (`plan/03-call-tree/02`). Keep it a `Vec`.

## `Ty(Type<'db>)` — the escape hatch, used constantly

You will reach for `Ty(...)` far more than any other variant: every literal,
every stdlib return, every arithmetic expression, every comprehension lands
here. It is not a failure case — it is "this expression doesn't carry
path-specific information, so hand it to the oracle." [`06-scaling-budget-and-memoization.md`](06-scaling-budget-and-memoization.md)
revisits why that's good for more than just code size.

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
#[test]
fn value_is_small() {
    assert!(std::mem::size_of::<AbstractValue>() <= 16);
}
```

`AbstractValue` gets copied, cloned into environments, and passed by value
millions of times on a real project. Every variant above is a salsa handle
(`u32`-ish) or a pair of them — **never** a `String` or a `Vec` directly.
The moment one variant owns a heap allocation, every `Env` clone pays for it.
This is why `Instance` carries an `OriginId` (a `u32` newtype) rather than,
say, the constructing arguments themselves — see the next file.

---

## Quick check

1. Why does `a if c else b` need to stay `Values` of length 2 instead of
   collapsing into one value?
2. Give one example where `Unknown` is the right answer and `Ty(Type::Dynamic)`
   would be wrong — i.e., where you genuinely never asked ty at all.
3. `AbstractValue::Class(ClassLiteral<'db>)` and `AbstractValue::Instance { class: ClassLiteral<'db>, .. }`
   both carry a `ClassLiteral`. What's the one-sentence difference in what
   they represent?

(Answers in this folder's [`answers.md`](answers.md).)

---

→ Next: [`02-origin-and-identity.md`](02-origin-and-identity.md)
