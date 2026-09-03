# Object 1 — `SystemPath` and `SystemPathBuf`

The first ty type you will touch. Nothing works until you can make one.

---

## What it is

A **file path**. That is all. But it is not Rust's `std::path::Path`, and the
difference matters.

```rust
std::path::Path      // Rust's standard path. May contain non-UTF-8 bytes.
ruff_db::system::SystemPath      // ty's path. ALWAYS valid UTF-8.
```

`SystemPath` is ty's own path type, built on top of the `camino` crate (which
exists purely to provide UTF-8-guaranteed paths).

### Why ty does not use `std::path::Path`

On Linux and macOS, a filename is just bytes — it does not have to be valid
text. Rust's `Path` reflects that honestly, which means every time you want to
*look* at a path as a string you get an `Option<&str>` and have to handle the
"this path is not text" case.

ty needs paths to be text, everywhere, all the time: they are hashed, interned,
compared, put in caches, and sent over JSON-RPC to editors. Handling
"unrepresentable path" at a thousand call sites would be miserable. So ty makes
the decision **once, at the boundary**: a path either converts to UTF-8 or it is
rejected.

You benefit immediately — `path.as_str()` returns `&str`, not `Option<&str>`.

---

## The borrowed/owned pair

This is a Rust pattern you will see over and over, so learn it here where it is
simple.

| borrowed (a view) | owned (has the data) |
|---|---|
| `str` | `String` |
| `Path` | `PathBuf` |
| **`SystemPath`** | **`SystemPathBuf`** |
| `[T]` | `Vec<T>` |

The left column is always used behind a reference: `&str`, `&SystemPath`. It
points at text someone else owns. The right column owns a heap allocation and
can grow.

**Rust note — deref coercion.** Because `SystemPathBuf` implements
`Deref<Target = SystemPath>` **[verified, `path.rs:695`]**, Rust will
automatically turn a `&SystemPathBuf` into a `&SystemPath` when a function asks
for the latter. So this just works:

```rust
let owned: SystemPathBuf = SystemPathBuf::from("/tmp/x.py");
some_function_taking(&owned);        // fn some_function_taking(p: &SystemPath)
```

You do not need `.as_path()` — though it exists if you want to be explicit. This
is the same reason you can pass a `&String` to a function taking `&str`.

---

## Where it comes from

```rust
use ruff_db::system::{SystemPath, SystemPathBuf};
```

---

## What you can do with it

**[verified]** from `ruff_db/src/system/path.rs`. The ones you will actually use
are marked ★.

### Making one

```rust
SystemPathBuf::from("/abs/path/main.py")     // ★ from &str
SystemPathBuf::from(some_string)             // ★ from String
SystemPath::new("/abs/path")                 // ★ borrow a &str as a &SystemPath
SystemPathBuf::from_path_buf(std_path_buf)   // from std::path::PathBuf — FALLIBLE
SystemPathBuf::from_path_buf_lossy(pb)       // same, but replaces bad bytes
SystemPath::absolute(path, cwd)              // make relative → absolute
```

`from_path_buf` returns `Result<SystemPathBuf, PathBuf>` — it hands your input
back if it was not UTF-8. That is the boundary check mentioned above, and it is
the *only* place you deal with it.

### Asking about one

| method | returns | use |
|---|---|---|
| `as_str()` | `&str` | ★ printing, comparing |
| `file_name()` | `Option<&str>` | ★ `"main.py"` |
| `file_stem()` | `Option<&str>` | `"main"` |
| `extension()` | `Option<&str>` | ★ `"py"` — is this a Python file? |
| `parent()` | `Option<&SystemPath>` | ★ the containing directory |
| `is_absolute()` | `bool` | |
| `starts_with(base)` | `bool` | ★★ **your project-code filter** |
| `ends_with(child)` | `bool` | |
| `components()` | iterator | walk the path piece by piece |
| `ancestors()` | iterator | ★ every parent, going up — used for config discovery |

### Building new ones

| method | returns |
|---|---|
| `join(other)` | `SystemPathBuf` — ★ `root.join("src").join("main.py")` |
| `with_extension("pyi")` | `SystemPathBuf` |
| `strip_prefix(base)` | `Result<&SystemPath, _>` — ★ make a path relative to the root |
| `to_path_buf()` | `SystemPathBuf` — clone a borrowed one |
| `push(other)` | mutates a `SystemPathBuf` in place |

### Escaping to std

```rust
path.as_std_path()          // -> &std::path::Path      (for std::fs, etc.)
buf.into_std_path_buf()     // -> std::path::PathBuf
buf.into_string()           // -> String
```

Useful when you need a crate that does not know about ty.

---

## Example 1 — make one from a command-line argument

```rust
use ruff_db::system::SystemPathBuf;

fn main() {
    // std::env::args() gives you the command-line arguments.
    // .nth(1) is the first one AFTER the program name. It is an Option,
    // because the user might not have passed anything.
    let arg: String = std::env::args()
        .nth(1)
        .expect("usage: pylspt <path>");

    let path = SystemPathBuf::from(arg);

    println!("path      = {}", path.as_str());
    println!("file_name = {:?}", path.file_name());
    println!("extension = {:?}", path.extension());
    println!("parent    = {:?}", path.parent().map(|p| p.as_str()));
}
```

Run it:

```
$ cargo run -- /Users/yared/Documents/Programing/rust/pylspt/experience/01-source-and-positions/python/ascii.py
path      = /Users/yared/.../python/ascii.py
file_name = Some("ascii.py")
extension = Some("py")
parent    = Some("/Users/yared/.../python")
```

**Rust notes on this snippet:**

- `{:?}` is the *debug* format. `Option<&str>` has no "display" form (what would
  `None` print as?), so you use `{:?}` and get `Some("ascii.py")` or `None`.
- `.map(|p| p.as_str())` — `parent()` gives `Option<&SystemPath>`, and
  `SystemPath` does not implement `Debug` in a way that prints nicely, so this
  converts the *inside* of the `Option` to a `&str` first. `map` on an `Option`
  means "if it is `Some`, apply this function to the value; if `None`, stay
  `None`."
- `.expect("…")` crashes with your message if the `Option` is `None`. Fine for a
  learning binary; you would use a real error later.

---

## Example 2 — joining and relativising

```rust
use ruff_db::system::SystemPathBuf;

fn main() {
    let root = SystemPathBuf::from("/Users/yared/proj");

    // join() builds a new path. It does NOT modify `root`.
    let main = root.join("src").join("app").join("main.py");
    println!("{}", main.as_str());
    // → /Users/yared/proj/src/app/main.py

    // strip_prefix goes the other way: make it relative to the root.
    match main.strip_prefix(&root) {
        Ok(rel) => println!("relative: {}", rel.as_str()),   // src/app/main.py
        Err(_)  => println!("not under root"),
    }

    // ancestors() walks upward. This is how config discovery works:
    // "look for pyproject.toml here, then in my parent, then its parent…"
    for ancestor in main.ancestors() {
        println!("  {}", ancestor.as_str());
    }
    // → /Users/yared/proj/src/app/main.py
    //   /Users/yared/proj/src/app
    //   /Users/yared/proj/src
    //   /Users/yared/proj
    //   /Users/yared
    //   /Users
    //   /
}
```

Notice `root.join(...)` did not consume `root` — you can still use it on the
next line. `join` takes `&self` and returns a brand-new `SystemPathBuf`.

---

## Example 3 — the one you will reuse for the rest of the port

```rust
use ruff_db::system::SystemPath;

/// Is `file` inside `project_root`?
///
/// This is the whole of your driver's `_is_project_code` check
/// (call_resolver.py:310), which decides what the call tree descends into.
fn is_project_code(file: &SystemPath, project_root: &SystemPath) -> bool {
    file.starts_with(project_root)
}
```

Three lines, and it is exercise 05's deliverable. Worth knowing it is this small
— `starts_with` compares **path components**, not string prefixes, so
`/proj2/x.py` does not match a root of `/proj`, which a naive
`str::starts_with` would get wrong.

---

## Exercise

Write a small binary that takes two arguments — a project root and a file path —
and prints:

1. the file's name, stem and extension
2. whether the file is a Python file (extension is `py` or `pyi`)
3. whether the file is inside the root
4. the file's path relative to the root, if it is inside
5. every ancestor directory of the file, one per line

Then run it on these, and check each answer against what you predicted:

```bash
cargo run -- /tmp/proj /tmp/proj/src/main.py       # inside
cargo run -- /tmp/proj /tmp/proj2/src/main.py      # NOT inside — does yours say so?
cargo run -- /tmp/proj /tmp/proj/src/main          # no extension
cargo run -- /tmp/proj relative/path.py            # relative input
```

Case 2 is the one that catches a string-prefix implementation. Case 4 is worth
thinking about: what *should* happen when the path is not absolute?

---

## Exam

**1.** Why does ty use `SystemPath` instead of `std::path::Path`? Give the
practical benefit at a call site.

**2.** What is the relationship between `SystemPath` and `SystemPathBuf`? Name
two other Rust type pairs with the same relationship.

**3.** You have a `SystemPathBuf` and a function that wants `&SystemPath`. What
do you write, and what Rust feature makes it work?

**4.** `SystemPathBuf::from_path_buf` returns a `Result`. What is in the error
case, and why does this function need to be fallible when
`SystemPathBuf::from("…")` does not?

**5.** Why is `path.starts_with(root)` correct for the project-code check when
`path.as_str().starts_with(root.as_str())` is not? Give an input where they
differ.

**6.** Which method would you use to implement "walk up from this file looking
for a `pyproject.toml`"?

**7.** `file_name()` returns `Option<&str>`. Give a path where it is `None`.

---

## Answers

**1.** Because ty requires paths to be text everywhere — hashed, interned,
compared, serialised to editors. `std::path::Path` may hold non-UTF-8 bytes, so
every text access would return an `Option`. ty converts once at the boundary and
never deals with it again. At a call site: `path.as_str()` gives you `&str`
directly instead of `Option<&str>`.

**2.** Borrowed view vs owned buffer. Same as `str`/`String`, `Path`/`PathBuf`,
`[T]`/`Vec<T>`. The borrowed one is always used behind a reference and points at
data someone else owns.

**3.** Just `&the_buf`. Deref coercion — `SystemPathBuf: Deref<Target = SystemPath>`
**[verified]** — so Rust inserts the conversion automatically. `.as_path()` does
the same thing explicitly.

**4.** The error case hands back the original `std::path::PathBuf`, because the
input might not be valid UTF-8 and the conversion cannot proceed. `from("…")`
takes a `&str` or `String`, which are UTF-8 *by definition* in Rust — so there is
nothing that can fail.

**5.** `starts_with` compares whole path components. With
`root = /proj` and `path = /proj2/src/main.py`, the string test says `true`
(because `"/proj2/…"` literally begins with `"/proj"`) and the path test says
`false`. The string version would pull an entirely unrelated project's files
into your call tree.

**6.** `ancestors()` — it yields the path, then its parent, then its parent, up
to the root. That is exactly what `ProjectMetadata::discover` does internally
(object 3).

**7.** A path ending in a directory traversal or the root itself: `"/"` has no
file name, and so does `".."`. Anything where the final component is not a name.
