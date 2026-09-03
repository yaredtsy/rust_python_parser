# 10 — Lossless edits and ID injection

**Goal:** you can inject a UUID into every def and class in a file, byte-identical
to what the Python driver produces, and running it twice changes nothing the
second time.

This is the port of `id_injector.py` (185 lines) and `file_folder_ids.py` (82).
It is the only part of the driver that **writes to the user's source files**, so
it is the part where a bug is expensive.

---

## Read first

- `plan/02-mapping/02-id-injection.md` — the whole chapter
- `plan/00-orientation/01` §"Side effects" — three RPCs write to disk

---

## The mental model

### Why ruff's AST cannot do this

You learned in exercise 02 that ruff's AST is abstract: no comments, no
whitespace, `x=1` and `x = 1` are identical. That is perfect for reading
structure and useless for rewriting a file, because printing the AST back
produces *a* valid file, not *the user's* file with one docstring changed.

So you need a CST. Ruff itself hits this exact wall and solves it the same way:

```toml
# ruff/Cargo.toml:132   [verified]
libcst = { version = "1.8.4", default-features = false }
```

used by `ruff_linter/src/fix/codemods.rs` **[verified]** for precisely the same
reason. The Rust crate's module name is `libcst_native`, and it is the same
library your Python already uses — so this port is closer to a translation than
a rewrite.

**Keep `default-features = false`.** The default features build a PyO3 Python
extension module, which you do not want.

### The design that keeps libcst off the hot path

The one real objection to libcst is parsing each file twice. Split **detection**
from **modification** and it disappears:

```
parse_file(path, content)
   │
   ├─ ruff AST (already parsed, salsa-cached)
   │     └─ does every def/class already have an `ID:`?
   │            │
   │       yes ─┴──►  done. no libcst, no write, no second parse.   ← ~99% of calls
   │
   └─ no ──►  libcst: parse, inject, codegen, write, sync db        ← cold path
```

Detection is **free** — you already walked the AST to build your node tree, and
the `ID:` check is the same lookup that fills in `BaseNode.id`. Injection only
runs on files that are new or newly edited, which after the first pass over a
project is close to never.

This also fixes the "three parsers per file" problem from
`plan/00-orientation/02`: today `parse_file` always runs libcst, always runs
parso, and sometimes runs parso again inside Jedi.

### ⚠ The bug that does not exist in Python

```
1. compute the new content        (libcst)
2. write it to disk
3. File::sync_path(&mut db, &path)     ← DO NOT SKIP
```

Skip step 3 and every later query serves the pre-injection source. Your node IDs
stay `None` forever, injection appears to do nothing, and the file on disk is
visibly correct. You met this in exercise 03's answer 8; this is where it
actually bites.

Because `&mut db` cancels in-flight queries on other threads, **batch it**:
collect all injections for a request, write them all, sync once.

---

## The API

```toml
# add to Cargo.toml
libcst = { version = "1.8.4", default-features = false }
ruff_python_codegen = { git = "https://github.com/astral-sh/ruff", rev = "ac201b8" }
```

`libcst` comes from crates.io — it is a normal published crate, unlike the
`ty_*` ones. Same version ruff pins, so if you ever end up in a shared graph
there is one copy.

```rust
use libcst_native::{parse_module, Codegen, CodegenState};

let module = libcst_native::parse_module(&source, None)   // None = UTF-8
    .map_err(|e| /* log and give up — never crash */)?;

// walk and mutate the tree directly; there is no CSTTransformer in the Rust crate
// (your Python's leave_ClassDef / leave_FunctionDef have no direct equivalent)

let mut state = CodegenState {
    default_newline: stylist.line_ending().as_str(),
    default_indent: stylist.indentation(),
    ..Default::default()
};
module.codegen(&mut state);
let new_source = state.to_string();
```

```rust
// ruff_python_codegen  [verified]
impl Stylist<'a> {
    pub fn from_tokens(tokens: &Tokens, source: &'a str) -> Self;
    pub fn line_ending(&self) -> LineEnding;
    pub fn indentation(&self) -> &Indentation;
}
```

> ⚠ **`default_newline` and `default_indent` apply only to nodes you create.**
> Existing nodes keep their own whitespace exactly. Since you are inserting a
> brand-new docstring statement, these two settings decide how it looks — and
> getting them from a constant instead of from the file is how a CRLF file ends
> up with one LF line in it, failing the user's next formatter check.
>
> `Stylist::from_tokens` needs the tokens, which you have from
> `parsed_module(...).tokens()`. This is the glue `codemods.rs` uses
> **[verified]**; copy it.

---

## The fixtures

```
python/
├── no_docstring.py ..... nothing has a docstring. every def needs one created.
├── has_docstring.py .... existing docstrings: with ID, without ID, raw-prefixed,
│                         containing """, single-quoted, class + method
├── crlf.py ............. CRLF line endings
├── tabs.py ............. tab indentation
├── nonascii.py ......... accented identifiers, emoji in a docstring, coding line
└── minimal.py .......... `pass`-only bodies, `...` bodies, one-line defs
```

`minimal.py` is the one that breaks naive implementations: `def stub(): ...` has
its body on the same line as the header, so "insert a line after the colon"
produces a syntax error.

---

## Build it

### Step 1 — detection first

Before touching libcst, write the fast path. For a file, report:

```
no_docstring.py    6 defs/classes, 6 missing IDs   → would inject
has_docstring.py   7 defs/classes, 5 missing IDs   → would inject
(after injection)  7 defs/classes, 0 missing IDs   → fast path, libcst not called
```

Add a **counter** for libcst invocations. You will assert on it in step 6.

### Step 2 — look at a CST

Parse `minimal.py` with `libcst_native::parse_module` and debug-print the tree.
Find the `FunctionDef` for `stub` and look at how its body is represented
compared to `only_pass`.

The plan marks the exact enum and field names `[check]` —
`Statement`, `CompoundStatement`, `FunctionDef.body`, `Suite`. **Check them,**
because libcst's Rust API is not identical to the Python one and the shapes are
where your time will go. `cargo doc -p libcst --open` is the fastest route.

### Step 3 — inject into one function

Get `no_docstring.py`'s `alpha` to gain:

```python
def alpha(x):
    """ID: <uuid>"""
    return x + 1
```

Then codegen the whole module and diff against the original. **Only that one
docstring line should differ.** If anything else moved — a blank line, an indent,
a quote style — your `CodegenState` is wrong or you rebuilt a node you should
have left alone.

### Step 4 — the two pre-existing bugs, reproduced on purpose

`plan/02-mapping/02` is explicit: reproduce them, do not fix them. Mark each
with `// PARITY:` so the choice is visible in the code.

1. **Prefix loss.** `id_injector.py:70` emits `f'"""{content}"""'`
   unconditionally, dropping an `r` prefix. So `raw_prefixed` in
   `has_docstring.py` loses its `r`, and `C:\new\table` changes meaning —
   `\n` and `\t` become real escapes.
2. **`"""` inside content.** `contains_triple_quotes` rebuilds into a broken
   literal.

Both are two-line fixes whenever someone decides they want them. Your job today
is byte-identical output, and that includes identical bugs.

If that feels wrong: the reasoning is in `MEMORY.md` — the contract is the JSON
and the file bytes, not the code. A port that "improves" behaviour is a port
whose diffs cannot be verified.

### Step 5 — the hard fixtures

Run injection over all six files and diff each against the Python driver's
output (or, if you do not have it running, against hand-checked expectations).

| fixture | what it tests |
|---|---|
| `minimal.py` | one-line bodies (`def stub(): ...`), `pass`, `...` |
| `crlf.py` | inserted line uses `\r\n`, not `\n` |
| `tabs.py` | inserted line uses a tab, not four spaces |
| `nonascii.py` | the coding line survives; the emoji docstring is untouched |
| `has_docstring.py` | key added to existing prose without destroying it |
| `no_docstring.py` | docstring created from nothing, at the right indent |

### Step 6 — idempotency and the db sync

Two properties, both testable, both load-bearing:

**Idempotent.** Run twice. The second run makes zero edits and returns
`modified: false`. Without this, files grow an ID block per request — which is
the worst possible failure for something that writes to source files.

**Detection is correct.** After the first run, assert your libcst counter does
**not** increase on the second. This is the test that protects your performance;
without it, a regression that makes libcst run on every file shows up only as
"it got slow", with no obvious cause.

**The sync.** Inject, write, `File::sync_path(&mut db, &path)`, then query
`parse_file` again **on the same db** and confirm the IDs are visible. Then
comment out the sync and watch the test fail. Do this once, deliberately — it is
the cheapest way to make that bug memorable.

### Step 7 — file and folder IDs

Same machinery at module level (`file_folder_ids.py`):

- module docstring = first statement of `module.body`
- keys are `FileID` / `FolderID`; results are prefixed `FileSchema/` / `FolderSchema/`
- `read_or_inject_folder_id` **creates `__init__.py` if absent** — preserve it,
  it is load-bearing for v-noc's folder identity
- on any error, the Python returns a **fresh random UUID with `modified=false`**
  (`file_folder_ids.py:22`) — a throwaway that is never persisted. Odd.
  Reproduce it.

### Step 8 — the dry-run guard

`plan/04-build/00-dev-cli.md` is emphatic and it is right:

> ⚠ **`--write` defaults off.** `parse_file` injects UUIDs into source files as
> a side effect. Running the CLI over a corpus would silently rewrite the user's
> code.

Implement dry-run as the default. Report what *would* change via
`modified: true`. Then run your CLI over a large corpus with confidence.

---

## Traps

- **Skipping `File::sync_path`.** The headline bug. Nothing errors.
- **Hard-coding `\n` and four spaces.** Use `Stylist`.
- **Rebuilding nodes you did not change.** libcst preserves existing formatting
  *because* you leave those nodes alone. Reconstructing one loses its trivia.
- **Assuming a body is a block.** `def stub(): ...` is not.
- **Fixing the two bugs.** Tempting, wrong today, and it invalidates every diff
  you are about to run.
- **Running injection during a read-only request without the guard.** Your
  `parse_file` writes to disk by design; make that explicit in the code so
  nobody is surprised by it later.

---

## Done when

- [ ] detection reports missing IDs without invoking libcst
- [ ] injection produces a one-line diff on `no_docstring.py`
- [ ] all six fixtures round-trip with only the intended change
- [ ] both pre-existing bugs are reproduced and marked `// PARITY:`
- [ ] second run makes zero edits and does not invoke libcst
- [ ] you have seen the missing-`sync_path` failure with your own eyes
- [ ] file/folder ID injection works, including creating `__init__.py`
- [ ] `--write` defaults to off

---

→ [`exam.md`](exam.md), then [`../11-capstone/README.md`](../11-capstone/README.md)
