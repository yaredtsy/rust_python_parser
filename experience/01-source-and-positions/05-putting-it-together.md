# 05 — Putting it together: `src/position.rs`

Four objects, one module, one function that everything downstream depends on.

---

## What you are building

```
   File ──line_index(db, file)──►  LineIndex  ┐
        └─source_text(db, file)──►  SourceText ┘
                                        │
                                   SourceCode
                                        │
   TextRange ───────────────────────────┴──► Position { line, column,
                                                        end_line, end_column }
```

One function, `to_position`. Every node your driver ever emits carries its
output.

---

## The module

`src/position.rs`:

```rust
use ruff_source_file::{LineIndex, SourceCode};
use ruff_text_size::TextRange;

/// Your wire format's position. 1-based line, 0-based column — parso's
/// convention, which v-noc depends on (plan/00-orientation/01).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct Position {
    pub line: usize,
    pub column: usize,
    pub end_line: usize,
    pub end_column: usize,
}

/// Convert a byte range to a wire-format position.
///
/// Columns are CHARACTER counts, matching parso: `LineIndex::line_column`
/// uses `PositionEncoding::Utf32` internally [verified, line_index.rs:117].
pub fn to_position(index: &LineIndex, source: &str, range: TextRange) -> Position {
    let start = index.line_column(range.start(), source);
    let end = index.line_column(range.end(), source);

    Position {
        line: start.line.get(),
        column: start.column.to_zero_indexed(),
        end_line: end.line.get(),
        end_column: end.column.to_zero_indexed(),
    }
}

/// The same thing, when you already have a `SourceCode`.
pub fn to_position_with(code: &SourceCode<'_, '_>, range: TextRange) -> Position {
    let start = code.line_column(range.start());
    let end = code.line_column(range.end());

    Position {
        line: start.line.get(),
        column: start.column.to_zero_indexed(),
        end_line: end.line.get(),
        end_column: end.column.to_zero_indexed(),
    }
}
```

Then `pub mod position;` in `src/lib.rs`.

**Why two functions?** Because you will have a `SourceCode` inside a walk
(exercise 02) and only a raw `(index, source)` pair at the RPC boundary, where
the content may have come from the client rather than the database. Two thin
wrappers is better than one function you have to adapt to at every call site.

**Rust note — the derives.** `Copy` because it is four `usize`s and copying is
cheaper than referencing. `PartialEq, Eq` so you can `assert_eq!` it in tests —
which you are about to. `Serialize` for the JSON. Deriving costs you nothing at
runtime and each one buys a specific thing.

---

## The tests — write these before the fixtures

In `src/position.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use ruff_source_file::LineIndex;
    use ruff_text_size::TextSize;

    fn pos(source: &str, start: u32, end: u32) -> Position {
        let index = LineIndex::from_source_text(source);
        to_position(
            &index,
            source,
            TextRange::new(TextSize::new(start), TextSize::new(end)),
        )
    }

    #[test]
    fn non_zero_column() {
        // `greet` in `def greet(name):` — bytes 4..9, line 1
        let p = pos("def greet(name):\n    return 1\n", 4, 9);
        assert_eq!(p, Position { line: 1, column: 4, end_line: 1, end_column: 9 });
    }

    #[test]
    fn characters_not_bytes() {
        // `(` in `def café(x):` is byte 9, character 8
        let src = "def café(x):\n";
        let p = pos(src, 9, 9);
        assert_eq!(p.column, 8, "columns must count characters, not bytes");
    }

    #[test]
    fn end_before_newline() {
        let src = "a = 1\nb = 2\n";
        // 0..5 is `a = 1`, not including the newline at byte 5
        let p = pos(src, 0, 5);
        assert_eq!((p.end_line, p.end_column), (1, 5));
        // 0..6 INCLUDES the newline, so the end lands on line 2
        let p = pos(src, 0, 6);
        assert_eq!((p.end_line, p.end_column), (2, 0));
    }
}
```

Three tests, three distinct wrong implementations rejected:

| test | rejects |
|---|---|
| `non_zero_column` | `.get()` where `.to_zero_indexed()` belongs |
| `characters_not_bytes` | any byte-based column implementation |
| `end_before_newline` | ranges that swallow their trailing newline |

`cargo test` should be green. **This is the first real test in your project** and
it will still be running when the interpreter is finished.

`LineIndex::from_source_text` is correct *in a test* — the source is a literal,
not a database file. That is the legitimate use from object 3.

---

## Now run it on the fixtures

Add a CLI command that takes a file and two offsets:

```bash
cargo run --bin pylspt-dev -- pos python/ascii.py 4 9
cargo run --bin pylspt-dev -- pos python/unicode.py 79 86
```

Fill in this table from your own output. Predictions first, in a second column.

| file | range | expected | why |
|---|---|---|---|
| `ascii.py` | `4..9` | 1:4 → 1:9 | `greet`, all ASCII |
| `ascii.py` | `77..99` | 7:0 → 7:22 | last statement, no trailing `\n` |
| `unicode.py` | offset `9` | col **8** | `(` on line 1: byte 9, char 8 |
| `unicode.py` | `20..57` | 2:4 → 2:**32** | full line 2: 37 bytes, 32 chars |
| `unicode.py` | `79..86` | 6:0 → 6:? | `café(1)` — predict the end column |
| `bom.py` | offset `3` | col **0** | BOM stripped by `line_column` |
| `tabs.py` | line 3 `return` | col **2** | a tab is one character |
| `crlf.py` | `0..15` | 1:0 → 1:15 | `\r` is at byte 15, excluded |

The `unicode.py` `79..86` row is the one to work out yourself: line 6 is
`café(1)`, which starts at byte 79. `café` is 5 bytes and 4 characters, so the
`(` is at byte 84 / character 4, and the range `79..86` covers `café(1` — six
characters. Check whether your output agrees.

---

## The timing experiment

```rust
use std::time::Instant;

let offsets: Vec<TextSize> = (0..200).map(|i| TextSize::new(i * 3)).collect();

// A: rebuild the index every time
let t = Instant::now();
for &o in &offsets {
    let index = LineIndex::from_source_text(source);
    let _ = index.line_column(o, source);
}
let rebuilt = t.elapsed();

// B: build it once
let t = Instant::now();
let index = LineIndex::from_source_text(source);
for &o in &offsets {
    let _ = index.line_column(o, source);
}
let once = t.elapsed();

println!("A (rebuild): {rebuilt:?}");
println!("B (once):    {once:?}");
println!("ratio:       {:.0}x", rebuilt.as_secs_f64() / once.as_secs_f64());
```

Run it on `ascii.py` (100 bytes) and then on a large real file — a few thousand
lines. **The ratio grows with file size**, because A is O(file) per conversion
and B is O(log lines).

Write down: file size, number of conversions, ratio. Then answer: in the real
pipeline, what makes this cost nearly vanish?

*(Answer: `line_index(db, file)` is salsa-tracked, so it is built once per file
per revision no matter how many requests, nodes or conversions ask for it. You
never pay A. Exercise 03 measures that directly.)*

---

## Done when

- [ ] `src/position.rs` exists with `Position` and `to_position`
- [ ] `pub mod position;` is in `lib.rs`
- [ ] all three unit tests pass
- [ ] the fixture table is filled in from real output, with predictions recorded
- [ ] you can state which encoding `line_column` uses and cite the line
- [ ] you know why `line_column` and `source_location` differ on `bom.py`
- [ ] the timing ratio is written down, with the file size
- [ ] your code uses `line_index(db, file)` on the database path

---

## Exam for the whole exercise

The combined exam is in [`exam.md`](exam.md) — it covers all four objects plus
this assembly. Do it after this file.

---

## What this unlocks

Exercise 02 emits a **node tree**, and every node in it has a `position` field.
That field is `to_position`'s output. If it is wrong, everything downstream is
wrong in a way that looks like a different bug — "v-noc highlights the wrong
line" is reported as a v-noc problem, not as a column-encoding problem.

So the four objects you just learned are load-bearing in a way that is easy to
underrate. The three unit tests above are the cheapest insurance in the project.

→ [`../02-parse-and-ast/README.md`](../02-parse-and-ast/README.md)
