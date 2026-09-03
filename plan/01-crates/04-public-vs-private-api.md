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

## Option A — Add `pylspt` to a vendored ruff workspace ★ recommended

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

The shims you will most likely need:

```rust
// crates/ty_python_semantic/src/types/ide_support.rs  — additions
pub fn member_type_of<'db>(db: &'db dyn Db, env: &ProgramEnvironment<'db>,
                           ty: Type<'db>, name: &str) -> Option<Type<'db>>;
pub fn call_bindings_for<'db>(db: &'db dyn Db, env: &ProgramEnvironment<'db>,
                              callee: Type<'db>, args: &CallArguments<'db>) -> Bindings<'db>;
```

## Option B — Git dependency, public API only

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

Viable, but the interpreter becomes AST-shaped rather than type-shaped, and
`self.handler.run()` with a path-chosen `handler` gets awkward.
→ see [`03-call-tree/06-attributes-and-self.md`](../03-call-tree/06-attributes-and-self.md)
for what B costs concretely.

## Option C — Own value domain, ty only for the edges

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

---

## Recommendation

**A + C together.**

- Vendor ruff, add `pylspt` as a workspace member (**A**). Removes the entire
  category of "I can't reach that" problems, permanently.
- Still define your own `AbstractValue` domain (**C**), because *jedi's
  context-sensitivity is not expressible in ty's `Type` anyway* — `Type` has no
  notion of "this specific object on this specific path".

Use ty's private internals as *primitives* (member lookup, MRO walk, signature
binding), not as your value model.

**Do not start with B.** You will build a half-interpreter, hit `self.handler`,
and have to restructure. If you must ship on B for licensing/vendoring reasons,
read [`03-call-tree/06`](../03-call-tree/06-attributes-and-self.md) first and
accept a documented precision loss on attribute-dispatched calls.

---

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
