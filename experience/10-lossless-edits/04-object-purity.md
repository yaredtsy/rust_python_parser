# Object 4 — purity, and the four-step order

Why injection is not a salsa query, and the sequence that keeps a stale range
from ever reaching the graph.

---

## ★★ Four reasons injection cannot be a tracked query

### 1. Tracked queries must be pure

Same input, same output, no effects outside themselves. Injection breaks both
halves: it **writes files**, and it mints **random** UUIDs, so two runs over one
unchanged file disagree.

Inside `definitions` that is not merely untidy. Salsa decides whether anything
downstream must re-run by comparing the old value to the new one, and the
default comparison is literally `old_value == new_value`
(`salsa-macros/src/tracked_fn.rs:103-115` **[verified]**). A random result is
therefore *never* equal:

- every re-read of the file invalidates every query beneath it, forever;
- the same def can be handed **two different IDs** on two reads of the same
  file, and the one the graph gets depends on which read happened last;
- `#[derive(PartialEq)]` on `DefNode`, which is the thing that makes backdating
  possible at all, becomes dead weight.

### 2. A query can be cancelled half-way through

Salsa cancels in-flight queries when another thread takes `&mut db`, and it does
it by **unwinding**: `Cancelled::PendingWrite` — "the query was operating on
revision R, but there is a pending write to move to revision R+1" — is thrown
with `panic::resume_unwind` (`salsa/src/cancelled.rs:12-29`, `storage.rs:160`
`cancel_others` **[verified]**).

A pure query loses nothing when that happens; it is simply re-run. A query that
was part-way through `fs::write` loses the user's file. There is no way to make
a file write safe against being unwound at an arbitrary point, so the write must
live somewhere cancellation cannot reach it — outside the query graph, on the
thread that owns `&mut db`.

### 3. It needs data the outline does not

To insert you need the closing quotes' offset, the quote style, the body's
indentation anchor, and whether the body shares a line with the `def` (objects 2
and 3). The outline needs none of it.

Putting it in `DefNode` makes every cached result bigger **and** makes every
`old == new` compare fields nobody else reads — so a reformatted docstring that
leaves the outline identical would still invalidate it. The fields that are
expensive to carry are exactly the fields that change most often.

### 4. It runs at a different time

| | collection | injection |
|---|---|---|
| when | every read of the outline | once per file, only when a key is missing |
| how often, after the first pass | constantly | ~never |
| effect on ranges | none | **shifts every range after the insert** |

That last row is the scheduling constraint: injection has to finish *before* you
compute the outline you send to the graph.

### What a second pass does **not** cost

A second parse. Both passes read the same salsa-cached `parsed_module`
(exercise 02, object 1), and a statement-only walk over an AST already in memory
is cheap — object 1's `child_defs` touches 8 of 25 `Stmt` variants and never
enters an expression.

So the shape is: **one cached parse, two cheap walks, one plain function that
writes.**

---

## The four-step order

```
1. parse the file                       (cached — parsed_module)
2. plan_ids(…, real_uuid)               (pure given new_id; borrows the AST)
3. if !edits.is_empty():
       apply → fs::write → File::sync(&mut db)
4. definitions(db, file)                (salsa re-walks the NEW text)
```

Step 4 is the point. **The outline you send to the graph is always computed from
the text after injection**, so a pre-write range cannot reach the graph — not
because you were careful, but because there is no code path on which it could.

```rust
pub fn parse_file(db: &mut ProjectDatabase, file: File, path: &SystemPath) -> anyhow::Result<Vec<DefNode>> {
    // 2. plan — this borrow ends before step 3 begins
    let edits = {
        let source = source_text(db, file);
        let parsed = parsed_module(db, db.program_file(file).python_file(db));
        let module = parsed.load(db);
        plan_ids(&module.syntax().body, &source, &mut || Uuid::new_v4().to_string())
    };

    // 3. write, then tell the db
    if !edits.is_empty() {
        let source = source_text(db, file);
        let new_source = apply(&source, edits);
        drop(source);
        std::fs::write(path.as_std_path(), &new_source)?;
        File::sync_path(db, path);                  // ★ needs &mut: see below
    }

    // 4. collect from the new text
    Ok(definitions(db, file).clone())
}
```

### ⚠ The bug that does not exist in Python

```
 write the file  →  …and do not tell the db
```

Skip `File::sync_path` and every later query serves the **pre-injection**
source. The IDs stay `None` forever, injection appears to do nothing, and the
file on disk is visibly, provably correct. Nothing errors. You met this in
exercise 03's answer 8; this is where it actually bites.

```rust
// ruff_db/src/files.rs:456   [verified]
pub fn sync_path(db: &mut dyn Db, path: &SystemPath);
// ruff_db/src/files.rs:478   [verified]
pub fn sync(self, db: &mut dyn Db);
```

Either works: `sync_path` takes the path and also invalidates the containing
directory listing; `sync` takes the `File` you already have and does not. Use
`sync` when the file definitely exists already — which it does, you just wrote
it — and `sync_path` when you may have *created* it (`__init__.py`, object 5).

**See the failure once, deliberately.** Comment the sync out, run injection,
then re-query and watch `id: null` come back from a file whose text on disk
contains the key. It is the cheapest way to make that bug memorable.

### ★ The borrow checker enforces step 4

```rust
let defs = definitions(db, file);     // &Vec<DefNode> — borrows db immutably
…
File::sync_path(db, path);            // ← needs &mut: the shared borrow must be dead
```

`definitions` is `#[salsa::tracked(returns(ref))]`, so its result borrows the
database; `sync` and `sync_path` take `&mut dyn Db`. **Holding an outline across
the write does not compile.** The stale-range rule is a type error, not a
convention.

The loophole to *not* take is cloning the outline out first. That compiles, and
the clone's ranges are stale. If you need something across the write, clone the
**names**; never the ranges.

And note this is the same force that shaped object 3: `plan_ids` has to return
owned `Edit`s for exactly the same reason, because its `&'a Stmt` borrows die at
the write too.

### ⚠ Batch the writes, sync once

`&mut db` **cancels every in-flight query on every other thread** (reason 2
above). One sync per file turns a 200-file first pass into 200 cancellation
storms.

```
collect all the files' edits  →  write them all  →  sync once  →  collect
```

`File::sync_all_recursive(db, paths)` (`files.rs:273` **[verified]**) exists for
the many-paths case. The rule from `plan/02-mapping/02` stands: one request, one
write phase, one sync.

---

## ⚠ The guards

Injection writes to the user's source files. Two guards, both from
`plan/04-build/00-dev-cli.md`, and neither is optional:

**`--write` defaults off.** Dry-run is the default; report what *would* change
through `modified: true`. Without it, running the CLI over a corpus to check
parity silently rewrites the corpus — including the goldens you were about to
compare against.

**Never write outside the project.** Quirk 2 is that only first-party code is
descended into; the same boundary has to gate the writes, or a `parse_file` on a
file in `site-packages` injects UUIDs into somebody's installed package.
`ty_project`'s own predicate is the one to use —
`project.is_file_included(db, path)` — and the check belongs *in* the injector,
not at the call site, because the injector is the thing that must never be
wrong about it.

**⚠ Write only `.py`.** This is the guard that is easiest to miss and worst to
get wrong, because the step that looks innocent is `fs::write(path,
new_source)`. That is only correct when `source_text` *is* the file's bytes —
and for a notebook it is not:

```rust
// ruff_db/src/source.rs:25-36   [verified]
let kind = if is_notebook(db.system(), path) {
    file.read_to_notebook(db)….into()      // ← the JOINED CODE CELLS
} else {
    file.read_to_string(db)….into()
};
```

So on an `.ipynb`, stage 2 would read the concatenated cell sources, insert a
key, and write **Python source over the user's notebook JSON**, destroying every
cell, output and piece of metadata in it. `PySourceType::Ipynb` is detected by
extension or by the system's `source_type`, so the check is cheap — make it
before planning, not before writing, so no UUID is minted for a file you will
not touch.

`.pyi` is the other extension to decide about *on purpose*: stubs parse fine and
have docstrings, so injection will happily key them, and you probably do not
want graph nodes for declarations with no bodies. Pick one and write it down.

**Refuse to write a file that does not parse.** Objects 2 and 3 already return
`None` per def for an unclosed string or an empty body, but a file with a syntax
error earlier in it can still yield plausible slots later, and rewriting a file
the user is mid-way through typing is the one failure they will never forgive.
`parse_unchecked` gives you the errors; if there are any, plan nothing.

> **And the fourth input that is not a file.** `parse_file(path, content, …)`
> takes client-supplied text — unsaved editor buffers. `source_text` honours
> `file.source_text_override(db)` (`source.rs:21` **[verified]**), which is the
> hook for that; but "inject into an unsaved buffer" means writing a file the
> user has not saved. Decide whether injection is skipped when an override is
> present. Exercise 02 object 6's third row — "parsing client-supplied unsaved
> content: your own" — is the read side of the same question.

---

## Exercise

**A.** Put `plan_ids` inside a `#[salsa::tracked]` function on purpose, with a
real UUID generator. Call it twice on the same unchanged file and print both
results. Then call `definitions` and see what the ID is.

**B.** Now do the same with the counter generator. Does the second call re-run?
Explain the difference in terms of `values_equal`.

**C.** Implement the four steps. Then comment out the sync and re-run. Write
down what you saw — the outline, and `cat` of the file — side by side.

**D.** Try to hold `definitions`' result across `File::sync_path`. Paste the
borrow-checker error into your notes; it is the clearest statement of the
stale-range rule you will get.

**E.** Then defeat it with a `.clone()` and print the cloned ranges against the
fresh ones for `shapes.py`. How far apart are they by the end of the file?

**F.** Inject into all ten fixtures with one sync, then with one sync per file.
Time both. (On ten files the difference is noise — the point is to see where
the `&mut` goes.)

**G.** Run the CLI over a directory **without** `--write` and confirm not one
byte changed. Then point it at a path outside the project with `--write` on and
confirm the guard refuses.

**H.** Feed the injector a file with a syntax error in the middle. What does
`plan_ids` produce, and what should the injector do with it?

---

## Exam

**1.** Give the two ways injection violates query purity.

**2.** A random UUID inside a tracked query does more than produce odd IDs.
What does it do to everything downstream, and which salsa mechanism is
responsible?

**3.** Salsa can cancel a query mid-flight. What is the mechanism, and why does
that rule out file writes specifically?

**4.** Why not just put the docstring offsets in `DefNode` and do it all in one
pass? Give the memory answer and the invalidation answer.

**5.** Write the four steps. Say what step 4 guarantees that no amount of care
in step 3 could.

**6.** What breaks if you skip `File::sync_path`, and what makes it hard to
notice?

**7.** `sync` and `sync_path` both exist. When would you pick each?

**8.** The stale-range rule is a compile error rather than a convention. Which
two signatures make it one, and what is the loophole?

**9.** Why batch the writes and sync once?

**10.** Name the two guards on a writing RPC, and the failure each prevents.

---

## Answers

**1.** It writes files (an effect outside the query), and it generates random
UUIDs (so the same input gives different outputs).

**2.** Salsa compares `old_value == new_value` to decide whether dependents can
be reused (`salsa-macros/src/tracked_fn.rs:103-115`). A random result is never
equal, so every re-read invalidates everything beneath it permanently — and the
same def can be given two different IDs on two reads, with the graph keeping
whichever came last.

**3.** Taking `&mut db` cancels in-flight queries on other threads by throwing
`Cancelled::PendingWrite` through `panic::resume_unwind`
(`salsa/src/cancelled.rs`, `storage.rs:160`). A pure query that is unwound loses
nothing and is re-run. A write that is unwound half-way leaves a truncated
source file, and no amount of structuring makes a write safe against being
interrupted at an arbitrary instruction — so it has to live outside the query
graph entirely.

**4.** Memory: those fields make every cached `DefNode` bigger, for every def in
the project, forever. Invalidation: they also join `old == new`, so a change
that leaves the outline identical — reflowing a docstring, re-indenting a body —
would still invalidate the outline and everything downstream. The fields that
cost the most to carry are the ones that change most often.

**5.** Parse (cached) → `plan_ids` → apply, write, `File::sync` → `definitions`.
Step 4 guarantees the outline sent to the graph is computed from the
**post-write** text. Care in step 3 could keep *you* from reusing a stale range;
step 4 removes the code path on which a stale range exists.

**6.** Every later query serves the pre-injection source, so the IDs stay
`None`, injection looks like a no-op, and the file on disk is correct. It is hard
to notice because nothing errors and the one place you would check — the file —
shows the right answer.

**7.** `sync` takes the `File` handle you already have and refreshes just that
file. `sync_path` takes a path and also invalidates the containing directory
listing, which is what you need when the file may be **new** — injecting a
`FolderID` into an `__init__.py` you just created (object 5).

**8.** `definitions` is `returns(ref)`, so its result borrows the db
immutably; `File::sync`/`sync_path` take `&mut dyn Db`. The borrows cannot
coexist, so holding an outline across the write is rejected. The loophole is
cloning the outline first — which compiles and whose ranges are stale. Clone
names if you must, never ranges.

**9.** Because `&mut db` cancels every in-flight query on every other thread.
One sync per file multiplies that by the number of files, so a first pass over a
project becomes a storm of cancelled and re-run work. One request, one write
phase, one sync.

**10.** `--write` off by default, which prevents a parity run over a corpus from
silently rewriting the corpus (goldens included); and a project-boundary check
inside the injector, which prevents a `parse_file` on a third-party file from
injecting UUIDs into an installed package. A third is worth having: refuse to
write a file that did not parse cleanly.

---

**Next:** [`05-putting-it-together.md`](05-putting-it-together.md) — the two
RPCs, `FileID`/`FolderID`, and what this diverges from.
