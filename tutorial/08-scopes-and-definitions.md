# 8. Scopes and definitions

Where names live, and where they came from. This is ty's answer to Jedi's
`Context` and `Name`.

---

## The semantic index

After parsing, ty builds a **semantic index** for each file. This is one salsa
query:

```rust
use ty_python_core::semantic_index;
let index = semantic_index(db, python_file);
```

The index answers structural questions:

- What scopes exist in this file?
- What names are defined in each scope?
- For a name being *used* here, which definition does it refer to?

It does **not** know types. That is the next layer (chapter 9). Keeping them
separate is deliberate: the index is cheap and rarely changes, while type
inference is expensive.

---

## Scopes

A scope is a region where names live. Python has these:

```python
# module scope
X = 1

def outer():          # function scope for `outer`
    y = 2
    def inner():      # function scope for `inner`, nested in outer
        return y      # reads from outer's scope
    return inner

class C:              # class scope
    z = 3
```

ty gives each scope an id:

```rust
FileScopeId      // a scope, numbered inside one file
ScopeId<'db>     // a scope, unique across the whole program
```

`FileScopeId` is just a number for one file. `ScopeId` is the file plus that
number, so it is globally unique. You will mostly use `ScopeId`.

Scopes form a tree. You can walk from a node up to the module:

```rust
for scope in model.ancestor_scopes(node) {
    // innermost first, then outward, ending at the module scope
}
```

That is how name lookup works — check the innermost scope, then the next one
out, and so on.

> **Compare to Jedi:** Jedi's `Context` does both jobs at once. It says *where
> you are* **and** *what values are visible*. ty splits them: `ScopeId` is only
> "where", and types are a separate layer.
>
> This split is exactly why your port is hard. Jedi's `Context` can be built
> with arguments (`as_context(arguments)`), which puts *values* into it. A
> `ScopeId` is only a number — there is nowhere to put values. That is why the
> plan says you must build your own `Env`.

---

## Definitions

A **`Definition`** is the place where a name was created. It is the single most
useful handle in ty for your work.

These are all definitions:

```python
def f(): ...          # a Function definition
class C: ...          # a Class definition
x = 1                 # an Assignment definition
import os             # an Import definition
def g(param): ...     # `param` is a Parameter definition
for item in xs:       # `item` is a definition
with open(p) as fh:   # `fh` is a definition
```

Each has a **kind**:

```rust
use ty_python_core::definition::DefinitionKind;

match definition.kind(db) {
    DefinitionKind::Function(func_ref) => { /* a def */ }
    DefinitionKind::Class(class_ref)   => { /* a class */ }
    _ => { /* assignment, import, parameter, ... */ }
}
```

Your call tree only cares about `Function` and `Class`. `ty_ide` filters the
same way:

```rust
// from ty_ide/src/call_hierarchy/outgoing_calls.rs
match def.kind(self.db) {
    DefinitionKind::Function(_) | DefinitionKind::Class(_) => {}
    _ => continue,       // skip everything else
}
```

### What you can ask a `Definition`

```rust
definition.file(db)                    // which file it is in
definition.scope(db)                   // which scope it is in
definition.name(db)                    // its name, if it has one
definition.full_range(db, &module)     // the whole `def f(): ...` block
definition.focus_range(db, &module)    // just the name `f`
definition.kind(db)                    // Function / Class / Assignment / ...
```

`full_range` vs `focus_range` matters for your output. `full_range` is the whole
function. `focus_range` is just the name — which is what an editor highlights,
and what makes a good stable identity key.

### Definitions are your node identity

Two useful properties:

1. A `Definition` is `Copy` and cheap to compare.
2. `(file, focus_range)` is a stable key you can use in a `HashMap`.

`ty_ide` uses exactly that:

```rust
struct CalleeKey {
    file: File,
    selection_range: TextRange,
}
```

Your call tree needs the same thing — a way to say "these two frames point at
the same function". Use `Definition` or `(file, focus_range)`, not the name
string. Two different functions can share a name.

---

## Going from a use to a definition

This is "go to definition". You have a `Name` node in the code; you want the
`def` it refers to.

```rust
use ty_python_semantic::{definitions_for_name, ImportAliasResolution};

let defs = definitions_for_name(
    &model,
    name_expr,
    ImportAliasResolution::ResolveAliases,
);
```

Two things to notice.

**It returns a list, not one answer.** A name can refer to several definitions:

```python
if config.fast:
    from fast import run
else:
    from slow import run

run()          # ← two possible definitions
```

Jedi's `helpers.infer` also returns a list, for the same reason. Your Python
code already loops over `callee_values`, so this will feel familiar.

**`ImportAliasResolution` controls how far to follow imports:**

```python
# a.py
from b import thing

# c.py
from a import thing        # ← where is `thing` defined?
```

- `ResolveAliases` — follow all the way to `b.py`. Usually what you want.
- The other option stops at the alias in `a.py`.

There is a matching function for attributes:

```rust
definitions_for_attribute(&model, attribute_expr, ImportAliasResolution::ResolveAliases)
```

That is for `obj.method`. It is the closest public thing to Jedi's
`py__getattribute__`.

---

## `ResolvedDefinition`

The functions above return `ResolvedDefinition`, not `Definition`. It is a small
wrapper that can also point at things that are not real definitions (a module,
or a location inside a stub file).

Get the real definition out with:

```rust
let Some(def) = resolved.definition() else { continue };
```

You will write that line a lot. It is the same `let ... else` pattern from
chapter 2.

---

## Building a qualified name

Your output uses names like `pkg.mod.Class.method`. Jedi gives you this with
`name.get_qualified_names(True)`. ty does not have one function for it — you
build it:

```
1. definition.file(db)             → which file
2. file_to_module(db, ...)         → "pkg.mod"
3. definition.name(db)             → "method"
4. walk up scopes for enclosing classes → "Class"
5. join with "."                   → "pkg.mod.Class.method"
```

Step 4 is the one to be careful with. For a nested class:

```python
class A:
    class B:
        def f(self): ...
```

Jedi says `mod.A.B.f`. If you only use `definition.name(db)`, you get `f`, and
if you only add the module you get `mod.f`. You must walk the enclosing scopes
and collect the class names.

> **Test this early.** Qualified names go into `target_qname`, which the rest of
> v-noc matches on. If they do not match Jedi's format exactly, everything
> downstream breaks quietly. The plan lists four specific cases to test
> ([`plan/02-mapping/03`](../plan/02-mapping/03-jedi-mro-to-ty-mro.md)).

---

## Word map

| Jedi | ty | Same? |
|---|---|---|
| `Context` | `ScopeId` | partly — ty's has no values in it |
| `ModuleContext` | `ProgramFile` + `SemanticModel` | close |
| `Name` | `Definition` | close |
| `name.get_qualified_names(True)` | build it yourself | needs care |
| `helpers.infer(state, ctx, leaf)` | `definitions_for_name(...)` | similar, but no context |
| `py__getattribute__("x")` | `definitions_for_attribute(...)` | similar |
| `FunctionExecutionContext` | **nothing** | ← the gap |

That last row is chapter 9.

---

## Check yourself

1. What does the semantic index know, and what does it *not* know?
2. Why does `definitions_for_name` return a list?
3. What is the difference between `full_range` and `focus_range`?
4. Why should you not use the name string as a frame's identity?
5. What must you do to build `pkg.mod.A.B.f` for a nested class?

---

→ Next: [`09-types-and-inference.md`](09-types-and-inference.md) — the important one
