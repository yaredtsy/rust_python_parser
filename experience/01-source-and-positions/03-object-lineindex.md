# Object 3 — `LineIndex`, `LineColumn`, `OneIndexed`, `PositionEncoding`

The bridge between ty's world (byte offsets) and your wire format's world
(1-based line, 0-based **character** column).

This is the most important object in exercise 01. Every node your driver ever
emits carries four numbers that come out of here.

---

## What it is

A `LineIndex` is a **sorted array of the byte offset where each line starts**.

```
ascii.py                                 line_starts = [0, 17, 41, 42, 43, 58, 77]
─────────────────────────────────────────────────────────────────────────────────
 0  def greet(name):                     ← line 1 starts at byte 0
17      return "hi " + name              ← line 2 starts at byte 17
41                                       ← line 3 starts at byte 41
42                                       ← line 4
43  class Greeter:                       ← line 5
58      def run(self):                   ← line 6
77          greet("world")               ← line 7
```

Given an offset, finding the line is a **binary search** in that array. The
column is then the distance from that line's start.

```
offset 20  →  binary search → line 2 (starts at 17)
           →  column = characters between byte 17 and byte 20 = 3
```

That is the whole data structure. Two ideas fall out of it:

1. **You build it once per file.** Building it is a scan of the whole file;
   using it is a binary search. Rebuilding per node is the classic quadratic
   mistake.
2. **The column needs the text, not just the index.** Counting *characters*
   between two byte offsets means looking at the bytes. That is why every
   column-producing method takes `content: &str` as well as the offset.

---

## Where it comes from

```rust
use ruff_source_file::{LineIndex, LineColumn, OneIndexed, PositionEncoding, SourceLocation};
use ruff_db::source::line_index;      // ★ the cached one
```

### Two ways to get one — and which to use

```rust
// ★ THE ONE YOU WANT: salsa-cached, per file, shared, auto-invalidated
let index = ruff_db::source::line_index(&db, file);

// only for text that is NOT in the database
let index = LineIndex::from_source_text(some_string);
```

`line_index(db, file)` is `#[salsa::tracked]` **[verified,
`ruff_db/src/source.rs:205`]** — the same query pattern as `source_text` from
object 2. Built once per file per revision, no matter how many nodes ask.

`from_source_text` has exactly one legitimate use in your driver: the
`parse_file(file_path, content)` RPC, where the client sent unsaved editor
content. That string has no `File`, so no query can be keyed on it.

---

## `OneIndexed` — the type that prevents an off-by-one

```rust
OneIndexed::from_zero_indexed(0).get()             // 1
OneIndexed::from_zero_indexed(0).to_zero_indexed() // 0
```

**[verified]**. It is a `usize` wrapper that makes you say which convention you
mean:

| call | returns | use for |
|---|---|---|
| `.get()` | 1-based `usize` | ★ **line** in your wire format |
| `.to_zero_indexed()` | 0-based `usize` | ★ **column** in your wire format |
| `OneIndexed::new(n)` | `Option<Self>` | from a 1-based number (`None` for 0) |
| `OneIndexed::from_zero_indexed(n)` | `Self` | from a 0-based number |

**Why this type exists.** Editors disagree: LSP is 0-based on both axes, parso is
1-based line / 0-based column, most compilers are 1-based on both. A bare
`usize` cannot tell you which you are holding. `OneIndexed` forces the
conversion to be written down, so a mistake is visible in the diff rather than
in a customer's editor.

Your wire format (from `plan/00-orientation/01`) is **1-based line, 0-based
column** — parso's convention. So:

```rust
line:       lc.line.get(),                   // 1-based ✓
column:     lc.column.to_zero_indexed(),     // 0-based ✓
```

That asymmetry — `.get()` on one field, `.to_zero_indexed()` on the other — is
the single most common off-by-one in this port. Write it once, in one function,
and never inline it anywhere else.

---

## `PositionEncoding` — the three ways to count a column

```rust
pub enum PositionEncoding { Utf8, Utf16, Utf32 }
```

For the line `    """Résumé 🎉 of the thing."""` from `python/unicode.py`:

| encoding | counts | length of that line |
|---|---|---|
| `Utf8` | **bytes** | 37 |
| `Utf16` | **UTF-16 code units** | 33 |
| `Utf32` | **characters** (code points) | **32** |

**[verified]** by measurement. Three different answers for the same line.

Why they differ: `é` is 2 bytes but 1 UTF-16 unit and 1 character. `🎉` is 4
bytes, **2** UTF-16 units (a surrogate pair), and 1 character. That emoji is the
one character in the fixture that separates all three at once, which is why it is
there.

Who wants which:

| consumer | encoding |
|---|---|
| **parso / your wire format** | `Utf32` — characters |
| LSP (default) | `Utf16` |
| a byte-oriented tool | `Utf8` |

---

## ★ The question the plan could not answer

`plan/02-mapping/01-parso-to-ruff-ast.md` says:

> ⚠ **Column semantics.** parso columns are *character* offsets within the line.
> Ruff's `LineIndex` can give you UTF-8 byte, UTF-16 code unit, or character
> columns depending on which method you call. […] **[check]** — this is a real
> parity bug generator.

Here is the answer, read out of the source:

```rust
// ruff_source_file/src/line_index.rs:117   [verified]
pub fn line_column(&self, offset: TextSize, content: &str) -> LineColumn {
    let location = self.source_location(offset, content, PositionEncoding::Utf32);
    …
}

// …and Utf32 is implemented as:            [verified, :210]
PositionEncoding::Utf32 => up_to_character.chars().count(),
```

**`line_column` counts characters.** That is exactly parso's convention, so the
plain `line_column` is the correct method and you never need to pass an encoding.

Do not take my word for it — step B of the exercise makes you prove it on
`unicode.py`.

> **Performance footnote.** Internally the index knows whether the file is
> pure ASCII, and if so returns the byte difference directly instead of decoding
> characters. So the common case costs nothing, and the correctness only costs
> you on files that actually contain non-ASCII. Good design — and another reason
> an all-ASCII test corpus tells you nothing about this code path.

---

## `line_column` vs `source_location` — the BOM

Two methods that look interchangeable and are not.

```rust
index.line_column(offset, content)                        // -> LineColumn
index.source_location(offset, content, encoding)          // -> SourceLocation
```

`LineColumn` has `.line` and `.column`. `SourceLocation` has `.line` and
`.character_offset`. Both `OneIndexed`.

The difference is the **byte-order mark**. `line_column` deliberately subtracts
it on line 1 **[verified, `:119-124`]**; `source_location` deliberately does
not, because it must round-trip:

| `bom.py`, offset 3 (the `d` of `def`) | result (0-based) |
|---|---|
| `line_column(3, src).column` | **0** |
| `source_location(3, src, Utf32).character_offset` | **1** |

**Which is right?** For you, `line_column` — CPython's tokenizer treats a leading
BOM as not part of the source, so parso reports column 0.

**Why does the other one exist?** Because `source_location` pairs with
`LineIndex::offset(...)` to convert *back* to a byte offset, and that only works
if the mapping is one-to-one. Stripping the BOM breaks that: bytes 0 and 3 would
both map to character 0, so you could not recover which you started from.

> `line_column` is the **display** function. `source_location` is the
> **coordinate** function. When you need a round trip, use the second.

---

## What you can do with it

**[verified]** from `ruff_source_file/src/line_index.rs`.

### Making one

| call | notes |
|---|---|
| `line_index(db, file)` | ★★ cached. use this |
| `LineIndex::from_source_text(text)` | only for text not in the db |

### Offset → position

| call | returns |
|---|---|
| `.line_column(offset, content)` | ★★ `LineColumn` — characters, BOM-aware |
| `.source_location(offset, content, encoding)` | `SourceLocation` — you pick the encoding |
| `.line_index(offset)` | `OneIndexed` — ★ line only, **no content needed** |

`.line_index(offset)` is worth remembering: if you only need the line number, it
is a pure binary search with no character counting at all.

### Position → offset (the reverse)

```rust
index.offset(
    SourceLocation { line: OneIndexed::from_zero_indexed(3),
                     character_offset: OneIndexed::from_zero_indexed(0) },
    &source,
    PositionEncoding::Utf32,
) -> TextSize
```

**[verified]**. You need this in exercise 09, where your CLI takes a
`line:column` argument and every ty entry point wants a `TextSize`.

### Line geometry

| call | returns |
|---|---|
| `.line_count()` | `usize` |
| `.line_start(line, content)` | `TextSize` |
| `.line_end(line, content)` | `TextSize` |
| `.line_range(line, content)` | `TextRange` — ★ the whole line as a range |
| `.line_len(line, content, encoding)` | `usize` |
| `.line_starts()` | `&[TextSize]` — the raw array |

---

## Example 1 — a complete program: offsets → positions

These examples read the file with `std::fs` and build the index with
`LineIndex::from_source_text`, so they run **standalone** — no database, no
exercise-00 helpers. The database version is two lines different and is shown at
the end of this file.

Put this in `src/bin/positions.rs` and run it with
`cargo run --bin positions -- <file> <start> <end>`.

**Rust note — you do not need to touch `Cargo.toml`.** Cargo auto-discovers every
`src/bin/*.rs` file as a binary named after the file. The explicit `[[bin]]`
section you added for `pylspt-dev` (exercise 00, file 09) does not disable that.
So each example below is: create the file, `cargo run --bin <name>`.

Run all the example programs from the **repo root**
(`/Users/yared/Documents/Programing/rust/pylspt`), since the paths below are
relative to it.

```rust
use ruff_source_file::LineIndex;
use ruff_text_size::{TextRange, TextSize};

/// Your wire format's position: 1-based line, 0-based column.
#[derive(Debug, PartialEq, Eq)]
pub struct Position {
    pub line: usize,
    pub column: usize,
    pub end_line: usize,
    pub end_column: usize,
}

pub fn to_position(index: &LineIndex, source: &str, range: TextRange) -> Position {
    let start = index.line_column(range.start(), source);
    let end = index.line_column(range.end(), source);

    Position {
        line: start.line.get(),                    // 1-based
        column: start.column.to_zero_indexed(),    // 0-based
        end_line: end.line.get(),
        end_column: end.column.to_zero_indexed(),
    }
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 4 {
        eprintln!("usage: {} <file> <start-byte> <end-byte>", args[0]);
        std::process::exit(2);
    }

    let source = std::fs::read_to_string(&args[1])?;
    let start: u32 = args[2].parse()?;
    let end: u32 = args[3].parse()?;

    let index = LineIndex::from_source_text(&source);
    let range = TextRange::new(TextSize::new(start), TextSize::new(end));

    let position = to_position(&index, &source, range);

    println!("file        {}", args[1]);
    println!("bytes       {}..{}   ({} bytes total)", start, end, source.len());
    println!("chars       {} total", source.chars().count());
    println!("slice       {:?}", &source[range.to_std_range()]);
    println!();
    println!("line        {}", position.line);
    println!("column      {}", position.column);
    println!("end_line    {}", position.end_line);
    println!("end_column  {}", position.end_column);

    Ok(())
}
```

Run it:

```
$ cargo run --bin positions -- experience/01-source-and-positions/python/ascii.py 4 9
file        experience/01-source-and-positions/python/ascii.py
bytes       4..9   (100 bytes total)
chars       100 total
slice       "greet"

line        1
column      4
end_line    1
end_column  9
```

**That `to_position` function is the deliverable of exercise 01.** Everything
else in this exercise exists to make you confident those four lines are right.
It belongs in `src/position.rs` (with `pub mod position;` in `lib.rs`); the
binary above is just a way to exercise it.

**Rust notes:**

- `let args: Vec<String> = std::env::args().collect();` — collecting first lets
  you check the count and index freely, which is nicer than three `.nth()` calls.
- `args[2].parse()?` — `parse` infers `u32` from the annotation on `start`. The
  `?` turns a bad number into an error rather than a panic.
- `&source[range.to_std_range()]` — ⚠ this **panics** if an offset is not on a
  character boundary. That is deliberate here: exercise B makes you trigger it.

---

## Example 2 — a complete program: check the known values

`src/bin/check_positions.rs`, run with `cargo run --bin check_positions`. No
arguments — it has the fixture path and the expected answers baked in, which is
the point.

```rust
use ruff_source_file::LineIndex;
use ruff_text_size::{TextRange, TextSize};

// …paste `Position` and `to_position` from example 1, or `use pylspt::position::*;`
// once you have moved them into the library.

fn check(source: &str, start: u32, end: u32, expected: (usize, usize, usize, usize)) {
    let index = LineIndex::from_source_text(source);
    let range = TextRange::new(TextSize::new(start), TextSize::new(end));
    let p = to_position(&index, source, range);
    let got = (p.line, p.column, p.end_line, p.end_column);

    let mark = if got == expected { "ok  " } else { "FAIL" };
    println!(
        "{mark} {start:>3}..{end:<3} {:?}  got {:?}  expected {:?}",
        &source[range.to_std_range()],
        got,
        expected,
    );
}

fn main() -> anyhow::Result<()> {
    let ascii = std::fs::read_to_string(
        "experience/01-source-and-positions/python/ascii.py",
    )?;

    println!("--- ascii.py (100 bytes, 100 chars) ---");
    // `greet` in `def greet(name):`
    check(&ascii, 4, 9, (1, 4, 1, 9));
    // the last statement, `        greet("world")`
    check(&ascii, 77, 99, (7, 0, 7, 22));
    // one byte further: offset 100 is AFTER the final newline
    check(&ascii, 77, 100, (7, 0, 8, 0));

    let unicode = std::fs::read_to_string(
        "experience/01-source-and-positions/python/unicode.py",
    )?;

    println!("\n--- unicode.py (88 bytes, 79 chars) ---");
    // the `(` on line 1: byte 9, character 8
    check(&unicode, 9, 9, (1, 8, 1, 8));
    // all of line 2: starts at byte 20, 37 bytes, 32 characters
    check(&unicode, 20, 57, (2, 0, 2, 32));

    Ok(())
}
```

```
--- ascii.py (100 bytes, 100 chars) ---
ok     4..9   "greet"  got (1, 4, 1, 9)  expected (1, 4, 1, 9)
ok    77..99  "        greet(\"world\")"  got (7, 0, 7, 22)  expected (7, 0, 7, 22)
ok    77..100 "        greet(\"world\")\n"  got (7, 0, 8, 0)  expected (7, 0, 8, 0)

--- unicode.py (88 bytes, 79 chars) ---
ok     9..9   ""  got (1, 8, 1, 8)  expected (1, 8, 1, 8)
ok    20..57  "    \"\"\"Résumé 🎉 of the thing.\"\"\""  got (2, 0, 2, 32)  expected (2, 0, 2, 32)
```

**Every line of that output is doing a job:**

| check | rejects |
|---|---|
| `4..9` → column **4** | `.get()` where `.to_zero_indexed()` belongs. A column of 0 would pass either way |
| `77..99` → `7:0..7:22` | nothing subtle — the baseline |
| `77..100` → ends `8:0` | shows what including the trailing newline does: the end jumps to the next line |
| `9..9` → column **8** | **any byte-based column implementation.** Byte 9, character 8 |
| `20..57` → end_column **32** | byte columns (would give 37) and UTF-16 columns (would give 33) |

The last two are the ones that matter. Everything above them passes on a
byte-based implementation.

---

## Example 3 — a complete program: all three encodings

`src/bin/encodings.rs`. This is the tool for exercise B, and it is what proves
the claim in the section above rather than taking my word for it.

```rust
use ruff_source_file::{LineIndex, PositionEncoding};
use ruff_text_size::TextSize;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        eprintln!("usage: {} <file> <byte-offset>", args[0]);
        std::process::exit(2);
    }

    let source = std::fs::read_to_string(&args[1])?;
    let offset = TextSize::new(args[2].parse()?);
    let index = LineIndex::from_source_text(&source);

    // The display function: characters, and BOM-aware.
    let lc = index.line_column(offset, &source);

    println!("offset {}  (file: {} bytes, {} chars)",
             args[2], source.len(), source.chars().count());
    println!();
    println!("line_column()          line {}  column {}   ← what YOUR wire format uses",
             lc.line.get(), lc.column.to_zero_indexed());
    println!();
    println!("source_location(), 0-based character_offset:");

    for (name, encoding) in [
        ("Utf8  (bytes)     ", PositionEncoding::Utf8),
        ("Utf16 (code units)", PositionEncoding::Utf16),
        ("Utf32 (characters)", PositionEncoding::Utf32),
    ] {
        let loc = index.source_location(offset, &source, encoding);
        println!("  {name}  line {}  character_offset {}",
                 loc.line.get(), loc.character_offset.to_zero_indexed());
    }

    // Round trip: position → offset → position, using the coordinate function.
    let loc = index.source_location(offset, &source, PositionEncoding::Utf32);
    let back = index.offset(loc, &source, PositionEncoding::Utf32);
    println!();
    println!("round trip  {} → source_location → offset → {}",
             args[2], back.to_u32());

    Ok(())
}
```

Run it on the `(` of `café(` — byte 9 of `unicode.py`:

```
$ cargo run --bin encodings -- experience/01-source-and-positions/python/unicode.py 9
offset 9  (file: 88 bytes, 79 chars)

line_column()          line 1  column 8   ← what YOUR wire format uses

source_location(), 0-based character_offset:
  Utf8  (bytes)       line 1  character_offset 9
  Utf16 (code units)  line 1  character_offset 8
  Utf32 (characters)  line 1  character_offset 8

round trip  9 → source_location → offset → 9
```

**There is the proof.** `line_column` gave 8, which matches `Utf32`, which is
characters — parso's convention. A byte-based implementation would say 9.

Now run it on **byte 3 of `bom.py`**, and on an offset on line 2 of
`unicode.py` (after the emoji), where all three encodings disagree.

---

## Using the database instead

Every program above reads the file itself. In the real driver the file is in the
database, and the change is two lines:

```rust
// standalone (above)
let source = std::fs::read_to_string(&path)?;
let index = LineIndex::from_source_text(&source);

// in the driver
let text  = ruff_db::source::source_text(&db, file);      // #[salsa::tracked]
let index = ruff_db::source::line_index(&db, file);       // #[salsa::tracked]
let source = text.as_str();
```

Everything after that is identical — `to_position(&index, source, range)` does
not care where the index came from.

**Use the database version in `src/`**, because the index is then built once per
file per revision and shared. The standalone version is for these binaries, for
tests with literal source, and for the `parse_file(content)` RPC where the
editor sent text that has no `File`.

---

## Exercise

**A.** Write `to_position` into `src/position.rs`, plus a binary that takes a
file and two offsets and prints the `Position`. Verify both assertions from
example 2 on `python/ascii.py`.

**B. Prove the encoding claim.** On `python/unicode.py`:

- line 1 is `def café(número):`. The `(` is at **byte 9** and **character 8**.
  Print `line_column`'s column for offset 9. Which number did you get?
- line 2 spans bytes 20..57 and is 32 characters / 37 bytes / 33 UTF-16 units.
  Print `to_position` for `TextRange::new(20.into(), 57.into())`. What is
  `end_column`?

Then print all three encodings for the same offset using `source_location`, and
confirm you can produce 32, 33 and 37 at will.

**C. The BOM.** On `python/bom.py`, print both:

```
line_column(3, src).column.to_zero_indexed()
source_location(3, src, Utf32).character_offset.to_zero_indexed()
```

Write down the two values and which one parso would agree with.

**D. Tabs and CRLF — predict first.**

- `python/tabs.py` line 3 is `\t\treturn tabbed(x - 1)`. What column does
  `return` start at — 2, 8 or 16?
- `python/crlf.py` line 1 is `def crlf_fn(x):\r\n`. What is `end_column` for
  `TextRange::new(0.into(), 15.into())`? What about `0..16`, and why would a node
  range never be `0..16`?

**E. The performance point.** Convert the same 200 offsets twice: once building
`LineIndex::from_source_text` **inside** the loop, once **outside**. Time both
with `std::time::Instant`. Write down the ratio and the file size.

Then answer: in the real pipeline, what makes this cost almost disappear?

**F. The reverse direction.** Write `offset_of(index, source, line, column) ->
TextSize` using `LineIndex::offset`, and check it round-trips: convert an offset
to a position and back, and assert you get the same offset. Try it on `ascii.py`
and then on `unicode.py` — does it still round-trip? What about on `bom.py`
using `line_column`'s output?

---

## Exam

**1.** What data structure is a `LineIndex`? What is the cost of building one,
and of using one?

**2.** Why do the column-producing methods need `content: &str` and not just the
offset?

**3.** Which of the two ways to get a `LineIndex` should you use, and what is the
one legitimate use of the other?

**4.** Why does `OneIndexed` exist instead of a plain `usize`?

**5.** Write the four expressions for your wire format's `line`, `column`,
`end_line`, `end_column`. Why is the asymmetry between them the most likely bug
in this exercise?

**6.** Give the three `PositionEncoding` variants, the length of
`    """Résumé 🎉 of the thing."""` in each, and which one your driver needs.

**7.** Which encoding does `line_column` use? Quote where you would look to
confirm it.

**8.** `line_column` and `source_location` disagree on `bom.py`. Give both
values, explain why, and say which one is correct for you.

**9.** Why does `source_location` *not* strip the BOM? What would break if it
did?

**10.** Which method gives you the line number without needing the file's text,
and why is that possible?

**11.** An assertion with an expected column of 0 cannot catch the
`.get()`/`.to_zero_indexed()` mistake. Why not? Write one that can.

**12.** Why can a test corpus of 200 real ASCII files not fail your column
implementation?

---

## Answers

**1.** A sorted array of the byte offset at which each line starts. Building it
is O(file) — one scan looking for newlines. Using it is O(log lines) — a binary
search — plus a short scan of the current line to count characters.

**2.** Because a column is a count of **characters**, and counting characters
between two byte offsets requires looking at the bytes to see how wide each
character is. The index alone knows only where lines begin.

**3.** Use `ruff_db::source::line_index(db, file)` — salsa-cached, built once per
file per revision, shared with everything else that needs it. Use
`LineIndex::from_source_text` only for text that has no `File`: the
`parse_file(file_path, content)` RPC where the editor sent unsaved content.

**4.** Because a bare `usize` cannot say which convention it holds, and the
conventions genuinely differ between consumers (LSP is 0-based on both axes,
parso is 1-based line / 0-based column). `OneIndexed` forces you to write the
conversion down, so a mistake appears in the code rather than in someone's
editor.

**5.**

```rust
line:       lc.line.get(),
column:     lc.column.to_zero_indexed(),
end_line:   end.line.get(),
end_column: end.column.to_zero_indexed(),
```

The asymmetry — `.get()` on lines, `.to_zero_indexed()` on columns — is the bug
source because both fields have the same type, so the compiler cannot help. Four
fields, two conventions, no type safety. Hence: write it exactly once and call
that function everywhere.

**6.** `Utf8` = 37 bytes, `Utf16` = 33 code units, `Utf32` = 32 characters.
Your driver needs **`Utf32`** (characters), because that is parso's convention.

**7.** `Utf32`. `ruff_source_file/src/line_index.rs:117` — `line_column` calls
`source_location(offset, content, PositionEncoding::Utf32)`, and `:210`
implements `Utf32` as `chars().count()`.

**8.** `line_column` gives **0**; `source_location` gives **1**. They differ
because `line_column` explicitly subtracts the BOM when the offset is on line 1
and `source_location` explicitly does not. **0** is correct for you — CPython
strips a leading BOM, so parso reports column 0.

**9.** Because it pairs with `LineIndex::offset` for round-tripping, and that
requires an injective mapping. If the BOM were stripped, byte 0 and byte 3 would
both map to character offset 0, so converting back could not recover which one
you meant.

**10.** `.line_index(offset)` — it is a pure binary search in the `line_starts`
array. No character counting is involved in finding *which* line an offset is
on; that only matters for the column.

**11.** Because `.get()` and `.to_zero_indexed()` differ by exactly one, and at
column 0 the wrong one produces 1 — which you would only notice if the expected
value were something other than 0 in *some* test. An assertion that can catch it:

```rust
// ascii.py, offset 4..9 = `greet` on line 1
let p = to_position(&index, source, TextRange::new(4.into(), 9.into()));
assert_eq!((p.line, p.column), (1, 4));
```

The `column == 4` does the work. Pick assertions by asking which wrong
implementation they reject.

**12.** Because in an all-ASCII file, byte offsets, character offsets and UTF-16
offsets are **identical** — one byte per character, always. A fully byte-based
implementation passes every one of those 200 files. The index even has an
internal ASCII fast path that skips character decoding entirely, so you are not
even exercising the code that could be wrong.

This is why `unicode.py`, `bom.py`, `crlf.py` and `tabs.py` have to be added
deliberately. The M2 gate in `plan/04-build/02-milestones.md` asks for 200 real
files; 200 real Western-codebase files are mostly ASCII, and would pass a broken
implementation.
