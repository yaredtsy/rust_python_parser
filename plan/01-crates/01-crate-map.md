# 01.01 — Crate map

All 54 crates in the workspace, tiered by whether you will ever type their name.
**[verified]** against `/Users/yared/Documents/Programing/ruff/crates` @ `ac201b8`.

---

## Tier 1 — you will use these every day

### `ruff_python_parser` — source → AST
Replaces **parso**. Error-recovering, so a file with syntax errors still yields
a usable tree (matters: your driver must never hard-fail on a broken file).

```rust
parse_module(source) -> Result<Parsed<ModModule>, ParseError>
parse_unchecked_source(source, PySourceType) -> Parsed<ModModule>   // never fails
parse(source, ParseOptions) -> Result<Parsed<Mod>, ParseError>
```

`Parsed<T>` gives you `.syntax()`, `.tokens()`, `.errors()`,
`.unsupported_syntax_errors()`.

> ⚠ `parse_module` and `ParseOptions::from(...)` silently target **Python 3.10**.
> This is the trap you flagged. → [`03-python-version.md`](03-python-version.md)

### `ruff_python_ast` — the AST itself
`Stmt`, `Expr`, `ExprCall`, `StmtFunctionDef`, `StmtClassDef`, `AnyNodeRef`.

The two sub-modules you need constantly:
- `ruff_python_ast::visitor::source_order` — `SourceOrderVisitor`, `walk_body`,
  `walk_expr`, `TraversalSignal`. This is your `_scan_children`.
- `ruff_python_ast::find_node::CoveringNode` — "what node covers this offset".

Also here: `PythonVersion` (yes, in the *ast* crate, not the parser crate).

### `ty_python_semantic` — ★ type inference
Your oracle. Key public surface **[verified]**:

```rust
pub use semantic_model::{SemanticModel, HasType, HasDefinition, ...};
pub use types::ide_support::{definitions_for_name, definitions_for_attribute, ...};
pub mod types;   // → ty_python_semantic::types::ide_support::* is reachable
```

The workhorse:
```rust
trait HasType { fn inferred_type<'db>(&self, model: &SemanticModel<'db>) -> Option<Type<'db>>; }
```

Caution: most of `Type`'s interesting methods are `pub(crate)`.
→ [`04-public-vs-private-api.md`](04-public-vs-private-api.md)

### `ruff_db` — the salsa substrate
`Db` trait, `File`, `PythonFile`, `source_text(db, file)`,
`parsed_module(db, file)` (salsa-cached — **this is the parse cache you want**),
`system::System` (the filesystem abstraction).

### `ruff_text_size` — `TextSize`, `TextRange`, `Ranged`
Everything is byte offsets. `node.range()` comes from the `Ranged` trait.
Import it or `.range()` won't resolve — a very common early stumble.

### `ruff_source_file` — `LineIndex`
Byte offset ↔ `(line, column)`. **Your only bridge to the parso-style
positions your wire format needs.** One per file, built once.

---

## Tier 2 — you will read these, and maybe copy from them

| Crate | Why |
|---|---|
| `ty_ide` | Has `call_hierarchy/`, `goto.rs`, `find_references.rs`. **Read `outgoing_calls.rs` (797 lines) before writing a line of your own.** It is the closest existing thing to your problem, and understanding exactly where it stops being enough is the core insight of this port. `publish = false`. |
| `ty_project` | `ProjectDatabase` — the concrete `Db` you instantiate. Config discovery (`ProjectMetadata::discover`), file walking, watching. `publish = false`. |
| `ty_module_resolver` | `ModuleName`, `Module`, `resolve_module`, `file_to_module`, search paths, typeshed `VERSIONS`. Replaces jedi's import machinery. |
| `ty_python_core` | Lower half of the semantic model: `semantic_index`, `ScopeId`, `FileScopeId`, `Definition`, `DefinitionKind`, `ProgramFile`, `Program`. You will touch `Definition` a lot — it is your node identity. |

---

## Tier 3 — occasional

| Crate | Why |
|---|---|
| **`libcst`** (crates.io) | **not a ruff crate, but ruff already depends on it** — `libcst = { version = "1.8.4", default-features = false }` **[verified, ruff Cargo.toml:132]**. The Rust build of LibCST; module name is `libcst_native`. Lossless CST, so it round-trips source exactly. **This is your ID injector** → [`02-mapping/02`](../02-mapping/02-id-injection.md). |
| `ruff_python_trivia` | comments, whitespace, `SimpleTokenizer`. Rarely needed — most position questions are answered by node ranges. |
| `ruff_python_codegen` | AST → source (reformats). You want its `Stylist` for line-ending/indent detection — see [`02-mapping/02`](../02-mapping/02-id-injection.md). |
| `ruff_python_stdlib` | is-this-a-builtin lookups. Replaces your `BUILTIN_NAMES` set. |
| `ruff_graph` | module-level import graph. Useful for scheduling/ordering work, not for call resolution. |
| `ruff_index` | `newtype_index!` macro — typed `u32` indices. Use it for your own arenas. |
| `ty_vendored` | bundled typeshed. You get it transitively; know it exists when stdlib types resolve "magically". |
| `ruff_notebook` | only if you need `.ipynb`. You don't. |

---

## Tier 4 — ignore entirely

`ruff_linter` (1000+ lint rules), `ruff_python_formatter`, `ruff_formatter`,
`ruff_workspace`, `ruff_server`, `ruff_wasm`, `ty_wasm`, `ruff_cache`,
`ruff_dev`, `ruff_benchmark`, `ruff_macros`, `ruff_annotate_snippets`,
`ruff_diagnostics`, `ruff_options_metadata`, `ruff_memory_usage`,
`ruff_markdown`, `ruff_mdtest`, `mdtest`, `ty_test`, `ty_combine`,
`ty_completion_bench`, `ty_completion_eval`, `ty_static`, `ty_site_packages`,
`ty_python_ast_integration_tests`, `ruff_python_trivia_integration_tests`,
`ruff_python_index`, `ruff_python_importer`, `ruff_python_edits`,
`ruff_python_literal`, `ruff_ranged_value`.

> `ruff_python_semantic` belongs in this tier **for your purposes** — it is the
> linter's binding table, not a type system. See
> [`00-orientation/03`](../00-orientation/03-what-ruff-is.md#the-split-that-will-confuse-you).

---

## Dependency shape of *your* crate

```
pylspt
├── ty_project           (ProjectDatabase — the Db you own)
├── ty_ide               (borrow goto/call-hierarchy machinery)
├── ty_python_semantic   (SemanticModel, Type, HasType, ide_support)
├── ty_python_core       (Definition, ScopeId, semantic_index, ProgramFile)
├── ty_module_resolver   (ModuleName, file_to_module)
├── ruff_db              (Db, File, parsed_module, source_text, System)
├── ruff_python_ast      (AST, visitors, PythonVersion)
├── ruff_python_parser   (only if you parse outside the db; usually you don't)
├── ruff_text_size       (TextRange, Ranged)
├── ruff_source_file     (LineIndex)
├── ruff_python_stdlib   (builtin names)
├── libcst               (lossless CST — ID injection only)
└── salsa                (must match ruff's exact version)
```

**`salsa` must be the identical version ruff uses**, or the `Db` traits won't
line up. Take it from ruff's workspace `Cargo.toml`, don't guess.
→ [`04-build/01-wiring-cargo.md`](../04-build/01-wiring-cargo.md)

---

→ Next: [`02-the-salsa-db.md`](02-the-salsa-db.md)
