# Answers 01 — Source and positions

---

**1.** Because a `(line, column)` pair cannot be produced without knowing where
every newline is. If nodes carry line/column, then *the parser* must maintain a
line table, and every layer above it inherits a two-field position it must keep
consistent. With byte offsets the position is intrinsic to the slice — the
parser already knows where it is in the buffer. Line/column becomes a rendering
concern at the edge, computed on demand, once, by whoever actually needs to show
a human a number.

The speed follows from that, but the structural point is the ordering:
*offsets are what the machine has; line/column is what the human wants.*

**2.**

- **Comparison and containment.** "Is node A inside node B" is two `u32`
  comparisons. In parso it is a lexicographic tuple comparison on two
  `(line, col)` pairs, four fields, with a subtlety when lines are equal.
- **Hashing and storage.** `TextRange` is `Copy` + `Hash`, so a set of seen
  positions is `FxHashSet<TextRange>`, 8 bytes per entry, no allocation. In
  Python each position is a tuple of four boxed ints.

**3.** `FxHashSet<TextRange>` — insert, and skip the node if `insert` returns
`false`. Cheaper because one 8-byte `Copy` key replaces a 4-tuple of Python
integers (each a heap object), and `FxHash` on a `u64` is a couple of
instructions versus tuple hashing plus per-element `__hash__` dispatch.

**4.** `line_index(db, file)` is `#[salsa::tracked]` **[verified,
`ruff_db/src/source.rs:205`]**: computed once per file per revision, shared
across every consumer in the process, invalidated automatically when the file
changes. `from_source_text` builds a fresh one every call.

Use `from_source_text` only for text that is **not in the database** — the
`parse_file(file_path, content)` RPC where the client sends unsaved editor
content. That string has no `File`, so no query can be keyed on it.

**5.**

```rust
line:       lc.line.get(),                  // 1-based, passes through
column:     lc.column.to_zero_indexed(),    // 0-based
end_line:   end.line.get(),
end_column: end.column.to_zero_indexed(),
```

`to_zero_indexed()` reads better than `get() - 1` and cannot underflow.

---

**6.** `PositionEncoding::Utf32`, implemented as `up_to_character.chars().count()`
— **[verified, `ruff_source_file/src/line_index.rs:117` and `:210`]**. "Utf32"
here means "count Unicode scalar values", i.e. characters. It is not about how
the file is stored.

**7.** `line_column` reports **8**. parso reports **8** — Python string indices
are code points, so parso's columns are character counts too. They agree, which
is why `line_column` is the right method with no encoding argument needed.

UTF-16 agrees on line 1 because every character there (`c`, `a`, `f`, `é`, …) is
in the Basic Multilingual Plane, so one code point = one UTF-16 unit. It stops
agreeing on line 2 because 🎉 is outside the BMP: one character, but a surrogate
**pair** — two UTF-16 units.

**8.** **[verified]** by measurement:

| encoding | length |
|---|---|
| characters (Utf32) | **32** |
| UTF-8 bytes | 37 |
| UTF-16 units | 33 |

`end_column` is **32**. If you got 37 you used a byte offset; if you got 33 you
used UTF-16.

**9.** `source_location(offset, text, PositionEncoding::Utf16)`, reading
`.character_offset` instead of `.column`. Your helper needs the encoding as a
parameter rather than baked in — and note that `source_location` does **not**
strip the BOM, so if you serve LSP positions you must decide the BOM policy
separately. (LSP's answer: the BOM is part of the document, so not stripping is
correct there. Your wire format's answer is the opposite. This is a real reason
to keep two functions rather than one clever one.)

---

**10.** Byte **3**. The BOM is `EF BB BF`, three bytes, and it is part of the
file content — ty does not strip it from the buffer.

**11.**

| call | value |
|---|---|
| `line_column(3).column.to_zero_indexed()` | **0** |
| `source_location(3, Utf32).character_offset.to_zero_indexed()` | **1** |

They differ because `line_column` explicitly subtracts the BOM when the offset
is on line 1 **[verified, `line_index.rs:119-124`]**, and `source_location`
explicitly does not.

**0** matches parso. CPython's tokenizer treats a leading BOM as not part of the
source text, so `def` is at column 0.

**12.** `source_location` is the bidirectional one — it pairs with
`LineIndex::offset(...)` to map back. That only works if the mapping is
injective, and stripping the BOM breaks that: both byte 0 and byte 3 would map
to character offset 0, so you could not recover which one you started from.
`line_column` is the *display* function; `source_location` is the *coordinate*
function.

---

**13.** Column **2**. `LineIndex` counts characters, and a tab is one character.
No tab expansion happens anywhere.

Guessing 8 or 16 means you were thinking of a *rendered* column — how far right
the text appears in an editor with a 4- or 8-wide tab stop. That is a display
concern that depends on editor settings, and no position API in this stack does
it. parso agrees: it counts characters too.

**14.** `end_column` for `0..15` is **15** — offset 15 is the `\r`, and the
distance from the line start is 15 characters.

For `0..16` it would be **16**: offset 16 is the `\n`, still counted as being on
line 1, one character further along.

A node range is never `0..16` because `\r\n` is trivia — the AST's
`StmtFunctionDef` for line 1 ends at the `:`, and no node's range includes its
line terminator. This is worth confirming in exercise 02 rather than trusting;
it is exactly the sort of thing that changes the last column of every node if it
is ever false.

**15.**

| fixture | what it catches |
|---|---|
| `unicode.py` | byte vs character columns — the big one. every offset in `ascii.py` is identical in all three encodings, so an entirely byte-based implementation passes |
| `bom.py` | off-by-one on the first line only, in files that look fine everywhere else |
| `crlf.py` | line-terminator handling; a naive "split on `\n`" leaves `\r` in the line and shifts nothing visibly until end columns matter |
| `tabs.py` | tab expansion, if someone "helpfully" adds it |

The general point: **a test corpus of ASCII files cannot fail a positions
implementation**, no matter how large it is. Two hundred real files from a
Western codebase are largely ASCII, so the M2 gate in `plan/04-build/02` needs
these fixtures deliberately added.

---

**16.** Expect roughly one to two orders of magnitude on a file of a few
thousand bytes, growing with file size, because loop A is O(file) per
conversion and loop B is O(log lines).

In the real pipeline the cost mostly disappears because `line_index(db, file)`
is salsa-tracked: it is built once per file per revision, no matter how many
requests, nodes, or conversions ask for it. The remaining cost is the binary
search per conversion, which is negligible. Exercise 03 makes you observe the
caching directly.

**17.** Something with a **non-zero expected column**, on a known offset:

```rust
// ascii.py, offset 4 is the `g` of `greet` on line 1
let p = to_position(&index, source, TextRange::new(4.into(), 9.into()));
assert_eq!((p.line, p.column), (1, 4));
assert_eq!((p.end_line, p.end_column), (1, 9));
```

The `column == 4` is what does the work. An assertion on offset 0 passes with
both `get()` and `to_zero_indexed()` if you also mis-assert the expected value,
and an assertion that only checks `line` never touches the bug at all. **Pick
the assertion by asking "which wrong implementation does this reject?"**
