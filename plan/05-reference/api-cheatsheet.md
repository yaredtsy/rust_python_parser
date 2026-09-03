# 05.01 — API cheat sheet

Verified surface, copy-paste oriented. **[verified]** = read from the source at
`ac201b8`. **[check]** = confirm at your revision before relying on it.

---

## Setup

```rust
use ruff_db::system::{OsSystem, SystemPathBuf};
use ty_project::{ProjectDatabase, ProjectMetadata};

let root     = SystemPathBuf::from("/path/to/project");
let system   = OsSystem::new(&root);
let metadata = ProjectMetadata::discover(&root, &system)?;       // [verified]
let db       = ProjectDatabase::use_defaults(metadata, system);  // [verified]
// or ProjectDatabase::fallible(metadata, system)?               // [verified]
```

## The two-line prologue — every analysis starts here

```rust
use ruff_db::parsed::parsed_module;
use ty_python_semantic::SemanticModel;

let module = parsed_module(db, file.python_file(db)).load(db);   // [verified]
let model  = SemanticModel::new(db, file);                       // [verified]
```
`file` is a `ProgramFile<'db>`. See
[`01-crates/02`](../01-crates/02-the-salsa-db.md#file-vs-pythonfile-vs-programfile--the-three-file-handles).

## Python version

```rust
use ty_python_core::Program;
use ruff_python_ast::PythonVersion;

Program::get(db).python_version(db)      // [verified] the resolved version

PythonVersion::default()      // ⚠ PY310  [verified]
PythonVersion::latest_ty()    //   PY314  [verified]
PythonVersion::latest()       //   PY314  [verified]
PythonVersion::lowest()       //   PY37   [verified]
```

Standalone parsing — **always** set the version explicitly:

```rust
use ruff_python_parser::{parse_unchecked, Mode, ParseOptions};

let parsed = parse_unchecked(
    source,
    ParseOptions::from(Mode::Module).with_target_version(version),   // [verified]
);
parsed.syntax(); parsed.tokens(); parsed.errors();
parsed.unsupported_syntax_errors();                                  // [verified]
```
→ [`01-crates/03`](../01-crates/03-python-version.md)

## Types

```rust
use ty_python_semantic::{HasType, HasDefinition};
use ty_python_semantic::types::Type;

expr.inferred_type(&model)      // -> Option<Type<'db>>   [verified] trait HasType
node.definition(&model)         // -> Definition<'db>     [verified] trait HasDefinition

ty.definition(db, &env)         // -> Option<TypeDefinition<'db>>  [verified] types.rs:9215
model.program_environment()     // -> ProgramEnvironment<'db>      [verified]
```

`Type` variants you will match on **[verified]**:
```
Dynamic  Never  Divergent
FunctionLiteral(FunctionType)   BoundMethod(BoundMethodType)
ClassLiteral(ClassLiteral)      GenericAlias(GenericAlias)
NominalInstance(NominalInstanceType)   ProtocolInstance(..)
SubclassOf(..)  ModuleLiteral(..)  Callable(CallableType)
KnownInstance(..)  SpecialForm(..)  TypeAlias(..)  NewTypeInstance(..)
PropertyInstance(..)  KnownBoundMethod(..)  WrapperDescriptor(..)
DataclassDecorator(..)  DataclassTransformer(..)
Union(..)  Intersection(..)   [check exact names]
```

## Definition resolution — `ide_support`

Re-exported at crate root **[verified]**:
```rust
use ty_python_semantic::{
    definitions_for_name, definitions_for_attribute, definitions_for_imported_symbol,
    definitions_for_bin_op, definitions_for_unary_op, map_stub_definition,
    ResolvedDefinition, ImportAliasResolution, ImplementationsFinder,
    type_hierarchy_prepare, type_hierarchy_supertypes, type_hierarchy_subtypes,
    TypeHierarchyClass, contains_identifier,
};
```

Reachable via `pub mod types; pub mod ide_support;` **[verified]** but not
re-exported:
```rust
use ty_python_semantic::types::ide_support::{
    static_member_type_for_attribute,   // (model, &ExprAttribute) -> Option<Type>
    resolved_call_signature,            // (model, &ExprCall) -> Option<CallSignatureDetails>
    call_signature_details,
    definitions_and_overloads_for_function,
    definitions_for_keyword_argument,
    call_argument_forms,
    constructor_signature,
    CallSignatureDetails, CallSignatureParameter, CallArgumentForm,
};
```

## ⛔ Not accessible from outside the crate **[verified]**

```rust
Type::static_member(..)                 // private          types.rs:4178
Type::bindings(..)                      // private          types.rs:5922
Type::member_lookup_with_policy(..)     // pub(crate)       types.rs:5031
Type::try_call_dunder_get(..)           // pub(crate)       types.rs:4231
```
Ratio in `types.rs`: **10 `pub fn` vs 17 `pub(crate) fn`**.
→ [`01-crates/04`](../01-crates/04-public-vs-private-api.md)

## Modules

```rust
use ty_module_resolver::{ModuleName, Module, file_to_module, resolve_module};  // [verified]

file_to_module(db, program_file.resolver_file(db))    // -> Option<Module<'db>>
module.name(db)                                        // -> &ModuleName
```

## Positions

```rust
use ruff_text_size::{Ranged, TextRange, TextSize};   // Ranged needed for .range()
use ruff_source_file::LineIndex;

let idx = LineIndex::from_source_text(source);       // build ONCE per file
let lc  = idx.line_column(offset, source);
// lc.line / lc.column are OneIndexed — subtract 1 for 0-based columns
```
⚠ character vs UTF-8 vs UTF-16 columns differ on non-ASCII — **[check]** the
exact method names at your revision and pick the *character* variant to match
parso. → [`02-mapping/01`](../02-mapping/01-parso-to-ruff-ast.md#positions-the-lineindex-bridge)

## AST traversal

```rust
use ruff_python_ast::visitor::source_order::{
    SourceOrderVisitor, TraversalSignal,
    walk_body, walk_stmt, walk_expr, walk_decorator, walk_parameters,
    walk_arguments, walk_type_params,
};                                                    // [verified] all used by ty_ide
use ruff_python_ast::find_node::CoveringNode;         // [verified]
use ruff_python_ast::{self as ast, AnyNodeRef};

impl<'a> SourceOrderVisitor<'a> for MyVisitor<'a> {
    fn enter_node(&mut self, node: AnyNodeRef<'a>) -> TraversalSignal {
        TraversalSignal::Traverse   // or ::Skip
    }
    fn visit_expr(&mut self, expr: &'a ast::Expr) { walk_expr(self, expr) }
    fn visit_stmt(&mut self, stmt: &'a ast::Stmt) { walk_stmt(self, stmt) }
}
```

## `ty_ide` — steal from these **[verified]**

```rust
use ty_ide::{
    prepare_call_hierarchy, outgoing_calls, incoming_calls,
    CallHierarchyItem, OutgoingCall, IncomingCall,
    goto_definition, goto_declaration, goto_type_definition,
    document_symbols, find_references, hover,
    SymbolKind, SymbolInfo,
};
```

Source worth reading, in order:
| File | Lines | Why |
|---|---:|---|
| `ty_ide/src/call_hierarchy/outgoing_calls.rs` | 797 | **read this first** — your traversal skeleton |
| `ty_ide/src/call_hierarchy.rs` | 416 | `CallHierarchyItem::from_definition`, `CalleeLeaf` |
| `ty_ide/src/goto.rs` | — | `find_goto_target`, `GotoTarget` |
| `ty_python_semantic/src/types/ide_support.rs` | 2171+ | the whole public bridge; the natural home for shims |
| `ty_python_semantic/src/semantic_model.rs` | 950+ | `HasType`/`HasDefinition` impls |
| `ty_python_semantic/src/types/mro.rs` | — | C3 linearisation |

## Version pin **[verified]**

```
ruff        ac201b8   (2026-09-02)
ruff crate  0.16.5
ty crates   0.0.11    (ty_ide, ty_project: 0.0.0, publish = false)
edition     2024      rust-version 1.96      toolchain 1.98.0
salsa       0.28.2  default-features = false
            features = ["compact_str", "macros", "salsa_unstable", "inventory"]
rustc-hash  2.0.0     smallvec 1.13.2 (union, const_generics, const_new)
indexmap    2.6.0     camino 1.1.7     serde 1.0.197    uuid 1.6.1
```

---

→ [`glossary.md`](glossary.md)
