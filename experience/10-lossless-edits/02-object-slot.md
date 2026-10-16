# Object 2 — `DocSlot`: where the key goes

One offset and one flag per def, derived from the AST, consumed by object 3.

---

## What it is

```rust
use ruff_text_size::TextSize;

/// Where to insert, and what shape to insert.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DocSlot {
    /// The offset the text goes at.
    pub at: TextSize,
    /// `body[0]`'s start: the indentation anchor, and the one-liner tell.
    /// Equal to `at` when there is no docstring yet.
    pub stmt_start: TextSize,
    pub kind: SlotKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotKind {
    /// A docstring exists: `at` is immediately before its closing quotes.
    InLiteral { triple_quoted: bool },
    /// None exists: a whole new docstring statement goes in at `at`.
    NewStatement,
}
```

Two offsets and one bool, all `Copy`. Everything else — the indentation, whether
this is a one-liner — object 3 derives from the source at `stmt_start`, which is
why this type does not carry it.

```rust
use ruff_python_ast::{Stmt, StringFlags};
use ruff_text_size::Ranged;
use ty_python_core::definition::docstring_from_body;

pub fn doc_slot(body: &[Stmt]) -> Option<DocSlot> {
    let stmt_start = body.first()?.start();     // broken source can give an empty body
    match docstring_from_body(body) {
        Some(doc) => {
            // The LAST part holds the closing quotes: `"a" "b"` is one literal.
            let last = doc.value.as_slice().last()?;
            let flags = last.flags;
            if flags.is_unclosed() {
                return None;                    // does not parse; write nothing
            }
            Some(DocSlot {
                at: last.range().end() - flags.closer_len(),
                stmt_start,
                kind: SlotKind::InLiteral { triple_quoted: flags.is_triple_quoted() },
            })
        }
        None => Some(DocSlot { at: stmt_start, stmt_start, kind: SlotKind::NewStatement }),
    }
}
```

---

## The API it rests on

`docstring_from_body` is ty's, and it is **public** — exercise 02 object 6 has
the citation and the correction to the plan:

```rust
// ty_python_core/src/definition.rs:229   [verified]
pub fn docstring_from_body(body: &[ast::Stmt]) -> Option<&ast::ExprStringLiteral>;
```

Everything else comes off one trait:

```rust
// ruff_python_ast/src/nodes.rs:799   [verified]
pub trait StringFlags: Copy {
    fn quote_style(self) -> Quote;
    fn triple_quotes(self) -> TripleQuotes;
    fn prefix(self) -> AnyStringPrefix;
    fn is_unclosed(self) -> bool;
    fn is_triple_quoted(self) -> bool { … }        // :811
    fn quote_str(self) -> &'static str { … }       // :817  — "\"\"\"", "'", …
    fn quote_len(self) -> TextSize { … }           // :828  — 3 or 1
    fn opener_len(self) -> TextSize { … }          // :838  — prefix + quotes
    fn closer_len(self) -> TextSize { … }          // :847  — quote_len, or 0 if unclosed
}
```

Four of those matter here, and the two easy to miss are the last two:

- **`closer_len()`, not `quote_len()`.** They differ by exactly one case —
  an unclosed string, where the closer is zero bytes long because there isn't
  one. Subtracting `quote_len()` from the end of an unclosed literal puts `at`
  three bytes *inside* the content.
- **`value.as_slice().last()`** (`nodes.rs:1420`) — not `first()`, and not the
  whole `ExprStringLiteral`. `"""a""" """b"""` is **one** docstring made of two
  `StringLiteral` parts, and only the last one has a closer.

---

## ★ Inserting before the closing quotes is always safe

The obvious worry is escapes: land after a trailing `\` and you change what the
string means. **That input cannot exist.** Python forbids a literal from ending
in an odd number of backslashes — the last one would escape the closing quote.

Measured on CPython 3.12.2:

| source | `__doc__` before | after inserting before the closer |
|---|---|---|
| `"""abc\\"""` (escaped backslash) | `'abc\\'` | `'abc\\ ID: u1'` ✓ |
| `r"""abc\"""` (raw, trailing `\`) | **SyntaxError: unterminated** | input is impossible |
| `"abc"` (single-quoted) | `'abc'` | `'abc ID: u1'` ✓ |
| `"ab" "cd"` (implicit concat) | `'abcd'` | `'abcd ID: u1'` ✓ |

`shapes.py:ends_with_backslash` is the first row, and its docstring really does
end in a backslash (`'The content ends with a backslash: C:\\'`, verified).

That table is the reason this whole exercise is insertion rather than
rewriting. Nothing has to be re-emitted, so nothing can be re-emitted *wrong*.

### The one shape that cannot take a newline

A newline inside a **single-quoted** string is a syntax error, so
`shapes.py:single_quoted` gets ` ID: u1` appended on the same line instead of
`\n\nID: u1`. That is the entire reason `SlotKind` carries `triple_quoted`, and
the reader does not care:

```python
'Single quotes: a newline before the closer would not parse. ID: u1'
```

`_extract_metadata`'s `(\S+)\s*:\s*(\S+)` finds `ID` → `u1` in the middle of a
line perfectly well (exercise 02, object 6).

### ★ Append at the end, never the start

The reader builds a dict, so for a repeated key **the last pair wins**. Appending
your key at the end of the docstring means your key is the one that wins, even
in a docstring that already mentions `ID:` in prose
(`idempotent.py:prose_mentions_id`, `docstrings.py:id_like_text`).

Insert at the *front* and prose would outrank you, intermittently, depending on
what the author wrote. The insert point is a correctness decision, not a
cosmetic one.

---

## The fourteen shapes of `shapes.py`

Nothing in that file has an `ID:`, so every row is an insert. The `byte at`
column is what `source.as_bytes()[slot.at]` is, which is the cheapest way to
check your offsets without printing the whole file.

`nl` is the file's line ending (object 3), and the triple-quoted rows split in
two depending on whether the closer owns its line — also object 3.

| def | `SlotKind` | byte at `at` | inserted text |
|---|---|---|---|
| `triple_multi` | `InLiteral { triple: true }` | `"`, closer owns its line | `{nl}    ID: u{nl}` *(at `at - 4`)* |
| `triple_single_line` | `InLiteral { triple: true }` | `"`, after text | `{nl}{nl}    ID: u{nl}    ` |
| `single_quoted` | `InLiteral { triple: false }` | `'` | ` ID: u` |
| `implicit_concat` | `InLiteral { triple: true }` | `"`, after text | `{nl}{nl}    ID: u{nl}    ` |
| `ends_with_backslash` | `InLiteral { triple: true }` | `"`, after text | `{nl}{nl}    ID: u{nl}    ` |
| `raw_prefixed` | `InLiteral { triple: true }` | `"`, closer owns its line | `{nl}    ID: u{nl}` *(at `at - 4`)* |
| `ends_with_colon` | `InLiteral { triple: true }` | `"`, after text | `{nl}{nl}    ID: u{nl}    ` ⚠ see below |
| `no_docstring` | `NewStatement` | `r` of `return` | `"""ID: u"""{nl}    ` |
| `one_liner` | `NewStatement` | `r` of `return` | `{nl}    """ID: u"""{nl}    ` |
| `ellipsis_body` | `NewStatement` | `.` of `...` | `{nl}    """ID: u"""{nl}    ` |
| `semicolon_body` | `NewStatement` | `a` of `a = 1` | `{nl}    """ID: u"""{nl}    ` |
| `NoDoc` | `NewStatement` | `d` of `def method` | `"""ID: u"""{nl}    ` |
| `NoDoc.method` | `NewStatement` | `r` of `return` | `"""ID: u"""{nl}        ` |
| `NoDoc.Inner` | `InLiteral { triple: true }` | `"`, after text | `{nl}{nl}        ID: u{nl}        ` |

Fourteen rows: twelve defs at top level, plus `NoDoc`'s two nested ones. Note
`raw_prefixed`: the `r` prefix is **in front of the opener**, so it is nowhere
near `at` and the insert cannot touch it. That is object 5's divergence from the
Python driver, which re-emits the literal and loses the prefix.

### ⚠ `NoDoc`'s slot points at `def method`

A class with no docstring whose first statement is a nested def gets
`at = stmt_start = ` the offset of `def method`. The insert goes **before** the
nested def, and the nested def's own insert is at a higher offset — which is
why object 3 applies edits bottom-up and why the two never collide.

---

## ⚠⚠ `ends_with_colon` — injection can create text the Python reader cannot read

This is the one place where writing a key can make a def look **unkeyed** to the
other implementation, and it is not hypothetical: insertion creates the input.

`shapes.py:ends_with_colon` has the docstring `"""Example:"""`. After injection
it holds `Example:` then, two lines down, `ID: u1`. The two readers disagree —
measured, both on the same string:

| reader | pairs found | `ID`? |
|---|---|---|
| Python `re.findall(r"(\S+)\s*:\s*(\S+)", doc)` | `[('Example', 'ID:')]` | **no** |
| Rust `extract_metadata` (`src/scan.rs`) | `{'ID': 'u1'}` | yes |

The regex is **consuming**. `\s*` happily crosses both newlines, so for the key
`Example` it takes the next non-space run as the value — and that run is `ID:`
itself. The match ends past it, so the scan never sees `ID` as a key. The Rust
version iterates every `:` independently and skips the pair whose value contains
a colon, so it finds the real key.

Two consequences, and they pull in opposite directions:

- **Keep the Rust reader for the "needs a key?" decision**, because it is the
  one that makes injection *idempotent against itself*. Swap in the Python
  behaviour and run two injects a second key — and then run three reads
  `ID → u2` and stops, so the file grows by exactly one key and stabilises.
  Measured; bounded, but it is still a file you did not mean to change twice.
- **The join is still broken** while the Python side reads the same docstring.
  If anything in v-noc extracts IDs itself, that def has no ID over there, and
  the graph node never links.

The durable fix is **one ID grammar, written the same way in both languages** —
something anchored, like `(?:^|\s)ID:\s*(\S+)`, which cannot be swallowed as
another key's value. That is a cross-language contract change: it must land on
both sides in the same commit, and it changes behaviour on inputs the current
readers agree about (`docstrings.py:id_like_text`, `multiple_keys`), so it needs
its own parity pass.

Until then it is a known divergence with a fixture, which is the whole point of
`ends_with_colon` existing. **Do not** "fix" it by inserting the key at the
*front* of the docstring: that dodges this input and breaks the last-wins rule
below, which is what keeps a prose `ID:` from outranking the real one.

---

## ⚠ An empty body is reachable

```rust
let stmt_start = body.first()?.start();
```

`body.first()` really can be `None`. On an error, `parse_body` returns
`Suite::new()` — `ruff_python_parser/src/parser/statement.rs:3117`
**[verified]** — so `def f():` at end of file yields a def with no statements at
all. `parse_unchecked` is what you use (exercise 02, object 1), so you *will*
see it.

Returning `None` from `doc_slot` is the quirk-13 path: "failures are swallowed
everywhere; a partial result is a valid result". A def with no slot gets no ID
this run, keeps `id: null` in the outline, and gets one as soon as the file
parses. An `unwrap` here takes the whole analyser down on a file the user is
mid-way through typing.

The same applies to `is_unclosed()`: an unterminated docstring means the file is
broken, and the correct amount of rewriting to do to a broken file is none.

---

## Rust notes

**`body.first()?.start()` needs `Ranged` in scope.** `start()` is a provided
method on the trait, not an inherent one; without `use ruff_text_size::Ranged;`
the error is "no method named `start`", which reads like a missing field rather
than a missing import.

**`last.range().end() - flags.closer_len()`** — `TextSize`'s `Sub` is plain
`u32` subtraction (`ruff_text_size/src/size.rs:159-166`, `:189`
**[verified]**), so underflow **panics in debug and wraps in release**. It
cannot underflow here — a closed literal is at least `closer_len()` bytes long —
but "cannot" is doing real work in that sentence, and a wrapped `TextSize` would
point four gigabytes into the file. Worth a one-line comment stating the
invariant; worth `checked_sub` the moment the invariant stops being one line
long.

**Offsets are bytes, not characters.** `nonascii.py` has an emoji in a docstring
and accented identifiers; `TextSize` counts UTF-8 bytes throughout (exercise 01).
Slicing a `&str` at a `TextSize` that is not a char boundary panics — which it
never is here, because every offset came from the AST.

---

## Exercise

**A.** Print a `DocSlot` for every def in `shapes.py`, as the table above:
kind, offset, and the byte at that offset. Thirteen rows — count them against
`ast` first, so you find out here rather than later that `NoDoc` holds two.

**B.** Swap `closer_len()` for `quote_len()` and run on a file with an
unterminated docstring. Then put it back and explain what the three bytes were.

**C.** Use `value.as_slice().first()` instead of `last()`. Which fixture breaks,
and what does the resulting file look like?

**D.** Take the slot for `single_quoted` and insert `\n\nID: u` anyway. Run
`python3 -c "import shapes"`. Then do the same to `triple_single_line` and
explain why only one of them fails.

**E.** Write a def whose docstring ends with `\\` and one whose docstring ends
with `\` (i.e. `r"""x\"""`). Report what the parser says about the second.

**F.** Insert the key at the **start** of `prose_mentions_id`'s docstring instead
of the end, then read it back with your `extract_metadata`. Which value comes
out, and why is appending the safer rule?

**G.** Delete the `body.first()?` guard and feed it `def f():` with nothing
after it. Then do the same with the guard in place and show the outline still
renders the def with `id: null`.

---

## Exam

**1.** What are the two offsets in `DocSlot`, and why is one of them not enough?

**2.** Why `closer_len()` rather than `quote_len()`?

**3.** `"""a""" """b"""` is one docstring. Which part do you measure from, and
what happens if you pick the other?

**4.** Inserting before a closing quote cannot corrupt an escape sequence. Why
not? Name the input you would expect to break it and say what Python does with
it.

**5.** Why does `SlotKind` carry `triple_quoted` when the offset alone would
locate the insert?

**6.** Why append the key at the end of the docstring rather than the start?
Name the fixture that makes the difference visible.

**7.** `raw_prefixed` keeps its `r` under insertion and loses it under the
Python driver. Where is the `r`, relative to `at`?

**8.** Name two inputs for which `doc_slot` returns `None`, and say what the
caller does about it.

**9.** `NoDoc` has no docstring and its first statement is `def method`. Where
does its slot point, and what does that imply about edit ordering?

**10.** A docstring whose closing quotes are on their own line needs a different
offset from `"""Summary."""`. What goes wrong if you treat them the same, and
which call tells them apart?

**11.** `"""Example:"""` plus an injected key reads differently in Python and in
Rust. Which one finds the ID, why, and which reader must injection use?

---

## Answers

**1.** `at` is where the text goes; `stmt_start` is where `body[0]` begins. `at`
alone cannot tell you the indentation to write at, nor whether `body[0]` shares
a line with the `def` — and for an `InLiteral` slot `at` is in the middle of a
line, so it cannot answer either question. `stmt_start` is the anchor for both
(object 3's `line_prefix`).

**2.** They differ on exactly one input: an unclosed string, where the closer is
zero bytes. `quote_len()` would put `at` three bytes inside the content of a
literal that has no closing quotes — in a file that does not parse, which is
the file you least want to rewrite.

**3.** The **last** part — it is the one holding the closing quotes. Measuring
from the first part puts `at` before the second part's opener, so the insert
lands between the two literals, in the gap, and produces either a syntax error
or a third silently-concatenated string. `shapes.py:implicit_concat` is the
fixture.

**4.** Because Python forbids a literal from ending in an odd number of
backslashes — the final backslash would escape the closing quote. The input you
would fear is `r"""abc\"""`, and Python rejects it with *unterminated
triple-quoted string literal*. The legal version is `"""abc\\"""`, where the
pair is already an escaped backslash and inserting after it is harmless.

**5.** Because a newline inside a single-quoted string is a syntax error, so
that shape must append ` ID: u` on the same line rather than `\n\nID: u`. The
offset says *where*; the flag says *what*.

**6.** Because the reader builds a dict and the **last** pair for a key wins.
Appending makes your key the winner even when the prose already says `ID:`
somewhere. `idempotent.py:prose_mentions_id` — whose reader value is
`'nothing-real,'` — is where inserting at the front would hand the graph a
garbage ID.

**7.** In front of the **opener**, at the other end of the literal. `at` is
derived from the closer, so the prefix is never in the edited region. The Python
driver loses it because it rebuilds the whole literal as
`f'"""{content}"""'`; insertion never rebuilds anything.

**8.** An empty body (`parse_body` returns `Suite::new()` on error, so `def f():`
at EOF has no statements) and an unclosed docstring. The caller skips the def:
no edit, no ID this run, `id: null` in the outline, and a correct ID as soon as
the file parses. Both are quirk 13 — swallow it, do not panic, and never rewrite
a file you could not read.

**9.** At the start of `def method`, since that is `body[0]`. The nested def's
own slot is at a higher offset, inside its body — so applying edits in
descending offset order writes the inner one first, and the outer insert then
shifts text that is already final. The two can never collide because a child's
offsets are always strictly greater than its parent's slot.

**10.** The multi-line form's `at` sits *after* the whitespace that indents the
closer, so inserting `{nl}{nl}{ind}ID: u{nl}{ind}` there strands that whitespace
alone on a line — trailing whitespace in the user's file, which every linter
flags. `line_prefix(source, slot.at)` tells them apart: `Some(before)` means the
closer owns its line, so back the offset up by `TextSize::of(before)` and insert
`{nl}{ind}ID: u{nl}`; `None` means the closer follows text and the original
shape is right.

**11.** **Rust** finds it. The Python regex is consuming: `(\S+)\s*:\s*(\S+)`
matches `Example` as a key, `\s*` crosses both newlines, and the value it takes
is the literal run `ID:` — so the scan ends past the real key and never sees it.
Rust iterates each `:` independently and skips a pair whose value contains a
colon. Injection must use the **Rust** reader, because that is the one that keeps
it idempotent against its own output; the Python side's blindness is a join bug
to fix with one shared, anchored grammar, not something to reproduce.

---

**Next:** [`03-object-edit.md`](03-object-edit.md) — plan the edits, then apply
them.
