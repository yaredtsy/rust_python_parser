# 06 — Scopes and definitions

**Goal:** for any name in any file, you can find where it is defined, and you
can build the qualified name that Jedi's `get_qualified_names(True)` would have
produced.

`target_qname` is the identity of every node in your call tree. Get it wrong and
nothing joins downstream — so this exercise is about identity, not about
lookups.

---

## Read first

- `tutorial/08-scopes-and-definitions.md` — the whole chapter
- `plan/05-reference/glossary.md` — the Jedi ↔ ty word map

---

## The mental model

### Jedi's `Context` vs ty's `Scope`

In Jedi you are always "in a context": `parent_context.create_context(leaf)`
gives you the thing that knows which names are visible where you are. Contexts
are created lazily, per query, and they are *positional* — where the cursor is
determines what you get.

ty builds a **semantic index** per file, once, cached. It is a static structure:

```
semantic_index(db, program_file) -> SemanticIndex
    ├── scopes            : every scope in the file, as a tree
    ├── place tables      : which names live in which scope
    ├── definitions       : every binding, keyed by AST node
    └── use-def map       : for each use of a name, which definitions reach it
```

The use-def map is the piece with no Jedi analogue. It answers "which
assignments could this read see, considering branches?" — computed once for the
whole file rather than per query.

### The seven scope kinds

```rust
pub enum ScopeKind { Module, TypeParams, Class, Function, Lambda, Comprehension, TypeAlias }
```

Two of those surprise people coming from Jedi:

- **`Comprehension` is its own scope.** `[x * x for x in range(n)]` binds `x` in
  a scope that is neither the enclosing function nor the module. That is real
  Python semantics (since 3.0), and ty models it.
- **`Class` is a scope, but not an enclosing one for lookup.** Names in a class
  body are not visible inside its methods. `registry` in `Service` is in the
  class scope; `run` cannot see it as a bare name.

### Definitions are your node identity

A `Definition` is "a place where a name gets bound" — a `def`, a `class`, an
assignment, an import, a parameter, a `for` target, a `with ... as`. It is
`Copy`, it is interned by salsa, and it is stable across queries within a
revision.

That makes it the natural identity for a function in your call tree. But note
what the plan says (`plan/01-crates/02`): **do not store `Definition<'db>` in a
long-lived structure.** Derive a stable owned key — `(file, range)` or the
qualified name — and re-derive the `Definition` when you need it.

### Shadowing: one name, four definitions

```python
value = 1
value = "two"
if COUNTER:
    value = 3.0
for value in range(2):
    pass
return value
```

Four bindings of `value` in one scope, and the `return` sees a *set* of them
depending on which branch ran. Jedi would infer a union too, but it computes it
by walking backwards from the use. ty precomputes the whole use-def map for the
file. Same answer; the difference is where the work happens, and therefore
whether it can be cached.

---

## The API, verified at `ac201b8`

All of this is public. That surprised me while writing this exercise — the plan
is pessimistic about what is reachable, and for the *semantic index* layer it is
more open than the type layer.

```rust
// ty_python_core
pub fn semantic_index<'db>(db: &'db dyn Db, file: ProgramFile<'db>) -> SemanticIndex<'db>;

impl SemanticIndex<'db> {
    pub fn scope_ids(&self) -> impl Iterator<Item = ScopeId<'db>>;
    pub fn scope(&self, id: FileScopeId) -> &Scope;
    pub fn parent_scope_id(&self, id: FileScopeId) -> Option<FileScopeId>;
    pub fn child_scopes(&self, scope: FileScopeId) -> ChildrenIter<'_>;
    pub fn ancestor_scopes(&self, scope: FileScopeId) -> AncestorsIter<'_>;   // ★ qualified names
    pub fn definitions(&self, key: impl Into<DefinitionNodeKey>) -> &[Definition<'db>];
    pub fn expression_scope_id<E>(&self, expr: &E) -> FileScopeId;
    pub fn class_definition_of_method(&self, …);
    pub fn imported_modules(&self) -> impl Iterator<Item = &ModuleName>;
    pub fn place_table(&self, scope_id: FileScopeId) -> &PlaceTable;
    pub fn use_def_map(&self, scope_id: FileScopeId) -> &UseDefMap<'db>;
}

impl ScopeId<'db> {
    pub fn name<'ast>(self, db, module: &'ast ParsedModuleRef) -> &'ast str;   // ★
    pub fn node(self, db) -> &'db NodeWithScopeKind;
    pub fn is_method_scope(self, db) -> bool;
    pub fn file(self, db) -> File;
}
impl Scope {
    pub fn parent(&self) -> Option<FileScopeId>;
    pub fn kind(&self) -> ScopeKind;
    pub fn node(&self) -> &NodeWithScopeKind;
    pub fn visibility(&self) -> ScopeVisibility;
}

impl Definition<'db> {
    pub fn scope(self, db) -> ScopeId<'db>;
    pub fn file(self, db) -> File;
    pub fn full_range(self, db, module: &ParsedModuleRef) -> FileRange;
    pub fn focus_range(self, db, module: &ParsedModuleRef) -> FileRange;
    pub fn name(self, db) -> Option<String>;
    pub fn docstring(self, db) -> Option<String>;      // ★★ see below
    pub fn is_reexported(self, db) -> bool;            // ★ relevant to exercise 05's re-export question
}

// ty_python_semantic — use → definition
use ty_python_semantic::{HasDefinition, definitions_for_name, ResolvedDefinition};
node.definition(&model)                       // trait HasDefinition
definitions_for_name(model, name_expr, …)     // -> Vec<ResolvedDefinition>
```

> ### ★★ `Definition::docstring` exists and is public
>
> **[verified, `ty_python_core/src/definition.rs:157`]** — it extracts the
> docstring for function, class and attribute definitions. And
> `docstring_from_body(body) -> Option<&ExprStringLiteral>` is a **`pub fn` in a
> `pub mod`** at `definition.rs:229`.
>
> `plan/02-mapping/01` says `docstring_from_body` "is `pub(crate)` to
> `ty_python_semantic`, but under Option A you can use it directly". **That is
> wrong at this revision** — it is fully public, and you are on Option B.
>
> So your exercise-02 hand-rolled docstring extraction has a supported
> alternative. Keep your own for the `parse_file` path (you are walking the AST
> anyway and you need the raw string for `ID:` scanning), but use
> `Definition::docstring` when you already have a `Definition` — which, in the
> call tree, you always will.

---

## Build it

### Step 1 — print the scope tree

For `python/scopes.py`, print every scope with its kind, name and parent:

```
module                                  Module
├── outer                               Function
│   ├── inner                           Function
│   ├── <lambda>                        Lambda
│   ├── <listcomp>                      Comprehension
│   └── <dictcomp>                      Comprehension
├── Service                             Class
│   ├── __init__                        Function
│   ...
```

Use `scope_ids()`, `scope(id).kind()`, `scope(id).parent()`, and
`ScopeId::name(db, &module)`.

**Predict the tree before you print it.** Specifically: how many scopes does
`outer` contain, and is the dict comprehension one scope or two?

### Step 2 — qualified names

Build `qualified_name(definition) -> String` matching Jedi's
`get_qualified_names(True)` joined with dots:

```
module name  +  every enclosing scope's name  +  the definition's own name
```

`ancestor_scopes(scope)` walks up the chain; `file_to_module` (exercise 05)
gives the module name. Test against these, which are the cases
`plan/02-mapping/03` flags as parity risks:

| definition | expected |
|---|---|
| `outer` | `scopes.outer` |
| `inner` | `scopes.outer.inner` |
| `Service.run` | `scopes.Service.run` |
| `Service.Nested.deep` | `scopes.Service.Nested.deep` ← **the one that breaks** |
| `generic` | `scopes.generic` |

The nested-class case is the one to check carefully. `plan/02-mapping/03` warns
that ty's `name` is the *immediate* name, so a naive implementation gives
`scopes.deep` or `scopes.Nested.deep`. Walking `ancestor_scopes` is what fixes
it.

Watch out for the scopes that should **not** contribute a component: does a
`TypeParams` scope appear between `generic` and the module? If so, you must skip
it, or you get `scopes.generic.generic`. Find out empirically.

### Step 3 — use → definition

For each call in `scopes.py`, resolve the callee name to its definition(s), and
print the definition's qualified name and range.

Two mechanisms, and you should try both:

- `HasDefinition::definition(&model)` — direct, when you have the right node
- `definitions_for_name(model, …)` — returns `Vec<ResolvedDefinition>`, because
  a name can resolve to several definitions

Then answer: for `self.dispatch(payload)` inside `run`, what do you get? This is
your first contact with attribute resolution, which exercise 08 covers properly
— note what happens now so you can compare.

### Step 4 — shadowing and the use-def map

For `shadowing()`, print every `Definition` of `value` in that scope, then print
what the `return value` use resolves to.

Predict first: **how many definitions, and how many reach the return?**

This is worth doing slowly. It is the clearest available demonstration of what
"flow-sensitive" means, and exercise 07 builds directly on it.

### Step 5 — re-exports

Go back to exercise 05's fixture. Take `pkg.load` (defined in `pkg/core.py`,
re-exported from `pkg/__init__.py`) and resolve it.

- Which file does the `Definition` live in?
- What does `is_reexported(db)` say?
- Does your `qualified_name` give `pkg.core.load` or `pkg.load`?

Check it against the prediction you wrote in exercise 05, step 5. Jedi follows
the definition, so `pkg.core.load` is the target. If yours says `pkg.load`, you
are naming by import route rather than by definition — which breaks the merge
behaviour in quirk 6.

### Step 6 — stubs

Resolve `json.dumps` from exercise 05's `entry.py`. The definition is in a
`.pyi` stub with no body.

`ty_python_semantic` exports `map_stub_definition` **[verified]** for mapping a
stub definition to its implementation when one exists. Try it on a stdlib symbol
and see what you get.

You will skip stdlib callees anyway (quirk 2), so this is not on your critical
path — but knowing what a stub definition looks like stops you from
misdiagnosing an empty body later.

---

## Traps

- **Treating a class scope as an enclosing scope for lookup.** It is a scope for
  *definitions*, not for name resolution from inside methods.
- **Forgetting comprehension scopes.** They are real; a `for` target inside one
  is not defined in the enclosing function.
- **Building qualified names from the AST alone.** You would have to re-derive
  the nesting that `ancestor_scopes` already gives you, and you would get
  comprehension and lambda scopes wrong.
- **Storing `Definition<'db>` in a long-lived map.** Lower to `(file, range)` or
  the qualified name.
- **Assuming one definition per name.** `definitions_for_name` returns a `Vec`,
  and `shadowing()` shows why.

---

## Done when

- [ ] you can print the full scope tree for `scopes.py` with kinds
- [ ] `qualified_name` produces all five expected strings, including the nested class
- [ ] you know whether `TypeParams` scopes contribute a name component
- [ ] you resolved every callee in `scopes.py` to a definition
- [ ] you can state how many definitions of `value` reach the `return`
- [ ] `pkg.load` resolves to a qualified name naming `pkg.core`

---

→ [`exam.md`](exam.md), then [`../07-types-and-inference/README.md`](../07-types-and-inference/README.md)
