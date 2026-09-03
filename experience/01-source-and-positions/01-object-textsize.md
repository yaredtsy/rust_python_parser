# Object 1 — `TextSize`, `TextRange`, and the `Ranged` trait

How ty points at code. Three small types, and they are everywhere.

---

## What they are

```rust
pub struct TextSize(u32);                       // a byte offset into a file
pub struct TextRange { start: TextSize, end: TextSize }   // a span
pub trait Ranged { fn range(&self) -> TextRange; }        // "I have a location"
```

That is the entire position system in ty. Two `u32`s and a trait.

```
source:   d  e  f     g  r  e  e  t  (  n  a  m  e  )  :
offset:   0  1  2  3  4  5  6  7  8  9 10 11 12 13 14 15
                      └──────────────┘
                      TextRange { start: 4, end: 9 }   ← the name `greet`
```

**`start` is inclusive, `end` is exclusive.** So `4..9` covers bytes 4, 5, 6, 7,
8 — five characters, `greet`. Same convention as Rust's own slices and as
parso's `end_pos`.

---

## Why byte offsets and not (line, column)

Jedi/parso puts `(line, column)` on every node. ty puts two integers. The
difference is not a micro-optimisation — it changes what the rest of the system
has to know.

**To produce `(line, column)` you must know where every newline is.** So if
nodes carry line/column, the *parser* must maintain a line table, and every
layer above inherits a two-field position it must keep consistent.

With offsets, the position is intrinsic: the parser already knows where it is in
the buffer, so recording it costs nothing. Line and column become a **rendering**
concern — computed once, at the edge, by whoever needs to show a human a number.

Three things fall out of that, and all three matter to you:

1. **Comparison is trivial.** "Is A inside B?" is two integer comparisons
   (`contains_range`). In parso it is a lexicographic comparison of two
   `(line, col)` tuples with a special case when the lines are equal.
2. **Positions are cheap to store.** `TextRange` is 8 bytes, `Copy`, `Hash`. Your
   `parser.py` position-dedup quirk (quirk 10) becomes an
   `FxHashSet<TextRange>` — one 8-byte key instead of a tuple of four boxed
   Python integers.
3. **Line/column costs a lookup** — which is object 3, and why you build a
   `LineIndex` once per file rather than once per node.

---

## Where they come from

```rust
use ruff_text_size::{Ranged, TextRange, TextSize};
```

⚠ **`Ranged` must be imported or `.range()` does not exist.** This is the
"no method named X" error from exercise 00, third occurrence:

```
error[E0599]: no method named `range` found for reference `&ExprCall`
```

Every AST node implements `Ranged`, but a trait method needs its trait in scope.
This is the single most common early stumble with ruff's API — the plan calls it
out explicitly in `plan/01-crates/01-crate-map.md`.

---

## What you can do with `TextSize`

**[verified]** from `ruff_text_size/src/size.rs`.

| item | notes |
|---|---|
| `TextSize::new(4u32)` | ★ construct |
| `TextSize::from(4u32)` | ★ same, via `From` |
| `4.into()` | ★ where the target type is known |
| `TextSize::of("greet")` | length of a string **in bytes** — handy |
| `.to_u32()`, `.to_usize()` | ★ get the number out |
| `+`, `-` | ordinary arithmetic (panics on overflow in debug) |
| `.checked_add(other)`, `.checked_sub(other)` | `Option` — no panic |
| `.saturating_add/sub` | clamps instead of panicking |

**Rust note — `.into()` and `From`.** `TextSize::from(4u32)` and `4u32.into()`
are the same call. `.into()` only works when Rust can infer the target type:

```rust
let a: TextSize = 4.into();                       // ✓ annotation tells it
let r = TextRange::new(4.into(), 9.into());       // ✓ parameter types tell it
let b = 4.into();                                 // ✗ into WHAT?
```

You will see `4.into()` throughout ruff's own code and tests. It is not magic —
it is `From` with the type inferred from context.

---

## What you can do with `TextRange`

**[verified]** from `ruff_text_size/src/range.rs`. The ones you will use are ★.

### Making one

| call | meaning |
|---|---|
| `TextRange::new(start, end)` | ★ from two offsets |
| `TextRange::at(offset, len)` | from a start and a length |
| `TextRange::empty(offset)` | ★ a zero-width range — "a point" |
| `TextRange::up_to(end)` | from 0 to `end` |

`TextRange::empty` is more useful than it sounds. When you need the *column of
the `(`* in a call (exercise 02's `call_col_pos`), you have a single offset and
need to convert it — so you make an empty range at that offset, or just convert
the offset directly.

### Reading one

| call | returns |
|---|---|
| `.start()`, `.end()` | ★ `TextSize` |
| `.len()` | `TextSize` — the width |
| `.is_empty()` | `bool` |
| `.to_std_range()` | `Range<usize>` — for slicing with `&source[..]` |

### Comparing

| call | meaning |
|---|---|
| `.contains(offset)` | ★ is this offset inside |
| `.contains_inclusive(offset)` | …including the end |
| `.contains_range(other)` | ★★ is `other` nested inside me — **the parent/child test** |
| `.intersect(other)` | `Option<TextRange>` — the overlap, if any |
| `.cover(other)` | ★ the smallest range containing both |
| `.ordering(other)` | an `Ordering`, for sorting |

`contains_range` is how you ask "is this call inside that function?" — which is
most of what a tree-building walk does. `cover` is how you compute a parent's
span from its children.

### Adjusting

| call | meaning |
|---|---|
| `.with_start(o)`, `.with_end(o)` | replace one edge |
| `.add_start(n)`, `.sub_start(n)` | move the start |
| `.add_end(n)`, `.sub_end(n)` | move the end |
| `.checked_add(o)`, `.checked_sub(o)` | shift the whole range |

These return **new** ranges; `TextRange` is `Copy` and immutable in practice.

---

## Example 1 — offsets by hand

```rust
use ruff_text_size::{TextRange, TextSize};

fn main() {
    let source = "def greet(name):";

    let name = TextRange::new(TextSize::new(4), TextSize::new(9));

    println!("start   = {}", name.start().to_u32());        // 4
    println!("end     = {}", name.end().to_u32());          // 9
    println!("len     = {}", name.len().to_u32());          // 5
    println!("text    = {:?}", &source[name.to_std_range()]); // "greet"

    // Ranges are Copy — using it twice is fine, no clone needed.
    let whole = TextRange::new(TextSize::new(0), TextSize::of(source));
    println!("nested  = {}", whole.contains_range(name));   // true
    println!("at 4    = {}", name.contains(TextSize::new(4)));  // true
    println!("at 9    = {}", name.contains(TextSize::new(9)));  // false ← exclusive end
}
```

The last two lines are the whole inclusive/exclusive story. Offset 9 is the `(`,
which is *not* part of the name.

**Rust note — slicing.** `&source[name.to_std_range()]` gives `&str`. You could
also write `&source[4..9]`. Both **panic** if the boundaries are not on UTF-8
character boundaries — see the trap below.

---

## Example 2 — the parent/child test

```rust
use ruff_text_size::{Ranged, TextRange};

/// Which of `candidates` is the innermost one containing `target`?
fn innermost_containing<'a, T: Ranged>(
    candidates: &'a [T],
    target: TextRange,
) -> Option<&'a T> {
    candidates
        .iter()
        .filter(|c| c.range().contains_range(target))
        .min_by_key(|c| c.range().len().to_u32())
}
```

**Rust notes:**

- `T: Ranged` — a **generic bound**. This function works for any type that has a
  range: `ExprCall`, `StmtFunctionDef`, your own node type. You write it once.
- `.filter(...).min_by_key(...)` — iterator chain. `filter` keeps the containing
  ones; `min_by_key` picks the smallest, i.e. the innermost.
- `|c| ...` is a closure (a lambda). `c` is a `&&T` here because `iter()` yields
  `&T` and `filter` passes `&&T`; Rust auto-derefs when you call `.range()`, so
  you do not have to think about it.

This is the shape of "what node is at this position", which your `parse_file`
RPC needs. Ruff has a purpose-built version —
`ruff_python_ast::find_node::CoveringNode` — but writing it once by hand is how
you learn that node containment is just integer comparison.

---

## The trap: slicing on a non-boundary

```rust
let source = "café";                 // 5 bytes: c a f 0xC3 0xA9
let bad = &source[0..4];             // 💥 PANIC
```

`é` occupies bytes 3 and 4. Slicing at 4 lands **inside** a character, and Rust
refuses:

```
byte index 4 is not a char boundary; it is inside 'é' (bytes 3..5) of `café`
```

**When does this bite you?** Not when you use node ranges — the parser always
produces boundaries. It bites when *you* compute a range arithmetically:
`range.add_start(1.into())` to "skip the quote", or `end - 3` to "drop the
`"""`". On ASCII it works; on `python/unicode.py` it panics.

Two defences:

- prefer ranges that came from the AST over ranges you computed
- when you must compute, use `SourceCode::slice` (object 4) or check with
  `source.is_char_boundary(n)` first

---

## Exercise

**A.** Write a binary that takes a file path and two numbers, and prints the
slice of the file between them, plus the range's length and whether the file's
whole range contains it. Run it on
`experience/01-source-and-positions/python/ascii.py` with `4 9` — you should get
`"greet"`.

**B.** Run the same program on `python/unicode.py` with `4 9`. Then with `4 8`.
One of them panics. Explain why, and fix your program so it reports a friendly
error instead of crashing. (`str::is_char_boundary` is the tool.)

**C.** Write `innermost_containing` from example 2, then use it on a hand-built
list of three overlapping `TextRange`s to prove it picks the smallest.

**D.** Implement this and test it on `ascii.py`:

```rust
/// The smallest range covering all of `ranges`, or None if empty.
fn cover_all(ranges: &[TextRange]) -> Option<TextRange>
```

`cover` plus `fold` does it in one line. This is how you would compute a
function's span from its children if you ever needed to.

---

## Exam

**1.** What is `TextSize`? How many bytes is a `TextRange`, and why does that
matter?

**2.** Give the structural reason ty uses offsets rather than `(line, column)` —
not "it is faster", but what it removes from the rest of the system.

**3.** Is `TextRange`'s end inclusive or exclusive? Which parso field does that
match?

**4.** Name three things that become cheap because `TextRange` is `Copy + Hash`,
and connect one of them to a quirk in `plan/00-orientation/01`.

**5.** You write `call.range()` and get "no method named `range`". What is wrong?

**6.** Which method answers "is this call inside that function"? Which answers
"what is the span covering both of these"?

**7.** `&source[range.to_std_range()]` panics. Give an input where it does, and
say whether a range that came from the AST could cause it.

**8.** What does `4.into()` mean, and when does it fail to compile?

**9.** You want the column of the `(` in `foo(1)`, and you have only its offset.
What do you build to pass it to a position-conversion function?

---

## Answers

**1.** A byte offset into a file, stored as a `u32`. A `TextRange` is two of
them — **8 bytes**, `Copy`, `Hash`, `Eq`. That matters because positions get
stored, compared and hashed constantly: an 8-byte `Copy` key needs no
allocation, so a set of seen positions is nearly free.

**2.** It removes the need for anything except the display layer to know where
the newlines are. Producing `(line, column)` requires a line table; if nodes
carry it, the parser must maintain one and every layer above inherits a
two-field position to keep consistent. With offsets the position is intrinsic to
the buffer the parser is already walking.

**3.** **Exclusive.** `4..9` covers bytes 4–8. It matches parso's `end_pos`,
which is the position just past the node — so no adjustment is needed when you
convert, provided you did not "helpfully" subtract one.

**4.** Containment and comparison (two integer compares); storage in a
`FxHashSet<TextRange>` with no allocation; passing by value with no clone or
borrow. The position-dedup rule (quirk 10) is the connection: `_scan_children`
drops nodes sharing an identical 4-tuple position, which in Rust is one hash-set
insert on an 8-byte key.

**5.** `use ruff_text_size::Ranged;` is missing. `.range()` comes from the trait,
and Rust requires a trait to be in scope before you can call its methods. Third
time you have met this error — `System`, `Db`, now `Ranged`.

**6.** `a.contains_range(b)` for nesting; `a.cover(b)` for the smallest range
containing both.

**7.** `&"café"[0..4]` panics — `é` is bytes 3–4, so 4 is inside a character.

A range from the AST **cannot** cause it: the parser only produces boundaries at
real token edges. The danger is ranges *you* compute — `add_start(1)`,
`end - 3`, or anything derived by arithmetic to "skip a quote".

**8.** It is `TextSize::from(4)` with the target type inferred from context. It
fails when there is nothing to infer from — `let b = 4.into();` cannot know what
to convert into. Give it an annotation or pass it where the parameter type is
known.

**9.** `TextRange::empty(offset)` — a zero-width range at that point. Converting
its start gives you the line and column of the `(`. (`plan/02-mapping/01` uses
exactly this for `call_col_pos`.)
