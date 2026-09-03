# Object 6 — `ProgramFile` (and `PythonFile`, and `ResolverFile`)

Three more file types. This file explains why that is not madness.

---

## What it is

A `File` (object 5) is just "a path in the database". But most of ty's
interesting questions do not depend only on *which file* — they also depend on
*what settings apply to it*.

So ty has three richer handles, each pairing the file with a different amount of
context:

```
   File                        just the path
     │
     ├── PythonFile   =  File + Python version
     │                   → enough to PARSE
     │
     ├── ResolverFile =  File + module-resolution environment (search paths)
     │                   → enough to FOLLOW IMPORTS
     │
     └── ProgramFile  =  File + the whole program (version + environment + platform)
                         → enough to INFER TYPES
```

You get all of them from one call:

```rust
use ty_python_semantic::Db as _;

let program_file = db.program_file(file);          // File → ProgramFile
let python_file  = program_file.python_file(&db);  // → PythonFile
let resolver_file = program_file.resolver_file(&db); // → ResolverFile
let back_to_file = program_file.file(&db);         // → File
```

---

## Why three, and not one

This looks like over-engineering until you read ty's own justification, which is
in the source **[verified, `ty_python_core/src/program_file.rs`]**:

> This allows programs with the same Python version to share parsed syntax, and
> programs with equivalent resolver environments to share module resolution,
> while keeping type inference isolated.

Unpack that with an example. Two projects on your machine, A and B. Both target
Python 3.12. They have different `sys.path` search paths.

| question about `helpers.py` | depends on | can A and B share the answer? |
|---|---|---|
| what is its text? | the file | **yes** — keyed on `File` |
| what is its AST? | file + version | **yes**, both are 3.12 — keyed on `PythonFile` |
| where does `import config` go? | file + search paths | **no**, paths differ — keyed on `ResolverFile` |
| what type is `x`? | everything | **no** — keyed on `ProgramFile` |

**The three handles are three cache keys**, chosen so each layer shares as much
as is safe and no more. One handle would force the narrowest sharing everywhere
and you would lose the parse cache across projects for nothing.

That is the whole idea. It is a good idea, and once you see it the API stops
looking redundant.

---

## Where it comes from

```rust
use ty_python_core::ProgramFile;    // the type
use ty_python_semantic::Db as _;    // the trait providing db.program_file()
```

⚠ Note the split: the **type** lives in `ty_python_core`, the **method that
makes one** is on a trait in `ty_python_semantic`. Forgetting the second import
gives you the "no method named `program_file`" error from object 4.

---

## What you can do with it

**[verified]** from `ty_python_core/src/program_file.rs`.

| method | returns | use |
|---|---|---|
| `python_file(&db)` | `PythonFile<'db>` | ★ what `parsed_module` needs |
| `file(&db)` | `File` | ★ back to the plain handle |
| `program(&db)` | `Program<'db>` | the whole environment |
| `python_version(&db)` | `PythonVersion` | ★ object 7 |
| `resolver_file(&db)` | `ResolverFile<'db>` | ★ what `file_to_module` needs |
| `resolver_environment(&db)` | `ResolverEnvironment<'db>` | search paths etc. |

And the constructor, if you ever need it explicitly:

```rust
ProgramFile::new(db, file, program)
```

You will almost always use `db.program_file(file)` instead, which finds the
right `Program` for you.

### Where each one gets used

This table is worth memorising — it tells you which handle to reach for:

| you want to call | it wants |
|---|---|
| `source_text(db, …)` / `line_index(db, …)` | `File` |
| `parsed_module(db, …)` | **`PythonFile`** |
| `semantic_index(db, …)` | **`ProgramFile`** |
| `SemanticModel::new(db, …)` | **`ProgramFile`** |
| `file_to_module(db, …)` | **`ResolverFile`** |
| `ty_ide::outgoing_calls(db, …, offset)` | **`ProgramFile`** |

---

## The two-line prologue

Every analysis in ty — and every analysis you write — starts with these two
lines. `ty_ide::outgoing_calls` opens with exactly this **[verified,
`outgoing_calls.rs:34`]**:

```rust
let module = parsed_module(db, file.python_file(db)).load(db);
let model  = SemanticModel::new(db, file);
```

where `file` is a `ProgramFile`. Learn it as a unit. When you start exercise 02
you will type it without thinking, and when you read ty's source you will
recognise it instantly.

---

## Lifetimes: `'db` shows up here

`ProgramFile<'db>` has a lifetime parameter. So do `PythonFile<'db>`,
`Type<'db>`, `Definition<'db>`. `File` does not.

**Rust note.** The `'db` says: *this value is only valid while that borrow of the
database is alive.* It is not decoration — it is the compiler enforcing a real
rule:

> You cannot keep ty values across a database mutation.

Which is correct, because a mutation may invalidate them. The practical
consequence for your code:

```rust
struct MyCache {
    program_file: ProgramFile<'???>,     // ✗ don't. what lifetime would this be?
}

struct MyCache {
    path: SystemPathBuf,                 // ✓ owned, 'static, survives anything
    range: TextRange,
}
```

**Store owned keys; re-derive ty values when you need them.** Re-deriving is a
cache hit, so it costs almost nothing. `plan/01-crates/02` states this as a
design rule; the lifetime is the compiler making sure you follow it.

If you find yourself fighting `'db` in a struct definition, that is the signal to
lower to owned data — not to add more lifetime parameters.

---

## Example 1 — all four handles for one file

```rust
use ruff_db::files::system_path_to_file;
use ruff_db::system::{OsSystem, SystemPath};
use ty_project::{ProjectDatabase, ProjectMetadata};
use ty_python_semantic::Db as _;

fn main() -> anyhow::Result<()> {
    let dir = std::env::args().nth(1).expect("usage: prog <dir> <file>");
    let file_arg = std::env::args().nth(2).expect("usage: prog <dir> <file>");

    let system = OsSystem::new(&dir);
    let metadata = ProjectMetadata::discover(SystemPath::new(&dir), &system)?;
    let db = ProjectDatabase::use_defaults(metadata, system);

    let file = system_path_to_file(&db, SystemPath::new(&file_arg))?;
    let program_file = db.program_file(file);

    println!("File          {:?}", file.path(&db));
    println!("ProgramFile   {:?}", program_file);
    println!("PythonFile    {:?}", program_file.python_file(&db));
    println!("ResolverFile  {:?}", program_file.resolver_file(&db));
    println!("version       {}", program_file.python_version(&db));

    // round trip
    assert_eq!(file, program_file.file(&db));
    println!("round trip OK");

    Ok(())
}
```

The debug output for the interned handles will look like small ids, not like
paths. That is the point — they are keys.

---

## Example 2 — a helper you will use everywhere

```rust
use ruff_db::files::{File, system_path_to_file};
use ruff_db::system::SystemPath;
use ty_python_core::ProgramFile;
use ty_python_semantic::Db as _;
use ty_project::ProjectDatabase;

/// Open `path` in `db` and return both handles you will need.
pub fn open<'db>(
    db: &'db ProjectDatabase,
    path: &SystemPath,
) -> anyhow::Result<(File, ProgramFile<'db>)> {
    let file = system_path_to_file(db, path)?;
    let program_file = db.program_file(file);
    Ok((file, program_file))
}
```

**Rust note — the `'db` in the signature.** The returned `ProgramFile<'db>`
borrows from `db`, so the signature must say so: the lifetime on `&'db
ProjectDatabase` and on `ProgramFile<'db>` are the same, which tells the
compiler "the result lives as long as the borrow you gave me". Without it, Rust
cannot know, and you get a lifetime error.

This is the pattern for every function of yours that returns a ty value. Write
it once here and it stops being mysterious.

---

## Exercise

**A.** Run example 1 against:

```
experience/03-the-database/python/proj              src/app/main.py
experience/04-python-version/python/proj-requires39 app.py
experience/04-python-version/python/proj-tytoml313  app.py
```

Same code, three projects. What differs between the three `ProgramFile`s? What
differs between the three `PythonFile`s? Predict before running.

**B.** Write the `open` helper into `src/db.rs`.

**C.** Try to write this struct and read the compiler error:

```rust
struct Analysis {
    program_file: ProgramFile<'static>,
}
```

Then fix it by storing a `SystemPathBuf` instead. Write down in one sentence why
the compiler was right.

**D.** For two *different* projects that both target the same Python version,
print `program_file.python_file(&db)` for the same file path. Are they equal?
What does your answer say about what the parse cache can share? (This one is
genuinely interesting — think about it before you run it.)

---

## Exam

**1.** Name the four file handles and what each one is `File` plus.

**2.** Quote (or paraphrase) ty's justification for having three rich handles.

**3.** For each, say which handle it needs: `source_text`, `parsed_module`,
`semantic_index`, `file_to_module`, `SemanticModel::new`.

**4.** Two projects target 3.12 with different search paths. Which caches can
they share for the same file, and which must they not?

**5.** Write the two-line prologue from memory. Which handle does it start from?

**6.** What does the `'db` lifetime on `ProgramFile<'db>` prevent you from doing?

**7.** You want to remember, between requests, which function you were analysing.
What do you store and what do you not?

**8.** Why does the *type* live in `ty_python_core` but the *constructor method*
on a trait in `ty_python_semantic`? What error does that cause if you import only
one?

---

## Answers

**1.**

- `File` — a path in the database.
- `PythonFile` = `File` + **Python version** → enough to parse.
- `ResolverFile` = `File` + **module-resolution environment** → enough to follow
  imports.
- `ProgramFile` = `File` + **the whole program** (version, environment,
  platform) → enough to infer types.

**2.** From the source: it lets programs with the same Python version share
parsed syntax, and programs with equivalent resolver environments share module
resolution, while keeping type inference isolated. In other words: **each handle
is a cache key chosen to share as much as is safe.**

**3.** `source_text` → `File`. `parsed_module` → `PythonFile`. `semantic_index` →
`ProgramFile`. `file_to_module` → `ResolverFile`. `SemanticModel::new` →
`ProgramFile`.

**4.** They can share **source text** (keyed on `File`) and the **AST** (keyed on
`PythonFile`, and the versions match). They must not share **module resolution**
(search paths differ, so `import config` may go elsewhere) or **type
inference** (which depends on module resolution).

**5.**

```rust
let module = parsed_module(db, file.python_file(db)).load(db);
let model  = SemanticModel::new(db, file);
```

It starts from a **`ProgramFile`** — that is what `file` is in ty's own code.

**6.** Keeping the value alive across a **database mutation**, and storing it in
any long-lived structure. Both would let you hold a handle that salsa may have
invalidated.

**7.** Store owned, `'static` data: the file path (`SystemPathBuf`), a
`TextRange`, or a qualified name string. Do **not** store `ProgramFile<'db>`,
`Definition<'db>`, `Type<'db>`, or a `ParsedModuleRef`. Re-derive them from the
owned key on the next request — that is a cache hit.

**8.** Because `ty_python_core` defines the semantic-index layer's types, while
the `Db` trait that knows how to *find the right `Program`* for a file is part of
the type-inference layer above it. Importing only the type gives you "no method
named `program_file`"; importing only the trait gives you "cannot find type
`ProgramFile`". You need both lines.
