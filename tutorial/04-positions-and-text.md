# 4. Positions: how ty points at code

This is the easiest chapter, and one of the most useful. Get this right and a
whole class of bugs disappears.

---

## The difference in one picture

Take this file:

```python
def f():
    return 1
```

**parso** says: `f` starts at line 1, column 4.

**Ruff** says: `f` starts at byte 4.

```
 byte:  0    1    2    3    4    5    6    7    8    9   10
        d    e    f    ' '  f    (    )    :    \n   ' '  ' '
        └──────────────┘    ↑
          "def "            └── byte 4
```

That is the whole difference. Ruff counts **bytes from the start of the file**.
parso counts **lines, then columns inside the line**.

---

## Why bytes are faster

To know that something is at "line 12, column 4", you must know where all the
newlines are. That means scanning the text, or storing a list of line starts,
and doing that work every time you make a node.

A byte offset is just a number. Making a node costs nothing extra.

```
parso node:   start_pos = (12, 4)     ← needs newline knowledge
              end_pos   = (12, 9)

ruff node:    range = 143..148        ← just two numbers
```

The types are:

```rust
pub struct TextSize(u32);              // one position: a byte offset

pub struct TextRange {                 // a span: from one offset to another
    start: TextSize,
    end: TextSize,
}
```

Both are 4 and 8 bytes. Both are `Copy`. Passing them around is free.

> `u32` means the maximum file size is about 4 GB. That is fine.

---

## Getting a range from a node

Every AST node can give you its range, through the `Ranged` trait:

```rust
use ruff_text_size::Ranged;        // ← REQUIRED, or .range() does not exist

let r: TextRange = call.range();
let start: TextSize = r.start();
let end: TextSize = r.end();
```

> **Remember from chapter 2:** if you forget the `use ruff_text_size::Ranged;`
> line, Rust will say "no method named `range`". The method exists, but a
> trait's methods only work when the trait is imported. This will happen to you
> at least once.

### A surprise: some nodes compute their range

Most nodes store the range in a field:

```rust
pub struct ExprName {
    pub range: TextRange,      // ← stored
    pub id: Name,
    pub ctx: ExprContext,
}
```

But `ExprCall` does not:

```rust
pub struct ExprCall {
    pub range_start: TextSize,   // ← only the start is stored!
    pub func: Box<Expr>,
    pub arguments: Arguments,
}
```

Its end comes from the arguments. The `Ranged` trait puts them together for you,
so `call.range()` still works. But if you ever try to read `call.range` as a
field, it is not there. Use the method, not the field.

---

## Getting the source text of a node

Because a range is byte offsets, slicing the source is direct:

```rust
let text = &source[call.func.range()];    // the text of the callee
```

That is it. No walking the tree, no joining child tokens.

Compare to your Python, which has to rebuild the text from parts:

```python
# parser.py:136-141
def _get_clean_code(self, node) -> str:
    if hasattr(node, "children"):
        return "".join(self._get_clean_code(child) for child in node.children)
    if hasattr(node, "get_code"):
        return node.get_code(include_prefix=False)
    return ""
```

The Rust version is one slice. It is also faster, because there is no string
building.

---

## Converting back to line and column

Your JSON output needs line and column, because that is what the current driver
sends. So you convert at the edge.

The tool is `LineIndex`:

```rust
use ruff_source_file::LineIndex;

let index = LineIndex::from_source_text(source);   // build ONCE per file
let lc = index.line_column(offset, source);
```

`LineIndex` scans the file once and stores where every line starts. After that,
each lookup is a fast binary search.

**Build it once per file, never per node.** Building it inside a loop over
nodes would make your parser slow in a way that is hard to notice.

### The 1-based / 0-based trap

Your wire format uses:

- **line**: starts at 1
- **column**: starts at 0

That is parso's rule. `LineIndex` gives you `OneIndexed` values, where both
start at 1. So the column needs a subtraction:

```rust
NodePosition {
    line:   lc.line.get(),            // 1-based → 1-based. No change.
    column: lc.column.get() - 1,      // 1-based → 0-based. Subtract 1.
}
```

Get this wrong and every column in your output is off by one. Everything will
look almost right, which is the worst kind of bug.

### The character-vs-byte trap

This one is more subtle. Consider:

```python
x = "héllo"
f()
```

The `é` character takes **two bytes** in UTF-8 but is **one character**.

So on that line:

- byte offset of the closing quote: one number
- character offset of the closing quote: a smaller number

parso counts **characters**. If you ask `LineIndex` for a byte column, your
output will disagree with the Python driver on any line with non-ASCII text.

`LineIndex` can give you different kinds of column. Check which methods your
version of Ruff offers and pick the **character** one.

> **Write this test early:**
>
> ```python
> def f():
>     """Doc with emoji 🎉 inside."""
>     g()
> ```
>
> Run it through both drivers. If the columns match, you chose right.

---

## Ranges are cheap to compare and store

Because a range is two numbers, you can use it as a map key:

```rust
let mut seen: FxHashSet<TextRange> = FxHashSet::default();
if seen.insert(node.range()) {
    // first time we have seen a node at this exact position
}
```

This replaces your position-dedup code:

```python
# parser.py:100-108
pos_key = (node.position.line, node.position.column,
           node.position.end_line, node.position.end_column)
if pos_key not in seen_positions:
    ...
```

A 4-tuple of Python ints becomes one 8-byte value. Hashing it is one step
instead of four.

---

## Useful `TextRange` operations

```rust
range.start()              // TextSize
range.end()                // TextSize
range.len()                // how many bytes
range.is_empty()           // start == end
range.contains(offset)     // is this offset inside?
range.contains_range(other)// is the other range fully inside?

TextRange::new(start, end)
TextRange::empty(at)       // a zero-width point — used for inserting text
```

`TextRange::empty(at)` is how you say "insert here" without replacing anything.
The plan uses it for injecting `ID:` docstrings.

---

## Summary table

| Task | parso | Ruff |
|---|---|---|
| position of a node | `node.start_pos` → `(line, col)` | `node.range()` → `TextRange` |
| text of a node | rebuild from children | `&source[node.range()]` |
| compare positions | compare 2-tuples | compare `u32` |
| dedup by position | 4-tuple in a set | `TextRange` in a set |
| line and column | free (already there) | `LineIndex`, built once |

Ruff makes the common case (ranges) free and the rare case (line/column) cost a
little. parso does the opposite.

---

## Check yourself

1. Why does Ruff store byte offsets instead of line and column?
2. What must you import before `node.range()` compiles?
3. Why is `ExprCall` different from other nodes?
4. What will break if you build a `LineIndex` inside your node loop?
5. Which two off-by-one traps are in this chapter?

---

→ Next: [`05-parser-and-ast.md`](05-parser-and-ast.md)
