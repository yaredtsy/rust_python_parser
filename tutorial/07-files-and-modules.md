# 7. Files, modules, and imports

You know how Jedi loads modules. This chapter maps that onto ty.

---

## The Jedi way (what you already know)

```python
project = jedi.Project(path=...)          # where the code lives
env = jedi.InterpreterEnvironment()       # which Python, which packages
script = jedi.Script(path=..., project=project, environment=env)
```

- **`Project`** knows the folder and the search paths.
- **`Environment`** knows which Python interpreter and which `site-packages`.
- **`Script`** is one file, ready to answer questions.

To follow `from pkg.mod import Thing`, Jedi searches the project paths and the
environment paths, loads that file, and parses it.

---

## The ty way

```python
Jedi                      ty
────                      ──
Project              →    ProjectMetadata + ProjectDatabase
Environment          →    the resolved Python environment (inside the db)
Script               →    SemanticModel::new(db, file)
module loading       →    ty_module_resolver
```

The big difference: **there is one database, not one Script per file.** The
database holds every file. `SemanticModel` is a thin view onto it for one file.

```rust
// once, at startup
let db = ProjectDatabase::use_defaults(metadata, system);

// per file, cheap
let model = SemanticModel::new(db, file);
```

Creating a `SemanticModel` costs almost nothing. Creating a `jedi.Script` costs
a project construction and a file parse. That is the difference in one line.

---

## Where the Python version comes from

This is the part you said confuses people. It is worth being exact.

Jedi asks the **running interpreter**:

```python
env = jedi.InterpreterEnvironment()    # → whatever Python is running the driver
```

ty asks the **configuration**. It looks in this order, and stops at the first
answer:

1. An explicit override you pass in
2. `[tool.ty.environment] python-version` in `ty.toml` or `pyproject.toml`
3. `requires-python` in `pyproject.toml` — and it takes the **lowest** version
4. The resolved Python environment (a `.venv`, a uv workspace)
5. Fallback: `PythonVersion::latest_ty()`, which is **3.14**

Read it back like this:

```rust
use ty_python_core::Program;
let version = Program::get(db).python_version(db);
```

> ### Two traps here
>
> **Trap 1: `requires-python` gives the lowest version.**
> A project with `requires-python = ">=3.9"` makes ty use **3.9** — even if the
> user is running Python 3.12. That is correct for a type checker (it wants to
> check that the code works on the oldest supported Python). It is probably
> *wrong* for you, since you just want to read the code.
>
> **Trap 2: Jedi and ty will disagree.**
> Jedi used the running interpreter. ty uses config. On the same project they
> can give different answers. This is the most likely reason your Rust driver
> will "find fewer nodes" than the Python one during testing.
>
> **What to do:** log the version and its source at startup. One line:
>
> ```
> pylspt: project=/x/y python_version=3.9 (source: requires-python ">=3.9")
> ```
>
> When something looks wrong later, this line usually explains it.

Why does the version matter at all? Because:

- Below **3.12**, `type X = int` and `def f[T]()` are syntax errors.
- At **3.12**, f-strings are tokenised differently, so **token ranges inside
  f-strings move**. Your `call_col_pos` could be wrong with no other symptom.
- The version also picks which stdlib symbols exist (see typeshed, below).

---

## Following an import

```python
from pkg.mod import Thing
```

In ty:

```rust
use ty_module_resolver::{ModuleName, resolve_module, file_to_module};

let name = ModuleName::new("pkg.mod")?;
let module = resolve_module(db, &name)?;     // → a Module, which knows its file
```

And the other direction — from a file back to its dotted name:

```rust
let module = file_to_module(db, program_file.resolver_file(db))?;
println!("{}", module.name(db));             // "pkg.mod"
```

You need that second one for building qualified names like
`pkg.mod.ClassName`, which your JSON output uses.

### Search paths

ty looks in several places, in order:

```
1. first-party code       your project's src/ or root
2. extra paths            configured search paths
3. stub packages          foo-stubs/
4. site-packages          the venv's installed packages
5. typeshed               bundled stub files for the standard library
```

You can inspect them:

```rust
use ty_module_resolver::search_paths;
for path in search_paths(db) {
    println!("{path:?}");
}
```

Print this at startup while developing. When a module does not resolve, the
search path list almost always shows why.

---

## Typeshed: stubs for the standard library

ty does **not** read the real `json.py` or `os.py` from your Python install.
Instead it reads **stub files** — files ending in `.pyi` that describe types
without any implementation:

```python
# a stub file: json/__init__.pyi
def dumps(obj: Any, *, indent: int | None = ...) -> str: ...
```

These come from **typeshed**, a big community project. Ruff bundles a copy in
the `ty_vendored` crate, so ty works with no setup.

Typeshed has a `VERSIONS` file saying which symbols exist in which Python
version. That is another reason the version setting matters — ask for 3.9 and
some 3.12 stdlib functions simply will not exist.

**What this means for you:** stdlib calls resolve fast and accurately, without
reading any real stdlib source. And your `_is_project_code` filter will
naturally exclude them, because their file paths are inside the vendored
typeshed, not your project.

---

## Deciding "is this my code?"

Your Python does this:

```python
# call_resolver.py:273-310
def _is_project_code(self, callee, inference_state):
    module_path = callee.get_root_context().py__file__()
    if norm_module.startswith(norm_project):
        return True
    # ... and everything else returns False
```

In ty, the same idea:

```rust
fn is_project_code(db: &dyn Db, definition: Definition<'_>) -> bool {
    let file = definition.file(db);
    let path = file.path(db);
    // is this path inside the project root?
    // (also: vendored typeshed paths are a distinct kind of path,
    //  so you can check for that directly)
}
```

Note that your Python version has a quirk worth knowing: the `site-packages`
and `lib/python` checks after the project check can never run, because the
function returns `False` at the end anyway. So the real behaviour is simply
**"inside the project, or excluded"**. That is fine — but reproduce it
deliberately, do not "fix" it by accident.

---

## What `SemanticModel` is for

`SemanticModel` is your question-asking handle for one file:

```rust
let model = SemanticModel::new(db, program_file);

// ask about an expression's type
let ty = expr.inferred_type(&model);

// ask what scope a node is in
let scope = model.scope(node);

// ask about the file
model.file();
model.line_index();
model.program_environment();
```

Compare to `jedi.Script`, which has `infer()`, `goto()`, `complete()`. Same job,
but `SemanticModel` is cheap to make and `jedi.Script` is not.

---

## Summary table

| Task | Jedi | ty |
|---|---|---|
| set up a project | `jedi.Project(path)` | `ProjectMetadata::discover` + `ProjectDatabase` |
| pick the Python version | `InterpreterEnvironment()` (running Python) | from config; falls back to 3.14 |
| open one file | `jedi.Script(path, project, env)` | `SemanticModel::new(db, file)` |
| cost of opening a file | high (parse + project) | near zero |
| resolve an import | search project + env paths | `resolve_module(db, &name)` |
| file → module name | `get_qualified_names` | `file_to_module(db, file)` |
| stdlib types | reads real stdlib source | reads bundled typeshed stubs |

---

## Check yourself

1. How many `ProjectDatabase` objects does a running driver need? How many
   `SemanticModel` objects?
2. Where does ty get the Python version, and where does Jedi get it?
3. What does `requires-python = ">=3.9"` make ty do?
4. What is typeshed, and why does ty not read the real `json.py`?
5. Which function turns a file back into a dotted module name?

---

→ Next: [`08-scopes-and-definitions.md`](08-scopes-and-definitions.md)
