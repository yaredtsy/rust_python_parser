# 08 — Classes, MRO and attributes

**Goal:** you can emit the `base_classes` field for any class, matching what
`mro_resolver.py` produces today — and you know exactly where attribute
resolution stops working for your purposes.

This exercise contained the biggest correction to the plan in the whole folder
— and then the fork reversed it. Read the DECIDED section carefully: what used
to be "you cannot call ty's MRO" is now "you call it through one wrapper you
own".

---

## Read first

- `plan/02-mapping/03-jedi-mro-to-ty-mro.md` — the chapter this exercise ports
- `plan/01-crates/04-public-vs-private-api.md` — the fork decision and the
  wrapper discipline. Read it **before** step 1 now, not after step 2

---

## The mental model

### What you do today

```python
script = self.project_manager.get_script(file_path)   # new Project + Script + reparse
defs   = script.infer(line=line, column=column)        # full inference to find the class
for infer in defs[0]._name.infer():                    # private API
    if hasattr(infer, "py__mro__"):
        contexts = infer.py__mro__()                   # private API
```

Plus `service._get_name_column`, which does `line.find(node.name)` to *guess*
where the class name starts. Once per class. On a 40-class file that is 40
project constructions and 40 reparses.

### Why ty should make this nearly free

You already have the `StmtClassDef` node from exercise 02. Its inferred type is
a `ClassLiteral`. MRO is a property of the class, computed once, salsa-cached —
no position arithmetic, no `infer()`, no string searching. `_get_name_column`
deletes entirely.

`ty_python_semantic/src/types/mro.rs` implements real C3 linearisation, handles
`Protocol`, `Generic`, `NamedTuple`, `Enum` and metaclasses, and degrades
gracefully on inheritance cycles rather than panicking — which matters, because
your driver must not die on pathological input.

### ⚠ …and until now you could not call it

**[verified at `ac201b8`]** — every entry point is private:

```rust
pub(crate) struct Mro<'db>(Box<[ClassBase<'db>]>);              // mro.rs:38
pub(crate) fn ClassLiteral::iter_mro(self, db) -> MroIterator   // class.rs:644
pub(super)  fn ClassType::iter_mro(self, db)   -> MroIterator   // class.rs:1362
pub(crate) fn ClassLiteral::explicit_bases(self, db) -> Box<[Type]>  // class.rs:1094
```

`plan/02-mapping/03` sketches `literal.iter_mro(...)` and marks the accessor
name `[check]`. The name was never the problem; the **visibility** was. From a
git dependency that snippet cannot compile at any accessor name.

That is what the fork changes.

---

## ★★ DECIDED — fork ty, and reach `iter_mro` through one wrapper

`plan/01-crates/04` recommended Option A (a workspace fork) and this exercise
was written against Option B (git dependency, public API only). **The decision
is now A**, with a discipline that makes it cheaper than the plan assumed:

> **One file in the fork, wrappers only, no logic.**
> `crates/ty_python_semantic/src/types/vnoc.rs`, plus one line — `pub mod
> vnoc;` — in `types.rs`. Every wrapper calls exactly one internal function and
> returns public types. Your C3 decisions, your IDs, your interpreter all stay
> in `pylspt`.

Why that file, in that module: Rust privacy is **module-scoped**, so a child
module sees its ancestors' private items. `Type::member` and friends are
private to `crate::types`, so a `crate::types::vnoc` can call them and nothing
else has to change. `pub mod types;` is already declared in `lib.rs`
**[verified, `lib.rs:68`]**, so `ty_python_semantic::types::vnoc::*` is reachable
from `pylspt` with **no edit to `lib.rs`** and **no visibility flipped** on any
existing item.

That last point is what keeps rebases cheap. The plan's Option A proposed
"small, surgical visibility patches" — every one of those is a modified line in
an upstream file, and modified lines are what conflict. An added file plus one
`pub mod` line conflicts with almost nothing.

### The two wrappers you need first

```rust
//! crates/ty_python_semantic/src/types/vnoc.rs
//! Public wrappers for V-NOC. One internal call each, no logic.

use super::{ClassBase, ProgramEnvironment, Type};
use crate::Db;
use ty_python_core::definition::Definition;

/// The MRO of `ty`, without the class itself, as definitions.
///
/// `None` holds a base's *position* when ty cannot tie it to a definition
/// (`Any`, `Unknown`, `Protocol`, `Generic`), instead of dropping it — losing
/// a slot silently would shift every base after it.
pub fn class_mro<'db>(db: &'db dyn Db, ty: Type<'db>) -> Vec<Option<Definition<'db>>> {
    let Some(class) = ty.to_class_type(db) else {
        return Vec::new();
    };
    class
        .iter_mro(db)
        .skip(1)                                  // [0] is the class itself
        .map(|base| match base {
            ClassBase::Class(c) => c.class_literal(db).definition(db),
            _ => None,
        })
        .collect()
}

/// `getattr(<value of type ty>, name)`, as ty resolves it — descriptors,
/// properties, metaclasses and `__getattr__` included.
pub fn member<'db>(
    db: &'db dyn Db,
    env: &ProgramEnvironment<'db>,
    ty: Type<'db>,
    name: &str,
) -> Option<Type<'db>> {
    ty.member(db, env, name).ignore_possibly_undefined()
}
```

Every item that touches is verified at `ac201b8`:

| used | visibility | where | reachable from `types::vnoc`? |
|---|---|---|---|
| `Type::member(db, env, name) -> PlaceAndQualifiers` | **private** to `types` | `types.rs:4929` | ✓ child module |
| `Type::to_class_type(db) -> Option<ClassType>` | private to `types` | `types.rs:2570` | ✓ |
| `ClassType::iter_mro(db) -> MroIterator` | `pub(super)` | `class.rs:1362` | ✓ `pub(in crate::types)` |
| `ClassType::class_literal(db)` | `pub(crate)` | `class.rs:1199` | ✓ |
| `ClassLiteral::definition(db) -> Option<Definition>` | `pub(crate)` | `class.rs:913` | ✓ |
| `PlaceAndQualifiers::ignore_possibly_undefined()` | `pub(crate)` | `place.rs:923` | ✓ |
| `Type`, `ProgramEnvironment` | **`pub`** | `types.rs:1629`, `:15` | ✓ and nameable from `pylspt` |

> **Use `to_class_type`, not `let Type::ClassLiteral(c) = ty`.** `Type` has two
> class-ish variants: `ClassLiteral` (`types.rs:1676`) and `GenericAlias`
> (`:1678`). `to_class_type` (`types.rs:2570`) maps both; matching only
> `ClassLiteral` returns an empty MRO for `IntBox` and for any `Box[int]`,
> which is exactly the distinction step 1 below asks you to notice.

`ClassBase`, `ClassType` and `ClassLiteral` are all `pub(crate)`
**[verified, `types.rs:75`, `:118`]**, so none of them may appear in a
wrapper's *signature*. Both wrappers above return public types only — that is
not a stylistic choice, it is what makes them callable from `pylspt` at all.

### `MroIterator` yields the class first

`MroIterator::next` returns `first_element()` before anything else
(`mro.rs:691-699`, `:623` **[verified]**), and `first_element` is the class
itself. Hence `.skip(1)`: the wire shape's `base_classes` is the MRO *without*
the class, and Jedi's `py__mro__()[1:]` is the same slice.

### The six rules that keep the fork cheap

1. **One file.** `types/vnoc.rs`, plus `pub mod vnoc;` in `types.rs`. It must
   live under `types` — that is where the private items are.
2. **Wrappers only.** One internal call each, public types out, no logic.
3. **Add a wrapper only when you need it.** Start with these two.
4. **Pin the fork and rebase on purpose.** After a rebase every compile error
   lands in one file.
5. **One test per wrapper, in `pylspt`.** Compile errors catch signature
   changes; only a test catches a behaviour change.
6. **Put `pylspt` in the fork's workspace**, so `salsa` and every shared
   dependency come from one place with `workspace = true`.

Ruff — ty included — is **MIT** (`Cargo.toml:13`, `LICENSE` at the root
**[verified]**), so a private fork inside a closed product is fine. Keep the
licence file.

---

## What this deletes

### Your own C3 goes away

`src/mro.rs` recurses `type_hierarchy_supertypes` and runs its own C3 merge,
because `iter_mro` was unreachable. With the wrapper it is ty's MRO, and three
problems go with it:

| your C3 | ty's `iter_mro` |
|---|---|
| inheritance cycles: your merge degrades to "take the first head anyway" | handled upstream, tested upstream |
| inconsistent hierarchies: you pick an order, CPython raises | ty's documented fallback, same answer ty's own diagnostics use |
| `Protocol`, `Generic`, `NamedTuple`, `Enum`, metaclasses | all in `iter_mro`; none in yours |
| a `TypeHierarchyClass` → range → `covering_node` → `Definition` round trip per base | `ClassBase` → `Definition`, one hop |

What stays in `pylspt`: turning each `Definition` into a **qualified name**
(exercise 06) or into an **ID** (exercise 02 object 6), deciding what to emit
for a `None` slot, and the wire shape. That is the logic that is *yours*; the
linearisation never was.

### The `member` component shrinks to a call

`plan/03-call-tree/07` once budgeted a `member.rs` that reimplements attribute lookup
against a receiver you choose: MRO walk, instance vs class attributes,
descriptors, properties, `__getattr__`, metaclasses. That is a second copy of
Python's semantics, and it will disagree with ty wherever ty is right.

Your own C3 is the small version of that problem. `member` is the big one, and
the wrapper removes it.

### What does *not* change

The reason for Option **C** is untouched: ty's `Type` still has no notion of
"this specific object on this specific path", so your `AbstractValue` domain
and the frame-sensitive environment stay exactly as planned. The fork buys you
**primitives** — MRO, member lookup, call binding — not a value model.
`attributes.py:dispatch` (step 5) still needs the interpreter.

---

## The old decision, kept for the record

Before the fork, `base_classes` had to be built from the one public entry point:

```rust
pub fn type_hierarchy_supertypes<'db>(
    db: &'db dyn Db, env: &ProgramEnvironment<'db>, ty: Type<'db>,
) -> Vec<TypeHierarchyClass<'db>>;
```

Three facts about it **[verified, `ide_support.rs:1896-1922`]**, still true and
still worth knowing, because it is the fallback if the fork ever becomes
untenable:

1. **It returns the DIRECT bases, one level** — internally
   `class_literal.explicit_bases(db)`. It is a tree-view API; you recurse it.
2. **`object` is special-cased.** Asking about `object` returns an empty vec; a
   class with no explicit bases gets an implicit `object` supertype.
3. **The name is not qualified.** You build `pkg.mod.Base` yourself.

And the consequence that forced the old three-way choice — recursing a DAG is
not a linearisation:

```
C3 (what jedi gives you):        Diamond, Left, Right, Base, object
naive depth-first recursion:     Diamond, Left, Base, Right, Base, object
                                                  ^^^^         ^^^^ duplicated,
                                                                    and too early
```

The options table said: implement C3 yourself (~30 lines), emit the deduped DAG,
or fork. **The answer is now the third one**, and it is the one the plan
recommended before this exercise's API survey talked it out of.

---

## The fixtures

```
python/
├── hierarchy.py .... diamond, Protocol, ABC, Generic, specialised base,
│                     nested class, and a class with no bases
└── attributes.py ... `self.handler.handle()` — the shape from
                      plan/03-call-tree/07, with two construction paths
```

---

## Build it

### Step 1 — get to a `ClassLiteral`

From a `StmtClassDef` node, get `Type::ClassLiteral`. `HasType` is implemented
for statement types, but confirm `StmtClassDef` is among them at your revision
— if not, go through the class **name** (`ExprName`) or through
`HasDefinition` → `Definition` → type.

Print the variant you got for each class in `hierarchy.py`. `Box` and `IntBox`
may not both be `ClassLiteral` — one may be a `GenericAlias`. That distinction
is the source of the `Base[int]` vs `Base` question below.

### Step 2 — the MRO, and the bases it is built from

Call `vnoc::class_mro` for each class and print the `Definition`s — that is the
answer. Then, to understand what it did, call the public
`type_hierarchy_supertypes` for the same class and print its **direct** bases.

The second list is one level of the DAG; the first is the linearisation. Seeing
them side by side on `Diamond` is the whole lesson of this exercise in two
lines of output.

Check the direct bases against your predictions:

| class | expected direct bases |
|---|---|
| `Base` | `object` |
| `Left` | `Base` |
| `Diamond` | `Left`, `Right` |
| `Plain` | `object` (implicit) |
| `Box` | `Generic` — or `Generic[T]`? |
| `IntBox` | `Box` — or `Box[int]`? |
| `Runner` | `Protocol` |
| `Abstract` | `ABC` |
| `Outer.Inner` | `Base` |

Then try `type_hierarchy_supertypes` on `object` itself and confirm you get an
empty vec, not a crash.

### Step 3 — qualified names, and the parity table

Turn each `TypeHierarchyClass` into a Jedi-style qualified name using
`file_to_module` (exercise 05) and your `qualified_name` (exercise 06).

`plan/02-mapping/03` lists four cases to test. Now you can actually run them:

| case | Jedi | yours | verdict |
|---|---|---|---|
| `object` | `builtins.object` | | should match |
| nested class `Outer.Inner` | `mod.Outer.Inner` | | **test this** — ty gives the immediate name only |
| class in `pkg/__init__.py` | `pkg.C` | | should match |
| `Box[int]` | `mod.Box` | | strip the specialisation, or not — match current output |

`TypeHierarchyClass` gives you `name` and `full_range`. For the nested case you
need the *scope chain*, which the name alone does not carry — so go from the
range back to the `Definition` and reuse exercise 06's ancestor walk. This is
where that exercise's failure-mode list pays off.

### Step 4 — the full sequence

Produce `base_classes` for every class in `hierarchy.py` — `vnoc::class_mro`,
then each `Definition` to a qualified name, then your decision for the `None`
slots. Then check `Diamond` against what Python itself says:

```bash
python3 -c "import hierarchy; print([c.__module__ + '.' + c.__qualname__ for c in hierarchy.Diamond.__mro__])"
```

Running the real interpreter as your oracle is legitimate and fast. Jedi is
modelling CPython here, so CPython is the ground truth for order — and now so
is ty, which is the point of calling its linearisation instead of writing one.

**The `None` slots are yours to decide.** `class_mro` yields `None` where ty
has a base it cannot tie to a definition — `Any`, `Unknown`, `Protocol`,
`Generic`. Dropping them silently shifts every later base; emitting a
placeholder keeps the positions honest. `hierarchy.py`'s `Runner(Protocol)` and
`Box(Generic[T])` are the two fixtures that produce one, so decide against real
output, and write the decision down next to `DECORATORS_IN_RANGE` as a thing
the wire shape depends on.

### Step 5 — attributes, and where they stop

Switch to `attributes.py`. For each of these, print the type of the attribute
expression and of the call's callee:

| expression | expect |
|---|---|
| `self.fallback.handle(...)` in `safe` | resolves — assigned from a literal `Handler()` |
| `self.default.handle(...)` in `inherited` | resolves through the class body |
| `self.handler.handle(...)` in `dispatch` | ← **the interesting one** |

There are now **two** tools, and the difference between them is the whole
reason for the fork:

```rust
// public, AST-shaped: ty picks the receiver
ty_python_semantic::types::ide_support::static_member_type_for_attribute(
    model, attribute: &ast::ExprAttribute
) -> Option<Type>                                        // ide_support.rs:449

// your wrapper, type-shaped: YOU pick the receiver
ty_python_semantic::types::vnoc::member(db, env, ty, name) -> Option<Type>
```

The public one takes `x.y` and looks `y` up on **ty's** type for `x`. The
interpreter asks the other question — "the receiver is this type, because my
environment says so; what is `.handle` on it?" — and only the wrapper answers
it.

Run both on all three expressions. For `dispatch`, the public call must
consider both constructions (`build_loud` passes a `LoudHandler`, `build_quiet`
passes a `Handler`), so you get a union or something widened. Correct, and not
what the call tree needs — under `use_loud → build_loud → dispatch`, the answer
must be `LoudHandler.handle`.

Now call the wrapper with `LoudHandler`'s type as the receiver and watch it
answer exactly that. **The wrapper does not solve `dispatch`** — knowing *which*
receiver to pass is still the interpreter's job, and that is the environment,
the keys and the object identity from `plan/03-call-tree/07`, `/08` and `/09`. What
the wrapper removes is having to reimplement `.handle`'s lookup once you know.

You saw this shape in exercise 07 with a parameter. Now you have seen it with an
**attribute**, which is worse: the value was stored on an object in one frame and
read in another, so even a per-call environment is not enough — you need object
identity and per-object state. That is `plan/03-call-tree/07` and `/09`, and it
is the single strongest argument for the plan's Option A + C recommendation.

**Do not build it.** Write down, in three sentences, what would be needed. Then
read `plan/03-call-tree/07` and compare.

---

## Traps

- **Putting logic in the fork.** The wrapper is one call and a `map`. The
  moment a wrapper grows an `if`, that `if` is a line you will re-resolve on
  every rebase, in the file you are least able to test.
- **A `pub(crate)` type in a wrapper signature.** `ClassBase`, `ClassType` and
  `ClassLiteral` are all `pub(crate)` — a wrapper returning one compiles inside
  the fork and is uncallable from `pylspt`. Return `Definition`, `Type`, or
  something you own.
- **Matching only `Type::ClassLiteral`.** `Box[int]` is a `GenericAlias`; use
  `to_class_type`.
- **Assuming supertypes are the MRO.** `type_hierarchy_supertypes` is one
  level, DAG, not linearised — that is why it is not what `class_mro` calls.
- **Emitting an unqualified name.** `Base` instead of `mod.Base` breaks every
  downstream join, and looks fine in a log.
- **Forgetting `builtins.object` at the tail.** Jedi includes it. Check whether
  yours does, in the same position.
- **Treating a nested class's name as its qualified name.** Exercise 06's
  answer 10 all over again.
- **Descending into `object`'s methods.** `builtins` is not project code; the
  filter from exercise 05 handles it, but check that it actually fires.

---

## Done when

- [ ] `types/vnoc.rs` exists in the fork with **two** wrappers and no logic
- [ ] `pylspt` is a member of the fork's workspace, and `salsa` is
      `{ workspace = true }`
- [ ] one test per wrapper lives in `pylspt`, not in the fork
- [ ] `src/mro.rs`'s C3 merge and its `covering_node` round trip are deleted
- [ ] every class in `hierarchy.py` yields a `ClassLiteral` **or** a
      `GenericAlias`, and `class_mro` handles both
- [ ] direct bases printed and checked against the table
- [ ] all four parity cases from `plan/02-mapping/03` answered with real output
- [ ] you decided what a `None` MRO slot emits, from `Runner` and `Box`
- [ ] `Diamond`'s output compared against CPython's `__mro__`
- [ ] you can name the three attribute cases in `attributes.py` and which resolves
- [ ] you wrote your three sentences on what `dispatch` would need

---

→ [`exam.md`](exam.md), then [`../09-ide-layer/README.md`](../09-ide-layer/README.md)
