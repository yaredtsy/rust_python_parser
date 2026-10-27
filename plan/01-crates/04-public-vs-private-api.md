# 01.04 — Public vs private API, and the fork decision

The decision that constrains everything else. Make it before Milestone 3.

---

## What is actually reachable from outside the workspace

**[verified]** by reading visibility modifiers @ `ac201b8`.

### ✅ Public — safe to build on

```rust
// ty_python_semantic (lib.rs re-exports)
pub use semantic_model::{SemanticModel, HasType, HasDefinition, MemberDefinition,
                         Completion, NameKind, ExpectedStringLiteralCompletion};
pub use types::ide_support::{ResolvedDefinition, ImportAliasResolution,
                             ImplementationsFinder, TypeHierarchyClass,
                             definitions_for_name, definitions_for_attribute,
                             definitions_for_imported_symbol, definitions_for_bin_op,
                             definitions_for_unary_op, map_stub_definition,
                             type_hierarchy_prepare, type_hierarchy_supertypes,
                             type_hierarchy_subtypes, contains_identifier};
pub mod types;          // ← so types::ide_support::* is ALSO reachable directly
pub use ty_python_core::Program;
```

Because `pub mod types;` and `pub mod ide_support;` are both declared
**[verified]**, you additionally get, un-re-exported but reachable:

```rust
ty_python_semantic::types::ide_support::{
    static_member_type_for_attribute,      // ← attribute type lookup
    resolved_call_signature,               // ← ★ argument→parameter matching
    call_signature_details,
    definitions_and_overloads_for_function,
    definitions_for_keyword_argument,
    call_argument_forms,
    constructor_signature,
    CallSignatureDetails, CallSignatureParameter,
};
```

`Type<'db>` itself is public, as is `Type::definition(db, env) -> Option<TypeDefinition<'db>>`
**[verified, types.rs:9215]**.

`ty_ide` exports **[verified]**:
```rust
pub use call_hierarchy::{CallHierarchyItem, prepare_call_hierarchy};
pub use call_hierarchy::outgoing_calls::{OutgoingCall, outgoing_calls};
pub use call_hierarchy::incoming_calls::{IncomingCall, incoming_calls};
pub use goto::{goto_declaration, goto_definition, goto_type_definition};
pub use symbols::{SymbolKind, SymbolInfo, HierarchicalSymbols, ...};
// ...and ~25 more
```

### ❌ Private — you cannot call these from outside `ty_python_semantic`

```rust
fn Type::static_member(...)                       // private (not even pub(crate))  [types.rs:4178]
fn Type::bindings(...)                            // private                        [types.rs:5922]
pub(crate) fn Type::member_lookup_with_policy(..) // pub(crate)                     [types.rs:5031]
pub(crate) fn Type::try_call_dunder_get(...)      // pub(crate)                     [types.rs:4231]
```

Count in `types.rs` **[verified]**: **10** `pub fn` vs **17** `pub(crate) fn`.
The type system's operational core is deliberately not public API.

### 🚫 Not on crates.io

**[verified]** `publish = false`, `version = "0.0.0"`:
- `ty_ide`
- `ty_project`
- `ty` (the CLI)

They **must** be consumed as `git` or `path` dependencies.

---

## Why this matters for *your* problem

Your interpreter's inner loop needs, repeatedly:

> "Given an abstract value `V` (a class instance I chose, not one ty inferred),
> what is member `m` on it?"

The public tools take **AST nodes**, not types:

```rust
static_member_type_for_attribute(model, attribute: &ast::ExprAttribute) -> Option<Type>
//                                      ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
// internally: attribute.value.inferred_type(model)  ← ty decides the receiver, not you
```

That is exactly backwards from what you need. You want to *supply* the receiver.
The function that would let you — `Type::static_member` — is private.

**This is the crux of the port.** Three ways out.

---

## ★★★ DECIDED (reversed 2026-10-17) — `pylspt` is the workspace root; vendor the needed closure

**`pylspt` stays the repo of record. A trimmed copy of just the ruff/ty crates
`ty_python_semantic` actually needs lives at `pylspt/vendor/`. Reach ty's
internals through one file of thin wrappers, and keep every line of logic in
`pylspt`.**

### What this reverses, and why

The section below this one (still here, unedited, for the reasoning) decided
**fork ruff wholesale and put `pylspt` inside it** — `crates/pylspt` nested in
a full clone of upstream's ~50-crate workspace. That was built, and it works:
`cargo check -p pylspt` resolved clean against it. It was undone anyway,
because the direction was wrong for what this repo *is*. `pylspt` is V-NOC's
product; a ruff checkout renamed into the role of "the repo" makes the git
identity of record an upstream monorepo with someone else's unrelated crates
(`ruff_linter`, the formatter, the language server, the playground) sitting
next to it. `pylspt`'s own `git log`, issues and releases belong to `pylspt`,
not to a fork of ruff that happens to contain it.

What stays true from the fork decision: the internals are only reachable from
*inside* `ty_python_semantic`, so some form of "a copy of that crate with one
file added" is unavoidable — that part of the reasoning two sections down does
not change. What changes is **which repo contains the other**: not all of
ruff containing `pylspt`, but `pylspt` containing the slice of ruff/ty it
actually calls. "Only a few of the ty crates, for class MRO and call
resolution" — not the fifty-crate workspace, and not `pylspt` nested inside
it.

A full-repo fork was considered and rejected for exactly the reason the
superseded `## Option A` text below once gave for choosing it:
"One `Cargo.lock`, no version skew, `salsa` version automatically correct."
That benefit comes from one workspace, one source for every ruff/ty/salsa
crate — and a *trimmed* vendor directory gets the same benefit, because it is
still one workspace with `pylspt` as the root member and `vendor/*` as the
rest. Trimming to the real dependency closure (not hand-picked — computed by
walking each vendored crate's own `[dependencies]`) avoids compiling ~25
unrelated crates for free, at the cost of a slightly fatter rebase (more
files touched when upstream renames something our closure happens to use).

### The shape

```
pylspt/                        ← the repo. workspace root AND the pylspt package.
├── Cargo.toml                 [workspace] members = ["vendor/*"]
├── src/                       ← your crate, all your logic
└── vendor/                    ← trimmed copy of ty_python_semantic's real
    ├── ruff_db/                 dependency closure (~26 crates), each
    ├── ruff_python_ast/         UNCHANGED except one:
    ├── ty_python_core/
    ├── ty_project/
    ├── ty_python_semantic/
    │   └── src/types.rs       + one line: `pub mod vnoc;`
    │   └── src/types/vnoc.rs  ← NEW. wrappers only.
    └── ... (the rest of the closure)
```

Same two edits as the fork-wholesale shape, ever: the `pub mod vnoc;` line and
the new `vnoc.rs` file. `vendor/*` crates otherwise need no edits at all —
they keep their own `{ workspace = true }` dependency lines unchanged;
`pylspt`'s root `Cargo.toml` just has to define a `[workspace.dependencies]`
entry for each one (path pointed at `vendor/<name>` instead of ruff's
`crates/<name>`) plus every external (crates.io) crate the closure needs, at
the same version/features ruff pins — the point of "one source, one copy"
still holds; it is just `pylspt`'s own workspace table doing the unifying
instead of ruff's.

### Why `types/vnoc.rs`, specifically

Rust privacy is **module-scoped**: a private item is visible in the module that
declares it *and all of that module's descendants*. The functions the
interpreter needs are private to `crate::types`:

| function | what it does | visibility | where |
|---|---|---|---|
| `Type::member` | `getattr(value, name)`, descriptors included | private | `types.rs:4929` |
| `Type::class_member` | look a name up through the MRO | private | `types.rs:3652` |
| `Type::try_call` | match arguments to a call | private | `types.rs:7037` |
| `Type::to_instance` | class to instance | private | `types.rs:1245` |
| `Type::to_class_type` | `ClassLiteral` **or** `GenericAlias` → `ClassType` | private | `types.rs:2570` |
| `ClassLiteral::iter_mro` | the MRO | `pub(crate)` | `class.rs:644` |
| `ClassType::iter_mro` | ditto, specialisation-aware | `pub(super)` | `class.rs:1362` |

**[all verified at `ac201b8`]**. A child module of `types` can call every one
of them; a sibling module, a new crate, or a `pub use` cannot. So the file has
to live there, and `pub mod types;` (`lib.rs:68` **[verified]**) means nothing
else has to change for `pylspt` to reach it.

### Why wrappers and not visibility patches

The earlier Option A proposed "small, surgical visibility patches" plus shims in
`ide_support.rs`. Prefer added files:

| | flip `pub(crate)` → `pub` | add `types/vnoc.rs` |
|---|---|---|
| upstream lines **modified** | one per item, in the files ty changes most | **one**, in a list of `mod` declarations |
| rebase conflicts | wherever upstream touched the same signature | almost never |
| after a rebase, errors appear | scattered through ty | in one file you wrote |
| upstreamable | awkward (why is this public?) | a wrapper is a reviewable PR if it is ever worth one |

### The rules

1. **One file.** `crates/ty_python_semantic/src/types/vnoc.rs` + `pub mod vnoc;`.
2. **Wrappers only.** One internal call each, **public** types in the signature,
   no logic. `ClassBase`, `ClassType` and `ClassLiteral` are `pub(crate)`
   (`types.rs:75`, `:118` **[verified]**) — a wrapper that returns one is
   uncallable from `pylspt`.
3. **Add a wrapper only when you need it.** Start with two: `class_mro` and
   `member`.
4. **Pin the fork; rebase on purpose,** not continuously.
5. **One test per wrapper, in `pylspt`.** The compiler catches a changed
   signature; only a test catches a changed answer.
6. **`pylspt` is a workspace member**, so `salsa` and every shared dependency
   come from one place with `workspace = true`. This retires the version/feature
   matching problem described in
   `experience/02-parse-and-ast/08-staged-pipeline.md`.

Ruff, ty included, is **MIT** (`Cargo.toml:13`, root `LICENSE` **[verified]**).
A private fork inside a closed-source product is allowed. Keep the licence file.

### What it costs

Rebasing onto newer ruff. ty is pre-1.0 and these internals are exactly what
moves. The cost scales with **how much of your code touches ty's internals**,
which is why the discipline above is the whole point: with wrappers-only, the
blast radius of any upstream change is one file and its tests.

### What it buys, beyond the MRO

The MRO is the cheap half. The expensive half is `member`: without it, the
interpreter's attribute lookup means reimplementing the MRO walk, instance vs
class attributes, descriptors, properties, `__getattr__` and metaclasses — a
second copy of Python's semantics that will disagree with ty wherever ty is
right. Your own C3 was the small version of that problem; `member` is the big
one. → [`03-call-tree/07`](../03-call-tree/07-attributes-and-self.md).

### What it does *not* change

**Option C stands.** `Type<'db>` still has no notion of "this specific object on
this specific path", so `AbstractValue`, the environment and frame identity are
unchanged. The fork buys **primitives** — MRO, member lookup, call binding — not
a value model. Use them from inside your own domain.

---

## Option A — Add `pylspt` to a vendored ruff workspace ← *chosen (trimmed), see above*

Vendor ruff as a git submodule / subtree, add your crate as a workspace member.

```toml
# ruff/Cargo.toml
[workspace]
members = ["crates/*", "../pylspt"]   # or move pylspt into crates/
```

| | |
|---|---|
| ✅ | Full access to `pub(crate)` internals via small, surgical visibility patches |
| ✅ | One `Cargo.lock`, no version skew, `salsa` version automatically correct |
| ✅ | You can add `pub fn` shims in `ide_support.rs` — the natural home, and the file is already the "expose internals to consumers" module |
| ✅ | Upstream-able: if a shim is generally useful, it's a clean PR |
| ❌ | You own a fork; rebasing onto upstream is periodic work |
| ❌ | Full workspace build (~5–10 min cold, but incremental after) |

**Mitigation for the fork cost:** keep your patch to a *single commit* that only
flips visibility and adds shims to `ide_support.rs`. Never edit inference logic.
Rebasing a 50-line visibility-only diff is a 10-minute job per upstream bump.

> ⚠ **Superseded in three details, not two.** First and second as originally
> written: prefer **added wrappers** to **flipped visibility**, and the home is
> `types/vnoc.rs`, not `ide_support.rs` — same reasoning as the DECIDED section.
> Third, added 2026-10-17: it is **`ruff` nested beside `pylspt`, not `pylspt`
> nested inside `ruff`** — `members = ["crates/*", "../pylspt"]` above already
> shows `pylspt` living *outside* `crates/`, which is closer to today's
> `vendor/*` layout than the fork-wholesale shape this doc tried in between.
> The git submodule/subtree mechanism is still not what's used (a plain
> directory copy, trimmed to the real dependency closure, is), but the
> *direction* — `pylspt` is the repo, ruff/ty is the dependency living inside
> it — is what today's DECIDED section restored.

## Option B — Git dependency, public API only ← *rejected*

```toml
ty_python_semantic = { git = "https://github.com/astral-sh/ruff", rev = "ac201b8" }
```

| | |
|---|---|
| ✅ | No fork, clean upgrades (change the `rev`) |
| ✅ | Fast build (only your dep tree) |
| ❌ | No arbitrary member lookup on a chosen receiver |
| ❌ | Must express every query as "synthesise/locate an AST node, then ask ty" |
| ❌ | `pub(crate)` can tighten under you on any bump |

This is what `experience/08-classes-and-mro` and `src/mro.rs` were built
against, and it is what the fork replaces. The cost was concrete and measurable:
a hand-written C3 merge, a `TypeHierarchyClass` → range → `covering_node` →
`Definition` round trip per base, and an inheritance-cycle fallback of our own
invention. All three delete.

## Option C — Own value domain, ty only for the edges ← *also chosen*

Don't use `Type<'db>` as your value domain at all. Build your own
`AbstractValue` over `ClassLiteral` / `FunctionType` / `Definition` handles,
and use ty only for: parsing, module resolution, MRO, and inferring
"leaf" expressions your interpreter can't handle.

| | |
|---|---|
| ✅ | Full control, no visibility fights, works with Option B's dependency shape |
| ✅ | Your domain can carry things ty's cannot (e.g. "this is the *same object* as that") |
| ❌ | You reimplement descriptor protocol, MRO attribute lookup, `super()`, properties |
| ❌ | Divergence from ty on edge cases |

> With the fork, C's two ❌ rows go away: the reimplementation is what
> `vnoc::member` and `vnoc::class_mro` replace. What survives is C's reason for
> existing — a value domain that can say "this specific object on this specific
> path", which `Type` cannot.

---

## Recommendation — unchanged in substance, sharper in form

**A + C**, as it always was. What changed is *how* A is done: one added file of
wrappers instead of a visibility patch, and — reversed once, back to this —
ruff/ty vendored inside `pylspt`'s workspace rather than `pylspt` living inside
a fork of ruff's.

**Do not start with B.** This is no longer advice — it is a description of what
happened. B was tried, it produced a working `base_classes` and a hand-rolled
C3, and it stopped at `member`.

## Pin the revision, whatever you choose

```toml
# document this. it is not optional.
# ruff pinned at ac201b8 (ruff 0.16.5, ty crates 0.0.11, 2026-09-02)
# bump procedure: plan/04-build/01-wiring-cargo.md
```

These are internal crates of a project that refactors aggressively. A pin is
the difference between "upgrade when we choose" and "broken on a Tuesday".

---

→ Next: [`02-mapping/01-parso-to-ruff-ast.md`](../02-mapping/01-parso-to-ruff-ast.md)
