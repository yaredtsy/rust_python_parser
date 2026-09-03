# 01.02 — The salsa database

The single biggest structural difference from your Jedi driver. Get this right
and most of the performance problem disappears before you write any analysis.

---

## The idea

Salsa is an incremental computation framework. You register **inputs** (file
contents, settings). You write **queries** (`#[salsa::tracked] fn`). Salsa
memoises every query result and records which inputs each one read.

When an input changes, salsa bumps a global **revision**. On the next query it
walks the dependency graph and re-runs *only* what transitively depended on the
changed input. Everything else is returned from cache.

```
                      revision 1                revision 2 (file b.py edited)
  source_text(a.py) ────┐                     ┌── cached ──┐
  source_text(b.py) ────┤                     ├── CHANGED ─┤
                        ▼                     ▼            ▼
  parsed_module(a.py) ──── cached ────────────── cached
  parsed_module(b.py) ──── recomputed ─────────── RECOMPUTED
                        ▼                                  ▼
  semantic_index(a.py) ── cached (a doesn't import b) ───── cached
  infer_scope(b.fn) ───── recomputed ───────────────────── RECOMPUTED
```

Contrast with `scanner.py`'s `@lru_cache(maxsize=50)` keyed on the full file
string: one keystroke is a total miss, and only the *parse* was cached anyway.

---

## The `Db` trait stack **[verified]**

```rust
ruff_db::Db            : salsa::Database            // files, source, parsed_module, System
  └─ ty_python_core::Db                             // semantic index, scopes, definitions
       └─ ty_python_semantic::Db                    // type inference
            └─ ty_ide::Db                           // IDE queries
                 └─ ty_project::ProjectDatabase     // the concrete impl
```

Your analysis functions should be generic over the narrowest trait they need —
take `&dyn ty_python_semantic::Db`, not `&ProjectDatabase`.

## Constructing one **[verified]**

```rust
use ty_project::{ProjectDatabase, ProjectMetadata};
use ruff_db::system::{OsSystem, SystemPathBuf};

let root = SystemPathBuf::from("/path/to/project");
let system = OsSystem::new(&root);

// Reads ty.toml / pyproject.toml, discovers the Python environment.
let metadata = ProjectMetadata::discover(&root, &system)?;

// Two constructors:
let db = ProjectDatabase::use_defaults(metadata, system);  // substitutes defaults on bad config
// let db = ProjectDatabase::fallible(metadata, system)?;  // errors on bad config
```

Use `use_defaults` for a driver. A user's broken `pyproject.toml` must not take
your analyser down — that matches your current swallow-everything posture.

There is also **[verified]** `ProjectMetadata::discover_with_uv` /
`discover_without_uv` if you need to control uv workspace detection.

## `File` vs `PythonFile` vs `ProgramFile` — the three file handles

You *will* mix these up. **[verified]** from the `ty_ide` signatures:

| Type | What it is | Get it from |
|---|---|---|
| `File` | a path in the salsa file system | `system_path_to_file(db, path)` |
| `PythonFile<'db>` | a `File` known to be Python | `program_file.python_file(db)` |
| `ProgramFile<'db>` | a Python file + its program/environment context | what `SemanticModel::new` takes |

Public API entry points take `ProgramFile`:

```rust
pub fn outgoing_calls(db: &dyn Db, file: ProgramFile<'_>, offset: TextSize) -> Vec<OutgoingCall>
```

and inside they do:
```rust
let module = parsed_module(db, file.python_file(db)).load(db);
let model  = SemanticModel::new(db, file);
```

Memorise that two-line prologue. Every analysis starts with it.

## `parsed_module` — the parse cache

```rust
use ruff_db::parsed::parsed_module;
let parsed = parsed_module(db, python_file).load(db);   // ParsedModuleRef
let ast: &ModModule = parsed.syntax();
let tokens: &Tokens = parsed.tokens();
```

`parsed_module` is salsa-tracked; `.load(db)` materialises the AST. Under memory
pressure ty can drop and re-parse, which is why the AST is behind a ref rather
than handed out directly. **Never call `ruff_python_parser::parse_module`
yourself for a file that is in the db** — you would bypass the cache *and* the
Python-version wiring.

> **[verified]** `ty_python_core/src/ast_node_ref.rs` has a test named
> `rejects_module_parsed_for_different_python_version` and keys node refs on
> `(file, python_version)`. Parsing outside the db can therefore produce nodes
> the semantic layer refuses. Don't.

---

## Lifetimes: `'db` is viral

Nearly everything is `Type<'db>`, `Definition<'db>`, `ProgramFile<'db>`,
`SemanticModel<'db>`. Your own structures inherit it:

```rust
struct CallFrame<'db> {
    target: Definition<'db>,
    env: Env<'db>,
    children: Vec<CallFrame<'db>>,
}
```

**Design consequence:** you cannot hold ty types across a db mutation. So the
shape of a request handler is:

1. borrow the db,
2. do all analysis, producing ty-typed values,
3. **lower to owned, `'static`, serde-able types** (your `BaseNode` /
   `CallFrameStack` equivalents),
4. release the borrow, serialise, respond.

Never try to cache `Type<'db>` in a long-lived struct. Cache
`Definition`-derived stable keys (file + range) instead.

---

## Mutation and cancellation

```rust
// after an edit:
File::sync_path(&mut db, &path);          // or db.apply_changes(...) for watch events
```

Taking `&mut db` **cancels all in-flight queries** on other threads (they
unwind with a salsa cancellation panic that the runtime catches). That is
correct and intended, but it means your RPC layer must be prepared to retry a
request that got cancelled mid-flight by a concurrent `didChange`.

## Parallelism — the answer to your GIL problem

```rust
let snapshot = db.clone();      // cheap; ProjectDatabase is designed for this
std::thread::spawn(move || { /* read-only queries on snapshot */ });
```

Salsa databases are cheaply clonable read-only snapshots. Combined with rayon
over files, this is where the multicore win over `run_in_threadpool` comes from.
`ty_project` has a `parallel.rs` **[verified]** — read it for the established
pattern rather than inventing one.

---

## Should *your* analysis be `#[salsa::tracked]`?

Partly.

| Layer | Tracked? | Why |
|---|---|---|
| syntax node tree (`parse_file` nodes) | **yes** | pure function of the file; big win on re-request |
| MRO / base classes | already tracked by ty | free |
| **context-sensitive call tree** | **no** | its input is a *path environment*, not a file. Tracking it would key the cache on an unbounded, rarely-repeating value. Use your own memo table instead — see [`03-call-tree/08`](../03-call-tree/08-termination-and-cycles.md). |

Note what `ty_ide` itself does **[verified]**, from the header comment of
`call_hierarchy.rs`:

> "The three entry points are deliberately not `#[salsa::tracked]` […]
> AST access goes through the salsa-cached `parsed_module`, which preserves
> incrementality without forcing the entry points themselves to be tracked."

Follow that precedent exactly. Untracked entry points, tracked primitives.

---

→ Next: [`03-python-version.md`](03-python-version.md) ⚠
