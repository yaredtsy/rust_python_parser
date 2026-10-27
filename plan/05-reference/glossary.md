# 05.02 — Glossary

Jedi word ↔ ty word, plus the terms this plan introduces.

---

## Jedi → ty

| Jedi | ty | Notes |
|---|---|---|
| `Script` | `SemanticModel` | **one db, not one per call** — see [`00-orientation/02`](../00-orientation/02-why-it-is-slow.md) |
| `Project` | `ProjectMetadata` + `ProjectDatabase` | constructed once, lives for the process |
| `InterpreterEnvironment` | resolved from config, not the running interpreter | ⚠ source of version divergence |
| `Value` | `Type<'db>` | ✗ not equivalent — a `Value` can be a specific object; a `Type` is a set |
| `Context` | `ScopeId` + inferred types | ✗ ty scopes have no per-call identity |
| `ModuleContext` | `ProgramFile` + `SemanticModel` | ≈ |
| **`as_context(arguments)`** | **nothing** | ★ **the gap this project fills** |
| `TreeArguments` | `CallArguments` / `Bindings` | mostly `pub(crate)` |
| `TreeInstance` | `Type::NominalInstance` | ✗ ty's has no per-construction identity → your `OriginId` |
| `BoundMethod` | `Type::BoundMethod` | ≈ |
| `helpers.infer(state, ctx, leaf)` | `expr.inferred_type(&model)` | ≈ but context-free |
| `py__getattribute__(name)` | `Type::member_lookup_with_policy` | `pub(crate)` |
| `py__mro__()` | `ClassLiteral::iter_mro` | ✓ **[check]** name |
| `name.get_qualified_names(True)` | module name + symbol name | ≈, parity-critical → [`02-mapping/03`](../02-mapping/03-jedi-mro-to-ty-mro.md) |
| `is_builtins_module()` | `ruff_python_stdlib` / `KnownModule` | ✓ |
| `inference_state` | `db` | ≈ |

## parso / libcst → ruff

| parso / libcst | ruff |
|---|---|
| `parso.parse()` | `parsed_module(db, file).load(db)` |
| `parso.python.tree.Class` | `ast::StmtClassDef` |
| `parso.python.tree.Function` | `ast::StmtFunctionDef` (covers `async def`) |
| `lambdef` | `ast::ExprLambda` (separate node) |
| `atom_expr` + call `trailer` | `ast::ExprCall` — **nested, not flat** |
| `node.start_pos` `(line, col)` | `node.range().start()` → `TextSize` (byte offset) |
| `node.children` | typed fields; no generic children list |
| `node.get_doc_node()` | first stmt if `Expr(StringLiteral)` |
| `cst.CSTTransformer` | ✗ no lossless rewriter — use text edits |
| `module.code` (lossless) | ✗ `ruff_python_codegen` **reformats** |

---

## Ruff-side terms

**salsa** — incremental computation framework. Queries memoise on inputs and
record a dependency graph; changing an input re-runs only dependents.

**`#[salsa::tracked]`** — marks a memoised query. Its args must be salsa
"ingredients" (interned/tracked/input structs), not arbitrary values.

**revision** — global counter bumped on any input change; how salsa knows what's
stale.

**cancellation** — taking `&mut db` unwinds in-flight queries on other threads.
Why `panic = "abort"` breaks salsa.

**`File` / `PythonFile` / `ProgramFile`** — three file handles at increasing
levels of context. → [`01-crates/02`](../01-crates/02-the-salsa-db.md)

**`Definition`** — a binding site (a `def`, `class`, assignment, import). Your
node identity.

**`ProgramEnvironment`** — the type-system context passed alongside `db` to most
`Type` methods.

**`ResolvedDefinition`** — `ty_ide`'s "here's where this symbol is defined",
possibly several.

**typeshed** — bundled stdlib type stubs (`ty_vendored`), gated by a `VERSIONS`
file that maps symbols to Python version ranges.

**`TextSize` / `TextRange`** — `u32` byte offsets. Everything positional.

**`Ranged`** — the trait providing `.range()`. Import it or `.range()` won't
resolve.

**`TraversalSignal`** — `Traverse` or `Skip`, returned from `enter_node` to
control descent. Your "stop at nested callables" rule.

---

## Terms introduced by this plan

**abstract interpreter** — the layer you're building: walks a function body
carrying an environment, resolving callees from it before falling back to ty.
→ [`03-call-tree/03`](../03-call-tree/03-design.md)

**`AbstractValue`** — the value domain inside one walk. Not `Type`, because
`Type` is a set and you need specific objects.
→ [`03-call-tree/04`](../03-call-tree/04-values.md)

**`Env`** — names → values, plus object attributes, for one walk. Persistent,
cheaply cloned. → [`03-call-tree/05`](../03-call-tree/05-the-walk.md)

**`OriginId`** — which object, inside one walk: an input object named by its
path, or an object built at a site. Keeps two instances of one class apart.
→ [`03-call-tree/04`](../03-call-tree/04-values.md)

**context** — the key of a call: the function plus fingerprints of its
arguments and captured values. One summary per context.
→ [`03-call-tree/08`](../03-call-tree/08-keys.md)

**fingerprint** — a caller value reduced to what the key holds: class and
attributes to `fp_depth` steps, function identity, data type.
→ [`03-call-tree/08`](../03-call-tree/08-keys.md)

**summary** (`FunctionResult`) — what one call did, relative to its inputs:
call edges, returns, effects on the caller's objects, objects it built.
→ [`03-call-tree/09`](../03-call-tree/09-summaries.md)

**effect** — a write a call made on an object its caller can see, reported as
a path from the call's inputs. → [`03-call-tree/09`](../03-call-tree/09-summaries.md)

**recursion group** — the contexts of one cycle, resolved together by repeating
their deferred operations until nothing changes.
→ [`03-call-tree/12`](../03-call-tree/12-recursion.md)

**fan-out cap** — limit on alternatives kept per expression, so a wide union
doesn't multiply everything after it.

**cut** — a cost limit (stack, contexts, deadline) truncating a result. Marked,
never reused. → [`03-call-tree/13`](../03-call-tree/13-solver-and-limits.md)

**no-reuse mode** — recompute every summary instead of reusing it; the output
must not change. → [`03-call-tree/15`](../03-call-tree/15-checks-and-build-order.md#no-reuse-mode)

**divergence log** — the categorised diff between Python and Rust output. Some
divergences are improvements; the dangerous category is "different callee
resolved". → [`04-build/03`](../04-build/03-transport-and-parity.md)

---

## Vocabulary for your own tool

**call graph** — nodes are functions, each appears once. What `ty_ide` gives.

**call tree** — nodes are *activations*; a function appears once per path
reaching it. What you build. The distinction is the product.

**context-sensitive** — the analysis result at a program point depends on how
control arrived there. Jedi is (for parameters); ty is not.

**flow-sensitive** — the result depends on statement order within a scope.
ty **is** flow-sensitive (narrowing works); that's orthogonal to
context-sensitivity, and conflating the two is a common source of confusion when
reading ty's docs.
