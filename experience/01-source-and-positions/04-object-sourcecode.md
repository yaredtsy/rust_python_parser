# Object 4 — `SourceCode`

A small convenience wrapper that removes a nuisance. Short file.

---

## What it is

Every method in object 3 takes **both** the index and the text:

```rust
index.line_column(offset, source)
index.line_start(line, source)
index.line_range(line, source)
```

That gets tedious, and it lets you pass the wrong text with the right index —
which produces plausible, wrong answers rather than an error.

`SourceCode` bundles the two:

```rust
use ruff_source_file::SourceCode;

let code = SourceCode::new(text.as_str(), &index);

code.line_column(offset)          // no `source` argument
code.slice(node)                  // ★ a node's source text, directly
code.line_text(line)
```

**[verified]** from `ruff_source_file/src/lib.rs:30-89`.

---

## Why it earns its own file

Because of one method:

```rust
pub fn slice<T: Ranged>(&self, ranged: T) -> &'src str;
```

**[verified, `lib.rs:59`]**.

That is "give me the source text of this AST node", in one call, for any node.
Compare with doing it by hand:

```rust
&source[node.range().to_std_range()]        // panics on a bad boundary
```

`slice` is how you will get every **name** in your node tree — exercise 02's
`name` field comes from the source text of `call.func`, and there is no other way
to get it (the AST stores structure, not text).

**Rust note — `T: Ranged`.** Generic over anything with a range, so it works for
`&ExprCall`, `&StmtFunctionDef`, `TextRange` itself, or your own types. Write
`code.slice(call.func.as_ref())` and it just works.

---

## What you can do with it

**[verified]** from `ruff_source_file/src/lib.rs`.

| method | returns | notes |
|---|---|---|
| `SourceCode::new(content, index)` | `Self` | ★ construct |
| `.slice(ranged)` | `&str` | ★★ a node's source text |
| `.line_column(offset)` | `LineColumn` | ★ no text argument |
| `.source_location(offset, encoding)` | `SourceLocation` | |
| `.line_index(offset)` | `OneIndexed` | line only |
| `.line_start(line)` | `TextSize` | |
| `.line_end(line)` | `TextSize` | includes the terminator |
| `.line_end_exclusive(line)` | `TextSize` | ★ excludes the terminator |
| `.line_text(line)` | `&str` | ★ a whole line, as text |
| `.line_count()` | `usize` | |
| `.text()` | `&str` | the whole file |

Note `line_end` vs `line_end_exclusive` — one includes the `\n` (and, on a CRLF
file, the `\r\n`) and one does not. If you ever print a "line" and get a stray
blank, that is which one you picked.

**Rust note — two lifetimes.** The full type is `SourceCode<'src, 'index>`: it
borrows the text *and* the index, independently. You will almost never write the
type out — take it as a parameter (`code: SourceCode<'_, '_>`) or let inference
handle it. If you find yourself annotating both lifetimes by hand, you are
probably storing it in a struct, which you should not do (it borrows two things
that live in the database).

---

## Example 1 — a node's name, the easy way

```rust
use ruff_db::source::{line_index, source_text};
use ruff_source_file::SourceCode;

let text = source_text(&db, file);
let index = line_index(&db, file);
let code = SourceCode::new(text.as_str(), &index);

// given some AST node…
let name: &str = code.slice(some_node);
let position = code.line_column(some_node.range().start());

println!("{name} at line {}, column {}",
         position.line.get(),
         position.column.to_zero_indexed());
```

Three lines of setup, then every question is one call. This is the prologue you
will write at the top of every analysis function in exercise 02 onward — right
next to the `parsed_module` / `SemanticModel` prologue from exercise 00, object 6.

---

## Example 2 — a diagnostic printer worth keeping

```rust
use ruff_source_file::SourceCode;
use ruff_text_size::TextRange;

/// Print a range with its line, the way a compiler would.
fn show(code: &SourceCode<'_, '_>, range: TextRange, label: &str) {
    let start = code.line_column(range.start());
    let line_no = start.line;
    let line_text = code.line_text(line_no);
    let col = start.column.to_zero_indexed();
    let width = (range.end().to_u32() - range.start().to_u32()).max(1) as usize;

    println!("{label} at {}:{}", line_no.get(), col);
    println!("  {}", line_text.trim_end());
    println!("  {}{}", " ".repeat(col), "^".repeat(width));
}
```

```
call at 7:8
          greet("world")
          ^^^^^^^^^^^^^^
```

Write this now. When exercise 02's node tree comes out wrong, being able to
*see* which bytes a node covers turns a twenty-minute puzzle into a five-second
look.

⚠ One caveat, and it is the exercise-01 lesson again: `" ".repeat(col)` aligns
the caret correctly only when one character is one column wide. On
`unicode.py`'s emoji line the caret will be off, because `col` counts characters
and your terminal renders the emoji two cells wide. Not worth fixing — worth
*knowing*, because it is the same class of confusion as byte-vs-character
columns.

---

## Exercise

**A.** Build a `SourceCode` for `python/ascii.py` and print, for each line
1 through 7: the line number, `line_start`, `line_end`, `line_end_exclusive`,
and `line_text`. Note where `line_end` and `line_end_exclusive` differ, and by
how much.

**B.** Do the same for `python/crlf.py`. How far apart are `line_end` and
`line_end_exclusive` now? Explain the number.

**C.** Write `show` from example 2 and use it to display `TextRange::new(4, 9)`
and `TextRange::new(77, 99)` on `ascii.py`. Confirm the carets land under
`greet` and under `        greet("world")`.

**D.** Run `show` on `python/unicode.py` for the range covering `café` on line 1.
Is the caret in the right place in your terminal? Is `column` still correct?
Write down the difference between "the column is wrong" and "the terminal
renders it differently" — they are not the same problem, and only one of them is
yours.

**E.** Replace any hand-rolled `&source[range.to_std_range()]` in your code with
`code.slice(...)`. Then try to make it panic. Can you?

---

## Exam

**1.** What does `SourceCode` bundle, and what nuisance does that remove?

**2.** Besides convenience, what *bug* does bundling prevent?

**3.** What does `slice` do, why is it generic, and why can the AST not answer
the same question by itself?

**4.** What is the difference between `line_end` and `line_end_exclusive`? On a
CRLF file, by how much do they differ?

**5.** `SourceCode<'src, 'index>` has two lifetimes. What does each borrow, and
why should you not store one in a struct?

**6.** Your caret is misaligned under a line containing an emoji, but the
`column` you report is correct. Whose bug is it?

**7.** Which is safer — `code.slice(node)` or `&source[node.range().to_std_range()]`
— and does it matter for ranges that came from the parser?

---

## Answers

**1.** The file's text and its `LineIndex`. It removes having to pass `source`
to every position method, and it gives you `slice` for free.

**2.** **Passing the wrong text with the right index.** If you hold both
separately, nothing stops you from calling `index_for_a.line_column(offset,
text_of_b)`. That does not error — it returns a plausible, wrong position.
Bundling makes the pair impossible to mismatch after construction.

**3.** `slice(ranged)` returns the source text a node covers. It is generic over
`T: Ranged` so it works for any AST node, your own types, or a bare `TextRange`.

The AST cannot answer it because it stores **structure, not text** —
`ExprCall.func` is a node, and the characters `obj.render` exist only in the
source buffer. That is a feature: no strings are copied during parsing, which is
a large part of why ruff parses so fast.

**4.** `line_end` includes the line terminator, `line_end_exclusive` does not. On
an LF file they differ by 1; on a **CRLF** file by **2**, because the terminator
is `\r\n`.

**5.** `'src` borrows the text; `'index` borrows the `LineIndex`. Two independent
borrows, which is why there are two parameters.

Do not store one because both borrowed things live in the database — the text
comes from `source_text`, the index from `line_index` — so the struct would be
tied to one borrow of one revision. Same rule as `ProgramFile<'db>` in exercise
00, object 6: **build it where you use it, store owned keys instead.**

**6.** **Neither, in the code.** The column is a count of characters, which is
what the wire format specifies, and it is right. The misalignment is a
*rendering* fact: terminals give some characters two cells. Fixing it would mean
computing display width, which is a different question from column position, and
one no consumer of your JSON is asking.

Worth stating plainly because it is the same trap as byte-vs-character columns:
three different notions of "how far along the line is this" — bytes, characters,
display cells — and you must know which one your consumer means.

**7.** `code.slice(node)` is safer: it cannot panic on a character boundary, and
it cannot be handed the wrong text.

For ranges that came from the parser it does not matter — those always land on
boundaries. It matters for ranges **you** computed by arithmetic, which is
exactly where the panics come from. Use `slice` by habit so the question never
arises.
