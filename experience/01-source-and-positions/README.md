# 01 — Source and positions

**Goal:** you can take any AST node and produce the exact `position` object your
wire format needs, and you can defend every number in it.

This is the smallest exercise with the highest bug density in the whole port.
Every node you ever emit carries four numbers from here. Get them wrong and the
error shows up months later as "v-noc highlights the wrong line", with nothing
in the logs.

---

## Read first

- `tutorial/04-positions-and-text.md` — the whole chapter, it is short
- `plan/02-mapping/01-parso-to-ruff-ast.md` §"Positions: the `LineIndex` bridge"

---

## The mental model

Jedi/parso hands you `(line, column)` on every node. Ruff hands you **two
integers**: byte offsets into the file, start and end.

```
source:   d  e  f     g  r  e  e  t  (  n  a  m  e  )  :  \n     …
offset:   0  1  2  3  4  5  6  7  8  9 10 11 12 13 14 15 16     …
                     └────────────────────────────────┘
                     TextRange { start: 4, end: 9 }   ← the name `greet`
```

`TextSize` is a `u32`. `TextRange` is two of them — **eight bytes, `Copy`,
comparable, hashable**. That is the entire position representation in ty.

Three consequences, and they are the point:

1. **Nothing needs to know where the newlines are.** parso cannot produce a
   position without having split the file into lines; ruff can, because the
   offset *is* the position. Line/column is a rendering concern, computed at the
   edge, once.
2. **Ranges are cheap to compare.** "Is this node inside that node?" is two
   integer comparisons. "Have I seen this position before?" is an
   `FxHashSet<TextRange>` — the position-dedup quirk from your `parser.py`
   (`plan/00-orientation/01`, quirk 10) becomes one hash lookup instead of a
   4-tuple comparison.
3. **Line/column costs a lookup.** You need a `LineIndex`: a sorted array of the
   byte offset where each line starts. Binary search gives the line; the column
   is the distance from that line's start. This is why you build it **once per
   file**, never per node.

```
LineIndex for ascii.py     line_starts = [0, 17, 41, 42, 43, 58, 77]
                                          ▲   ▲
offset 20  →  binary search finds line 2 ──┘   └── line 2 starts at byte 17
              column = characters between byte 17 and byte 20 = 3
```

### Your wire format

From `plan/00-orientation/01`: **line is 1-based, column is 0-based** — parso's
convention, and v-noc downstream depends on it.

```jsonc
"position": { "line": 1, "column": 0, "end_line": 3, "end_column": 12 }
```

Ruff's `LineColumn` gives you **both fields `OneIndexed`**. So `line` passes
through and `column` needs converting. That asymmetry is the single most common
off-by-one in this port.

---

## The API, verified at `ac201b8`

```rust
// ruff_text_size
pub struct TextSize(u32);
pub struct TextRange { /* start, end */ }
impl TextRange {
    pub fn start(self) -> TextSize;
    pub fn end(self) -> TextSize;      // EXCLUSIVE
    pub fn len(self) -> TextSize;
    pub fn contains_range(self, other: TextRange) -> bool;
    pub fn empty(offset: TextSize) -> TextRange;
}
pub trait Ranged { fn range(&self) -> TextRange; }   // ← import it or `.range()` won't resolve

// ruff_db::source  — BOTH are #[salsa::tracked]
pub fn source_text(db: &dyn Db, file: File) -> SourceText;   // .as_str()
pub fn line_index(db: &dyn Db, file: File) -> LineIndex;     // ★ cached per file

// ruff_source_file
impl LineIndex {
    pub fn from_source_text(text: &str) -> Self;
    pub fn line_column(&self, offset: TextSize, content: &str) -> LineColumn;
    pub fn source_location(&self, offset: TextSize, text: &str, encoding: PositionEncoding) -> SourceLocation;
    pub fn line_index(&self, offset: TextSize) -> OneIndexed;      // line only, no content needed
    pub fn line_start(&self, line: OneIndexed, contents: &str) -> TextSize;
    pub fn line_count(&self) -> usize;
}
pub struct LineColumn { pub line: OneIndexed, pub column: OneIndexed }
impl OneIndexed {
    pub const fn get(self) -> usize;              // 1-based
    pub const fn to_zero_indexed(self) -> usize;  // 0-based  ← you want this for column
}
pub enum PositionEncoding { Utf8, Utf16, Utf32 }

// ruff_source_file::SourceCode — a convenience wrapper over (text, index)
impl SourceCode<'_, '_> {
    pub fn new(content: &str, index: &LineIndex) -> Self;
    pub fn line_column(&self, offset: TextSize) -> LineColumn;   // no need to pass content
    pub fn slice<T: Ranged>(&self, ranged: T) -> &str;           // ★ node → its source text
    pub fn line_text(&self, index: OneIndexed) -> &str;
}
```

> **Use `ruff_db::source::line_index(db, file)`, not
> `LineIndex::from_source_text`.** The former is salsa-tracked: built once,
> shared with everything else in the process that needs it, and invalidated
> automatically when the file changes. The latter rebuilds it every call. You
> will only use `from_source_text` for content that is not in the database — the
> `parse_file(content=...)` RPC case, where the client sent you unsaved text.

### The `[check]` in the plan, now resolved

`plan/02-mapping/01` warns that character vs UTF-8 vs UTF-16 columns differ and
tells you to check which method matches parso. Here is the answer, read out of
the source at `ac201b8`:

```rust
// ruff_source_file/src/line_index.rs:117  [verified]
pub fn line_column(&self, offset: TextSize, content: &str) -> LineColumn {
    let location = self.source_location(offset, content, PositionEncoding::Utf32);
    …
}
// and Utf32 is implemented as:  up_to_character.chars().count()   [verified, :210]
```

**`line_column` counts characters (Unicode scalar values).** That is exactly
parso's convention — Python string indices are code points. So the plain
`line_column` is the correct method, and you do *not* need `source_location`
with an explicit encoding.

Do not take my word for it. Fixture `python/unicode.py` exists so you can prove
it, and step 3 makes you do that.

---

## The fixtures

```
python/
├── ascii.py ....... plain. 100 bytes, 100 characters. your baseline.
├── unicode.py ..... 88 bytes, 79 characters. accented identifiers, an emoji.
├── tabs.py ........ tab-indented. does a tab count as 1 column or 4?
├── crlf.py ........ CRLF line endings. where does a line end?
└── bom.py ......... starts with a UTF-8 BOM. 3 invisible bytes.
```

Each one exists to break a different assumption. `ascii.py` is the only file
where byte offset, character offset, and column all agree — which is why a test
suite made only of ASCII files proves nothing about positions.

---

## Build it

### Step 1 — split your crate

Before anything else, do the split from exercise 00: move your code into
`src/lib.rs`, leave a thin `src/main.rs`. Create `src/position.rs`.

Two minutes now, and every later exercise adds a module instead of growing a
1000-line `main.rs`.

### Step 2 — the conversion function

In `src/position.rs`, define your wire type and one function:

```rust
pub struct Position { line: usize, column: usize, end_line: usize, end_column: usize }

// signature only — you write the body
pub fn to_position(index: &LineIndex, source: &str, range: TextRange) -> Position;
```

Two `line_column` calls, four field extractions. The only decisions are
`get()` vs `to_zero_indexed()` on each of the four numbers, and there are
exactly two right answers and two wrong ones.

Then a small binary that takes a file path and a pair of byte offsets, and
prints the `Position`. Two checks on `ascii.py` (100 bytes, ASCII only, so byte
offsets and character offsets agree — that is what makes it the baseline):

| range | expected `Position` | why |
|---|---|---|
| `0..15` | line 1, col 0 → line 1, col 15 | `def greet(name)` — one line, no surprises |
| `77..99` | line 7, col 0 → line 7, col 22 | the last statement, `        greet("world")` |

If the second one gives you `end_line: 8, end_column: 0`, you passed 100 instead
of 99 — byte 99 is the final `\n`, and an offset *after* a newline is on the
next line. Ranges that include their trailing newline are a real source of
off-by-one lines, and AST node ranges never do.

### Step 3 — prove the encoding claim

Print, for `python/unicode.py`, the position of the **whole file** and of a few
handpicked offsets. Then answer, from your own output:

- The `(` in `café(` on line 1 is at byte offset **9** within the line, and
  character offset **8**. Which one does `line_column` report?
- Line 2 (`    """Résumé 🎉 of the thing."""`) is **37 bytes** and **32
  characters** long. What is `end_column` for a range covering that line?

The emoji is doing real work here: it is 4 bytes in UTF-8 and **2** UTF-16 code
units, so it is the one character that distinguishes all three encodings at
once. If you ever need LSP positions (UTF-16), this is the fixture that tells
you whether you got it right.

### Step 4 — BOM

`python/bom.py` starts with three bytes you cannot see: `EF BB BF`. So `def`
starts at byte offset **3**, not 0.

Print `line_column(3)` and `source_location(3, …, Utf32)` for that file. They
disagree. **[verified]** — `line_column` deliberately subtracts the BOM on line
1, `source_location` deliberately does not, because `source_location` has to
round-trip back to an offset.

Which one matches what parso reports for that `def`? Answer it, write it down,
and note which method you must therefore use. (You already chose `line_column`
in step 3 for a different reason. This is a second, independent reason.)

### Step 5 — tabs and CRLF

Predict before running:

- `python/tabs.py` line 3 is `\t\treturn tabbed(x - 1)`. What column does
  `return` start at — 2, or 8, or 16?
- `python/crlf.py` line 1 is `def crlf_fn(x):\r\n`. What is the `end_column` of
  a range covering just `def crlf_fn(x):`? Does the `\r` count?

Then run and compare. One of these two probably surprised you; both have
bitten real language servers.

### Step 6 — the performance point, felt not argued

Write two loops over `ascii.py`, converting the same 200 offsets to positions:

- loop A calls `LineIndex::from_source_text(source)` inside the loop
- loop B builds it once outside

Time both with `std::time::Instant`. The ratio is the whole argument for why ty
does not carry line/column on nodes — and it is bigger than most people guess.
On a real 2000-line file the gap is dramatic.

Then note what this means for the real pipeline: `line_index(db, file)` is
salsa-tracked, so in production you pay that cost **once per file per edit**, not
once per request. Exercise 03 makes you measure that.

---

## Traps

- **`.range()` does not resolve.** You forgot `use ruff_text_size::Ranged;`.
  The method comes from the trait, and the trait must be in scope. This is the
  single most common early stumble with ruff's API.
- **`end` is exclusive.** `TextRange { start: 4, end: 9 }` covers bytes 4–8.
  This matches parso's `end_pos`, so no adjustment — but only if you did not
  "helpfully" subtract 1.
- **`column.get()` when you meant `to_zero_indexed()`.** Every column in your
  output is one too big. `ascii.py` will not catch it if you only eyeball line
  numbers, so assert on a known value.
- **Building a `LineIndex` per node.** Correct output, quadratic cost. Nothing
  fails; it just gets slow on big files, which is the failure mode you are
  supposedly fixing.
- **Slicing source by byte range and panicking.** `&source[range]` panics if the
  range does not land on a character boundary. Node ranges always do — but a
  range *you* computed by arithmetic might not. Prefer
  `SourceCode::slice(ranged)`.

---

## Done when

- [ ] `to_position` converts any `TextRange` to your wire shape
- [ ] you can state which encoding `line_column` uses, and cite where you saw it
- [ ] you know why `line_column` and `source_location` disagree on `bom.py`
- [ ] you can answer the tab and CRLF questions from your own output
- [ ] you measured loop A vs loop B and wrote the ratio down
- [ ] you use `ruff_db::source::line_index(db, file)` in the db path

---

→ [`exam.md`](exam.md), then [`../02-parse-and-ast/README.md`](../02-parse-and-ast/README.md)
