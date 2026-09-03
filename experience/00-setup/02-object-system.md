# Object 2 — `System` and `OsSystem`

The thing that reads files. Everything else in ty goes through it.

---

## What it is

`System` is a **trait** — an interface. It describes "a filesystem" as a set of
operations: read a file, does this path exist, list a directory, what is the
current directory.

`OsSystem` is one **implementation** of that trait: the real filesystem on your
machine.

```
        trait System                    ← the interface: "a filesystem"
        ├── OsSystem                    ← the real disk. what you use.
        ├── TestSystem                  ← an in-memory fake, for ty's own tests
        └── (others)
```

**Rust note — traits.** A trait is like a Python protocol or an ABC: a set of
method signatures a type can promise to provide. `impl System for OsSystem`
means "`OsSystem` provides all of these". Code written against `&dyn System`
works with any implementation.

If you have not read `tutorial/02-rust-refresher.md` §2.8, this is the moment.

---

## Why the indirection exists

It would be simpler for ty to call `std::fs::read_to_string` directly. Three
reasons it does not, and each one matters to you:

**1. Tests without a disk.** ty's own test suite creates whole projects in
memory. No temp directories, no cleanup, no flakiness. When you write tests for
your driver later, you will want this too.

**2. The editor's unsaved buffer.** Your `parse_file` RPC receives `content` from
the client that may not be on disk yet. A filesystem abstraction is what lets
"the file" mean "what the editor is showing" rather than "what is saved".

**3. The vendored typeshed.** `import json` resolves to a stub file *compiled
into your binary*. It has no disk path at all. That is a different filesystem,
and only an abstraction can hold both.

---

## Where it comes from

```rust
use ruff_db::system::{System, OsSystem};
```

⚠ **`OsSystem` only exists if you enabled the `os` feature** on `ruff_db`:

```toml
ruff_db = { git = "…", rev = "ac201b8", features = ["os"] }
```

**[verified]** — `ruff_db/src/system.rs:7` is `#[cfg(feature = "os")]`, and
`ruff_db` declares no default features. Without the flag you get:

```
error[E0432]: unresolved import `ruff_db::system::OsSystem`
```

and nothing else is wrong. The crate compiles fine; the type simply is not in
your build. This is the single most common way to get stuck on this exercise.

---

## Making one

```rust
let system = OsSystem::new("/Users/yared/proj");
```

**[verified]** — `OsSystem::new(cwd: impl AsRef<SystemPath>) -> Self`.

The argument is the **current working directory** for this system: what relative
paths resolve against. Pass your project root.

**Rust note — `impl AsRef<SystemPath>`.** This means "anything that can be viewed
as a `SystemPath`". All of these work:

```rust
OsSystem::new("/abs/path")                        // &str
OsSystem::new(String::from("/abs/path"))          // String
OsSystem::new(&some_system_path_buf)              // &SystemPathBuf
OsSystem::new(some_system_path_buf)               // SystemPathBuf
```

You will see `impl AsRef<T>` all over ruff. It is the Rust idiom for "be
convenient about what callers pass in". When you see it, stop worrying about the
exact type you have.

---

## What you can do with it

**[verified]** from the `System` trait in `ruff_db/src/system.rs`. You will
rarely call these directly — ty calls them for you — but knowing what is there
tells you what ty can and cannot see.

### Reading

| method | returns |
|---|---|
| `read_to_string(&path)` | `Result<String>` — ★ the file's text |
| `read_to_notebook(&path)` | `Result<Notebook, NotebookError>` — `.ipynb` only |
| `read_directory(&path)` | iterator of entries |
| `walk_directory(&path)` | ★ a builder for recursive traversal |

### Asking

| method | returns |
|---|---|
| `path_exists(&path)` | `bool` |
| `is_file(&path)` / `is_directory(&path)` | `bool` |
| `path_metadata(&path)` | `Result<Metadata>` — revision, permissions |
| `source_type(&path)` | `Option<PySourceType>` — ★ py / pyi / ipynb |
| `canonicalize_path(&path)` | `Result<SystemPathBuf>` — resolve symlinks |
| `is_same_file(a, b)` | `Result<bool>` |

### Environment

| method | returns |
|---|---|
| `current_directory()` | `&SystemPath` |
| `env_var(name)` | `Result<String, VarError>` |
| `user_config_directory()` | `Option<SystemPathBuf>` |
| `cache_dir()` | `Option<SystemPathBuf>` |
| `which(binary_name)` | find an executable — this is how uv/python get located |

### Writing

Writing is a **separate trait**, `WritableSystem`, reached through:

```rust
system.as_writable()      // -> Option<&dyn WritableSystem>
```

with `write_file`, `write_file_bytes`, `create_directory_all`,
`create_new_file`.

That separation is deliberate and worth noticing: **the ability to write is not
assumed**. A read-only system returns `None`. Exercise 10 (ID injection) is the
only place in your driver that needs it, and it is the only place that writes to
the user's source — the API is telling you something true about your design.

---

## Example 1 — read a file through the system

```rust
use ruff_db::system::{OsSystem, System, SystemPath};

fn main() -> anyhow::Result<()> {
    let root = std::env::args().nth(1).expect("usage: prog <dir> <file>");
    let file = std::env::args().nth(2).expect("usage: prog <dir> <file>");

    let system = OsSystem::new(&root);

    let path = SystemPath::new(&file);
    println!("exists      = {}", system.path_exists(path));
    println!("is_file     = {}", system.is_file(path));
    println!("source_type = {:?}", system.source_type(path));

    let text = system.read_to_string(path)?;
    println!("{} bytes, first line: {:?}", text.len(), text.lines().next());

    Ok(())
}
```

**Rust notes:**

- `use ruff_db::system::System;` — **you must import the trait to call its
  methods.** Without that line, `system.path_exists(...)` fails with "no method
  named `path_exists`". This will happen to you repeatedly in ty; remember the
  symptom.
- `-> anyhow::Result<()>` plus `?`. The `?` operator says "if this is an error,
  return it from this function; otherwise unwrap it". `anyhow::Result` accepts
  any error type, which is why it is convenient in a `main`.
- `SystemPath::new(&file)` borrows the `String` as a path — no allocation.

---

## Example 2 — every Python file under a directory

```rust
use ruff_db::system::{OsSystem, System, SystemPath};

fn main() -> anyhow::Result<()> {
    let root = std::env::args().nth(1).expect("usage: prog <dir>");
    let system = OsSystem::new(&root);

    let mut count = 0;
    for entry in system.walk_directory(SystemPath::new(&root)).build() {
        let entry = entry?;
        let path = entry.path();
        if path.extension() == Some("py") {
            println!("{}", path.as_str());
            count += 1;
        }
    }
    println!("{count} Python files");
    Ok(())
}
```

⚠ **[check]** the exact shape of `walk_directory`'s builder and its entry type at
your revision — it returns a `WalkDirectoryBuilder`, and the `.build()` /
iteration details are the part most likely to differ from what I wrote. Use
`cargo doc -p ruff_db --no-deps --open` and search for `WalkDirectoryBuilder`.

Getting this working is genuinely useful: it is how you will feed a corpus to
your CLI in exercise 11.

---

## Exercise

**A.** Take example 1 and make it not crash when the file does not exist. Print a
friendly message instead. (Hint: `read_to_string` returns a `Result`; use
`match` instead of `?`.)

**B.** Write a function:

```rust
fn python_files(system: &dyn System, root: &SystemPath) -> Vec<SystemPathBuf>
```

that returns every `.py` and `.pyi` file under `root`. Note the parameter type:
`&dyn System`, not `&OsSystem`. Ask yourself why that is the better signature —
then answer exam question 4.

**C.** Call `system.as_writable()` and print whether you got `Some` or `None` for
an `OsSystem`. Then find, in `ruff_db`'s docs, one system type where it would be
`None`.

---

## Exam

**1.** What is the difference between `System` and `OsSystem`?

**2.** Give the three reasons ty abstracts the filesystem instead of calling
`std::fs` directly. Which one affects *your* driver most?

**3.** You wrote `use ruff_db::system::OsSystem;` and got `E0432: unresolved
import`. The crate compiled fine. What is wrong?

**4.** Your helper takes `&dyn System` rather than `&OsSystem`. Name two things
that buys you.

**5.** You call `system.path_exists(p)` and the compiler says there is no such
method, but you can see it in the docs. What did you forget?

**6.** Why is writing on a separate trait (`WritableSystem`) reached through
`as_writable()`, rather than just being more methods on `System`?

**7.** `OsSystem::new` takes `impl AsRef<SystemPath>`. Name three different types
you can pass, and say what the idiom is for.

---

## Answers

**1.** `System` is a trait — an interface describing filesystem operations.
`OsSystem` is a concrete type implementing that trait using the real disk. One
is the contract, the other is one fulfilment of it.

**2.** In-memory testing; the editor's unsaved buffer (your `parse_file(content)`
RPC); the vendored typeshed, which is a filesystem inside your binary.

The vendored typeshed is arguably the biggest for you, because it is why a
`File` may have **no disk path at all** — a case you must handle in exercise 05
and which would otherwise arrive as a surprise `None`.

**3.** You did not enable the `os` feature on `ruff_db`. `OsSystem` is behind
`#[cfg(feature = "os")]` **[verified]** and `ruff_db` has no default features, so
the module is simply not compiled. Add `features = ["os"]`.

**4.** Your function works with any implementation — including the in-memory one
you will want for tests — and it does not force callers to have the concrete
type. It also documents the *minimum* your function needs, which makes it easier
to reason about. This is the same principle as `plan/01-crates/02`'s advice to
take `&dyn ty_python_semantic::Db` rather than `&ProjectDatabase`.

**5.** `use ruff_db::system::System;` — the trait must be in scope to call its
methods. Rust does not auto-import traits. Symptom to memorise: "no method named
X" when X clearly exists in the docs means a missing trait import.

**6.** Because not every filesystem can be written to, and the type system should
say so. A read-only system returns `None` from `as_writable()`, and a caller
*must* handle that before writing. If `write_file` were on `System`, every
implementation would have to provide one — and read-only ones would have to fail
at runtime instead of at the type level.

For your driver this maps onto something real: ID injection is the only feature
that writes, and it is the only one that would need `as_writable()`.

**7.** `&str`, `String`, `SystemPathBuf`, `&SystemPathBuf`, `&SystemPath` — all
work. The idiom means "anything convertible to a view of this type", and it
exists so callers do not have to think about which exact form they are holding.
