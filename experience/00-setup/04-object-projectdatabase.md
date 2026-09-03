# Object 4 — `ProjectDatabase`

The single most important object in the whole port. Everything goes through it.

---

## What it is

The **database**. It is ty's memory: every file it has seen, every parse, every
type it has inferred, and the dependency graph tying them together.

You build one at startup and keep it alive. You do not build one per request —
that is exactly what your Python driver does with `jedi.Project`, and it is the
main reason it is slow.

```
   ProjectMetadata  +  OsSystem
            │
            │  ProjectDatabase::use_defaults(metadata, system)
            ▼
   ┌──────────────────────────────────────────────┐
   │  ProjectDatabase                             │
   │                                              │
   │   files:    path → File handles              │
   │   caches:   source_text, parsed_module,      │
   │             semantic_index, inferred types…  │
   │   graph:    which query read which input     │
   └──────────────────────────────────────────────┘
            │
            │  you ask it questions; it remembers the answers
            ▼
```

The technology underneath is **salsa**, an incremental computation framework.
You are not using salsa directly (it is not even in your `Cargo.toml`) — you are
using a database somebody else built with it.

---

## The one idea to take from this file

> Ask the same question twice, and the second answer is free.

`parsed_module(db, file)` the first time parses the file. The second time it is
a hash lookup. Change the file, and only the things that actually depended on it
recompute.

Exercise 03 is entirely about measuring this. Here you just need to build one.

---

## The `Db` traits — why methods are not where you expect

This confuses everyone once. Here is the whole story.

`ProjectDatabase` is the concrete type. But the *methods* live on a stack of
traits:

```
ruff_db::Db                    files, source text, parsed modules
   └── ty_python_core::Db         semantic index, scopes, definitions
        └── ty_python_semantic::Db   type inference   ← program_file() lives here
             └── ty_ide::Db             IDE queries
                  └── ProjectDatabase   implements all of the above
```

**Consequence:** to call `db.program_file(file)`, the trait that declares it must
be **in scope**. Otherwise:

```
error[E0599]: no method named `program_file` found for struct `ProjectDatabase`
```

The fix:

```rust
use ty_python_semantic::Db as _;
```

**Rust note — `as _`.** This imports the trait *for method resolution* without
binding the name `Db` in your module. That matters here because there are four
different traits called `Db` in this stack, and importing two of them by name
would be an ambiguity error. `as _` says "I want the methods, not the name".

**When you see "no method named X" and X is right there in the docs, it is a
missing trait import.** That is the same lesson as object 2's `System`, and it
will happen a third time. After that it stops being confusing.

---

## Where it comes from

```rust
use ty_project::ProjectDatabase;
```

---

## Making one

```rust
let db = ProjectDatabase::use_defaults(metadata, system);       // ★ never fails
let db = ProjectDatabase::fallible(metadata, system)?;          // errors on bad config
```

**[verified]** both exist.

**Use `use_defaults`.** A user's broken `pyproject.toml` must not take your
analyser down — it substitutes defaults for anything misconfigured and carries
on. That matches your Python driver's swallow-everything posture (quirk 13), and
it is the right choice for a background service. `fallible` is for a CLI that
wants to tell the user their config is wrong.

**Rust note.** Look at the signature:

```rust
pub fn use_defaults<S>(project_metadata: ProjectMetadata, system: S) -> Self
where S: System + 'static + Send + Sync + RefUnwindSafe
```

It takes `system` **by value** — the database *owns* the system from now on. If
you need it afterwards, clone it first (`OsSystem` is cheap to clone) or get it
back from the db. This trips people up on line 3 of their first program.

The bounds say: any `System` that lives long enough (`'static`), can be moved
between threads (`Send`), shared between threads (`Sync`), and survives a panic
unwinding past it (`RefUnwindSafe` — remember salsa cancels queries by
unwinding). `OsSystem` satisfies all four.

---

## What you can do with it

### Directly on the type **[verified]**

| method | use |
|---|---|
| `use_defaults(metadata, system)` | ★ construct |
| `fallible(metadata, system)` | construct, strictly |
| `apply_changes(&[ChangeEvent])` | ★ tell it files changed (watcher) |
| `freeze(&mut self)` | optimisation for one-shot runs; **not** for incremental use |
| `freeze_open_files(&mut self)` | same idea, narrower |

### Through the traits (import the trait first)

| method | trait | use |
|---|---|---|
| `program_file(file)` | `ty_python_semantic::Db` | ★★ `File` → `ProgramFile` |
| `python_version_with_source(file)` | `ty_python_semantic::Db` | ★ version **and where it came from** |
| `should_check_file(file)` | `ty_python_core::Db` | is this file in scope for checking |

### As a value

| operation | meaning |
|---|---|
| `db.clone()` | ★ a cheap read-only **snapshot** — hand it to another thread |
| `&db` | read queries |
| `&mut db` | ⚠ mutation — **cancels in-flight queries on other threads** |

The `&mut` rule is not a footgun, it is the design: a mutation changes what
every running query is reading, so those queries must be abandoned. Exercise 03
covers it; here, just know that `&mut db` is a heavier act than it looks.

---

## Example 1 — build one and prove it works

```rust
use ruff_db::system::{OsSystem, SystemPath};
use ty_project::{ProjectDatabase, ProjectMetadata};

fn main() -> anyhow::Result<()> {
    let dir = std::env::args().nth(1).expect("usage: prog <dir>");

    let system = OsSystem::new(&dir);
    let metadata = ProjectMetadata::discover(SystemPath::new(&dir), &system)?;

    // `system` is MOVED here. You cannot use it after this line.
    let db = ProjectDatabase::use_defaults(metadata, system);

    println!("database built");
    Ok(())
}
```

That is a complete, working program. It does nothing visible, and it is still
worth running: it proves your manifest, features and toolchain are correct, and
that a real project's configuration parsed.

**Try breaking it on purpose**, once:

```rust
let db = ProjectDatabase::use_defaults(metadata, system);
println!("{}", system.current_directory());     // ← ERROR
```

```
error[E0382]: borrow of moved value: `system`
```

Read that message properly. It is Rust telling you the ownership rule you just
read about. Move the `println!` above the `use_defaults` line and it compiles.
Understanding this error now saves you an hour later.

---

## Example 2 — a trait method, and the error you get without the import

```rust
use ruff_db::files::system_path_to_file;
use ruff_db::system::{OsSystem, SystemPath};
use ty_project::{ProjectDatabase, ProjectMetadata};
use ty_python_semantic::Db as _;          // ← ★ without this, the next line fails

fn main() -> anyhow::Result<()> {
    let dir = std::env::args().nth(1).expect("usage: prog <dir> <file>");
    let file_arg = std::env::args().nth(2).expect("usage: prog <dir> <file>");

    let system = OsSystem::new(&dir);
    let metadata = ProjectMetadata::discover(SystemPath::new(&dir), &system)?;
    let db = ProjectDatabase::use_defaults(metadata, system);

    let file = system_path_to_file(&db, SystemPath::new(&file_arg))?;
    let program_file = db.program_file(file);          // ← the trait method

    println!("python version = {}", program_file.python_version(&db));
    Ok(())
}
```

**This is the smoke test.** Objects 5, 6 and 7 explain the last three lines;
file `08-putting-it-together.md` walks the whole thing again slowly.

Do the experiment: comment out the `use ty_python_semantic::Db as _;` line and
compile. Read the error. Put it back. That is thirty seconds and it makes the
error recognisable forever.

---

## Example 3 — a snapshot

```rust
let snapshot = db.clone();

let handle = std::thread::spawn(move || {
    // read-only queries on `snapshot`, in parallel, sharing one cache
    // …
});

handle.join().unwrap();
```

`ProjectDatabase` derives `Clone` **[verified]**, and a clone is a cheap handle
to the same shared storage — not a copy of the cache. This is the answer to the
GIL problem from `plan/00-orientation/02`: your Python driver's
`run_in_threadpool` gives concurrency for I/O and nothing for CPU work; here the
work genuinely runs on several cores against one cache.

You do not need this yet. Know it exists.

---

## Exercise

**A.** Get example 1 compiling and running against
`experience/03-the-database/python/proj`. Then run it against a directory that
does not exist, and against a file instead of a directory. What happens in each
case? Should it?

**B.** Do the two deliberate-error experiments: use `system` after the move, and
remove the `Db as _` import. Write down both error messages in your notes, in
your own words.

**C.** Move database construction into a function in `src/db.rs`:

```rust
pub fn open_project(dir: &SystemPath) -> anyhow::Result<ProjectDatabase>
```

using the `metadata_for` helper from object 3. Every later exercise starts by
calling this.

**D.** Print `db.python_version_with_source(file).source` with `{:?}` for one of
the exercise-04 fixture projects. You cannot name the type
(`PythonVersionSource` lives in `ty_site_packages`, which is not in your
`Cargo.toml`) — but you can still call the method and debug-print the field.
Confirm that for yourself; it is a genuinely useful Rust fact.

---

## Exam

**1.** What does `ProjectDatabase` hold? Name three kinds of thing.

**2.** Why do you build one at startup and keep it, rather than one per request?
Which line of your Python driver is the counterexample?

**3.** Draw the `Db` trait stack. Which trait declares `program_file`?

**4.** You get `no method named program_file found for struct ProjectDatabase`.
What is wrong, and what is the exact fix?

**5.** Why `as _` in `use ty_python_semantic::Db as _;` rather than plain
`use ty_python_semantic::Db;`?

**6.** `use_defaults` vs `fallible` — which do you ship, and why?

**7.** `use_defaults` takes `system` by value. What does that mean for code after
that line, and what error do you get if you forget?

**8.** What is `db.clone()`, and what can you not do with the clone?

**9.** Why does `&mut db` cancel queries on other threads? (One sentence — the
full answer is exercise 03.)

---

## Answers

**1.** File handles (path → `File`), query caches (source text, parsed modules,
semantic indexes, inferred types), and the dependency graph recording which
query read which input.

**2.** Because the caches and the dependency graph are the entire point — a
fresh database knows nothing, so every question costs full price. The
counterexample is `jedi_manager.py`, which builds a new `jedi.Project` and
`Script` per request; `mro_resolver.py` then does it *once per class*, so a
40-class file rebuilds the project 40 times.

**3.** `ruff_db::Db` → `ty_python_core::Db` → `ty_python_semantic::Db` →
`ty_ide::Db`, with `ProjectDatabase` implementing all of them.
`program_file` is declared on **`ty_python_semantic::Db`** **[verified,
`ty_python_semantic/src/db.rs:14`]**.

**4.** The trait is not in scope. Rust does not auto-import traits, and a method
only exists for you if its trait is imported. Fix:
`use ty_python_semantic::Db as _;`

**5.** Because there are four traits named `Db` in this stack, and importing two
of them by name collides. `as _` brings in the methods without binding the name.

**6.** Ship `use_defaults`. A user's malformed `pyproject.toml` should degrade to
defaults, not kill the analyser — consistent with quirk 13, where every level of
the Python driver catches and logs. `fallible` suits a CLI whose job includes
telling the user their configuration is broken.

**7.** After that line `system` has been **moved into the database** and you
cannot use it. Forgetting gives `error[E0382]: borrow of moved value: system`.
Fix by doing whatever you needed with `system` *before* the call, or by cloning
it beforehand.

**8.** A cheap reference-counted handle to the same shared storage — a read-only
**snapshot**, not a copy. You can run queries on it from another thread, sharing
one cache. You cannot mutate through it, and you cannot hold one while something
else takes `&mut db`.

**9.** Because a mutation changes the inputs those queries are currently reading,
so their results would be inconsistent; salsa abandons them by unwinding (which
is why `panic = "abort"` is forbidden).
