# 08 — Classes, MRO and attributes

**Goal:** you can emit the `base_classes` field for any class, matching what
`mro_resolver.py` produces today — and you know exactly where attribute
resolution stops working for your purposes.

This exercise contains the biggest correction to the plan in the whole folder.
Read the API section carefully.

---

## Read first

- `plan/02-mapping/03-jedi-mro-to-ty-mro.md` — the chapter this exercise ports
- `plan/01-crates/04-public-vs-private-api.md` — re-read after step 2, it will
  mean more

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

### ⚠ …but you cannot call it

**[verified at `ac201b8`]**:

```rust
pub(crate) struct Mro<'db>(Box<[ClassBase<'db>]>);              // mro.rs:38
pub(crate) fn ClassLiteral::iter_mro(self, db) -> MroIterator   // class.rs:644
pub(super)  fn ClassType::iter_mro(self, db)   -> MroIterator   // class.rs:1362
pub(crate) fn ClassLiteral::explicit_bases(self, db) -> Box<[Type]>  // class.rs:1094
```

**All of it is private.** `plan/02-mapping/03` sketches
`literal.iter_mro(model.db(), &model.program_environment(), None)` and marks the
accessor name `[check]`. The name is not the problem — the **visibility** is.
That snippet cannot compile from a git dependency, at any accessor name.

So the plan's "smallest port, biggest speedup per line, 77 lines → ~30" needs
revisiting. It is still the smallest port. It is just not a one-liner.

---

## The API that *is* public

```rust
use ty_python_semantic::{
    type_hierarchy_prepare, type_hierarchy_supertypes, TypeHierarchyClass,
};

pub fn type_hierarchy_supertypes<'db>(
    db: &'db dyn Db,
    env: &ProgramEnvironment<'db>,
    ty: Type<'db>,
) -> Vec<TypeHierarchyClass<'db>>;

pub struct TypeHierarchyClass<'db> {
    pub name: Name,                  // just the class name — NOT qualified
    pub file: ResolverFile<'db>,     // → file_to_module → module name
    pub full_range: TextRange,       // the class definition header
    pub selection_range: TextRange,  // the class name
}
```

Three things about it that you must know before designing around it
**[verified, `ide_support.rs:1896-1922`]**:

1. **It returns the DIRECT bases, one level.** Internally it is
   `class_literal.explicit_bases(db)` mapped to hierarchy info. It is a
   type-hierarchy *tree view* API — you recurse it yourself.
2. **`object` is special-cased.** Asking about `object` returns an empty vec.
   A class with no explicit bases gets an implicit `object` supertype added.
3. **The name is not qualified.** You build `pkg.mod.Base` yourself, from
   `file` + the ranges + exercise 06's `qualified_name`.

### The consequence: DAG walk ≠ MRO

Jedi's `py__mro__()` returns a **linearised** sequence — C3 order, ending in
`builtins.object`. Recursing `type_hierarchy_supertypes` gives you the
inheritance **DAG**, not a linearisation.

For single inheritance they coincide. For `Diamond(Left, Right)` they do not:

```
C3 (what jedi gives you):        Diamond, Left, Right, Base, object
naive depth-first recursion:     Diamond, Left, Base, Right, Base, object
                                                  ^^^^         ^^^^ duplicated,
                                                                    and Base is
                                                                    too early
```

You have three options, and this is a real decision:

| option | cost | when it is right |
|---|---|---|
| **Implement C3 yourself** over the base DAG | ~30 lines, well-specified algorithm | you need `base_classes` to match `py__mro__()` exactly |
| **Emit the DAG, deduped, depth-first** | trivial | if v-noc only ever asks "is X in the bases", order does not matter |
| **Take Option A** (vendored workspace) and call `iter_mro` | a fork to maintain | if you need ty's exact handling of `Protocol`/`Generic`/metaclass edge cases |

**Find out which one you need before you build it** — from the golden files
(M0), not from reasoning. If nothing downstream depends on order, option 2 is
correct and you have just saved yourself the third option's maintenance cost.
That is a decision the plan could not make for you, because it depends on data
you have and it did not.

C3 is genuinely small: take the class, merge the linearisations of its bases
plus the list of bases, repeatedly picking the first head that appears in no
other list's tail. Python's own documentation of the algorithm is enough to
implement it, and your inputs are small.

---

## The fixtures

```
python/
├── hierarchy.py .... diamond, Protocol, ABC, Generic, specialised base,
│                     nested class, and a class with no bases
└── attributes.py ... `self.handler.handle()` — the shape from
                      plan/03-call-tree/06, with two construction paths
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

### Step 2 — direct bases

For each class, call `type_hierarchy_supertypes` and print the results.

Check against your predictions:

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

Produce `base_classes` for every class in `hierarchy.py`, however you decided in
the options table. Then check `Diamond` against what Python itself says:

```bash
python3 -c "import hierarchy; print([c.__module__ + '.' + c.__qualname__ for c in hierarchy.Diamond.__mro__])"
```

Running the real interpreter as your oracle is legitimate and fast. Jedi is
modelling CPython here, so CPython is the ground truth for order.

### Step 5 — attributes, and where they stop

Switch to `attributes.py`. For each of these, print the type of the attribute
expression and of the call's callee:

| expression | expect |
|---|---|
| `self.fallback.handle(...)` in `safe` | resolves — assigned from a literal `Handler()` |
| `self.default.handle(...)` in `inherited` | resolves through the class body |
| `self.handler.handle(...)` in `dispatch` | ← **the interesting one** |

The public tool is:

```rust
ty_python_semantic::types::ide_support::static_member_type_for_attribute(
    model, attribute: &ast::ExprAttribute
) -> Option<Type>
```

For `dispatch`, ty must consider both constructions (`build_loud` passes a
`LoudHandler`, `build_quiet` passes a `Handler`), so you get both or you get
something widened. Correct, and not what the call tree needs — under
`use_loud → build_loud → dispatch`, the answer must be `LoudHandler.handle`.

You saw this shape in exercise 07 with a parameter. Now you have seen it with an
**attribute**, which is worse: the value was stored on an object in one frame and
read in another, so even a per-call environment is not enough — you need object
identity and per-object state. That is `plan/03-call-tree/06` and `/10`, and it
is the single strongest argument for the plan's Option A + C recommendation.

**Do not build it.** Write down, in three sentences, what would be needed. Then
read `plan/03-call-tree/06` and compare.

---

## Traps

- **Trusting the plan's MRO snippet.** It does not compile on Option B; the
  accessor is private, not merely renamed.
- **Assuming supertypes are the MRO.** One level, DAG, not linearised.
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

- [ ] every class in `hierarchy.py` yields a `ClassLiteral` (or you know why not)
- [ ] direct bases printed and checked against the table
- [ ] all four parity cases from `plan/02-mapping/03` answered with real output
- [ ] you decided DAG vs C3, and wrote down what evidence decided it
- [ ] `Diamond`'s output compared against CPython's `__mro__`
- [ ] you can name the three attribute cases in `attributes.py` and which resolves
- [ ] you wrote your three sentences on what `dispatch` would need

---

→ [`exam.md`](exam.md), then [`../09-ide-layer/README.md`](../09-ide-layer/README.md)
