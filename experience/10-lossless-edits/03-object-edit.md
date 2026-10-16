# Object 3 — `Edit`, `plan_ids`, `apply`

Plan every insert first, apply them second. The split is not tidiness; it is
what makes the borrow checker let you write the file at all.

---

## What it is

```rust
use ruff_text_size::TextSize;

/// One insertion. Owned, so it outlives the AST it was derived from.
pub struct Edit {
    pub offset: TextSize,
    pub text: String,
}

/// Plan an `ID:` insert for every def below `body` that has no key yet.
///
/// `new_id` is a parameter so tests can pass a counter. Nothing in here
/// touches the filesystem or the database.
pub fn plan_ids(
    body: &[Stmt],
    source: &str,
    new_id: &mut impl FnMut() -> String,
) -> Vec<Edit> {
    let mut edits = Vec::new();
    for d in child_defs(body) {                       // ★ object 1: the shared rule
        if id_from_body(d.body()).is_none() {         // ★ object 1: the shared reader
            if let Some(edit) = id_edit(d, source, new_id) {
                edits.push(edit);
            }
        }
        edits.extend(plan_ids(d.body(), source, new_id));
    }
    edits
}

fn apply(source: &str, mut edits: Vec<Edit>) -> String {
    edits.sort_by_key(|e| std::cmp::Reverse(e.offset));   // ★ bottom up
    let mut out = source.to_string();
    for e in edits {
        out.insert_str(e.offset.to_usize(), &e.text);
    }
    out
}
```

`id_edit` is object 2's slot plus the text to put in it:

```rust
fn id_edit(
    d: Def<'_>,
    source: &str,
    nl: &str,                              // ★ the file's line ending — see below
    new_id: &mut impl FnMut() -> String,
) -> Option<Edit> {
    let slot = doc_slot(d.body())?;        // None: empty body or unclosed string
    let uuid = new_id();
    let indent = line_prefix(source, slot.stmt_start);

    let (offset, text) = match (slot.kind, indent) {
        // a docstring exists: extend it in place
        (SlotKind::InLiteral { triple_quoted: true }, Some(ind)) => {
            match line_prefix(source, slot.at) {
                // ★ the closer starts its own line: insert BEFORE its indent,
                //   or that indent is left stranded on a line of its own
                Some(before) => (
                    slot.at - TextSize::of(before),
                    format!("{nl}{ind}ID: {uuid}{nl}"),
                ),
                // the closer follows text, as in `"""Summary."""`
                None => (slot.at, format!("{nl}{nl}{ind}ID: {uuid}{nl}{ind}")),
            }
        }
        (SlotKind::InLiteral { triple_quoted: false }, _) =>
            (slot.at, format!(" ID: {uuid}")),           // a newline would not parse
        (SlotKind::InLiteral { triple_quoted: true }, None) =>
            (slot.at, format!(" ID: {uuid}")),           // `def f(): """doc"""`

        // none exists: a whole new statement before body[0]
        (SlotKind::NewStatement, Some(ind)) =>
            (slot.at, format!("\"\"\"ID: {uuid}\"\"\"{nl}{ind}")),
        (SlotKind::NewStatement, None) => {
            let ind = one_liner_indent(source, d);       // parent indent + one unit
            (slot.at, format!("{nl}{ind}\"\"\"ID: {uuid}\"\"\"{nl}{ind}"))
        }
    };

    Some(Edit { offset, text })
}

/// The whitespace between `at`'s line start and `at` — the indentation to reuse
/// **verbatim**, tabs and all. `None` when `at` is not the first thing on its
/// line, which is the `def f(): body` one-liner.
fn line_prefix(source: &str, at: TextSize) -> Option<&str> {
    let at = at.to_usize();
    let line_start = source[..at].rfind('\n').map_or(0, |nl| nl + 1);
    let prefix = &source[line_start..at];
    prefix.bytes().all(|b| b == b' ' || b == b'\t').then_some(prefix)
}
```

### ★ `line_prefix` answers both hard questions at once

"What do I indent with?" and "is this a one-liner?" are the same lookup:

- **`Some(prefix)`** — `body[0]` begins its line, and `prefix` is the exact
  bytes to reuse. Reusing them **verbatim** gets `tabs.py` and any mixed-indent
  file right with no style detection at all. `ruff_python_codegen::Stylist`
  exists for this and is not needed: the file already told you.
- **`None`** — something precedes `body[0]` on its line, so the insert has to
  open a new line. This is the only case that must *synthesise* an indent, and
  the only place `Stylist::indentation()` would earn a dependency.

And it is why object 2 carries `stmt_start` separately from `at`: for an
`InLiteral` slot, `at` is mid-line (just before the closer), so asking
`line_prefix(source, at)` would report the docstring's *last* line, not the
statement's indentation.

### ⚠ …and asking it about `at` as well answers a third one

`line_prefix(source, slot.at)` is not a mistake — it is the discriminator for
the two triple-quoted shapes, which need **different offsets**:

```python
def one_line():
    """Summary."""          # line_prefix(at) == None  → closer follows text

def many_lines():
    """Summary.

    Details.
    """                      # line_prefix(at) == Some("    ") → closer owns its line
```

For `many_lines`, `at` sits *after* the four spaces that indent the closer.
Inserting `{nl}{nl}    ID: u{nl}    ` there leaves those four spaces alone on a
line, followed by a blank one:

```
    Details.
␣␣␣␣                    ← trailing whitespace. every linter flags this.

    ID: u
    """
```

So when the closer owns its line, back the offset up over that indent
(`slot.at - TextSize::of(before)`) and insert `{nl}{ind}ID: u{nl}` instead. The
result is `Details.`, one blank line, the key, then the closer — which is the
shape a human would have written:

```
    Details.

    ID: u
    """
```

The one-line shape keeps the original text, because there `at` is mid-line and
there is no stranded indent to avoid.

### ⚠ `nl`, because the inserted text is not inherited

`line_prefix` inherits the file's *indentation* — it hands back real bytes. It
does **not** inherit the file's line ending, because the newlines in
`format!("\n\n…")` are ones you wrote. On `crlf.py` that gives every injected
def LF endings inside an otherwise CRLF file: Python accepts it, `git diff`
shows `^M` noise, and the user's editor fights them over it.

Detect once per file and thread it through:

```rust
use ruff_source_file::find_newline;

// ruff_source_file/src/newlines.rs:58, :300, :316   [verified]
//   pub fn find_newline(text: &str) -> Option<(usize, LineEnding)>
//   LineEnding::{Lf, Cr, CrLf}.as_str() -> "\n" | "\r" | "\r\n"
let nl = find_newline(source).map_or("\n", |(_, ending)| ending.as_str());
```

Prefer that to `source.contains("\r\n")`: it reports the ending of the **first**
line rather than the presence of any CRLF anywhere, and it also recognises a
bare `\r` file, which the `contains` check silently treats as LF. A file with no
newline at all has no ending to inherit, so `\n` is the fallback.

---

## ⚠ One-line bodies: `minimal.py` and three of `shapes.py`

```python
def stub(): ...
```

`body[0]` is on the same line as `def`, so inserting a docstring statement
before it with no newline produces `def stub(): """ID: u"""` and leaves `...`
orphaned on an indented line below — a syntax error. Open a line first and the
original statement rides along behind the new docstring:

```python
def stub(): 
    """ID: u"""
    ...
```

Measured on CPython 3.12.2: `__doc__` is `'ID: u'`, and the function still
works. `semicolon_body` behaves the same way — `a = 1; return a` moves down as
one compound line, and the function still returns `1`.

The trailing space after the colon is cosmetic. A formatter removes it; the
alternative is a second edit that deletes it, which doubles the number of ways
this can go wrong for no behavioural gain.

**Fixtures:** `minimal.py:stub`, `shapes.py:{one_liner, ellipsis_body,
semicolon_body}`, `no_docstring.py:delta`.

---

## ★★ Bottom-up, and why

An insert shifts every byte after it. `apply` therefore sorts **descending** and
inserts from the end of the file backwards, so every offset it has not reached
yet is still the offset it was planned at. No running total, no fixups.

```rust
edits.sort_by_key(|e| std::cmp::Reverse(e.offset));
```

Two properties fall out of that, and both are worth being able to state:

**Children before parents, for free.** A def's slot is at the top of its body,
strictly before any def nested inside it. Descending offsets therefore reach the
inner def first; the parent's insert later shifts text that is already final —
which is fine, because by then it is bytes, not ranges.

**No two edits can share an offset.** Every offset lies inside a distinct def's
body, after that def's own header. A parent's slot is before its child's header;
a sibling's is in a different body entirely. So the sort order is total and
`sort_by_key`'s stability never matters.

```rust
debug_assert!(
    edits.windows(2).all(|w| w[0].offset != w[1].offset),
    "two edits at one offset: the shared rule produced a def twice",
);
```

That assertion is cheap and it catches exactly one bug — object 1's def arm
calling `walk_stmt` as well as pushing, which yields every nested def twice.

### The alternative: a builder, and no ordering rule at all

Descending order is a consequence of **mutating in place**. Build the output
instead and the rule evaporates:

```rust
fn apply(source: &str, mut edits: Vec<Edit>) -> String {
    edits.sort_by_key(|e| e.offset);              // ascending
    let mut out = String::with_capacity(source.len() + 64 * edits.len());
    let mut last = 0;
    for e in edits {
        let at = e.offset.to_usize();
        out.push_str(&source[last..at]);          // the source never changes
        out.push_str(&e.text);
        last = at;
    }
    out.push_str(&source[last..]);
    out
}
```

| | in-place, descending | builder, ascending |
|---|---|---|
| offsets stay valid because… | you never touch text below the cursor | **you never touch the source** |
| cost | one memmove of the tail per edit | one pass, one allocation |
| ordering rule | load-bearing | an implementation detail of the loop |
| reads as | "be careful" | "copy, insert, copy" |

Both are correct. The builder is O(n) instead of O(n·edits) and has one fewer
invariant to remember, which is the better trade on a corpus where a module can
hold a hundred defs. Know the descending rule anyway: the moment you edit a
`String` in place — in a test, in a fixup script — it is the rule that keeps you
out of trouble.

---

## ★ `new_id` is a parameter, and that is a test-design decision

```rust
pub fn plan_ids(…, new_id: &mut impl FnMut() -> String) -> Vec<Edit>
```

Production passes a UUID generator. Tests pass a counter, so the output is
identical on every run and can be snapshotted:

```rust
let mut n = 0;
let edits = plan_ids(body, source, &mut || { n += 1; format!("id-{n}") });
```

Without this, every fixture test is unreproducible: `insta` snapshots fail on
the second run, a byte-comparison against a golden is impossible, and a diff of
"what did injection do to `shapes.py`" is 36 random hex digits of noise per def.
With it, `shapes.py` after injection is a file you can commit and review as a
`git diff`.

It is also the seam where the **randomness** lives — which matters for object 4:
`plan_ids` is pure *given* `new_id`, so everything impure about injection is one
closure and one `fs::write`.

**Rust note.** `&mut impl FnMut() -> String` is callable directly as `new_id()`,
because `&mut F` implements `FnMut` when `F` does. Recursing with
`plan_ids(d.body(), source, new_id)` reborrows the `&mut`, which is why the
parameter is a reference rather than `impl FnMut` by value — a by-value closure
would be moved into the first recursive call.

---

## Worked example — `shapes.py`, with a counter

```
$ pylspt-dev inject --dry-run --ids=counter python/shapes.py
 13 edits planned, 13 defs, 0 already keyed

offset  kind                       text
  178   InLiteral{triple:true}     "\n\n    ID: id-1\n    "
  271   InLiteral{triple:true}     "\n\n    ID: id-2\n    "
  …
  612   NewStatement               "\"\"\"ID: id-7\"\"\"\n    "
  651   NewStatement               "\n    \"\"\"ID: id-8\"\"\"\n    "
```

Then the check that matters:

```
$ pylspt-dev inject --ids=counter --out /tmp/shapes.py python/shapes.py
$ python3 -c "import sys; sys.path.insert(0,'/tmp'); import shapes; print(shapes.triple_multi.__doc__)"
Several lines of prose.

    The key joins the end of this block, at this indentation.

    ID: id-1
```

**Every def keyed, and the file still imports.** That pair — reads back, and
still parses — is the whole contract of this object.

---

## Exercise

**A.** Plan the edits for `shapes.py` with a counter and print them as the table
above. Thirteen rows — eleven defs at top level, two nested in `NoDoc`.

**B.** Apply them to a copy and `python3 -c "import shapes"`. Then
`diff` original against output: every hunk must be one added key and nothing
else.

**C.** Sort **ascending** and apply in place anyway. Measure how far off the
second insert lands and relate the number to the length of the first inserted
string.

**D.** Write both `apply`s — in-place descending and the builder — and assert
they produce identical output on all ten fixtures. Then time them on a file
with 200 defs.

**E.** Trigger the `debug_assert`: make object 1's def arm call `walk_stmt` as
well as pushing, and run on `shared_rule.py:outer`.

**F.** Swap the counter for a real UUID generator and run the fixture test
twice. Then put the counter back. This is the test-reproducibility argument,
felt rather than read.

**G.** `tabs.py` and `crlf.py`: inject into both and check the inserted bytes
with `cat -A`. The tab file must gain a tab, not four spaces. What does
`line_prefix` do about `\r`, and does `crlf.py` need anything extra?

**H.** Delete the `if let Some(edit)` guard in `plan_ids` so `id_edit` can
`unwrap`. Feed it `def f():` with nothing after it.

---

## Exam

**1.** Why does `plan_ids` return edits instead of applying them?

**2.** Why descending order? State the invariant it preserves in one sentence.

**3.** What does descending order give you for free about a def and the defs
nested inside it?

**4.** Can two edits share an offset? Prove it either way, and say what the
`debug_assert` is really testing.

**5.** The builder version needs no ordering rule. Why not — and why learn the
rule anyway?

**6.** `new_id` is a parameter rather than a call to a UUID crate. Give the
testing reason and the purity reason.

**7.** `line_prefix` returns `Option<&str>`. What are the two cases, and what
does each one mean for the inserted text?

**8.** Why does `id_edit` need `stmt_start` rather than using `slot.at` for the
indentation lookup?

**9.** `def stub(): ...` — what exactly does the insert add, and why does the
original statement end up where it does?

**10.** `Edit` holds a `String`, not a `&str`. Why does it have to?

---

## Answers

**1.** Because applying needs `&mut db` later (object 4) and planning holds
`&'a Stmt` borrowed from the parse. `Vec<Edit>` is **owned** — `TextSize` plus
`String` — so it survives the AST borrow ending. Returning edits is what lets
the write happen at all; the split is forced by the types, not chosen for
style.

**2.** Because an insert shifts every byte after it. Going from the end
backwards means every offset not yet reached is still the offset it was planned
at.

**3.** Children are written before parents, with no extra logic: a def's slot is
at the top of its body, strictly before any nested def, so the nested def's
offset is always larger and is reached first.

**4.** No. Every offset is inside a distinct def's body and after that def's own
header, so a parent's offset is strictly less than its child's, and siblings are
in different bodies. The `debug_assert` therefore is not testing the sort — it
is testing **object 1**, because the only way to get two edits at one offset is
for `child_defs` to yield one def twice.

**5.** Because the builder never mutates the source: it copies spans out of an
immutable `&str`, so no offset is ever stale. Learn the descending rule anyway —
any in-place edit, in a test or a one-off script, needs it, and it is the rule
that makes the two-phase plan/apply split obviously correct.

**6.** Testing: a counter makes fixture output byte-identical across runs, so it
can be snapshotted, committed and reviewed as a diff. Purity: it puts all the
nondeterminism in one closure, so `plan_ids` is pure *given* `new_id` and the
only impure things left in injection are that closure and one `fs::write` —
which is what keeps the pass auditable (object 4).

**7.** `Some(prefix)`: `body[0]` starts its line, and `prefix` is the exact
indentation bytes to reuse verbatim — tabs, spaces, width, all inherited from
the file. `None`: something precedes `body[0]` on its line, so this is a
one-liner and the inserted text must begin with a newline and a synthesised
indent.

**8.** Because for an `InLiteral` slot `at` sits mid-line, immediately before the
closing quotes. `line_prefix(source, at)` would measure from the start of the
docstring's **last** line and would also report `None` whenever that line has
content — which is almost always. `stmt_start` is the statement's own start, so
it answers both the indentation question and the one-liner question correctly.

**9.** `"\n{indent}\"\"\"ID: u\"\"\"\n{indent}"` at the offset of `...`. The
leading newline closes the header line, the docstring goes on its own line, and
the trailing newline-plus-indent re-opens a line for the original statement,
which is still sitting at that offset and therefore follows. Verified: the
docstring reads back and the function still runs.

**10.** Because the text is *built*, not found: `format!` produces a new
`String` that no span of the source contains. And because `Edit` must outlive
the parse — a `&str` borrowed from the source would be fine, but one borrowed
from the AST would not, and the generated UUID belongs to neither.

---

**Next:** [`04-object-purity.md`](04-object-purity.md) — why none of this can
live inside a salsa query.
