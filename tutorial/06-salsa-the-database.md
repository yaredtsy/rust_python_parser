# 6. Salsa: the database that remembers

This is the idea with **no Jedi equivalent**. It is also the main reason the
Rust version will be fast. Take your time with this chapter.

---

## The problem it solves

Look at what your driver does today:

```python
# call_resolver.py:74-77 — inside resolve_call_hierarchy
jm = JediProjectManager(self.jedi_manager.project_path)
self.script = jm.get_script(file_path)
```

And `get_script` does:

```python
# jedi_manager.py:26-29
def get_script(self, path):
    project = jedi.Project(path=str(self.project_path.parent))
    env = jedi.InterpreterEnvironment()
    return jedi.Script(path=path, project=project, environment=env)
```

And `service.resolve_calls` makes a new `CallHierarchyResolver` **for every
top-level call** (`service.py:120`).

So: a file with 200 call sites builds 200 projects and re-reads the file 200
times, before any real work happens.

You cannot fix this by being careful. Jedi's design has no place to keep the
work. Salsa's whole purpose is to be that place.

---

## The idea

Salsa has two kinds of thing:

- **Inputs** — raw facts you tell it. "The file `app.py` contains this text."
- **Queries** — functions that compute something from inputs, or from other
  queries.

Salsa stores every query answer. It also records **which inputs each answer
depended on**.

```
INPUTS                    QUERIES
──────                    ───────
source_text(app.py) ────► parsed_module(app.py) ────► semantic_index(app.py)
                                                            │
source_text(lib.py) ────► parsed_module(lib.py) ─────┐      ▼
                                                     └──► infer_types(app.f)
```

Now you change `lib.py`. Salsa walks the graph:

- `source_text(lib.py)` changed.
- So `parsed_module(lib.py)` must re-run.
- `infer_types(app.f)` used `lib.py`, so it must re-run.
- `parsed_module(app.py)` did **not** use `lib.py`. **Keep the cached answer.**
- `semantic_index(app.py)` did not either. **Keep it.**

Only what actually depended on the change is recomputed. Everything else is a
lookup.

---

## Compare to your `lru_cache`

You already have a cache:

```python
# scanner.py:9
@lru_cache(maxsize=50)
def _inner_scan(content: str):
```

Three problems with it:

| Problem | Effect |
|---|---|
| The key is the whole file text | one keystroke = total miss |
| It hashes the whole string every call | slow even on a hit |
| `maxsize=50` | a medium project throws things away constantly |
| It only caches the parse | inference is never cached at all |

Salsa fixes all four. The key is a file handle (a number). Dependencies are
tracked properly. And **every layer is cached**, not just the parse.

---

## What a query looks like

```rust
#[salsa::tracked]
fn semantic_index<'db>(db: &'db dyn Db, file: PythonFile<'db>) -> SemanticIndex<'db> {
    let parsed = parsed_module(db, file).load(db);
    // ... expensive work building scopes and definitions ...
}
```

The `#[salsa::tracked]` attribute is what makes it cached. Without it, the
function just runs.

Calling it looks completely normal:

```rust
let index = semantic_index(db, file);       // first time: does the work
let index = semantic_index(db, file);       // second time: free
```

There is no `if cached` check to write. The macro handles it.

> **This changes how you write code.** In Jedi, you carefully avoid asking the
> same question twice, because each question is expensive. In ty, **just ask.**
> Calling `parsed_module(db, file)` a thousand times is fine. Trying to cache
> the result yourself is usually a mistake, because your cache will not know
> when the file changed and salsa's will.

---

## Building the database

```rust
use ruff_db::system::{OsSystem, SystemPathBuf};
use ty_project::{ProjectDatabase, ProjectMetadata};

let root = SystemPathBuf::from("/path/to/project");
let system = OsSystem::new(&root);                    // how to read files
let metadata = ProjectMetadata::discover(&root, &system)?;   // read config
let db = ProjectDatabase::use_defaults(metadata, system);
```

Four lines. Then keep `db` alive **for the whole life of the process**.

That last part is the point. One database, one process. Not one per request,
and definitely not one per call site.

`use_defaults` means: if the user's config file is broken, use sensible
defaults instead of failing. That fits your driver, which should never crash on
a bad project.

---

## The three file types

You will meet three types that all mean "a file". This is confusing at first.

| Type | Means | Where it comes from |
|---|---|---|
| `File` | a path in salsa's file system | `system_path_to_file(db, path)` |
| `PythonFile<'db>` | a `File` that is known to be Python | `program_file.python_file(db)` |
| `ProgramFile<'db>` | a Python file **plus its program context** | what most APIs take |

Public ty functions usually want `ProgramFile`. Then, inside, they do this:

```rust
let module = parsed_module(db, file.python_file(db)).load(db);
let model  = SemanticModel::new(db, file);
```

**Memorise those two lines.** Almost every analysis in ty starts with them. The
first gets the tree; the second gets the thing that answers type questions.

---

## Telling salsa a file changed

When you edit a file, salsa must be told:

```rust
File::sync_path(&mut db, &path);
```

Notice `&mut db` — a *writable* borrow. From chapter 2: you can have many read
borrows, or one write borrow, never both.

Two consequences:

**1. This bites you with ID injection.** Your `parse_file` writes UUIDs into the
source file. If you write the file and forget to sync, every later query serves
the *old* text. Your IDs will be missing forever, and nothing will look wrong.

This bug cannot happen in Python, because there is no cache. It will happen in
Rust. The order must be:

```
1. compute the new text
2. write it to disk
3. File::sync_path(&mut db, &path)     ← do not skip
```

**2. `&mut db` cancels work on other threads.** Any query running elsewhere
stops, by unwinding (a Rust panic that gets caught). This is on purpose — the
answer would have been stale anyway.

But it means two things for you:

- Your RPC layer must be ready to retry a request that got cancelled.
- **Never set `panic = "abort"` in `Cargo.toml`.** Salsa needs unwinding to
  work. With `abort`, a normal cancellation kills your whole process. This looks
  like random crashes and is hard to debug.

---

## Parallelism — the answer to the GIL

Your Python driver wraps everything in `run_in_threadpool`. That keeps the
server responsive, but Python's GIL means **zero real parallelism** for the
CPU work.

Salsa databases can be cloned cheaply into read-only snapshots:

```rust
let snapshot = db.clone();               // cheap
std::thread::spawn(move || {
    // read-only queries here, on another core, for real
});
```

So you can analyse many files at once, on many cores. This is a genuine win that
Python could not give you at any amount of effort.

`ty_project` has a `parallel.rs` file with the established pattern. Read it
rather than inventing your own.

---

## Should your own code be cached?

Not all of it. Here is the rule:

| Your code | Cache it? | Why |
|---|---|---|
| the node tree from `parse_file` | **yes** | it is a pure function of the file |
| MRO / base classes | already cached by ty | free |
| the context-sensitive call tree | **no** | its input is a *path*, not a file |

That last row is important. Your call tree depends on "which arguments were
passed on this path". That value is different almost every time, so a cache
keyed on it would never hit. Worse, salsa cannot even use it as a key — it is
not a "salsa ingredient".

ty itself makes the same choice. From the top of
`ty_ide/src/call_hierarchy.rs`:

> "The three entry points are deliberately not `#[salsa::tracked]` […]
> AST access goes through the salsa-cached `parsed_module`, which preserves
> incrementality without forcing the entry points themselves to be tracked."

**Follow that pattern: untracked entry points, cached primitives.**

---

## A mental model

Think of salsa as a spreadsheet.

- Inputs are the cells you type into.
- Queries are the cells with formulas.
- Change one input cell, and the spreadsheet recomputes only the formulas that
  used it.

That is exactly what salsa does, for your whole program.

---

## Check yourself

1. Why is a `lru_cache` keyed on file content a bad cache?
2. How many `ProjectDatabase` objects should your driver create?
3. What are the two lines that start almost every ty analysis?
4. What breaks if you write a file but do not call `File::sync_path`?
5. Why must you avoid `panic = "abort"`?
6. Why should your call tree function *not* be `#[salsa::tracked]`?

---

→ Next: [`07-files-and-modules.md`](07-files-and-modules.md)
