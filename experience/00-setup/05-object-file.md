# Object 5 — `File`

A file, as far as the database is concerned. Not its contents.

---

## What it is

A **handle**. An identity. Internally it is a small integer that salsa assigns to
a path the first time anyone mentions it.

```rust
let file: File = system_path_to_file(&db, path)?;
```

`file` does **not** contain the source code. It is a ticket you present to the
database to ask questions:

```rust
source_text(&db, file)      // → the text
line_index(&db, file)       // → the line table
parsed_module(&db, pyfile)  // → the AST     (needs PythonFile — object 6)
```

**Rust note.** `File` is `Copy` — eight bytes, cheap to pass around, no
lifetimes, no cloning ceremony. Pass it by value everywhere.

### The mental model

```
   "/proj/src/main.py"  ──system_path_to_file──►  File(#42)
                                                    │
                                                    │  source_text(db, File(#42))
                                                    ▼
                                                  "def main():\n    ..."
```

The `File` is the *name*; the database holds the *value*. This split is what
makes caching possible: the handle stays stable while the content changes, so
salsa can say "file #42 is now at revision 7" and invalidate exactly the queries
that read it.

Compare with your Python driver, where `scanner.py`'s cache is keyed on the
**content string** — so a new string is a new key and nothing survives an edit.

---

## Where it comes from

```rust
use ruff_db::files::{File, FileError, system_path_to_file};
```

---

## Opening one

```rust
pub fn system_path_to_file(db: &dyn Db, path: impl AsRef<SystemPath>)
    -> Result<File, FileError>;
```

**[verified]**. And the error type is small **[verified,
`ruff_db/src/files.rs:653`]**:

```rust
pub enum FileError {
    IsADirectory,
    NotFound,
}
```

Two cases, both of which you will hit within ten minutes of writing a CLI.

**Rust note — `&dyn Db`.** The parameter is a *trait object*. You pass `&db`
where `db: ProjectDatabase`, and Rust converts automatically because
`ProjectDatabase` implements `ruff_db::Db`. If the compiler complains, being
explicit — `&db as &dyn ruff_db::Db` — usually resolves it. You need
`use ruff_db::Db as _;` in scope for this in some positions.

---

## The surprising part: files exist even when they do not

Salsa keeps an entry for paths that are **not on disk**. The doc comment says
why **[verified]**:

> The map also stores entries for files that don't exist on the file system.
> This is necessary so that queries that depend on the existence of a file are
> re-executed when the file is created.

Think about what that means. `import config` fails to resolve today because
`config.py` does not exist. That resolution result is cached. When the user
creates `config.py`, something must invalidate the cache — and the only way is
if the *absence* was recorded as a dependency.

So a `File` can exist and refer to nothing. Hence:

```rust
file.exists(&db)      // -> bool
```

Do not assume a `File` you were handed is real. This is the same lesson as
`Module::file()` returning `Option` in exercise 05: **absence is data**.

---

## What you can do with it

**[verified]** from `ruff_db/src/files.rs`.

### Identity and metadata

| method | returns | notes |
|---|---|---|
| `path(&db)` | `&FilePath` | ★ where it is — see below |
| `exists(&db)` | `bool` | ★ is it actually there |
| `source_type(&db)` | `PySourceType` | ★ py / pyi / ipynb |
| `is_stub(&db)` | `bool` | ★ is this a `.pyi` |
| `is_package(&db)` | `bool` | is this an `__init__.py` |
| `revision(&db)` | `FileRevision` | changes when the file changes |
| `permissions(&db)` | `Option<u32>` | unix only |

### Content

```rust
ruff_db::source::source_text(&db, file)   // -> SourceText   #[salsa::tracked]
ruff_db::source::line_index(&db, file)    // -> LineIndex    #[salsa::tracked]
```

Both are **cached queries**, not methods. `SourceText::as_str()` gives you the
text; `read_error()` tells you if the read failed.

`line_index` is exercise 01's `LineIndex`, already built and shared. Use this
rather than `LineIndex::from_source_text` whenever the file is in the database.

### Telling the database it changed

| function | scope |
|---|---|
| `File::sync_path(&mut db, path)` | ★ one path |
| `File::sync_all(&mut db)` | everything |
| `file.sync(&mut db)` | this file |

⚠ All take `&mut db`, which cancels in-flight queries. **Batch them.** And
forgetting to call one after writing a file is exercise 10's headline bug.

### `FilePath` — three kinds of location

```rust
pub enum FilePath {
    System(Box<SystemPath>),          // ★ a real file on disk
    SystemVirtual(Box<SystemVirtualPath>),  // editor buffer, untitled document
    Vendored(Box<VendoredPath>),      // ★ inside ty's binary — typeshed stubs
}
```

**[verified, `ruff_db/src/files/path.rs:15`]**.

This is where "a `File` may have no disk path" becomes concrete. `import json`
resolves to a `Vendored` path. If your code does
`file.path(&db).as_system_path().unwrap()`, it panics the first time it meets
the standard library — which is roughly the first real file you analyse.

**Handle all three, or filter to `System` deliberately and say so.**

---

## Example 1 — open a file and read it

```rust
use ruff_db::files::system_path_to_file;
use ruff_db::source::source_text;
use ruff_db::system::{OsSystem, SystemPath};
use ty_project::{ProjectDatabase, ProjectMetadata};

fn main() -> anyhow::Result<()> {
    let dir = std::env::args().nth(1).expect("usage: prog <dir> <file>");
    let file_arg = std::env::args().nth(2).expect("usage: prog <dir> <file>");

    let system = OsSystem::new(&dir);
    let metadata = ProjectMetadata::discover(SystemPath::new(&dir), &system)?;
    let db = ProjectDatabase::use_defaults(metadata, system);

    let file = system_path_to_file(&db, SystemPath::new(&file_arg))?;

    println!("path        = {:?}", file.path(&db));
    println!("exists      = {}", file.exists(&db));
    println!("source_type = {:?}", file.source_type(&db));
    println!("is_stub     = {}", file.is_stub(&db));
    println!("is_package  = {}", file.is_package(&db));

    let text = source_text(&db, file);
    println!("bytes       = {}", text.as_str().len());
    println!("first line  = {:?}", text.as_str().lines().next());

    Ok(())
}
```

Run it on a few of the fixtures:

```bash
cargo run -- experience/03-the-database/python/proj \
             experience/03-the-database/python/proj/src/app/main.py

cargo run -- experience/03-the-database/python/proj \
             experience/03-the-database/python/proj/src/app/__init__.py   # is_package?

cargo run -- experience/01-source-and-positions/python \
             experience/01-source-and-positions/python/unicode.py         # bytes vs chars?
```

For `unicode.py`, `bytes` should print **88**, not 79. The file is 88 bytes and
79 characters — exercise 01's whole subject, visible here for the first time.

---

## Example 2 — handling both errors properly

```rust
use ruff_db::files::{system_path_to_file, FileError};

match system_path_to_file(&db, SystemPath::new(&file_arg)) {
    Ok(file) => {
        println!("opened {:?}", file.path(&db));
    }
    Err(FileError::NotFound) => {
        eprintln!("no such file: {file_arg}");
    }
    Err(FileError::IsADirectory) => {
        eprintln!("that is a directory, not a file: {file_arg}");
    }
}
```

**Rust note — `match` on an enum.** The compiler checks you covered every
variant. Add a third variant to `FileError` upstream and this stops compiling —
which is the good kind of breakage. That is why matching explicitly beats
`if let ... else`.

For your driver, both cases are "log and carry on", never a crash (quirk 13).

---

## Example 3 — the vendored path trap

```rust
use ruff_db::files::FilePath;

fn system_path_of(db: &ProjectDatabase, file: File) -> Option<&SystemPath> {
    match file.path(db) {
        FilePath::System(p)        => Some(p),
        FilePath::SystemVirtual(_) => None,   // editor buffer, not on disk
        FilePath::Vendored(_)      => None,   // inside our own binary
    }
}
```

Write this now, as a helper, in `src/db.rs`. Every later exercise that asks "is
this project code?" or "print the file path" needs it, and writing it as a
`match` means you have already thought about the two cases that would otherwise
panic.

---

## Exercise

**A.** Get example 1 running. Then run it on a path that does not exist and one
that is a directory. Confirm you get the two distinct errors, then implement
example 2's handling.

**B.** For the four files in `experience/03-the-database/python/proj/src/app/`,
print a table: name, `exists`, `source_type`, `is_package`, byte length. Predict
`is_package` for each before running.

**C.** Open a file that does **not** exist, then check `exists(&db)` on the
resulting `File`. (You will need to construct the `File` a different way — look
for a function that returns a `File` without a `Result`, or think about why
`system_path_to_file` cannot give you one. Write down what you conclude.)

**D.** Write `system_path_of` from example 3 and use it to print the path of a
file. Then, in exercise 05, come back and run it on `import json`'s resolved
file — that is where the `Vendored` arm finally fires.

---

## Exam

**1.** What is a `File` — what does it contain, and what does it not contain?

**2.** Why is a handle-plus-database better than an object holding its own
content? Answer in terms of caching.

**3.** `scanner.py` caches on the file's content string. Name the property a
`File` has that a content string does not.

**4.** What are the two `FileError` variants, and how should your driver react to
each?

**5.** Why does salsa keep entries for files that do not exist? Give the concrete
scenario.

**6.** Name the three `FilePath` variants and give an example input that produces
each.

**7.** What panics if you assume every `File` has a system path, and roughly how
soon does it happen in a real project?

**8.** `source_text` and `line_index` are functions, not methods on `File`. Why
does that distinction matter?

**9.** You write a file to disk. What must you call, and what happens if you do
not?

---

## Answers

**1.** An identity — a small salsa-assigned handle for a path. It contains no
source text, no AST, no types. Everything is looked up from the database using
the handle as the key.

**2.** Because the handle is **stable across changes** while the content is not.
Salsa can record "query Q read file #42 at revision 6", and when #42 moves to
revision 7 it knows exactly what to recompute. If the identity were the content,
every edit would create a brand-new identity with no relationship to the old one
— which is precisely why an `lru_cache` keyed on content cannot be incremental.

**3.** **Stability.** The `File` for `/proj/main.py` is the same handle before
and after an edit. The content string is a different key every keystroke, so no
dependency can be tracked across time.

**4.** `NotFound` and `IsADirectory`. Both are log-and-continue for your driver —
a missing or wrong path must never crash the analyser (quirk 13). Distinguish
them in the message, because "you passed a directory" is a different user
mistake from "that file is gone".

**5.** So that a query which depended on a file's **absence** can be invalidated
when it appears. Scenario: `import config` does not resolve, and that failure is
cached. The user creates `config.py`. Without an entry recording the absence,
nothing would tell salsa the earlier answer is stale, and the import would stay
broken until a restart.

**6.** `System` — an ordinary file on disk. `SystemVirtual` — an editor buffer or
untitled document that has no disk location. `Vendored` — a typeshed stub inside
ty's own binary, e.g. whatever `import json` resolves to.

**7.** Anything of the form `file.path(db).as_system_path().unwrap()` (or a match
that ignores the other arms). It happens as soon as you touch the standard
library — so, in practice, on the first real file, because almost every Python
file imports something from stdlib.

**8.** Because they are `#[salsa::tracked]` **queries**, not properties of the
handle. They take the database, they are cached, and their results are
invalidated by revision changes. A method on `File` would suggest the data lives
*in* the file object, which is exactly the wrong mental model.

**9.** `File::sync_path(&mut db, &path)`. Without it, every later query serves
the pre-write content — so in exercise 10 your injected IDs never appear, the
file on disk looks correct, and nothing errors. Batch the syncs, because `&mut
db` cancels in-flight queries.
