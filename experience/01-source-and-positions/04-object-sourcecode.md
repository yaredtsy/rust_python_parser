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
| `.line_end_exclusive(line)` | `TextSize` | ★ excludes the terminator — **except on the last line**, see example 1 |
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

## Example 1 — a complete program: the line table

`src/bin/lines.rs`, run with `cargo run --bin lines -- <file>`. It prints one
row per line and makes `line_end` vs `line_end_exclusive` visible, which is the
distinction that catches people.

```rust
use ruff_source_file::{LineIndex, OneIndexed, SourceCode};

fn main() -> anyhow::Result<()> {
    let path = std::env::args().nth(1).expect("usage: lines <file>");
    let source = std::fs::read_to_string(&path)?;

    let index = LineIndex::from_source_text(&source);
    let code = SourceCode::new(&source, &index);

    println!("{}  —  {} bytes, {} chars, {} lines",
             path, source.len(), source.chars().count(), code.line_count());
    println!();
    println!("{:>4}  {:>6} {:>6} {:>6}   {}",
             "line", "start", "end", "end_ex", "text");

    for n in 1..=code.line_count() {
        let line = OneIndexed::new(n).expect("line numbers start at 1");
        println!(
            "{:>4}  {:>6} {:>6} {:>6}   {:?}",
            n,
            code.line_start(line).to_u32(),
            code.line_end(line).to_u32(),
            code.line_end_exclusive(line).to_u32(),
            code.line_text(line),
        );
    }

    Ok(())
}
```

```
$ cargo run --bin lines -- experience/01-source-and-positions/python/ascii.py
experience/.../ascii.py  —  100 bytes, 100 chars, 7 lines

line   start    end end_ex   text
   1       0     17     16   "def greet(name):\n"
   2      17     41     40   "    return \"hi \" + name\n"
   3      41     42     41   "\n"
   4      42     43     42   "\n"
   5      43     58     57   "class Greeter:\n"
   6      58     77     76   "    def run(self):\n"
   7      77    100    100   "        greet(\"world\")\n"
```

**Read the `end` and `end_ex` columns.** They differ by **1** on lines 1–6 — the
`\n`. Now run it on `python/crlf.py` and watch the gap become **2**, because the
terminator is `\r\n`.

⚠ **Line 7 is different: both say 100.** The last line has no line *after* it, so
there is no "next line start" to subtract a terminator from, and both methods
fall back to the file's total length **[verified,
`line_index.rs:275, 288`]** — even though the file does end with a `\n`.

So `line_end_exclusive` means "excluding the terminator **if we can tell where
the next line starts**". On the final line you get the terminator back. That is
the kind of edge case that produces one wrong node in a thousand, and you would
never guess it from the method name.

Note also `line_text` **includes** the terminator (it uses `line_range`, which is
`line_start..line_end` **[verified]**) — which is why the printer in example 2
calls `.trim_end()`.

> `LineIndex::line_end_exclusive` is `pub(crate)` **[verified]** — only the
> `SourceCode` wrapper exposes it publicly. One more small reason to hold a
> `SourceCode` rather than a bare `LineIndex`.

**Rust notes:**

- `OneIndexed::new(n)` returns `Option` because 0 is not a valid 1-based line
  number. `.expect(...)` is fine here since the loop starts at 1.
- `{:>4}` right-aligns in a 4-wide field; `{:?}` on a `&str` shows the escapes,
  which is exactly what you want when the question is "where does this line
  actually end".

---

## Example 2 — a complete program: the caret printer

`src/bin/show.rs`, run with `cargo run --bin show -- <file> <start> <end>`.
This is the tool you will reach for constantly in exercise 02.

```rust
use ruff_source_file::{LineIndex, SourceCode};
use ruff_text_size::{TextRange, TextSize};

/// Print a range with its line underlined, the way a compiler would.
fn show(code: &SourceCode<'_, '_>, range: TextRange, label: &str) {
    let start = code.line_column(range.start());
    let end = code.line_column(range.end());
    let line_no = start.line;
    let col = start.column.to_zero_indexed();

    // Width in CHARACTERS on this line, not bytes — and clamp to the line
    // if the range spans several lines.
    let width = if end.line == start.line {
        end.column.to_zero_indexed().saturating_sub(col).max(1)
    } else {
        code.line_text(line_no).trim_end().chars().count().saturating_sub(col).max(1)
    };

    println!("{label} at {}:{}", line_no.get(), col);
    println!("     |");
    println!("{:>4} | {}", line_no.get(), code.line_text(line_no).trim_end());
    println!("     | {}{}", " ".repeat(col), "^".repeat(width));
    if end.line != start.line {
        println!("     | … continues to line {}, column {}",
                 end.line.get(), end.column.to_zero_indexed());
    }
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 4 {
        eprintln!("usage: {} <file> <start-byte> <end-byte>", args[0]);
        std::process::exit(2);
    }

    let source = std::fs::read_to_string(&args[1])?;
    let index = LineIndex::from_source_text(&source);
    let code = SourceCode::new(&source, &index);

    let range = TextRange::new(
        TextSize::new(args[2].parse()?),
        TextSize::new(args[3].parse()?),
    );

    // `slice` takes anything Ranged — including a bare TextRange. [verified]
    println!("slice: {:?}", code.slice(range));
    println!();
    show(&code, range, "range");

    Ok(())
}
```

```
$ cargo run --bin show -- experience/01-source-and-positions/python/ascii.py 77 99
slice: "        greet(\"world\")"

range at 7:0
     |
   7 |         greet("world")
     | ^^^^^^^^^^^^^^^^^^^^^^

$ cargo run --bin show -- experience/01-source-and-positions/python/ascii.py 85 90
slice: "greet"

range at 7:8
     |
   7 |         greet("world")
     |         ^^^^^
```

**Write this now.** When exercise 02's node tree comes out wrong, being able to
*see* which bytes a node covers turns a twenty-minute puzzle into a five-second
look. Every compiler you have ever used prints this because it works.

⚠ Now run it on the emoji line. `python/unicode.py` line 2 starts at byte 20 and
the emoji occupies bytes 36–39. Point at the word `of`, which sits **after** it —
bytes 41..43, character column 16:

```
$ cargo run --bin show -- experience/01-source-and-positions/python/unicode.py 41 43
slice: "of"

range at 2:16
     |
   2 |     """Résumé 🎉 of the thing."""
     |                 ^^
```

The caret lands **one cell to the left** of `of`. Column 16 is correct — there
really are 16 characters before it — but your terminal draws 🎉 two cells wide,
so 16 characters occupy 17 columns on screen.

Try `27 33` (`Résum`, entirely *before* the emoji) and the caret is perfect. The
misalignment starts exactly at the emoji, which is how you know what caused it.

The reported `column` is right in both cases. This is a *rendering* difference,
not a position bug.

Same family as byte-vs-character columns, and worth meeting deliberately: there
are **three** notions of "how far along this line", and you now know all three.

| notion | used by |
|---|---|
| bytes | `TextSize`, `TextRange` |
| characters | your wire format, parso, `line_column` |
| display cells | terminals, editors drawing a caret |

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

**Except on the last line, where they are equal.** Both fall back to the file's
total length when there is no following line to subtract a terminator from
**[verified, `line_index.rs:275, 288`]** — even if the file ends with a newline.
So `line_end_exclusive` really means "excluding the terminator, if we can see
where the next line starts".

Also worth knowing: `LineIndex::line_end_exclusive` is `pub(crate)`; only
`SourceCode` exposes it **[verified]**.

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
