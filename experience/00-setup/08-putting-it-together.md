# 08 — Putting it together: the smoke test

Seven objects, one program, twenty lines. Now we go through it slowly.

**Do not read this file until you have read objects 1–7.** It assumes every
type in it is already familiar.

---

## What the program does

```
   a directory path                a file path
        │                               │
        ▼                               │
   OsSystem                             │           object 2
        │                               │
        ▼                               │
   ProjectMetadata::discover            │           object 3
        │                               │
        ▼                               │
   ProjectDatabase ◄────────────────────┘           object 4
        │
        ├── system_path_to_file  ──►  File          object 5
        │                               │
        │        db.program_file  ──►  ProgramFile  object 6
        │                               │
        └───────────────────────────►  PythonVersion  object 7
                                        │
                                        ▼
                                     printed
```

Every arrow is a call you now understand. Nothing else is happening.

---

## The complete program

Type this into `src/main.rs`. Do not paste it — typing it is how the imports and
the shapes stick.

```rust
use ruff_db::files::{system_path_to_file, FileError};
use ruff_db::system::{OsSystem, SystemPath};
use ty_project::{ProjectDatabase, ProjectMetadata};
use ty_python_semantic::Db as _;

fn main() -> anyhow::Result<()> {
    // ---- 1. arguments ------------------------------------------------
    let dir = std::env::args()
        .nth(1)
        .expect("usage: pylspt <project-dir> <file.py>");
    let file_arg = std::env::args()
        .nth(2)
        .expect("usage: pylspt <project-dir> <file.py>");

    // ---- 2. the filesystem -------------------------------------------
    let system = OsSystem::new(&dir);

    // ---- 3. the configuration ----------------------------------------
    let metadata = ProjectMetadata::discover(SystemPath::new(&dir), &system)?;
    println!("project root   = {}", metadata.root());

    // ---- 4. the database (takes ownership of `system`) ---------------
    let db = ProjectDatabase::use_defaults(metadata, system);

    // ---- 5. the file --------------------------------------------------
    let file = match system_path_to_file(&db, SystemPath::new(&file_arg)) {
        Ok(file) => file,
        Err(FileError::NotFound) => {
            eprintln!("no such file: {file_arg}");
            return Ok(());
        }
        Err(FileError::IsADirectory) => {
            eprintln!("that is a directory, not a file: {file_arg}");
            return Ok(());
        }
    };

    // ---- 6. the file, with its program context -----------------------
    let program_file = db.program_file(file);

    // ---- 7. the answer ------------------------------------------------
    let version = program_file.python_version(&db);
    let source = db.python_version_with_source(file);

    println!("file           = {:?}", file.path(&db));
    println!("python_version = {version}");
    println!("version source = {:?}", source.source);

    Ok(())
}
```

---

## Line by line

### The imports

```rust
use ruff_db::files::{system_path_to_file, FileError};
use ruff_db::system::{OsSystem, SystemPath};
use ty_project::{ProjectDatabase, ProjectMetadata};
use ty_python_semantic::Db as _;
```

Four lines, four crates. The last one is the odd one: it imports a **trait**, not
a type, and `as _` means "for method calls only, do not bind the name" (object
4). Delete it and line 6 stops compiling.

### `fn main() -> anyhow::Result<()>`

**Rust note.** A `main` returning `Result` lets you use `?`. On `Err`, Rust
prints the error and exits non-zero. `()` is the unit type — "no useful value" —
so `Ok(())` at the end means "finished, nothing to report".

`anyhow::Result<T>` is shorthand for `Result<T, anyhow::Error>`, and
`anyhow::Error` swallows any error type. That is why one `?` can handle both a
`ProjectMetadataError` and a `FileError` without conversion boilerplate.

### 1. Arguments

```rust
let dir = std::env::args().nth(1).expect("usage: …");
```

`args()` is an iterator; item 0 is the program's own path, so `nth(1)` is the
first real argument. It returns `Option<String>`; `.expect(msg)` unwraps it or
panics with your message.

Fine for now. A real CLI would use `clap` — exercise 11.

### 2. The filesystem

```rust
let system = OsSystem::new(&dir);
```

`&dir` is a `&String`, and `new` takes `impl AsRef<SystemPath>`, so Rust
converts. The argument is this system's working directory (object 2).

### 3. The configuration

```rust
let metadata = ProjectMetadata::discover(SystemPath::new(&dir), &system)?;
```

`SystemPath::new(&dir)` borrows the `String` as a path — no allocation.
Discovery walks **up** from `dir` looking for config, and falls back to `dir`
itself (object 3). The `?` propagates *broken* configuration, not *missing*
configuration.

`metadata.root()` prints without `.as_str()` because `SystemPath` implements
`Display`.

### 4. The database

```rust
let db = ProjectDatabase::use_defaults(metadata, system);
```

Both arguments are **moved**. After this line, `metadata` and `system` are gone
— using either is `error[E0382]: borrow of moved value`. That is why the
`println!` of `metadata.root()` is on the line *above*.

`use_defaults` never fails; it substitutes defaults for anything misconfigured
(object 4).

### 5. The file

```rust
let file = match system_path_to_file(&db, SystemPath::new(&file_arg)) { … };
```

`&db` coerces to the `&dyn Db` the function wants. The `match` handles both
error variants explicitly, which is what your driver should do everywhere —
never crash on a bad path (quirk 13).

**Rust note.** `return Ok(())` inside a match arm exits `main` early. The arms
have to agree on a type: two of them return from the function, and the third
evaluates to a `File`, so the whole `match` has type `File`. That is why `let
file = match …` works.

### 6. The program context

```rust
let program_file = db.program_file(file);
```

`file` is `Copy`, so this does not move it — you can still use `file` afterwards
(and line 7 does). This is the trait method that needs the `Db as _` import
(objects 4 and 6).

### 7. The answer

```rust
let version = program_file.python_version(&db);
let source = db.python_version_with_source(file);
```

Note the asymmetry: the version comes off the `ProgramFile`, the *source* of the
version comes off the `db` with a `File`. Both are public **[verified]**; there
is no `Program::get` (object 7).

---

## Expected output

```
$ cargo run -- experience/03-the-database/python/proj \
               experience/03-the-database/python/proj/src/app/main.py

project root   = /Users/yared/.../experience/03-the-database/python/proj
file           = System("/Users/yared/.../src/app/main.py")
python_version = 3.11
version source = ConfigFile(...)
```

`3.11` because that fixture's `pyproject.toml` says
`requires-python = ">=3.11"`, and rule 3 takes the **lower bound**.

The `file` line shows `System(...)` — the `FilePath` enum variant from object 5.
When you eventually print a stdlib file, that will read `Vendored(...)` instead.

---

## The error catalogue

Every one of these will happen to you. Recognising them is worth more than
avoiding them.

### `error[E0432]: unresolved import 'ruff_db::system::OsSystem'`

You did not enable the `os` feature on `ruff_db`. Object 2.

```toml
ruff_db = { git = "…", rev = "ac201b8", features = ["os"] }
```

### `error[E0599]: no method named 'program_file' found for struct 'ProjectDatabase'`

Missing trait import. Object 4.

```rust
use ty_python_semantic::Db as _;
```

**The general rule:** "no method named X" when X is visibly in the docs means a
trait is not in scope. You will meet this with `System`, with `Db`, and later
with `Ranged` and `HasType`.

### `error[E0382]: borrow of moved value: 'system'`

You used `system` (or `metadata`) after `use_defaults` took ownership. Move your
use above that line.

### `error[E0277]: the trait bound '…: ruff_db::Db' is not satisfied` — with both types printing identically

Two copies of a crate in your dependency graph, from mismatched `rev` values.
Run `cargo tree -d`. Object: exercise 00's manifest section.

### `error: rustc 1.94.1 is not supported by the following packages`

Missing `rust-toolchain.toml`. This fails during *resolution*, before compiling
anything, and lists ~25 crates requiring 1.96.

```bash
rustup toolchain install 1.98.0
printf '[toolchain]\nchannel = "1.98.0"\n' > rust-toolchain.toml
```

### `error[E0106]: missing lifetime specifier`

You are writing a function that returns a ty value — `ProgramFile`, `Type`,
`Definition` — and have not told the compiler how long it lives. Tie it to the
database borrow. Object 6, example 2:

```rust
pub fn open<'db>(db: &'db ProjectDatabase, path: &SystemPath)
    -> anyhow::Result<(File, ProgramFile<'db>)>
```

If the lifetimes get complicated, that is the signal to **return owned data
instead** (a `String`, a `TextRange`) rather than to add more parameters.

---

## Exercise

**A.** Get the program running. Then run it against all six of these and record
version + source for each:

```
03-the-database/python/proj                  src/app/main.py
04-python-version/python/proj-bare           app.py
04-python-version/python/proj-requires39     app.py
04-python-version/python/proj-tytoml313      app.py
05-modules-and-imports/python/proj           src/pkg/core.py
01-source-and-positions/python               unicode.py         ← no project at all
```

The last one has no `pyproject.toml` anywhere. Predict what happens before
running it.

**B.** Trigger three of the errors from the catalogue on purpose — the feature
flag, the trait import, and the moved value. Read each message. Fix each. Ten
minutes, and those three stop costing you time forever.

**C.** Refactor: move everything except argument handling into
`src/db.rs`, exposing

```rust
pub fn open_project(dir: &SystemPath) -> anyhow::Result<ProjectDatabase>
pub fn open<'db>(db: &'db ProjectDatabase, path: &SystemPath)
    -> anyhow::Result<(File, ProgramFile<'db>)>
```

`main.rs` should then be about ten lines. File 09 explains why this split
matters before you write anything else.

**D.** Add a third argument: if the user passes `--json`, print the same
information as a JSON object using `serde_json`. You will need
`serde_json::json!` and `to_string_pretty`. This is the first step of the CLI you
finish in exercise 11.

---

## Exam

**1.** Put these in the order the program uses them, and say what each produces:
`ProjectDatabase`, `File`, `OsSystem`, `PythonVersion`, `ProjectMetadata`,
`ProgramFile`.

**2.** Which two values are *moved* into the database, and what must you do
before that line if you need them?

**3.** Why is `file` still usable after `db.program_file(file)`?

**4.** The version comes from `program_file`, but the version *source* comes from
`db` with a `File`. Why the asymmetry? (One honest sentence is enough — this is
an API shape, not a deep truth.)

**5.** For each error, give the one-line fix:
- `unresolved import OsSystem`
- `no method named program_file`
- `borrow of moved value: system`
- `rustc 1.94.1 is not supported`
- both types print identically in a trait-bound error

**6.** The fixture at `03-the-database/python/proj` reports 3.11. Where did that
come from, and which of the five version sources is it?

**7.** You run the program on `01-source-and-positions/python/unicode.py`, where
there is no project configuration at all. What version do you expect, and by
which rule?

**8.** `main` returns `anyhow::Result<()>`. What does that buy you, and what does
`()` mean?

---

## Answers

**1.**

| order | object | produces |
|---|---|---|
| 1 | `OsSystem` | access to the real filesystem |
| 2 | `ProjectMetadata` | the project's root + merged options |
| 3 | `ProjectDatabase` | the cache and the query engine |
| 4 | `File` | a handle for one path |
| 5 | `ProgramFile` | that file + its program context |
| 6 | `PythonVersion` | the answer you print |

**2.** `metadata` and `system`, both moved by `use_defaults`. Anything you need
from them — like `metadata.root()` — must be read (or cloned) **before** that
line.

**3.** Because `File` is `Copy` — passing it copies the handle instead of moving
it. That is deliberate: it is a small integer identity, and ty passes it by value
everywhere.

**4.** Because `python_version` is a natural property of a `ProgramFile` (the
program *is* the thing that has a version), whereas the *source* is diagnostic
information the project database tracks per file, so it lives on the
`ty_python_semantic::Db` trait. It is API shape, not a principle — and worth
noting because it is the kind of asymmetry that makes you hunt for a method on
the wrong type.

**5.**

- add `features = ["os"]` to `ruff_db`
- add `use ty_python_semantic::Db as _;`
- move the use of `system` above `use_defaults`
- create `rust-toolchain.toml` with `channel = "1.98.0"`
- mismatched `rev`s → make every git dependency use the same one; check with
  `cargo tree -d`

**6.** From `pyproject.toml`'s `requires-python = ">=3.11"` — source #3, the
**lower bound** of the declared range. Not the installed interpreter, and not
`latest_ty()`.

**7.** **3.14** (`latest_ty()`), by rule 5 — the fallback — since there is no
config file and no environment to resolve.

If you got something else, discovery found a project or an interpreter somewhere
up the directory tree. That is worth investigating rather than shrugging at: it
means your "isolated" fixtures are not isolated, which will make later exercises
give inconsistent answers.

**8.** It lets you use `?` on any error type inside `main` — `anyhow::Error`
converts from anything implementing `std::error::Error` — and Rust prints the
error and returns a non-zero exit code for you. `()` is the unit type, meaning
the success case carries no value; `Ok(())` is "done, nothing to report".
