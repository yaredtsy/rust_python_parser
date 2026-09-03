# Object 2 — `source_text` and `SourceText`

Getting the actual characters. A short file — this object is simple, but it is
the first **cached query** you will call, and that idea is worth meeting on
something easy.

---

## What it is

```rust
ruff_db::source::source_text(db, file) -> SourceText
```

A **function**, not a method — you pass the database and a `File` (exercise 00,
object 5) and get back the file's contents.

`SourceText` is a cheap handle around the text. `.as_str()` gives you `&str`.

---

## Why it is a function and not `file.text()`

Because it is a **query**:

```rust
#[salsa::tracked(returns(clone), heap_size = …)]
pub fn source_text(db: &dyn Db, file: File) -> SourceText { … }
```

**[verified, `ruff_db/src/source.rs:15`]**.

That attribute means: **the result is memoised, and salsa records that this
query read that file.** Call it a thousand times and the file is read from disk
once. Change the file, sync the database, and only the queries that transitively
read it recompute.

If the text were a field on `File`, none of that would be possible — the handle
would have to hold data, and salsa would have nothing to invalidate.

**The pattern to internalise:** in ty, "data about a file" is a *function of*
`(db, file)`, not a property of `file`. You will see the same shape for
`line_index` (object 3), `parsed_module` (exercise 02) and `semantic_index`
(exercise 06). Once you expect it, the API stops looking scattered.

---

## Where it comes from

```rust
use ruff_db::source::{source_text, SourceText};
```

---

## What you can do with it

**[verified]** from `ruff_db/src/source.rs`.

| method | returns | notes |
|---|---|---|
| `.as_str()` | `&str` | ★ the text. what you want 95% of the time |
| `.read_error()` | `Option<&SourceTextError>` | ★ did the read fail |
| `.is_notebook()` | `bool` | `.ipynb` files |
| `.as_notebook()` | `Option<&Notebook>` | the parsed notebook, if it is one |
| `.to_bytes()` | `Cow<[u8]>` | raw bytes |
| `.with_text(new, map)` | `SourceText` | a modified copy — used by fix application |

### The part people miss: `read_error`

`source_text` does **not** return a `Result`. If the file cannot be read —
deleted between your `system_path_to_file` and now, permission denied, invalid
UTF-8 — you get a `SourceText` whose `as_str()` is empty and whose
`read_error()` is `Some`.

That is a deliberate design choice and it is the right one for a language
server: a query that returns `Result` poisons every caller with error handling
for a case that should degrade gracefully. But it means:

> **An empty file and an unreadable file look identical unless you check
> `read_error()`.**

For your driver this maps onto a real bug class. "That file produced no nodes"
has two very different causes — it was empty, or it could not be read — and only
one of them is worth telling the user about. Log `read_error()` at debug level
and the question answers itself.

---

## Example 1 — read a file's text

```rust
use pylspt::db::{open, open_project};        // your helpers from exercise 00
use ruff_db::source::source_text;
use ruff_db::system::SystemPath;

fn main() -> anyhow::Result<()> {
    let dir = std::env::args().nth(1).expect("usage: prog <dir> <file>");
    let file_arg = std::env::args().nth(2).expect("usage: prog <dir> <file>");

    let db = open_project(SystemPath::new(&dir))?;
    let (file, _program_file) = open(&db, SystemPath::new(&file_arg))?;

    let text = source_text(&db, file);

    if let Some(err) = text.read_error() {
        eprintln!("could not read: {err}");
        return Ok(());
    }

    let s = text.as_str();
    println!("bytes      = {}", s.len());
    println!("chars      = {}", s.chars().count());
    println!("lines      = {}", s.lines().count());
    println!("first line = {:?}", s.lines().next());

    Ok(())
}
```

Run it on the exercise-01 fixtures and record the numbers:

| file | bytes | chars |
|---|---|---|
| `ascii.py` | 100 | 100 |
| `unicode.py` | **88** | **79** |
| `bom.py` | 30 | 28 |
| `tabs.py` | 55 | 55 |

`unicode.py` is the interesting row and the reason the fixture exists: **bytes ≠
characters**. Every offset in ty is a byte offset; every column in your wire
format is a character count. Object 3 is where those two facts meet.

`bom.py` is 30 bytes and 28 characters: the 3-byte BOM counts as **one**
character.

**Rust note.** `s.len()` on a `&str` is **bytes**, always — not characters. This
surprises people coming from Python, where `len("café")` is 4. In Rust it is 5.
`s.chars().count()` is the Python answer, and it costs a full scan, which is why
nobody in ty calls it on a hot path.

---

## Example 2 — prove the caching

```rust
use std::time::Instant;

let t0 = Instant::now();
let a = source_text(&db, file);
let first = t0.elapsed();

let t1 = Instant::now();
let b = source_text(&db, file);
let second = t1.elapsed();

println!("first  = {first:?}");
println!("second = {second:?}");
println!("same text: {}", a.as_str() == b.as_str());
```

The second call does not touch the disk. Predict the ratio before you run it —
then check whether you were within an order of magnitude.

⚠ **Do not draw conclusions from a debug build.** With exercise 00's profile
(`opt-level = 1` for you, `3` for dependencies) the numbers mean something. At
`-O0` they do not. Exercise 03 does this properly with the parser, where the work
is big enough to be interesting.

---

## Exercise

**A.** Get example 1 running against all five fixtures in
`experience/01-source-and-positions/python/`. Fill in the bytes/chars/lines
table. Note which files have bytes ≠ chars and why.

**B.** Delete a file after opening it but before calling `source_text`, and
confirm you get a `read_error` rather than a crash. (Easiest version: open a
path, then `std::fs::remove_file`, then query. Use a copy in your scratch
directory, not a fixture.)

**C.** Add a `--stats` style line to your binary printing bytes, chars, lines and
whether a read error occurred. You will grow this into exercise 11's `--stats`.

**D.** For `crlf.py`, print `s.lines().count()` and also
`s.matches('\n').count()`. Are they the same? Then print
`s.lines().next().unwrap().len()` and think about whether the `\r` is in there.
Write down what you conclude — object 3 will ask you about it.

---

## Exam

**1.** Why is `source_text` a function taking `(db, file)` rather than a method
on `File`?

**2.** What does `#[salsa::tracked]` change about calling it twice?

**3.** State the general pattern this object is an instance of. Name two other
queries with the same shape.

**4.** `source_text` does not return a `Result`. What happens when the file
cannot be read, and what must you call to find out?

**5.** Why is not-returning-`Result` the right choice here? What would it cost
every caller if it did?

**6.** "That file produced no nodes." Give the two causes this object
distinguishes, and how you would tell them apart in a log.

**7.** For `unicode.py`, `s.len()` is 88 and `s.chars().count()` is 79. Which one
is a `TextSize` measured in, and which one does your wire format's `column`
need?

**8.** Why does nobody in ty call `.chars().count()` on a hot path?

**9.** `bom.py` is 30 bytes and 28 characters. How many characters is the BOM,
and how many bytes?

---

## Answers

**1.** Because the text is not part of the file's **identity** — it is data
*about* the file, which changes over time while the identity does not. Making it
a query lets salsa memoise the result and record the dependency, so an edit
invalidates exactly the work that read it. A field on `File` could do neither.

**2.** The second call returns the memoised result without touching the disk —
a hash lookup against work already done. Salsa also recorded that the query read
this file, so it knows to recompute when the file's revision changes.

**3.** **Data about a file is a function of `(db, file)`, not a property of the
file.** Same shape: `line_index(db, file)` (object 3), `parsed_module(db,
python_file)` (exercise 02), `semantic_index(db, program_file)` (exercise 06).

**4.** You get a `SourceText` whose `as_str()` is empty and whose `read_error()`
is `Some(...)`. Nothing panics and nothing propagates — you must ask.

**5.** Because a `Result` would force every caller — including deep inside
inference, where there is nothing sensible to do about it — to handle a case that
should simply degrade to "no content". A language server must keep working when
one file of five thousand is unreadable. The cost of `Result` here is error
handling at hundreds of call sites for a case none of them can fix.

**6.** The file was **empty**, or the file could not be **read**. Both give an
empty `as_str()`. Tell them apart by logging `read_error()` — if it is `Some`,
say so; if `None`, the file really is empty. Without that log the two are
indistinguishable, and one of them is a real problem.

**7.** `TextSize` is **bytes** (88). Your wire format's `column` needs
**characters** (79), because that is parso's convention and v-noc depends on it.
Bridging the two is object 3's entire job.

**8.** Because it is O(n) in the length of the string — it decodes every
character to count them. A position lookup that did this per node would be
quadratic in file size. `LineIndex` exists precisely to make the conversion a
binary search plus a short scan instead.

**9.** The BOM is **1 character** and **3 bytes** (`EF BB BF`). Hence 30 bytes /
28 characters for a file whose visible content is 27 characters plus one
invisible one.
