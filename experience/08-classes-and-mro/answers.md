# Answers 08 — Classes, MRO and attributes

---

**1.** Per class: builds a `jedi.Project`, builds a `Script` (reparsing the
file), runs `script.infer(line, column)` — full inference just to *locate* the
class — then reaches into `_name.infer()` and `py__mro__()`, both private API.

The non-obvious cost: `infer()` at a position is a full inference query, and it
is used here only as a lookup mechanism. You already know which class you mean;
you are paying for a search you do not need. Forty classes, forty projects,
forty reparses.

**2.** `line.find(node.name)` finds the **first** occurrence of the name text on
the line, so `class Foo(Foo_Base):` finds the `Foo` inside `Foo_Base`'s
position... actually it finds the class's own `Foo` first here, but
`class Base(BaseMixin)` style names, comments containing the name, or a
decorator on the same line all break it. Second reason: it points *just past*
the name and hopes that position infers to the class.

It disappears because you already hold the `StmtClassDef` node from your own
walk. There is no position to guess — you go from node to type directly.

**3.** C3 linearisation itself; `Protocol`, `Generic`, `NamedTuple` and `Enum`
special cases; metaclasses; and inheritance **cycles**, which it degrades on
rather than panicking. That last one matters most for you: your driver must not
die on pathological input (quirk 13).

---

**4.** The problem is **visibility**, not naming **[verified at `ac201b8`]**:

```rust
pub(crate) struct Mro<'db>                                   // mro.rs:38
pub(crate) fn ClassLiteral::iter_mro(self, db)               // class.rs:644
pub(super)  fn ClassType::iter_mro(self, db)                 // class.rs:1362
pub(crate) fn ClassLiteral::explicit_bases(self, db)         // class.rs:1094
```

Checking the *name* would not have found it because every candidate name in the
plan's list is real — they exist, they are just not reachable from outside the
crate. A `[check]` on the name implies "confirm the spelling"; the actual
question was "confirm you can call it at all". **When a plan marks an API
`[check]`, check visibility first and spelling second.**

**5.** See the table above: `pub(crate)`, `pub(crate)`, `pub(super)`,
`pub(crate)`. None reachable from a git dependency.

**6.** `ty_python_semantic::type_hierarchy_supertypes(db, env, ty) ->
Vec<TypeHierarchyClass>`.

It returns the **direct explicit bases, one level deep** — internally
`explicit_bases` mapped to hierarchy info. Three surprises **[verified,
`ide_support.rs:1896-1922`]**:

1. Asking about `object` returns an **empty** vec (special-cased, not an error).
2. A class with no explicit bases gets an **implicit `object`** added.
3. `TypeHierarchyClass.name` is the bare class name — **not qualified**.

---

**7.**

```
C3:                Diamond, Left, Right, Base, object
naive depth-first: Diamond, Left, Base, Right, Base, object
```

Two things wrong with the second: `Base` is **duplicated**, and it appears
**before `Right`** — which inverts the method resolution order. If anything
downstream answers "which `run` wins", the naive order gives `Base.run` where
Python gives `Right.run`.

**8.**

| pick | evidence |
|---|---|
| deduped DAG | golden `base_classes` output is only ever *searched*, never indexed by position, and no diamond in the corpus changes meaning |
| implement C3 | the goldens contain a diamond whose order differs from a DAG walk, or anything downstream indexes `base_classes[0]` |
| fork (Option A) | you find a `Protocol`/`Generic`/metaclass case where your own C3 disagrees with ty's and you cannot reproduce ty's rule |

The evidence comes from the **M0 golden files** —
`plan/04-build/02-milestones.md`, "before writing any Rust". This is a concrete
instance of why M0 is first: it converts a design argument into a lookup.

**9.** Grep the v-noc side for what consumes `base_classes`. If every use is
`in`, `any()`, or set membership, order is unobservable. If anything does
`[0]` or compares sequences, it is observable.

Failing that: run both implementations over the corpus and diff. If DAG-order
and C3-order produce identical JSON for all 200 files, the question is moot for
your data, and you can revisit it if a diamond ever appears.

---

**10.** Report your own output. Expected shapes:

| case | Jedi | expected from yours |
|---|---|---|
| `object` | `builtins.object` | should match, if you resolve the module of the vendored `builtins.pyi` |
| `Outer.Inner` | `hierarchy.Outer.Inner` | **`hierarchy.Inner` unless you walk ancestors** |
| `IntBox`'s base | `hierarchy.Box` | may render `Box[int]` — decide and match |
| `Runner`'s base | `typing.Protocol` | should match |

The `object` row has a subtlety worth noticing: `builtins` resolves to a
**vendored typeshed** file with no system path (exercise 05). If your
`qualified_name` assumes a system path, this is where it returns `None`.

**11.** Because `name` is the immediate name — `Inner` — and the qualified name
needs the enclosing `Outer`. The file tells you the module; nothing in
`TypeHierarchyClass` tells you the scope chain.

What to do: use `full_range` (or `selection_range`) to find the class's
`Definition` in that file, then walk `ancestor_scopes` as in exercise 06. You
already wrote that function; this is its second caller, which is exactly why
exercise 06's answer 17 asked you to enumerate its failure modes.

**12.** `Box`'s base is `Generic` — likely a `SpecialForm` or `KnownInstance`
rather than a plain `ClassLiteral`. `IntBox`'s base is `Box[int]`, which is a
**`GenericAlias`**, not a `ClassLiteral`.

That is why step 1 asks you to print the variant: `extract_class_literal` inside
`type_hierarchy_supertypes` filters to class literals, so what survives and how
it is named depends on the variant. Jedi reports `mod.Box` — the origin class
without the specialisation — so if you get `Box[int]`, strip it. Do it
deliberately and note it, because "strip the specialisation" is a lossy choice
that someone will eventually want reversed.

---

**13.**

| expression | resolves? | why |
|---|---|---|
| `self.fallback.handle` | **yes** | `self.fallback = Handler()` in `__init__` — a literal construction ty can see |
| `self.default.handle` | **yes** | `default = Handler()` in the class body; found through the class, not the instance |
| `self.handler.handle` | **no single answer** | `self.handler = handler`, a parameter — so the type is whatever any caller passed: `Handler` and `LoudHandler` |

**14.** In `emit`, the unknown value was a **parameter of the frame you are
resolving in** — so a per-call environment binding `writer ↦ JsonWriter()` is
enough. In `dispatch`, the value was stored on an **object** in a *different*
frame (`__init__`, called from `build_loud`) and read here.

So you need more than a parameter environment: you need object identity plus
per-object state that survives across frames. One sentence: **the value crosses
frames through an object rather than through an argument.**

**15.** *Model answer.*

I would need an abstract value for the receiver that identifies a **specific
object**, not just its class — so that the `Service` built in `build_loud` is
distinguishable from the one built in `build_quiet`. That object needs
per-instance attribute state, populated by interpreting `__init__` with the
constructor's arguments bound, so `handler ↦ LoudHandler()` is recorded on
*that* object. Then resolving `self.handler.handle` means looking up `self` in
the environment, reading the `handler` slot off the chosen object, and walking
that value's MRO for `handle`.

What people usually miss on the first pass: the MRO walk at the end still has to
happen, and it is the step that needs the private member-lookup API — which is
what makes this the argument for Option A + C rather than just for a smarter
environment.

**16.** It infers the receiver from the syntax: for `self.handler.handle`, the
receiver expression is `self.handler`, whose type it computes statically —
`Handler | LoudHandler`, the union over all assignments anywhere in the program.

Wrong receiver for the call tree because on the path
`use_loud → build_loud → dispatch`, the receiver is one specific `LoudHandler`
object. ty's receiver is a *type over all runs*; yours is *an object on one
path*. No argument you can pass to that function changes which receiver it uses
— the receiver is not a parameter.

---

**17.** If they differ and the difference is *order*, that is the DAG-vs-C3
distinction, not a bug — you computed something well-defined that is not the
MRO, and the fix is either to implement C3 or to establish (answer 9) that order
is unobservable.

If they differ in *membership* — a class present in one and not the other — that
is a bug, and the usual causes are: `object` missing from the tail, a base that
came back as a `GenericAlias` and got filtered out, or a base in a stub file
whose module you failed to resolve.

Membership differences are worth fixing immediately. Order differences are worth
a decision, and the decision belongs to the goldens.

---

## The fork (8b, 8c, 8d, 16b)

**8.** That the next component is `member`, and it is the same problem one
order of magnitude larger. Writing your own C3 costs ~30 lines of a
well-specified algorithm. Writing your own `member` costs the MRO walk *plus*
instance-vs-class attributes, descriptors, properties, `__getattr__` and
metaclasses — a second copy of Python's attribute semantics, which will
disagree with ty exactly where ty is right and you are not. You do not fork for
the MRO; you fork once, for the MRO *and* `member`, and the MRO is the cheap
one that proves the mechanism.

**8b.** Rust privacy is **module-scoped**: a private item is visible in the
module that declares it and in all of that module's descendants. `Type::member`
is private to `crate::types`, so only a module *under* `types` can call it —
`types/vnoc.rs`. A sibling module or a separate crate cannot, at any
visibility short of editing the item itself.

Which is the rebase argument. Flipping `pub(crate)` to `pub` edits existing
lines in upstream files, and edited lines are what conflict when upstream
touches the same region — which it will, because these are exactly the
functions ty is still changing. An added file conflicts with nothing, and the
single `pub mod vnoc;` line in `types.rs` sits in a list of module declarations
that rarely churns. After a rebase, every error that remains is in a file you
wrote.

**8c.** Because `ClassBase`, `ClassType` and `ClassLiteral` are all
`pub(crate)` **[verified, `types.rs:75`, `:118`]**. A wrapper returning one
compiles inside the fork and is uncallable from `pylspt` — the error lands at
the call site in your own crate, which is a confusing place to learn about ty's
visibility. Returning `Definition` and `Type`, both `pub`, is what makes the
wrapper a wrapper.

`Option` rather than skipping, because a skipped base **shifts every base after
it**. `Any`, `Unknown`, `Protocol` and `Generic` all appear in real MROs
(`hierarchy.py:Runner` and `Box` produce two of them), and "position 3 of this
class's MRO" is a thing downstream can care about. A `None` says "there is a
base here that has no definition"; a shorter list says nothing at all, and
silently.

**8d.** (1) **Wrappers only, no logic** — a wrapper with an `if` is a decision
you must re-verify on every rebase, in the one file your tests cover least.
(2) **Add one only when you need it** — every unused wrapper is maintenance for
a call that never happens. (3) **One test per wrapper, in `pylspt`** — the
compiler catches a changed *signature* for free, so the only thing a test adds
is the thing the compiler cannot see: a changed *answer*. Ignore that one and a
silent upstream behaviour change reaches the graph as a wrong edge.

A fourth, if you want it: **pin and rebase deliberately.** Continuous rebasing
spreads the cost over every day; a pinned rev concentrates it into an hour you
chose, with every compile error in one file.

**16b.** No — and the distinction is worth being exact about.

`vnoc::member` answers "given **this** receiver type, what is `name` on it?",
including descriptors and the MRO. What it cannot do is tell you *which*
receiver to pass. For `dispatch`, the receiver is whatever `self.handler` holds
on this path, which depends on which of `build_loud` / `build_quiet` ran, which
is frame- and object-state, and ty has no representation for it.

So from `plan/03-call-tree/07`'s workload the wrapper removes the **lookup**
half — all of `member.rs`'s MRO walk, descriptor protocol and property
handling. It leaves the **identity** half untouched: the environment, per-object
state, and the rule for what `self.x` means in a frame. That half was always
the part only you could write, and it is the reason Option C survives the fork.
