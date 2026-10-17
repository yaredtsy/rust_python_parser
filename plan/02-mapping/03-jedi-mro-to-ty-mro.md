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

Two pieces: a **wrapper in the fork** that hands back ty's own linearisation,
and a function in `pylspt` that names each entry. The split is the rule from
[`01-crates/04`](../01-crates/04-public-vs-private-api.md): ty's internals
behind one thin call, every decision on your side of the line.

### In the fork — `crates/ty_python_semantic/src/types/vnoc.rs`

```rust
/// The MRO of `ty`, without the class itself, as definitions.
/// `None` keeps a base's POSITION when ty cannot tie it to a definition
/// (`Any`, `Unknown`, `Protocol`, `Generic`).
pub fn class_mro<'db>(db: &'db dyn Db, ty: Type<'db>) -> Vec<Option<Definition<'db>>> {
    let Some(class) = ty.to_class_type(db) else {
        return Vec::new();
    };
    class
        .iter_mro(db)
        .skip(1)                                  // [0] is the class itself
        .map(|base| match base {
            ClassBase::Class(c) => c.class_literal(db).definition(db),
            _ => None,
        })
        .collect()
}
```

**[verified at `ac201b8`]**, every hop:

| item | visibility | where |
|---|---|---|
| `Type::to_class_type -> Option<ClassType>` | private to `types` | `types.rs:2570` |
| `ClassType::iter_mro -> MroIterator` | `pub(super)` | `class.rs:1362` |
| `MroIterator: Iterator<Item = ClassBase>` | — | `mro.rs:691` |
| first element is the class itself | — | `mro.rs:623, 694-698` |
| `ClassType::class_literal` | `pub(crate)` | `class.rs:1199` |
| `ClassLiteral::definition -> Option<Definition>` | `pub(crate)` | `class.rs:913` |

Three corrections to the sketch this chapter used to carry:

- **`iter_mro` takes only `db`** at this rev — not
  `(db, &program_environment, None)`. The specialisation argument lives on
  `MroIterator::new`, which `iter_mro` calls for you.
- **Go through `to_class_type`, not `let Type::ClassLiteral(..)`.** `Type` has
  two class-ish variants, `ClassLiteral` (`types.rs:1676`) and `GenericAlias`
  (`:1678`); matching only the first returns an empty MRO for `Box[int]`.
- **`.skip(1)`**, because the iterator yields the class before its bases, and
  `base_classes` is the bases.

### In `pylspt` — naming, and the `None` slots

```rust
pub fn base_classes<'db>(db: &'db dyn Db, ty: Type<'db>) -> Vec<String> {
    vnoc::class_mro(db, ty)
        .into_iter()
        .map(|base| match base {
            Some(def) => qualified_name(db, def),     // exercise 06
            None => "<unresolved>".to_string(),       // ← YOUR decision; see below
        })
        .collect()
}
```

That is the whole port now. What it replaced — a recursion over
`type_hierarchy_supertypes`, a hand-written C3 merge, a
`TypeHierarchyClass` → range → `covering_node` → `Definition` round trip per
base, and an inheritance-cycle fallback of our own invention — is in
`src/mro.rs` and git history.

⚠ **The `None` slot is an open decision.** Dropping it shifts every later base;
emitting a placeholder keeps positions honest but puts a non-name in a field of
names. `hierarchy.py`'s `Runner(Protocol)` and `Box(Generic[T])` are the two
fixtures that produce one — decide against real output, against a golden, and
write it down.

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
