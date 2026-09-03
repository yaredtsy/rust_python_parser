# 02.03 — `mro_resolver.py` → ty MRO

The smallest port and the biggest speedup per line. 77 lines → ~30.

---

## What you do today

```python
script = self.project_manager.get_script(file_path)   # ← new Project + Script + reparse
defs = script.infer(line=line, column=column)          # ← full inference to find the class
for infer in defs[0]._name.infer():                    # ← private API
    if hasattr(infer, "py__mro__"):
        contexts = infer.py__mro__()                   # ← private API
        for c in contexts:
            qnames = c.name.get_qualified_names(True)
```

Plus, from `service.py:19-30`, a `_get_name_column` helper that does
`line.find(node.name)` to guess where the class name starts, then passes
`name_column + len(node.name)` as the inference position — i.e. it points
*just past* the class name and hopes.

Called **once per class**, each call rebuilding a `jedi.Project`. On a file with
40 classes that is 40 project constructions and 40 file reparses.

---

## Why ty makes this nearly free

MRO is a property of a class, computed once and salsa-cached. There is no
position guessing, no `infer()`, no string searching — you already *have* the
`StmtClassDef` node from your parse walk, so you go directly from AST node to
class type.

`ty_python_semantic/src/types/mro.rs` **[verified]** implements C3
linearisation, handles `Protocol`, `Generic`, `NamedTuple`, `Enum`, metaclasses,
and inheritance cycles (which it degrades gracefully on rather than panicking —
important, because your driver must not die on pathological input).

---

## The port

```rust
use ty_python_semantic::{SemanticModel, HasType};
use ty_python_semantic::types::Type;

fn base_classes(model: &SemanticModel<'_>, class: &ast::StmtClassDef) -> Vec<String> {
    // The class *name* is a definition; the StmtClassDef itself has a type
    // (the ClassLiteral). No position arithmetic needed.
    let Some(Type::ClassLiteral(literal)) = class.inferred_type(model) else {
        return Vec::new();
    };
    // `iter_mro` / `py__mro__` equivalent — exact name to confirm at your rev.
    literal
        .iter_mro(model.db(), &model.program_environment(), None)
        .filter_map(|base| qualified_name_of(model.db(), base))
        .collect()
}
```

**[check]** the exact accessor name at your pinned revision. Candidates seen in
`types/mro.rs` and `types/class.rs`: `ClassLiteral::iter_mro`,
`ClassType::iter_mro`, `Mro::of_class`. Under Option A (workspace member) any of
them is reachable; under Option B check what's `pub`.

`HasType` is implemented for statement types too **[verified]** —
`semantic_model.rs:900-925` has macro-generated `impl HasType for $ty` blocks
covering definition-carrying nodes. Confirm `StmtClassDef` is among them; if
not, use the class name `ExprName` or `HasDefinition` → `Definition` →
`Type::ClassLiteral`.

---

## Qualified names — the parity risk

Jedi's `get_qualified_names(True)` returns e.g. `["pkg", "mod", "Base"]`, joined
to `pkg.mod.Base`. You must produce the same strings or v-noc's downstream
matching breaks.

```rust
fn qualified_name_of(db: &dyn Db, class: ClassType<'_>) -> Option<String> {
    let literal = class.class_literal(db)?;          // [check] accessor name
    let module = ty_module_resolver::file_to_module(db, literal.file(db).resolver_file(db))?;
    Some(format!("{}.{}", module.name(db), literal.name(db)))
}
```

Three concrete divergences to test for:

| Case | Jedi | ty | Action |
|---|---|---|---|
| `object` | `builtins.object` | `builtins.object` | ✓ should match |
| Nested class `class A: class B: ...` | `mod.A.B` | may give `mod.B` | **test this** — ty's `name` is the immediate name; you may need to walk enclosing scopes |
| Class in `__init__.py` of `pkg` | `pkg.C` | `pkg.C` | ✓ |
| Generic alias `Base[int]` | `mod.Base` | may render as `mod.Base[int]` | strip the specialisation, or don't — match current output |

Write a table-driven test with these four cases before trusting the port.

---

## Where it plugs in

`service._apply_mro_to_classes` walks the node tree recursively applying MRO to
every `ClassNode`, only when `resolve_mro=true`. Keep that gating — the flag is
part of the RPC contract, and MRO forces the semantic index to be built for
imported modules, which is the expensive part.

In Rust the recursion is the same shape, but instead of a `MROResolver` holding
a `JediProjectManager`, you just thread `&SemanticModel`. The whole `MROResolver`
struct disappears; it becomes a free function.

Delete `service._get_name_column` entirely. It exists only to feed Jedi a
position, and its `line.find(node.name)` would find the wrong occurrence for
`class Foo(Foo_Base):` anyway.

---

## Expected result

MRO resolution goes from "the expensive flag you avoid setting" to
"free enough to always leave on". Consider whether v-noc should just always
pass `resolve_mro=true` once this lands — but that is a v-noc-side decision,
not a driver change.

---

→ Next: [`04-jedi-inference-to-ty.md`](04-jedi-inference-to-ty.md)
