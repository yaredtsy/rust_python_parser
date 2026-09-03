# Answers 06 — Scopes and definitions

Where an answer depends on behaviour I did not verify at `ac201b8`, it says so.
Those are the ones to check against your own output rather than trust.

---

**1.**

*Jedi's per-query context* makes **positional** questions easy: "what is visible
at line 12 column 8" is the natural query, because a context is built from a
leaf. It makes **repeated** questions expensive — every query rebuilds.

*ty's per-file index* makes **whole-file** questions easy and cheap to repeat:
every scope, every definition, every use-def edge, computed once and cached. It
makes positional questions slightly indirect — you first map an offset to a node
(`CoveringNode`), then the node to a scope.

The trade is exactly the one the whole port is about: pay once per file instead
of once per question.

**2.** `Module`, `TypeParams`, `Class`, `Function`, `Lambda`, `Comprehension`,
`TypeAlias`.

The surprising two are **`Comprehension`** (a list/dict/set comprehension has
its own scope — real Python semantics since 3.0, but easy to forget) and
**`TypeParams`** (PEP 695 introduces a scope for `[T]` that did not exist as a
concept before 3.12).

**3.** A use-def map records, **for each use of a name, the set of definitions
that can reach it**, accounting for branches, loops and narrowing.

"All definitions in this scope" cannot answer *which* of them is live at a
particular use. In `shadowing()` there are four bindings of `value` but the
`return` cannot see the first one — it is unconditionally killed. Only a
flow-aware structure knows that.

**4.** A place where a name gets bound. Among others: a `def`, a `class`, an
assignment, an `import`/`from … import`, a function parameter, a `for` target, a
`with … as` target, an `except … as`, a walrus, a match capture pattern.

**5.** Because it carries `'db` — it is valid only for one borrow of one
revision of the database. Salsa may invalidate it, and you cannot hold it across
a mutation.

Keep an owned key instead: `(file path, TextRange)` or the qualified name, and
re-derive the `Definition` when you need it. Re-deriving is a cache hit.

---

**6.** Four child scopes: `inner` (Function), the lambda (Lambda), the list
comprehension (Comprehension), and the dict comprehension (Comprehension).

If you predicted three, you probably merged the two comprehensions or forgot the
lambda. If you predicted five, check whether you counted a `TypeParams` scope
that is not there — `outer` is not generic.

**7.** **No.** Class scopes do not participate in the lexical lookup chain for
code inside methods. `run` referring to a bare `registry` is a `NameError` at
runtime, and ty models that.

A class body is a scope for *executing the body and collecting definitions*, not
an enclosing scope for the functions defined in it. This is why attribute access
(`self.registry`, `Service.registry`) is a completely different mechanism from
name lookup — and why exercise 08 exists.

**8.** `x` is defined in the **Comprehension** scope. `seed` is read from the
enclosing **Function** scope (`outer`).

Not the same answer because the comprehension scope is a real scope that binds
its own iteration variable while still closing over names from outside. The
practical consequence: `x` is not visible after the comprehension, and a naive
"walk the AST and collect assignments per function" would wrongly put it in
`outer`.

**9.** **Verify this one yourself** — I did not confirm it at `ac201b8`. Ruff
models PEP 695 type parameters with a `TypeParams` scope kind, so the expected
answer is that one *does* appear for `def generic[T]`, sitting between the
function and its parent.

What your builder must do either way: **filter scopes by kind before appending a
name component.** Only `Function` and `Class` scopes contribute to a Jedi-style
qualified name. `TypeParams`, `Comprehension`, `Lambda` and `TypeAlias` must not.

How to find out: print `kind()` for every ancestor of `generic`'s definition.
That is a two-line change to step 1's output, and it is more reliable than
anything you read — including this file.

---

**10.**

| definition | qualified name |
|---|---|
| `outer` | `scopes.outer` |
| `inner` | `scopes.outer.inner` |
| `Service.run` | `scopes.Service.run` |
| `Service.Nested.deep` | `scopes.Service.Nested.deep` ← **the parity risk** |
| the lambda | `scopes.outer.<lambda>` or similar — and it does not matter, because quirk 8 drops lambdas entirely |

The nested class is the risk `plan/02-mapping/03` flags: ty's `name` is the
immediate name, so anything that does not walk the full ancestor chain produces
`scopes.deep` or `scopes.Nested.deep`. Both are wrong, and both look plausible
in a log.

**11.** Jedi produces **`pkg.core.load`** — it follows the definition, not the
import route.

If yours says `pkg.load`, yours is wrong for this contract. The fix is to build
the name from `definition.file(db)` (which is `core.py`) rather than from the
module you resolved the import *through*.

**12.** Suppose `entry.main` calls `load(...)` twice: once imported as
`from pkg import load`, once as `from pkg.core import load`.

With definition-based naming both produce `target_qname = pkg.core.load`, so
`add_child` merges them into one child with `call_count = 1` — one function,
called twice from this frame.

With route-based naming you get `pkg.load` and `pkg.core.load`: two children,
each `call_count = 0`, and downstream v-noc sees two functions where there is
one. Worse, `target_id` (the injected UUID) is the same for both, so you have
two nodes claiming the same identity — which is the kind of inconsistency that
corrupts a join rather than failing it.

---

**13.** **Four** definitions of `value`: `= 1`, `= "two"`, `= 3.0` (conditional),
and the `for` target.

Reaching the `return`: **three** — `"two"`, `3.0`, and the `for` target. The
first (`= 1`) is unconditionally killed by the second, so it cannot reach any
use below it.

*(Report what you actually observed. If ty gives a different count, the
interesting question is which binding it eliminated and what it knew that you
did not — for instance, reasoning about whether `range(2)` can be empty.)*

The gap between "four definitions exist" and "three reach here" **is**
flow-sensitivity. Exercise 07 takes this one step further and shows why it is
still not enough for your call tree.

**14.** `self.dispatch` is an **attribute**, not a name — so `definitions_for_name`
is the wrong tool and `definitions_for_attribute` is the right one. Depending on
which you tried, you got nothing, or you got the definition of `self`.

What it suggests: name resolution is lexical and cheap (walk the scope chain);
attribute resolution requires knowing the **type** of the receiver and then
walking its MRO. Two different machines. Exercise 08 is the second one, and
`plan/03-call-tree/06` is where it becomes the hard part of the port.

---

**15.** At `ac201b8`, `docstring_from_body` is **`pub fn` in `pub mod
definition`** **[verified, `ty_python_core/src/definition.rs:229`]** — fully
reachable from Option B, no fork needed. And `Definition::docstring(db) ->
Option<String>` is public too **[verified, `:157`]**.

What it changes: your exercise-02 extractor now has a supported alternative, and
the plan's Option-A justification for that particular line evaporates. It is one
data point for a bigger conclusion — **the plan's visibility survey is a
snapshot, and it is wrong in both directions.** Check before you accept "you
cannot reach that".

**16.** Not interchangeable, and you want both.

`Definition::docstring` gives you a `String` for a definition you already have
— ideal in the call tree, where you hold a `Definition` and need to scan for
`ID:`.

Your own extractor works from the AST while you are already walking it, which is
what `parse_file` does; going through `Definition` there would mean resolving
every node into the semantic index for something you can read off the tree in
front of you. Also, your version is what runs on unsaved client content, which
has no `Definition` at all.

Rule of thumb: **AST path → your extractor. Semantic path → ty's.**

---

**17.** A shippable signature:

```rust
fn qualified_name(db: &dyn Db, def: Definition<'_>, module: &ParsedModuleRef) -> Option<String>
```

`Option`, because it genuinely fails. The failure modes to write down now,
because exercise 08 sends MRO entries through this same function:

1. **No module** — `file_to_module` returns `None` (a file outside every search
   path, or a vendored file).
2. **Anonymous scopes** — lambdas and comprehensions have no meaningful name.
3. **Generic aliases** — `Base[int]` may render with its specialisation;
   `plan/02-mapping/03` says decide whether to strip it, and match current
   output.
4. **Stub vs implementation** — a definition in a `.pyi` names the same symbol
   as the `.py`; `map_stub_definition` exists to bridge them.
5. **Re-exports** — answered in 11; name by definition site.
6. **Nested classes** — answered in 10; walk all ancestors, filter by kind.

That list *is* the test table for exercise 08's MRO work. Write it as tests now
and you get that exercise's gate for free.
