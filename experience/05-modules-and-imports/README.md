# 05 — Modules and imports

**Goal:** given any file, you can name its module, follow any import to the file
it resolves to, and answer "is this project code?" — which is the filter that
decides what your call tree descends into.

---

## Read first

- `tutorial/07-files-and-modules.md` — the whole chapter
- `plan/00-orientation/01-what-you-have-today.md` quirks **2** and **3** —
  the project filter and the builtin filter are specification, not suggestion

---

## The mental model

### Jedi's way, and why it was slow

```python
project = jedi.Project(path)                 # rebuilt per call site
script  = jedi.Script(code, path=path, project=project)
```

Jedi resolves imports by walking `sys.path`-like search paths at inference time,
per `Script`. `jedi_manager.py` builds a fresh `Project` per request, so the
search-path work is redone constantly.

### ty's way

Module resolution is a **query**. Ask twice, pay once — and it is shared across
every file in the project, because two files with the same resolver environment
have the same answer by construction (that is what `ResolverFile` keys on;
exercise 03).

```
ModuleName "pkg.sub.deep"
      │  resolve_module(db, importing_file, &name)
      ▼
   Module  ──.file(db)──►  File  ──►  everything else
      │
      └──.search_path(db)──►  SearchPath ──► is_first_party() / is_standard_library()
                                              / is_site_packages()
```

That last branch matters more than it looks. Jedi made you compare path strings
to decide what kind of code you were looking at. ty already knows, because it
found the file *through* a classified search path.

### The typeshed layer

ty ships typeshed stubs inside the binary (`ty_vendored`). `import json`
resolves to a `.pyi` stub in a vendored filesystem, not to your installed
Python's `json.py`.

Consequences worth internalising now:

- Stdlib types resolve even with no interpreter installed at all.
- The file a stdlib import resolves to **is not on your disk**. Any code of
  yours that assumes "a `File` has a system path" will meet its first
  counterexample here.
- Stub files have signatures but no bodies. There is nothing to descend into
  even if you wanted to — which happens to align with quirk 2.

---

## The API, verified at `ac201b8`

```rust
use ty_module_resolver::{
    ModuleName, Module, SearchPath, KnownModule,
    resolve_module, file_to_module, search_paths,
};

// file → module
pub fn file_to_module<'db>(db: &'db dyn Db, resolver_file: ResolverFile<'db>) -> Option<Module<'db>>;
//   get the ResolverFile with:  program_file.resolver_file(db)

// module name → module
pub fn resolve_module<'db>(db: &'db dyn Db, importing_file: ImportingFile<'db>, name: &ModuleName) -> Option<Module<'db>>;
// also: resolve_module_for_import_from, resolve_real_module, resolve_module_confident

impl Module<'db> {
    pub fn name(self, db) -> &'db ModuleName;          // "pkg.sub.deep"
    pub fn file(self, db) -> Option<File>;             // ← Option. stubs, namespace packages
    pub fn search_path(self, db) -> Option<&'db SearchPath>;
    pub fn kind(self, db) -> ModuleKind;               // module vs package
    pub fn is_known(self, db, KnownModule) -> bool;    // "is this `typing`?"
}

impl SearchPath {
    pub fn is_standard_library(&self) -> bool;
    pub fn is_first_party(&self) -> bool;
    pub fn is_site_packages(&self) -> bool;
}
```

> `Module::file` returns `Option` for a reason — namespace packages have no
> file. Handle the `None`; do not `unwrap`.

---

## The fixture

```
python/proj/
├── pyproject.toml ................ [tool.ty.environment] root = ["src"]
└── src/pkg/
    ├── __init__.py ............... re-exports load, transform from pkg.core
    ├── core.py ................... every import kind, including a broken one
    ├── entry.py .................. six calls: first-party, stdlib, builtin
    └── sub/
        ├── __init__.py
        └── deep.py ............... reachable by three different module paths
```

`core.py` deliberately contains an import that cannot resolve
(`definitely_not_a_real_module_xyz`). Your driver must not care. Quirk 13 —
failures are swallowed, a partial answer is a valid answer.

---

## Build it

### Step 1 — name every file

For each `.py` under `proj/`, print `path → module name`. Use `file_to_module`.

Predict first: what is the module name of `src/pkg/__init__.py`? Of
`src/pkg/sub/deep.py`? Is the `src` directory part of the name?

Then comment out the `[tool.ty.environment]` section in `pyproject.toml` and run
again. Watch what happens to the names — that config line is what tells ty where
first-party code begins, and without it the answers change (or disappear).
Restore it afterwards.

### Step 2 — follow every import

Walk `core.py`'s AST for `Stmt::Import` and `Stmt::ImportFrom`, build a
`ModuleName` for each, resolve it, and print:

```
import json                → json          [stdlib]   file: <vendored>/stdlib/json/__init__.pyi
from pkg.sub.deep import descend → pkg.sub.deep [first-party] file: /…/src/pkg/sub/deep.py
import definitely_not_a_real…    → UNRESOLVED
```

Things to notice, and to write down:

- The stdlib file is **not on your disk**. Where does that path point?
- `import os.path` — what module name do you build, and what resolves?
- The relative import `from .sub.deep import ...` — how do you turn `level=1`
  plus `sub.deep` into an absolute `ModuleName`? (`ModuleName` has helpers; find
  them before hand-rolling string surgery.)
- The unresolvable import returns `None`. Confirm nothing panics.

### Step 3 — the project filter, twice

This is the deliverable of the exercise. Implement `is_project_code(file)` two
ways:

**Way A — Jedi's way, for parity.** `call_resolver.py:310`'s `_is_project_code`
returns `True` only when the callee's file lives under the project path. Note
what the plan says about it:

> the trailing `return False` at `call_resolver.py:310` — the `site-packages` /
> `lib/python` / `is_stdlib` checks above it are **unreachable**, so *everything*
> outside the project path is excluded regardless.

So the effective rule is: **a path prefix test, nothing more.**

**Way B — ty's way.** `module.search_path(db)` then `is_first_party()`.

Run both over every callee in `entry.py` and compare. They should agree on this
fixture. Now think about where they would not:

- a first-party file *outside* the configured `root`
- a source file inside `.venv` that is also inside the project directory
- a namespace package with no file at all
- an editable install pointing back at your own source tree ← the interesting one

**For the port, ship Way A.** The contract is the output, and today's output is
a prefix test, quirks included. But implement B as well and log when they
disagree: that log is a free discovery mechanism for parity edge cases you would
otherwise find in production.

### Step 4 — builtins, skipped by name

Quirk 3: `BUILTIN_NAMES` at `call_resolver.py:22` is consulted **by name, before
any inference**. A user-defined function called `list` is also skipped.

`ruff_python_stdlib` has builtin-name lookups. Find the function, and compare
its set against your `BUILTIN_NAMES` list. Any difference is a parity difference
— report it, do not silently adopt ruff's set.

Then confirm the ordering matters: in `entry.py`, `len(text)` and `list(blob)`
must be skipped **without** resolving them. If your implementation resolves
first and filters after, you get the same answer here but different behaviour
when a project defines its own `list` — and it is slower for no reason.

### Step 5 — three paths, one function

`deep.descend` is reachable as `pkg.sub.deep.descend`, via
`from .sub.deep import descend`, and (indirectly) through `pkg`'s re-exports.

Resolve all three and confirm they land on the same `File`. Then ask the
question that exercise 06 answers properly: **what qualified name should the
node carry?** Jedi's `get_qualified_names(True)` gives the *defining* module's
path, not the importing one. Predict what that means for `pkg.load`, which is
defined in `pkg.core` but imported through `pkg/__init__.py`.

Write your prediction down. You will check it in exercise 06.

---

## Traps

- **Building `ModuleName` by string concatenation.** Relative imports, dotted
  names and `__init__` all have rules. Use the constructors.
- **Assuming `Module::file()` is `Some`.** Namespace packages break that.
- **Assuming a `File` has a system path.** Vendored typeshed files do not.
- **Filtering builtins after inference.** Correct answer, wrong semantics
  (a project-defined `list` must still be skipped) and wasted work.
- **Using ty's classification for the parity build.** It is better, and it is
  not what today's driver does. Log the disagreements instead.

---

## Done when

- [ ] every fixture file prints its module name
- [ ] every import in `core.py` resolves, or reports UNRESOLVED without panicking
- [ ] you can state where a stdlib import's file lives
- [ ] `is_project_code` implemented both ways, with disagreement logging
- [ ] builtins are skipped by name, before inference
- [ ] you have a written prediction for `pkg.load`'s qualified name

---

→ [`exam.md`](exam.md), then [`../06-scopes-and-definitions/README.md`](../06-scopes-and-definitions/README.md)
