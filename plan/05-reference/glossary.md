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

**abstract interpreter** — the layer you're building: walks the AST carrying an
environment, resolving callees from it before falling back to ty.
→ [`03-call-tree/03`](../03-call-tree/03-the-abstract-interpreter.md)

**`AbstractValue`** — your value domain. Not `Type`, because `Type` is a set and
you need specific objects. → [`03-call-tree/04`](../03-call-tree/04-value-domain.md)

**`Env`** — name → `AbstractValue` for one activation. The replacement for
`as_context(arguments)`. Persistent/immutable, cheaply cloned.

**`OriginId`** — identifies a construction site, so two instances of the same
class stay distinct. What makes per-path object state work.
→ [`03-call-tree/06`](../03-call-tree/06-attributes-and-self.md)

**`PathKey` / call string** — the chain of definitions from root to a node.
Formally this is *k*-CFA call-string sensitivity.
→ [`03-call-tree/09`](../03-call-tree/09-path-identity.md)

**context-independent** — a function whose call subtree doesn't vary with its
arguments. Memoisable, and most functions qualify. The big M8 win.
→ [`03-call-tree/08`](../03-call-tree/08-termination-and-cycles.md)

**fan-out cap** — limit on alternatives explored per expression, so a wide union
doesn't multiply the whole subtree.

**budget** — global node counter, decremented per `resolve_call`. Your hard
latency guarantee.

**graft** — splicing a memoised subtree into a frame instead of recomputing it.

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
